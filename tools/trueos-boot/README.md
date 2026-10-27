# TRUEOS native UEFI loader

The TRUEOS Rust loader starts the kernel directly on x86-64 UEFI hardware.
It provides the kernel request/response handoff, loads firmware modules, and
starts secondary processors. The Rust `limine` crate supplies the existing
handoff ABI types shared by the kernel and this loader.

Prepare the release files:

```sh
bash tools/trueos-boot/prepare.sh --release
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

Preparation uses Cargo's normal parallelism. Set
`CARGO_BUILD_JOBS` when a particular build needs a different job count.
For an explicit resource-limited diagnostic run, prefix the preparation command
with `bash tools/build-safe.sh`; normal rig builds do not use that wrapper.

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

Both commands build all apps from the sibling `TRUEOS-Buildins/buildins.json`,
create and compress an app.db redb image containing their normal Blueprint
packages, embed that image in the kernel, build the release EFI loader, and
create `TrueOS.tar.lz4` from the prepared native boot files. They publish the
complete generation via `tgt/pxe-current` and invoke `tools/rig-reset.sh` once.
Build, archive, or staging failure prevents the reset. Concurrent rig commands
are rejected.
Ordinary `cargo build` does not reset the rig. These commands do not start,
stop, or restart PXE and do not require sudo.
The staged runtime ELF has debug sections removed with the pinned toolchain's
`rust-objcopy --strip-debug`; the original Cargo ELF retains its debug information.
The native loader keeps the runtime file below 4 GiB, so keeping host debug
sections out of that file preserves boot-time DMA memory. PXE and USB receive
the same stripped runtime image. A stripping failure prevents publication and reset.

The buildins belong to the kernel generation; they have no separate publishing
step. The small driver in `TRUEOS-Buildins/tools/build.py` uses the pinned Rust
toolchain, Python 3.11+, `patch`, `nm`, `readelf`, and the host `liblz4` library.
It keeps the TRUEOS std adaptations in a private sysroot and leaves the installed
toolchain unchanged. Set `TRUEOS_BUILDINS_SOURCE` to select another source checkout.
The Linux compiler is the one archived in
[`TRUEOS-Rust-Toolchain-nightly-2026-07-10`](https://github.com/t4ce/TRUEOS-Rust-Toolchain-nightly-2026-07-10).
The private sysroot retains that archive's Blueprint std port and applies the
kernel's current thread backend. The archive also contains sources for the
native `rustc-min` Blueprint compiler; this builtin workflow does not build or
execute that compiler.

Each app follows the Paper object-build flow: a normal Cargo build invokes the
app linker with `-r` to produce the tracked `.o` module. Dev app builds explicitly
use optimization level 0, disabled LTO, incremental compilation, and 256 codegen
units; the dev kernel also compiles unoptimized. The driver adapts the target
and std sources, explicitly strips debug information from the packaged module
with `objcopy`, and creates the existing Blueprint package and AppDB image from it.

Generated app sources, dependency locks, Cargo caches, linked objects, packages,
database images, and logs live in `tgt/buildins/dev/` or `tgt/buildins/release/`.
Dev and release caches stay separate. The driver invokes Cargo for every app on
each run; Cargo decides which Rust inputs need recompilation. Unchanged staged
sources retain their timestamps. Packaging runs when the Cargo `.o` content,
Blueprint flags, or package format changes, and validates the stripped module
and package hashes before reusing them. Unchanged packages retain their bytes
and timestamps, so an unchanged AppDB image can be reused too. A kernel
implementation change does not recompile apps unless their shared inputs
changed; ABI changes are checked against their imports.

`app.db` is a normal inspectable redb file with the existing `apps`, `hashes`, and
`updated` tables. `app.db.lz4` is its embedded form. The host helper and kernel pin
the same redb version. Boot decompresses the image and opens it in writable RAM;
it does not rebuild the tables. Downloads still update that RAM database until
reboot. If the app records and database builder are unchanged, both image files
retain their bytes and timestamps.

Each prepared generation contains `TrueOS.tar.lz4`, a standard LZ4 frame around
a USTAR archive with exactly these files:

```text
TRUEOS.elf
EFI/BOOT/BOOTX64.EFI
EFI/BOOT/tgl_guc_70.bin
```

The archive uses the same stripped kernel as PXE and USB, including its embedded
builtins AppDB. It contains no separate database or ISO. It is created inside
the immutable `tgt/rig-images/<generation>/` directory before publication; a
failure leaves the previous generation selected. Stable links select the
latest successful archive:

| Path | Selection |
| --- | --- |
| `tgt/rigdev.tar.lz4` | Latest dev generation |
| `tgt/rigreal.tar.lz4` | Latest release generation |
| `tgt/TrueOS.tar.lz4` | Current `tgt/pxe-current/TrueOS.tar.lz4` |

RAM live update extracts `TRUEOS.elf` from this native archive. Its online URL
is `https://trueos.eu/TrueOS.tar.lz4`; the LAN URL is
`http://192.168.178.111:8080/TrueOS.tar.lz4`. To serve the current archive from
the development host manually, run this from the repository root:

```sh
python3 -m http.server 8080 --bind 0.0.0.0 --directory tgt
```

The rig build does not start this HTTP server. `pxe3.js` continues to serve only
the three native boot files over TFTP. Online disk installation retains its
separate `https://trueos.eu/TrueOS.lz4` endpoint and older ISO-containing bundle.

The kernel build verifies that the prepared set matches the current sources and
contains every configured app. Missing, stale or failed app builds stop the flow
before the kernel is published or the rig is reset. A direct `cargo build`
consumes a verified prepared set. Refresh the complete dev generation without
resetting the rig with:

```sh
bash tools/trueos-boot/prepare.sh --dev
```

Run the PXE server separately using the instructions below. It automatically
loads each completed publication, including switches between dev and release.
The rig must be configured to boot via IPv4 UEFI PXE. A reset acknowledgement
confirms the packet was sent; it does not verify that the kernel finished booting
or that a PXE server is running.

## IPv4 PXE file server

After preparing the files at least once, start the minimal network server with Node.js,
`ip` (iproute2), and `dnsmasq` installed:

```sh
sudo node tools/netboot/pxe3.js
# Or select the LAN interface and optionally a different staging directory:
sudo node tools/netboot/pxe3.js enp3s0
```

This uses IPv4 ProxyDHCP alongside the LAN's existing DHCP server and dnsmasq's
TFTP server. It advertises `EFI/BOOT/BOOTX64.EFI` to x86-64 UEFI PXE clients
(architecture IDs 7 and 9). Only the three staged files above are copied to a
temporary TFTP root; no ISO, HTTP, DNS, IPv6, or address leases are served.
Stop with Ctrl-C. The temporary snapshot is removed when the server exits;
completed builds are automatically reloaded without restarting dnsmasq. The
default `tgt/pxe-current` follows the most recently prepared dev or release
build. An explicit artifact-directory argument watches only that directory.
Old transfer snapshots remain until the server exits. Use a LAN interface reachable
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
