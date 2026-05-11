# TRUEOS vs Mesa gfx125 Simple-Shader Comparison

Purpose: compare TRUEOS's emitted draw-path packets against the public Mesa gfx125/simple-shader path and the local host validation artifacts, focusing on the runtime contracts most likely to explain `VS_INVOCATION_COUNT > 0` with `CL_INVOCATION_COUNT == 0`.

## Public / local references

- Host state summary: [.codex_tmp/host_shader_validation/pipeline_exec/host_state_reference.txt](/home/t4ce/REPOS/TRUEOS/.codex_tmp/host_shader_validation/pipeline_exec/host_state_reference.txt)
- Host VS assembly: [.codex_tmp/host_shader_validation/pipeline_exec/00_vertex_01_GEN_Assembly.txt](/home/t4ce/REPOS/TRUEOS/.codex_tmp/host_shader_validation/pipeline_exec/00_vertex_01_GEN_Assembly.txt)
- Mesa simple shader: [src/intel/reference/mesa/src/intel/vulkan/genX_simple_shader.c](/home/t4ce/REPOS/TRUEOS/src/intel/reference/mesa/src/intel/vulkan/genX_simple_shader.c:172)
- Mesa URB setup: [src/intel/reference/mesa/src/intel/vulkan/genX_gfx_state.c](/home/t4ce/REPOS/TRUEOS/src/intel/reference/mesa/src/intel/vulkan/genX_gfx_state.c:3545)
- TRUEOS batch builder: [src/intel/render.rs](/home/t4ce/REPOS/TRUEOS/src/intel/render.rs:1738)

## Host facts we should trust

- Host state says `target=gfx125`.
- Host state says `clip perspective_divide_disable=1`.
- Host state says `sbe read_offset=1 read_length=1 num_sf_attrs=0 force_read_offset=1 force_read_length=1`.
- Host state says `ps dispatch=simd8 max_threads_per_psd=63`.
- Host VS assembly ends with a real URB `send ... EOT`, so the imported VS is performing a genuine VUE export.

## Packet-by-packet comparison

### 3DSTATE_URB_ALLOC_VS

- Mesa contract:
  `VSURBEntryAllocationSize = urb_cfg->size[i] - 1`
- TRUEOS now:
  `programmed_vs_urb_output_length.saturating_sub(1)`
- Status:
  matched after the recent fix in [src/intel/render.rs](/home/t4ce/REPOS/TRUEOS/src/intel/render.rs:1932)
- Why it matters:
  gfx12 encodes this field as "64B units minus 1"; a raw `1` for a one-slot VUE is wrong.

### 3DSTATE_VS

- Mesa/host expectation:
  a position-only VUE export with one 64B URB slot is valid for this path.
- TRUEOS now:
  `dw8 = programmed_vs_urb_output_length << 16`, with `programmed_vs_urb_output_length = 1`
- Status:
  looks consistent with the host `sbe read_length=1` and the VS assembly's URB export.
- Remaining question:
  if clipper still does not start, we should decode `3DSTATE_VS` field-by-field against Mesa pack output, not just by the few fields we currently log.

### 3DSTATE_CLIP

- Mesa simple-shader explicitly sets:
  `PerspectiveDivideDisable = true`
- Host summary says:
  `clip perspective_divide_disable=1`
- TRUEOS now:
  `clip_dw1 = statistics + early_cull`
  `clip_dw2 = provoking bits + perspective_divide_disable + guardband + clip_enable`
  `clip_dw3 = max_point_width + max_vp_idx`
- Status:
  partially matched
- Known gap:
  TRUEOS does not currently document an exact Mesa-equivalent `ClipMode`/`ViewportXYClipTestEnable`/`APIMode` comparison in the note or logs.
- Why it is still suspicious:
  your current failure is pre-clipper, so this packet remains high-priority.

### 3DSTATE_SF

- Mesa simple-shader explicitly sets:
  `sf.DerefBlockSize = urb_cfg.deref_block_size`
- TRUEOS now:
  `sf_dw1 = viewport_transform_enable | statistics_enable`
  `sf_dw2 = 0`
  `sf_dw3 = 0`
- Status:
  likely incomplete
- Strong suspicion:
  TRUEOS is not programming the URB-config-derived `DerefBlockSize` contract that Mesa sets on gfx12.
- Why it matters:
  this is one of the exact runtime-contract details you were pointing at: shader is public and valid, but fixed-function state derived from URB config may still be missing.

### 3DSTATE_SBE

- Mesa simple-shader sets:
  `VertexURBEntryReadOffset = 1`
  `VertexURBEntryReadLength = max((num_varyings + 1) / 2, 1)`
  `NumberofSFOutputAttributes = num_varying_inputs`
  `ForceVertexURBEntryReadOffset = true`
  `ForceVertexURBEntryReadLength = true`
  `AttributeActiveComponentFormat[*] = XYZW`
- Host summary says:
  `read_offset=1 read_length=1 num_sf_attrs=0 force_read_offset=1 force_read_length=1 active_components=xyzw`
- TRUEOS now:
  `read_offset=1` via bit 5 in `sbe_dw1`
  `read_length=1` when PS varyings are zero
  `num_sf_attrs = pipeline.ps.meta.num_varying_inputs`
  force bits enabled
  active components written as XYZW mask dwords
- Status:
  appears matched for the position-only path
- Important nuance:
  this is downstream of clipper, so it does not explain `CL_INVOCATION_COUNT == 0` by itself. It is still worth keeping aligned because it affects the next handoff after clipper.

### 3DSTATE_PRIMITIVE_REPLICATION

- Mesa simple-shader emits:
  `3DSTATE_PRIMITIVE_REPLICATION`
- TRUEOS now:
  not emitted in the draw path
- Status:
  missing
- Confidence:
  medium
- Why it matters:
  Mesa programs it even for the trivial path on gfx12; if TRUEOS relies on reset/default state here, that assumption should be tested rather than trusted.

## Short checklist for your own comparison

- Confirm TRUEOS log still shows `baked_urb_out_len=1 programmed_urb_out_len=1`.
- Confirm TRUEOS `probe-handoff-decoded` says `baked_vs_urb_out_len=1 programmed_vs_urb_out_len=1 sbe_read_len=1 ps_varyings=0`.
- Compare TRUEOS `probe-clip-decoded` against Mesa/host for:
  `PerspectiveDivideDisable`
  `ClipMode`
  `GuardbandClipTestEnable`
  `ViewportXYClipTestEnable`
  `APIMode`
  `MaximumVPIndex`
- Compare TRUEOS `3DSTATE_SF` against Mesa for:
  `ViewportTransformEnable`
  `StatisticsEnable`
  `DerefBlockSize`
- Check whether omitting `3DSTATE_PRIMITIVE_REPLICATION` is intentional or just an unimplemented gfx12 packet.

## Current ranking of likely missing runtime contracts

1. `3DSTATE_SF.DerefBlockSize` derived from URB config is not programmed in TRUEOS.
2. `3DSTATE_PRIMITIVE_REPLICATION` is omitted entirely.
3. `3DSTATE_CLIP` may still differ from Mesa in one of the exact decoded policy fields even though `PerspectiveDivideDisable` matches.

## Bottom line

Yes, this operation is meaningful.

The public/local evidence already says the shader target is correct and the host gfx125 path works. The most useful next step is to keep diffing TRUEOS's runtime packet contracts against Mesa gfx125, with the highest attention on `CLIP`, `SF`, URB-derived state, and omitted gfx12 packets.
