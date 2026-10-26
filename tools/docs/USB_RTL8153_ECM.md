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

xHCI interface endpoint lookup uses only the selected configuration. The vendor
and ECM configurations can reuse interface/alternate numbers; searching all
configurations can configure ECM pipes with the vendor layout. The regression
check is `python3 tools/testpy/test_usb_active_configuration.py`. Physical
validation of this correction on the laptop remains pending.

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

## Laptop display diagnostics

The retained text surface now shows a live Yes/No IP-assignment verdict with
one USB stage underneath. Yes only means the laptop profile address was assigned;
it does not prove carrier, packet exchange, or an SSH listener. No remains live
and can change after 30 seconds. Stage publication is synchronous, including
before the background display task starts. Controller discovery cannot replace
NIC progress once an RTL8153 has been found.

On the next laptop boot, record the final USB line:

- `USB service not started`: the USB service has not reached its entry point.
- `xHCI: ...`: controller admission, mapping, initialization, or discovery.
- `USB discovered: waiting for RTL8153`: enumeration ran, but the adapter has
  not reached the RTL8153 dispatcher.
- `RTL8153: ...` / `ECM: ...`: adapter open or the named configuration operation.
- `ECM ready: waiting for carrier`: network registration succeeded; no carrier
  notification has been observed yet.
- `USB Ethernet: carrier up`: carrier notification arrived. Check the IP verdict,
  then test ARP/ping and SSH at the configured laptop address `192.168.178.50`.
- `USB Ethernet: transport failed`: a pipe failed; detailed errors remain in logs.

This instrumentation has host coverage and a successful kernel cargo check;
it has not been validated on the laptop. It does not fix or establish the cause
of the current USB failure. Keep UI4, sprite animation, and Wi-Fi work deferred
until the USB NIC can carry a working SSH session.
