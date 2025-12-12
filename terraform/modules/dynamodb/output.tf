output "bots_table_name" {
  value = aws_dynamodb_table.bots.name
}

output "bots_table_arn" {
  value = aws_dynamodb_table.bots.arn
}
