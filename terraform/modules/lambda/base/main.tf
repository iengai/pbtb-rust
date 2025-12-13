// terraform/modules/lambda/base/main.tf
data "archive_file" "lambda_zip" {
  type        = "zip"
  source_file = var.bootstrap_path
  output_path = "${path.module}/../.build/${var.env}-${var.function_name}.zip"
}

resource "aws_s3_object" "lambda_zip" {
  bucket = var.code_s3_bucket
  key    = "${var.code_s3_key_prefix}/${var.project}/${var.env}/${var.function_name}.zip"

  source      = data.archive_file.lambda_zip.output_path
  source_hash = data.archive_file.lambda_zip.output_base64sha256

  content_type = "application/zip"
  tags         = var.common_tags
}

resource "aws_iam_role" "lambda_exec" {
  name = "${var.project}-${var.env}-${var.function_name}-role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Principal = { Service = "lambda.amazonaws.com" }
      Action = "sts:AssumeRole"
    }]
  })

  tags = merge(
    var.common_tags,
    { Name = "${var.project}-${var.env}-${var.function_name}-role" }
  )
}

resource "aws_iam_role_policy_attachment" "basic" {
  role       = aws_iam_role.lambda_exec.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
}

resource "aws_lambda_function" "this" {
  function_name = "${var.project}-${var.env}-${var.function_name}"
  role          = aws_iam_role.lambda_exec.arn

  s3_bucket = aws_s3_object.lambda_zip.bucket
  s3_key    = aws_s3_object.lambda_zip.key

  source_code_hash = data.archive_file.lambda_zip.output_base64sha256

  runtime       = "provided.al2023"
  handler       = "bootstrap"
  architectures = [var.architecture]
  timeout       = var.timeout_seconds
  memory_size   = var.memory_mb

  environment {
    variables = var.environment_variables
  }

  tags = merge(
    var.common_tags,
    { Name = "${var.project}-${var.env}-${var.function_name}" }
  )
}