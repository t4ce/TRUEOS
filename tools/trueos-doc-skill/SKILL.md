---
name: trueos-docs
description: Query compact repository-local TRUEOS knowledge for the cargo rigdev/rigreal builtin and kernel workflow, physical-rig SSH access, Shell3 modes and Matrix navigation, terminal UI handoff, logging, TRUEOSFS HTTP, or checked-in architecture references. Do not use for unrelated implementation details.
---

# TRUEOS docs

Run `tools/testpy/trueos-doc` from the TRUEOS repository root before reasoning
from memory about rig builds or Shell3 access. The command is repository-local;
it need not be installed on PATH. An absolute path to the script also works
from another directory.

## Builtins, kernel, and rig builds

`cargo rigdev` is the working development build and deploy command;
`cargo rigreal` selects release. Both build the configured apps from the sibling
`TRUEOS-Buildins`, create one redb `app.db`, compress it as `app.db.lz4`, embed it
in the kernel, build the native UEFI loader, create `TrueOS.tar.lz4`, and
atomically publish boot files through `tgt/pxe-current`.
The existing `tools/netboot/pxe3.js` process reloads
that publication automatically. The rig command then invokes one physical
reset. A failed build or archive step prevents publication and reset.

The native USTAR/LZ4 archive contains exactly the stripped `TRUEOS.elf`,
`EFI/BOOT/BOOTX64.EFI`, and `EFI/BOOT/tgl_guc_70.bin`; the kernel already embeds
the builtin AppDB. Use `tgt/rigdev.tar.lz4` or `tgt/rigreal.tar.lz4` for the
latest successful profile archive, or `tgt/TrueOS.tar.lz4` for the current PXE
generation. RAM live update uses `https://trueos.eu/TrueOS.tar.lz4` or the LAN
URL `http://192.168.178.111:8080/TrueOS.tar.lz4`. Query `topic rig-builds` for
manual HTTP serving; `pxe3.js` provides only TFTP. Online disk installation
keeps the separate older ISO bundle at `https://trueos.eu/TrueOS.lz4`.

Apps use the Paper object-build flow: normal Cargo builds link with `-r` to
produce tracked `.o` modules, followed by explicit stripping and the existing
Blueprint/AppDB packaging. Cargo keeps its normal parallelism. Dev apps and the
dev kernel compile unoptimized; app dev builds disable LTO and use incremental
compilation. `cargo rigreal` applies the shared builtin configuration at
`TRUEOS-Buildins/tools/release-profile.toml`: full LTO, one codegen unit, aborting
panics, full host debug information, and incremental compilation. Each app keeps
its release optimization level: Cargo defaults to 3; Hull and Edit use `"s"`.
Packaging strips debug information and removes the compiler-only `.llvmbc` and
`.llvmcmd` sections; preserve symbols and relocations needed by the loader.
Split debug information stays off for the custom target.
The rig workflow does not force a single CPU or enter `build-safe.sh`;
that wrapper remains an explicit diagnostic
option. Use `CARGO_BUILD_JOBS` to select a different Cargo job count when needed.

Builtins belong to the kernel generation and use the regular AppDB at boot;
they have no separate Blueprint publication step. Cargo checks every app on
each run and reuses fresh artifacts; unchanged packages and database images
remain cached. Query `tools/testpy/trueos-doc topic rig-builds` for paths,
incremental behavior, and preparation without a reset. Ordinary `cargo build`
consumes a verified prepared builtin set and does not reset the rig.

## Current physical test-rig access

The physical rig remains at `192.168.178.94`, but its network shell is now
Shell3 over SSH on port **22**. Shell2 NetTCP on port 4245 is retired; do not
use `nc` or the old NetTCP commander launcher to connect.

Use the configured local SSH authentication:

```sh
ssh -tt -p 22 t4ce@192.168.178.94
```

Allocate a real terminal with nonzero dimensions (at least 20 columns and
5 rows); the SSH shell requires a PTY. For agent exec sessions, enable
`tty: true` and set `stty cols 120 rows 40` before connecting if needed.
Keep host-key verification enabled. If the local system SSH config has
invalid ownership or permissions, use `ssh -F /dev/null` with the same
authentication and host. Supply `-i` only when the selected local key exists;
do not assume a project-named identity file. SSH host keys and client identity
are distinct.

Shell3 starts in CMD. One TAB selects System/ADM; a second selects HV.
System helpers use exact names: type `gpu` to open its TUI. Do not append
retired command arguments. Select a retained Matrix slot with `§<slot>` and
Enter, then use `tui` to enter its terminal explicitly.

Start with `tools/testpy/trueos-doc context` for current build and access facts.

- Use `tools/testpy/trueos-doc topic <name>` for `rig-builds`, `rig`, `shell3`, `headjack`, `tui`, `runscripts`, `logs`, `trueosfs-http`, `references`, or `apps`.
- Query `topic logs` before diagnosing a missing record. Area/level filtering happens before the sinks, so a filtered record cannot appear in a host capture file. The bare-metal log drain still writes under `bld/baremetal-logs`; the rig build does not start it automatically.
- Query `topic references` to locate checked-in architecture and hardware HTML fact surfaces.
- Query `topic trueosfs-http` before enumerating or transferring rig files. Discover mounted root IDs at runtime; never replace an absent expected artifact with an older file.
- Use `tools/testpy/trueos-doc command <name>` for current Shell3 names read from `src/shell3/names.rs`. Shell3 recognizes exact names; it does not expose the retired Shell2 argument schemas.
- Use `tools/testpy/trueos-doc search <terms>` when the topic or command name is unclear.
- Treat every response as JSON. A successful response has `ok: true`; an error exits 2 with `ok: false` on stderr.

The documentation command is read-only. Its examples describe syntax; they do not authorize connecting to the rig, launching apps, resetting hardware, or performing any other external mutation.
