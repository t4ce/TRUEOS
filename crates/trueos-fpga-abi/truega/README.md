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
   for exactly three physical circuits: `led_step_heartbeat`, `add_u32`, and
   `lfm25_q8_row_block`.
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

The physical shell backs the complete ABI-reserved 96-byte input and 96-byte output
envelopes. Every generated slot uses one clocked `start/busy/done/error` handoff; the shell
validates the exact declared input length and output capacity before asserting `start`,
then retires only after `done`. Slot 2 consumes a four-byte row-control header plus two
unchanged 34-byte Q8_0 blocks and returns the signed integer dot, block Q30 term, and
partial/final signed Q30 row accumulator.

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

The FPGA raises MSI on retirement. Vector `0x42` wakes the single kernel worker, which
acknowledges the slot and delivers the result to the registered Rust callback. Polling is
retained only as timeout recovery and is expected to remain at zero during normal calls.

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
The source GGUF is not required to build the firmware. The model ROM remains uninstantiated:
the first Q8_0 function receives native blocks through its fixed work-package input rather
than reading the complete model image itself.

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
- `truega_q8_0_scale_q30_seq.v` decodes the two native FP16 scales and performs the
  variable shift over multiple cycles, rounding the scaled block term to signed Q30 with
  round-to-nearest, ties-to-even.
- `truega_q8_0_block_slot.v` latches one 68-byte call, sequences the dot and scale units,
  and exposes the common reusable `start/busy/done/error` contract.
- `truega_q8_0_gemv.v` streams blocks and retires one signed 64-bit Q30 row result.
- `truega_lfm25_gate_row_slot.v` wraps that engine with the fixed 32-block layer-0
  gate-row contract. Its ready/valid feeder and requested block index are the explicit
  boundary for the future native-image DDR reader. `DIAGNOSTIC_ENABLE` defaults to zero,
  and the slot is not instantiated by `top.vhd`, so heartbeat firmware is unchanged.
- `truega_q8_0_row_block_slot.v` is the nearer inline-BAR boundary: a 4-byte
  first/last/index header plus the existing 68-byte native blocks per call. It retains
  the signed-Q30 accumulator across calls 0..31 and returns dot, term, and partial/final
  row result. Its enable parameter defaults to zero, and the generated slot-2 wrapper
  explicitly enables it together with the paired 72-byte/20-byte Rust ABI.

The checked-in `artifacts/lfm25_q8_block.golden.bin` is a separately sealed 336-byte
runtime vector derived from gate row 0, block 0. Its 68-byte input is the activation block
followed by the native-image weight block; its 12-byte expected output is dot `-14901` and
Q30 term `-9429888`. The generator verifies the artifact provenance, payload hash, and
self-seal before emitting the Rust constants used by `tga q8` and `tga test`; the active
slot wraps that fixture with `first|last,index=0` and also returns row Q30 `-9429888`.

TRUEOS exposes two fixed model checks. `tga model verify` streams
`trueosfs:/models/lfm2.5/LFM2.5-350M-Q8_0.truega.bin` in 256 KiB chunks and checks the
exact 376,701,952-byte size and pinned SHA-256 without parsing GGUF. `tga model row0`
range-reads the 32 native layer-0 gate-row blocks, supplies the sealed activation blocks,
and requires every callback's dot, term, and accumulated row result to match bit-for-bit.
The final exact row is `29481209` Q30; its distance from captured F32 is 9 against the
frozen bound of 2148.

Run the reproducible checks with:

```sh
./tools/capture_lfm25_ffn_golden.sh
./tools/simulate_q8_0_gemv.sh
./tools/synthesize_q8_0_gemv.sh
./tools/synthesize_q8_0_block_slot.sh
```

The HDL simulation covers 208 unchanged native-image blocks (row 0 of gate, up, and down)
plus two signed extremes. All 210 integer dots and scaled Q30 terms match bit-for-bit, and
the row results satisfy the captured-F32 error bound. The same simulation also calls the
multi-cycle block slot for all 210 vectors and calls the generated 96-byte wrapper twice
with the sealed runtime vector. It also runs the fixed layer-0 gate row slot twice, with
intentional feeder stalls, proves its default-disabled state, and runs the BAR-oriented
32-call sequencer including one-block compatibility and malformed-order recovery. The
isolated block-slot synthesis reaches 142.908 MHz and
uses 2,155 logic elements, 1,429 registers, 33 `MULT12X12` plus one `MULT27X36`, and no
block RAM. It does not emit an `.fs`; pre/post hashes guard the integrated project inputs
and published firmware files.

The integrated image closes the 100 MHz TLP clock with zero setup/hold violations. The
build refuses to publish if the timing report is missing, if TLP Fmax is below 100 MHz, or
if any endpoint is violated. The row-enabled image closes at 100.004 MHz and uses
6,550/138,240 logic elements (5%), 4,902/139,140 registers (4%), 18.5/298 DSP units
(7%), and no SSRAM.
