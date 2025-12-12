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
