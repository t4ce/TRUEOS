# Runtime Odin to native GPGPU Mandelbrot

The experiment joins the freestanding odin-son compiler to the existing direct
RCS compute carrier and brokered UI4 frame. Source is byte-embedded, compiled
on the CPU while TRUEOS runs, and emitted as real uncompacted Xe-LP SIMD8 EU
instructions. The GPGPU kernel performs the Mandelbrot recurrence and escape
comparisons. The driver owns coordinate generation, grayscale packing,
tail-masked surface writes, memory fence and EOT.

The recovered older raw-EU experiments are documented separately in
[`odin-native-eu-history.md`](odin-native-eu-history.md). Their instructions
provide launch and retirement evidence; the compiler does not substitute their
Mandelbrot machine code for source compilation.

## Build

From TRUEOS, build without deploying or resetting the rig:

```sh
CARGO_NET_OFFLINE=true make iso CARGO_BUILD_FLAGS='--features odin-native-gpgpu' \
  START_BAREMETAL_LOG=0 PUBLISH_RELEASE_SMB=0
```

From the sibling odin-son repository:

```sh
ODIN=/home/t4ce/Programmme/odin/odin bash trueos/build.sh
python3 trueos/test_mandelbrot.py
```

The compiler Blueprint is `trueos/build/odin-son.bp`. Build and deploy the named
test pack from the sibling TRUEOS-Blueprints repository:

```sh
ODIN=/home/t4ce/Programmme/odin/odin cargo bp odin-son
```

In Shell3 over SSH, submit `§§odin-son` and Enter to fetch and launch it. Select
`gpu-mandelbrot` in the compiler menu; Enter emits `mandelbrot.eu.bin`, and `r`
compiles the embedded source again and displays it. The image supports mouse
drag to pan and wheel to zoom. Q/Escape in the image returns to the compiler.
The currently booted kernel must enable `odin-native-gpgpu`; publishing the
compiler package does not change the kernel.

A launcher providing argv
can launch it with
`--verify-gpu-mandelbrot` for the two-program test or `--run-gpu-mandelbrot`
for the retained pan/zoom window. Delivery uses the existing Blueprint loader.
`--gpu-mandelbrot` exports the raw arithmetic body to `mandelbrot.eu.bin`
without executing the GPU.

For the current Shell3 AppDB launcher, prepare a separate package whose no-args
entry runs the verification mode. From odin-son:

```sh
ODIN=/home/t4ce/Programmme/odin/odin bash trueos/build-mandelbrot.sh
```

This produces `trueos/build/odin-mandelbrot.bp` and rebuilds the ordinary
`odin-son.bp` with its usual compiler entry. From TRUEOS, package the dedicated
demo and all current builtins into a private kernel and ISO:

```sh
python3 tools/odin-native/prepare_demo.py \
  --base-iso bld/trueos.iso --output /tmp/odin-native-demo \
  --cache /home/t4ce/Repos/TRUEOS/tgt
```

The output directory must be new. The helper enables `odin-native-gpgpu`, copies
the current builtins and demo into its own AppDB bundle, and records source
snapshots, package hashes and build commands in `build.json`. It produces
`TRUEOS.full.elf`, stripped `TRUEOS.elf` and `trueos.iso` without deploying or
resetting hardware. `--cache` reuses the selected Cargo output directory;
omitting it clones the current `tgt` cache into the private output directory.
After explicitly deploying and booting that image, enter exact name
`odin-mandelbrot` in Shell3 CMD/AppDB mode. HTTP uploading a `.bp` alone does
not install it into volatile AppDB.

## Boundary

`trueos_cabi_ui4_scene_native_compute_upload_v1` receives ordered chunks of at
most 2048 bytes into the caller's visual window. A complete upload admits an
immutable body of at most 24 KiB. The body is straight-line SIMD8 ALU code;
the runtime checks register initialization and reserves its header, offsets
and high-GRF envelope temporaries. There is no static executable hash allowlist
in this generated-code path. The Cargo feature is off by default, and host
dispatch repeats that check so a raw VMCALL cannot bypass it.

`trueos_cabi_ui4_scene_native_compute_render_v1` consumes four f32 values:
complex-plane origin x/y and pixel step x/y. Pixel coordinates are sampled at
integer x/y; no half-pixel bias is implicit. UI4 supplies the exact owned back
buffer and dimensions. The driver launches full SIMD8 workgroups, masks stores
past the visible width, batches rows, and returns a surface release only after
ordered GPU completion and context retirement. The CPU does not write pixels.
An ambiguous submission retains the carrier and frame lease.

This is raw compute, with a runtime-owned thread fence/EOT tail. It does not
use a render pipeline, IGC, SPIR-V, or a static ShaderToy program slot. It
shares the existing system GPGPU context, address-space mapping, submission
and retirement helpers used by the established compute paths.

## Evidence

The 25-iteration source emits a 14,496-byte arithmetic body; the runtime
16-iteration variant emits 9,168 bytes. Host/native compiler outputs match,
IGA disassembly/reassembly matches their bytes, and independent reference
calculations match the interpreted SIMD lanes. The complete embedded-source
runner also executes under a host C-ABI harness with all source reads forbidden.

Run the driver and ownership checks from TRUEOS:

```sh
python3 tools/testpy/test_odin_generated_pixel.py
python3 tools/testpy/test_odin_native_bridge.py
```

On hardware, a successful `--verify-gpu-mandelbrot` must show two
`stage=runtime-compiled` application records, followed by two actual
`odin-native-pixel: completed` records with different body hashes and visible
framebuffer hashes. Completion records use Important level so the usual GPGPU
Warn filter retains them. An application upload/queue marker alone does not
prove hardware execution.

Physical execution passed on ADL-S `4680:0c` on 2026-10-22. The corrected R4
image was verified by fresh PXE reads of the kernel and bootloader. All three
native kernels retired, and every visible pixel matched the independent C
reference in [`reference.c`](../../tools/odin-native/reference.c):

| Runtime-compiled source | EU body bytes | GPU/reference framebuffer FNV-1a64 |
| --- | ---: | --- |
| Constant `0.5`, 64×64 | 16 | `679ddd60d6b96325` |
| Mandelbrot, 25 iterations, 320×240 | 14,496 | `cfa5852b3c69e7a6` |
| Mandelbrot, 16 iterations, 320×240 | 9,168 | `8a797794491ba3af` |

The two Mandelbrot bodies were compiled inside the same running TRUEOS app
from source strings with different loop bounds, uploaded through the generated
code path, executed and published. Neither body was selected from a shader
catalog. The published `cargo bp odin-son` package was also fetched through
Shell3, used to emit the body from its menu, and successfully rendered the same
25-iteration GPU checksum. Kernel/package identities and actual completion
records are preserved in
[`odin-runtime-mandelbrot-20261022.json`](odin-runtime-mandelbrot-20261022.json).
The corrected published package was also left running without keyboard focus.
A fresh physical-display WD postblend screenshot shows its Mandelbrot window
at the requested location. Local capture, runtime-emitted EU binary, reset
receipt and completion excerpts are in `bld/odin-native-mandelbrot-proof/`.

The first bring-up runs exposed two wrapper defects: unsupported Xe-LP
dword×dword integer multiplication to a dword GRF, and an indirect-payload
descriptor with zero per-thread read length but nonzero cross-thread length.
The wrapper now uses SHL/UD×UW and three per-thread GRFs with zero cross-thread
GRFs. TGL PRM Vol. 2d p.650 permits the latter profile; Intel IGC's hardware
conformity pass documents the multiply restriction
([capability table](https://github.com/intel/intel-graphics-compiler/blob/master/visa/HWCaps.inc),
[lowering](https://github.com/intel/intel-graphics-compiler/blob/master/visa/HWConformity.cpp)).
These changes affect the
runtime envelope, not the compiler's floating-point Mandelbrot recurrence.

Current device admission is ADL-S `4680:0c` and TGL `9a49:01`. The physical
execution proof above covers ADL-S; TGL is admitted but was not tested here.
