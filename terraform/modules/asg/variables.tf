# modules/asg/variables.tf
variable "name" {}
variable "vpc_subnet_ids" { type = list(string) }
variable "security_group_ids" { type = list(string) }
variable "cluster_name" {}
variable "ami_id" {}                    # ECS 优化 AMI
variable "instance_profile_arn" {}      # ECS 节点实例角色
variable "min_size" { default = 0 }
variable "max_size" { default = 50 }
variable "desired_capacity" { default = 0 }

# 实例优先级：小→大
variable "instance_types" {
  type    = list(string)
  default = ["t3a.large","m6a.large","m6a.xlarge","m7a.xlarge"]
}

variable "on_demand_percent" { default = 30 } # 30% 按需, 70% Spot
