# ShaderToy reviewed Image catalog

The six reviewed shaders are owned by `TRUEOS-Blueprints/apps/shadertoy/assets/`.
Each directory contains `input.glsl`, generated `kernel.clcpp`, `kernel.bin`,
`kernel.spv`, `kernel.manifest.json`, and `kernel.contract.rs`. Each `.stpkg`
bundles those six files. The kernel retains the six `.contract.rs` files here
and the small generated package hash/length catalog in
`src/intel/gpgpu/artifacts/shadertoy_packages.rs`; it embeds no ShaderToy payload.

Regenerate and reproducibly verify the catalog with:

```sh
make intel-gpu-bake-shadertoy-cpp
python3 tools/shadertoy-cpp-offline/test_blueprint_packages.py
```

The bake script uses the existing compiler lock and writes the payloads to the
Blueprint checkout (`TRUEOS_BLUEPRINTS_ROOT` overrides the sibling default).
It compiles an exact C++ staging copy under ignored `bld/shadertoy-blueprint-bake/`
so provenance uses stable relative paths across checkout locations. Packaging
checks that this baked source hash equals the Blueprint's generated C++ hash.
Package generation includes raw source and provenance in the authenticated hash.
`package_blueprint.py --check` verifies the result without updating trust.

## From Blueprint to pixels

1. Offline, GLSL is adapted to C++ for OpenCL, compiled to SPIR-V, and baked to
   an exact-target Intel Zebin and ABI contract. The package includes every input
   and output listed above; LLVM bitcode remains a transient build intermediate.
2. At startup, `Frame::register_shadertoy` transfers each package in chunks of at
   most 2048 bytes through `trueos_cabi_ui4_scene_shadertoy_upload_v1` / guest
   opcode `0x12F`. The kernel derives the Blueprint owner and checks its visual
   window. The total size must equal the kernel's trusted catalog size. Offset
   zero resets staging; subsequent offsets must be contiguous and shader IDs
   must agree. One bounded staging buffer belongs to each window and is released
   on completion, invalid ordering, replacement, or window teardown.
3. On the final chunk, the kernel authenticates its complete, immutable copy.
   The package SHA-256 covers the header, binary, SPIR-V, raw GLSL, generated
   C++, manifest and contract copy. Existing Zebin/SPIR-V hashes, target policy,
   ABI constraints, and ELF entry-range validation are then applied before DMA
   allocation. This remains an exact-byte allowlist, not public-key signing.
4. Approved executable bytes are copied and flushed into kernel-owned DMA
   pages; equal resident artifacts are reused without replacing live code. The
   window gains permission to render that registered catalog ID. Rendering has
   no kernel-embedded or filesystem fallback.
5. Each frame sends the unchanged pointer-free 64-byte uniform block via
   `0x11D`. UI4 checks registration and ownership, takes the leased back buffer,
   maps the executable into PPGTT and dispatches the existing SIMD16 walker.
   Completion fencing and timeout quarantine are unchanged.

Rebuild both the kernel and Shadertoy Blueprint for this transition: an old
Blueprint has no package registration step and rendering fails closed.

The package wire format is eight bytes `STPKG01\0`, followed by six little-endian
u32 lengths, then the six files in the order listed above **with Zebin first,
SPIR-V second, GLSL third, C++ fourth, manifest fifth and contract sixth**.
Kernel slicing uses trusted lengths only after whole-package authentication.
The packed blueprint compresses these files; compressed size is not resident
payload size.

## What Blueprint actually controls

`TrueosUi4ShadertoyParamsV1` is exactly 16 little-endian 32-bit words:

```text
version, shader_id, frame, flags,
time_seconds, delta_seconds, frame_rate, sample_rate,
mouse_x, mouse_y, click_x, click_y,
date_year, date_month, date_day, date_seconds
```

`version` must be `1`, `flags` must be zero, and `shader_id` is one of the
catalog IDs below.  All scalar inputs must be finite; time and delta are
non-negative and frame rate is positive.  `window_id` is a separate argument.
Blueprint supplies authenticated package bytes during registration. It cannot
choose an unreviewed kernel, target, workgroup geometry, surface or GPU address.

The host translates the time-related values into the shader's 64-byte
`ShaderToyUniforms` block:

```text
resolution_time = (surface_width, surface_height, 1.0, time_seconds)
mouse           = (mouse_x, mouse_y, click_x, click_y)
date            = (date_year, date_month, date_day, date_seconds)
timing          = (delta_seconds, frame_rate, sample_rate, float(frame))
```

## The compute launch, unpacked

`direct_rcs_encode_shadertoy_batch` is the point where this becomes a native
Intel compute dispatch.  It first checks that the uploaded artifact matches the
catalog hash and its generated ABI contract.  It then lays out in the RCS batch
allocation:

| Item | Contents |
|---|---|
| Interface descriptor | Kernel text offset, binding-table offset, and cross-thread length. |
| Binding table | UI4 RGBA8 output at BTI 0; cube-field additionally exposes uniforms at BTI 1. |
| Cross-thread payload | Global-offset/local-size values, output and uniform pointers, width, height, and pitch. |
| Per-thread payload | Local IDs for the 16 SIMD lanes. |
| Uniform block | The four `float4` values above. |

The encoder emits the necessary cache stalls/flushes, selects the GPGPU
pipeline, loads the interface descriptor, stores a pre-marker, and emits one
2D walker.  Its geometry is fixed by the leased surface:

```text
local size  = (16, 1, 1)       // required SIMD16 subgroup
groups      = (ceil(width / 16), height, 1)
right mask  = active lanes in the final partial 16-pixel group
```

The generated entry point uses `get_global_id(0)` and `get_global_id(1)` as
pixel `x` and `y`, flips `y` into ShaderToy's bottom-left coordinate convention,
calls `mainImage`, packs the result as opaque RGBA8, and writes directly into
the UI4 back buffer.  A post-marker is written only after the walker and flush;
the CPU polls it before releasing that buffer to the compositor.  A timeout
quarantines the direct RCS context instead of attempting a CPU fallback over a
possibly in-flight buffer.

The current artifacts are:

| ID | Entry point | Zebin SHA-256 |
|---:|---|---|
| 1 | `shadertoy_mandelbrot` | `79e566ad2db01a1a2467e0289bd97e9c77c67be7bd4a59d957dadd84e0ec32d1` |
| 2 | `shadertoy_cube_field` | `04f940ae84746975d6c11033ce7899ccc8307badcaf3091f53a654ca10256f10` |
| 3 | `shadertoy_nguyen` | `7140703571a20d5640876caddbe5948aa84f8828ff1d621b6eae1ef7d67af54d` |
| 4 | `shadertoy_palette_grid` | `2174c3002ff5e0c489de3ea4aff8da5b922b995e6075967a326eeb656e280124` |
| 5 | `shadertoy_cosmic_strands` | `bf7e5b8a590526a36fa9684a4055d9dd255e36cba8b5ab75813ca3b59b4569d4` |
| 6 | `shadertoy_protean_clouds` | `19119f60ffe6a9207bd24d40aecde2b672a27aa253e228f048f36a9c81782696` |

All six contracts are SIMD16 with 96 bytes of cross-thread data, 96 bytes of
per-thread local IDs, and no scratch or SLM. The cube-field and Protean Clouds artifacts expose
its read-only uniform argument as both a stateless pointer and BTI 1; direct RCS
dispatch binds the same kernel-owned block through both representations.
