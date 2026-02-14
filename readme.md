/* TRUE OS § ® 2026
██████████████████████████████████████████████████████████████████████
██░        ░░       ░░░  ░░░░  ░░        ░░░░░░░░░      ░░░░      ░░██
██▒▒▒▒  ▒▒▒▒▒  ▒▒▒▒  ▒▒  ▒▒▒▒  ▒▒  ▒▒▒▒▒▒▒▒▒▒▒▒▒▒  ▒▒▒▒  ▒▒  ▒▒▒▒▒▒▒██
██▓▓▓▓  ▓▓▓▓▓       ▓▓▓  ▓▓▓▓  ▓▓      ▓▓▓▓▓▓▓▓▓▓  ▓▓▓▓  ▓▓▓      ▓▓██
██████  █████  ███  ███  ████  ██  ██████████████  ████  ████████  ███
██████  █████  ████  ███      ███        █████████      ████      ████
██████████████████████████████████████████████████████████████████████
A Rust Based 64 Bit Paged X84 Baremetal OS Targeted at Intel and GOWIN

Think of rust as the world’s quiet, slow-moving “entropy tax”:
A constant drain of resources, money, and safety.

Think of TRUE OS as the world’s fast-moving “entropy dividend”:
A constant influx of resources, money, and safety.
*/


# Steps Fresh Sys, clone repo , then
sudo apt update 
sudo apt install -y rustup
sudo apt install autoconf automake mtools nasm xorriso
sudo apt-get install qemu-system
sudo apt install gdb
cargo install cargo-outdated 
cargo install cargo-edit --locked
cargo outdated -R
cargo upgrade
cargo update

# and so nomachione with pxe work:
sudoedit /usr/NX/etc/server.cfg
# Add:
UDPPort 50000-50999
# Restart NoMachine:
sudo systemctl restart nxserver

konsole -e sh -c 'stty -echo -icanon cols 200 rows 60; nc 192.168.178.78 4245; stty sane'

PXE_IF=br0
sudo ufw allow in on "$PXE_IF" proto udp from 192.168.178.0/24 to any port 67
sudo ufw allow in on "$PXE_IF" proto udp from 192.168.178.0/24 to any port 4011
sudo ufw allow in on "$PXE_IF" proto udp from 192.168.178.0/24 to any port 69
sudo ufw allow in on "$PXE_IF" proto udp from 192.168.178.0/24 to any port 1024:65535
sudo node pxe2.js --iface "$PXE_IF" --verbose

/*
ConPink 	FF_55_FF 
ConBlue 	08_18_30
ConWhite 	FF_FF_FF
*/

# PASS IN USB DEVICE / NVMe data partition permissions
sudo install -m 0644 99-trueos-usb.rules /etc/udev/rules.d/99-trueos-usb.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=block --subsystem-match=usb
sudo udevadm trigger --name-match=nvme2n1p1
ls -l /dev/nvme2n1p1

## LAN bridge for QEMU (rerunnable)
UPLINK=enp5s0
WIRED_CON="Kabelgebundene Verbindung 1"
BR=br0
TAP=tap0
SLAVE_CON="$BR-$UPLINK"   # -> br0-enp5s0
nmcli -t -f NAME con show | grep -Fxq "$BR" \
  || sudo nmcli con add type bridge ifname "$BR" con-name "$BR" ipv4.method auto ipv6.method ignore
sudo nmcli con mod "$BR" bridge.stp no bridge.forward-delay 0
nmcli -t -f NAME con show | grep -Fxq "$SLAVE_CON" \
  || sudo nmcli con add type bridge-slave ifname "$UPLINK" con-name "$SLAVE_CON" master "$BR"
sudo nmcli con mod "$WIRED_CON" connection.autoconnect no 2>/dev/null || true
sudo nmcli con down "$WIRED_CON" 2>/dev/null || true
sudo nmcli con up "$SLAVE_CON"
sudo nmcli con up "$BR"
sudo nmcli con delete "$TAP" 2>/dev/null || true
if ! ip link show "$TAP" >/dev/null 2>&1; then
  sudo ip tuntap add dev "$TAP" mode tap user "$USER" group "$USER"
fi
sudo nmcli dev set "$TAP" managed no 2>/dev/null || true
sudo ip link set "$TAP" master "$BR"
sudo ip link set "$TAP" up
bridge link show
ip -4 -br addr show "$BR" "$UPLINK" "$TAP" 2>/dev/null || true
ip -4 route show default
ip -4 -br addr show | egrep "^($BR|$UPLINK|$TAP)\\b" || true

# VFIO USB CONTROLLER (no persist across reboot)
sudo modprobe vfio-pci
echo 0000:06:00.0 | sudo tee /sys/bus/pci/devices/0000:06:00.0/driver/unbind
echo vfio-pci | sudo tee /sys/bus/pci/devices/0000:06:00.0/driver_override
echo 0000:06:00.0 | sudo tee /sys/bus/pci/drivers/vfio-pci/bind
sudo bash -lc '
modprobe vfio vfio_pci vfio_iommu_type1

for dev in 0000:06:00.0 0000:06:00.1; do
  [ -e /sys/bus/pci/devices/$dev ] || continue
  echo vfio-pci > /sys/bus/pci/devices/$dev/driver_override
  echo $dev > /sys/bus/pci/drivers_probe
done

echo -n "IOMMU group: "
readlink -f /sys/bus/pci/devices/0000:06:00.0/iommu_group || true
ls -l /dev/vfio || true
lspci -nnk -s 06:00.0
'


# dummy (no persist across reboot)
sudo ip link add NIC type dummy
sudo ip link set dev NIC address 5c:60:ba:b5:58:0f