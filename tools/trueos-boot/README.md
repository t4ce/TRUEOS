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

### Kernel only / no buildins

This is a regular build option:

```sh
bash tools/trueos-boot/prepare.sh --release --no-buildins
# Or: bash tools/trueos-boot/prepare.sh --dev --no-buildins
```

It skips building and embedding the bundled apps, including the file explorer,
app store, and text editor. The selected kernel capabilities remain available,
and the app database starts empty and writable. TRUEOS-Buildins is not needed
for this mode; the kernel's TRUEOS-Blueprints source dependencies and archived
toolchain still are. The cloud selector exposes the same choice as the regular
**Kernel only** preset or **None** in the bundled-app checklist. It combines with
subsystem deselection; see [the kernel-only guide](../cloudbuild/KERNEL_ONLY.md).

If `TRUEOS_CLOUDBUILD_SPEC` is set, that manifest determines the app-payload
choice. `--no-buildins` cannot override a manifest that includes buildins.

### Individual buildins

Each of the 12 existing buildins is independently selectable. For example,
keep the directory explorer and text editor with the normal kernel features:

```sh
bash tools/trueos-boot/prepare.sh --release --buildins termdir,edit
```

Use `--buildins all` for the normal app payload, or `--buildins none` for the
same payload as `--no-buildins`. A subset compiles and embeds only its selected
app packages; empty skips the entire app build. A supplied cloud manifest
must agree with an explicit local selection. See
[Individual buildins](../cloudbuild/BUILDINS.md) for the app list, cloud request
format, and first-boot behavior.

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

The installer links `cargo-rig` and `cargo-rigreal` into `$CARGO_HOME/bin`
(default `~/.cargo/bin`), which must be on PATH. Then run:

```sh
cargo rig   # dev kernel
cargo rigreal  # release kernel
```

All twelve configured builtins use the audited core/alloc target: Commander,
Img, OS, Backup, Termdir, Paper, Edit, Apps, Hull, Share, Trauma and SSH.
`cargo rigreal --native-only` verifies them in isolation. Each invocation creates a fresh
`tgt/native-only.XXXXXX/` directory containing separate builtin and kernel
targets, an AppDB for the exact selected apps, and a kernel link map. No prior
Cargo object or AppDB is reused in these targets. The default selection still
includes all twelve apps in `TRUEOS-Buildins/buildins.json`.

The preparation script combines this validation mode with an explicit app list:

```sh
bash tools/trueos-boot/prepare.sh --release --buildins termdir,edit --native-only
bash tools/trueos-boot/prepare.sh --release --no-buildins --native-only
```

`--native-only` validates the selected list; it never removes apps from a local
or supplied cloud manifest. With no buildins, it still creates a fresh kernel
target and link map while skipping all app work. Outside this isolated mode,
the preparer respects `CARGO_TARGET_DIR` for the kernel ELF.

The isolated generation is published and resets the rig through the usual
workflow. Its `native-only-build-root`, `buildins.json` and
`buildins-ready.json` record the exact inputs outside the three-member boot
archive. The driver rejects a selected builtin that requires std or imports a
symbol from the existing Unix classifier. A no-buildins generation records an
empty `buildins.json` and has no app readiness record. The kernel's libc/POSIX allocation,
descriptor, process, signal and pthread compatibility providers have been
removed, including their reexported C allocation aliases. Legacy Blueprint
imports fail prebind and cannot fall through to loader stubs. Freestanding
memory, string and math helpers remain available.

SSH uses the shared
Sunset source for incoming and outgoing sessions; the outgoing app adds its
key-file and curve features without enabling them for the incoming kernel.

By default, both commands build all apps from the sibling `TRUEOS-Buildins/buildins.json`,
create and compress an app.db redb image containing their normal Blueprint
packages, embed that image in the kernel, build the release EFI loader, and
create `TrueOS.tar.lz4` from the prepared native boot files. They publish the
complete generation via `tgt/pxe-current` and invoke `tools/rig-reset.sh` once.
Build, archive, or staging failure prevents the reset. Concurrent rig commands
are rejected.
Keep `TRUEOS-Blueprints` checked out beside `TRUEOS` and `TRUEOS-Buildins`;
the editor's `trueos-ide` path dependency requires its manifests even when
the optional IDE feature is disabled. The release workflow checks out all three.
Ordinary `cargo build` does not reset the rig. These commands do not start,
stop, or restart PXE and do not require sudo.
The dev kernel uses optimization level 1, generates no debug information, and
strips its symbol tables. Runtime logging, debug assertions, overflow checks,
and frame-pointer backtraces remain enabled. Incremental compilation remains
enabled for development builds.
The staged runtime ELF also passes through the pinned toolchain's
`rust-objcopy --strip-debug` to remove debug data from local profile overrides.
The native loader keeps that file below 4 GiB. PXE and USB receive the same
runtime image. A stripping failure prevents publication and reset.

The buildins belong to the kernel generation; they have no separate publishing
step. The selected-set adapter in `tools/cloudbuild/buildins.py` reuses the
app compiler and packager in `TRUEOS-Buildins/tools/build.py` and seeds AppDB
from the exact selected manifest. When Edit is selected, its shared IDE crate
and generated API reference are staged under the selected build output's `dependencies/`
so the sibling source checkout remains clean. The builder uses the pinned Rust
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
units; the dev kernel uses optimization level 1. The driver adapts the target
and std sources, explicitly strips debug information from the packaged module
with `objcopy`, and creates the existing Blueprint package and AppDB image from it.

`cargo rigreal` applies `TRUEOS-Buildins/tools/release-profile.toml` to every
builtin: full LTO, one codegen unit, aborting panics, full host debug information,
and incremental compilation. Optimization levels follow each app's release
manifest: Cargo defaults to level 3, while Hull and Edit retain `"s"`.
This shared file is the
release configuration location; it does not change the fast dev profile or the
host AppDB helper. Packaged modules still use `--strip-debug`: their symbols and
relocations are required by the TRUEOS loader, so `strip = "symbols"` cannot be
used. Packaging also removes the compiler-only `.llvmbc` and `.llvmcmd` sections
left by the relocatable LTO link. Split debug information stays off for the
custom object target.

Generated app sources, dependency locks, Cargo caches, linked objects, packages,
database images, and logs live in `tgt/buildins/dev/` or `tgt/buildins/release/`.
Dev and release caches stay separate. The driver invokes Cargo for every selected app on
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
`http://192.168.178.111:8080/TrueOS.tar.lz4`. The running `pxe3.js` serves this
archive on port 8080, bound to its selected LAN interface. Select **Live update
from LAN** in the OS builtin. To serve the current archive without `pxe3.js`, from
the development host manually, run this from the repository root:

```sh
python3 -m http.server 8080 --bind 0.0.0.0 --directory tgt
```

The rig build does not start an HTTP server. `pxe3.js` serves the three native
boot files over TFTP and only `/TrueOS.tar.lz4` over HTTP. Online disk installation retains its
separate `https://trueos.eu/TrueOS.lz4` endpoint and older ISO-containing bundle.

With `buildin-apps` enabled, the kernel build verifies that the prepared set matches the current sources and
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

## GitHub release

Once [`.github/workflows/release.yml`](../../.github/workflows/release.yml) is on
the default branch, choose **Actions → Release → Run workflow**. Commit a new
positive integer in `tools/cnt` before each release: counter `N` produces version
`0.0.N` and tag `v0.0.N`. An existing tag or release stops the workflow before
compilation.

The cloud job checks out the kernel, current `TRUEOS-Buildins` `master`, and the
archived TRUEOS toolchain as siblings. It runs the same
`bash tools/trueos-boot/prepare.sh --release` without a rig reset and uploads one
custom asset, `TrueOS-0.0.N.lz4`. This is the native three-file tar/LZ4 archive
described above, including the builtin AppDB inside `TRUEOS.elf`. Release notes
contain its download link, SHA-256, and kernel source commit. Signing, provenance,
extra bundles, and shader baking are deferred. GitHub adds its own source-code
ZIP and tar downloads automatically.

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
uses that monitor data for physical font sizing, including Shell3 text.
When EDID is unavailable on an active display, fonts use their native 6×11
pixel proportions: 12×22 cells at the default text scale. This keeps the same line/quad R8 production;
the general millimetre APIs still require measured monitor dimensions.

## Hardware diagnostics

The GOP framebuffer shows a large centered `Boot-Loading TrueOS §` title and
one smaller centered stage beneath it: loading the kernel, loading firmware
modules, preparing boot responses, starting secondary CPU threads (when
available), and entering the kernel. A boot stop displays its reason there.
Detailed diagnostics go to COM1, including the boot source, file downloads,
kernel image size, discovered logical CPU count, firmware exit, and individual
CPU startup results. P cores, E cores and SMT threads are enumerated through
ACPI MADT and identified by their APIC IDs.

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
