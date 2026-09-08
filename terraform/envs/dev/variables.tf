variable "region" {
  type        = string
  description = "AWS region"
  default     = "ap-northeast-1"
}


variable "profile" {
  type        = string
  description = "AWS CLI profile"
}

variable "project" {
  type = string
}

variable "env" {
  type = string
}


variable "vpc_cidr_block" {
  type        = string
  description = "VPC CIDR Block"
}


variable "azs" {
  type        = list(string)
  description = "availability zones"
}


variable "public_subnet_cidrs" {
  type        = list(string)
  description = "public subnet CIDR list"
}


variable "private_subnet_cidrs" {
  type        = list(string)
  description = "private subnet CIDR list"
}

variable "common_tags" {
  description = "Common tags to apply to all resources"
  type        = map(string)
  default     = {}
}

variable "ecs_cluster_name" {
  description = "Name of the ECS cluster"
  type        = string
}

variable "ecs_instance_type" {
  description = "EC2 instance type for ECS container instances"
  type        = string
}

variable "min_size" {
  description = "Minimum number of EC2 instances in the auto scaling group"
  type        = number
}

variable "max_size" {
  description = "Maximum number of EC2 instances in the auto scaling group"
  type        = number
}

variable "passivbot_engines" {
  description = "One task definition per ENGINE LINE (and runtime) a bot may launch on, keyed by the config's config_version major plus an optional runtime suffix: \"7\", \"8\" are the Python passivbot image of that line, \"8rs\" the pb-runner (Rust) image of line 8. A bot launches on the entry matching its config's line and its own runtime attribute (py default / rs), so a strategy validated on v7 keeps running the v7 image after v8 is rolled out, and only bots explicitly moved to rs use pb-runner. image_tag: tag in the image_repo ECR repo. image_repo: module.ecr key of the repo the image lives in (default the passivbot-live repo; \"pb_runner\" for rs entries). command: container command override (pb-runner takes \"--live\" to trade; without it it only plans and logs); null leaves the image's own entrypoint. memory: the task's hard limit in MiB, sized per entry. family_suffix: defaults to \"-v<key>\"; the line that inherited the original, unsuffixed family sets \"\" so its running tasks and log group are untouched."
  type = map(object({
    image_tag     = string
    memory        = optional(number, 400)
    family_suffix = optional(string)
    image_repo    = optional(string, "passivbot_v741")
    command       = optional(list(string))
  }))

  # The keys are parsed verbatim by the telebot and the lambda
  # (`EngineTaskDefinitions::parse`); a key outside `<major>[rs]` would pass
  # `terraform validate` and only fail at their config load, taking the
  # auto-restart lambda down. Reject it at plan time instead.
  validation {
    condition     = alltrue([for k in keys(var.passivbot_engines) : can(regex("^[0-9]+(rs)?$", k))])
    error_message = "passivbot_engines keys must be <major>[rs] (e.g. \"7\", \"8\", \"8rs\"); the telebot and lambda parse the table with exactly that syntax."
  }

  validation {
    condition     = alltrue([for e in values(var.passivbot_engines) : contains(["passivbot_v741", "pb_runner"], e.image_repo)])
    error_message = "passivbot_engines[*].image_repo must be a module.ecr key: \"passivbot_v741\" or \"pb_runner\"."
  }
}

variable "passivbot_container_name" {
  description = "Container name for the passivbot task (must match the RunTask container override used by telebot + lambda)"
  type        = string
  default     = "passivbot-container"
}

variable "log_retention_days" {
  type = number
}

variable "s3_bucket_name" {
  description = "S3 bucket name suffix"
  type        = string
  default     = "bot-configs"
}

variable "nat_instance_type" {
  description = "EC2 instance type for the NAT instance (also hosts the telebot container)"
  type        = string
  default     = "t4g.micro"
}

variable "telebot_image_tag" {
  description = "Image tag of the telebot image to provision on the NAT instance"
  type        = string
  default     = "latest"
}

variable "telebot_memory" {
  description = "Hard memory limit for the telebot container (docker --memory / --memory-swap)"
  type        = string
  default     = "256m"
}

variable "github_repo" {
  description = "GitHub repo (owner/name) allowed to assume the CI roles via OIDC"
  type        = string
  default     = "iengai/pbtb-rust"
}

variable "github_oidc_provider_arn" {
  description = "Existing GitHub OIDC provider ARN. Empty string = create the provider here."
  type        = string
  default     = ""
}
# ---- Temporary standby NAT (maintenance-window egress) ----
# Set both in terraform.tfvars, never with -var: every apply during the window
# must agree on them, or a scoped apply silently routes egress back onto the NAT
# it is busy destroying. Procedure: RUNBOOK.md, "Rebuilding the NAT without an
# egress outage".

variable "nat_standby_enabled" {
  description = "Create the temporary standby NAT instance. false outside a maintenance window."
  type        = bool
  default     = false
}

variable "nat_standby_instance_type" {
  description = "EC2 instance type for the standby NAT (packet forwarding only)"
  type        = string
  default     = "t4g.nano"
}

variable "nat_egress_active" {
  description = "Which NAT carries the private default route and the EIP: 'primary' or 'standby'"
  type        = string
  default     = "primary"
}
