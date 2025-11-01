output "network" {
  description = "All outputs from the network module"
  value       = module.network
}

output "security_groups" {
  description = "All outputs from the security groups module"
  value       = module.security_groups
}
