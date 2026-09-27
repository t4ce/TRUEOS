# WC3 fixed-function GPU draw package, version 1

`python3 tools/wc3-fixed-bake/bake.py` compiles the GLSL VS/PS through the
instrumented Mesa ANV no-op DRM shim for ADL-S 8086:4680. It emits native
SIMD8 VS and SIMD16 PS code and independently decodes both with IGA. The bake
rejects a changed compiler payload, VUE routing, scratch requirement, or target.
It performs no GPU submission. GLSL uses the existing compiler lane; WGSL is
not required by the runtime.

The package identifier is FNV-1a64 over `wc3-fixed-v1:state96:vertex4x4` followed
by the exact vertex and fragment source bytes. Both trueos-v copies must use
`metadata.json`'s identifier. Deploy matching kernel and xpapp packages.

## Input and lifetime

The indexed draw ABI retains its size. This package requires vertex stride 64,
position offset 0, and vertex buffer offset 1536. Bytes 0..1536 hold 96 vec4
state rows; the remaining bytes contain position, normal, primary color and
texture coordinates, each vec4. Indices remain u32 triangle lists. The broker
bounds-checks and snapshots both state and arrays before submission. It appends
state to the resident mesh allocation so the same completion fence protects
both. No GPU pointer comes from the client state.

State rows (all finite float32, matrices column-major):

| Rows | Meaning |
|---|---|
| 0–3 | Modelview |
| 4–7 | Projection |
| 8–11 | Texture matrix |
| 12–15 | Inverse-transpose normal matrix |
| 16 | Global ambient |
| 17–20 | Material ambient, diffuse, specular, emission |
| 21 | Shininess, lighting enable, normalize enable, local viewer |
| 22 | Color material: 0 off, 1 ambient, 2 diffuse, 3 specular, 4 emission, 5 ambient+diffuse |
| 23 | Fog: mode (0 off, 1 linear, 2 exp, 3 exp2), density, start, end |
| 24 | Fog color |
| 25 | Texture environment (0 off, 1 modulate, 2 decal, 3 replace, 4 blend), RGB internal format flag, S/T wrap (0 repeat, 1 legacy clamp, 2 clamp-to-edge) |
| 26 | Texture environment color (currently GL default zero) |
| 27 | XY viewport scale/offset (currently 1,1,0,0) |
| 28 | Depth near/far (currently 0,1), back-face cull enable |
| 29 | Top-left scissor: left, top, exclusive right, exclusive bottom |
| 30 | Base texture width/height, minification mode (0 nearest, 1 linear, 2 nearest/mip-nearest, 3 linear/mip-nearest, 4 nearest/mip-linear, 5 linear/mip-linear), linear magnification flag |
| 31 | Last mip level, alpha-test enable, compare function (never through always = 0–7), reference |
| 32+7i | Light i eye-space position |
| 33+7i–35+7i | Light ambient, diffuse, specular |
| 36+7i | Spot direction xyz, cosine cutoff |
| 37+7i | Constant/linear/quadratic attenuation, spot exponent |
| 38+7i | Light enable, omnidirectional spotlight flag |
| 88 | Blend enable, RGB source factor, RGB destination factor |
| 89 | Alpha source factor, alpha destination factor |

The GPU transforms vertices and normals, evaluates all eight lights, preserves
clip W through clipping/perspective interpolation, combines texture and primary
color, applies fog, and tests/writes shared drawable depth. Scissoring and
back-face culling use native raster state. CPU work packs arrays, computes the
normal matrix once per draw, and submits commands; it does not shade vertices
or rasterize pixels.

## Native contract

VF: four float4 elements, packing 0xffff, no SGVS, input GRF 2/read length 2.
VUE: header/position at slots 0/1, primary/UV/fog at 2/3/4, two 64-byte entries.
SBE: read offset 1, read length 2, explicit identity routing for three attributes.
PS: perspective pixel barycentrics, setup GRF 6, SIMD16, no scratch or push data.
VS BTI1 reads state. PS BTI2 fetches texture texels and BTI3 reads state. Separate
stage binding tables share the state surface, texture surface and RT0.

## Scope and validation

This addresses WC3's reported enabled masks through alpha testing and blending.
Alpha comparison executes in the fragment shader, and each draw's OpenGL blend
factors program the Intel color-buffer state. Two-sided lighting, texgen,
polygon offset, nondefault viewport/depth range, and texture addressing beyond
repeat still fail explicitly. This is not a full OpenGL implementation or an
FPS result.

Run xpapp host tests, `python3 tools/test_wc3_fixed_shader.py`,
`python3 tools/test_clip_position3_uv_texture.py`, and
`python3 tools/test_drawable_depth.py`, then build the kernel and xpapp.
Hardware image correctness and performance require a run of the matching pair.

## Mipmapped filtering

The upload contains the complete authored mip chain, stacked vertically at the
base-level row pitch. Non-mip filters upload only level zero. Levels are checked
for dimensions, format and byte length before submission; missing levels remain
an explicit incomplete-texture frontier. The kernel validates atlas dimensions
and level bounds against the state consumed by the shader.

The GPU calculates LOD from UV derivatives in base-level texel units. It applies
nearest/bilinear filtering within a level and nearest/linear selection between
levels. Integer texel fetches apply repeat, legacy clamp with the default
transparent border, or clamp-to-edge independently on each axis. Atlas row
offsets are applied only after wrapping, so bilinear taps cannot read padding
or another mip. The CPU only copies existing rows into the upload; it does not
filter pixels or generate replacement mip levels.

Magnification crossover and nearest-mip half-level ties follow OpenGL 1.1
sections 3.8.1–3.8.2 of the
[Khronos specification](https://registry.khronos.org/OpenGL/specs/gl/glspec11.pdf).
The compiler now reports sampler_count=0 because texel-fetch messages do not
consume sampler state; their BTI is 2. This changes the package identifier.
