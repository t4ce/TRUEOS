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

1. `tools/tga-gen/src/firmware.rs` is the RustHDL source for exactly three physical
   circuits: `led_step_heartbeat`, `add_u32`, and `xor_u32`.
2. `tools/tga-gen` emits `src/generated/truega_functions.v`, the 128-byte
   `artifacts/truega_firmware.manifest.bin`, and the ordinary typed Rust interface at
   `../src/generated.rs`.
3. `tools/build_fs.sh` feeds the generated Verilog, the VHDL PCIe/BAR handoff shell,
   and the preserved Gowin SerDes IP into Gowin and emits `impl/pnr/min_pci_led.fs`.

Slot 0 advances the five-bit LED state and returns `TGAT`. A blinking sequence therefore
proves that the same work-package handoff, fixed function circuit, completion flag, and
result-copy path used by normal calls completed end to end. The raw BAR LED write remains
available only as a transport/debug fallback.

The authoritative ABI layout is in `../src/lib.rs`. `src/top.vhd` only performs PCIe TLP
capture, fixed BAR decoding, work-package ownership transitions, and retirement of the
generated circuit result. TRUEOS links the generated Rust interface and its single-worker
service; it does not link RustHDL, Gowin, a compiler, or HDL source.

Build on Ubuntu with:

```sh
./tools/build_fs.sh
```

The current completion callback is delivered by the single kernel worker after polling
the hardware state. The ABI reserves interrupt-on-complete and IRQ-ack fields, but actual
MSI/MSI-X wiring is a later transport optimization and does not change the call interface.
