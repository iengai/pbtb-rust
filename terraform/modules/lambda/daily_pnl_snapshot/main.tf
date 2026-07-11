// terraform/modules/lambda/daily_pnl_snapshot/main.tf
module "base" {
  source = "../base"

  project     = var.project
  env         = var.env
  common_tags = var.common_tags

  function_name  = "daily-pnl-snapshot"
  bootstrap_path = "${path.root}/../../../target/lambda/daily_pnl_snapshot/bootstrap"
  architecture   = "x86_64"
  code_s3_bucket = var.lambda_code_bucket

  # In-VPC so outbound Bybit calls egress via the NAT instance's fixed EIP (the
  # read-only keys are IP-whitelisted). S3/DynamoDB are reached over that same
  # egress; the daily volume is tiny.
  subnet_ids         = var.subnet_ids
  security_group_ids = var.security_group_ids

  # Routine runs are incremental (only new days since the stored state), so they
  # are quick; only a bot's FIRST run back-fills `backfill_days`. 900s (the Lambda
  # max) gives that first run headroom on an active account.
  timeout_seconds = 900
  memory_mb       = 256

  environment_variables = merge(
    var.environment_variables,
    {
      # The zip ships only the bootstrap, so all config comes from env.
      APP__DYNAMODB__REGION     = var.region
      APP__DYNAMODB__TABLE_NAME = var.dynamodb_table_name
      # Bot-configs bucket (holds api-keys.json). ENDPOINT_URL starting with
      # https://s3. tells the S3 client to use default AWS resolution.
      APP__S3__REGION           = var.region
      APP__S3__BUCKET_NAME      = var.config_bucket_name
      APP__S3__ENDPOINT_URL     = "https://s3.${var.region}.amazonaws.com"
      APP__BYBIT__BASE_URL      = var.bybit_base_url
      APP__BYBIT__BACKFILL_DAYS = tostring(var.backfill_days)
      # Where the per-bot return series (and private state) are written.
      APP__CHART__BUCKET_NAME = var.chart_bucket_name
      APP__CHART__KEY_PREFIX  = var.chart_key_prefix
    }
  )
}

# Enumerate bots (Scan) and read each bot's config-switch timeline (Query).
resource "aws_iam_role_policy" "dynamodb" {
  name = "${var.project}-${var.env}-daily-pnl-snapshot-dynamodb"
  role = module.base.role_name

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "BotsTableRead"
        Effect   = "Allow"
        Action   = ["dynamodb:Scan", "dynamodb:Query"]
        Resource = var.dynamodb_table_arn
      }
    ]
  })
}

# Read each bot's api-keys.json from the bot-configs bucket; read/write the
# per-bot series + private incremental state on the separate chart bucket.
resource "aws_iam_role_policy" "s3" {
  name = "${var.project}-${var.env}-daily-pnl-snapshot-s3"
  role = module.base.role_name

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "ReadApiKeys"
        Effect   = "Allow"
        Action   = ["s3:GetObject"]
        Resource = "arn:aws:s3:::${var.config_bucket_name}/*"
      },
      {
        # GetObject is needed to read the accumulated per-bot state each run.
        Sid      = "ReadWriteChartData"
        Effect   = "Allow"
        Action   = ["s3:GetObject", "s3:PutObject"]
        Resource = "${var.chart_bucket_arn}/*"
      },
      {
        # ListBucket so a GetObject on a not-yet-existing state key returns a
        # clean 404 (NoSuchKey) rather than 403 — the first run then reads "no
        # state yet" as an absence, not a fault.
        Sid      = "ListChartBucket"
        Effect   = "Allow"
        Action   = ["s3:ListBucket"]
        Resource = var.chart_bucket_arn
      }
    ]
  })
}

# Daily schedule. The handler acts only on detail-type "Scheduled Event"; any
# other invocation (e.g. the deploy smoke test) returns before any Bybit/S3 call.
resource "aws_cloudwatch_event_rule" "daily" {
  name                = "${var.project}-${var.env}-daily-pnl-snapshot"
  description         = "Daily trigger for the return-curve collector"
  schedule_expression = var.schedule_expression

  tags = var.common_tags
}

resource "aws_cloudwatch_event_target" "daily_to_lambda" {
  rule      = aws_cloudwatch_event_rule.daily.name
  target_id = "daily-pnl-snapshot"
  arn       = module.base.function_arn
}

resource "aws_lambda_permission" "allow_eventbridge_invoke" {
  statement_id  = "AllowExecutionFromEventBridgeDailySchedule"
  action        = "lambda:InvokeFunction"
  function_name = module.base.function_name
  principal     = "events.amazonaws.com"
  source_arn    = aws_cloudwatch_event_rule.daily.arn
}
