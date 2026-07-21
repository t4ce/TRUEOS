# TRUEGA source salvage

This directory was migrated into `trueos-fpga-abi` from `t4ce/TRUEGA` at commit
`e6f7160e22f4463b3167b14a418a6d2ff2cc4322` (the same snapshot as
`TRUEGA-main.zip`). The original PCIe SerDes configuration, Gowin project, board
constraints, Analyzer probes, schematics, generator, build scripts, and flash scripts
are preserved here.

The recovered design proved the TRUEOS `22c2:1100` PCI path, BAR0 posted writes,
BAR0 read completions, and the active-low LED debug path. The function-call redesign
keeps that debug plane and adds one fixed BAR work-package window shared with the
Rust `trueos-fpga-abi` crate. It deliberately does not add an FPGA processor, DMA
requester, TLB, command language, or runtime compiler.

The Ubuntu build has three inputs/outputs:

1. `tools/tga-gen/src/firmware.rs` is the authoritative catalogue and RustHDL source
   for exactly three physical circuits: `led_step_heartbeat`, `add_u32`, and `xor_u32`.
2. `tools/tga-gen` emits staged copies of `src/generated/truega_functions.v`, the
   128-byte `artifacts/truega_firmware.manifest.bin`, and the ordinary typed Rust
   interface at `../src/generated.rs`.
3. `tools/build_fs.sh` feeds the generated Verilog, the VHDL PCIe/BAR handoff shell,
   and the preserved Gowin SerDes IP into Gowin. After successful place-and-route it
   publishes the actual bitstream as `artifacts/min_pci_led.fs` together with the
   generated RTL, binary manifest, and Rust interface.

Slot 0 advances the five-bit LED state and returns `TGAT`. A blinking sequence therefore
proves that the same work-package handoff, fixed function circuit, completion flag, and
result-copy path used by normal calls completed end to end. The raw BAR LED write remains
available only as a transport/debug fallback.

The authoritative ABI layout is in `../src/lib.rs`. `src/top.vhd` only performs PCIe TLP
capture, fixed BAR decoding, work-package ownership transitions, and retirement of the
generated circuit result. TRUEOS links the generated Rust interface and its single-worker
service; it does not link RustHDL, Gowin, a compiler, or HDL source.

The generator also fuses the exact 128-byte binary manifest into a read-only constant ROM
at BAR0 `0x200..0x27f`. TGA compares every manifest word with the generated Rust interface
before it publishes the offload transport. The `TGAT` liveness word proves the common BAR
protocol; the manifest gate proves that slot IDs, signatures, ABI layout, and generated
function RTL are the bundle the running kernel was compiled to call.

The v1 physical function shell intentionally admits only `() -> u32` and
`(u32, u32) -> u32` catalogue entries. The build rejects any other declared shape instead
of emitting metadata the current two-input/one-output circuit ports cannot execute.

Build on Ubuntu with:

```sh
./tools/build_fs.sh
```

Generation occurs in a temporary staging directory. Because the Gowin project refers to
the checked-in generated-Verilog path, the build temporarily swaps that file while Gowin
runs and restores the prior version if synthesis, place-and-route, or bitstream freshness
validation fails. The manifest and Rust interface are never published by a failed build.
On success, every output is copied to a same-directory temporary file and atomically
renamed into place. `artifacts/SHA256SUMS` is published last as the seal for the relative
`min_pci_led.fs` and `truega_firmware.manifest.bin` entries; the legacy single-file
`artifacts/min_pci_led.fs.sha256` is retained for convenience.

The current completion callback is delivered by the single kernel worker after polling
the hardware state. The ABI reserves interrupt-on-complete and IRQ-ack fields, but actual
MSI/MSI-X wiring is a later transport optimization and does not change the call interface.
