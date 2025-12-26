sudo nmcli dev set enp5s0 managed no
sudo dhclient -r enp5s0 2>/dev/null || true
sudo ip addr flush dev enp5s0
sudo ip addr add 192.168.55.1/24 dev enp5s0
sudo pkill dnsmasq
sudo dnsmasq --no-daemon --port=0 --interface=enp5s0 --bind-interfaces \
  --dhcp-range=192.168.55.50,192.168.55.150,255.255.255.0,12h \
  --dhcp-authoritative \
  --dhcp-option=option:router,192.168.55.1 \
  --dhcp-option=option:tftp-server,192.168.55.1 \
  --enable-tftp --tftp-root=/home/t4ce/Dokumente/Repos/FalseOS/bld/isofiles \
  --dhcp-boot=BOOTX64.EFI \
  --pxe-service=BC_EFI,"FalseOS UEFI PXE",BOOTX64.EFI \
  --log-dhcp --dhcp-leasefile=/tmp/pxe.leases

#wieder wie vorher:
sudo nmcli dev set enp5s0 managed yes
sudo nmcli dev set wlo1 managed yes
sudo ip addr flush dev enp5s0