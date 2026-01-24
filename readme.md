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
qemu-img create -f raw disk.img 1G
sudo apt install gdb

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

check disc files after install
// mdir -i disk.img@@$((2048*512)) ::

## FIREWALL netboot auf interface alles erlauben ##
sudo ufw allow in on enx047bcb669593
sudo ufw allow out on enx047bcb669593

sudo ufw allow in on enx047bcb669593 to any port 80 proto tcp
# sudo ufw allow in on enx047bcb669593 to any port 67 proto udp
# sudo ufw allow in on enx047bcb669593 to any port 80 proto tcp
sudo ip addr add 192.168.55.1/24 dev enx047bcb669593
sudo ip addr flush dev enx047bcb669593
