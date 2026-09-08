project = "scalable-cluster"
env     = "dev"
region  = "ap-northeast-1"
profile = "dev"

vpc_cidr_block = "10.10.0.0/16"

azs = [
  "ap-northeast-1a",
  # "ap-northeast-1c"
]

public_subnet_cidrs = [
  "10.10.0.0/24",
  # "10.10.1.0/24"
]

private_subnet_cidrs = [
  "10.10.10.0/24",
  # "10.10.11.0/24"
]

common_tags = {
  Project = "scalable-cluster"
  Env     = "dev"
}

ecs_cluster_name  = "ecs-self-scaling-cluster"
ecs_instance_type = "t4g.medium"

# NAT instance is upsized to micro and also hosts the telebot container.
nat_instance_type = "t4g.micro"

# Temporary standby NAT. Steady state is false / "primary": no standby instance
# exists and nothing is billed for it. Flip these -- here, in this file, not with
# -var -- to carry trading egress across a NAT rebuild.
# Full procedure: RUNBOOK.md, "Rebuilding the NAT without an egress outage".
nat_standby_enabled = false
nat_egress_active   = "primary"

telebot_image_tag = "latest"

# GitHub repo allowed to assume the CI (build/deploy) roles via OIDC.
github_repo = "iengai/pbtb-rust"
min_size    = 0
max_size    = 3
# Engine lines bots may run on. "7" keeps the original family name (its live
# tasks and log group are untouched); "8" is a separate family. v8 memory is a
# provisional copy of v7's -- measure RSS after the first v8 bot start.
passivbot_engines = {
  "7" = { image_tag = "v7.12.0-arm64", memory = 400, family_suffix = "" }
  "8" = { image_tag = "v8.1.0-arm64", memory = 400 }
  # pb-runner (pure-Rust) image of line 8, used only by bots whose runtime is
  # `rs` (/runtime <bot_id> rs). Enabled 2026-09-08: the image is in the
  # pb-runner ECR repo and the telebot + lambda binaries that parse `8rs` are
  # live (an older lambda binary given a table with this key fails at config
  # load). Changing it needs the scoped apply of module.passivbot_task["8rs"]
  # + the lambda + telebot base-env, then telebot-deploy (RUNBOOK "pb-runner
  # runtime", follow the step order).
  # 64 MB measured, not guessed: both rs bots sit at 16 MB RSS against the
  # 96 MB placeholder this line launched with, so 64 keeps 4x headroom.
  "8rs" = { image_tag = "ca832b69b11314b7eda0565073b540e8a5543686", memory = 64, family_suffix = "-v8-rs", image_repo = "pb_runner", command = ["--live"] }
}

log_retention_days = 30

s3_bucket_name = "bot-configs"
