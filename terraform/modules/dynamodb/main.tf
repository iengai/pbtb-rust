resource "aws_dynamodb_table" "bots" {
  name         = "${var.project}-${var.env}-bots"
  billing_mode = "PAY_PER_REQUEST"

  hash_key  = "pk"
  range_key = "sk"

  attribute {
    name = "pk"
    type = "S"
  }

  attribute {
    name = "sk"
    type = "S"
  }

  server_side_encryption {
    enabled = true
  }

  # Only the link flow's short-lived tickets carry `expires_at`; no other row
  # shape has the attribute, so enabling this cannot reach a bot, a config or a
  # runtime record. Deletion lags by up to 48 hours, so it is a sweeper, not the
  # expiry check — that is enforced on read.
  ttl {
    attribute_name = "expires_at"
    enabled        = true
  }

  point_in_time_recovery {
    enabled = true
  }

  tags = merge(
    var.common_tags,
    {
      Name    = "${var.project}-${var.env}-bots"
      Project = var.project
      Env     = var.env
    }
  )
}
