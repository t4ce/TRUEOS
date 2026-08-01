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
assets/helio/simple-cube.trueos.intel.helio
```

Run from anywhere:

```sh
tools/helio-build/build-simple-cube.sh
```

The default Helio checkout is the sibling `../Helio`. A different checkout
can supply the real capture with `HELIO_REPO=/path/to/Helio`. Set
`INTEL_DEVICE_ID=0x....` only when the Intel Vulkan compiler device must be
selected explicitly.

The final rename occurs in `assets/helio`, so a failed capture, compile, or
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
assets/helio/churn-forward.trueos.intel.helio
```

Build or only validate it with:

```sh
tools/helio-build/build-churn-forward.sh
tools/helio-build/build-churn-forward.sh --validate-only
```

`make iso` runs the lightweight validation for both checked-in programs before
linking. It does not repeat the hosted Vulkan compilation on every OS build.
Use the explicit build scripts when Helio, WGPU, a shader, or a captured ABI
changes; a failed rebuild never replaces the last validated asset.

The artifact also carries Helio's versioned churn contracts. Helio example 2
uses `scene/churn-v1.bin` for geometry and animation plus
`scene/churn-light-v1.bin` for the original ambient, two point lights, and four
material surface parameters. TRUEOS currently lowers that rig to 24 retained
material/face light batches while preserving one Helio indirect command per
batch and one GuC-scheduled frame. Press `C` in the UI4 window to toggle the
bounded collision-style burst; press it again to return to the procedural
orbit.
