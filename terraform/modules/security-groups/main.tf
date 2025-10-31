# security group - nat instance
resource "aws_security_group" "nat" {
  name        = "${var.project}-${var.env}-nat-instance-sg"
  description = "Security group for nat instance"
  vpc_id      = var.vpc_id

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  ingress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = [var.vpc_cidr_block]
  }

  tags = merge(
    var.common_tags,
    {
      Name = "${var.project}-${var.env}-nat-instance-sg"
    }
  )
}
#
# # 安全组规则 - 允许 SSH 访问（如果配置了密钥对）
# resource "aws_security_group_rule" "ssh_ingress" {
#   type              = "ingress"
#   from_port         = 22
#   to_port           = 22
#   protocol          = "tcp"
#   cidr_blocks       = ["0.0.0.0/0"] # 生产环境应该限制为特定 IP
#   security_group_id = aws_security_group.ecs_instance.id
#   description       = "SSH access"
# }
#
# # 安全组规则 - 允许 ECS 代理通信
# resource "aws_security_group_rule" "ecs_agent_ingress" {
#   type              = "ingress"
#   from_port         = 0
#   to_port           = 65535
#   protocol          = "tcp"
#   self              = true
#   security_group_id = aws_security_group.ecs_instance.id
#   description       = "ECS agent communication"
# }
#
# # 安全组 - ECS 服务
# resource "aws_security_group" "ecs_service" {
#   name        = "${var.project}-${var.env}-ecs-service-sg"
#   description = "Security group for ECS services"
#   vpc_id      = var.vpc_id
#
#   # 允许出站所有流量
#   egress {
#     from_port   = 0
#     to_port     = 0
#     protocol    = "-1"
#     cidr_blocks = ["0.0.0.0/0"]
#   }
#
#   tags = merge(
#     var.common_tags,
#     {
#       Name = "${var.project}-${var.env}-ecs-service-sg"
#     }
#   )
# }
#
# # 安全组规则 - 允许服务间通信
# resource "aws_security_group_rule" "service_ingress" {
#   type              = "ingress"
#   from_port         = 0
#   to_port           = 65535
#   protocol          = "tcp"
#   self              = true
#   security_group_id = aws_security_group.ecs_service.id
#   description       = "Service to service communication"
# }