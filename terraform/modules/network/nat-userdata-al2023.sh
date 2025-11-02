#!/bin/bash
set -euxo pipefail

# 1) 开启内核转发
if ! grep -q '^net.ipv4.ip_forward' /etc/sysctl.conf; then
  echo 'net.ipv4.ip_forward = 1' >> /etc/sysctl.conf
else
  sed -i 's/^net.ipv4.ip_forward.*/net.ipv4.ip_forward = 1/' /etc/sysctl.conf
fi
sysctl -p

# 2) 计算外网接口（走默认路由的网卡）
WAN_IF=$(ip route | awk '/^default/ {print $5; exit}')

# 3) 安装 iptables 服务（AL2023 默认是 nft，iptables 会映射到 nft）
yum -y install iptables-services || true
systemctl enable iptables || true
systemctl start iptables || true

# 4) 清理旧规则（幂等）
iptables -t nat -F
iptables -F FORWARD

# 5) 放行转发 + 做源地址伪装（MASQUERADE）
iptables -A FORWARD -m state --state RELATED,ESTABLISHED -j ACCEPT
iptables -A FORWARD -i ${WAN_IF} -o ${WAN_IF} -j ACCEPT
iptables -t nat -A POSTROUTING -o ${WAN_IF} -j MASQUERADE

# 6) 持久化
service iptables save || true

# 7) 小健诊：打印关键状态
echo "=== ip_forward ==="
sysctl net.ipv4.ip_forward
echo "=== route ==="
ip route
echo "=== iptables nat ==="
iptables -t nat -S
echo "=== iptables FORWARD ==="
iptables -S FORWARD
