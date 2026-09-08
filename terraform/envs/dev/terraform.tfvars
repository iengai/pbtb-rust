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
  # load).
  #
  # `v810` is a MOVING tag, unlike every other line here, and that is the
  # point: the passivbot images are cut once per upstream release, but
  # pb-runner is our own code and ships fixes far more often. The tag names
  # the passivbot VERSION LINE the image serves (v8.1.0); pb-runner's
  # image-build workflow re-points it at each build, and ECS re-pulls it on
  # every task start, so shipping a fix is a build plus a bot restart -- no
  # apply here, no telebot-deploy. What still belongs in terraform is a new
  # version line (a v8.2.0 runner: its own tag, its own entry).
  #
  # The cost is that this file no longer records which build is running.
  # `pb-runner` logs `build=<git sha>` on its first line for that, and every
  # build also keeps an immutable `<git sha>` tag to roll back to (RUNBOOK
  # "pb-runner runtime").
  #
  # 64 MB measured, not guessed: both rs bots sit at 16 MB RSS against the
  # 96 MB placeholder this line launched with, so 64 keeps 4x headroom.
  "8rs" = { image_tag = "v810", memory = 64, family_suffix = "-v8-rs", image_repo = "pb_runner", command = ["--live"] }
}

# pb-runner shadow runs (P5.2): dry-run tasks watching a live bot's account,
# config and key, planning what they would do and logging only. See
# passivbot-shadow.tf for why they are not engine entries and must be launched
# without RunTask overrides.
#
# Only line-8 bots can be shadowed: the image is built `engine-v8` and refuses
# a v7 config at startup. Of the four live bots today, `xxbot` (516903813) and
# `abot` (415196485) are v8.1.0; DollarDigger (436713564) and Low-Risk Trader
# (516889601) are v7.12.0 and need P7 first.
passivbot_shadows = {
  xxbot = { user_id = "5351347639", bot_id = "516903813" }

  # NOT a parity shadow: DollarDigger (436713564) trades a v7.12.0 config, so
  # pb-runner cannot run its config at all. This one runs the v8.1.0 migration
  # of the same strategy (predefined/bybit-cap300-iter1-winner-v810.json --
  # same cap tier, same 8 coins, long n_positions=1 / twel=1.75, short off) on
  # DollarDigger's ACCOUNT, to see what v8 would plan there. Its cancel/create
  # lines are the two strategy versions disagreeing, not a port defect, so do
  # not read `cancels=0 creates=0` into this one.
  #
  # `436713564-v8ref` is an S3 config directory, not a bot: it holds that
  # config with `live.user = "436713564"` and a copy of DollarDigger's
  # api-keys.json, so the key is the live bot's own. There is no DynamoDB row
  # and telebot does not know about it.
  dollardigger_v8ref = { user_id = "5351347639", bot_id = "436713564-v8ref" }
}

log_retention_days = 30

s3_bucket_name = "bot-configs"
