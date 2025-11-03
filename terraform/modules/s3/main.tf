resource "aws_s3_bucket" "main" {
  bucket = "${var.project}-${var.env}-bot-configs"
  force_destroy = false

  tags = merge(
    var.common_tags,
    { Name = "${var.project}-${var.env}-bot-configs" }
  )
}

resource "aws_s3_bucket_versioning" "main" {
  bucket = aws_s3_bucket.main.id
  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "main" {
  bucket = aws_s3_bucket.main.id
  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

resource "aws_s3_bucket_public_access_block" "artifact_bucket" {
  bucket                  = aws_s3_bucket.main.id
  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

# S3 Bucket Policy for ECS access
resource "aws_s3_bucket_policy" "ecs_access" {
  bucket = aws_s3_bucket.main.id
  policy = data.aws_iam_policy_document.ecs_s3_access.json
}

# IAM Policy Document for ECS S3 access
data "aws_iam_policy_document" "ecs_s3_access" {
  statement {
    effect = "Allow"

    principals {
      type        = "AWS"
      identifiers = [var.ecs_task_role_arn]
    }

    actions = [
      "s3:GetObject",
      "s3:GetObjectVersion",
      "s3:ListBucket",
      "s3:PutObject",
      "s3:PutObjectAcl",
      "s3:DeleteObject"
    ]

    resources = [
      aws_s3_bucket.main.arn,
      "${aws_s3_bucket.main.arn}/*"
    ]
  }
}