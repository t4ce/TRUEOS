# Steps Fresh Sys

## Kernel modules (no_std)

See docs/kernel-module-surface.md

# clone repo , then
sudo apt update 
sudo apt install -y rustup
sudo apt install autoconf automake mtools nasm xorriso
sudo apt-get install qemu-system
sudo apt install gdb

cargo install cargo-outdated 
cargo install cargo-edit --locked
#  Use `make iso` or pass `--target 86_64.json`.
cargo outdated -R
cargo upgrade
cargo update

konsole -e sh -c 'stty -echo -icanon cols 100 rows 100; nc 127.0.0.1 4245; stty sane'

check disc files after install
// mdir -i disk.img@@$((2048*512)) ::


# good luck with this one

# PASS IN USB DEVICE
sudo install -m 0644 99-trueos-usb.rules /etc/udev/rules.d/99-trueos-usb.rules
sudo udevadm control --reload-rules && sudo udevadm trigger -s usb --action=add

# VFIO USB CONTROLLER (persistent across reboot)
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
# unbind all
sudo modprobe vfio-pci
echo 0000:06:00.0 | sudo tee /sys/bus/pci/devices/0000:06:00.0/driver/unbind
echo vfio-pci | sudo tee /sys/bus/pci/devices/0000:06:00.0/driver_override
echo 0000:06:00.0 | sudo tee /sys/bus/pci/drivers/vfio-pci/bind
# 

## FIREWALL netboot auf interface alles erlauben ## enx047bcb669593
sudo ufw allow in on enx047bcb669593
sudo ufw allow out on enx047bcb669593
# go netboot
sysctl net.ipv4.ip_nonlocal_bind 2>/dev/null || true
sudo nmcli dev disconnect enx047bcb669593 || true
sudo nmcli dev set enx047bcb669593 managed no || true
sudo ip link set enx047bcb669593 up
sudo ip addr flush dev enx047bcb669593
sudo ip addr add 192.168.55.1/24 dev enx047bcb669593
sudo ip addr replace 192.168.55.1/24 dev enx047bcb669593
ip -4 -br addr show dev enx047bcb669593
sudo node pxe.js 

/*
ConPink 	FF_55_FF 
ConBlue 	08_18_30
ConWhite 	FF_FF_FF
*/

## QuickJS filesystem modules (/qjs)

# Verify:
# mdir -i disk.img@@$((2048*512)) ::

mformat -i disk.img -F -v TRUEOS ::
mmd -i disk.img ::/qjs
# NOTE: `/qjs/cdn` is created automatically on first URL import (cache write).
# mmd -i disk.img ::/qjs/cdn
mcopy -o -s -i disk.img crates/trueos-qjs/app/* ::/qjs/
mdir -i disk.img
mdir -i disk.img ::/qjs

or

mmd -i disk.img@@$((2048*512)) ::/qjs
# NOTE: `/qjs/cdn` is created automatically on first URL import (cache write).
# mmd -i disk.img@@$((2048*512)) ::/qjs/cdn
mcopy -o -s -i disk.img@@$((2048*512)) crates/trueos-qjs/app/* ::/qjs/
mdir -i disk.img@@$((2048*512)) 
mdir -i disk.img@@$((2048*512)) ::/qjs
mdir -i disk.img@@$((2048*512)) ::/qjs/cdn


qjs @/qjs/main.mjs