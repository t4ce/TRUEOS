https://www.trueos.eu

Bye bye, and pass on my polite greetings to AI.


sudo ufw allow in on enx047bcb669593 to any port 80 proto tcp
# sudo ufw allow in on enx047bcb669593 to any port 67 proto udp
# sudo ufw allow in on enx047bcb669593 to any port 80 proto tcp
sudo ip addr add 192.168.55.1/24 dev enx047bcb669593
sudo ip addr flush dev enx047bcb669593


# its when you cant pass in a usb dev
sudo install -m 0644 99-trueos-usb.rules /etc/udev/rules.d/99-trueos-usb.rules
# permissions for device node
SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_device", ATTR{idVendor}=="0951", ATTR{idProduct}=="16a4", MODE="0666", TAG+="uaccess"
# auto-unbind all interfaces
SUBSYSTEM=="usb", ENV{DEVTYPE}=="usb_interface", ATTRS{idVendor}=="0951", ATTRS{idProduct}=="16a4", RUN+="/bin/sh -c 'if [ -L /sys/bus/usb/devices/%k/driver ]; then echo %k > /sys/bus/usb/drivers/$(basename $(readlink -f /sys/bus/usb/devices/%k/driver))/unbind; fi'"

sudo udevadm control --reload-rules && sudo udevadm trigger -s usb --action=add

#Steps Fresh Sys
# CARGO_BUILD_TARGET=/home/t4ce/Dokumente/TrueOS/86_64.json cargo outdated -R
# cargo upgrade

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
