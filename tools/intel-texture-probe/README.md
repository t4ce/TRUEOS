# Intel texture-to-mesh bring-up probe

This directory isolates the first GPU operation that the older TRUEOS image
paths did **not** prove: an Intel shader sampler reading a texture while a
graphics workload is active.

The stages are intentionally kept separate from HelioV's world renderer:

1. the existing constant-colour indexed draw proves raster, Render0 and UI4;
2. a texture can be resident and bound without being dereferenced;
3. a fixed texel load proves the sampled surface and sampler message;
4. a filtered sample proves interpolation and filter state;
5. only then does HelioV enable the full voxel material shader.

`sampler_probe.cl` is an offline compiler oracle, not an alternate renderer.
Compile it for both the physical ADL-S `0x4680` and the hosted RPL-S target and
compare their generated sampler sends. A graphics package executed on ADL-S
must be baked for ADL-S; a successful RPL-S bake is not accepted as proof of
ISA compatibility.

The eventual bare-metal probe must run only when explicitly selected because a
non-retiring sampler command wedges the shared Render0 engine until reboot.
