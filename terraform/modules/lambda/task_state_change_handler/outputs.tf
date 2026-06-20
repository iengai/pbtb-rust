# terraform/modules/lambda/task_state_change_handler/outputs.tf
output "function_name" {
  description = "task-state-change-handler lambda function name"
  value       = module.base.function_name
}

output "function_arn" {
  description = "task-state-change-handler lambda function arn"
  value       = module.base.function_arn
}