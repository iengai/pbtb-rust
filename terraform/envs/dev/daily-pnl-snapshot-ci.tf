# ---------------------------------------------------------------------------
# GitHub Actions OIDC role for the daily-pnl-snapshot-deploy workflow.
#
# Mirrors lambda-ci.tf: the workflow ships the collector bootstrap straight to
# the function with `aws lambda update-function-code` — no Terraform, no state
# lock, no NAT touch. So this role needs nothing beyond updating/invoking that
# one function. OIDC provider + github_oidc_arn local live in telebot.tf.
# ---------------------------------------------------------------------------

resource "aws_iam_role" "gh_daily_pnl_deploy" {
  name = "${var.project}-${var.env}-daily-pnl-snapshot-gh-deploy"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Federated = local.github_oidc_arn }
      Action    = "sts:AssumeRoleWithWebIdentity"
      Condition = {
        StringEquals = { "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com" }
        StringLike   = { "token.actions.githubusercontent.com:sub" = "repo:${var.github_repo}:ref:refs/heads/main" }
      }
    }]
  })

  tags = var.common_tags
}

resource "aws_iam_role_policy" "gh_daily_pnl_deploy" {
  name = "lambda-deploy"
  role = aws_iam_role.gh_daily_pnl_deploy.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid    = "UpdateAndInvokeFunction"
        Effect = "Allow"
        Action = [
          "lambda:UpdateFunctionCode",
          "lambda:GetFunctionConfiguration",
          "lambda:PublishVersion",
          "lambda:InvokeFunction"
        ]
        Resource = [
          module.lambda_daily_pnl_snapshot.function_arn,
          "${module.lambda_daily_pnl_snapshot.function_arn}:*"
        ]
      }
    ]
  })
}

output "daily_pnl_snapshot_gh_deploy_role_arn" {
  description = "Set as GitHub secret AWS_DAILY_PNL_DEPLOY_ROLE_ARN"
  value       = aws_iam_role.gh_daily_pnl_deploy.arn
}
