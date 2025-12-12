variable "project" {
  type = string
}

variable "env" {
  type = string
}

variable "common_tags" {
  type        = map(string)
  description = "Common tags to apply to all resources"
  default     = {}
}
