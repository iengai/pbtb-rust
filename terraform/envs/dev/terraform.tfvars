project = "scalable-cluster"
env     = "dev"
region  = "ap-northeast-1"
profile = "dev"

vpc_cidr_block = "10.10.0.0/16"

azs = [
  "ap-northeast-1a",
  # "ap-northeast-1c"
]

public_subnet_cidrs = [
  "10.10.0.0/24",
  # "10.10.1.0/24"
]

private_subnet_cidrs = [
  "10.10.10.0/24",
  # "10.10.11.0/24"
]

common_tags = {
  Project = "scalable-cluster"
  Env     = "dev"
}

ecs_cluster_name  = "ecs-self-scaling-cluster"
ecs_instance_type = "t4g.medium"

# NAT instance is upsized to micro and also hosts the telebot container.
nat_instance_type = "t4g.micro"
telebot_image_tag = "latest"

# GitHub repo allowed to assume the CI (build/deploy) roles via OIDC.
github_repo         = "iengai/pbtb-rust"
min_size            = 0
max_size            = 3
passivbot_image_tag = "v7.12.0-arm64"

log_retention_days = 30

s3_bucket_name = "bot-configs"
