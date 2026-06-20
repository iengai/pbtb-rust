# ---------------------------------------------------------------------------
# GitHub Actions OIDC role for the lambda-deploy workflow.
#
# The workflow ships the task_state_change_handler bootstrap straight to the
# function with `aws lambda update-function-code` — no Terraform, no S3 backend
# lock, no NAT touch. So this role needs nothing beyond updating/invoking that
# one function. OIDC provider + github_oidc_arn local live in telebot.tf.
# ---------------------------------------------------------------------------

resource "aws_iam_role" "gh_lambda_deploy" {
  name = "${var.project}-${var.env}-task-state-change-handler-gh-deploy"

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

resource "aws_iam_role_policy" "gh_lambda_deploy" {
  name = "lambda-deploy"
  role = aws_iam_role.gh_lambda_deploy.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid    = "UpdateAndInvokeFunction"
        Effect = "Allow"
        # Exactly the calls lambda-deploy.yml makes: UpdateFunctionCode (+ --publish
        # -> PublishVersion), `wait function-updated` polls GetFunctionConfiguration,
        # and the smoke invoke.
        Action = [
          "lambda:UpdateFunctionCode",
          "lambda:GetFunctionConfiguration",
          "lambda:PublishVersion",
          "lambda:InvokeFunction"
        ]
        # Bare ARN covers code update / config reads; ":*" covers published
        # versions (PublishVersion result, version-qualified invoke).
        Resource = [
          module.lambda_task_state_change_handler.function_arn,
          "${module.lambda_task_state_change_handler.function_arn}:*"
        ]
      }
    ]
  })
}

output "lambda_task_state_change_gh_deploy_role_arn" {
  description = "Set as GitHub secret AWS_LAMBDA_DEPLOY_ROLE_ARN"
  value       = aws_iam_role.gh_lambda_deploy.arn
}
