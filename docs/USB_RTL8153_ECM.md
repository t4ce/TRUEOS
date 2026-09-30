# RTL8153 USB Ethernet bring-up

The USB dispatcher claims only `0bda:8153`. Its dedicated
`usb_rtl8153_worker_task` owns the device for its lifetime and selects the CDC ECM
configuration from descriptors. Configuration 1's vendor protocol is not used.
The CDC Union descriptor determines the control/data pairing; the Ethernet
functional descriptor supplies the factory MAC string index. Missing or malformed
ECM descriptors stop initialization with a log message.

The worker activates the data alternate setting, enables directed/broadcast/all
multicast reception, and connects bulk RX/TX to bounded Ethernet queues. A regular
`NetDevice` exposes those queues to the existing network service and vnet. Link
state comes from CDC notifications; USB bus speed is not Ethernet link speed.
The existing primary stays selected while it has a link. A newly linked USB NIC
can replace a primary that is down. Explicit NIC selection accepts `@0bda:8153`.

## Next boot

- `usb-net: ready ctrl=0 slot=... vid=0bda pid=8153 net=... cfg=2 ...` means
  configuration, MAC, interfaces, endpoints, and network registration succeeded.
- `usb-net: net=... link=up` means the device reported Ethernet carrier.
- Inspect the regular network interface list for `Realtek RTL8153 USB ECM`, its
  factory MAC, DHCP address, and packet counters. Verify ARP/DHCP and a vnet TCP
  connection using that NIC.
- Initialization and transport failures are logged as `usb-net:` with the failing
  stage or pipe. A transport failure marks the NIC down and stops accepting TX.

This is a boot-time ECM path with a 1500-byte Ethernet payload limit. Automatic
recovery/rebinding after unplug or transport failure is not implemented. Pending
USB transfers retain their DMA buffers and device owner until completion; they
are not cancelled by dropping timed-out futures. Duplex is not queried through
Realtek vendor registers.

Validation: kernel `cargo check -p TRUEOS`; host tests exercise ECM selection,
truncated descriptors, union pairing, MAC validation, queue bounds, link gating,
and frame preservation. Physical link/traffic validation remains pending.

Protocol reference: Linux's [RTL8153 ECM driver](https://github.com/torvalds/linux/blob/master/drivers/net/usb/r8153_ecm.c)
uses the [CDC Ethernet class binding](https://github.com/torvalds/linux/blob/master/drivers/net/usb/cdc_ether.c).
