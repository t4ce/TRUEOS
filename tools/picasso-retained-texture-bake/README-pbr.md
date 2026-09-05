# Retained five-map material

`bake_pbr.py` produces a separate ADL-S `8086:4680` artifact in
`picasso/picasso-retained-pbr-forward`. It leaves the old base-color executable
and QuadTexture's reused fragment binary unchanged. The runtime entry is the
generated `pipeline.rs` module.

The shader follows glTF's texture channel and color conventions: base color
and emissive use sRGB surfaces, metallic comes from B, roughness from G,
occlusion from R, and tangent normals from linear RGB. Normal scale affects
X/Y before normalization. AO attenuates indirect lighting. The bitangent is
`cross(normal, tangent.xyz) * tangent.w`; mirrored transforms adjust its sign.
See the [Khronos glTF specification](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html#materials).

Lighting combines a GGX/Smith/Schlick direct BRDF with warm key, cool fill and
rim lights. An analytic studio sky and softbox approximate indirect reflection;
there is no environment-map IBL asset or precomputed BRDF lookup. Tone mapping
and sRGB output encoding target the existing UNORM render surface. The package
supports opaque materials and double-sided normal reversal. Alpha-mask and
blend modes require separate runtime/shader contracts and are not admitted.

## Data contract

Vertices are 48 bytes: position `float3@0`, normal `float3@12`, UV `float2@24`,
and MikkTSpace tangent `float4@32`. The fifth VF element provides starting
instance and instance ID. Packed VF components are `0x000AF377`, so the VS
reads two URB units beginning at GRF2. Its output is six 16-byte VUE slots:
header, position, world position, world normal, UV, and world tangent. Allocate
two 64-byte URB units. SBE reads two 32-byte units at offset one and routes four
attributes in identity order. PS SIMD16 setup starts at GRF6, after the four
barycentric GRFs; it requires no source-depth/W or barycentric-plane extras.

The PS binding table contains nine entries:

| BTI | Resource |
| --- | --- |
| 0 | UNORM render target |
| 1 | Reserved descriptor slot (no shader reads) |
| 2 | Existing 368-byte camera buffer |
| 3 | Base-color texture, sRGB |
| 4 | Metallic/roughness texture, UNORM |
| 5 | Normal texture, UNORM |
| 6 | Occlusion texture, UNORM |
| 7 | Emissive texture, sRGB |
| 8 | Material parameters, 64-byte RAW buffer |

Sampler zero uses linear min/mag filtering, no mip chain, and repeat addressing.
Every texture slot must contain a valid descriptor. Missing textures can use a
valid fallback because the material presence mask selects neutral values.
VS bindings stay reserved/camera/instances/compacted at BTI0/1/2/3.

The material buffer has four 16-byte records:

| Offset | Values |
| --- | --- |
| 0 | Base-color factor RGBA, four floats |
| 16 | Emissive factor RGB and normal scale, four floats |
| 32 | Metallic factor, roughness factor, AO strength, alpha cutoff, four floats |
| 48 | Four u32 flags: X double-sided bit 2; Y texture presence; Z/W zero |

Presence bits are base `1`, MR `2`, emissive `4`, AO `8`, normal `16`.
Alpha cutoff is carried for ABI completeness but unused by this opaque shader.

## Bake and verification

The tool needs `cargo`, the checked-in Naga wrapper, `iga64`, a C compiler,
and instrumented Mesa ANV plus its no-op DRM shim. Apply the existing
`tools/clip-position3-uv-bake/mesa-vs-capture.patch` and `mesa-ps-capture.patch`,
then this directory's `mesa-pbr-capture.patch`, and rebuild ANV. The latter adds
all four VUE/PS attribute mappings. The older capture field named `uv_slot`
means the first generic output; for PBR use `VUE_MAP location2_slot=4` for UV.

```sh
python3 tools/picasso-retained-texture-bake/bake_pbr.py
```

The default Mesa build is `.codex_tmp/trueos-adj-instrumented-rpls/mesa-build`;
`--mesa-build` accepts another instrumented build. The shim prevents hardware
submission. Native bytes come from ANV's explicitly serialized shader record,
not a heuristic search of its outer pipeline cache. The SIMD16 slice starts at
the captured compiler offset and ends after its decoded RT EOT instruction;
Mesa's following alignment padding is excluded. Both instruction counts must
match the compiler assembly, and IGA must decode all five sampler sends.

The checked-in evidence records compiler-selected payloads, descriptor maps,
ISA, sizes and hashes. It proves compilation and the runtime data contract,
not host image rendering or bare-metal image correctness. Hardware admission
and visual validation remain separate steps.
