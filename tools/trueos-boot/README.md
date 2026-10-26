# TRUEOS native UEFI loader

The TRUEOS Rust loader starts the kernel directly on x86-64 UEFI hardware.
It provides the kernel request/response handoff, loads firmware modules, and
starts secondary processors. The Rust `limine` crate supplies the existing
handoff ABI types shared by the kernel and this loader.

Prepare the release files:

```sh
tools/trueos-boot/prepare.sh --release
```

Copy the **contents** of `tgt/usb-release/` to the root of the FAT-formatted USB
boot partition. The resulting layout is:

```text
EFI/BOOT/BOOTX64.EFI
EFI/BOOT/tgl_guc_70.bin
TRUEOS.elf
```

Boot that USB stick through the machine's UEFI boot menu. Keep the previous
boot files available for comparison and recovery. The preparer writes only to
`tgt`; the actual USB copy is manual.

The preparer defaults to release. `--dev` uses the dev kernel and stages
`tgt/usb-debug/`.
`cargo boot` builds only the EFI loader; the kernel ELF keeps its normal Cargo
path. When launched from FAT, the loader reads `\TRUEOS.elf` from that volume.

## Build and reboot the rig

Install the repository's Cargo commands once per checkout:

```sh
tools/install-rig-commands.sh
```

The installer links `cargo-rigdev` and `cargo-rigreal` into `$CARGO_HOME/bin`
(default `~/.cargo/bin`), which must be on PATH. Then run:

```sh
cargo rigdev   # dev kernel
cargo rigreal  # release kernel
```

Both commands build the kernel and release EFI loader, stage all three boot
files, restart this checkout's `pxe3.js` server with the selected staging
directory, wait for dnsmasq's TFTP readiness message, and invoke
`tools/rig-reset.sh` once. The ESP32 receives one reset acknowledgement.
Build or PXE startup failure prevents the reset. Concurrent rig commands are
rejected. Ordinary `cargo build` does not reset the rig.

PXE runs as a system service and remains available after the command exits.
The command may prompt for sudo authentication after building. It requires
Node.js, dnsmasq, iproute2, systemd, Python 3, and `flock`. The default-route
interface is selected unless `TRUEOS_PXE_INTERFACE` is set, for example:

```sh
TRUEOS_PXE_INTERFACE=enp5s0 cargo rigdev
```

The printed `journalctl -u ...` command shows the PXE logs. Stop the service
with `sudo systemctl stop <printed-unit>.service`. A manually started
`pxe3.js` from this checkout is also stopped before replacement; unrelated
PXE servers are left running and may need to be stopped if their ports conflict.
The rig must be configured to boot via IPv4 UEFI PXE. A reset acknowledgement
confirms the packet was sent; it does not verify that the kernel finished booting.

## IPv4 PXE file server

After preparing the files, start the minimal network server with Node.js,
`ip` (iproute2), and `dnsmasq` installed:

```sh
sudo node tools/netboot/pxe3.js
# Or select the LAN interface and optionally a different staging directory:
sudo node tools/netboot/pxe3.js enp3s0 tgt/usb-release
```

This uses IPv4 ProxyDHCP alongside the LAN's existing DHCP server and dnsmasq's
TFTP server. It advertises `EFI/BOOT/BOOTX64.EFI` to x86-64 UEFI PXE clients
(architecture IDs 7 and 9). Only the three staged files above are copied to a
temporary TFTP root; no ISO, HTTP, DNS, IPv6, or address leases are served.
Stop with Ctrl-C. The temporary snapshot is removed when the server exits;
restart after rebuilding to serve updated files. Use a LAN interface reachable
by the client and allow DHCP/PXE UDP 67/4011, TFTP UDP 69, and TFTP transfer
traffic through the host firewall. Stop other PXE/TFTP servers using those ports.

Select **IPv4 UEFI PXE** in the test rig's boot menu. Firmware downloads
`EFI/BOOT/BOOTX64.EFI`; the Rust loader reuses that boot device's PXE Base Code
protocol to fetch `TRUEOS.elf`, the GuC firmware and its own installer-module
copy over TFTP. It prefers the PXE/ProxyDHCP reply's boot server over the LAN's
address-lease server. No Rust network stack, C compiler, ISO or disk image is
needed. The same prepared directory works for both PXE and the FAT USB copy.

All downloads finish before firmware boot services end. The loader stops the
firmware PXE session, retains the downloaded files for the kernel, and uses the
same ELF loading, CPU startup and handoff path as USB. This implementation uses
the firmware's active IPv4 PXE session; IPv6 PXE and HTTP Boot are not supported.
This path has booted the physical test rig through to the running kernel.

The loader selects the console's 2560×1440 RGB/BGR framebuffer when firmware
offers it, otherwise retaining a usable firmware mode. It captures EDID from
the same GOP device and copies it into retained boot responses. The kernel
uses that monitor data for physical font sizing, including Shell3 text and
the home button. When EDID is unavailable on an active display, fonts use
their native 6×11 pixel proportions: 12×22 cells at the default text scale,
with a proportional home glyph. This keeps the same line/quad R8 production;
the general millimetre APIs still require measured monitor dimensions.

## Hardware diagnostics

The loader prints milestones to the GOP framebuffer and COM1, including the
boot source, file downloads, kernel image size, discovered logical CPU count,
firmware exit, successfully started CPUs, and entry into the kernel. P cores,
E cores and SMT threads are enumerated through ACPI MADT and identified by
their APIC IDs.

The loader also retains a bounded transcript as a boot module. The kernel
replays it through LogOs between `native-loader-log: begin` and `end`, including
GOP/EDID probes from before and after mode selection. It remains available in
the System → TLB → dump output (`trueos/pci/tlb.txt`) after the log ring rolls
over. No extra PXE file is required.

Secondary processors receive INIT/SIPI, enter 64-bit mode with retained page
tables and 16 MiB stacks, and wait for the kernel's existing `bootstrap()` call.
Processors that fail startup are omitted from the kernel's CPU list. If SMP
preparation fails, a BSP-only response still reports the actual BSP APIC ID.

On BSP-only and one-AP systems, incoming Shell3 SSH/terminal work can use the
available executor. Native blocking/compute lanes keep their normal
restrictions. GUI Shell3 requests require a background font lane and are
unavailable in that reduced topology.

The first implementation uses four-level paging, keeps AP bootstrap allocations
below 4 GiB, supports at most 256 discovered logical CPUs, and retains firmware
boot-services data to preserve configuration-table pointers. The kernel has
its own CPU-slot limit. The HII capture extension is not supplied by this loader
yet; the existing FirmwareScout code is available for a later integration.

Host tests validate the real ELF, request layouts, page tables, memory ownership,
RTC conversion, CPU enumeration, PXE protocol calls, boot-server selection,
network error handling, display mode selection, EDID retention and physical
font sizing. Firmware and display changes still need testing on the rig.

After preparing the release files, run those checks with:

```sh
tools/trueos-boot/test.sh
```
