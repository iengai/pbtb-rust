# 直接输出模块对象，减少重复代码
output "network" {
  description = "All outputs from the network module"
  value       = module.network
}

output "security_groups" {
  description = "All outputs from the security groups module"
  value       = module.security_groups
}
#
# output "iam" {
#   description = "All outputs from the IAM module"
#   value       = module.iam
# }
#
# output "ecs_cluster" {
#   description = "All outputs from the ECS cluster module"
#   value       = module.ecs_cluster
# }
