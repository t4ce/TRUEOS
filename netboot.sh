sudo nmcli dev set enp5s0 managed no
sudo dhclient -r enp5s0 2>/dev/null || true
sudo ip addr flush dev enp5s0
sudo ip addr add 192.168.55.1/24 dev enp5s0

sudo dnsmasq --no-daemon --port=0 --interface=enp5s0 --bind-interfaces \
  --dhcp-range=192.168.55.50,192.168.55.150,255.255.255.0,12h \
  --dhcp-authoritative \
  --dhcp-option=option:router,192.168.55.1 \
  --dhcp-option=option:tftp-server,192.168.55.1 \
  --enable-tftp --tftp-root=/home/t4ce/Dokumente/TrueOS/bld \
  --dhcp-boot=EFI/BOOT/BOOTX64.EFI \
  --pxe-service=BC_EFI,"TRUEOS UEFI PXE",EFI/BOOT/BOOTX64.EFI \
  --log-dhcp --dhcp-leasefile=/tmp/pxe.leases

#wieder wie vorher:
sudo nmcli dev set enp5s0 managed yes
sudo nmcli dev set wlo1 managed yes
sudo ip addr flush dev enp5s0

# CARGO_BUILD_TARGET=/home/t4ce/Dokumente/TrueOS/86_64.json cargo outdated -R
# cargo upgrade


# full build from github
# 


___

# its when you cant pass in a usb dev
sudo install -m 0644 99-trueos-usb.rules /etc/udev/rules.d/99-trueos-usb.rules
sudo udevadm control --reload-rules && sudo udevadm trigger -s usb --action=add


#Steps Fresh Sys
# get vscode -> für debugger c/c++ extension
# get clone repo
git submodule init 
git submodule update
sudo apt update 
sudo apt install -y rustup
sudo apt install autoconf automake mtools nasm xorriso
sudo apt-get install qemu-system
qemu-img create -f raw disk.img 1G
sudo apt install gdb
