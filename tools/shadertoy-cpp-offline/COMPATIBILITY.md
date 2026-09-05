# ShaderToy compatibility after the textured-geometry bring-up

Local toolchain and source audit: 2026-09-06.

The working PicassoExample and QuadTexture demonstrations make static 2D
texture channels a sensible next ShaderToy milestone. They establish useful
graphics texture infrastructure. They do not yet connect image resources to
the separate ShaderToy compute dispatch.

The five existing entries remain the supported runtime catalog. This audit
does not admit a sixth entry or change the Blueprint ABI.

## Baseline checked locally

The adapter regenerates all five checked-in `.clcpp` files exactly. Each entry
was rebuilt twice in distinct directories using the existing
`adls-shadertoy-cpp-proof.lock.json` and ADL-S C++ profile. For every entry:

- LLVM bitcode, SPIR-V and Zebin were reproducible between the two builds;
- SPIR-V, Zebin and the generated Rust contract matched the checked-in files
  byte for byte;
- the resulting contract retained SIMD16, 96-byte cross-thread data,
  96-byte per-thread data, and zero scratch/SLM.

The 15 adapter tests and the native preview build also passed with
`make -C tools/shadertoy-cpp-offline test all`.

The toolchain was present under `bld/shadertoy-cpp-toolchain/root/usr`; its
Clang, LLVM-SPIRV, ocloc and compiler libraries were selected explicitly for
the audit. Merely invoking the bakery with the current shell's PATH did not
find LLVM-SPIRV. No package installation was needed.

This is offline validation. Runtime success of the five entries and the
textured-geometry examples is user-reported; no new hardware run was made.
Local outputs and catalog comparisons are under `bld/shadertoy-audit/`.

## What the working texture path contributes

| Existing code | Reusable foundation | Remaining ShaderToy work |
|---|---|---|
| `src/gpu/vgpu.rs`: `create_retained_texture`, `resolve_retained_texture` | Immutable decoded RGBA8 textures; principal/device/generation/epoch/carrier validation; an `Arc` that pins the resident resource | A ShaderToy resource request and operation lifetime; explicit mapping into the consuming compute context |
| `src/intel/render/pipeline.rs`: sampled surface and sampler writers | ADL-S 2D surface encoding, nearest repeat, and the PBR linear/sRGB sampler configuration | Compute interface descriptor sampler state and bindings, validated against an actual image kernel contract |
| `src/intel/render/resources.rs`: resident sampled textures | Dimensions, pitch, backing allocation and carrier-specific residency | Keep the source allocation alive through compute completion and quarantine; a graphics GPU address alone is not a compute mapping |
| Existing ShaderToy bakery/catalog/UI4 path | Reviewed executable selection and completed-frame publication | Channel-aware adaptation, artifact contracts, host preview inputs, dispatch, and a reviewed texture-backed entry |

These are implementation reuse points, not evidence that every graphics
texture setting is already supported by ShaderToy. In particular, the current
retained-texture creation path fixes repeat addressing; the graphics PBR path
also selects sRGB decoding for particular material roles. ShaderToy needs its
own explicit channel interpretation, filtering and orientation contract.

## The actual gates today

1. `adapter.py` rejects `iChannel0` through `iChannel3`, channel state, sampler
   types, texture calls and derivatives before compilation.
2. `main.c` allocates and binds only output and the 64-byte uniform block,
   followed by width, height and pitch. The Ubuntu preview has no channel
   image loading or binding path.
3. `TrueosUi4ShadertoyParamsV1` carries only catalog selection and scalar
   uniforms. There are no device or channel resource handles in the request.
4. `src/intel/gpgpu/rcs/shadertoy.rs` allows only output and optional uniform
   buffer bindings. `rcs/payloads.rs` writes zero to the interface descriptor's
   sampler word. The submission maps the kernel and destination, with no
   channel mappings.
5. The C++ bakery's current image-sampling output is not admissible as a
   direct-RCS buffer kernel, as the probe below demonstrates.

## Compiler probe result

The existing `tools/intel-texture-probe/sampler_probe.cl` was compiled as C++
for OpenCL using the local tools and the production ADL-S C++ profile, with
outputs kept outside the runtime catalog. Two follow-up variants used a local
constant sampler and an explicit LOD of zero.

All three reached a Zebin accepted by `ocloc validate`, but failed the TRUEOS
contract audit with `pointer arg 0 has no payload address`. The image is
represented by `.ze_info` as a stateful, read-only `image_2d` argument with
zero payload size. The existing auditor expects a buffer pointer address.

The local-sampler and explicit-LOD variants additionally emitted
`has_stack_calls: true`, an `Intel_Symbol_Table_Void_Program` pseudo-kernel,
and a 131072-byte scratch requirement. They are unsuitable for the existing
zero-scratch catalog even if the image-argument audit were relaxed. This is a
result for these source forms and this local compiler path, not a claim that
the hardware cannot sample images from compute. The lowering must be resolved
and its emitted metadata inspected before adding a compute sampler encoder.

## Recommended first extension

Keep the five-entry V1 path working while developing one static 2D channel
from source through runtime:

1. Produce a minimal image sample with the exact C++ toolchain that has one
   entry point, SIMD16, no stack calls, no scratch and no SLM. Make image and
   sampler metadata explicit in the artifact audit and generated Rust contract.
2. Add a versioned channel contract using opaque device/texture handles. Resolve
   and pin textures for the authenticated Blueprint, validate dimensions and
   format, and establish mappings and completion ownership for the compute
   consumer. Kernel-owned catalog textures could instead provide the first
   fixed proof without extending the public request.
3. Add adapter support and matching preview inputs for one RGBA8 `sampler2D`:
   `iChannelResolution`, normalized `texture` sampling at the single available
   level, `textureSize`, and `texelFetch` at level zero. Define wrap/filter,
   linear versus sRGB interpretation, and upload Y orientation explicitly.
   Reject unsupported LODs or channel types with a precise diagnostic.
4. Add an original asymmetric test texture and one reviewed Image pass. Verify
   channels, corners, texel centers, wrapping and interpolation in the host
   preview, then validate the exact admitted artifact on hardware before
   claiming runtime texture support.
5. Extend the same contract to four static channels and then select additional
   licensed shaders whose dependencies fit it.

## Compatibility classes

| Shader dependency | Status after this audit |
|---|---|
| Procedural, single Image pass within the adapter subset | Supported now, subject to the existing artifact constraints |
| Static 2D color/noise/lookup textures | First extension to pursue; graphics infrastructure is useful, but channel plumbing and compute image support are still required |
| Buffer A-D, previous-frame feedback, temporal accumulation or simulation | Requires a pass graph, intermediate formats, ping-pong resources, channel timing and synchronization |
| Cubemap, 3D texture, mip chains, nonzero `textureLod`, `textureGrad` | Requires the corresponding resource and sampling contracts; not established by the 2D examples |
| `dFdx`, `dFdy`, `fwidth` | Requires derivative semantics in compute, or a separately integrated fragment-stage authoring/dispatch path |
| Audio/video inputs, keyboard textures or sound output | Requires their input producers or output/pass contracts |

Removing the adapter's rejection alone would leave the preview, artifact
auditor and Blueprint dispatcher unable to supply what those shaders require.
