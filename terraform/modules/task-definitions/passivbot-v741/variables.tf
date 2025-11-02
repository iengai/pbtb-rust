variable "project" {
  description = "Project name"
  type        = string
}

variable "env" {
  description = "Environment name"
  type        = string
}

variable "region" {
  description = "AWS region"
  type        = string
}

variable "common_tags" {
  description = "Common tags for all resources"
  type        = map(string)
  default     = {}
}

variable "execution_role_arn" {
  description = "ECS task execution role ARN"
  type        = string
}

variable "task_role_arn" {
  description = "ECS task role ARN"
  type        = string
}

variable "container_name" {
  description = "Container name"
  type        = string
  default     = "passivbot-v741-container"
}

variable "container_image" {
  description = "Container image URI"
  type        = string
  default     = "your-registry/passivbot:v7.4.1"
}

variable "cpu" {
  description = "CPU units for the task"
  type        = number
  default     = 256
}

variable "memory" {
  description = "Memory for the task (MB)"
  type        = number
  default     = 512
}

variable "log_retention_days" {
  description = "CloudWatch log retention in days"
  type        = number
  default     = 30
}

variable "port_mappings" {
  description = "Container port mappings"
  type = list(object({
    containerPort = number
    hostPort      = number
    protocol      = string
  }))
  default = [{
    containerPort = 8080
    hostPort      = 0
    protocol      = "tcp"
  }]
}

variable "health_check" {
  description = "Container health check configuration"
  type = object({
    command     = list(string)
    interval    = number
    timeout     = number
    retries     = number
    startPeriod = number
  })
  default = {
    command     = ["CMD-SHELL", "curl -f http://localhost:8080/health || exit 1"]
    interval    = 30
    timeout     = 5
    retries     = 3
    startPeriod = 60
  }
}