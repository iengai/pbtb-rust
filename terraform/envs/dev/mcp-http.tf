// terraform/envs/dev/mcp-http.tf
//
// The MCP surface behind a Lambda Function URL.
//
// A Function URL with authorization_type = NONE is open to the internet: AWS
// forwards every request and the bearer check inside the function is the only
// thing between a stranger and a live trading account. That is why the whole
// file is behind `mcp_http_enabled`, default false — merging it creates nothing,
// and standing the endpoint up is a deliberate, separate act.
//
// The token is a SecureString set out-of-band, the same way the telegram token
// is: Terraform holds the parameter, never its value.

locals {
  mcp_http_enabled = var.mcp_http_enabled ? 1 : 0
  mcp_http_name    = "${var.project}-${var.env}-mcp-http"
  mcp_token_param  = "/${var.project}/${var.env}/mcp/bearer-token"

  # This server's own public URL, which it needs to know as the audience tokens
  # must be minted for. It cannot arrive as an environment variable: the URL is a
  # resource that depends on the function, so building the function's env from it
  # is a cycle. Terraform writes it here and the function reads it at cold start.
  mcp_resource_param = "/${var.project}/${var.env}/mcp/resource-url"

  # With an issuer, each caller is a person and the shared bearer is not created
  # at all — a door that does not exist cannot be left unlocked.
  mcp_shared_bearer = var.mcp_http_enabled && var.mcp_issuer == "" ? 1 : 0
}

resource "aws_ssm_parameter" "mcp_bearer_token" {
  count = local.mcp_shared_bearer

  name        = local.mcp_token_param
  description = "Bearer token for the pbtb-rust MCP HTTP endpoint"
  type        = "SecureString"
  value       = "REPLACE_ME" # placeholder; set with: aws ssm put-parameter --overwrite ...

  lifecycle {
    ignore_changes = [value] # real value is managed out-of-band, not by Terraform
  }

  tags = var.common_tags
}

resource "aws_ssm_parameter" "mcp_resource_url" {
  count = local.mcp_http_enabled

  name        = local.mcp_resource_param
  description = "Public URL of the pbtb-rust MCP endpoint (the OAuth resource identifier)"
  type        = "String"
  value       = aws_lambda_function_url.mcp_http[0].function_url

  tags = var.common_tags
}

module "lambda_mcp_http" {
  count  = local.mcp_http_enabled
  source = "../../modules/lambda/base"

  project     = var.project
  env         = var.env
  common_tags = var.common_tags

  function_name  = "mcp-http"
  bootstrap_path = "${path.root}/../../../target/lambda/mcp_http/bootstrap"
  architecture   = "x86_64"
  code_s3_bucket = module.lambda_code_bucket.bucket_name

  # A tool call can wait on ECS RunTask and a DynamoDB CAS round trip; the client
  # gives up long before this, so the ceiling is only there to bound a hang.
  timeout_seconds = 30
  memory_mb       = 256

  environment_variables = {
    ENV = var.env

    APP__DYNAMODB__REGION     = var.region
    APP__DYNAMODB__TABLE_NAME = module.dynamodb.bots_table_name

    APP__S3__REGION       = var.region
    APP__S3__BUCKET_NAME  = module.s3_bucket.bucket_name
    APP__S3__ENDPOINT_URL = "https://s3.${var.region}.amazonaws.com"

    APP__ECS__REGION                      = var.region
    APP__ECS__CLUSTER_ARN                 = module.ecs.cluster_arn
    APP__ECS__TD_PASSIVBOT_BY_ENGINE      = local.td_passivbot_by_engine
    APP__ECS__TD_PASSIVBOT_CONTAINER_NAME = var.passivbot_container_name

    # The tenant every row is keyed under, and the allowlist it must appear on —
    # the same list telebot enforces, so removing someone from the bot takes
    # their MCP access with it.
    APP__MCP__USER_ID               = var.mcp_user_id
    APP__MCP__TOKEN_PARAM           = local.mcp_token_param
    APP__MCP__RESOURCE_URL_PARAM    = local.mcp_resource_param
    APP__MCP__ISSUER                = var.mcp_issuer
    APP__TELEGRAM__ALLOWED_USER_IDS = join(",", var.telegram_allowed_user_ids)
  }
}

# Open to the internet by design: MCP clients speak Bearer, not SigV4, so IAM
# auth here would lock out every client this endpoint exists for.
resource "aws_lambda_function_url" "mcp_http" {
  count = local.mcp_http_enabled

  function_name      = module.lambda_mcp_http[0].function_name
  authorization_type = "NONE"
}

# The tools drive the same use cases telebot does, so they need the same access:
# the bots table (including the CAS start lock), the config bucket, and RunTask.
resource "aws_iam_role_policy" "mcp_http_app" {
  count = local.mcp_http_enabled

  name = "${local.mcp_http_name}-app"
  role = module.lambda_mcp_http[0].role_name

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid    = "DynamoDB"
        Effect = "Allow"
        Action = [
          "dynamodb:GetItem", "dynamodb:PutItem", "dynamodb:UpdateItem",
          "dynamodb:DeleteItem", "dynamodb:Query"
        ]
        Resource = [local.dynamodb_table_arn, "${local.dynamodb_table_arn}/index/*"]
      },
      {
        Sid    = "S3Configs"
        Effect = "Allow"
        Action = [
          "s3:GetObject", "s3:GetObjectVersion", "s3:ListBucket",
          "s3:PutObject", "s3:DeleteObject"
        ]
        Resource = [local.s3_bucket_arn, "${local.s3_bucket_arn}/*"]
      },
      {
        Sid      = "EcsRunTasks"
        Effect   = "Allow"
        Action   = ["ecs:RunTask", "ecs:StopTask", "ecs:DescribeTasks", "ecs:ListTasks"]
        Resource = "*"
        Condition = {
          ArnEquals = { "ecs:cluster" = local.ecs_cluster_arn }
        }
      },
      {
        Sid      = "EcsDescribe"
        Effect   = "Allow"
        Action   = ["ecs:DescribeTaskDefinition", "ecs:DescribeClusters"]
        Resource = "*"
      },
      {
        Sid      = "PassTaskRoles"
        Effect   = "Allow"
        Action   = "iam:PassRole"
        Resource = [module.task_base.task_execution_role_arn, module.task_base.task_role_arn]
        Condition = {
          StringEquals = { "iam:PassedToService" = "ecs-tasks.amazonaws.com" }
        }
      },
      {
        Sid    = "ReadOwnParameters"
        Effect = "Allow"
        Action = ["ssm:GetParameter"]
        Resource = concat(
          [aws_ssm_parameter.mcp_resource_url[0].arn],
          aws_ssm_parameter.mcp_bearer_token[*].arn,
        )
      },
      {
        Sid      = "DecryptToken"
        Effect   = "Allow"
        Action   = ["kms:Decrypt"]
        Resource = "*"
        Condition = {
          StringEquals = { "kms:ViaService" = "ssm.${var.region}.amazonaws.com" }
        }
      }
    ]
  })
}

output "mcp_http_url" {
  description = "MCP endpoint (set the bearer token in SSM before handing this out)"
  value       = var.mcp_http_enabled ? aws_lambda_function_url.mcp_http[0].function_url : null
}

output "mcp_token_ssm_parameter" {
  description = "Set the real bearer token here (SecureString). Null once an issuer is configured — there is no shared bearer then."
  value       = one(aws_ssm_parameter.mcp_bearer_token[*].name)
}
