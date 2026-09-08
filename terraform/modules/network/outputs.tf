output "vpc_id" {
  description = "ID of the VPC"
  value       = aws_vpc.main.id
}

output "private_subnet_ids" {
  description = "IDs of the private subnets"
  value       = aws_subnet.private[*].id
}

output "app_sg_id" {
  value = aws_security_group.app_sg.id
}

output "nat_instance_id" {
  description = "ID of the primary NAT instance (also the telebot host)"
  value       = aws_instance.nat.id
}

output "nat_standby_instance_id" {
  description = "ID of the temporary standby NAT instance, or null when it is not deployed"
  value       = one(aws_instance.nat_standby[*].id)
}

output "nat_eip_public_ip" {
  description = "The whitelisted egress address. It follows nat_egress_active, so this is always the address the exchange sees."
  value       = aws_eip.nat.public_ip
}

output "nat_egress_active" {
  description = "Which NAT currently carries the private default route and the EIP"
  value       = var.nat_egress_active
}

output "s3_gateway_endpoint_id" {
  description = "Gateway endpoint carrying the private subnets' S3 traffic (including ECR image layers)"
  value       = aws_vpc_endpoint.s3.id
}

output "dynamodb_gateway_endpoint_id" {
  description = "Gateway endpoint carrying the private subnets' DynamoDB traffic"
  value       = aws_vpc_endpoint.dynamodb.id
}
