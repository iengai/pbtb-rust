# ---------------------------------------------------------------------------
# Telebot (pbtb-rust control bot) co-located on the NAT instance.
#
# NOTE: ideally the control bot should NOT share the NAT host — the NAT is the
# single egress point for all trading traffic. This co-location is a deliberate
# cost-driven choice for the dev env. Guardrails applied in user-data: hard
# container memory cap, capped json-file logs, restart-on-failure, host swap.
#
# Cycle avoidance: anything feeding `module.network` (which `module.ecs` depends
# on) must not transitively depend on the network module. The ECS cluster ARN is
# therefore built from a known naming convention instead of `module.ecs`.
# ---------------------------------------------------------------------------

data "aws_caller_identity" "current" {}

locals {
  telebot_name        = "${var.project}-${var.env}-telebot"
  ecr_registry        = "${data.aws_caller_identity.current.account_id}.dkr.ecr.${var.region}.amazonaws.com"
  telebot_image       = "${aws_ecr_repository.telebot.repository_url}:${var.telebot_image_tag}"
  telebot_token_param = "/${var.project}/${var.env}/telebot/teloxide-token"

  # Built by convention (matches aws_ecs_cluster.main.name = "$${project}-$${env}-cluster")
  # to avoid a network -> ecs -> network dependency cycle.
  ecs_cluster_arn = "arn:aws:ecs:${var.region}:${data.aws_caller_identity.current.account_id}:cluster/${var.project}-${var.env}-cluster"

  # DynamoDB table created by module.dynamodb is "$${project}-$${env}-bots".
  dynamodb_table_name = "${var.project}-${var.env}-bots"
  dynamodb_table_arn  = "arn:aws:dynamodb:${var.region}:${data.aws_caller_identity.current.account_id}:table/${local.dynamodb_table_name}"

  s3_bucket_arn = "arn:aws:s3:::${module.s3_bucket.bucket_name}"

  # user_data carries ZERO app-level config — only the NAT/host bootstrap. All
  # telebot runtime config is delivered out-of-band: terraform publishes the
  # stable config to the base-env SSM parameter, and telebot-deploy composes
  # /etc/telebot/telebot.env on the host (base-env + the resolved passivbot
  # task-def ARN) and restarts. So passivbot/image churn never reaches the NAT
  # instance lifecycle.
  telebot_user_data = templatefile("${path.module}/telebot-userdata.sh.tftpl", {
    nat_setup_script = file("${path.module}/../../modules/network/nat-userdata-al2023.sh")
  })

  # Stable telebot runtime config (everything except the SecureString token, which
  # is fetched from SSM at container start, and the passivbot task-def ARN, which
  # telebot-deploy resolves live so a passivbot bump never re-renders this).
  telebot_base_env = join("\n", [
    "RUST_LOG=info",
    "APP__DYNAMODB__REGION=${var.region}",
    "APP__DYNAMODB__TABLE_NAME=${local.dynamodb_table_name}",
    "APP__S3__REGION=${var.region}",
    "APP__S3__BUCKET_NAME=${module.s3_bucket.bucket_name}",
    "APP__S3__ENDPOINT_URL=https://s3.${var.region}.amazonaws.com",
    "APP__ECS__REGION=${var.region}",
    "APP__ECS__CLUSTER_ARN=${local.ecs_cluster_arn}",
    "APP__ECS__TD_PASSIVBOT_V741_CONTAINER_NAME=${var.passivbot_v741_container_name}",
    "TELEBOT_REGION=${var.region}",
    "TELEBOT_ECR_REGISTRY=${local.ecr_registry}",
    "TELEBOT_IMAGE=${local.telebot_image}",
    "TELEBOT_TOKEN_PARAM=${local.telebot_token_param}",
    "TELEBOT_MEMORY=${var.telebot_memory}",
  ])
}

# --- ECR repository for the bot image ---
resource "aws_ecr_repository" "telebot" {
  name                 = local.telebot_name
  image_tag_mutability = "MUTABLE"
  force_delete         = true

  image_scanning_configuration {
    scan_on_push = true
  }

  tags = var.common_tags
}

# --- Telegram bot token (set the real value out-of-band; never in TF state) ---
resource "aws_ssm_parameter" "telebot_token" {
  name        = local.telebot_token_param
  description = "TELOXIDE_TOKEN for the pbtb-rust telebot"
  type        = "SecureString"
  value       = "REPLACE_ME" # placeholder; set with: aws ssm put-parameter --overwrite ...

  lifecycle {
    ignore_changes = [value] # real value is managed out-of-band, not by Terraform
  }

  tags = var.common_tags
}

# --- Stable telebot config (source of truth read by telebot-deploy) ---
# Non-secret; updated on every apply so infra renames propagate without rebuilding
# the NAT. telebot-deploy reads this, appends the resolved passivbot task-def ARN,
# and writes /etc/telebot/telebot.env on the NAT host.
resource "aws_ssm_parameter" "telebot_base_env" {
  name  = "/${var.project}/${var.env}/telebot/base-env"
  type  = "String"
  value = local.telebot_base_env

  tags = var.common_tags
}

# --- IAM role / instance profile for the NAT host (so the bot can reach AWS) ---
resource "aws_iam_role" "telebot" {
  name = "${local.telebot_name}-nat-role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Action    = "sts:AssumeRole"
      Effect    = "Allow"
      Principal = { Service = "ec2.amazonaws.com" }
    }]
  })

  tags = var.common_tags
}

# SSM core (makes the host SSM-manageable) + ECR pull
resource "aws_iam_role_policy_attachment" "telebot_ssm_core" {
  role       = aws_iam_role.telebot.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"
}

resource "aws_iam_role_policy_attachment" "telebot_ecr_read" {
  role       = aws_iam_role.telebot.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryReadOnly"
}

# App permissions: DynamoDB + S3 + ECS(RunTask) + PassRole + read the token param
resource "aws_iam_role_policy" "telebot_app" {
  name = "${local.telebot_name}-app"
  role = aws_iam_role.telebot.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid    = "DynamoDB"
        Effect = "Allow"
        Action = [
          "dynamodb:GetItem", "dynamodb:PutItem", "dynamodb:UpdateItem",
          "dynamodb:DeleteItem", "dynamodb:Query", "dynamodb:Scan",
          "dynamodb:BatchGetItem", "dynamodb:BatchWriteItem"
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
        Sid      = "ReadToken"
        Effect   = "Allow"
        Action   = ["ssm:GetParameter"]
        Resource = aws_ssm_parameter.telebot_token.arn
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

resource "aws_iam_instance_profile" "telebot" {
  name = "${local.telebot_name}-nat-profile"
  role = aws_iam_role.telebot.name

  tags = var.common_tags
}

output "telebot_ecr_repository_url" {
  description = "Push the bot image here (linux/arm64)"
  value       = aws_ecr_repository.telebot.repository_url
}

output "telebot_token_ssm_parameter" {
  description = "Set the real Telegram token here (SecureString)"
  value       = aws_ssm_parameter.telebot_token.name
}

# ---------------------------------------------------------------------------
# GitHub Actions OIDC: build role (push to ECR) + deploy role (re-tag + SSM).
# Least privilege via the OIDC `sub` claim:
#   - build  role: only jobs on refs/heads/main
#   - deploy role: only jobs on refs/heads/main (deploy is a manual workflow_dispatch)
# ---------------------------------------------------------------------------

resource "aws_iam_openid_connect_provider" "github" {
  count          = var.github_oidc_provider_arn == "" ? 1 : 0
  url            = "https://token.actions.githubusercontent.com"
  client_id_list = ["sts.amazonaws.com"]
  # AWS no longer validates this for the GitHub provider, but the field is required.
  thumbprint_list = [
    "6938fd4d98bab03faadb97b34396831e3780aea1",
    "1c58a3a8518e8759bf075b76b750d4f2df264fcd",
  ]
  tags = var.common_tags
}

locals {
  github_oidc_arn = var.github_oidc_provider_arn != "" ? var.github_oidc_provider_arn : one(aws_iam_openid_connect_provider.github[*].arn)
}

# --- Build role: push images to the telebot ECR repo ---
resource "aws_iam_role" "gh_build" {
  name = "${local.telebot_name}-gh-build"

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

resource "aws_iam_role_policy" "gh_build" {
  name = "ecr-push"
  role = aws_iam_role.gh_build.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "EcrAuth"
        Effect   = "Allow"
        Action   = "ecr:GetAuthorizationToken"
        Resource = "*"
      },
      {
        Sid    = "EcrPush"
        Effect = "Allow"
        Action = [
          "ecr:BatchCheckLayerAvailability",
          "ecr:InitiateLayerUpload",
          "ecr:UploadLayerPart",
          "ecr:CompleteLayerUpload",
          "ecr:PutImage",
          "ecr:BatchGetImage",
          "ecr:GetDownloadUrlForLayer",
          "ecr:DescribeImages"
        ]
        Resource = aws_ecr_repository.telebot.arn
      }
    ]
  })
}

# --- Deploy role: re-tag :latest in ECR + restart telebot on the NAT via SSM ---
resource "aws_iam_role" "gh_deploy" {
  name = "${local.telebot_name}-gh-deploy"

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

resource "aws_iam_role_policy" "gh_deploy" {
  name = "deploy"
  role = aws_iam_role.gh_deploy.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "EcrRetag"
        Effect   = "Allow"
        Action   = ["ecr:BatchGetImage", "ecr:PutImage", "ecr:DescribeImages"]
        Resource = aws_ecr_repository.telebot.arn
      },
      {
        Sid      = "FindNat"
        Effect   = "Allow"
        Action   = "ec2:DescribeInstances"
        Resource = "*"
      },
      {
        Sid      = "SsmSendToNat"
        Effect   = "Allow"
        Action   = "ssm:SendCommand"
        Resource = "arn:aws:ec2:${var.region}:${data.aws_caller_identity.current.account_id}:instance/*"
        Condition = {
          StringEquals = { "ssm:resourceTag/Name" = "nat-instance" }
        }
      },
      {
        Sid      = "SsmSendDocument"
        Effect   = "Allow"
        Action   = "ssm:SendCommand"
        Resource = "arn:aws:ssm:${var.region}::document/AWS-RunShellScript"
      },
      {
        Sid      = "SsmReadResult"
        Effect   = "Allow"
        Action   = ["ssm:GetCommandInvocation", "ssm:ListCommandInvocations"]
        Resource = "*"
      },
      {
        Sid      = "ReadBaseEnv"
        Effect   = "Allow"
        Action   = ["ssm:GetParameter"]
        Resource = aws_ssm_parameter.telebot_base_env.arn
      },
      {
        Sid      = "ResolvePassivbotTaskDef"
        Effect   = "Allow"
        Action   = ["ecs:DescribeTaskDefinition"]
        Resource = "*" # DescribeTaskDefinition does not support resource-level scoping
      }
    ]
  })
}

output "telebot_gh_build_role_arn" {
  description = "Set as GitHub secret AWS_BUILD_ROLE_ARN"
  value       = aws_iam_role.gh_build.arn
}

output "telebot_gh_deploy_role_arn" {
  description = "Set as GitHub secret AWS_DEPLOY_ROLE_ARN"
  value       = aws_iam_role.gh_deploy.arn
}
