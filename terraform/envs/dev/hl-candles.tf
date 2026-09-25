# Hyperliquid 1m candles for the strategy lab's backtests: a daily Lambda
# outside the VPC stores each coin's UTC day in a private bucket, which the lab
# syncs (scripts/hl_candles_pull.py). Hyperliquid serves only its latest 5000
# candles, so a day not stored within ~3 days is gone. Kept in its own file so
# a scoped `-target` apply never sweeps in the NAT instance / ECS ASG.
#
# First apply (the bootstrap must be built first,
# target/lambda/hl_candle_collector/bootstrap):
#   terraform apply \
#     -target=aws_s3_bucket.candles \
#     -target=aws_s3_bucket_public_access_block.candles \
#     -target=aws_s3_bucket_server_side_encryption_configuration.candles \
#     -target=aws_s3_bucket_ownership_controls.candles \
#     -target=module.lambda_hl_candle_collector \
#     -target=aws_iam_role_policy.gh_lambda_deploy

resource "aws_s3_bucket" "candles" {
  bucket = "${var.project}-${var.env}-market-data"

  tags = merge(
    var.common_tags,
    { Name = "${var.project}-${var.env}-market-data" }
  )
}

resource "aws_s3_bucket_public_access_block" "candles" {
  bucket = aws_s3_bucket.candles.id

  block_public_acls       = true
  ignore_public_acls      = true
  block_public_policy     = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_server_side_encryption_configuration" "candles" {
  bucket = aws_s3_bucket.candles.id
  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

resource "aws_s3_bucket_ownership_controls" "candles" {
  bucket = aws_s3_bucket.candles.id
  rule {
    object_ownership = "BucketOwnerEnforced"
  }
}

module "lambda_hl_candle_collector" {
  source = "../../modules/lambda/hl_candle_collector"

  project     = var.project
  env         = var.env
  common_tags = var.common_tags
  region      = var.region

  environment_variables = {
    ENV                      = var.env
    APP__SENTRY__DSN         = var.sentry_dsn
    APP__SENTRY__ENVIRONMENT = var.env
  }

  lambda_code_bucket = module.lambda_code_bucket.bucket_name
  config_bucket_name = module.s3_bucket.bucket_name
  candle_bucket_name = aws_s3_bucket.candles.bucket
  candle_bucket_arn  = aws_s3_bucket.candles.arn
}
