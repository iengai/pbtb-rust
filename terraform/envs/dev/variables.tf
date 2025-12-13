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
  type        = string
}

variable "env" {
  type        = string
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

variable "common_tags"  {
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

variable "passivbot_v741_image" {
  description = "Docker image for the passivbot v7.4.1"
  type        = string
}

variable "passivbot_v741_container_name" {
  description = "Container name for passivbot v741"
  type        = string
  default     = "passivbot-v741-container"
}

variable "log_retention_days" {
  type        = number
}

variable "s3_bucket_name" {
  description = "S3 bucket name suffix"
  type        = string
  default     = "bot-configs"
}