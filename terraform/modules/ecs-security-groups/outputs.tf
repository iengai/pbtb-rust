output "ecs_instance_sg_id" {
  description = "Security group ID for ECS instances"
  value       = aws_security_group.ecs_instance.id
}

output "ecs_service_sg_id" {
  description = "Security group ID for ECS services"
  value       = aws_security_group.ecs_service.id
}