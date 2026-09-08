# ---------------------------------------------------------------------------
# GitHub Actions OIDC role for the pb-runner repo's image build.
#
# pb-runner (the pure-Rust runner of engine line 8, `8rs`) builds its own
# linux/arm64 image and pushes it to the `pb-runner` ECR repo managed here.
# Before this, that image was built by hand on a dev box under QEMU (~65 min)
# and the trading image's supply chain ran through one laptop.
#
# The OIDC provider + `github_oidc_arn` local live in telebot.tf.
#
# pb-runner is a PUBLIC repo, so the trust policy is what keeps forks and pull
# requests out: only the `master` branch ref of that exact repo can assume the
# role, and all it may do is push to one ECR repository.
# ---------------------------------------------------------------------------

variable "pb_runner_github_repo" {
  description = "owner/name of the pb-runner repo allowed to assume the build role"
  type        = string
  default     = "iengai/pb-runner"

  validation {
    condition     = can(regex("^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$", var.pb_runner_github_repo))
    error_message = "pb_runner_github_repo must be \"owner/name\"."
  }
}

# GitHub is moving the OIDC subject to immutable ids, and pb-runner already
# gets the new form while pbtb-rust still gets the classic one:
#
#   gh api repos/iengai/pb-runner/actions/oidc/customization/sub
#     -> sub_claim_prefix "repo:iengai@22829148/pb-runner@1360175272"
#   gh api repos/iengai/pbtb-rust/actions/oidc/customization/sub
#     -> sub_claim_prefix "repo:iengai/pbtb-rust"
#
# A trust policy written in the classic form simply never matches such a
# token: the assume fails with "Not authorized to perform
# sts:AssumeRoleWithWebIdentity" and nothing says why. Both forms are
# accepted here; both name the same repository and branch, so accepting the
# pair grants nothing extra, and the role keeps working whichever form
# GitHub hands out. `owner@<owner id>/repo@<repo id>` also survives a rename,
# which the classic form does not.
variable "pb_runner_github_repo_immutable" {
  description = "owner@ownerid/repo@repoid form of pb_runner_github_repo; \"\" to accept only the classic form"
  type        = string
  default     = "iengai@22829148/pb-runner@1360175272"
}

resource "aws_iam_role" "gh_pb_runner_build" {
  name = "${var.project}-${var.env}-pb-runner-gh-build"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Federated = local.github_oidc_arn }
      Action    = "sts:AssumeRoleWithWebIdentity"
      Condition = {
        StringEquals = { "token.actions.githubusercontent.com:aud" = "sts.amazonaws.com" }
        StringLike = { "token.actions.githubusercontent.com:sub" = compact([
          "repo:${var.pb_runner_github_repo}:ref:refs/heads/master",
          var.pb_runner_github_repo_immutable == "" ? "" : "repo:${var.pb_runner_github_repo_immutable}:ref:refs/heads/master",
        ]) }
      }
    }]
  })

  tags = var.common_tags
}

resource "aws_iam_role_policy" "gh_pb_runner_build" {
  name = "ecr-push"
  role = aws_iam_role.gh_pb_runner_build.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "EcrAuth"
        Effect   = "Allow"
        Action   = "ecr:GetAuthorizationToken"
        Resource = "*"
      },
      {
        Sid    = "EcrPush"
        Effect = "Allow"
        Action = [
          "ecr:BatchCheckLayerAvailability",
          "ecr:InitiateLayerUpload",
          "ecr:UploadLayerPart",
          "ecr:CompleteLayerUpload",
          "ecr:PutImage",
          "ecr:BatchGetImage",
          "ecr:GetDownloadUrlForLayer",
          "ecr:DescribeImages"
        ]
        Resource = module.ecr.repository_arns["pb_runner"]
      }
    ]
  })
}

output "pb_runner_gh_build_role_arn" {
  description = "Set as the AWS_BUILD_ROLE_ARN secret in the pb-runner repo."
  value       = aws_iam_role.gh_pb_runner_build.arn
}
