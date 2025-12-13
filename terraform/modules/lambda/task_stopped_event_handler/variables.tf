// terraform/modules/lambda/task_stopped_event_handler/variables.tf
variable "project" {
  type        = string
  description = "Project name"
}

variable "env" {
  type        = string
  description = "Environment name"
}

variable "common_tags" {
  type        = map(string)
  default     = {}
  description = "Common tags"
}

variable "environment_variables" {
  type    = map(string)
  default = {}
}

variable "ecs_cluster_arn" {
  type        = string
  description = "ECS cluster ARN to filter ECS Task State Change events"
}

variable "ecs_region" {
  type        = string
  description = "ECS region for AWS SDK client"
}

variable "td_passivbot_v741_arn" {
  type        = string
  description = "Task definition ARN for passivbot-v741"
}

variable "passivbot_v741_container_name" {
  description = "Container name for passivbot v741"
  type        = string
  default     = "passivbot-v741-container"
}

variable "lambda_code_bucket" {
  type        = string
  description = "S3 bucket to store lambda zip for deployment"
}

variable "ecs_task_execution_role_arn" {
  type        = string
  description = "ECS task execution role ARN referenced by the task definition (executionRoleArn)"
}

variable "ecs_task_role_arn" {
  type        = string
  description = "ECS task role ARN referenced by the task definition (taskRoleArn)"
}
