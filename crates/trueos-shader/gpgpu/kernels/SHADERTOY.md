# ShaderToy reviewed Image catalog

This directory carries a deliberately narrow first ShaderToy integration. The
review inputs are in `shadertoy/*.glsl`; the generated C++ for OpenCL kernels
are `shadertoy_*.clcpp`; and the exact ADL-S artifacts and ABI contracts are in
`artifacts/adls/cpp/`.

Regenerate and reproducibly verify the complete catalog with:

```text
make intel-gpu-bake-shadertoy-cpp
```

The Blueprint ABI admits only catalog IDs 1 through 3 and a pointer-free,
64-byte ShaderToy uniform block. Source text, SPIR-V, Zebin, arbitrary dispatch
geometry, pointers, and GPU virtual addresses remain kernel-owned pending a
broader security analysis.

## One clean map: from Blueprint to pixels

There are deliberately two separate paths.  The **authoring path** creates a
reviewed, immutable artifact; the **frame path** selects one of those artifacts
and dispatches it.  The important boundary is that the Blueprint crosses only
the frame path.

```text
AUTHORING (offline; changes what can be selected)

  shadertoy/<name>.glsl
        |
        | adapter.py / export_kernel.py: GLSL Image pass -> C++ for OpenCL
        v
  shadertoy_<name>.clcpp
        |
        | Clang (spir64) -> LLVM bitcode -> llvm-spirv -> IGC/ocloc
        v
  <name>.spv + <name>.bin (Zebin) + <name>.contract.rs
        |
        | compiled into the kernel catalog
        v
  reviewed runtime catalog: IDs 1, 2, 3

FRAME (runtime; writes one UI4 back buffer)

  Blueprint: window_id + TrueosUi4ShadertoyParamsV1 (64 bytes)
        |
        | guest: OP_BP_UI4_SCENE_SHADERTOY_RENDER (0x11D)
        | host: C ABI directly
        v
  UI4 validates the catalog ID and takes the current write lease
        |
        v
  GPGPU uploads/selects the matching checked Zebin and maps it into PPGTT
        |
        v
  RCS encoder builds interface state, payload, and a GPGPU_WALKER command
        |
        v
  SIMD16 compute kernel writes opaque RGBA8 pixels to the leased back buffer
        |
        v
  post-dispatch marker is polled -> producer release -> compositor may scan out
```

The source files to read in that order are:

| Concern | Starting point | What it answers |
|---|---|---|
| Author a catalog entry | `shadertoy/<name>.glsl` | The ordinary ShaderToy `mainImage` source. |
| Adapt and bake it | `tools/shadertoy-cpp-offline/adapter.py`, `tools/intel-gpu-bakery/bake_adls_cpp_shadertoy.sh` | How an Image pass becomes a target-specific artifact and ABI contract. |
| Blueprint contract | `crates/trueos-v/src/bp_abi.rs` | The exact 16-word / 64-byte request struct. |
| Guest crossing | `src/hv/vmcall.rs` | How the 64-byte payload reaches the host through opcode `0x11D`. |
| UI4 ownership | `src/ui4/blueprint_text.rs` | Validation, back-buffer lease, and release to the compositor. |
| Submission orchestration | `src/intel/gpgpu/operations/shadertoy.rs` | Upload, PPGTT mapping, submit, poll, and quarantine behavior. |
| Actual GPU launch | `src/intel/gpgpu/rcs/shadertoy.rs` | Interface descriptor, bindings, cross-thread payload, and walker geometry. |
| What executes | `shadertoy_<name>.clcpp` and `artifacts/adls/cpp/shadertoy_<name>.contract.rs` | The kernel entry point and the generated ABI it requires. |

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
Blueprint cannot choose a kernel name, target, workgroup geometry, surface,
address, source, SPIR-V, or Zebin.

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
| 2 | `shadertoy_cube_field` | `0d48ef4d170eafe0cec5ae3952abdc6e57e865b195dbc3fc137ca7eb1b25d736` |
| 3 | `shadertoy_nguyen` | `1dbc80b468dd896073dd17c3963a5c7cccf814365e21f040e05a3522fea4cd9c` |

All three contracts are SIMD16 with 96 bytes of cross-thread data, 96 bytes of
per-thread local IDs, and no scratch or SLM. The cube-field artifact exposes
its read-only uniform argument as both a stateless pointer and BTI 1; direct RCS
dispatch binds the same kernel-owned block through both representations.
