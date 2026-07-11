// terraform/modules/lambda/daily_pnl_snapshot/variables.tf
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
  description = "AWS region for the SDK clients + S3 endpoint"
}

variable "environment_variables" {
  type    = map(string)
  default = {}
}

variable "lambda_code_bucket" {
  type        = string
  description = "S3 bucket that stores the lambda zip"
}

variable "dynamodb_table_name" {
  type        = string
  description = "Bots table name"
}

variable "dynamodb_table_arn" {
  type        = string
  description = "Bots table ARN (bot rows + config-switch timeline rows)"
}

variable "config_bucket_name" {
  type        = string
  description = "Bot-configs bucket holding {user_id}/{bot_id}/api-keys.json"
}

variable "chart_bucket_name" {
  type        = string
  description = "Private bucket the per-bot return JSON is written to"
}

variable "chart_bucket_arn" {
  type        = string
  description = "ARN of the chart bucket (for the PutObject policy)"
}

variable "chart_key_prefix" {
  type    = string
  default = "charts"
}

variable "bybit_base_url" {
  type    = string
  default = "https://api.bybit.com"
}

variable "backfill_days" {
  type        = number
  default     = 90
  description = "Days a bot's FIRST run back-fills; routine runs are incremental (only new days)"
}

variable "subnet_ids" {
  type        = list(string)
  description = "Private subnets for NAT egress (fixed whitelisted EIP)"
}

variable "security_group_ids" {
  type        = list(string)
  description = "Security groups for the VPC-attached ENI"
}

variable "schedule_expression" {
  type        = string
  default     = "cron(0 1 * * ? *)"
  description = "EventBridge schedule; defaults to 01:00 UTC daily"
}
