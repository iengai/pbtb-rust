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


data "aws_ami" "amazon_linux_2023" {
  most_recent = true
  owners      = ["amazon"]

  filter {
    name   = "name"
    values = ["al2023-ami-*-kernel-6.1-arm64"]
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
  ami                         = data.aws_ami.amazon_linux_2023.id
  instance_type               = "t4g.nano"
  subnet_id                   = aws_subnet.public[0].id
  vpc_security_group_ids      = [aws_security_group.nat_sg.id]
  associate_public_ip_address = true
  source_dest_check           = false

  user_data = file("../../modules/network/nat-userdata-al2023.sh")

  tags = {
    Name = "nat-instance"
  }

  depends_on = [aws_internet_gateway.main]
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

resource "aws_eip_association" "nat" {
  allocation_id = aws_eip.nat.id
  instance_id   = aws_instance.nat.id
}

resource "aws_route" "private_nat" {
  route_table_id         = aws_route_table.private.id
  destination_cidr_block = "0.0.0.0/0"
  network_interface_id   = aws_instance.nat.primary_network_interface_id
  depends_on = [aws_instance.nat]
}

resource "aws_instance" "ecs-test" {
  ami                    = data.aws_ami.amazon_linux_2023.id
  instance_type          = "t4g.nano"
  count = length(aws_subnet.private)
  subnet_id              = aws_subnet.private[count.index].id
  # subnet_id = aws_subnet.private.id
  vpc_security_group_ids = [aws_security_group.app_sg.id]
  associate_public_ip_address = false
  tags = { Name = "ecs-test" }
}
