# ECR repositories for the project's container images (telebot, passivbot, ...).
# Encryption is left at the AWS default (AES256); add an encryption_configuration
# block only if a repo needs KMS, to avoid drift on imported repos.
resource "aws_ecr_repository" "this" {
  for_each = var.repositories

  name                 = each.value.name
  image_tag_mutability = each.value.image_tag_mutability
  force_delete         = each.value.force_delete

  image_scanning_configuration {
    scan_on_push = each.value.scan_on_push
  }

  tags = var.tags
}
