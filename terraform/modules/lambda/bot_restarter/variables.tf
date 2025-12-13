// terraform/modules/lambda/bot_restarter/variables.tf
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