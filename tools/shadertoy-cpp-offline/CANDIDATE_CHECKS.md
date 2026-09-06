# Protean Clouds and Aiekick sphere candidate checks

Local validation: 2026-09-06. These checks extend the offline adapter; they do
not add catalog IDs or dispatch new shaders on the bare-metal rig.

## Source provenance and page access

The user supplied two sources followed by these URLs:

- https://www.shadertoy.com/view/4lSSDW
- https://www.shadertoy.com/view/4t2SWW
- https://www.shadertoy.com/view/3l23Rh

All three page requests returned access errors; direct HTTP requests returned
403, and no browser session was available. The first pasted source identifies
itself as nimitz's Protean Clouds, `3l23Rh`. The second credits Stephane
Cuillerdier / Aiekick (2015) and retains the supplied CC BY-NC-SA 3.0 notice.
Its `llSSWD` link identifies the underlying 2D effect. The mapping of that
source to either new URL, the other page's source, and the actual channel
assets/settings could not be verified.

Both pasted sources contained Markdown fences, escaped punctuation and, in
Protean, damaged multiplication operators. Reconstructed GLSL is kept under
ignored `bld/shadertoy-candidates/{protean_clouds,aiekick_sphere}/input.glsl`.
These are tests of that reconstruction, not a byte-exact export of the pages.

Protean repairs restore `*` / `*=` in the density accumulation, grid scaling,
`prm1` density terms and `iLerp` expression. In `mainImage`,
`gl_FragCoord.xy` was replaced with the supplied `fragCoord.xy`, using the
adapter's bottom-left pixel-center coordinates. No march counts, lighting,
palette formulas or scene features were removed.

## Protean Clouds: passes the current artifact shape

Protean is a procedural, single Image pass with no channel inputs. The updated
adapter handles its initialized writable globals (`prm1`, `bsMo`), scaled
constant matrix and RGBA swizzle spellings. Matrix scaling keeps matrix
columns intact instead of accidentally selecting a vector overload. Sources
that need private state keep their globals as fields in a per-pixel C++
aggregate.

The reconstructed source passed two builds with the existing locked compiler
and `adls-4680-r0c-cpp.json` profile. LLVM bitcode, SPIR-V and Zebin were
byte-identical across the two build roots; `ocloc validate` and the TRUEOS
artifact audit both passed.

| Property | Result |
|---|---|
| Kernel | `shadertoy_protean_clouds` |
| SIMD width | 16 |
| Scratch / SLM | 0 / 0 bytes |
| Cross-thread / per-thread data | 96 / 96 bytes |
| Bindings | Output at BTI 0, read-only uniforms at BTI 1 |
| Pointer offsets | Output 48, uniforms 56 |
| Scalar offsets | Width 64, height 68, pitch 72 |
| Zebin SHA-256 | `42708149dff28c43df0cc2bdd4147169aec93ecabbc003122c1e76a5b00b7cbc` |

This layout fits the existing ShaderToy output/uniform dispatcher shape. The
candidate still needs catalog integration and validation on the exact
bare-metal target before claiming Blueprint runtime success.

## Protean host rendering

The generated SPIR-V was loaded through the local Intel OpenCL runtime on
UHD 770 (`0xA780`), using the same 64-byte uniforms, opaque RGBA8 packing and
`(16, 1)` local size as the preview. This exercises a driver-compiled host
kernel, not execution of the `0x4680` Zebin on bare metal.

Seven 640x360 frames completed: times 0, 2, 4, 6 and 8; time 0 with moved mouse
input; and time 0 again with the original inputs. Animation and mouse input
changed the pixels. The repeated time-0 RGBA frame was byte-identical. Saved
images at times 0 and 6 were visually inspected and show the cloud volume.

Warm dispatch-plus-readback samples took approximately 95–110 ms, with the
first frame around 123 ms. These are diagnostic samples including readback,
not a sustained frame-rate benchmark.

Evidence under `bld/shadertoy-candidates/protean_clouds/`:

- `bake.log` and `bakery/adls/cpp-native/shadertoy_protean_clouds/run-a/`
- `host-640/proof.log`
- `host-640/frame-0.png` through `host-640/frame-6.png`

The headless test source is `bld/shadertoy-candidates/headless_probe.c`.

## Pasted Aiekick sphere: resource support required

The reconstructed GLSL passed `glslc` syntax compilation with explicitly
declared `samplerCube iChannel0` and `sampler2D iChannel1`. Those types are
inferred from the source's environment directions and 2D sampling coordinates;
the page configuration is still unavailable. This syntax-only fragment
compilation is not a TRUEOS C++ bake or a rendered image proof.

The actual TRUEOS adapter rejects the source at its channel boundary.

| Input / operation | What the pasted source requires |
|---|---|
| `iChannel0` | Environment lookup using reflected, refracted and background 3D directions; expected to be a cubemap |
| `iChannel1` | A 2D image sampled inside the displacement function |
| `textureLod(iChannel1, ..., 4*(sin(t)*.5+.5))` | A continuous requested LOD from 0 to 4, so the mip chain and mip filtering settings matter |
| Resource configuration | Actual images/cubemap faces, wrap/filter modes and orientation from the source page |

The texture-driven displacement is evaluated in ray marching, normals and
ambient occlusion. Substituting an arbitrary image or forcing LOD zero would
not establish the supplied shader's appearance. Single-level 2D channel
support alone would not cover this candidate. No texture substitutions or
rendering claims were made, and no scratch/ABI result can be inferred before
the real channel-aware kernel exists.

## Regression checks

All 24 adapter tests passed, and all five existing catalog sources still
regenerate byte-identical `.clcpp` files. Their executable artifacts were not
changed.

A small initialized-state/matrix/RGBA fixture also passed the locked,
reproducible zero-scratch bake and seven host GPU frames. Every pixel matched
the expected values, checking per-pixel and per-frame initialization, matrix
column scaling/orientation, RGBA aliases, and uniform-dependent changes.
Fixture evidence is under `bld/shadertoy-candidates/adapter-semantics/`.

## Runtime catalog follow-up

Protean Clouds is now ID 6 / F6 in the Shadertoy Blueprint. Its raw source,
generated C++, binary, SPIR-V and provenance live in
`TRUEOS-Blueprints/apps/shadertoy/assets/protean_clouds/` and are bundled with
kernel-authenticated package metadata. The ABI remains SIMD16, zero scratch/SLM,
with 96-byte cross-thread and per-thread payloads. Bare-metal visual confirmation
of this new entry remains distinct from the recorded host preview.

Hex Array Pulse and the Aiekick sphere sources have moved into the Blueprint's
`assets/candidates/` tree, with status files explaining their missing runtime
support. The user accepts representative textures for the sphere; exact original
asset selection is no longer a prerequisite. Scratch and compute channel support
remain unimplemented, so these two are not selectable runtime entries.
