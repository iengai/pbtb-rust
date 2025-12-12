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