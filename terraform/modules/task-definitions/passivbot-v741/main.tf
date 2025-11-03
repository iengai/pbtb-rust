# CloudWatch Log Group for this task type
resource "aws_cloudwatch_log_group" "main" {
  name              = "/ecs/${var.project}-${var.env}/passivbot-v741"
  retention_in_days = var.log_retention_days

  tags = merge(
    var.common_tags,
    {
      Name     = "${var.project}-${var.env}-passivbot-v741-logs"
      TaskType = "passivbot-v741"
    }
  )
}

# ECS Task Definition for Bot Processor
resource "aws_ecs_task_definition" "main" {
  family                   = "${var.project}-${var.env}-passivbot-v741"
  network_mode             = "bridge"
  requires_compatibilities = ["EC2"]
  cpu                      = 128
  memory                   = 512
  execution_role_arn       = var.execution_role_arn
  task_role_arn            = var.task_role_arn

  container_definitions = jsonencode([
    {
      name      = var.container_name
      image     = var.container_image
      cpu       = 128
      memory    = 512
      memoryReservation = 200
      essential = true

      portMappings = var.port_mappings

      logConfiguration = {
        logDriver = "awslogs"
        options = {
          awslogs-group         = aws_cloudwatch_log_group.main.name
          awslogs-region        = var.region
          awslogs-stream-prefix = "passivbot-v741"
        }
      }

      healthCheck = {
        command     = ["CMD-SHELL", "curl -fsS http://localhost:8000/health || exit 1"]
        interval    = 30
        timeout     = 5
        retries     = 3
        startPeriod = 60
      }

      environment = concat([
        {
          name  = "DEFAULT_USER_ID"
          value = "unknown"
        },
        {
          name  = "DEFAULT_BOT_ID"
          value = "unknown"
        },
        {
          name  = "ENVIRONMENT"
          value = var.env
        },
        {
          name  = "PROJECT"
          value = var.project
        },
        {
          name  = "BUCKET"
          value = var.s3_bucket_name
        },
      ])
    }
  ])

  tags = merge(
    var.common_tags,
    {
      TaskType = "passivbot"
    },
    {
      Version = "v741"
    }
  )
}