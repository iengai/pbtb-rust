// terraform/modules/lambda/base/variables.tf
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

variable "function_name" {
  type        = string
  description = "Short function name suffix, e.g. task-state-change-handler"
}

variable "bootstrap_path" {
  type        = string
  description = "Path to the built bootstrap binary (must exist on the machine running terraform)"
}

variable "architecture" {
  type        = string
  description = "Lambda architecture: arm64 or x86_64"
  default     = "arm64"
}

variable "timeout_seconds" {
  type    = number
  default = 10
}

variable "memory_mb" {
  type    = number
  default = 128
}

variable "environment_variables" {
  type    = map(string)
  default = {}
}

variable "code_s3_bucket" {
  type        = string
  default     = ""
  description = "S3 bucket for lambda code (required if use_s3_code=true)"
}

variable "code_s3_key_prefix" {
  type        = string
  default     = "lambda"
  description = "S3 key prefix for lambda code objects"
}

variable "subnet_ids" {
  type        = list(string)
  default     = []
  description = "Private subnet IDs to attach the function to. Empty (default) leaves the function outside the VPC, so existing lambdas on this base are unaffected."
}

variable "security_group_ids" {
  type        = list(string)
  default     = []
  description = "Security group IDs for the VPC-attached ENI (used only when subnet_ids is non-empty)."
}
