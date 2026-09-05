variable "project" {
  description = "Project name"
  type        = string
}

variable "env" {
  description = "Environment name"
  type        = string
}

variable "region" {
  description = "AWS region"
  type        = string
}

variable "common_tags" {
  description = "Common tags for all resources"
  type        = map(string)
  default     = {}
}

variable "execution_role_arn" {
  description = "ECS task execution role ARN"
  type        = string
}

variable "task_role_arn" {
  description = "ECS task role ARN"
  type        = string
}

variable "container_name" {
  description = "Container name"
  type        = string
  default     = "passivbot-container"
}

variable "container_image" {
  description = "Container image URI"
  type        = string
  default     = "your-registry/passivbot:v7.12.0-arm64"
}

variable "log_retention_days" {
  description = "CloudWatch log retention in days"
  type        = number
  default     = 30
}

variable "port_mappings" {
  description = "Container port mappings"
  type = list(object({
    containerPort = number
    hostPort      = number
    protocol      = string
  }))
  default = [{
    containerPort = 8080
    hostPort      = 0
    protocol      = "tcp"
  }]
}

variable "s3_bucket_name" {
  description = "S3 bucket name for file downloads"
  type        = string
}

variable "family_suffix" {
  description = "Appended to the task-definition family (and log group) so each passivbot engine line has its own family, e.g. \"-v8\". The line that inherited the original family keeps \"\"."
  type        = string
  default     = ""
}

variable "memory" {
  description = "Task-level hard memory limit (MiB) for this engine line. Sized per engine: a newer passivbot can have a different RSS profile, so it must not inherit another line's number blindly."
  type        = number
  default     = 400
}
