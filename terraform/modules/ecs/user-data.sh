#!/bin/bash
echo "Starting ECS instance initialization with SSM support..."

# 设置 ECS 配置
cat << EOF >> /etc/ecs/ecs.config
ECS_CLUSTER=${cluster_name}
ECS_ENABLE_TASK_IAM_ROLE=true
ECS_AVAILABLE_LOGGING_DRIVERS=["json-file","awslogs"]
ECS_LOGLEVEL=info
AWS_DEFAULT_REGION=${region}
EOF

# 安装和启动 SSM Agent（Amazon Linux 2 通常已预装，但确保其运行）
echo "Configuring SSM Agent..."

# 检查 SSM Agent 是否已安装
if command -v amazon-ssm-agent &> /dev/null; then
    echo "SSM Agent is already installed"
else
    echo "Installing SSM Agent..."
    sudo yum install -y https://s3.${region}.amazonaws.com/amazon-ssm-${region}/latest/linux_amd64/amazon-ssm-agent.rpm
fi

# 确保 SSM Agent 服务已启用并启动
sudo systemctl enable amazon-ssm-agent
sudo systemctl start amazon-ssm-agent

# 检查 SSM Agent 状态
echo "SSM Agent status:"
sudo systemctl status amazon-ssm-agent

# 安装 ECS 代理启动后钩子以确保 SSM 在 ECS 代理之前运行
mkdir -p /etc/ecs/ecs.config.d/
cat << 'EOF' > /etc/ecs/ecs.config.d/ssm-setup.sh
#!/bin/bash
# 确保 SSM Agent 在 ECS 代理之前运行
systemctl is-active --quiet amazon-ssm-agent || systemctl start amazon-ssm-agent
EOF

chmod +x /etc/ecs/ecs.config.d/ssm-setup.sh

# 重启 ECS 代理以确保配置生效
echo "Restarting ECS agent..."
systemctl try-restart ecs --no-block

# 等待服务启动
sleep 10

# 验证服务状态
echo "Final service status:"
echo "ECS Agent: $(systemctl is-active ecs)"
echo "SSM Agent: $(systemctl is-active amazon-ssm-agent)"

echo "Instance initialization completed successfully."