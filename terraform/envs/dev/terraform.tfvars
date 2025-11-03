project = "scalable-cluster"
env = "dev"
region = "ap-northeast-1"
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

# 初学 & 节省成本可先设为 false，全部资源走公有子网做最小验证
# create_nat_gateway = true


common_tags = {
  Project     = "scalable-cluster"
  Env = "dev"
}

ecs_cluster_name = "ecs-self-scaling-cluster"
ecs_instance_type = "t3.small"
min_size = 0
max_size = 3
passivbot_v741_image  = "025418542265.dkr.ecr.ap-northeast-1.amazonaws.com/passivbot-live:v7.4.1"  # 替换为你的实际镜像
passivbot_v741_cpu    = 256
passivbot_v741_memory = 512

log_retention_days = 30

s3_bucket_name = "bot-configs"
