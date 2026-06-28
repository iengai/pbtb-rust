terraform {
  required_version = ">= 1.13.0"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 6.18.0"
    }
  }

  # 远程状态存储：S3 后端（桶手工创建，不纳入本配置管理）
  # 使用 S3 原生锁 (use_lockfile)，无需 DynamoDB 锁表。
  # 注意：backend 块不支持变量插值，所有值必须写死。
  backend "s3" {
    bucket       = "pbtb-rust-tfstate-025418542265"
    key          = "envs/dev/terraform.tfstate"
    region       = "ap-northeast-1"
    profile      = "dev"
    encrypt      = true
    use_lockfile = true
  }
}

provider "aws" {
  region = var.region
  # 如果你使用了 profile：
  profile = var.profile
}

module "network" {
  source               = "../../modules/network"
  project              = var.project
  env                  = var.env
  vpc_cidr_block       = var.vpc_cidr_block
  azs                  = var.azs
  public_subnet_cidrs  = var.public_subnet_cidrs
  private_subnet_cidrs = var.private_subnet_cidrs
  tags                 = var.common_tags
  nat_ami              = "ami-0ab96e70798e83256"

  # Upsize the NAT to t4g.micro and co-locate the telebot container on it.
  nat_instance_type        = var.nat_instance_type
  nat_iam_instance_profile = aws_iam_instance_profile.telebot.name
  nat_user_data            = local.telebot_user_data
}

module "ecs" {
  source = "../../modules/ecs"

  project            = var.project
  env                = var.env
  common_tags        = var.common_tags
  private_subnet_ids = module.network.private_subnet_ids
  ecs_sg_id          = module.network.app_sg_id
  ec2_instance_type  = var.ecs_instance_type
  min_capacity       = var.min_size
  max_capacity       = var.max_size
}

module "task_base" {
  source = "../../modules/task-definitions/base"

  project     = var.project
  env         = var.env
  region      = var.region
  common_tags = var.common_tags
}

# Container image registries. Both repos pre-exist in AWS and are adopted into
# state (telebot via `state mv`, passivbot-live via `import`) — never recreated.
# See terraform/envs/dev/RUNBOOK.md.
module "ecr" {
  source = "../../modules/ecr"
  tags   = var.common_tags

  repositories = {
    telebot = {
      name                 = local.telebot_name # scalable-cluster-dev-telebot
      image_tag_mutability = "MUTABLE"          # deploy re-points :latest
      scan_on_push         = true
      force_delete         = true
    }
    # Internal map key is left as `passivbot_v741` on purpose: it addresses the
    # imported live repo `passivbot-live` (force_delete=false). Renaming the key
    # would plan a destroy/recreate of that repo. The repo name is already
    # version-agnostic, so the key is just a stable internal handle.
    passivbot_v741 = {
      name                 = "passivbot-live" # pre-existing, non-conventional name
      image_tag_mutability = "MUTABLE"
      scan_on_push         = false # matches the live repo (clean import)
      force_delete         = false # live trading image — never auto-delete
    }
  }
}

module "passivbot_task" {
  source = "../../modules/task-definitions/passivbot"

  project            = var.project
  env                = var.env
  region             = var.region
  common_tags        = var.common_tags
  execution_role_arn = module.task_base.task_execution_role_arn
  task_role_arn      = module.task_base.task_role_arn
  container_image    = "${module.ecr.repository_urls["passivbot_v741"]}:${var.passivbot_image_tag}"
  log_retention_days = var.log_retention_days
  container_name     = var.passivbot_container_name

  s3_bucket_name = module.s3_bucket.bucket_name
}

# Preserve state across the module rename so the apply doesn't read as a
# destroy+create of the wrapper (the task def still gets a new revision because
# the family string changed).
moved {
  from = module.passivbot_v741_task
  to   = module.passivbot_task
}

module "s3_bucket" {
  source = "../../modules/s3"

  project     = var.project
  env         = var.env
  common_tags = var.common_tags
  bucket_name = var.s3_bucket_name

  ecs_task_role_arn = module.task_base.task_role_arn
}

module "dynamodb" {
  source = "../../modules/dynamodb"

  project     = var.project
  env         = var.env
  common_tags = var.common_tags
}

module "lambda_task_state_change_handler" {
  source = "../../modules/lambda/task_state_change_handler"

  project     = var.project
  env         = var.env
  common_tags = var.common_tags

  environment_variables = {
    ENV = var.env
    # The lambda zip ships only the bootstrap (no config files), so DynamoDB
    # config must come from env. Point at the real regional table.
    APP__DYNAMODB__REGION     = var.region
    APP__DYNAMODB__TABLE_NAME = module.dynamodb.bots_table_name
  }

  ecs_region       = var.region
  ecs_cluster_arn  = module.ecs.cluster_arn
  td_passivbot_arn = module.passivbot_task.task_definition_arn
  # Pass the container name explicitly so the lambda's RunTask override targets
  # the same container as the task def + telebot (all from one source of truth).
  passivbot_container_name    = var.passivbot_container_name
  lambda_code_bucket          = module.lambda_code_bucket.bucket_name
  ecs_task_execution_role_arn = module.task_base.task_execution_role_arn
  ecs_task_role_arn           = module.task_base.task_role_arn
  dynamodb_table_arn          = module.dynamodb.bots_table_arn
}

module "lambda_code_bucket" {
  source = "../../modules/lambda/s3"

  common_tags = var.common_tags

  bucket_name = "${var.project}-${var.env}-lambda-code"
}