# Native UEFI boot test

This experimental Rust loader starts the existing TrueOS kernel directly on
x86-64 UEFI hardware. It does not load Limine or include Blueprint build-ins.

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
path. The loader uses the volume it was launched from and reads `\TRUEOS.elf`.

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

**This is currently file delivery, not a complete native PXE boot.** Firmware
can fetch the EFI loader, but the loader opens the launch device's UEFI Simple
File System to read the kernel and GPU firmware. Ordinary PXE does not supply
that filesystem. The loader needs a network download path before it can boot
these files end to end over PXE. Use the FAT USB method for the current loader.

## First hardware test

The loader prints milestones to the GOP framebuffer and COM1, including the
kernel image size, discovered logical CPU count, firmware exit, successfully
started CPUs, and entry into the kernel. P cores, E cores and SMT threads are
enumerated through ACPI MADT and identified by their APIC IDs.

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
RTC conversion and CPU enumeration. Compilation and host tests do not establish
that firmware handoff or processor startup works on physical hardware.

After preparing the release files, run those checks with:

```sh
tools/trueos-boot/test.sh
```
