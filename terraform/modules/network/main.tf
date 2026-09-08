# create VPC
resource "aws_vpc" "main" {
  cidr_block           = var.vpc_cidr_block
  enable_dns_hostnames = true
  enable_dns_support   = true

  tags = merge(
    var.tags,
    {
      Name = "${var.project}-${var.env}-vpc"
    }
  )
}

# create internet gateway
resource "aws_internet_gateway" "main" {
  vpc_id = aws_vpc.main.id

  tags = merge(
    var.tags,
    {
      Name = "${var.project}-${var.env}-igw"
    }
  )
}

# create public subnet
resource "aws_subnet" "public" {
  count = length(var.public_subnet_cidrs)

  vpc_id                  = aws_vpc.main.id
  cidr_block              = var.public_subnet_cidrs[count.index]
  availability_zone       = var.azs[count.index]
  map_public_ip_on_launch = true

  tags = merge(
    var.tags,
    {
      Name = "${var.project}-${var.env}-public-subnet-${count.index + 1}"
      Type = "public"
    }
  )
}

# create private subnet
resource "aws_subnet" "private" {
  count = length(var.private_subnet_cidrs)

  vpc_id            = aws_vpc.main.id
  cidr_block        = var.private_subnet_cidrs[count.index]
  availability_zone = var.azs[count.index]

  tags = merge(
    var.tags,
    {
      Name = "${var.project}-${var.env}-private-subnet-${count.index + 1}"
      # Name = "${var.project}-${var.env}-private-subnet"
      Type = "private"
    }
  )
}

# create public route table
resource "aws_route_table" "public" {
  vpc_id = aws_vpc.main.id

  tags = merge(
    var.tags,
    {
      Name = "${var.project}-${var.env}-public-rt"
    }
  )
}

resource "aws_route" "public_default_to_igw" {
  route_table_id         = aws_route_table.public.id
  destination_cidr_block = "0.0.0.0/0"
  gateway_id             = aws_internet_gateway.main.id
}

# associate public subnet to route table
resource "aws_route_table_association" "public" {
  count          = length(aws_subnet.public)
  subnet_id      = aws_subnet.public[count.index].id
  route_table_id = aws_route_table.public.id
}

# create private route table
resource "aws_route_table" "private" {
  vpc_id = aws_vpc.main.id

  tags = merge(
    var.tags,
    {
      Name = "${var.project}-${var.env}-private-rt"
    }
  )
}

# associate private subnet to route table
resource "aws_route_table_association" "private" {
  count          = length(aws_subnet.private)
  subnet_id      = aws_subnet.private[count.index].id
  route_table_id = aws_route_table.private.id
}


# ---- Gateway VPC endpoints ----
#
# S3 and DynamoDB are the two services the private subnets talk to that AWS
# offers as a *gateway* endpoint: free, no ENI, just prefix-list routes in the
# route table. Attaching them takes that traffic off the NAT entirely -- bot
# config reads/writes and, because ECR serves image layers from S3 in-region,
# every image pull as well. What remains on the NAT is the exchange traffic and
# the ECR/SSM/CloudWatch APIs (those are interface endpoints, which are billed
# per hour per AZ and are not worth it here).
#
# Only the private route table is associated. The NAT host sits in the public
# subnet and already reaches S3 and DynamoDB over the IGW at no cost, so routing
# telebot through the endpoints would change its data path for no gain.
#
# No endpoint policy is set, so the default full-access policy applies. Do not
# narrow it casually: ECR image pulls resolve to S3 buckets owned by AWS, and a
# bucket-scoped policy here would break every task launch.
resource "aws_vpc_endpoint" "s3" {
  vpc_id            = aws_vpc.main.id
  service_name      = "com.amazonaws.${var.region}.s3"
  vpc_endpoint_type = "Gateway"
  route_table_ids   = [aws_route_table.private.id]

  tags = merge(
    var.tags,
    {
      Name = "${var.project}-${var.env}-s3-gateway-endpoint"
    }
  )
}

resource "aws_vpc_endpoint" "dynamodb" {
  vpc_id            = aws_vpc.main.id
  service_name      = "com.amazonaws.${var.region}.dynamodb"
  vpc_endpoint_type = "Gateway"
  route_table_ids   = [aws_route_table.private.id]

  tags = merge(
    var.tags,
    {
      Name = "${var.project}-${var.env}-dynamodb-gateway-endpoint"
    }
  )
}

# ---- Security groups ----
resource "aws_security_group" "nat_sg" {
  name        = "${var.project}-${var.env}-nat-instance-sg"
  description = "Security group for nat instance"
  vpc_id      = aws_vpc.main.id

  ingress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = [var.vpc_cidr_block]
  }

  ingress {
    from_port   = 22
    to_port     = 22
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
    description = "admin ssh"
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = { Name = "${var.project}-${var.env}-nat-instance-sg" }
}

resource "aws_security_group" "app_sg" {
  name        = "${var.project}-${var.env}-ecs-sg"
  description = "Security group for ECS container instances"
  vpc_id      = aws_vpc.main.id

  ingress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = [var.vpc_cidr_block]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
  tags = { Name = "${var.project}-${var.env}-ecs-sg" }
}

# create nat instance
resource "aws_instance" "nat" {
  ami                         = var.nat_ami
  instance_type               = var.nat_instance_type
  subnet_id                   = aws_subnet.public[0].id
  vpc_security_group_ids      = [aws_security_group.nat_sg.id]
  associate_public_ip_address = true
  source_dest_check           = false
  iam_instance_profile        = var.nat_iam_instance_profile

  # Enforce IMDSv2. hop_limit=2 (not 1) so the Docker-bridged telebot container
  # — one hop from the host — can still reach IMDS for the instance-role creds;
  # hop_limit=1 would block the container entirely and break the bot's AWS access.
  metadata_options {
    http_endpoint               = "enabled"
    http_tokens                 = "required"
    http_put_response_hop_limit = 2
  }

  user_data = var.nat_user_data != null ? var.nat_user_data : file("${path.module}/nat-userdata-al2023.sh")
  # Force a fresh instance when user-data changes, so cloud-init actually re-runs
  # (an in-place instance_type/user_data change would NOT re-execute user-data).
  user_data_replace_on_change = true

  # user_data is the NAT/host bootstrap ONLY — it carries no app-level config
  # (telebot config is delivered out-of-band via SSM + the telebot-deploy
  # pipeline), so it changes only on a deliberate bootstrap-script edit.
  # user_data_replace_on_change then forces a fresh instance so cloud-init
  # re-runs; because this NAT is the sole egress for all trading traffic, this
  # replaces the egress host and leaves telebot down until the next
  # telebot-deploy. Run such an apply in a maintenance window — see the
  # procedure in terraform/envs/dev/RUNBOOK.md.
  tags = {
    Name = "nat-instance"
  }

  depends_on = [aws_internet_gateway.main]
}

# ---- Temporary standby NAT ----
#
# A throwaway egress path used only while the primary NAT is being replaced.
# It runs the plain NAT bootstrap (no telebot, no app config), so its user_data
# never changes with app churn and it can be created and destroyed freely.
#
# It carries the same instance profile as the primary so it is reachable over
# SSM for verification, and it is deliberately NOT tagged `nat-instance`: the
# telebot-deploy role scopes ssm:SendCommand by that exact tag, so a deploy can
# never land on this host.
resource "aws_instance" "nat_standby" {
  count = var.nat_standby_enabled ? 1 : 0

  ami                         = var.nat_ami
  instance_type               = var.nat_standby_instance_type
  subnet_id                   = aws_subnet.public[0].id
  vpc_security_group_ids      = [aws_security_group.nat_sg.id]
  associate_public_ip_address = true
  source_dest_check           = false
  iam_instance_profile        = var.nat_iam_instance_profile

  metadata_options {
    http_endpoint               = "enabled"
    http_tokens                 = "required"
    http_put_response_hop_limit = 1
  }

  user_data                   = file("${path.module}/nat-userdata-al2023.sh")
  user_data_replace_on_change = true

  tags = merge(
    var.tags,
    {
      Name = "${var.project}-${var.env}-nat-standby"
      Role = "nat-standby"
    }
  )

  depends_on = [aws_internet_gateway.main]
}

locals {
  standby_active = var.nat_egress_active == "standby"

  # Both the default route and the EIP follow this choice, so the egress IP the
  # exchange sees stays the whitelisted one on whichever host is active.
  active_nat_instance_id = local.standby_active ? one(aws_instance.nat_standby[*].id) : aws_instance.nat.id
  active_nat_eni_id      = local.standby_active ? one(aws_instance.nat_standby[*].primary_network_interface_id) : aws_instance.nat.primary_network_interface_id
}

# elastic ip for nat
resource "aws_eip" "nat" {
  domain = "vpc"

  tags = merge(
    var.tags,
    {
      Name = "${var.project}-${var.env}-nat-eip"
    }
  )
}

# The exchange API keys are IP-whitelisted, so the EIP moves with the route:
# whichever NAT is active must present the whitelisted address. Changing
# instance_id replaces the association (disassociate, then associate), which is
# the few-second window where egress leaves through an unwhitelisted address --
# allow_reassociation is deliberately left unset, since it is ForceNew and would
# make the very next apply replace this association for nothing.
resource "aws_eip_association" "nat" {
  allocation_id = aws_eip.nat.id
  instance_id   = local.active_nat_instance_id
}

# Changing network_interface_id is an in-place ReplaceRoute -- one atomic API
# call -- so the failover itself costs seconds, not the minutes an instance
# replacement costs.
resource "aws_route" "private_nat" {
  route_table_id         = aws_route_table.private.id
  destination_cidr_block = "0.0.0.0/0"
  network_interface_id   = local.active_nat_eni_id
}
#
# resource "aws_instance" "ecs-test" {
#   ami                    = data.aws_ami.amazon_linux_2023.id
#   instance_type          = "t4g.nano"
#   count = length(aws_subnet.private)
#   subnet_id              = aws_subnet.private[count.index].id
#   # subnet_id = aws_subnet.private.id
#   vpc_security_group_ids = [aws_security_group.app_sg.id]
#   associate_public_ip_address = false
#   tags = { Name = "ecs-test" }
# }
