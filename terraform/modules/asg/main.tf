# # modules/asg/main.tf
# resource "aws_launch_template" "lt" {
#   name_prefix   = "${var.name}-lt-"
#   image_id      = var.ami_id
#   instance_type = var.small_instance_type
#   update_default_version = true
#
#
#   user_data = base64encode(<<-EOF
#     #!/bin/bash
#     echo "ECS_CLUSTER=${var.cluster_name}" >> /etc/ecs/ecs.config
#   EOF
#   )
#   iam_instance_profile {
#     name = var.instance_profile_name
#   }
#
#   monitoring {
#     enabled = true
#   }
#
#   network_interfaces {
#     security_groups = var.instance_security_group_ids
#   }
# }
#
# resource "aws_autoscaling_group" "asg" {
#   name                = "${var.name}-asg"
#   min_size            = var.min_size
#   max_size            = var.max_size
#   desired_capacity    = var.desired_capacity
#   vpc_zone_identifier = var.vpc_subnet_ids
#
#   mixed_instances_policy {
#     launch_template {
#       launch_template_specification {
#         launch_template_id = aws_launch_template.lt.id
#         version            = "$Latest"
#       }
#       # 优先顺序：小→大（仅当 OD 策略为 prioritized 才按此顺序）
#       dynamic "override" {
#         for_each = var.instance_types
#         content {
#           instance_type = override.value
#           weighted_capacity = 1
#         }
#       }
#     }
#     instances_distribution {
#       on_demand_percentage_above_base_capacity = var.on_demand_percent
#       on_demand_allocation_strategy            = "prioritized"
#       spot_allocation_strategy                 = "capacity-optimized"
#     }
#   }
#
#   lifecycle { create_before_destroy = true }
# }
#
# output "asg_arn" { value = aws_autoscaling_group.asg.arn }
