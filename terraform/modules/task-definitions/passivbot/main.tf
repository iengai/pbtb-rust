# CloudWatch Log Group for this task type
resource "aws_cloudwatch_log_group" "main" {
  name              = "/ecs/${var.project}-${var.env}/passivbot${var.family_suffix}"
  retention_in_days = var.log_retention_days

  tags = merge(
    var.common_tags,
    {
      Name     = "${var.project}-${var.env}-passivbot${var.family_suffix}-logs"
      TaskType = "passivbot"
    }
  )
}

# ECS Task Definition for Bot Processor
resource "aws_ecs_task_definition" "main" {
  family                   = "${var.project}-${var.env}-passivbot${var.family_suffix}"
  network_mode             = "bridge"
  requires_compatibilities = ["EC2"]
  cpu                      = 128
  # Task-level hard limit, set per engine line. The v7 line's 400 MB came from an
  # observed peak (incl. startup) of ~357 MB on the heaviest bot (dual-sided
  # DollarDigger): ~12% headroom while keeping placement dense (~9 tasks/host on the
  # 3835 MiB t4g.medium). With task-level memory set, ECS reserves THIS value for
  # scheduling, so there is no overcommit.
  memory             = var.memory
  execution_role_arn = var.execution_role_arn
  task_role_arn      = var.task_role_arn

  container_definitions = jsonencode([
    {
      name      = var.container_name
      image     = var.container_image
      cpu       = 128
      essential = true
      # No container-level memory/memoryReservation: the single container is capped
      # by the task-level `memory` (400). The old memoryReservation (256, soft) was
      # ignored for placement anyway, since task-level memory drives the reservation.
      # No healthCheck: passivbot serves no HTTP endpoint, so the old curl
      # localhost:8000/health check always failed and marked tasks UNHEALTHY for no
      # reason. `essential = true` already stops the task if the process dies.

      portMappings = var.port_mappings

      logConfiguration = {
        logDriver = "awslogs"
        options = {
          awslogs-group         = aws_cloudwatch_log_group.main.name
          awslogs-region        = var.region
          awslogs-stream-prefix = "passivbot"
        }
      }

      environment = concat([
        {
          name  = "USER_ID"
          value = "required"
        },
        {
          name  = "BOT_ID"
          value = "required"
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
      # Derived from the image URI so the tag can never drift from what runs.
      Version = regex("[^:/]+$", var.container_image)
    }
  )
}