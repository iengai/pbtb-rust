# terraform/modules/s3_chart/main.tf
# Private bucket for the per-bot return-curve JSON. No public access and no
# bucket policy: the collector Lambda writes and the API function reads each
# tenant's series for its owner, both via their own IAM roles, so nothing is
# granted at the bucket level. The one prefix that leaves the bucket is
# `public/`, the showcase artifacts, read by the pages-publish role and copied
# to the site; the bucket itself stays closed.
resource "aws_s3_bucket" "this" {
  bucket = var.bucket_name

  tags = merge(
    var.common_tags,
    { Name = var.bucket_name }
  )
}

resource "aws_s3_bucket_public_access_block" "this" {
  bucket = aws_s3_bucket.this.id

  block_public_acls       = true
  ignore_public_acls      = true
  block_public_policy     = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_versioning" "this" {
  bucket = aws_s3_bucket.this.id
  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "this" {
  bucket = aws_s3_bucket.this.id
  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

resource "aws_s3_bucket_ownership_controls" "this" {
  bucket = aws_s3_bucket.this.id
  rule {
    object_ownership = "BucketOwnerEnforced"
  }
}
