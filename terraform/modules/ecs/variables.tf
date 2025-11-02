variable "project" {
  description = "Project name"
  type        = string
}

variable "env" {
  description = "Environment name"
  type        = string
}

variable "common_tags" {
  description = "Common tags for all resources"
  type        = map(string)
  default     = {}
}

variable "private_subnet_ids" {
  description = "List of private subnet IDs for ECS instances"
  type        = list(string)
}

variable "ecs_sg_id" {
  description = "Security group ID for ECS instances"
  type        = string
}

variable "ec2_instance_type" {
  description = "EC2 instance type for ECS container instances"
  type        = string
  default     = "t4g.micro"
}

variable "min_capacity" {
  description = "Minimum number of ECS instances"
  type        = number
  default     = 1
}

variable "max_capacity" {
  description = "Maximum number of ECS instances"
  type        = number
  default     = 10
}

variable "enable_spot_draining" {
  description = "Enable Spot Instance draining"
  type        = bool
  default     = false
}

variable "target_capacity" {
  description = "Size of the EBS volume in GB"
  type        = number
  default     = 100
}
