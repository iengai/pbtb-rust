# terraform/modules/s3_lambda_code/outputs.tf
output "bucket_name" {
  description = "Lambda code S3 bucket name"
  value       = aws_s3_bucket.this.bucket
}

output "bucket_arn" {
  description = "Lambda code S3 bucket ARN"
  value       = aws_s3_bucket.this.arn
}