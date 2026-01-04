#!/system/bin/sh
# Android root PXE helper (UEFI x86_64)
# - Sets static IP 192.168.55.1/24 on USB-Ethernet
# - Runs dnsmasq for DHCP + TFTP serving TrueOS bld/
#
# Requirements on phone:
# - root (su)
# - dnsmasq binary available (e.g. via Termux pkg, or your own build)
# - TrueOS bld/ copied to phone (default: /data/local/tmp/trueos-bld)
#
# Usage:
#   su -c '/data/local/tmp/android_netboot_pxe.sh start'
#   su -c '/data/local/tmp/android_netboot_pxe.sh stop'
# Optional env:
#   IFACE=eth0 TFTP_ROOT=/data/local/tmp/trueos-bld su -c '... start'

set -eu

IP_ADDR="192.168.55.1/24"
GW_ADDR="192.168.55.1"
RANGE_START="192.168.55.50"
RANGE_END="192.168.55.150"
LEASE_TIME="12h"

LEASE_FILE="${LEASE_FILE:-/data/local/tmp/pxe.leases}"
PID_FILE="${PID_FILE:-/data/local/tmp/dnsmasq-pxe.pid}"

is_ethernet_iface() {
  iface="$1"
  [ -d "/sys/class/net/$iface" ] || return 1
  # type 1 == ARPHRD_ETHER
  [ "$(cat "/sys/class/net/$iface/type" 2>/dev/null || echo 0)" = "1" ] || return 1
  case "$iface" in
    lo*|wlan*|rmnet*|p2p*|ap*|dummy*|ifb*|sit*|ip6tnl*|tun*|tap*|vti*|gre*|bond*|br*|wpan*)
      return 1
      ;;
  esac
  return 0
}

autodetect_iface() {
  # Prefer connected Ethernet interfaces.
  for iface in $(ls /sys/class/net 2>/dev/null); do
    is_ethernet_iface "$iface" || continue
    carrier="$(cat "/sys/class/net/$iface/carrier" 2>/dev/null || echo 0)"
    [ "$carrier" = "1" ] || continue
    echo "$iface"
    return 0
  done
  # Fallback: first Ethernet-like interface.
  for iface in $(ls /sys/class/net 2>/dev/null); do
    is_ethernet_iface "$iface" || continue
    echo "$iface"
    return 0
  done
  return 1
}

need_file() {
  path="$1"
  if [ ! -f "$path" ]; then
    echo "Missing required file: $path" >&2
    exit 1
  fi
}

start_pxe() {
  IFACE="${IFACE:-}"
  if [ -z "$IFACE" ]; then
    IFACE="$(autodetect_iface)" || {
      echo "Could not autodetect Ethernet interface. Set IFACE=..." >&2
      exit 1
    }
  fi

  echo "[+] Using IFACE=$IFACE"
  echo "[+] Using TFTP_ROOT=$TFTP_ROOT"

  need_file "$TFTP_ROOT/EFI/BOOT/BOOTX64.EFI"

  # Bring interface up with static IP.
  ip addr flush dev "$IFACE" || true
  ip link set "$IFACE" up || true
  ip addr add "$IP_ADDR" dev "$IFACE"

  # Kill any previous instance we started.
  if [ -f "$PID_FILE" ]; then
    oldpid="$(cat "$PID_FILE" 2>/dev/null || true)"
    if [ -n "$oldpid" ]; then
      kill "$oldpid" 2>/dev/null || true
    fi
    rm -f "$PID_FILE" || true
  fi

  # Start dnsmasq in background.
  # Note: some Android SELinux policies may block binding low ports.
  dnsmasq --port=0 --interface="$IFACE" --bind-interfaces \
    --dhcp-range="$RANGE_START","$RANGE_END",255.255.255.0,"$LEASE_TIME" \
    --dhcp-authoritative \
    --dhcp-option=option:router,"$GW_ADDR" \
    --dhcp-option=option:tftp-server,"$GW_ADDR" \
    --enable-tftp --tftp-root="$TFTP_ROOT" \
    --dhcp-boot=EFI/BOOT/BOOTX64.EFI \
    --pxe-service=BC_EFI,"FalseOS UEFI PXE",EFI/BOOT/BOOTX64.EFI \
    --log-dhcp --dhcp-leasefile="$LEASE_FILE" \
    --pid-file="$PID_FILE" \
    &

  sleep 0.2
  if [ -f "$PID_FILE" ]; then
    echo "[+] dnsmasq started (pid $(cat "$PID_FILE"))"
  else
    echo "[!] dnsmasq did not write pidfile; check output / SELinux" >&2
  fi

  echo "[+] Ready: connect PC via USB-C Ethernet and PXE boot (UEFI)."
}

stop_pxe() {
  IFACE="${IFACE:-}"
  if [ -f "$PID_FILE" ]; then
    pid="$(cat "$PID_FILE" 2>/dev/null || true)"
    if [ -n "$pid" ]; then
      echo "[+] Stopping dnsmasq pid $pid"
      kill "$pid" 2>/dev/null || true
    fi
    rm -f "$PID_FILE" || true
  else
    # best-effort fallback
    pkill dnsmasq 2>/dev/null || true
  fi

  if [ -n "$IFACE" ]; then
    ip addr flush dev "$IFACE" 2>/dev/null || true
  fi

  echo "[+] Stopped"
}

cmd="${1:-}"
case "$cmd" in
  start) start_pxe ;;
  stop) stop_pxe ;;
  *)
    echo "Usage: $0 {start|stop}" >&2
    exit 2
    ;;
esac
