# TRUEOS vs Mesa gfx125 Trivial Path

Purpose: give a direct checklist for comparing the current TRUEOS draw path
against the host-validated Mesa `genX_simple_shader` trivial triangle path.

Primary references:
- TRUEOS packet builder: `/home/t4ce/REPOS/TRUEOS/src/intel/render.rs`
- Mesa simple path: `/home/t4ce/REPOS/TRUEOS/src/intel/reference/mesa/src/intel/vulkan/genX_simple_shader.c`
- Host state summary: `/home/t4ce/REPOS/TRUEOS/.codex_tmp/host_shader_validation/pipeline_exec/host_state_reference.txt`
- Host VS assembly: `/home/t4ce/REPOS/TRUEOS/.codex_tmp/host_shader_validation/pipeline_exec/00_vertex_01_GEN_Assembly.txt`

## Current status

Clean TRUEOS draw-path boot still shows:
- IA vertices = 3
- IA primitives = 1
- VS invocations = 3
- CL invocations = 0
- PS invocations = 0
- render target unchanged
- engine parked at post-draw `PIPE_CONTROL`

That still places the failure window at post-VS / pre-clipper-visible progress.

## Side-by-side checklist

| Area | Mesa host reference | TRUEOS current | Status | Note |
|---|---|---|---|---|
| Platform target | `gfx125` / ADL-S public target | `gfx125` / device `0x4680` | Match | Public/open target exists already |
| Topology | `triangle_list` | `trilist` | Match | Real draw path, no longer pointlist-only |
| VS dispatch | `simd8` | `Simd8` | Match | Seen in logs |
| VS URB output length | `1` | `1` | Match | Override removed |
| CLIP perspective divide disable | `1` | `1` | Match | Patched and confirmed in logs |
| RASTER cull mode | `none` | `none` | Match | Logs agree |
| SBE read offset | `1` | encoded in `sbe=0x30200820` | Likely match | Log decodes read offset = 1 |
| SBE read length | `1` | `1` | Match | Logs agree |
| SBE force read offset | `1` | encoded in `sbe_dw1` | Likely match | Needs exact packet decode if still suspicious |
| SBE force read length | `1` | encoded in `sbe_dw1` | Likely match | Same as above |
| SBE num SF attrs | `0` | `0` | Match | PS varyings = 0 |
| SBE active components | `xyzw` | `0xFFFF_FFFF` masks | Match-intent | Same broad intent |
| PS vector mask | `0` | `uses_vmask = false` | Match | `ps3=0` in logs |
| PS binding table entry count | `0` effective | `0` effective | Match | Metadata count 1 encodes to 0 for PS packet |
| PS push constants | `0` | `0` | Match | Logs agree |
| PS dispatch | `simd8` | `Simd8` | Match | Logs agree |
| PS extra | valid=1, attr=0, per_sample=0, depth=0, stencil=0 | `ps_extra=0x80000000` | Match | Logs agree |
| PS blend | `HasWriteableRT = 1` | `1 << 30` | Match | Same key bit |
| Blend-state payload | boring zeroed state | attempt 1 now `mesa-zeroed` | Pending next boot | Clean boot should now test this first |
| Blend-state pointer | normal pointer | attempt-dependent | Pending next boot | Attempt 3 removes pointer entirely |
| SF DerefBlockSize | explicitly set from `urb_cfg.deref_block_size` | not modeled, `sf_dw1/sf_dw2/sf_dw3` only | Mismatch | Best remaining concrete state mismatch |
| FF_DOP clock gate WA | enabled in Linux/Xe via `CS_DEBUG_MODE1[1]` | enabled and read back as set | Match | Patched and confirmed |

## Most important remaining mismatch

Mesa simple path does this on gfx12+:

```c
anv_batch_emit(batch, GENX(3DSTATE_SF), sf) {
   sf.DerefBlockSize = urb_cfg.deref_block_size;
}
```

Relevant Mesa logic:
- if VS is last enabled shader and VS URB handles < 192, use `PER_POLY`
- enum value `INTEL_URB_DEREF_BLOCK_SIZE_PER_POLY = 1`

For this trivial path, Mesa's URB config code strongly suggests `DerefBlockSize = 1`
is the expected gfx125 setting.

TRUEOS currently emits:

```rust
let sf_dw1 = (1 << 1) | (1 << 10);
let sf_dw2 = 0;
let sf_dw3 = 0;
```

and does not model `DerefBlockSize` at all.

## Why this file exists

This is the shortest public diff list that still looks technically alive after
the recent probes. A lot of previous suspects have already been tested and shown
not to change the failure shape:
- VS URB output length override
- streamout declaration layout
- post-draw flush bits
- clip perspective divide disable
- FF_DOP clock gate workaround

## Next boot expectations

Because attempt ordering was changed, the next clean boot should show:
- `blend_probe=mesa-zeroed` on attempt 1

If that still fails with the same counters, the next small patch worth trying is:
- model `3DSTATE_SF.DerefBlockSize` to match Mesa's gfx125 trivial path
