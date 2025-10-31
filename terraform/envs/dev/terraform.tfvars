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
##########################
# ECS 集群配置
##########################
# ecs_cluster_name = "ecs-self-scaling-cluster"
# ecs_instance_type = "t4g.medium"
# min_size = 0
# max_size = 0
# desired_capacity = 0
#
# # ECS 优化 AMI（留空使用最新版本）
# ecs_ami_id = ""
#
# # 密钥对（如果需要 SSH 访问）
# key_name = ""
#
# # 监控配置
# enable_container_insights = true
#
#
# # ECS 任务定义配置
# task_family      = "passivbot"
# container_name   = "passivbot-live"
# container_image  = "your-registry/trading-script:latest"  # 替换为你的实际镜像
# container_cpu    = 256
# container_memory = 512
#
# # 环境变量示例（用于传递不同的脚本参数）
# container_environment = [
#   {
#     name  = "user_id"
#     value = "user1"
#   },
#   {
#     name  = "bot_id"
#     value = "bot1"
#   }
# ]
#
# # ECS 服务配置
# service_name = "passivbot-live-service"
# desired_count = 0
# enable_execute_command = true
#
# # 自动扩展配置
# enable_autoscaling        = true
# autoscaling_min_capacity  = 0
# autoscaling_max_capacity  = 20
# autoscaling_target_cpu    = 70
#
# # 日志配置
# log_retention_in_days = 30
#
# # 固定出口 IP 配置
# enable_nat_gateway   = true
# single_nat_gateway   = true  # 使用单个 NAT 网关降低成本，同时提供固定出口 IP