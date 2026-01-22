https://www.trueos.eu

sudo ufw allow in on enx047bcb669593 to any port 80 proto tcp
# sudo ufw allow in on enx047bcb669593 to any port 67 proto udp
# sudo ufw allow in on enx047bcb669593 to any port 80 proto tcp
sudo ip addr add 192.168.55.1/24 dev enx047bcb669593
sudo ip addr flush dev enx047bcb669593



#Steps Fresh Sys
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
# Goal: pass through a whole PCI USB controller (e.g. 0000:06:00.0) with a single QEMU flag:
#   -device vfio-pci,host=0000:06:00.0
#
# One-time setup:
# 1) Put your user in kvm group (needed for /dev/vfio/* access)
sudo usermod -aG kvm $USER
# Then log out/in (or reboot). Verify: groups | grep kvm
#
# 2) Install VFIO udev permissions rule (repo copy: 99-vfio-permissions.rules)
sudo install -m 0644 99-vfio-permissions.rules /etc/udev/rules.d/99-vfio-permissions.rules
sudo udevadm control --reload-rules && sudo udevadm trigger --subsystem-match=vfio
#
# 3) Auto-load VFIO modules at boot
printf '%s\n' vfio vfio-pci vfio_iommu_type1 | sudo tee /etc/modules-load.d/vfio.conf
#
# 4) Bind the controller to vfio-pci automatically (replace IDs if your controller differs)
# ASMedia ASM3241 is: 1b21:3241
echo 'options vfio-pci ids=1b21:3241' | sudo tee /etc/modprobe.d/vfio-pci.conf

# 5) Increase memlock limit (VFIO_MAP_DMA "Cannot allocate memory" fix)
# Your QEMU config uses 8000M RAM; VFIO typically needs memlock >= guest RAM.
sudo tee /etc/security/limits.d/99-trueos-vfio.conf >/dev/null <<'EOF'
@kvm soft memlock unlimited
@kvm hard memlock unlimited
EOF
# Log out/in (or reboot) for PAM limits to apply. Verify: ulimit -l

# On Debian/Ubuntu/Pop!_OS you usually also want this so it takes effect early:
sudo update-initramfs -u

# After reboot, verify:
#   lspci -k -s 06:00.0   (should say: Kernel driver in use: vfio-pci)
#   ls -l /dev/vfio       (should contain the group node e.g. /dev/vfio/21)


## help
# unbind all
sudo modprobe vfio-pci
echo 0000:06:00.0 | sudo tee /sys/bus/pci/devices/0000:06:00.0/driver/unbind
echo vfio-pci | sudo tee /sys/bus/pci/devices/0000:06:00.0/driver_override
echo 0000:06:00.0 | sudo tee /sys/bus/pci/drivers/vfio-pci/bind
# 
