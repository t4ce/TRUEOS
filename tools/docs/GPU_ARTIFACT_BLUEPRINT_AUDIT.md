# GPU artifact / Blueprint ownership audit

Status: inventory and migration decision, 2026-09-19.

This is a static repository scan plus migration record. It does not bake, sign,
launch, deploy, or contact the hardware rig.

## Decision in one page

The existing Shadertoy path is the right ownership model:

```text
Blueprint owns source + translated source + SPIR-V + native GPU binary
        -> Blueprint packages the complete payload
        -> TRUEOS kernel keeps ABI contract/trust metadata and the hash
        -> Blueprint registers the payload before dispatch
```

The kernel should retain only artifacts that are part of the OS rendering,
UI, font, media, boot-probe, or model-service boundary, or that are currently
shared by more than one independent Blueprint without a package-registration
seam.

The first clean migration target, `skybox_sample_rgb565`, is now migrated: it
has one Blueprint consumer (`skybox`), a Blueprint-owned authenticated package,
and a dedicated registration API. The next target remains Cubes' generated
patch-cube pipeline, which needs its custom render-package registration seam.
The second target is Cubes' generated patch-cube pipeline, but it needs a
custom render-package registration seam before the bytes can leave the kernel.

## Scope and totals

| Class | Current location | Count / size | Interpretation |
|---|---|---:|---|
| AOT GPGPU payloads | `crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/*.bin` + `*.spv` | 21 pairs / 2,948,332 bytes | Actually embedded by `src/intel/gpgpu/kernel_catalog.rs`; skybox is Blueprint-owned; stale chart/plasma/Mandelbrot preview payloads retired |
| Contract-only shader slots | TRUEOS contract/trust files | 7 | Payload is already supplied by a Blueprint: Shadertoy IDs 1–6 and Cubes ID 16 |
| Native render artifact | `picasso/churn-forward.trueos.intel.helio` | 44,742 bytes | Kernel render boundary; shared by retained-render consumers |
| Small retained texture shader binaries | `picasso/picasso-retained-textured-forward/*.bin` | 3 / 904 bytes | Kernel vGPU retained-texture boundary |
| Code-native shared shaders | `generated_triangle.rs`, `generated_adjacency_gs.rs`, `generated_clip_position3_uv_texture.rs` | 4 / 1,648 bytes of code words | Shared vGPU/render packages, not app-specific |
| Code-native Cubes patch pipeline | `generated_patch_cube.rs` | 36,000 bytes of shader code words | App-specific today, but currently linked into TRUEOS |
| Unavailable font tessellation placeholder | `generated_font_patch.rs` | 0 executable bytes | Not a current GPU artifact; fail-closed placeholder |

The AOT total excludes the seven contract-only Blueprint payloads. It also
does not count source, manifests, contracts, or package framing bytes.

## AOT payload inventory and ownership decision

All rows below use C++ for OpenCL (`.clcpp`) as the maintained source language
unless stated otherwise. `bin` is the device-native ADL-S/Zebin payload and
`spv` is the validation/input payload. The full hash remains authoritative in
the generated `.contract.rs`; the table records the current native-binary hash
so a migration can carry it unchanged.

| Artifact | Current source | bin + spv bytes | Current dispatch boundary | Blueprint multiplicity | Decision |
|---|---|---:|---|---|---|
| `copy_rect_rgba8` | `gpgpu/kernels/copy_rect_rgba8.clcpp` | 11,328 + 4,788 | TRUEOS 2D primitives, probes, OpenCL registry | Kernel/shared | Keep builtin-shader |
| `subset_sum_collapse5_merge10` | `gpgpu/kernels/subset_sum_collapse5_merge10.clcpp` | 13,320 + 6,488 | Boot subset-sum probe | Kernel-only | Keep builtin-shader |
| `fill_rect_rgba8` | `gpgpu/kernels/fill_rect_rgba8.clcpp` | 7,928 + 2,864 | TRUEOS 2D surface primitives and vGPU support | Kernel/shared | Keep builtin-shader |
| `gradient_rect_worklist_rgba8` | `gpgpu/kernels/gradient_rect_worklist_rgba8.clcpp` | 18,896 + 11,980 | TRUEOS worklist/OpenCL registry | Kernel/shared | Keep builtin-shader |
| `alpha_blend_worklist_rgba8` | `gpgpu/kernels/alpha_blend_worklist_rgba8.clcpp` | 27,440 + 16,656 | UI4 compositor and worklists | Kernel/shared | Keep builtin-shader |
| `glyph_mask_rgba8` | `gpgpu/kernels/glyph_mask_rgba8.clcpp` | 16,440 + 8,556 | TRUEOS font/2D submission | Kernel/shared | Keep builtin-shader |
| `ui4_nv12_tile64_to_rgba8_frame` | `gpgpu/kernels/ui4_nv12_tile64_to_rgba8_frame.clcpp` | 23,936 + 13,188 | TRUEOS video-frame conversion | Kernel/shared | Keep builtin-shader |
| `sprite_quad_worklist_rgba8` | `gpgpu/kernels/sprite_quad_worklist_rgba8.clcpp` | 54,152 + 33,628 | UI4, font, sprite worklists, display probes | Kernel/shared | Keep builtin-shader |
| `ui4_compose_layers_rgba8` | `gpgpu/kernels/ui4_compose_layers_rgba8.clcpp` | 40,120 + 26,032 | UI4 compositor | Kernel/shared | Keep builtin-shader |
| `mandel64_worklist_rgba8` | `gpgpu/kernels/mandel64_worklist_rgba8.clcpp` | 21,728 + 14,280 | Former TRUEOS effect preview/worklist path | Kernel/internal | Retired stale preview kernel; source and AOT removed |
| `skybox_sample_rgb565` | `TRUEOS-Blueprints/apps/skybox/assets/skybox_sample_rgb565/` | 37,624 + 21,292 | `Frame::render_skybox_rgb565`; `apps/skybox` | One Blueprint | Migrated; contract/hash retained in TRUEOS |
| `chart_sine_rgba8` | `gpgpu/kernels/chart_sine_rgba8.clcpp` | 35,328 + 21,132 | Former TRUEOS effect preview/OpenCL path | Kernel/internal | Retired stale preview kernel; source and AOT removed |
| `pixel_plasma_rgba8` | `gpgpu/kernels/pixel_plasma_rgba8.clcpp` | 36,544 + 23,632 | Former TRUEOS effect preview/OpenCL path | Kernel/internal | Retired stale preview kernel; source and AOT removed |
| `cpp_demo_rgba8` | `gpgpu/kernels/cpp_demo_rgba8.clcpp` | 222,576 + 157,140 | Kernel preview plus Shadertoy IDs 8–14 | Mixed | Keep until kernel preview gets package registration |
| `cpp_audio_visualizer_rgba8` | `gpgpu/kernels/cpp_audio_visualizer_rgba8.clcpp` | 77,592 + 57,800 | Kernel audio preview plus Shadertoy ID 7 | Mixed | Keep until kernel preview gets package registration |
| `particle_craft` | `gpgpu/kernels/particle_craft.clcpp` | 157,536 + 91,764 | `particle` Blueprint API plus Shadertoy ID 15 | Multiple Blueprints | Keep builtin-shader until direct `particle` registration is added |
| `font_instance_rgba8` | `gpgpu/kernels/font_instance_rgba8.clcpp` | 72,896 + 50,160 | TRUEOS font service / GridPaper path | Kernel service/shared | Keep builtin-shader |
| `lfm25_q8_project_packed` | `gpgpu/kernels/lfm25_q8_project_packed.clcpp` | 23,432 + 14,740 | TRUEOS LFM model service | Kernel-only | Keep builtin-shader |
| `kokoro_qgemm_u8_i8` | `gpgpu/kernels/kokoro_qgemm_u8_i8.clcpp` | 24,032 + 10,212 | TRUEOS Kokoro model service | Kernel-only | Keep builtin-shader |
| `kokoro_conv1d_u8_u8` | `gpgpu/kernels/kokoro_conv1d_u8_u8.clcpp` | 29,480 + 15,124 | TRUEOS Kokoro model service | Kernel-only | Keep builtin-shader |
| `font_outline_coverage_r8` | `gpgpu/kernels/font_outline_coverage_r8.clcpp` | 75,048 + 32,536 | TRUEOS SVG/font outline service | Kernel service/shared | Keep builtin-shader |
| `helio_retained_transform` | `gpgpu/kernels/helio_retained_transform.clcpp` | 206,768 + 151,324 | Picasso retained-scene transform path | Multiple retained-render consumers | Keep builtin-shader |
| `lab256_multiphase` | `gpgpu/kernels/lab256_multiphase.clcpp` | 88,344 + 63,320 | Kernel continuous execution / lab preview | Kernel-only | Keep builtin-shader |
| `spirit_vfx_background_rgba8` | `gpgpu/kernels/spirit_vfx_background_rgba8.clcpp` | 109,608 + 71,288 | TRUEOS Spirit window VFX | Kernel service | Keep builtin-shader |
| `spirit_vfx_sprite_rgba8` | `gpgpu/kernels/spirit_vfx_sprite_rgba8.clcpp` | 656,728 + 151,144 | TRUEOS Spirit window VFX | Kernel service | Keep builtin-shader |

Native binary hashes for the AOT rows, in table order, are:

| Artifact | Native binary SHA-256 |
|---|---|
| `copy_rect_rgba8` | `5a271ee6c38bbf7d9d95752924d3cc326a5ed42e6593bc88990f5708ba6a87c1` |
| `subset_sum_collapse5_merge10` | `73df21076d93609f1b277d289d540e068b57adf7486fd5a1daf934fa4d2c2670` |
| `fill_rect_rgba8` | `f7e53ab702a31d578aaa5f2de9c933283a10091dc1ddc549df5d0a4911a6cff2` |
| `gradient_rect_worklist_rgba8` | `a4318e3c62588ec6ff19154be0e49cfbc10c6b082429601515bf3a24edc7a782` |
| `alpha_blend_worklist_rgba8` | `860424362019fd36f320320c7875ccc75b8521339450c729395e25ecff82dda6` |
| `glyph_mask_rgba8` | `c42b1956532beed93ad422ca5aa6c9293a05f68ec36918223872d878fc2006dd` |
| `ui4_nv12_tile64_to_rgba8_frame` | `4e1bd61f292059592b4dc50e79d27c4f099f28c8d6823ab95fdead81c0a418e1` |
| `sprite_quad_worklist_rgba8` | `d4e75acec4af2b707ec7085e4652f9cd4847fcc219fe4199baee86fe490e1fd8` |
| `ui4_compose_layers_rgba8` | `3176d77c68d19c0dabe7f18a86ef48357b6bc7f0b43b2b3b0e4e6d1d055b6b63` |
| `skybox_sample_rgb565` | `437228ca4c4df96b4d2357c5f08b36fe76e4d919f2f90e535177adc4d75c76e7` |
| `cpp_demo_rgba8` | `6dd432e9666035c5d68b6c9fe71abaec72a4e09dfdf7f2c0e9e07043da4e7ab5` |
| `cpp_audio_visualizer_rgba8` | `86cfdb6afdb08538d3d636130e8d4b9020adc499da28d0084efcb4854867ad9c` |
| `particle_craft` | `8b3d026f2129593c9344c01c5f6cd89ecf213dcaa5adf8cd3c843d990783e113` |
| `font_instance_rgba8` | `f89e4fb1a764f4345e2b7a0b71d54c5dd9af1461218c9cb9ff3ca085a369244e` |
| `lfm25_q8_project_packed` | `4451845617f1392118d4104ef2b9f35a0aa466e30a3affbe09c20d17b274055e` |
| `kokoro_qgemm_u8_i8` | `6beab345c67eb085f5b11bb937a319a77035b223f174ebecb9fca134c79acdac` |
| `kokoro_conv1d_u8_u8` | `4ffc7bef37caeda768d71af8be00c22aa5be18f52a171def22f79720385886c6` |
| `font_outline_coverage_r8` | `c3d576d96ecec9e3a42a46a9456350188646e1a1a556b32817fc2d92955897b6` |
| `helio_retained_transform` | `09445c8abb0b8a768f2feea0ea432f6a9dab07ca71db5c18bd037046483c6d31` |
| `lab256_multiphase` | `63ff153f710221a8812d23e5b7466d254dc0befdc97dbca5eb8b0f952d1d6646` |
| `spirit_vfx_background_rgba8` | `73c583b7cfbd0b0d95294380452799840b059ddb607b9eaa0da3d4ba8209a23e` |
| `spirit_vfx_sprite_rgba8` | `a5bca7b15af0bcab952d093a184876fed01fbce4e4c7d55feffaaf55c33ce5f7` |

## Blueprint dispatch map

| Blueprint / consumer | GPU artifact(s) it dispatches | Current payload ownership | Result |
|---|---|---|---|
| `shadertoy` | IDs 1–6, 7, 8–14, 15 | Complete `.stpkg` files in `TRUEOS-Blueprints/apps/shadertoy/assets`; kernel keeps trust contracts; IDs 7/8/15 also have kernel copies for internal preview | Correct reference model; preserve |
| `Cubes` | ID 16 Mandelbox; ID 4 Palette Grid; generated patch-cube pipeline | `.stpkg` files and source live in `Cubes/Cube`; patch-cube native code is exported into TRUEOS `generated_patch_cube.rs` | Keep package ownership; migrate patch pipeline after API seam |
| `skybox` | `skybox_sample_rgb565` | Authenticated `.stpkg` and source/binaries are Blueprint-owned; TRUEOS retains contract/hash and admission | Migrated |
| `particle` | `particle_craft` | Direct API dispatch currently finds the kernel artifact; Shadertoy separately carries a package for ID 15 | Shared/mixed; do not move until direct registration exists |
| `trueos-picasso-example` | Retained render pipeline, `helio_retained_transform`, Picasso native artifact | Kernel render boundary | Shared render stack; keep builtin-shader |
| `Cubes` / `PotatoStamps` indexed UI4 | Builtin vGPU package digests: color, immediate, textured | Kernel-generated code and fixed package allowlist | Multiple consumers; keep builtin-shader |
| `gridpaper`, `solara`, UI4 font services | Font instance, glyph mask, outline coverage, sprite/composition primitives | Kernel font/UI services | App-independent service; keep builtin-shader |
| TRUEOS video/UI/Spirit/model services | NV12 conversion, UI4 composition, Spirit VFX, LFM/Kokoro | Kernel services | Keep builtin-shader |

## Contract-only Blueprint-owned shader slots

These are already the desired end state. TRUEOS has the ABI contract and
trusted hash, but not the executable payload in the kernel image.

| Kernel name | Language / artifact | Blueprint payload |
|---|---|---|
| `shadertoy_mandelbrot` | GLSL source -> C++ for OpenCL -> SPIR-V + Zebin | `TRUEOS-Blueprints/apps/shadertoy/assets/mandelbrot/` |
| `shadertoy_cube_field` | GLSL source -> C++ for OpenCL -> SPIR-V + Zebin | `.../shadertoy/assets/cube_field/` |
| `shadertoy_nguyen` | GLSL source -> C++ for OpenCL -> SPIR-V + Zebin | `.../shadertoy/assets/nguyen/` |
| `shadertoy_palette_grid` | GLSL source -> C++ for OpenCL -> SPIR-V + Zebin | `.../shadertoy/assets/palette_grid/`; duplicated by Cubes because Cubes dispatches it too |
| `shadertoy_cosmic_strands` | GLSL source -> C++ for OpenCL -> SPIR-V + Zebin | `.../shadertoy/assets/cosmic_strands/` |
| `shadertoy_protean_clouds` | GLSL source -> C++ for OpenCL -> SPIR-V + Zebin | `.../shadertoy/assets/protean_clouds/` |
| `shadertoy_mandelbox` | Cubes GLSL/C++ source -> SPIR-V + Zebin | `Cubes/Cube/mandelbox/` |

`shadertoy` package sizes are intentionally not added to the kernel AOT
total. The package includes the raw/source side, generated C++,
SPIR-V/native bytes, manifest, contract, and framing. Registration extracts
the authenticated binary and SPIR-V and performs the same ABI/hash admission
as a built-in artifact.

## Migration order

1. **Keep the core table unchanged.** Do not move UI4 primitives, font/media
   service kernels, model kernels, Spirit, Helio transform, boot probes, or
   shared vGPU render packages. They have kernel-owned dispatch or multiple
   consumers.
2. **Skybox migration complete.** Its `.clcpp`, `.bin`, `.spv`, manifest,
   generated contract, and package hash are under the Blueprint. The skybox
   frame admission path registers the package before dispatch; bake/sign
   validation and the contract/hash checks remain unchanged.
3. **Migrate Cubes patch-cube next.** Keep the current source/bake/export
   process, but package the generated vertex/tessellation/fragment stages with
   Cubes and register the package before the retained draw. The current
   `SOURCE_SHA256`, `PALETTE_SHA256`, and contract version remain the build
   inputs.
4. **Resolve mixed artifacts explicitly.** `cpp_demo_rgba8`,
   `cpp_audio_visualizer_rgba8`, and `particle_craft` are not single-app
   artifacts today: they are Blueprint packages for Shadertoy but are also
   reachable through kernel/internal paths. Move them only after those paths
   accept a Blueprint package; otherwise the migration would change dispatch
   behavior.

## Validation invariant

The migration should preserve this invariant for every moved row:

```text
same language/source -> same bake -> same native bytes + SPIR-V
                  -> same contract fields and hashes
                  -> same admission and dispatch behavior
```

Only the ownership and transport change. A build should carry the contract
and expected hashes into TRUEOS; the Blueprint carries the bytes. A missing,
stale, wrong-target, or wrong-hash payload must fail closed exactly as it does
for the existing Shadertoy package path.

## Deliberately excluded from the embedded-runtime table

| Item | Reason |
|---|---|
| `TRUEOS-Blueprints/apps/wgpu-hello-compute/src/shader.wgsl` | Already Blueprint-owned source; it is not included in the TRUEOS kernel image |
| `tools/wgpu-video-mesh/src/shader.wgsl` | Host/tool source, not a Blueprint or kernel payload |
| `crates/trueos-shader/intel_userland_oracle/**` dumps and sentinels | Capture/probe evidence and host utilities, not runtime-embedded application shaders |
| `crates/trueos-shader/host_shader_validation/**` and bake outputs | Validation fixtures, not the runtime artifact graph |
| `generated_font_patch.rs` | Explicit zero-byte unavailable placeholder; no executable GPU artifact is present |

## Scan evidence

- Kernel embedding sites: `src/intel/gpgpu/kernel_catalog.rs`,
  `src/intel/render/resources.rs`, and `src/intel/shader.rs`.
- Dispatch/upload registry: `src/intel/gpgpu/artifacts/metadata.rs`,
  `src/intel/gpgpu/artifacts/uploads.rs`, and `src/intel/opencl/registry.rs`.
- Blueprint catalog: `../../TRUEOS-Blueprints/apps.json` and
  `../../TRUEOS-Blueprints/buildins.json`.
- Existing Blueprint-owned package implementation:
  `../../TRUEOS-Blueprints/apps/shadertoy/build.rs` and
  `../../TRUEOS-Blueprints/apps/shadertoy/src/main.rs`.
- Existing second package implementation: `../../Cubes/build.rs` and
  `../../Cubes/src/background.rs`.
