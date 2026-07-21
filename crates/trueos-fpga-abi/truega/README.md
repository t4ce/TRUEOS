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

## LFM2.5 native model checkpoint

`tools/lfm25-seal` is the host-only converter for the pinned LFM2.5-350M Q8_0 appliance.
It validates the complete GGUF SHA-256, architecture metadata, exact 148 names, shapes,
types, and hybrid layer schedule before publishing anything. It then copies every Q8_0
block bit-for-bit, converts the 55 F32 vectors/kernels to little-endian BF16 using
round-to-nearest-even, and starts every tensor on a zero-filled 256-byte boundary.

Run the complete pack and verification step on Ubuntu with:

```sh
./tools/build_lfm25_image.sh
```

This produces the ignored, licensed weight image
`../../../tools/lfm2.5-350m/LFM2.5-350M-Q8_0.truega.bin` plus three small checked-in
views of one canonical contract:

- `artifacts/lfm25_model.contract.bin`: 192-byte seal plus 148 24-byte descriptors.
- `../src/lfm25_generated.rs`: ordinary `no_std` Rust constants for TRUEOS.
- `src/generated/truega_lfm25_model.v`: a synthesizable 936-word synchronous ROM.

The native image is exactly `376701952` bytes with SHA-256
`051c60856786de2ac7089109354259fa29fcd57e83d585efc86afa0fb605bb86`.
The source GGUF is not required to build the current heartbeat firmware. The model ROM is
intentionally not instantiated in `top.vhd` or added to the Gowin project yet, so this
checkpoint does not alter BAR0, the three slots, timing, the bitstream, or live hardware.

## Layer-0 FFN golden checkpoint and Q8_0 GEMV

`tools/capture_lfm25_ffn_golden.sh` instruments the exact official llama.cpp `b10075`
commit `76f46ad29d61fd8c1401e8221842934bf62a6064`. Its two-line graph patch only exposes
the post-down tensor and fixes LFM2's callback to name the actual FFN result; no model
operation, weight, Q8_0 block, or arithmetic implementation is changed. The capture uses
one CPU thread, token 1 (BOS), layer 0, and disables offload and warmup.

The checked-in `artifacts/lfm25_layer0_ffn.golden.bin` is a 64,000-byte sealed artifact
with five little-endian F32 vectors:

| Vector | Elements |
| --- | ---: |
| normalized layer-0 FFN input | 1,024 |
| gate projection | 4,608 |
| up projection | 4,608 |
| `SiLU(gate) * up` | 4,608 |
| down projection | 1,024 |

The header binds the llama.cpp commit, source GGUF SHA-256, unchanged native-image
SHA-256, model-contract SHA-256, payload SHA-256, token, and layer into a final artifact
seal. The complete-file SHA-256 is
`eb124c333e7a7095a78fc6c0004f90a43fa825bdfd1a8f74ac9d67c538484185`.

The host verifier quantizes both activation vectors as llama.cpp Q8_0, reads layer-0 gate,
up, and down matrices directly from the unchanged native image, and checks all 10,240
projection outputs. Measured maximum absolute errors against the captured tensors are
`1.20e-7` (F32 block accumulation), `7.46e-8` (deterministic Q30 accumulation), and
`1.87e-9` (SiLU product), against a frozen `2.0e-6` acceptance bound.

The synthesizable implementation is under `src/compute`:

- `truega_q8_0_dot32.v` is a six-stage, 32-lane signed 8x8 multiplier and exact 21-bit
  adder tree accepting one native Q8_0 block per cycle.
- `truega_q8_0_scale_q30.v` decodes the two native FP16 scales and rounds each scaled
  block term to signed Q30 with round-to-nearest, ties-to-even.
- `truega_q8_0_gemv.v` streams blocks and retires one signed 64-bit Q30 row result.

Run the reproducible checks with:

```sh
./tools/capture_lfm25_ffn_golden.sh
./tools/simulate_q8_0_gemv.sh
./tools/synthesize_q8_0_gemv.sh
```

The HDL simulation covers 208 unchanged native-image blocks (row 0 of gate, up, and down)
plus two signed extremes. All 210 integer dots and scaled Q30 terms match bit-for-bit, and
the row results satisfy the captured-F32 error bound. The isolated Gowin synthesis uses a
temporary project containing only the standalone compute top. It used 2,453 LUTs, 658
ALUs, 1,491 registers, and 34 DSP blocks in the current synthesis report. It does not run
place-and-route or emit an `.fs`; pre/post hashes also guard the heartbeat project, top,
generated function RTL, bitstream, and checksum files.
