# Return-curve feature: a daily in-VPC Lambda that pulls Bybit history and
# writes per-bot JSON to a private chart bucket, plus that bucket. Kept in its
# own file (and its own modules) so a scoped `-target` apply of this feature
# never sweeps in the NAT instance / ECS ASG — see AGENTS.md + RUNBOOK.md.
#
# First apply must target only these resources, e.g.:
#   terraform apply \
#     -target=module.chart_bucket \
#     -target=module.lambda_daily_pnl_snapshot
# and the collector bootstrap must be built first (target/lambda/daily_pnl_snapshot/bootstrap).

module "chart_bucket" {
  source = "../../modules/s3_chart"

  common_tags = var.common_tags
  bucket_name = "${var.project}-${var.env}-return-charts"
}

module "lambda_daily_pnl_snapshot" {
  source = "../../modules/lambda/daily_pnl_snapshot"

  project     = var.project
  env         = var.env
  common_tags = var.common_tags
  region      = var.region

  environment_variables = { ENV = var.env }

  lambda_code_bucket  = module.lambda_code_bucket.bucket_name
  dynamodb_table_name = module.dynamodb.bots_table_name
  dynamodb_table_arn  = module.dynamodb.bots_table_arn
  config_bucket_name  = module.s3_bucket.bucket_name
  chart_bucket_name   = module.chart_bucket.bucket_name
  chart_bucket_arn    = module.chart_bucket.bucket_arn

  # In-VPC egress via the NAT instance's fixed EIP (IP-whitelisted Bybit keys).
  # Reuses the same subnet/SG references the ECS module consumes; does NOT touch
  # the NAT/EIP/route resources themselves.
  subnet_ids         = module.network.private_subnet_ids
  security_group_ids = [module.network.app_sg_id]
}
