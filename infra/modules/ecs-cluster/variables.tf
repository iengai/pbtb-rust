# 基础配置
variable "project" {
  description = "Name of the project"
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

# ECS 集群配置
variable "ecs_cluster_name" {
  description = "Name of the ECS cluster"
  type        = string
}

variable "enable_container_insights" {
  description = "Whether to enable Container Insights"
  type        = bool
  default     = true
}

# 网络配置
variable "vpc_id" {
  description = "ID of the VPC"
  type        = string
}

variable "private_subnet_ids" {
  description = "List of private subnet IDs"
  type        = list(string)
}

# 安全组配置
variable "security_group_ids" {
  description = "List of security group IDs for ECS instances"
  type        = list(string)
}

# IAM 配置
variable "ecs_instance_role_arn" {
  description = "ARN of the ECS instance role"
  type        = string
}

variable "ecs_instance_profile_name" {
  description = "Name of the ECS instance profile"
  type        = string
}

# 实例配置
variable "ecs_instance_type" {
  description = "EC2 instance type for ECS container instances"
  type        = string
}

variable "min_size" {
  description = "Minimum number of instances in ASG"
  type        = number
}

variable "max_size" {
  description = "Maximum number of instances in ASG"
  type        = number
}

variable "desired_capacity" {
  description = "Desired number of instances in ASG"
  type        = number
}

# AMI 配置
variable "ecs_ami_id" {
  description = "Custom AMI ID for ECS instances. If empty, uses latest ECS-optimized AMI"
  type        = string
  default     = ""
}

variable "key_name" {
  description = "EC2 Key Pair name for SSH access"
  type        = string
  default     = ""
}

# 标签
variable "common_tags" {
  description = "Common tags for all resources"
  type        = map(string)
  default     = {}
}