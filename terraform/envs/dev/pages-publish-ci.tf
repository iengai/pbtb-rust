# ---------------------------------------------------------------------------
# GitHub Actions OIDC role for the pages-publish workflow.
#
# Read-only on the chart bucket's `public/` prefix alone: the workflow copies
# the showcase artifacts into the site before it builds. The bucket stays
# private (no public policy); this scoped role is the only way in, and it
# cannot see a tenant's `charts/` or the collector's `_state/`. OIDC provider
# + github_oidc_arn live in telebot.tf.
# ---------------------------------------------------------------------------

resource "aws_iam_role" "gh_pages_publish" {
  name = "${var.project}-${var.env}-pages-publish-gh"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Federated = local.github_oidc_arn }
      Action    = "sts:AssumeRoleWithWebIdentity"
      Condition = {
        StringEquals = { "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com" }
        # The publish job runs in the `github-pages` environment, so its OIDC
        # token subject is scoped to that environment, not a branch ref.
        StringLike = { "token.actions.githubusercontent.com:sub" = "repo:${var.github_repo}:environment:github-pages" }
      }
    }]
  })

  tags = var.common_tags
}

resource "aws_iam_role_policy" "gh_pages_publish" {
  name = "pages-publish-read-public"
  role = aws_iam_role.gh_pages_publish.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "ListPublicPrefix"
        Effect   = "Allow"
        Action   = ["s3:ListBucket"]
        Resource = module.chart_bucket.bucket_arn
        Condition = {
          StringLike = { "s3:prefix" = ["public/*"] }
        }
      },
      {
        Sid      = "ReadPublicPrefix"
        Effect   = "Allow"
        Action   = ["s3:GetObject"]
        Resource = "${module.chart_bucket.bucket_arn}/public/*"
      }
    ]
  })
}

output "pages_publish_gh_role_arn" {
  description = "Set as GitHub secret AWS_PAGES_PUBLISH_ROLE_ARN"
  value       = aws_iam_role.gh_pages_publish.arn
}
