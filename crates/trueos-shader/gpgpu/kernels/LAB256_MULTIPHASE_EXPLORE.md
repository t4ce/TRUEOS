# Lab256 multi-phase GPGPU exploration

## Outcome

`lab256_multiphase.cl` proves that the local Intel IGC path can produce one
ADL-S zebin containing three independently dispatchable kernels for a fixed
256x256 Shell2 artifact:

```text
CPU control ───────────────────────┐
                                  v
state A ── lab256_step ──> state B ── lab256_reduce ──> 576-byte report
                                  │                         │
                                  └── lab256_composite <────┘ ──> RGBA8 frame
```

The visible composition is now a compact radial flare inspired by
`https://webgl-shaders.com/flare-example.html`, while preserving the original
three-pass persistent architecture:

- persistent Gray-Scott state supplies real frame-to-frame computation for a
  small mouse-authored trail layer;
- the centered flare is procedural and consumes neither pointer coordinates nor
  reaction state;
- one radial wave, an irregular hot body, a rational halo, and four analytical
  ray axes produce the flare silhouette;
- native premultiplied alpha makes inactive background pixels translucent
  while the flare body, rays, and hot core become progressively more opaque;
- compact GPU telemetry supplies a frame audit and field mean;
- one CUR_SURFLIVE-rate dot supplies a coarse presentation-health hint;
- Spirit's preferred physical cursor injects only the independent reaction
  layer at its latched coordinate;
- a quiet initial state, six-pixel brush, and explicit decay keep that wake
  restrained instead of growing autonomous reaction colonies.

The artifact is hash-allowlisted and wired into both TrueOS-Spirit's
continuous cursor-plane stream and the live UI4 preview service. Shell2 can
exercise that exact runtime path as a fixed-size premultiplied-alpha window.

## Why 256x256 is an advantage

The fixed extent turns several otherwise open-ended costs into small constants:

| Resource | Logical bytes | Page-mapped bytes |
| --- | ---: | ---: |
| packed state A, UNORM16x2 | 262,144 | 262,144 |
| packed state B, UNORM16x2 | 262,144 | 262,144 |
| RGBA8 destination | 262,144 | 262,144 |
| control, 19 dwords | 76 | 4,096 |
| report, 144 dwords | 576 | 4,096 |
| **total** | **787,084** | **794,624** |

The maximum visible work is also bounded: two 4096-thread SIMD16 2D passes,
one 16-lane reduction pass, one radial sine per output pixel, and no dynamic
allocation or data-dependent dispatch dimensions.

## IGC result

The artifact was compiled locally for PCI device `0x4680` (`adl-s`) with the
repository bake script and an available `ocloc` reporting driver version
`25.31.034666`.

```text
artifact: artifacts/adls/lab256_multiphase.bin
size:     57,496 bytes
sha256:   aa7a557ec6e8834195d7a3ebb8484fc5bbc0c85755f9d8496a3a70d1f1cac7cf

spir-v:   artifacts/adls/lab256_multiphase.spv
sha256:   cf85bc467ab3317a260a938a3593d98c4861ef09fd13815ac643e607ef73ef31

source sha256:
ee5e3aabd305b2137934f3a50c86d315fae9fe235a73df712e9b6368411f3318
```

`ocloc validate` reports the binary as valid and finds exactly three kernels.
Its bundled decoder warns that zebin `.ze_info` minor version 64 is newer than
the decoder's version 54 and that it does not interpret the optional metrics
note. Neither warning prevents decoding or validation.

The whole ELF must be uploaded because each entry point occupies a separate
text section:

| Entry point | ELF file/text offset | Text bytes | BTIs | Cross-thread | Per-thread |
| --- | ---: | ---: | ---: | ---: | ---: |
| `lab256_step` | `0x0040` | 5,376 | 3 | 96 | 96 |
| `lab256_reduce` | `0x1540` | 1,664 | 3 | 96 | 96 |
| `lab256_composite` | `0x1BC0` | 8,192 | 4 | 96 | 96 |

The symbol payloads within those padded text sections are 5,232, 1,536, and
8,024 bytes respectively. Production wiring can hard-code these offsets only
under the matching artifact hash, as the existing one-kernel path does with
its `0x40` text offset.

The visual histogram and CPU-fed history chart were removed from this build.
That also removes the reducer's 16 repeated histogram scans and leaves one
bounded scan per lane with no implicit scratch allocation.

## Pass contract

### Pass 1: persistent state

`lab256_step(state_in, state_out, control)` dispatches `16 x 256` SIMD16
groups. Every work item owns one pixel. It applies a nine-tap Gray-Scott update,
optional wraparound, a bounded six-pixel pointer injection, explicit trail
decay, and deterministic low-bit dither. Reset begins from an almost chemically
quiet field, so cursor input is the only strong chemical-B source. State is
packed as two UNORM16 values in one dword.

The host swaps A/B only after the complete three-pass batch retires. A submitted
batch that times out quarantines both the direct-RCS context and Lab256 state;
the Spirit backbuffer is never made presentation-eligible.

### Pass 2: telemetry

`lab256_reduce(state, report, control)` dispatches one SIMD16 group. Each lane
owns every sixteenth pixel and writes its own 8-dword stripe: sum, sum-square,
maximum, weighted X/Y, active count, checksum, and a lane done marker. There
are no atomics and no inter-lane barrier.

The composite consumes the field mean on-GPU. After final retirement the CPU
reads only the 576-byte report for its magic/version/frame and lane-marker
audit. Pixel state and the RGBA frame remain GPU-resident.

### Pass 3: composition

`lab256_composite(state, report, control, dst_rgba, dst_pitch_bytes)` dispatches
`16 x 256` SIMD16 groups. It produces one complete premultiplied
AABBGGRR/RGBA8 pixel per work item. It combines the persistent reaction field,
centered radial flare composition, and one presentation-rate status dot as
separate layers. The flare uses one radial wave, a small algebraic lobe, four
reciprocal-distance ray axes, a rational core, and a halo. No pointer coordinate
or reaction value enters its position, body, plume, or ray math.

The reaction layer is composited in actual surface coordinates. A mean-relative
floor rejects the quiet seed, hot chemical-B values appear mint, and the fading
wake cools toward blue. It contributes its own color and alpha without moving or
warping the centered flare. The former direct pointer cross was removed.

Alpha is the only output path. Control dword 17 supplies a finite f32 background
alpha clamped to `[0, 1]` (fallback `0.08`). Reaction trail concentration, core,
corona, and rays raise opacity toward one. The FPS dot is fully opaque. RGB is
multiplied by alpha before packing, matching UI4's
`Rgba8888Premultiplied` contract rather than relying on straight-alpha blending.
Setting background alpha to `1.0` remains available when an opaque presentation
is wanted, but it does not select a separate shader branch or format.

One small shader-authored dot consumes control dword 18. Spirit averages only
successful cursor-plane CUR_SURFLIVE transitions in fixed 500 ms windows and
seeds the first window at its 60 Hz target. The dot is green above 50 FPS,
yellow from 30 through 50 FPS, and orange below 30 FPS. GuC admission and
producer-marker completion do not count as visible frames.

## Host bridge

TrueOS-Spirit now uses one GuC/direct-RCS submission containing three IDDs and
three walkers, not three separately polled submissions:

1. Add a hash-locked `lab256_multiphase` artifact entry and one upload VA.
2. Allocate persistent page-aligned A, B, control, and report storage; map the
   current Spirit 256x256 cursor backbuffer lease as the destination.
3. Validate and clamp the CPU control page before flushing it for GPU use,
   including the normalized physical-cursor coordinate in dword 5, background
   alpha in dword 17, and the rounded presentation-rate estimate in dword 18.
   Dwords 11 through 14 provide bounded flare radius, turbulence, ray gain, and
   pulse speed.
4. Dispatch step with groups `16 x 256`, all SIMD16 lanes enabled.
5. Emit `MEDIA_STATE_FLUSH`, then the existing stalling HDC/L3 producer flush
   and consumer invalidation before reduce reads state B.
6. Dispatch reduce with groups `1 x 1`, all SIMD16 lanes enabled.
7. Emit the same full producer/consumer boundary before composite reads the
   report.
8. Dispatch composite with groups `16 x 256`, then use the post-sync marker to
   release Spirit's GPU-only producer bit before cursor-plane arming.
9. After retirement, audit report magic/version/frame and all 16 lane markers
   without adding a CPU producer gate, then swap A/B.

The resulting producer path is Spirit Embassy worker -> GPU executor -> vGPU ->
physical GuC scheduler. CUR_SURFLIVE remains the separate display-retirement
proof that completes the Spirit fence; no CPU producer bit participates in the
continuous stream's GPU-only producer gate.

## Capability and privilege boundary

Making the artifact more capable should not mean granting it arbitrary kernel
memory or MMIO. The useful privilege is a narrow, trusted Shell2 service:

- fixed 256x256 dispatches only;
- five exact buffers with separately validated sizes and stable PPGTT VAs;
- hash-allowlisted instruction image and compile-time entry offsets;
- CPU writes only the control page and reads only the report page;
- Spirit snapshots at most the kernel cursor store's bounded 32 records once
  per submitted frame and exposes only one clamped 2D coordinate to the shader;
- alpha changes only per-pixel coverage inside Spirit's existing premultiplied
  cursor backbuffer; it grants no additional MMIO or plane ownership;
- the shader cannot select addresses, pitches other than the validated RGBA8
  pitch, dispatch dimensions, or another display plane;
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

Physical-pointer snapshots map only to reaction-injection coordinates. A small
follow-up command may set
only named, clamped values such as `feed`, `kill`, `flare-radius`,
`flare-turbulence`, `flare-ray-gain`, and `flare-pulse-speed`; raw addresses and
dispatch sizes should never be user-facing.
