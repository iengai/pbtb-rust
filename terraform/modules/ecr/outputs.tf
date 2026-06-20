output "repository_urls" {
  description = "Map of logical name => repository URL (registry/name, no tag)."
  value       = { for k, r in aws_ecr_repository.this : k => r.repository_url }
}

output "repository_arns" {
  description = "Map of logical name => repository ARN."
  value       = { for k, r in aws_ecr_repository.this : k => r.arn }
}

output "repository_names" {
  description = "Map of logical name => repository name."
  value       = { for k, r in aws_ecr_repository.this : k => r.name }
}
