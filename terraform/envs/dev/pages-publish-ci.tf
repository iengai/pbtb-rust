# ---------------------------------------------------------------------------
# GitHub Actions OIDC role for the pages-publish workflow.
#
# Read-only on just the chart bucket: the workflow pulls the per-bot JSON and
# deploys it to GitHub Pages. The bucket stays private (no public policy); this
# scoped role is the only way in. OIDC provider + github_oidc_arn live in
# telebot.tf.
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
  name = "pages-publish-read"
  role = aws_iam_role.gh_pages_publish.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "ListChartBucket"
        Effect   = "Allow"
        Action   = ["s3:ListBucket"]
        Resource = module.chart_bucket.bucket_arn
      },
      {
        Sid      = "ReadChartData"
        Effect   = "Allow"
        Action   = ["s3:GetObject"]
        Resource = "${module.chart_bucket.bucket_arn}/*"
      }
    ]
  })
}

output "pages_publish_gh_role_arn" {
  description = "Set as GitHub secret AWS_PAGES_PUBLISH_ROLE_ARN"
  value       = aws_iam_role.gh_pages_publish.arn
}
