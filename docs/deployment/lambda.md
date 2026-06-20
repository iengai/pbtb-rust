# Lambda deploy: task_state_change_handler

The `task_state_change_handler` Lambda ships its code **out-of-band** through the
manual **lambda-deploy** GitHub Actions workflow (`.github/workflows/lambda-deploy.yml`),
**not** Terraform. The workflow builds the bootstrap and pushes it with
`aws lambda update-function-code`, so it never touches the env S3 state, the
backend lock, or the NAT lifecycle. Terraform still owns the function's
config/wiring (role, env vars, runtime, memory/timeout); CI owns only the code.

## Function

- **Name:** `scalable-cluster-dev-task-state-change-handler`
  (mirrors Terraform's `${project}-${env}-${function_name}`, with
  `project=scalable-cluster`, `env=dev`). If the project/env or function name is
  renamed, update `FUNCTION_NAME` in the workflow or `update-function-code` 404s.
- **Runtime:** `provided.al2023`
- **Handler:** `bootstrap`
- **Architecture:** `x86_64`
- **Binary / Cargo target:** `task_state_change_handler` (the package also defines
  a `pbtb-rust` bin; the workflow sets `BIN_NAME=task_state_change_handler`).

## Why the build reuses the devcontainer builder stage

The binary is built through the `lambda-export` stage of `.devcontainer/Dockerfile`,
which is `FROM` the same `rust:1.89-bullseye` `builder` stage the devcontainer and
telebot use. That builder links against **glibc 2.31**, which stays below
**AL2023's glibc 2.34**. A plain `cargo build` on the `ubuntu-24.04` runner links
against glibc 2.39 and could reference symbols the Lambda runtime host lacks,
failing at runtime.

The `lambda-export` stage is `FROM scratch` and contains only the bootstrap:

```dockerfile
FROM scratch AS lambda-export
ARG BIN_NAME=task_state_change_handler
COPY --from=builder /out/bin/${BIN_NAME} /bootstrap
```

The workflow builds it for `linux/amd64` (matching the function arch — the x86_64
runner builds natively, no QEMU) and writes the artifact locally:

```
docker build --target lambda-export \
  --build-arg BIN_NAME=task_state_change_handler \
  --platform linux/amd64 --output type=local,dest=artifact .
# -> ./artifact/bootstrap
```

## Shipping new code

Run the **lambda-deploy** workflow manually (`workflow_dispatch`). The
`publish` input (default `true`) controls whether an immutable Lambda version is
published for rollback.

The workflow:

1. Builds the `lambda-export` stage (cargo registry + `target/` cached across runs
   via BuildKit cache mounts and GHA cache).
2. Packages `artifact/bootstrap` into `out.zip` with `bootstrap` at the zip root,
   and records the local SHA-256 (base64).
3. Assumes the deploy role via OIDC.
4. `aws lambda update-function-code --function-name "$FUNCTION_NAME" --zip-file fileb://out.zip`,
   adding `--publish` when the `publish` input is `true` (an immutable version for
   rollback).
5. Waits for the update to settle (`aws lambda wait function-updated`).

## Post-deploy verification

The workflow gates the deploy on two checks:

- **CodeSha256 compare:** the base64 SHA-256 of the local `out.zip` must equal the
  `CodeSha256` returned by `update-function-code`. A mismatch (corrupted upload)
  fails the job.
- **Benign smoke invoke:** it invokes the function with a non-ECS event:

  ```json
  {"version":"0","source":"pbtb.smoke-test","detail-type":"SmokeTest","detail":{}}
  ```

  This hits the source/detail-type guard's early return in `event_handler.rs` and
  returns `Ok(())` **before any ECS or DynamoDB call**, so it **never launches a
  task**. The check passes only on `StatusCode=200` with **no** `FunctionError`.

  > Never smoke-test with a real `STOPPED` event — that would drive the actuation
  > path and could launch a task.

## One-time setup

The deploy role and its policy are Terraform-managed in
`terraform/envs/dev/lambda-ci.tf`. Create them with a targeted apply:

```
AWS_PROFILE=dev terraform apply \
  -target=aws_iam_role.gh_lambda_deploy \
  -target=aws_iam_role_policy.gh_lambda_deploy
```

Then set the GitHub repository secret **`AWS_LAMBDA_DEPLOY_ROLE_ARN`** to the
value of the Terraform output **`lambda_task_state_change_gh_deploy_role_arn`**.

The role is assumable only via GitHub OIDC from `refs/heads/main`, and its policy
grants exactly the calls the workflow makes:
`lambda:UpdateFunctionCode`, `lambda:GetFunctionConfiguration` (the
`wait function-updated` poll), `lambda:PublishVersion` (the `--publish` path), and
`lambda:InvokeFunction` (the smoke invoke), scoped to this function and its
published versions.

> As with any apply in this env, the lambda's `archive_file` data source needs
> `target/lambda/task_state_change_handler/bootstrap` to exist — build the lambda
> first or the command errors on the missing file.

## Drift and emergency Terraform deploy

`aws_lambda_function.this` (in `terraform/modules/lambda/base/main.tf`) carries:

```hcl
lifecycle {
  ignore_changes = [source_code_hash]
}
```

So a CI-shipped code update is **not** treated as drift, and `terraform apply` will
**not** revert it. To deploy lambda code through Terraform in an emergency,
`-replace` the function. The function resource lives in the `base` module, which the
`task_state_change_handler` module wraps, so the address carries both module
segments:

```
terraform apply -replace=module.lambda_task_state_change_handler.module.base.aws_lambda_function.this
```
