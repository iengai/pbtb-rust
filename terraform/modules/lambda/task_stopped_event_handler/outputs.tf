# terraform/modules/lambda/task_stopped_event_handler/outputs.tf
output "function_name" {
  description = "bot-restarter lambda function name"
  value       = module.base.function_name
}

output "function_arn" {
  description = "bot-restarter lambda function arn"
  value       = module.base.function_arn
}