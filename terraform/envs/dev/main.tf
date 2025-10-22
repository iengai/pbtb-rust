terraform {
  required_version = ">= 1.6.0"
  required_providers {
    aws = {
      source = "hashicorp/aws"
      version = ">= 5.0"
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
  source = "../../modules/network"
  project = var.project
  env = var.env
  vpc_cidr_block = var.vpc_cidr_block
  azs = var.azs
  public_subnet_cidrs = var.public_subnet_cidrs
  private_subnet_cidrs = var.private_subnet_cidrs
  tags = var.common_tags
}

output "vpc_id" {
  description = "ID of the created VPC"
  value       = module.network.vpc_id
}

output "public_subnet_ids" {
  description = "IDs of the public subnets"
  value       = module.network.public_subnet_ids
}

output "private_subnet_ids" {
  description = "IDs of the private subnets"
  value       = module.network.private_subnet_ids
}

output "vpc_cidr_block" {
  description = "CIDR block of the VPC"
  value       = module.network.vpc_cidr_block
}

output "nat_gateway_ips" {
  description = "Elastic IPs of the NAT Gateways"
  value       = module.network.nat_gateway_ips
}
#######################
# 调用安全组模块
#####################
module "security_groups" {
  source = "../../modules/ecs-security-groups"

  project = var.project
  env  = var.env
  vpc_id       = module.network.vpc_id

  common_tags = var.common_tags
}

# 调用 IAM 模块
module "iam" {
  source = "../../modules/iam"

  project = var.project
  env  = var.env

  common_tags = var.common_tags
}

# 调用 ECS 集群模块
module "ecs_cluster" {
  source = "../../modules/ecs-cluster"

  # 基础配置
  project  = var.project
  env   = var.env
  region    = var.region

  # 集群配置
  ecs_cluster_name           = var.ecs_cluster_name
  enable_container_insights  = var.enable_container_insights

  # 网络配置
  vpc_id              = module.network.vpc_id
  private_subnet_ids  = module.network.private_subnet_ids

  # 安全组配置
  security_group_ids = [
    module.security_groups.ecs_instance_sg_id,
    module.security_groups.ecs_service_sg_id
  ]

  # IAM 配置
  ecs_instance_role_arn   = module.iam.ecs_instance_role_arn
  ecs_instance_profile_name = module.iam.ecs_instance_profile_name

  # 自动扩展配置
  ecs_instance_type = var.ecs_instance_type
  min_size          = var.min_size
  max_size          = var.max_size
  desired_capacity  = var.desired_capacity

  # AMI 和密钥配置
  ecs_ami_id = var.ecs_ami_id
  key_name   = var.key_name

  # 标签
  common_tags = var.common_tags

  depends_on = [
    module.network,
    module.security_groups,
    module.iam
  ]
}

