#!/usr/bin/env bash
set -euo pipefail

LAN_IFACE="${LAN_IFACE:-br0}"
HTTP_PORT="${HTTP_PORT:-80}"
HTTPS_PORT="${HTTPS_PORT:-443}"
NOMACHINE_PORT="${NOMACHINE_PORT:-4000}"
NOMACHINE_UDP_PORT_RANGE="${NOMACHINE_UDP_PORT_RANGE:-50000:50999}"
LAN_SUBNET="${LAN_SUBNET:-192.168.178.0/24}"

cat <<EOF
Applying host firewall policy with UFW:
  default allow outgoing
  default deny incoming
  LAN interface: ${LAN_IFACE}
  allowed inbound TCP ports on ${LAN_IFACE}: ${HTTP_PORT}, ${HTTPS_PORT}, ${NOMACHINE_PORT}
  allowed inbound UDP ports on ${LAN_IFACE} from ${LAN_SUBNET}: ${NOMACHINE_UDP_PORT_RANGE}

Interface note:
  - br0 is the host LAN-facing bridge.
  - tap0 is the VM TAP attached to br0 and is not opened for host services.
EOF

sudo ufw --force reset
sudo ufw default allow outgoing
sudo ufw default deny incoming
sudo ufw default deny routed

sudo ufw allow in on "${LAN_IFACE}" proto tcp to any port "${HTTP_PORT}" comment 'HTTP on LAN bridge'
sudo ufw allow in on "${LAN_IFACE}" proto tcp to any port "${HTTPS_PORT}" comment 'HTTPS on LAN bridge'
sudo ufw allow in on "${LAN_IFACE}" proto tcp to any port "${NOMACHINE_PORT}" comment 'NoMachine on LAN bridge'
sudo ufw allow in on "${LAN_IFACE}" proto udp from "${LAN_SUBNET}" to any port "${NOMACHINE_UDP_PORT_RANGE}" comment 'NoMachine UDP on LAN bridge'

sudo ufw --force enable
sudo ufw status verbose
