#!/bin/bash
set -euxo pipefail

##############################################
# 1) Write the actual NAT setup script to EC2
##############################################
cat >/usr/local/bin/setup-nat.sh <<'EOF'
#!/bin/bash
set -euxo pipefail

# Enable IPv4 forwarding (idempotent)
if ! grep -q '^net.ipv4.ip_forward' /etc/sysctl.conf; then
  echo 'net.ipv4.ip_forward = 1' >> /etc/sysctl.conf
else
  sed -i 's/^net.ipv4.ip_forward.*/net.ipv4.ip_forward = 1/' /etc/sysctl.conf
fi
sysctl -p

# Detect outbound network interface (usually eth0)
WAN_IF=$(ip route | awk '/^default/ {print $5; exit}')

# Install iptables-services if available (best effort)
yum -y install iptables-services || true
systemctl enable iptables || true
systemctl start iptables || true

# --- Fast path: if NAT already correctly configured, do nothing ---

# 1) Check if IP forwarding is enabled
if sysctl net.ipv4.ip_forward | grep -q ' = 1'; then
  # 2) Check if MASQUERADE rule for this interface already exists
  if iptables -t nat -C POSTROUTING -o "${WAN_IF}" -j MASQUERADE 2>/dev/null; then
    echo "NAT already configured (ip_forward=1 and MASQUERADE on ${WAN_IF}). Nothing to do."
    exit 0
  fi
fi

echo "NAT not fully configured yet. Applying iptables rules..."

# Clear NAT and FORWARD rules (idempotent, but be cautious if you ever add other rules)
iptables -t nat -F
iptables -F FORWARD

# Allow forwarding
iptables -P FORWARD ACCEPT
iptables -A FORWARD -m state --state RELATED,ESTABLISHED -j ACCEPT
iptables -A FORWARD -i "${WAN_IF}" -o "${WAN_IF}" -j ACCEPT

# NAT: MASQUERADE all outbound traffic through this interface
iptables -t nat -A POSTROUTING -o "${WAN_IF}" -j MASQUERADE

EOF

chmod +x /usr/local/bin/setup-nat.sh


#########################################################
# 2) Create a systemd service so it runs on every reboot
#########################################################
cat >/etc/systemd/system/nat-setup.service <<'EOF'
[Unit]
Description=Configure NAT iptables rules
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
ExecStart=/usr/local/bin/setup-nat.sh
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
EOF


#########################################
# 3) Enable service and run it immediately
#########################################
systemctl daemon-reload
systemctl enable nat-setup.service
systemctl start nat-setup.service


#########################################################
# 4) Turn off the ECS agent this AMI ships enabled
#########################################################
# The AMI is the ECS-optimized AL2023 arm64 image, picked for its ready Docker
# setup, but this box is a NAT/telebot host and never receives an ECS task. The
# agent it starts by default registers into the `default` cluster, the instance
# role has no ecs:RegisterContainerInstance, and systemd restarts it forever on
# that terminal error -- a permanent crash loop holding memory on a t4g.micro
# and burying real container failures in `docker ps -a`.
systemctl disable --now ecs || true
