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

variable "ecs_ami" {
  description = "AMI for the ECS container instances, pinned to an exact id. Resolving \"most recent\" here instead would put a launch-template change in every plan the moment Amazon publishes a new image, and the ASG's Rolling instance_refresh would then recycle the host -- which on a single-instance ASG kills every live trading task. Bumping this is therefore a deliberate, scheduled act: find a candidate with `aws ec2 describe-images --owners amazon --filters \"Name=name,Values=al2023-ami-*-kernel-6.1-arm64\" --query 'reverse(sort_by(Images,&CreationDate))[:3].[ImageId,Name]'`, and expect the host to be replaced when you apply it."
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
