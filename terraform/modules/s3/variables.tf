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

variable "bucket_name" {
  description = "S3 bucket name (will be prefixed with project and env)"
  type        = string
  default     = "files"
}

variable "ecs_task_role_arn" {
  description = "ECS task role ARN for S3 access"
  type        = string
}