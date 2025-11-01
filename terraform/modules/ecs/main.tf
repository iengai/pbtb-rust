# ECS Cluster
resource "aws_ecs_cluster" "main" {
  name = "${var.project}-${var.env}-cluster"

  setting {
    name  = "containerInsights"
    value = "enabled"
  }

  tags = merge(
    var.common_tags,
    {
      Name = "${var.project}-${var.env}-ecs-cluster"
    }
  )
}

# ECS Instance Role
resource "aws_iam_role" "ecs_instance_role" {
  name = "${var.project}-${var.env}-ecs-instance-role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Action = "sts:AssumeRole"
        Effect = "Allow"
        Principal = {
          Service = "ec2.amazonaws.com"
        }
      }
    ]
  })

  tags = merge(
    var.common_tags,
    {
      Name = "${var.project}-${var.env}-ecs-instance-role"
    }
  )
}

resource "aws_iam_role_policy_attachment" "ecs_instance_role_policy" {
  role       = aws_iam_role.ecs_instance_role.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonEC2ContainerServiceforEC2Role"
}

resource "aws_iam_instance_profile" "ecs_instance_profile" {
  name = "${var.project}-${var.env}-ecs-instance-profile"
  role = aws_iam_role.ecs_instance_role.name
}

data "aws_ami" "ecs_optimized_al2023" {
  most_recent = true
  owners      = ["amazon"]

  filter {
    name   = "name"
    values = ["al2023-ami-ecs-*-arm64"]
  }

  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }

  filter {
    name   = "architecture"
    values = ["arm64"]
  }

  filter {
    name   = "root-device-type"
    values = ["ebs"]
  }
}
# Launch Template for ECS instances
resource "aws_launch_template" "ecs" {
  name_prefix   = "${var.project}-${var.env}-ecs-"
  image_id      = data.aws_ami.ecs_optimized_al2023.id
  instance_type = var.ec2_instance_type

  block_device_mappings {
    device_name = "/dev/sdf"

    ebs {
      volume_size = 8
      volume_type = "gp3"
    }
  }

  iam_instance_profile {
    name = aws_iam_instance_profile.ecs_instance_profile.name
  }

  network_interfaces {
    associate_public_ip_address = false
    security_groups             = [var.ecs_sg_id]
  }

  user_data = base64encode(<<-EOF
              #!/bin/bash
              echo ECS_CLUSTER=${aws_ecs_cluster.main.name} >> /etc/ecs/ecs.config
              echo ECS_ENABLE_TASK_IAM_ROLE=true >> /etc/ecs/ecs.config
              echo ECS_AVAILABLE_LOGGING_DRIVERS='["json-file","awslogs"]' >> /etc/ecs/ecs.config
              echo ECS_ENABLE_SPOT_INSTANCE_DRAINING=${var.enable_spot_draining} >> /etc/ecs/ecs.config
              EOF
  )

  tag_specifications {
    resource_type = "instance"

    tags = merge(
      var.common_tags,
      {
        Name = "${var.project}-${var.env}-ecs-instance"
      }
    )
  }

  tag_specifications {
    resource_type = "volume"

    tags = merge(
      var.common_tags,
      {
        Name = "${var.project}-${var.env}-ecs-volume"
      }
    )
  }
}

# Auto Scaling Group for ECS instances
resource "aws_autoscaling_group" "ecs" {
  name_prefix = "${var.project}-${var.env}-ecs-asg-"

  launch_template {
    id      = aws_launch_template.ecs.id
    version = "$Latest"
  }

  vpc_zone_identifier  = var.private_subnet_ids

  min_size         = var.min_capacity
  max_size         = var.max_capacity

  health_check_type         = "EC2"
  health_check_grace_period = 300

  protect_from_scale_in = true

  # 实例刷新配置
  instance_refresh {
    strategy = "Rolling"
    preferences {
      min_healthy_percentage = 50
      instance_warmup        = 300
    }
  }

  tag {
    key                 = "Name"
    value               = "${var.project}-${var.env}-ecs-instance"
    propagate_at_launch = true
  }

  tag {
    key                 = "AmazonECSManaged"
    value               = true
    propagate_at_launch = true
  }

  tag {
    key                 = "aws:ecs:clusterName"
    value               = aws_ecs_cluster.main.name
    propagate_at_launch = true
  }

  dynamic "tag" {
    for_each = var.common_tags
    content {
      key                 = tag.key
      value               = tag.value
      propagate_at_launch = true
    }
  }

  lifecycle {
    create_before_destroy = true
    ignore_changes        = [desired_capacity]
  }
}

# ECS Capacity Provider
resource "aws_ecs_capacity_provider" "ec2" {
  name = "${var.project}-${var.env}-ec2-capacity-provider"

  auto_scaling_group_provider {
    auto_scaling_group_arn         = aws_autoscaling_group.ecs.arn
    managed_termination_protection = "ENABLED"

    managed_scaling {
      status                    = "ENABLED"
      target_capacity           = var.target_capacity
    }
  }

  tags = merge(
    var.common_tags,
    {
      Name = "${var.project}-${var.env}-ec2-capacity-provider"
    }
  )
}

resource "aws_ecs_cluster_capacity_providers" "main" {
  cluster_name = aws_ecs_cluster.main.name

  capacity_providers = [aws_ecs_capacity_provider.ec2.name]

  default_capacity_provider_strategy {
    capacity_provider = aws_ecs_capacity_provider.ec2.name
    weight            = 100
  }
}
