variable "project" {
  description = "Name of the project"
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