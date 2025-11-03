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
  default     = "passivbot-v741-container"
}

variable "container_image" {
  description = "Container image URI"
  type        = string
  default     = "your-registry/passivbot:v7.4.1"
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