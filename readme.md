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


# install
sudo apt update && sudo apt upgrade
apt install git gh make rustup autoconf automake mtools nasm xorriso qemu-system gdb build-essential konsole
cargo install fmt cargo-outdated cargo-edit --locked
snap install code-insiders --classic
rustup component add clippy
rustup toolchain install nightly --profile minimal --component rust-src,rustfmt,rust-analyzer,llvm-tools-preview

git config --global user.email "jonasb@post.com"
git config --global user.name "t4ce"
gh auth login

apt install npm
sudo npm install node

# update
cargo outdated -R
cargo upgrade
cargo update
cargo clippy --fix --broken-code --bin "TRUEOS" -p TRUEOS

# and so nomachione with pxe work add to
sudoedit /usr/NX/etc/server.cfg
UDPPort 50000-50999

sudo systemctl restart nxserver

# host firewall baseline
# Current host layout:
# - br0 is the LAN-facing bridge and owns the host IPs.
# - enp5s0 is the physical uplink enslaved into br0.
# - tap0 is the VM TAP bridged into br0 and should not be used for host allow rules.
#
# Current NoMachine config/listener:
# - /usr/NX/etc/server.cfg -> Port 4000
# - TCP listener on 0.0.0.0:4000 and [::]:4000
#
# This applies:
# - allow all outgoing
# - block all incoming by default
# - allow inbound TCP 80, 443, and 4000 only on br0
chmod +x scripts/configure-host-firewall.sh
./scripts/configure-host-firewall.sh

# optional: reopen PXE-related UDP only when you are actively using PXE on br0
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

# PASS IN USB DEVICE / NVMe data partition / VFIO permissions
sudo install -m 0644 99-trueos-usb.rules /etc/udev/rules.d/99-trueos-usb.rules
sudo udevadm control --reload-rules

sudo usermod -aG kvm "$USER"
newgrp kvm
id

sudo udevadm trigger --subsystem-match=block --subsystem-match=usb --subsystem-match=vfio
sudo udevadm trigger --name-match=nvme2n1p1
ls -l /dev/nvme2n1p1
ls -l /dev/vfio || true

# Optional: keep router/DHCP seeing the *same* MAC as the physical uplink
# (otherwise br0 may present a different MAC than $UPLINK)
sudo nmcli con mod "$BR" 802-3-ethernet.cloned-mac-address "$(cat /sys/class/net/$UPLINK/address)"
sudo nmcli con down "$BR" 2>/dev/null || true
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

## LAN bridge for QEMU (rerunnable)
sudo nmcli con up br0-enp5s0
sudo nmcli con up br0
sudo ip link set tap0 master br0
sudo ip link set tap0 up
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
sudo nmcli con up br0

# persist TAP across reboot (one-time setup)
echo tun | sudo tee /etc/modules-load.d/tun.conf
if ! nmcli -t -f NAME con show | grep -Fxq "tap0"; then
  sudo nmcli con add type tun ifname tap0 con-name tap0 mode tap \
    owner "$(id -u)" group "$(id -g)" autoconnect yes \
    controller br0 port-type bridge
fi
if ! nmcli -t -f NAME con show --active | grep -Fxq "tap0"; then
  sudo nmcli con up tap0 || true
fi
nmcli -t -f NAME,TYPE,DEVICE con show | grep -E '^tap0:' || true
nmcli -f GENERAL.STATE,GENERAL.NAME con show tap0
bridge link show | grep -E "tap0|br0" || true


konsole -e sh -c 'stty -echo -icanon cols 200 rows 60; nc 192.168.178.94 4245; stty sane'






SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_device", ATTR{idVendor}=="2109", ATTR{idProduct}=="2813", MODE="0666", TAG+="uaccess"
SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_interface", ATTRS{idVendor}=="2109", ATTRS{idProduct}=="2813", RUN+="/bin/sh -c 'if [ -L /sys/bus/usb/devices/%k/driver ]; then echo %k > /sys/bus/usb/drivers/$(basename $(readlink -f /sys/bus/usb/devices/%k/driver))/unbind; fi'"
SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_device", ATTR{idVendor}=="0951", ATTR{idProduct}=="16a4", MODE="0666", TAG+="uaccess"
SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_interface", ATTRS{idVendor}=="0951", ATTRS{idProduct}=="16a4", RUN+="/bin/sh -c 'if [ -L /sys/bus/usb/devices/%k/driver ]; then echo %k > /sys/bus/usb/drivers/$(basename $(readlink -f /sys/bus/usb/devices/%k/driver))/unbind; fi'"
SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_device", ATTR{idVendor}=="303a", ATTR{idProduct}=="1001", MODE="0666", TAG+="uaccess"
SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_interface", ATTRS{idVendor}=="303a", ATTRS{idProduct}=="1001", RUN+="/bin/sh -c 'if [ -L /sys/bus/usb/devices/%k/driver ]; then echo %k > /sys/bus/usb/drivers/$(basename $(readlink -f /sys/bus/usb/devices/%k/driver))/unbind; fi'"

SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_device", ATTR{idVendor}=="058f", ATTR{idProduct}=="6387", MODE="0666", TAG+="uaccess"
SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_interface", ATTRS{idVendor}=="058f", ATTRS{idProduct}=="6387", RUN+="/bin/sh -c 'if [ -L /sys/bus/usb/devices/%k/driver ]; then echo %k > /sys/bus/usb/drivers/$(basename $(readlink -f /sys/bus/usb/devices/%k/driver))/unbind; fi'"

SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_device", ATTR{idVendor}=="07cf", ATTR{idProduct}=="6803", MODE="0666", TAG+="uaccess"
SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_interface", ATTRS{idVendor}=="07cf", ATTRS{idProduct}=="6803", RUN+="/bin/sh -c 'if [ -L /sys/bus/usb/devices/%k/driver ]; then echo %k > /sys/bus/usb/drivers/$(basename $(readlink -f /sys/bus/usb/devices/%k/driver))/unbind; fi'"

SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_device", ATTR{idVendor}=="1462", ATTR{idProduct}=="7e03", MODE="0666", TAG+="uaccess"
SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_interface", ATTRS{idVendor}=="1462", ATTRS{idProduct}=="7e03", RUN+="/bin/sh -c 'if [ -L /sys/bus/usb/devices/%k/driver ]; then echo %k > /sys/bus/usb/drivers/$(basename $(readlink -f /sys/bus/usb/devices/%k/driver))/unbind; fi'"


























sudo modprobe vfio
sudo modprobe vfio-pci
sudo modprobe vfio_iommu_type1

ls /sys/bus/pci/drivers | grep vfio


echo 0000:00:02.0 | sudo tee /sys/bus/pci/devices/0000:00:02.0/driver/unbind
echo vfio-pci | sudo tee /sys/bus/pci/devices/0000:00:02.0/driver_override
echo 0000:00:02.0 | sudo tee /sys/bus/pci/drivers_probe

ls -l /dev/vfio
lspci -nnk -s 00:02.0



t4ce@PCJB:~/REPOS/TRUEOS$ echo 0000:00:02.0 | sudo tee /sys/bus/pci/drivers/vfio-pci/bind
ls -l /dev/vfio
lspci -nnk -s 00:02.0
tee: /sys/bus/pci/drivers/vfio-pci/bind: No such file or directory (os error 2)
0000:00:02.0
total 0
crw-rw-rw- 1 root root 10, 196 Mar 16 21:47 vfio
00:02.0 Display controller [0380]: Intel Corporation Raptor Lake-S GT1 [UHD Graphics 770] [8086:a780] (rev 04)
        DeviceName: Onboard - Video
        Subsystem: Micro-Star International Co., Ltd. [MSI] Device [1462:7e03]
        Kernel modules: i915, xe
t4ce@PCJB:~/REPOS/TRUEOS$ 


## rebnoot
sudo modprobe vfio-pci
sudo modprobe vfio_iommu_type1

echo vfio-pci | sudo tee /sys/bus/pci/devices/0000:00:02.0/driver_override
echo 0000:00:02.0 | sudo tee /sys/bus/pci/drivers_probe

ls -l /dev/vfio
lspci -nnk -s 00:02.0

echo 0000:00:02.0 | sudo tee /sys/bus/pci/devices/0000:00:02.0/driver/unbind
echo vfio-pci | sudo tee /sys/bus/pci/devices/0000:00:02.0/driver_override
echo 0000:00:02.0 | sudo tee /sys/bus/pci/drivers_probe

ls -l /dev/vfio
lspci -nnk -s 00:02.0



...



echo 0000:00:02.0 | sudo tee /sys/bus/pci/devices/0000:00:02.0/driver/unbind
echo vfio-pci | sudo tee /sys/bus/pci/devices/0000:00:02.0/driver_override
echo 0000:00:02.0 | sudo tee /sys/bus/pci/drivers_probe

ls -l /dev/vfio
lspci -nnk -s 00:02.0










Use this on your Ubuntu 25.10 + GRUB host:

sudo tee /etc/modprobe.d/trueos-vfio-intel.conf >/dev/null <<'EOF'
options vfio-pci ids=8086:a780
softdep i915 pre: vfio-pci
softdep xe pre: vfio-pci
EOF

sudo tee -a /etc/initramfs-tools/modules >/dev/null <<'EOF'
vfio
vfio_pci
vfio_iommu_type1
EOF
Then edit /etc/default/grub and change:

GRUB_CMDLINE_LINUX_DEFAULT="quiet splash intel_iommu=on iommu=pt vfio-pci.ids=8086:a780"
Then apply it:

sudo update-initramfs -u
sudo update-grub
sudo reboot
After reboot, verify:

lspci -nnk -s 00:02.0
ls -l /dev/vfio




# whipe nvme


Do this in this order:

Give the SSD back to the host NVMe driver
echo 0000:08:00.0 | sudo tee /sys/bus/pci/devices/0000:08:00.0/driver/unbind
echo nvme | sudo tee /sys/bus/pci/devices/0000:08:00.0/driver_override
echo 0000:08:00.0 | sudo tee /sys/bus/pci/drivers_probe
Confirm the block device is back
lspci -nnk -s 08:00.0
ls -l /dev/nvme*
Wipe it
Use this:

sudo wipefs -a /dev/nvme2n1
sudo sgdisk --zap-all /dev/nvme2n1
sudo dd if=/dev/zero of=/dev/nvme2n1 bs=1M count=64 status=progress
sudo dd if=/dev/zero of=/dev/nvme2n1 bs=1M seek=$(( $(sudo blockdev --getsz /dev/nvme2n1) / 2048 - 64 )) count=64 status=progress
sudo partprobe /dev/nvme2n1 || true
sudo wipefs /dev/nvme2n1
sudo sgdisk -p /dev/nvme2n1
What you want after that:

wipefs
shows no signatures

sgdisk -p
shows no valid GPT / empty disk

Then bind it back to VFIO:

echo 0000:08:00.0 | sudo tee /sys/bus/pci/devices/0000:08:00.0/driver/unbind
echo vfio-pci | sudo tee /sys/bus/pci/devices/0000:08:00.0/driver_override
echo 0000:08:00.0 | sudo tee /sys/bus/pci/drivers_probe
That is the first time in this thread we are actually operating on the real intended disk.
Do this exact reset:

echo 0000:08:00.0 | sudo tee /sys/bus/pci/devices/0000:08:00.0/driver/unbind
echo nvme | sudo tee /sys/bus/pci/devices/0000:08:00.0/driver_override
echo 0000:08:00.0 | sudo tee /sys/bus/pci/drivers_probe
sudo udevadm trigger --subsystem-match=block --action=add
sudo udevadm settle
ls -l /dev/nvme2*
If /dev/nvme2n1 is still missing, force a clean re-enumeration:

echo 0000:08:00.0 | sudo tee /sys/bus/pci/devices/0000:08:00.0/driver/unbind
echo vfio-pci | sudo tee /sys/bus/pci/devices/0000:08:00.0/driver_override
echo 0000:08:00.0 | sudo tee /sys/bus/pci/drivers_probe
echo 0000:08:00.0 | sudo tee /sys/bus/pci/devices/0000:08:00.0/driver/unbind
echo nvme | sudo tee /sys/bus/pci/devices/0000:08:00.0/driver_override
echo 0000:08:00.0 | sudo tee /sys/bus/pci/drivers_probe
sudo udevadm trigger --subsystem-match=block --action=add
sudo udevadm settle
ls -l /dev/nvme2*
Only when /dev/nvme2n1 actually exists, wipe it:

sudo wipefs -a /dev/nvme2n1
sudo sgdisk --zap-all /dev/nvme2n1
sudo blkdiscard -f /dev/nvme2n1
If you want, paste just the output of ls -l /dev/nvme2* after that reset.