# ECS 实例角色 - 允许 ECS 容器实例调用 AWS API
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

# ECS 实例角色策略附件
resource "aws_iam_role_policy_attachment" "ecs_instance_role_policy" {
  role       = aws_iam_role.ecs_instance_role.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonEC2ContainerServiceforEC2Role"
}

# 额外的 ECR 读取权限（用于拉取镜像）
resource "aws_iam_role_policy_attachment" "ecr_read_only" {
  role       = aws_iam_role.ecs_instance_role.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryReadOnly"
}

# CloudWatch Agent 服务器策略（用于容器监控）
resource "aws_iam_role_policy_attachment" "cloudwatch_agent_server_policy" {
  role       = aws_iam_role.ecs_instance_role.name
  policy_arn = "arn:aws:iam::aws:policy/CloudWatchAgentServerPolicy"
}

# SSM 管理策略（用于会话管理）
resource "aws_iam_role_policy_attachment" "ssm_managed_instance_core" {
  role       = aws_iam_role.ecs_instance_role.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonSSMManagedInstanceCore"
}

# 创建实例配置文件
resource "aws_iam_instance_profile" "ecs_instance_profile" {
  name = "${var.project}-${var.env}-ecs-instance-profile"
  role = aws_iam_role.ecs_instance_role.name

  tags = merge(
    var.common_tags,
    {
      Name = "${var.project}-${var.env}-ecs-instance-profile"
    }
  )
}