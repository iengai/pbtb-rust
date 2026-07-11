variable "common_tags" {
  type    = map(string)
  default = {}
}

variable "bucket_name" {
  type        = string
  description = "S3 bucket name for the return-curve JSON (must be globally unique)"
}
