# passivbot live image

The `linux/arm64` live-trading image that the ECS `…-passivbot` task definition
runs. Source of truth for the build files; the passivbot application code comes
from a checkout of upstream (`E:/projects/passivbot`, currently **v7.12.0**).

ECR repo: `passivbot-live` (account `025418542265`, `ap-northeast-1`).
Current tag: **`v7.12.0-arm64`**.

## Files

| File | Purpose |
|------|---------|
| `Dockerfile.ecs` | multi-stage arm64 build: compiles the Rust ext + live wheel, installs awscli v2, ships `src/` + `entrypoint.sh`, `SKIP_RUST_COMPILE=true` |
| `entrypoint.sh` | runtime contract: ECS injects `BUCKET`/`USER_ID`/`BOT_ID`; pulls config + api-keys from S3, runs `python src/main.py configs/$BOT_ID.json` (launches passivbot live; user comes from `live.user`) |
| `buildspec.yml` | CodeBuild spec: ECR login → `docker build` → push |

## Why CodeBuild (not local)

The cluster is `t4g` (Graviton), so the image must be `arm64`. Building arm64 on
an x86 host needs QEMU (slow, heavy). Instead we build on a **native arm64**
CodeBuild fleet — and `arm1.small` is in the CodeBuild free tier (100 min/month),
so a build costs nothing.

## Build a new version

```bash
python scripts/build_passivbot_image.py --tag v7.12.0-arm64
# next bump, after pointing --passivbot-dir at the new checkout:
python scripts/build_passivbot_image.py --tag v7.13.0-arm64
```

The script overlays these build files onto a subset of the passivbot checkout
(excluding `api-keys.json`, caches, `*.pyd`), zips it to
`s3://scalable-cluster-dev-lambda-code/passivbot-build/source.zip`, and starts the
CodeBuild project with `IMAGE_TAG` overridden.

Then roll it out (see `terraform/envs/dev/RUNBOOK.md` → "Bump the passivbot
version"): set `passivbot_image_tag` in `terraform.tfvars` → `terraform apply`
(registers a new `…-passivbot` task-def revision) → run **telebot-deploy**
(`passivbot_revision=latest`) so telebot launches the new revision too.

## One-time AWS setup (already done)

- **CodeBuild project** `pbtb-passivbot-image-builder`: `ARM_CONTAINER`,
  `BUILD_GENERAL1_SMALL`, `aws/codebuild/amazonlinux2-aarch64-standard:3.0`,
  privileged (for `docker build`), source = the S3 zip above. Env: `AWS_REGION`,
  `ECR_REGISTRY`, `ECR_REPO=passivbot-live`, `IMAGE_TAG` (overridden per build).
- **IAM role** `pbtb-passivbot-image-builder`: ECR push to `passivbot-live`, read
  the S3 source object, CloudWatch Logs, and `ecr-public:GetAuthorizationToken` +
  `sts:GetServiceBearerToken` (the base image is pulled from ECR Public, not
  Docker Hub, to dodge Docker Hub's anonymous 429 rate limit on the shared
  CodeBuild egress IP).

These are CLI-managed build infra (not in Terraform). To recreate, see the role
trust/permission policy and `aws codebuild create-project` invocation in the
project's git history for this change.
