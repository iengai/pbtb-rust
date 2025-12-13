// terraform/modules/lambda/bot_restarter/main.tf
module "base" {
  source = "../base"

  project     = var.project
  env         = var.env
  common_tags = var.common_tags

  function_name   = "bot-restarter"
  bootstrap_path  = "${path.root}/../../../target/lambda/bot_restarter/bootstrap"
  architecture    = "x86_64"

  environment_variables = var.environment_variables
}

resource "aws_cloudwatch_event_rule" "ecs_task_state_change" {
  name        = "${var.project}-${var.env}-ecs-task-state-change"
  description = "Trigger bot-restarter on ECS task state change"

  event_pattern = jsonencode({
    "source"      : ["aws.ecs"],
    "detail-type" : ["ECS Task State Change"],
    "detail" : merge(
      {
        "clusterArn" : [var.ecs_cluster_arn]
      },
      { "lastStatus" : ["STOPPED"] }
    )
  })

  tags = var.common_tags
}

resource "aws_cloudwatch_event_target" "ecs_task_state_change_to_lambda" {
  rule      = aws_cloudwatch_event_rule.ecs_task_state_change.name
  target_id = "bot-restarter"
  arn       = module.base.function_arn
}

resource "aws_lambda_permission" "allow_eventbridge_invoke" {
  statement_id  = "AllowExecutionFromEventBridgeEcsTaskStateChange"
  action        = "lambda:InvokeFunction"
  function_name = module.base.function_name
  principal     = "events.amazonaws.com"
  source_arn    = aws_cloudwatch_event_rule.ecs_task_state_change.arn
}
