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
