sudo nmcli dev set enp5s0 managed no
sudo dhclient -r enp5s0 2>/dev/null || true
sudo ip addr flush dev enp5s0
sudo ip addr add 192.168.55.1/24 dev enp5s0

sudo dnsmasq --no-daemon --port=0 --interface=enp5s0 --bind-interfaces \
  --dhcp-range=192.168.55.50,192.168.55.150,255.255.255.0,12h \
  --dhcp-authoritative \
  --dhcp-option=option:router,192.168.55.1 \
  --dhcp-option=option:tftp-server,192.168.55.1 \
  --enable-tftp --tftp-root=/home/t4ce/Dokumente/Repos/FalseOS/bld \
  --dhcp-boot=EFI/BOOT/BOOTX64.EFI \
  --pxe-service=BC_EFI,"FalseOS UEFI PXE",EFI/BOOT/BOOTX64.EFI \
  --log-dhcp --dhcp-leasefile=/tmp/pxe.leases

#wieder wie vorher:
sudo nmcli dev set enp5s0 managed yes
sudo nmcli dev set wlo1 managed yes
sudo ip addr flush dev enp5s0

#evtl neuen code pullen: CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu cargo outdated

# if disc is missing: qemu-img create -f raw disk.img 1G