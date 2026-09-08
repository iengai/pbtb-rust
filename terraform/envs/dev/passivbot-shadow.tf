# ---------------------------------------------------------------------------
# Shadow runs (pb-runner P5.2).
#
# A shadow is a pb-runner task in DRY-RUN watching the same account, the same
# config object and the same API key as a live Python bot, planning what it
# would do and only logging it. It is how the port is checked against real
# exchange inputs: the four offline harnesses (diffcheck / snapcheck /
# plancheck / mockrun) all replay RECORDED inputs, so they pin computation,
# not input acquisition -- which is exactly where D21 hid.
#
# The comparison needs no log diffing. The shadow sees the orders the live bot
# actually has resting, so its own reconciliation IS the metric:
#
#   agreement  -> cancels=0 creates=0 matched=N
#   difference -> the cancel/create lines name symbol, price and order type
#
# Two properties this shape depends on, both deliberate:
#
# 1. NO `command`, so the container runs `pb-runner` with no `--live` and can
#    only read. The live lines pass ["--live"]; this one must never.
# 2. USER_ID / BOT_ID are baked into the task definition and the task is
#    launched with NO RunTask overrides, which makes it invisible to the
#    restart lambda (see the module's `user_id` variable for why that matters:
#    with overrides, a shadow would silently disable the real bot's
#    auto-restart and could be relaunched as a second LIVE bot).
#
# Consequences of (2), accepted: nothing restarts a shadow, and telebot cannot
# see or stop it. Start one with
#   aws ecs run-task --cluster <cluster> --task-definition <family> --count 1
# and stop it with `aws ecs stop-task`.
#
# Deliberately NOT part of var.passivbot_engines: that map is the engine table
# handed to telebot and the lambda, whose parser accepts only `<major>[rs]`
# keys. A shadow is not an engine line, and adding it there would reshape a
# table that a running lambda binary must be able to parse.
# ---------------------------------------------------------------------------

variable "passivbot_shadows" {
  description = <<-EOT
    Dry-run pb-runner shadows, keyed by a short name used in the family and log
    group (`...-passivbot-v8-rs-shadow-<key>`). `bot_id` is the LIVE bot being
    shadowed: the shadow reads that bot's own config and api-keys objects from
    S3, so the two are identical by construction rather than by copying.
    Only engine line 8 configs work -- this image is built `engine-v8`, and a
    v7 config is refused at startup ("config targets engine line 7").
  EOT
  type = map(object({
    user_id = string
    bot_id  = string
    memory  = optional(number, 64)
  }))
  default = {}
}

module "passivbot_shadow" {
  for_each = var.passivbot_shadows
  source   = "../../modules/task-definitions/passivbot"

  project            = var.project
  env                = var.env
  region             = var.region
  common_tags        = var.common_tags
  execution_role_arn = module.task_base.task_execution_role_arn
  task_role_arn      = module.task_base.task_role_arn
  container_name     = var.passivbot_container_name
  log_retention_days = var.log_retention_days
  s3_bucket_name     = module.s3_bucket.bucket_name

  # The same image the `8rs` line runs, so the shadow and the runtime under
  # test never drift apart. `v810` moves with each build (D23 in pb-runner):
  # a restart of the shadow picks up whatever that line would deploy.
  container_image = "${module.ecr.repository_urls[var.passivbot_engines["8rs"].image_repo]}:${var.passivbot_engines["8rs"].image_tag}"
  # No command: dry run. Never ["--live"] here.
  command = null

  family_suffix = "-v8-rs-shadow-${each.key}"
  memory        = each.value.memory
  user_id       = each.value.user_id
  bot_id        = each.value.bot_id
}

output "passivbot_shadow_families" {
  description = "Task-definition family per shadow; run-task these by name."
  value       = { for k, m in module.passivbot_shadow : k => m.task_definition_family }
}
