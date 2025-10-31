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
      Type = "private"
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

# create public route table
resource "aws_route_table" "public" {
  vpc_id = aws_vpc.main.id

  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.main.id
  }

  tags = merge(
    var.tags,
    {
      Name = "${var.project}-${var.env}-public-rt"
    }
  )
}

# associate public subnet to route table
resource "aws_route_table_association" "public" {
  count = length(aws_subnet.public)

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
  count = length(aws_subnet.private)

  subnet_id      = aws_subnet.private[count.index].id
  route_table_id = aws_route_table.private.id
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

#  Amazon Linux 2 AMI
data "aws_ami" "amazon_linux_2" {
  most_recent = true
  owners      = ["amazon"]

  filter {
    name   = "name"
    values = ["amzn2-ami-hvm-*-arm64-gp2"]
  }

  filter {
    name   = "virtualization-type"
    values = ["hvm"]
  }

  filter {
    name   = "architecture"
    values = ["arm64"]
  }
}

# create nat instance
resource "aws_instance" "nat" {
  ami                         = data.aws_ami.amazon_linux_2.id
  instance_type               = "t4g.nano"
  subnet_id                   = aws_subnet.public[0].id
  vpc_security_group_ids      = [var.nat_sg_id]
  associate_public_ip_address = true
  source_dest_check           = false

  user_data = <<-EOF
              #!/bin/bash
              sudo sysctl -w net.ipv4.ip_forward=1
              sudo iptables -t nat -A POSTROUTING -o eth0 -j MASQUERADE
              echo 'net.ipv4.ip_forward=1' | sudo tee -a /etc/sysctl.conf
              sudo yum update -y
              sudo yum install iptables-services -y
              sudo service iptables save
              EOF

  tags = {
    Name = "nat-instance"
  }

  depends_on = [aws_internet_gateway.main]
}

resource "aws_eip_association" "nat" {
  instance_id   = aws_instance.nat.id
  allocation_id = aws_eip.nat.id
}

resource "aws_route" "private_nat" {
  route_table_id         = aws_route_table.private.id
  destination_cidr_block = "0.0.0.0/0"
  network_interface_id   = aws_instance.nat.primary_network_interface_id
}
