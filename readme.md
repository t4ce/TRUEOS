# Steps Fresh Sys
# CARGO_BUILD_TARGET=/home/t4ce/Dokumente/TrueOS/86_64.json cargo outdated -R
# cargo upgrade
# clone repo , then
git submodule init 
git submodule update
sudo apt update 
sudo apt install -y rustup
sudo apt install autoconf automake mtools nasm xorriso
sudo apt-get install qemu-system

sudo apt install gdb

check disc files after install
// mdir -i disk.img@@$((2048*512)) ::

# good luck with this one

# PASS IN USB DEVICE
sudo install -m 0644 99-trueos-usb.rules /etc/udev/rules.d/99-trueos-usb.rules
sudo udevadm control --reload-rules && sudo udevadm trigger -s usb --action=add

NOTE: The "RUN+=...unbind" lines in 99-trueos-usb.rules will intentionally unbind the host driver.
That can make hubs show up as Driver=[none]/0p in lsusb -t (host won't enumerate devices behind them).

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

# If `disk.img` is a partitioned MBR image, the FAT partition typically starts at LBA 2048.
# If it's a FAT "superfloppy", the filesystem starts at offset 0.
# Auto-detect the correct offset (bytes):

# Verify:
# mdir -i disk.img@@$((2048*512)) ::

rm -f disk.img && truncate -s 1G disk.img
mformat -i disk.img -F -v TRUEOS ::
mmd -i disk.img ::/qjs
mcopy -o -s -i disk.img crates/trueos-qjs/app/* ::/qjs/
mdir -i disk.img
mdir -i disk.img ::/qjs

or

mmd -i disk.img@@$((2048*512)) ::/qjs
mcopy -o -s -i disk.img@@$((2048*512)) crates/trueos-qjs/app/* ::/qjs/
mdir -i disk.img@@$((2048*512)) 
mdir -i disk.img@@$((2048*512)) ::/qjs

qjsm @/qjs/main.mjs