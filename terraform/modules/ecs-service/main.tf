# # modules/ecs-service/main.tf
# resource "aws_cloudwatch_log_group" "lg" {
#   name              = "/ecs/${var.name}"
#   retention_in_days = 14
# }
#
# resource "aws_ecs_task_definition" "td" {
#   family                   = "${var.name}-td"
#   network_mode             = "awsvpc"
#   requires_compatibilities = ["EC2"]
#   cpu                      = "128"
#   memory                   = "512"
#   execution_role_arn       = var.task_execution_role_arn
#   task_role_arn            = var.task_role_arn
#
#   container_definitions = jsonencode([
#     {
#       name  = "app",
#       image = var.image,
#       essential = true,
#       logConfiguration = {
#         logDriver = "awslogs",
#         options = {
#           awslogs-group         = aws_cloudwatch_log_group.lg.name,
#           awslogs-region        = data.aws_region.current.name,
#           awslogs-stream-prefix = "app"
#         }
#       }
#       # 默认 CMD 留空，运行时覆盖 (--overrides command / env)
#     }
#   ])
# }
#
# data "aws_region" "current" {}
#
# resource "aws_ecs_service" "svc" {
#   name            = var.name
#   cluster         = var.cluster_name
#   task_definition = aws_ecs_task_definition.td.arn
#   desired_count   = var.desired_count
#
#   network_configuration {
#     subnets         = var.private_subnet_ids
#     security_groups = var.security_group_ids
#     assign_public_ip = false
#   }
#
#   capacity_provider_strategy {
#     capacity_provider = "${var.cluster_name}-cp"
#     weight            = 1
#   }
#
#   # 关键：内存优先装箱
#   placement_strategy {
#     type  = "binpack"
#     field = "memory"
#   }
#
#   deployment_minimum_healthy_percent = 100
#   deployment_maximum_percent         = 200
# }
#
# output "task_definition_arn" { value = aws_ecs_task_definition.td.arn }
