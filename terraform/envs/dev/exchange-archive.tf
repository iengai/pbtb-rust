# Raw exchange history: the daily collector's monthly job copies every Bybit
# bot's transaction log and fills here, one object per month, because Bybit
# serves only the last two years. Versioned and with no lifecycle rule: nothing
# here expires. Kept in its own file so a scoped `-target` apply never sweeps
# in the NAT instance / ECS ASG.
#
# First apply (the collector bootstrap must be built first,
# target/lambda/daily_pnl_snapshot/bootstrap):
#   terraform apply \
#     -target=aws_s3_bucket.exchange_archive \
#     -target=aws_s3_bucket_public_access_block.exchange_archive \
#     -target=aws_s3_bucket_versioning.exchange_archive \
#     -target=aws_s3_bucket_server_side_encryption_configuration.exchange_archive \
#     -target=aws_s3_bucket_ownership_controls.exchange_archive \
#     -target=module.lambda_daily_pnl_snapshot

resource "aws_s3_bucket" "exchange_archive" {
  bucket = "${var.project}-${var.env}-exchange-archive"

  tags = merge(
    var.common_tags,
    { Name = "${var.project}-${var.env}-exchange-archive" }
  )
}

resource "aws_s3_bucket_public_access_block" "exchange_archive" {
  bucket = aws_s3_bucket.exchange_archive.id

  block_public_acls       = true
  ignore_public_acls      = true
  block_public_policy     = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_versioning" "exchange_archive" {
  bucket = aws_s3_bucket.exchange_archive.id
  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "exchange_archive" {
  bucket = aws_s3_bucket.exchange_archive.id
  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

resource "aws_s3_bucket_ownership_controls" "exchange_archive" {
  bucket = aws_s3_bucket.exchange_archive.id
  rule {
    object_ownership = "BucketOwnerEnforced"
  }
}
