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


variable "create_nat_gateway" {
  type        = bool
  description = "是否创建 NAT 网关"
  default     = true
}

variable "common_tags"  {
  description = "Common tags to apply to all resources"
  type        = map(string)
  default     = {}
}

###############################
# ECS 集群配置
###############################
variable "ecs_cluster_name" {
  description = "Name of the ECS cluster"
  type        = string
}

variable "ecs_instance_type" {
  description = "EC2 instance type for ECS container instances"
  type        = string
}

variable "min_size" {
  description = "Minimum number of EC2 instances in the auto scaling group"
  type        = number
}

variable "max_size" {
  description = "Maximum number of EC2 instances in the auto scaling group"
  type        = number
}

variable "desired_capacity" {
  description = "Desired number of EC2 instances in the auto scaling group"
  type        = number
}

# ECS 优化 AMI 配置
variable "ecs_ami_id" {
  description = "AMI ID for ECS-optimized instances. If empty, will use the latest ECS-optimized AMI"
  type        = string
  default     = ""
}

# 密钥对配置
variable "key_name" {
  description = "Name of the existing EC2 Key Pair to allow SSH access to the instances"
  type        = string
  default     = ""
}

# 容器实例配置
variable "enable_container_insights" {
  description = "Whether to enable Container Insights for the ECS cluster"
  type        = bool
  default     = true
}