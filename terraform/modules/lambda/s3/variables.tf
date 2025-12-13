variable "common_tags" {
  type        = map(string)
  default     = {}
  description = "Common tags"
}

variable "bucket_name" {
  type        = string
  description = "S3 bucket name for lambda code artifacts (must be globally unique)"
}
