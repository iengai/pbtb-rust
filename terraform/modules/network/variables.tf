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
