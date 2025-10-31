variable "region" {
  type        = string
  description = "AWS 区域"
  default     = "ap-northeast-1"
}


variable "profile" {
  type        = string
  description = "AWS CLI 配置的 profile 名称（可选）"
}

variable "project" {
  description = "项目名（用来区分不同项目）"
  type        = string
}

variable "env" {
  type        = string
  description = "环境名（用作资源名前缀）"
}


variable "vpc_cidr_block" {
  type        = string
  description = "VPC CIDR Block"
}


variable "azs" {
  type        = list(string)
  description = "可用区列表"
}


variable "public_subnet_cidrs" {
  type        = list(string)
  description = "公网子网 CIDR 列表"
}


variable "private_subnet_cidrs" {
  type        = list(string)
  description = "私网子网 CIDR 列表"
}

#
# variable "create_nat_gateway" {
#   type        = bool
#   description = "是否创建 NAT 网关"
#   default     = true
# }

variable "common_tags"  {
  description = "Common tags to apply to all resources"
  type        = map(string)
  default     = {}
}

# ###############################
# # ECS 集群配置
# ###############################
# variable "ecs_cluster_name" {
#   description = "Name of the ECS cluster"
#   type        = string
# }
#
# variable "ecs_instance_type" {
#   description = "EC2 instance type for ECS container instances"
#   type        = string
# }
#
# variable "min_size" {
#   description = "Minimum number of EC2 instances in the auto scaling group"
#   type        = number
# }
#
# variable "max_size" {
#   description = "Maximum number of EC2 instances in the auto scaling group"
#   type        = number
# }
#
# variable "desired_capacity" {
#   description = "Desired number of EC2 instances in the auto scaling group"
#   type        = number
# }
#
# # ECS 优化 AMI 配置
# variable "ecs_ami_id" {
#   description = "AMI ID for ECS-optimized instances. If empty, will use the latest ECS-optimized AMI"
#   type        = string
#   default     = ""
# }
#
# # 密钥对配置
# variable "key_name" {
#   description = "Name of the existing EC2 Key Pair to allow SSH access to the instances"
#   type        = string
#   default     = ""
# }
#
# # 容器实例配置
# variable "enable_container_insights" {
#   description = "Whether to enable Container Insights for the ECS cluster"
#   type        = bool
#   default     = true
# }
#
# # ECS 任务定义配置
# variable "task_family" {
#   description = "Family name for the ECS task definition"
#   type        = string
# }
#
# variable "container_name" {
#   description = "Name of the main container"
#   type        = string
# }
#
# variable "container_image" {
#   description = "Docker image for the container"
#   type        = string
# }
#
# variable "container_cpu" {
#   description = "CPU units for the container (1024 = 1 vCPU)"
#   type        = number
# }
#
# variable "container_memory" {
#   description = "Memory for the container (in MiB)"
#   type        = number
# }
#
# variable "essential" {
#   description = "Whether the container is essential (if it dies, the task stops)"
#   type        = bool
#   default     = true
# }
#
# # 环境变量配置（用于传递脚本参数）
# variable "container_environment" {
#   description = "Environment variables to pass to the container"
#   type        = list(object({
#     name  = string
#     value = string
#   }))
#   default = []
# }
#
# # ECS 服务配置
# variable "service_name" {
#   description = "Name of the ECS service"
#   type        = string
# }
#
# variable "desired_count" {
#   description = "Desired number of tasks to run"
#   type        = number
# }
#
# variable "enable_execute_command" {
#   description = "Whether to enable ECS Exec for the service"
#   type        = bool
#   default     = true
# }
#
# # 自动扩展配置
# variable "enable_autoscaling" {
#   description = "Whether to enable autoscaling for the ECS service"
#   type        = bool
#   default     = true
# }
#
# variable "autoscaling_min_capacity" {
#   description = "Minimum number of tasks for autoscaling"
#   type        = number
# }
#
# variable "autoscaling_max_capacity" {
#   description = "Maximum number of tasks for autoscaling"
#   type        = number
# }
#
# variable "autoscaling_target_cpu" {
#   description = "Target CPU utilization percentage for autoscaling"
#   type        = number
#   default     = 75
# }
#
# variable "autoscaling_scale_in_cooldown" {
#   description = "Cooldown period for scale in events in seconds"
#   type        = number
#   default     = 300
# }
#
# variable "autoscaling_scale_out_cooldown" {
#   description = "Cooldown period for scale out events in seconds"
#   type        = number
#   default     = 300
# }
#
# # 日志配置
# variable "log_retention_in_days" {
#   description = "Number of days to retain CloudWatch logs"
#   type        = number
#   default     = 30
# }
#
# # 固定出口 IP 配置
# variable "enable_nat_gateway" {
#   description = "Whether to enable NAT Gateway for fixed egress IP"
#   type        = bool
#   default     = true
# }
#
# variable "single_nat_gateway" {
#   description = "Whether to use a single NAT Gateway for all AZs (cost optimization)"
#   type        = bool
#   default     = true
# }