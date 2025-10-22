project = "scalable-cluster"
env = "dev"
region = "ap-northeast-1"
profile = "dev"

vpc_cidr_block = "10.10.0.0/16"

azs = [
  "ap-northeast-1a",
  "ap-northeast-1c"
]

public_subnet_cidrs = [
  "10.10.0.0/24",
  "10.10.1.0/24"
]

private_subnet_cidrs = [
  "10.10.10.0/24",
  "10.10.11.0/24"
]

# 初学 & 节省成本可先设为 false，全部资源走公有子网做最小验证
create_nat_gateway = true


common_tags = {
  Project     = "scalable-cluster"
  Env = "dev"
}
##########################
# ECS 集群配置
##########################
ecs_cluster_name = "ecs-self-scaling-cluster"
ecs_instance_type = "t3.medium"
min_size = 1
max_size = 10
desired_capacity = 1

# ECS 优化 AMI（留空使用最新版本）
ecs_ami_id = ""

# 密钥对（如果需要 SSH 访问）
key_name = ""

# 监控配置
enable_container_insights = true