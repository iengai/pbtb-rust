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