# modules/ecs-service/variables.tf
variable "name" {}
variable "cluster_name" {}
variable "image" {}
variable "private_subnet_ids" { type = list(string) }
variable "security_group_ids" { type = list(string) }
variable "task_execution_role_arn" {}
variable "task_role_arn" {}
variable "desired_count" { default = 0 }  # 长驻数量, 也可置0 以 run-task 方式管理
