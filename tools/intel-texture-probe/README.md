# Intel texture and cloud-volume bring-up probes

This directory isolates sampler operations that the older TRUEOS image paths
did **not** prove.  The probes are offline compiler oracles first; bare-metal
execution is admitted only after the generated target package and fixed-function
state have been reviewed for the exact physical GPU.

## Existing 2D texture ladder

Picasso keeps the texture ladder separate from application content:

1. the existing constant-colour indexed draw proves raster, Render0 and UI4;
2. a texture can be resident and bound without being dereferenced;
3. a fixed texel load proves the sampled surface and sampler message;
4. a filtered sample proves interpolation and filter state;
5. only then does Picasso admit filtered sampled materials.

`sampler_probe.cl` is the small 2D compiler oracle. Compile it for both the
physical ADL-S `0x4680` and the hosted RPL-S target and compare their generated
sampler sends. A graphics package executed on ADL-S must be baked for ADL-S; a
successful RPL-S bake is not accepted as proof of ISA compatibility.

## Cloud RGBA16F 3D ladder

`cloud_volume_probe.clcpp` makes the browser/native cloud divergence explicit
without changing the live driver yet.  It contains two SIMD16 C++ for OpenCL
entry points with the **same normalized coordinate and float4 output contract**:

- `cloud_volume_buffer_sample` reads linear `half4` / RGBA16F storage and uses
  `cloud_volume_sampling.hpp` for software trilinear filtering. This is already
  compatible with TRUEOS's established stateful/stateless buffer dispatch.
- `cloud_volume_image_sample` reads `image3d_t` with normalized coordinates,
  clamp-to-edge addressing and linear filtering. This is the desired one-sampler
  Intel path once direct RCS can safely program a 3D sampled surface and sampler
  state.

The intended migration is therefore not two different cloud implementations:

```text
persistent RGBA16F volume A/B
          |
          +-- today: linear buffer -> software trilinear
          |
          `-- next:  3D SURFACE_STATE -> Intel sampler message

same normalized sample coordinates
same simulation ownership
same JSON-derived parameters
same render math
```

The host-side `GpgpuRgba16FloatVolume3d` contract lives in
`src/intel/gpgpu/types/volumes.rs`. It records width, height, depth, row pitch
and slice pitch explicitly, so a later sampled-surface binding cannot silently
reinterpret the linear simulation allocation.

For the cloud reference extent `96 x 48 x 96`, one tightly packed RGBA16F
volume is 3,538,944 bytes and the ping-pong pair is 7,077,888 bytes (6.75 MiB).

Before enabling the hardware path on bare metal, compare the two probe kernels
on a deterministic RGBA16F volume at coordinates that cover texel centers,
half-texel interpolation, all six faces and slightly out-of-range values. The
results should agree within binary16 input precision. Then inspect the hardware
entry point's `.ze_info` and GEN assembly and add only the fixed image/sampler
resource facts that IGC actually emitted.

The eventual bare-metal sampler probe must run only when explicitly selected.
A non-retiring sampler command can wedge the shared Render0 engine until reboot,
so this change deliberately does **not** guess sampler-state or 3D surface-state
bitfields in the direct-RCS encoder.
