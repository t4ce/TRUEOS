https://www.trueos.eu

sudo ufw allow in on enx047bcb669593 to any port 80 proto tcp
# sudo ufw allow in on enx047bcb669593 to any port 67 proto udp
# sudo ufw allow in on enx047bcb669593 to any port 80 proto tcp
sudo ip addr add 192.168.55.1/24 dev enx047bcb669593
sudo ip addr flush dev enx047bcb669593

# PASS IN USB DEVICE
sudo install -m 0644 99-trueos-usb.rules /etc/udev/rules.d/99-trueos-usb.rules
sudo udevadm control --reload-rules && sudo udevadm trigger -s usb --action=add

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