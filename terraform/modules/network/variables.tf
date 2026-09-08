# 基础配置变量
variable "project" {
  description = "Name of the project"
  type        = string
}

variable "env" {
  description = "Environment name"
  type        = string
}

# 网络配置变量
variable "vpc_cidr_block" {
  description = "CIDR block for the VPC"
  type        = string
}

variable "public_subnet_cidrs" {
  description = "List of CIDR blocks for public subnets"
  type        = list(string)
}

variable "private_subnet_cidrs" {
  description = "List of CIDR blocks for private subnets"
  type        = list(string)
}

variable "azs" {
  description = "List of availability zones to use for subnets"
  type        = list(string)
}

# 标签变量
variable "tags" {
  description = "Common tags for all resources"
  type        = map(string)
  default     = {}
}

variable "region" {
  description = "AWS region, used to name the gateway endpoint services"
  type        = string
}

variable "nat_ami" {
  type = string
}

variable "nat_instance_type" {
  description = "EC2 instance type for the NAT instance"
  type        = string
  default     = "t4g.nano"
}

variable "nat_iam_instance_profile" {
  description = "IAM instance profile name to attach to the NAT instance (e.g. to also run the telebot container). Null = none."
  type        = string
  default     = null
}

variable "nat_user_data" {
  description = "Override user-data for the NAT instance. Null = default NAT-only setup script."
  type        = string
  default     = null
}

# ---- Temporary standby NAT (maintenance-window egress) ----
#
# The primary NAT is the sole egress for all trading traffic AND the telebot
# host, so any user_data/AMI change replaces it and blackholes egress for
# minutes. These three variables let a throwaway NAT carry egress across that
# window: bring it up, flip the route + EIP onto it, rebuild the primary, flip
# back, destroy it. See terraform/envs/dev/RUNBOOK.md.

variable "nat_standby_enabled" {
  description = "Create the temporary standby NAT instance. Keep false outside a maintenance window -- it exists only to carry egress while the primary NAT is rebuilt."
  type        = bool
  default     = false
}

variable "nat_standby_instance_type" {
  description = "EC2 instance type for the standby NAT. It only forwards packets (no telebot), so nano is enough."
  type        = string
  default     = "t4g.nano"
}

variable "nat_egress_active" {
  description = "Which NAT carries the private subnets' default route and the EIP: 'primary' or 'standby'."
  type        = string
  default     = "primary"

  validation {
    condition     = contains(["primary", "standby"], var.nat_egress_active)
    error_message = "nat_egress_active must be either 'primary' or 'standby'."
  }

  validation {
    condition     = var.nat_egress_active != "standby" || var.nat_standby_enabled
    error_message = "nat_egress_active = 'standby' requires nat_standby_enabled = true."
  }
}
