# Helio runtime builds

`build-simple-cube.sh` is the single paved build-time path for the first Helio
program. It captures Helio's real `build_simple_graph`, lowers its wgpu trace
to the pointer-free `HELIOIR` contract, and emits `render/replay-v1.bin` with
Helio/wgpu's exact 20-byte `DrawIndexedIndirectArgs`. The replay plan carries
artifact resource IDs and the source-IR CRC, never GPU addresses or patched
Intel packets. The same build compiles the captured WGSL through the existing
Intel baker, validates every container CRC, replay/IR link, and native shader
hash, then atomically publishes:

```text
picasso/simple-cube.trueos.intel.helio
```

Run from anywhere:

```sh
tools/helio-build/build-simple-cube.sh
```

The default Helio checkout is the sibling `../helio`. A different checkout
can supply the real capture with `HELIO_REPO=/path/to/Helio`. Set
`INTEL_DEVICE_ID=0x....` only when the Intel Vulkan compiler device must be
selected explicitly.

The final rename occurs in `picasso`, so a failed capture, compile, or
validation leaves the previously published runtime artifact intact. To check
the checked-in artifact without rebuilding it:

```sh
tools/helio-build/build-simple-cube.sh --validate-only
```

`build-churn-forward.sh` is the second path and the first genuinely instanced
Helio program. It captures a real hosted frame through Helio's vendored WGPU,
using `libhelio::GpuCameraUniforms`, `GpuInstanceData`, compacted instance
indices, and `DrawIndexedIndirectArgs`. The Intel baker authenticates the
native stages together with their binding-table, vertex-fetch, SGVS, SBE, and
fixed-function ABI before atomically publishing:

```text
picasso/churn-forward.trueos.intel.helio
```

Build or only validate it with:

```sh
tools/helio-build/build-churn-forward.sh
tools/helio-build/build-churn-forward.sh --validate-only
```

`build-gbuffer.sh` is the native compiler baseline for Helio's complete
deferred material path. It feeds the unmodified G-buffer WGSL to vendored
Naga, then creates an Intel Vulkan pipeline with the real two-group bindless
layout, 40-byte vertices, eight color targets, and depth attachment. The
published directory contains hash-bound WGSL, SPIR-V, Intel ISA, assembly, and
pipeline ABI metadata:

```text
picasso/helio-gbuffer/
```

Build or validate it with:

```sh
tools/helio-build/build-gbuffer.sh
tools/helio-build/build-gbuffer.sh --validate-only
```

`make iso` runs the lightweight validation for both checked-in programs and
the G-buffer compiler baseline before linking. It does not repeat the hosted
Vulkan compilation on every OS build.
Use the explicit build scripts when Helio, WGPU, a shader, or a captured ABI
changes; a failed rebuild never replaces the last validated asset.

Both artifacts carry
`scene/retained-transform-template-v1.bin` as `SectionKind::Other`. This is a
canonical, pointer-free retained graph seed: two authored identity transform
operations have already been folded into one static root, and the runtime may
instantiate one dynamic child of that root for every render row. The template
caps the expansion at 4096 rows (4097 nodes including the root) and traversal
at two nodes.

The section is exactly 128 bytes, little-endian:

| Offset | Bytes | Field |
| ---: | ---: | --- |
| 0 | 8 | magic `HRTXFM\0\0` |
| 8 | 2 | version (`1`) |
| 10 | 2 | header bytes (`80`) |
| 12 | 4 | total bytes (`128`) |
| 16 | 4 | flags (`0x0f`: pointer-free, row-major affine 3x4, dynamic child per row, constant run folded) |
| 20 | 4 | affine stride (`48`) |
| 24 | 4 | root-affine offset (`80`) |
| 28 | 4 | root-affine count (`1`) |
| 32 | 16 | fold report: authored constant ops (`2`), constant runs (`1`), emitted affines (`1`), removed ops (`1`) |
| 48 | 20 | row template: children per row (`1`), max rows (`4096`), max nodes (`4097`), traversal depth (`2`), root index (`0`) |
| 68 | 8 | dynamic parent index (`0`) and binding kind (`1`, render-row index) |
| 76 | 4 | reserved zero |
| 80 | 48 | folded root, 12 IEEE-754 `f32` values in row-major 3x4 identity order |

The validator checks every header/report field, the reserved word, the full
identity payload, container kind, length, and CRC on both program paths.

The artifact also carries Helio's versioned churn contracts. Helio example 2
uses `scene/churn-v1.bin` for geometry and animation plus
`scene/churn-light-v1.bin` for the original ambient, two point lights, and four
material surface parameters. TRUEOS currently lowers that rig to 24 retained
material/face light batches while preserving one Helio indirect command per
batch and one GuC-scheduled frame. Press `C` in the UI4 window to toggle the
bounded collision-style burst; press it again to return to the procedural
orbit.

The simple-cube artifact also carries `scene/portal-rooms-v1.bin` for the
`helio_portal_trueos` Blueprint. The
fixed 3,072-byte section describes six portal frames, fourteen materials, and
74 texture-free room objects. Runtime projection clips each room's boxes and
octa-spheres against its portal rectangle and camera depth planes before the
retained Intel batch dispatch; UI4 provides fly-camera control and Tab toggles
the editor-style checkerboard overlay.
