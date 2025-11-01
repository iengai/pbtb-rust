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

# security group for ECS instances
resource "aws_security_group" "ecs" {
  name        = "${var.project}-${var.env}-ecs-sg"
  description = "Security group for ECS container instances"
  vpc_id      = var.vpc_id

  # outbound
  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  # inbound
  ingress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = [var.vpc_cidr_block]
  }

  # allow ssh for debug
  ingress {
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = [var.vpc_cidr_block]
  }

  tags = merge(
    var.common_tags,
    {
      Name = "${var.project}-${var.env}-ecs-sg"
    }
  )
}
