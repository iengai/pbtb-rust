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

# Expiry, only for repos that asked for it (see `keep_last_images` /
# `expire_untagged_after_days` in variables.tf). ECR applies rules in
# priority order and requires the `any` rule to come last.
resource "aws_ecr_lifecycle_policy" "this" {
  for_each = {
    for k, v in var.repositories : k => v
    if v.keep_last_images != null || v.expire_untagged_after_days != null
  }

  repository = aws_ecr_repository.this[each.key].name

  policy = jsonencode({
    rules = concat(
      each.value.expire_untagged_after_days == null ? [] : [{
        rulePriority = 1
        description  = "Expire untagged images after ${each.value.expire_untagged_after_days} days"
        selection = {
          tagStatus   = "untagged"
          countType   = "sinceImagePushed"
          countUnit   = "days"
          countNumber = each.value.expire_untagged_after_days
        }
        action = { type = "expire" }
      }],
      each.value.keep_last_images == null ? [] : [{
        rulePriority = 2
        description  = "Keep the newest ${each.value.keep_last_images} images"
        selection = {
          tagStatus   = "any"
          countType   = "imageCountMoreThan"
          countNumber = each.value.keep_last_images
        }
        action = { type = "expire" }
      }],
    )
  })
}
