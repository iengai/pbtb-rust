// terraform/modules/lambda/hl_candle_collector/main.tf
module "base" {
  source = "../base"

  project     = var.project
  env         = var.env
  common_tags = var.common_tags

  function_name  = "hl-candle-collector"
  bootstrap_path = "${path.root}/../../../target/lambda/hl_candle_collector/bootstrap"
  architecture   = "x86_64"
  code_s3_bucket = var.lambda_code_bucket

  # Outside the VPC: the candle query is public and needs no fixed IP, so the
  # function never shares the NAT instance with trading traffic.

  # A routine run is one request per coin, paced ~2.5 s apart; a run catching
  # up on three days per coin takes a few minutes.
  timeout_seconds = 600
  memory_mb       = 128

  environment_variables = merge(
    var.environment_variables,
    {
      # The zip ships only the bootstrap, so all config comes from env.
      # Bot-configs bucket, read for its predefined/ templates only.
      # ENDPOINT_URL starting with https://s3. tells the S3 client to use
      # default AWS resolution.
      APP__S3__REGION           = var.region
      APP__S3__BUCKET_NAME      = var.config_bucket_name
      APP__S3__ENDPOINT_URL     = "https://s3.${var.region}.amazonaws.com"
      APP__CANDLES__BUCKET_NAME = var.candle_bucket_name
      APP__CANDLES__KEY_PREFIX  = var.candle_key_prefix
    }
  )
}

# Read the templates for the coins to collect; nothing else in the bot-configs
# bucket (it also holds every bot's credentials).
resource "aws_iam_role_policy" "s3" {
  name = "${var.project}-${var.env}-hl-candle-collector-s3"
  role = module.base.role_name

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "ListTemplates"
        Effect   = "Allow"
        Action   = ["s3:ListBucket"]
        Resource = "arn:aws:s3:::${var.config_bucket_name}"
        Condition = {
          StringLike = { "s3:prefix" = ["predefined/*"] }
        }
      },
      {
        Sid      = "ReadTemplates"
        Effect   = "Allow"
        Action   = ["s3:GetObject"]
        Resource = "arn:aws:s3:::${var.config_bucket_name}/predefined/*"
      },
      {
        # HeadObject is authorised by GetObject.
        Sid      = "ReadWriteCandles"
        Effect   = "Allow"
        Action   = ["s3:GetObject", "s3:PutObject"]
        Resource = "${var.candle_bucket_arn}/*"
      },
      {
        # ListBucket so a HEAD on a day not stored yet returns 404, not 403,
        # and to tell a coin with history from a new one.
        Sid      = "ListCandles"
        Effect   = "Allow"
        Action   = ["s3:ListBucket"]
        Resource = var.candle_bucket_arn
      }
    ]
  })
}

# The handler acts only on detail-type "Scheduled Event"; any other invocation
# (the deploy smoke test) returns before any S3 or Hyperliquid call.
resource "aws_cloudwatch_event_rule" "daily" {
  name                = "${var.project}-${var.env}-hl-candle-collector"
  description         = "Daily trigger for the Hyperliquid 1m candle collector"
  schedule_expression = var.schedule_expression

  tags = var.common_tags
}

resource "aws_cloudwatch_event_target" "daily_to_lambda" {
  rule      = aws_cloudwatch_event_rule.daily.name
  target_id = "hl-candle-collector"
  arn       = module.base.function_arn
}

resource "aws_lambda_permission" "allow_eventbridge_invoke" {
  statement_id  = "AllowExecutionFromEventBridgeDailySchedule"
  action        = "lambda:InvokeFunction"
  function_name = module.base.function_name
  principal     = "events.amazonaws.com"
  source_arn    = aws_cloudwatch_event_rule.daily.arn
}
