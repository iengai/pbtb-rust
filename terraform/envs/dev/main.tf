terraform {
  required_version = ">= 1.13.0"
  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 6.18.0"
    }
  }

  # 可选但推荐：使用S3后端远程存储状态文件，便于团队协作
  # 在首次初始化前，你需要先手动创建这个S3存储桶和DynamoDB表。
  # backend "s3" {
  #   bucket = "pbtb-rust-dev-tfstate" # 请替换为全局唯一的桶名
  #   key    = "/network/terraform.tfstate"
  #   region = "ap-northeast-1"
  #   dynamodb_table = "${var.env}-terraform-state-lock" # 用于状态锁，防止并发操作冲突
  # }
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

module "passivbot_v741_task" {
  source = "../../modules/task-definitions/passivbot-v741"

  project              = var.project
  env                  = var.env
  region               = var.region
  common_tags          = var.common_tags
  execution_role_arn   = module.task_base.task_execution_role_arn
  task_role_arn        = module.task_base.task_role_arn
  container_image      = var.passivbot_v741_image
  log_retention_days   = var.log_retention_days

  s3_bucket_name    = module.s3_bucket.bucket_name
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

module "lambda_bot_restarter" {
  source = "../../modules/lambda/bot_restarter"

  project     = var.project
  env         = var.env
  common_tags = var.common_tags

  environment_variables = {
    ENV = var.env
  }
  ecs_cluster_arn = module.ecs.cluster_arn
}