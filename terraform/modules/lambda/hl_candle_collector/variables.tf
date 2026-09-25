// terraform/modules/lambda/hl_candle_collector/variables.tf
variable "project" {
  type = string
}

variable "env" {
  type = string
}

variable "common_tags" {
  type    = map(string)
  default = {}
}

variable "region" {
  type        = string
  description = "AWS region for the S3 client"
}

variable "environment_variables" {
  type    = map(string)
  default = {}
}

variable "lambda_code_bucket" {
  type        = string
  description = "S3 bucket that stores the lambda zip"
}

variable "config_bucket_name" {
  type        = string
  description = "Bot-configs bucket; only its predefined/ templates are read"
}

variable "candle_bucket_name" {
  type        = string
  description = "Private bucket the day objects are written to"
}

variable "candle_bucket_arn" {
  type        = string
  description = "ARN of the candle bucket"
}

variable "candle_key_prefix" {
  type    = string
  default = "hyperliquid/1m"
}

variable "schedule_expression" {
  type        = string
  default     = "cron(15 0 * * ? *)"
  description = "EventBridge schedule; 00:15 UTC reaches the three whole days before today"
}
