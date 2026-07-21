# Lab256 multi-phase GPGPU exploration

## Outcome

`lab256_multiphase.cl` proves that the local Intel IGC path can produce one
ADL-S zebin containing three independently dispatchable kernels for a fixed
256x256 Shell2 artifact:

```text
CPU control/history ───────────────┐
                                  v
state A ── lab256_step ──> state B ── lab256_reduce ──> 1.6 KiB report
                                  │                         │
                                  └── lab256_composite <────┘ ──> RGBA8 frame
```

The concept deliberately combines the current Chart, Plasma, and Mandelbrot
ideas instead of presenting three unrelated single-pass windows:

- persistent Gray-Scott state supplies real frame-to-frame computation;
- a plasma palette visualizes that state;
- the state can warp a bounded Mandelbrot evaluation;
- native premultiplied alpha makes inactive background pixels translucent
  while field/fractal features become progressively more opaque;
- GPU telemetry supplies a live histogram;
- a 256-sample CPU-fed history supplies the scope line;
- pointer injection changes simulation state rather than only changing color.

The artifact is hash-allowlisted and wired into both TrueOS-Spirit's
deterministic startup sequence and the live UI4 preview service. Shell2 can now
exercise that exact runtime path as a fixed-size premultiplied-alpha window.

## Why 256x256 is an advantage

The fixed extent turns several otherwise open-ended costs into small constants:

| Resource | Logical bytes | Page-mapped bytes |
| --- | ---: | ---: |
| packed state A, UNORM16x2 | 262,144 | 262,144 |
| packed state B, UNORM16x2 | 262,144 | 262,144 |
| RGBA8 destination | 262,144 | 262,144 |
| control, 288 dwords | 1,152 | 4,096 |
| report, 400 dwords | 1,600 | 4,096 |
| **total** | **789,184** | **794,624** |

The maximum visible work is also bounded: two 4096-thread SIMD16 2D passes,
one 16-lane reduction pass, at most 96 Mandelbrot iterations per pixel, and no
dynamic allocation or data-dependent dispatch dimensions.

## IGC result

The artifact was compiled locally for PCI device `0x4680` (`adl-s`) with the
repository bake script and an available `ocloc` reporting driver version
`25.31.034666`.

```text
artifact: artifacts/adls/lab256_multiphase.bin
size:     79,040 bytes
sha256:   77e999ce8a4c5bd308c6ed1d1d139f131c7b77dba45bd7608213e7428c51edf6

spir-v:   artifacts/adls/lab256_multiphase.spv
sha256:   6140cfa8d8200435a7e3786e04237f0e3abbfcc25513c599611117585c00be3a

source sha256:
ea31313ac4259b3577230c0dbac343569c943c045cb612c3b5e496372b22e9d8
```

`ocloc validate` reports the binary as valid and finds exactly three kernels.
Its bundled decoder warns that zebin `.ze_info` minor version 64 is newer than
the decoder's version 54 and that it does not interpret the optional metrics
note. Neither warning prevents decoding or validation.

The whole ELF must be uploaded because each entry point occupies a separate
text section:

| Entry point | ELF file/text offset | Text bytes | BTIs | Cross-thread | Per-thread |
| --- | ---: | ---: | ---: | ---: | ---: |
| `lab256_step` | `0x0040` | 6,272 | 3 | 96 | 96 |
| `lab256_reduce` | `0x18C0` | 8,448 | 3 | 96 | 96 |
| `lab256_composite` | `0x39C0` | 9,792 | 4 | 96 | 96 |

The symbol payloads within those padded text sections are 6,144, 8,304, and
9,624 bytes respectively. Production wiring can hard-code these offsets only
under the matching artifact hash, as the existing one-kernel path does with
its `0x40` text offset.

The first reducer draft used a private 16-element histogram. IGC then emitted
a `private_base_stateless` payload requirement, which the current direct-RCS
lane does not provide. The final reducer performs 16 fixed repeated scans
instead. This removes implicit scratch and leaves only the explicit bindings
listed above.

## Pass contract

### Pass 1: persistent state

`lab256_step(state_in, state_out, control)` dispatches `16 x 256` SIMD16
groups. Every work item owns one pixel. It applies a nine-tap Gray-Scott update,
optional wraparound, a bounded pointer injection, and deterministic low-bit
dither. State is packed as two UNORM16 values in one dword.

The host swaps A/B only after the complete three-pass batch retires. A submitted
batch that times out quarantines both the direct-RCS context and Lab256 state;
the Spirit backbuffer is never made presentation-eligible.

### Pass 2: telemetry

`lab256_reduce(state, report, control)` dispatches one SIMD16 group. Each lane
owns every sixteenth pixel and writes its own 24-dword stripe: sum, sum-square,
maximum, weighted X/Y, active count, checksum, 16 histogram bins, and a lane
done marker. There are no atomics and no inter-lane barrier.

The composite consumes the stripes on-GPU. After final retirement the CPU may
read only this 1.6 KiB report, derive a metric, and append one Q0.16 sample to
the next frame's control history. Pixel state and the RGBA frame remain
GPU-resident.

### Pass 3: composition

`lab256_composite(state, report, control, dst_rgba, dst_pitch_bytes)` dispatches
`16 x 256` SIMD16 groups. It produces one complete premultiplied
AABBGGRR/RGBA8 pixel per work item. It combines scientific plasma color, the
reaction field, optional field-warped Mandelbrot, the GPU histogram, and the
CPU history scope.

Alpha is the only output path. Control dword 17 supplies a finite f32 background
alpha clamped to `[0, 1]` (fallback `0.08`). Field concentration, reaction
edges, and escaped fractal detail raise opacity toward one. The chart panel is
at least `0.82`, bars at least `0.90`, and the scope core, pointer, and pass LEDs
are fully opaque. RGB is multiplied by alpha before packing, matching UI4's
`Rgba8888Premultiplied` contract rather than relying on straight-alpha blending.
Setting background alpha to `1.0` remains available when an opaque presentation
is wanted, but it does not select a separate shader branch or format.

Three tiny status LEDs are shader-authored so a capture can visibly distinguish
the new path without relying on Shell2 log text.

## Host bridge

TrueOS-Spirit now uses one GuC/direct-RCS submission containing three IDDs and
three walkers, not three separately polled submissions:

1. Add a hash-locked `lab256_multiphase` artifact entry and one upload VA.
2. Allocate persistent page-aligned A, B, control, and report storage; map the
   current Spirit 256x256 cursor backbuffer lease as the destination.
3. Validate and clamp the CPU control page before flushing it for GPU use,
   including the f32 background alpha in reserved control dword 17.
4. Dispatch step with groups `16 x 256`, all SIMD16 lanes enabled.
5. Emit `MEDIA_STATE_FLUSH`, then the existing stalling HDC/L3 producer flush
   and consumer invalidation before reduce reads state B.
6. Dispatch reduce with groups `1 x 1`, all SIMD16 lanes enabled.
7. Emit the same full producer/consumer boundary before composite reads the
   report.
8. Dispatch composite with groups `16 x 256`, then use the post-sync marker to
   release Spirit's GPU-only producer bit before cursor-plane arming.
9. After retirement, audit report magic/version/frame and all 16 lane markers
   without adding a CPU producer gate, then swap A/B. The fixed startup uses a
   deterministic host-authored history; telemetry feedback remains a later API.

The resulting producer path is Spirit Embassy worker -> GPU executor -> vGPU ->
physical GuC scheduler. CUR_SURFLIVE remains the separate display-retirement
proof that completes the Spirit fence; no CPU producer bit participates in the
ten-frame startup gate.

## Capability and privilege boundary

Making the artifact more capable should not mean granting it arbitrary kernel
memory or MMIO. The useful privilege is a narrow, trusted Shell2 service:

- fixed 256x256 dispatches only;
- five exact buffers with separately validated sizes and stable PPGTT VAs;
- hash-allowlisted instruction image and compile-time entry offsets;
- CPU writes only the control page and reads only the report page;
- alpha changes only per-pixel coverage inside Spirit's existing premultiplied
  cursor backbuffer; it grants no additional MMIO or plane ownership;
- the shader cannot select addresses, pitches other than the validated RGBA8
  pitch, iteration counts above 96, or another display plane;
- Spirit retains frame, cursor-plane, and SURFLIVE ownership.

That boundary is substantially richer than an average one-shot shader while
remaining smaller and easier to audit than a generic OpenCL command interface.

## Live Shell2 test

The focused test command starts a 30-second run at the default 33 ms cadence:

```text
gpgpu test lab256
```

Its optional arguments are duration, cadence, and publish interval. A duration
of zero runs continuously:

```text
gpgpu test lab256 [duration_ms] [cadence_ms] [publish_every]
gpgpu test lab256 0 33 1
```

The same producer is also available through the configurable preview surface:

```text
gpgpu preview start lab256 [duration_ms] [cadence_ms] [publish_every]
gpgpu preview status
gpgpu preview stop
```

Pointer events map to injection coordinates. A small follow-up command may set
only named, clamped values such as `feed`, `kill`, `mandel-scale`, and
`mandel-iters`; raw addresses and dispatch sizes should never be user-facing.

For CPU history, the most appealing TRUEOS-native sources are compositor/RCS
retirement time, package power/temperature, scheduler pressure, or audio
energy. RCS retirement time is the safest first source because it is already
available in the preview metrics and proves a real CPU/GPU feedback loop with
no additional device privilege.
