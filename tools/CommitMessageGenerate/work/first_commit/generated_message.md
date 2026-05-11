# Commit Message Draft: fc28fd19d15a54eb06fdf29ec97c84abb9e657a9

Original:

```text
SMP AP Stress Loop
```

Generated:

```text
SMP AP Stress Loop

Bootstrap the first bare-metal x86_64 Rust kernel and Limine ISO flow.

Add the initial no_std/no_main kernel crate, custom x86_64-none target, linker
script, nightly rust-src toolchain pin, and Cargo configuration needed to build
core, compiler_builtins, and alloc for a freestanding kernel. Wire the Makefile
to build the kernel, assemble a Limine BIOS/UEFI boot ISO, install the Limine
BIOS boot sector, and run it in QEMU with four virtual CPUs and debugcon output.

Implement the first SMP experiment through a Limine SMP request. The bootstrap
processor reports whether long mode is active, emits its LAPIC id, assigns
ap_entry as the startup address for every non-BSP CPU, and then enters a visible
stress loop. Application processors report their LAPIC ids on entry and spin in
their own periodic debug loop, giving a minimal smoke test that AP startup and
multi-core execution are alive.

This root commit also captures the initial build products and ISO image that
were produced from that setup, including the Limine staging tree, kernel binary,
and Cargo target artifacts.
```

Evidence notes:

- Root commit authored by `t4ce <jonasb@post.com>` on `2025-12-21T15:29:32+01:00`.
- Adds `.cargo/config.toml`, `86_64.json`, `Cargo.toml`, `rust-toolchain.toml`, `linker.ld`, `limine.cfg`, `Makefile`, and `src/main.rs`.
- `src/main.rs` defines the Limine SMP request/response structures, `_start`, `start_aps`, `ap_entry`, debugcon writes to port `0xE9`, and the long-mode MSR check.
- `Makefile` builds with nightly `-Z build-std=core,compiler_builtins,alloc`, stages Limine files, creates `bld/falseos.iso`, installs Limine BIOS boot support, and runs QEMU with `-smp cores=4 -debugcon stdio`.
- The commit includes generated artifacts under `bld/` and `target/`; the generated message calls them out separately from the authored kernel/build setup.
