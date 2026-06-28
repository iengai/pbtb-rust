// terraform/modules/lambda/task_state_change_handler/main.tf
module "base" {
  source = "../base"

  project     = var.project
  env         = var.env
  common_tags = var.common_tags

  function_name  = "task-state-change-handler"
  bootstrap_path = "${path.root}/../../../target/lambda/task_state_change_handler/bootstrap"
  architecture   = "x86_64"
  code_s3_bucket = var.lambda_code_bucket

  environment_variables = merge(
    var.environment_variables,
    {
      APP__ECS__REGION                      = var.ecs_region
      APP__ECS__CLUSTER_ARN                 = var.ecs_cluster_arn
      APP__ECS__TD_PASSIVBOT_ARN            = var.td_passivbot_arn
      APP__ECS__TD_PASSIVBOT_CONTAINER_NAME = var.passivbot_container_name
    }
  )
}

# Allow this Lambda to run ECS tasks and pass the task roles.
resource "aws_iam_role_policy" "ecs_run_task" {
  name = "${var.project}-${var.env}-task-state-change-handler-ecs-run-task"
  role = module.base.role_name

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid    = "EcsRunTask"
        Effect = "Allow"
        Action = [
          "ecs:RunTask",
          "ecs:DescribeTasks",
          "ecs:DescribeTaskDefinition",
          "ecs:DescribeClusters"
        ]
        Resource = "*"
      },
      {
        Sid    = "PassEcsTaskRoles"
        Effect = "Allow"
        Action = [
          "iam:PassRole"
        ]
        Resource = [
          var.ecs_task_execution_role_arn,
          var.ecs_task_role_arn
        ]
        Condition = {
          StringEquals = {
            "iam:PassedToService" = "ecs-tasks.amazonaws.com"
          }
        }
      }
    ]
  })
}

# DynamoDB: read Bot desired-state and read/write observed-runtime rows.
# UpdateItem is required by the OOM-restart path: reconcile_stopped_task claims the
# restart via the CAS lock (try_acquire_restart / attach_started_task /
# release_start), all of which are update_item. Without it those ops fail with
# AccessDenied (surfaced only as a generic "service error"), so a memory-related
# stop never auto-restarts and the runtime row is left stuck on `running`.
resource "aws_iam_role_policy" "dynamodb" {
  name = "${var.project}-${var.env}-task-state-change-handler-dynamodb"
  role = module.base.role_name

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "BotsTableRW"
        Effect   = "Allow"
        Action   = ["dynamodb:GetItem", "dynamodb:PutItem", "dynamodb:UpdateItem"]
        Resource = var.dynamodb_table_arn
      }
    ]
  })
}

resource "aws_cloudwatch_event_rule" "ecs_task_state_change" {
  name        = "${var.project}-${var.env}-ecs-task-state-change"
  description = "Trigger task-state-change-handler on ECS task state change"

  event_pattern = jsonencode({
    "source" : ["aws.ecs"],
    "detail-type" : ["ECS Task State Change"],
    "detail" : merge(
      {
        "clusterArn" : [var.ecs_cluster_arn]
      },
      { "lastStatus" : ["RUNNING", "STOPPED"] }
    )
  })

  tags = var.common_tags
}

resource "aws_cloudwatch_event_target" "ecs_task_state_change_to_lambda" {
  rule      = aws_cloudwatch_event_rule.ecs_task_state_change.name
  target_id = "task-state-change-handler"
  arn       = module.base.function_arn
}

resource "aws_lambda_permission" "allow_eventbridge_invoke" {
  statement_id  = "AllowExecutionFromEventBridgeEcsTaskStateChange"
  action        = "lambda:InvokeFunction"
  function_name = module.base.function_name
  principal     = "events.amazonaws.com"
  source_arn    = aws_cloudwatch_event_rule.ecs_task_state_change.arn
}
