# ---------------------------------------------------------------------------
# GitHub Actions OIDC role for the incident-diagnose workflow.
#
# The workflow lets a language model read the deployment to diagnose an
# Incident issue, and posts the result on a public repository. So this role
# is the ceiling on what such a run can ever see: describe / list / read-log
# calls, the bots table without the two key attributes a bot row carries,
# the config objects but never api-keys.json, and no SSM at all (SendCommand
# is a shell on the NAT host). Nothing here can start, stop, deploy or write.
# OIDC provider + github_oidc_arn local live in telebot.tf.
# ---------------------------------------------------------------------------

# Every ARN below is built by naming convention rather than read from a
# module output, so that `-target`ing this role never drags the lambda, ECR
# or bucket modules into the same apply (a plan for this role must stay at
# "2 to add").
locals {
  diagnose_account           = data.aws_caller_identity.current.account_id
  diagnose_config_bucket_arn = "arn:aws:s3:::${var.project}-${var.env}-bot-configs" # module.s3_bucket's name
  diagnose_lambda_arns       = ["arn:aws:lambda:${var.region}:${local.diagnose_account}:function:${var.project}-${var.env}-*"]
  diagnose_ecr_arns          = ["arn:aws:ecr:${var.region}:${local.diagnose_account}:repository/*"]
  diagnose_log_group_arns = [
    "arn:aws:logs:${var.region}:${local.diagnose_account}:log-group:/aws/lambda/${var.project}-${var.env}-*",
    "arn:aws:logs:${var.region}:${local.diagnose_account}:log-group:/ecs/${var.project}-${var.env}/*",
    "arn:aws:logs:${var.region}:${local.diagnose_account}:log-group:/aws/ecs/containerinsights/${var.project}-${var.env}-cluster/performance",
  ]
}

resource "aws_iam_role" "gh_diagnose" {
  name = "${var.project}-${var.env}-gh-diagnose"

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

resource "aws_iam_role_policy" "gh_diagnose" {
  name = "diagnose-read-only"
  role = aws_iam_role.gh_diagnose.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        # bot-status scans the table with a projection; the Attributes
        # condition makes the projection mandatory and pins its members, so a
        # scan that asks for api_key / secret_key is denied outright.
        Sid      = "BotRowsWithoutKeys"
        Effect   = "Allow"
        Action   = ["dynamodb:Scan", "dynamodb:Query", "dynamodb:GetItem"]
        Resource = [local.dynamodb_table_arn, "${local.dynamodb_table_arn}/index/*"]
        Condition = {
          "ForAllValues:StringEquals" = {
            "dynamodb:Attributes" = ["pk", "sk", "name", "enabled", "applied_at", "status", "task_id"]
          }
          StringEquals = { "dynamodb:Select" = "SPECIFIC_ATTRIBUTES" }
        }
      },
      {
        Sid      = "BotConfigsReadable"
        Effect   = "Allow"
        Action   = ["s3:GetObject"]
        Resource = "${local.diagnose_config_bucket_arn}/*"
      },
      {
        Sid      = "ApiKeysNever"
        Effect   = "Deny"
        Action   = ["s3:*"]
        Resource = "${local.diagnose_config_bucket_arn}/*/api-keys.json"
      },
      {
        Sid      = "ListConfigBucket"
        Effect   = "Allow"
        Action   = ["s3:ListBucket"]
        Resource = local.diagnose_config_bucket_arn
      },
      {
        Sid    = "EcsDescribe"
        Effect = "Allow"
        Action = [
          "ecs:ListTasks", "ecs:DescribeTasks", "ecs:DescribeClusters",
          "ecs:ListTaskDefinitionFamilies", "ecs:ListTaskDefinitions", "ecs:DescribeTaskDefinition"
        ]
        Resource = "*"
      },
      {
        Sid      = "LambdaConfig"
        Effect   = "Allow"
        Action   = ["lambda:GetFunctionConfiguration"]
        Resource = local.diagnose_lambda_arns
      },
      {
        Sid      = "ReadLogs"
        Effect   = "Allow"
        Action   = ["logs:FilterLogEvents", "logs:GetLogEvents", "logs:DescribeLogStreams"]
        Resource = concat(local.diagnose_log_group_arns, [for a in local.diagnose_log_group_arns : "${a}:*"])
      },
      {
        Sid      = "DescribeLogGroups"
        Effect   = "Allow"
        Action   = ["logs:DescribeLogGroups"]
        Resource = "*"
      },
      {
        Sid      = "EcrImages"
        Effect   = "Allow"
        Action   = ["ecr:DescribeImages"]
        Resource = local.diagnose_ecr_arns
      },
      {
        Sid      = "NatInstanceLookup"
        Effect   = "Allow"
        Action   = ["ec2:DescribeInstances"]
        Resource = "*"
      }
    ]
  })
}

output "gh_diagnose_role_arn" {
  description = "Set as GitHub secret AWS_DIAGNOSE_ROLE_ARN"
  value       = aws_iam_role.gh_diagnose.arn
}
