# Xe-LP Triangle Bake Format

This file defines the exact contract between the offline shader bake step and
the in-kernel import in [src/intel/shader/generated.rs](/home/t4ce/REPOS/TRUEOS/src/intel/shader/generated.rs).

The goal is narrow:

- one baked vertex shader
- one baked fragment shader
- one fixed triangle pipeline
- no runtime compilation
- no general shader asset system yet

## Output Shape

The offline exporter should emit one Rust source file that replaces the
placeholder contents of [src/intel/shader/generated.rs](/home/t4ce/REPOS/TRUEOS/src/intel/shader/generated.rs).

The generated file must define these symbols:

```rust
pub(crate) const TRIANGLE_PIPELINE_NOTE: &str = "...";

static TRIANGLE_VS_CODE: [u32; N] = [ ... ];
static TRIANGLE_PS_CODE: [u32; M] = [ ... ];

static TRIANGLE_VS: BakedVertexShader = BakedVertexShader { ... };
static TRIANGLE_PS: BakedFragmentShader = BakedFragmentShader { ... };

static TRIANGLE_PIPELINE: TrianglePipeline = TrianglePipeline {
    vs: &TRIANGLE_VS,
    ps: &TRIANGLE_PS,
    vertex_stride_bytes: TRIANGLE_VERTEX_STRIDE_BYTES as u32,
    vertex_count: 3,
    rt_binding_table_index: 0,
};

pub(crate) fn triangle_pipeline() -> &'static TrianglePipeline {
    &TRIANGLE_PIPELINE
}
```

## Code Array Rules

- `TRIANGLE_VS_CODE` and `TRIANGLE_PS_CODE` are the exact uploaded stage code
  payloads.
- The arrays are expressed as `u32` words in native little-endian order.
- `code_size_bytes` means the exact uploaded blob size in bytes.
- `code_size_bytes` must equal `code.len() * 4` exactly.
- Empty stage blobs are invalid for real bring-up.
- The runtime validates this and rejects mismatches.

## Addressing Rules

The runtime uploads stage code into the draw-state BO rooted at GPU address
`GPU_VA_DRAW_STATE_BASE`.

Each stage metadata block must provide:

- `code_offset_bytes`: byte offset from the draw-state BO base to the start of
  the stage code blob
- `ksp_offset_bytes`: byte offset from the start of that stage code blob to the
  hardware kernel start pointer entry point

`ksp_offset_bytes = 0` is valid and means the kernel entry point is at the
first byte of the uploaded stage blob.

The runtime computes:

- `stage_code_gpu = GPU_VA_DRAW_STATE_BASE + code_offset_bytes`
- `stage_ksp_gpu = stage_code_gpu + ksp_offset_bytes`

So the exporter must treat `ksp_offset_bytes` as relative to the stage blob, not
relative to the BO.

## Runtime Assumptions

The runtime side does not relocate or repack stage code.

- the exporter decides `code_offset_bytes`
- the exporter decides `ksp_offset_bytes`
- the exporter decides the stage ordering inside the uploaded BO
- the runtime validates those offsets and uploads the bytes as-is

In other words, generated offsets are authoritative. The runtime is only a
validator and uploader.

## Packing Constraints

These constraints are enforced by the runtime in
[src/intel/render.rs](/home/t4ce/REPOS/TRUEOS/src/intel/render.rs):

- `code_offset_bytes % code_alignment_bytes == 0`
- `code_alignment_bytes != 0`
- `code_size_bytes != 0`
- `ksp_offset_bytes < code_size_bytes`
- `ksp_offset_bytes % 64 == 0`
- VS and PS code ranges must not overlap
- both stage ranges must fit in `warm.draw_state_len`

Conservative first values are:

- `code_alignment_bytes = 64`
- VS at `code_offset_bytes = 0x0000`
- PS at `code_offset_bytes = 0x0400` or the next 64-byte aligned offset after
  the VS blob

The runtime reserves the next 4 KiB aligned offset after the uploaded shader
bytes as the future fixed-function state region.

## Metadata Field Units

### ShaderKernelMetadata

- `ksp_offset_bytes`: bytes, relative to the start of this stage blob
- `code_offset_bytes`: bytes, relative to the draw-state BO base
- `code_size_bytes`: bytes
- `code_alignment_bytes`: bytes
- `ksp_offset_bytes` must be 64-byte aligned
- `grf_start_register`: GRF index as programmed into stage packets
- `dispatch_mode`: one of `DispatchMode::Simd8`, `DispatchMode::Simd16`, or
  `DispatchMode::Simd32`
- `sampler_count`: sampler table entry count
- `binding_table_entry_count`: required binding table entry count
- `push_constant_bytes`: bytes
- `grf_used`: GRF count in hardware GRF units
- `grf_used` must be nonzero for real baked shaders

### VertexShaderMetadata

- `urb_entry_output_length`: 64-byte URB units
- `max_threads`: raw hardware thread-count field value intended for
  `3DSTATE_VS`
- `max_threads` must be nonzero for real baked shaders

### FragmentShaderMetadata

- `num_varying_inputs`: 16-byte attribute slots consumed by the PS payload
- `flat_inputs`: bitmask indexed by varying slot
- `uses_vmask`: boolean
- `computed_depth_mode`: raw hardware value for PS extra state
- `computed_stencil`: boolean
- `persample_dispatch`: boolean

## First Triangle Expectations

For the current bring-up shaders:

- VS input: one vertex attribute, `vec3 position`
- VS output: clip-space position only
- PS input: no varyings required for constant-color output
- PS output: one RT0 color write
- no samplers
- no textures
- no push constants
- one binding-table entry for the render target on the PS side

That means the first exporter should usually emit roughly:

- VS `binding_table_entry_count = 0`
- VS `sampler_count = 0`
- VS `push_constant_bytes = 0`
- PS `binding_table_entry_count = 1`
- PS `sampler_count = 0`
- PS `push_constant_bytes = 0`
- PS `num_varying_inputs = 0`

## Provenance Note

`TRIANGLE_PIPELINE_NOTE` should record enough provenance to debug stale blobs.
Recommended contents:

- compiler/tool used
- target platform, for example `gfx125`
- shader hashes
- a visible build or bake stamp
- bake date
- whether metadata is provisional or verified on hardware

Example:

```rust
pub(crate) const TRIANGLE_PIPELINE_NOTE: &str =
  "mesa-brw offline bake target=gfx125 build=example vs_sha=... ps_sha=... date=2026-04-09 verified=0";
```

## First Real Bake Checklist

Before replacing the placeholder in
[src/intel/shader/generated.rs](/home/t4ce/REPOS/TRUEOS/src/intel/shader/generated.rs), verify:

- VS blob is non-empty
- PS blob is non-empty
- VS `code_offset_bytes` is 64-byte aligned
- PS `code_offset_bytes` is 64-byte aligned
- VS `ksp_offset_bytes` is 64-byte aligned
- PS `ksp_offset_bytes` is 64-byte aligned
- VS `code_size_bytes == TRIANGLE_VS_CODE.len() * 4`
- PS `code_size_bytes == TRIANGLE_PS_CODE.len() * 4`
- VS and PS code ranges do not overlap
- VS `grf_used > 0`
- PS `grf_used > 0`
- VS `max_threads > 0`
- PS `binding_table_entry_count == 1`
- PS `num_varying_inputs == 0`
- `TRIANGLE_PIPELINE_NOTE` includes target and hashes

## Minimal Generated Example

This example is shape-only. The machine code values are not real, and the
metadata values below are intentionally nonzero placeholders so readers do not
copy a zeroed example into `generated.rs` and trip the real-shader validation
rules.

```rust
static TRIANGLE_VS_CODE: [u32; 4] = [
    0x00000000,
    0x00000000,
    0x00000000,
    0x00000000,
];

static TRIANGLE_PS_CODE: [u32; 4] = [
    0x00000000,
    0x00000000,
    0x00000000,
    0x00000000,
];

static TRIANGLE_VS: BakedVertexShader = BakedVertexShader {
    code: &TRIANGLE_VS_CODE,
    meta: VertexShaderMetadata {
        kernel: ShaderKernelMetadata {
            ksp_offset_bytes: 0,
            code_offset_bytes: 0x0000,
            code_size_bytes: 16,
            code_alignment_bytes: 64,
            grf_start_register: 0,
            dispatch_mode: DispatchMode::Simd8,
            sampler_count: 0,
            binding_table_entry_count: 0,
            push_constant_bytes: 0,
            grf_used: 16,
        },
        urb_entry_output_length: 1,
        max_threads: 1,
    },
};

static TRIANGLE_PS: BakedFragmentShader = BakedFragmentShader {
    code: &TRIANGLE_PS_CODE,
    meta: FragmentShaderMetadata {
        kernel: ShaderKernelMetadata {
            ksp_offset_bytes: 0,
            code_offset_bytes: 0x0040,
            code_size_bytes: 16,
            code_alignment_bytes: 64,
            grf_start_register: 0,
            dispatch_mode: DispatchMode::Simd8,
            sampler_count: 0,
            binding_table_entry_count: 1,
            push_constant_bytes: 0,
          grf_used: 16,
        },
        num_varying_inputs: 0,
        flat_inputs: 0,
        uses_vmask: false,
        computed_depth_mode: 0,
        computed_stencil: false,
        persample_dispatch: false,
    },
};
```

The values above are only a structural example. They are not valid shader
machine code and are not expected to execute.