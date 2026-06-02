# XeLP Render WM Frontier Memo

Date: 2026-06-02

This memo captures the current Intel XeLP render-engine triangle/fragment
frontier. It exists to keep the next work anchored to evidence, not to the
noise of the rotating boot logs.

## Current Verdict

The render path still stops before WM coverage:

```text
first_good=raster-input-ready
first_bad=wm-coverage
wm_coverage=0
psd_dispatch=0
fragment_observed=0
```

The most recent selected-rung run is
`bld/baremetal-logs/latest.log -> trueos-baremetal.0.log` from
2026-06-02 07:23:25 +0200. It says:

```text
primary-single-raster-isolation-screen-trilist-mesa-raster-mesa-sf-mesa-sbe-bary8-header completed=1 ... geometry=screen-space-oversized vue_contract=header-pos1 raster_dw1=0x04A11003 sf=[0x00080402,0x00000000,0x02004808] sbe=[0x30200000,0x00000000,0x00000000,0xFFFFFFFF,0xFFFFFFFF] fragment_candidate=1 wm_coverage=0 psd_dispatch=0 fragment_observed=0
wm-input-reference geometry=screen-space-oversized ... active_interpretation=pretransformed-screen-space expected_samples=256 bbox=0x0..15x15
probe-vf-vue-contract vf_synthesized_vue=1 experiment=header+pos-slots01 ve_count=2 vue_contract=slot0=header slot1=position header_slot=source-offset0 position_slot=slot1=offset16-xyz+forced-w1 position_format=R32G32B32_FLOAT position_components=src,src,src,1.0 vertex_stride=32
probe-preclip-sbe-ps-state backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8-header-mesa-raster-mesa-sf-mesa-sbe emitted=1 sbe=[0x30200000,0x00000000,0x00000000,0xFFFFFFFF,0xFFFFFFFF] sbe_read_offset=0 sbe_read_length=0
probe-mesa-active-block backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8-header-mesa-raster-mesa-sf-mesa-sbe stamped=1 clip=[0x00000400,0xC0008300,0x00000020] sf=[0x00080402,0x00000000,0x02004808] raster=[0x04A11003,0x00000000,0x00000000,0x00000000] sample_mask=0x0000FFFF wm_depth=[0x00000010,0x00000000,0x00000000] prim_repl=0x00010000 wm=0x80004040 ps_blend=0x518C6200
probe-sbe-decoded read_offset=0 read_length=0 attr_swizzle=1 emit_sbe_swiz=1 num_sf_outputs=0 force_offset=1 force_length=1 active_component_dw4=0xFFFFFFFF active_component_dw5=0xFFFFFFFF
probe-state-contract ... vs[enabled=0 ksp=0x0 scratch=0x0 sampler_count=0 binding_table_count=0] urb_vs[entries=192 start=4 size_64b=1] sf[dw=[0x00080402,0x00000000,0x02004808] output_attr_count=0] wm[dw=0x80004040 stats=1 force_dispatch=0 ps_dispatch_formula_enable=1] ps[ksp=[0xC0,0x0,0x0] simd=simd8 dispatch_bits=100 bt_count=0 sampler_count=0 scratch=0x0]
probe-pointer-surface-contract ... binding_table_ps=0x0 bt_entry0=0x00001040 rt_surface[type=1 format=0xA width=32 height=32 pitch=128 mocs=0x1 base=0x880000] viewport[generic_emitted=0 generic_ptr=0x0 sf_clip_ptr=0x1180 cc_ptr=0x1140] scissor_ptr=0x11C0 scissor=[0,0..31,31] blend_ptr=0x10C1 cc_state_ptr=0x1101 depth_stencil_state_ptr=not-emitted
probe-clip-decoded topo=trilist ... ClipMode=4(CLIPMODE_ACCEPT_ALL) PerspectiveDivideDisable=1 NonPerspectiveBarycentricEnable=1 EarlyCullEnable=0
probe-sf-decoded sf_dw=[0x00080402,0x00000000,0x02004808] ViewportTransformEnable=1 LineWidth=0x80 PointWidth=0x8 PointWidthSource=1 AALineDistanceMode=1 LastPixelEnable=0 TriangleFanProvokingVertexSelect=1
probe-raster-decoded ... front=ccw api_mode=2(DX10.1+) z_near_clip=1 z_far_clip=1 raster_scissor_legacy_bit=1 dx_ms_enable=1 sample_mask=0xFFFF
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_vs=0 delta_cl=1 delta_cl_prim=1 delta_ps=0
vf-proof accepted=1 ia_vtx_delta=3 ia_prim_delta=1
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=1 clip_counter_required=1
ps-dispatch-proof accepted=0 ps_delta=0 cps_delta=0 ps_depth_delta=0
render-target completed=1 any_change=0 triangle_change=0
scratch-rt-fragment-proof accepted=0 completed=1 changed=0 ps_delta=0
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 begin_id=0x0A0A2101 end_id=0x0A0A2102 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
primary-single-raster-isolation-ladder stopped index=19 completed=1
```

Interpretation: the latest VF screen-space oversized TRIANGLELIST
SF-sane+bary8 header Mesa-raster+Mesa-SF+Mesa-SBE rung keeps the strongest
previous TRIANGLELIST state and changes the SBE body to the verified Mesa
active draw's `sbe=[0x30200000,0,0,0xFFFFFFFF,0xFFFFFFFF]`. The packet decoded
as read offset/length 0/0, attribute swizzle enabled, zero SF outputs, and
forced offset/length. This run also forced the selected path's `3DSTATE_PS`
binding-table count to zero, matching the Mesa oracle while keeping
`3DSTATE_BINDING_TABLE_POINTERS_PS=0`, BT entry 0 pointing at the color RT
surface state, KSP0 `0xC0`, SIMD8 dispatch, scratch zero, and the same writeable
RT contract. The draw is still a full-surface positive CPU coverage candidate
(`expected_samples=256`) that reaches VF and CL with `cl_prim_delta=1`. It still
produces zero raster samples, zero WM coverage, zero PSD/PS dispatch, and no
scratch render-target change. This rules out missing Mesa active
`3DSTATE_SBE` body and the Mesa-oracle PS binding-table-count value as first
missing SF-to-WM handoff bits on the current header+mesa-raster+mesa-sf path.
The new contract snapshot exposes a still-open adjacent-state gap:
TRUEOS emits SF_CLIP and CC viewport pointer packets, but not the generic
`3DSTATE_VIEWPORT_STATE_POINTERS` zero packet present in the Mesa active
sequence.

The previous real-VS selected-rung run was
`bld/baremetal-logs/latest.log -> trueos-baremetal.2.log` from
2026-06-02 06:06:26 +0200. It says:

```text
primary-single-raster-isolation-real-vs-ndc-mesa-active-block-swiz-vs-grf2-urbdf8-vb0204400c-prim0 completed=1 ... fragment_candidate=1 wm_coverage=0 psd_dispatch=0 fragment_observed=0
probe-vs ksp=0x00000000 dw3=0x00000000 dw6=0x00200800 dw7=0x88400405 dw8=0x00000000 ... programmed_urb_entries=3576 baked_grf_start=0 applied_grf_start=2
probe-frontend-dwords contract=mesa-vs-grf2-urbdf8-vb0204400c-prim0 vb=[0x78080003,0x0204400C,0x00870000,0x00000000,0x00000024] ve0=[0x78090001,0x02400000,0x11130000] vf_topology=[0x784B0000,0x00000004] primitive=[0x7B000808,0x00000000,0x00000003,0x00000000,0x00000001,0x00000000,...] mesa_active_vb_dw1=0x0204400C mesa_active_ve0=[0x78090001,0x02400000,0x11130000] mesa_active_vf_topology=0x00000004 mesa_active_primitive_ext=[0x7B000808,0x00000000,0x00000003,0x00000000,0x00000001,0x00000000,...]
probe-handoff-decoded ... programmed_vs_urb_entries=3576 baked_vs_grf_start=0 programmed_vs_grf_start=2 sbe_read_offset=1 sbe_read_len=1
stage-diagnosis completed=1 verdict=completed delta_vs=3 delta_cl=0 delta_cl_prim=0 delta_ps=0
clip-raster-proof accepted=0 cl_delta=0 cl_prim_delta=0 clip_counter_required=1
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
primary-single-raster-isolation-ladder stopped index=10 completed=1
```

The first missing behavior is not VF fetch, CL input, line/point/rect object
input, SBE_SWIZ emission, Mesa backend gate state, `NUMRASTSAMPLES_1`, explicit
on-pixel WM_INT/SF_INT multisample raster mode, Mesa-like SF `dw3`, SBE
`VertexURBEntryReadLength=0`, real-VS execution, or switching from
pretransformed screen coordinates to NDC plus SF viewport transform. It is also
not fixed by enabling non-perspective barycentric clip state and WM barycentric
mode `0x08` on the strongest screen-space TRIANGLELIST, or by changing that
same path to a synthesized `slot0=header slot1=position` VUE layout, or by
changing that header path's `3DSTATE_RASTER dw1` to the verified Mesa active
draw value `0x04A11003`, or by changing that header+mesa-raster path's
`3DSTATE_SF` body to the verified Mesa active draw value
`[0x00080402,0,0x02004808]`, or by changing that
header+mesa-raster+mesa-sf path's `3DSTATE_SBE` body to the verified Mesa
active draw value `[0x30200000,0,0,0xFFFFFFFF,0xFFFFFFFF]`, or by changing that
same path's `3DSTATE_PS` binding-table count from 1 to the Mesa-oracle value 0
while keeping the PS binding table pointer and RT surface state intact. It is also
not fixed by enabling Mesa's SBE
attribute-swizzle bit with a zeroed `SBE_SWIZ` packet on the strongest line probe, by switching the VF line path to
NDC plus Mesa's active CLIP/SF/RASTER/backend block, or by switching the VF
NDC path to a positive-coverage RECTLIST with the same Mesa-active block and a
valid OA counter window. It is also not fixed by switching that NDC path to an
oversized positive-coverage TRIANGLELIST with `vf_topology=trilist` and the
Mesa active extended `3DPRIMITIVE` `dw1=0`, or by keeping that TRIANGLELIST
shape while switching back to host-style pretransformed screen-space vertices
with `PerspectiveDivideDisable=1` and SF viewport transform disabled, including
the SF sane-default packet shape `sf=[0x00080400,0x00000000,0x80000808]`. The
real-VS path is not fixed by
programming the Mesa oracle VS dispatch GRF-start field (`dw6=0x00200800`),
Mesa's active VS `dw8=0x00000000`, Mesa's active VS URB allocation entry count
(`0x0DF8`), Mesa's active `3DSTATE_VERTEX_BUFFERS` `dw1=0x0204400C`, and Mesa's
active extended `3DPRIMITIVE` `dw1=0x00000000`. The visible real-VS frontend
packet dwords now match the Mesa active draw shape for vertex-buffer `dw1`,
first vertex element, VF topology, and the first six extended primitive dwords,
yet the real-VS triangle path still stops before CL input. A new streamout
oracle did not prove real-VS output bytes: both the default Mesa-like contract
and the selected GRF2/URBDF8 contract advance VS by 3, then stop before SOL
writes or CL input. The boundary remains the transition from a
CL/raster-input-ready object into WM scan conversion coverage for the VF path,
while real-VS is an earlier VS-to-CL/SOL frontier.

## Latest Screen-Space Trilist Mesa-SBE PS-BT0 Contract Snapshot Probe

Measured on 2026-06-02 in
`bld/baremetal-logs/latest.log -> trueos-baremetal.0.log`:

```text
late-vf-screen-trilist-header-pos1-mesa-raster-mesa-sf-mesa-sbe-bary8-raster-wm-oa-probe
primary-single-raster-isolation-screen-trilist-mesa-raster-mesa-sf-mesa-sbe-bary8-header
```

This selected single-raster index 19 rung keeps the
header+position screen-space TRIANGLELIST Mesa-raster+Mesa-SF+Mesa-SBE packet
body and changes the selected path's `3DSTATE_PS` binding-table entry count to
Mesa's active-oracle value 0. It also logs the requested frontend/backend state
contract in one place.

Evidence:

```text
probe-state-contract backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8-header-mesa-raster-mesa-sf-mesa-sbe vs[enabled=0 ksp=0x0 scratch=0x0 sampler_count=0 binding_table_count=0] urb_vs[entries=192 start=4 size_64b=1 size_field=0 dw1=0x00801000 dw2=0x00C000C0] sf[dw=[0x00080402,0x00000000,0x02004808] output_attr_count=0] wm[dw=0x80004040 stats=1 force_dispatch=0 ps_dispatch_formula_enable=1] ps[ksp=[0xC0,0x0,0x0] simd=simd8 dispatch_bits=100 bt_count=0 sampler_count=0 scratch=0x0 dw3=0x00000000 dw6=0x1F800001 dw7=0x00000000] primitive[cmd=0x7B000808 topology=trilist count=3 start=0 instance_count=1 base_vertex=0]
probe-pointer-surface-contract backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8-header-mesa-raster-mesa-sf-mesa-sbe binding_table_ps=0x0 bt_entry0=0x00001040 surface_off=0x1040 rt_surface[type=1 format=0xA width=32 height=32 pitch=128 mocs=0x1 base=0x880000 swizzle=0x09770000] viewport[generic_emitted=0 generic_ptr=0x0 sf_clip_ptr=0x1180 cc_ptr=0x1140 sf_m00=16.000 sf_m11=-16.000 sf_m30=16.000 sf_m31=16.000 cc_min=0.000 cc_max=1.000] scissor_ptr=0x11C0 scissor=[0,0..31,31] blend_ptr=0x10C1 blend_dwords=[0x00000000,0x00000000] cc_state_ptr=0x1101 depth_stencil_state_ptr=not-emitted wm_depth_packet=[0x00000010,0x00000000,0x00000000]
probe-pipe-control-contract backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8-header-mesa-raster-mesa-sf-mesa-sbe before_draw=[flush_flags=0x041010A0,invalidate_flags=0x00140C1C,binding_pool_pre_cs=0x00100000,binding_pool_post_invalidate=0x00140C1C] after_draw=[light_flags=0x01004000,light_post_sync=1,heavy_flags=0x00000000,heavy_post_sync=0,oa_fence_flags=0x00000000] postdraw_variant=pc-postsync-no-cs
mesa-compare host_ps[vector_mask=0 binding_table_entry_count=0 push_constants=0 dispatch=simd8 max_threads_per_psd=63] trueos_ps[vector_mask=0 binding_table_entry_count=0 push_constants=0 dispatch=simd8 max_threads_per_psd=63]
```

Negative evidence:

```text
ps-dispatch-proof accepted=0 ps_delta=0 cps_delta=0 ps_depth_delta=0 ps_state_marker=1 completed=1
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
scratch-rt-fragment-proof accepted=0 ... changed=0
render-target completed=1 any_change=0 triangle_change=0
```

Interpretation: the selected path now matches Mesa's active `3DSTATE_PS`
binding-table count while preserving a valid PS binding table pointer to BT
offset 0, BT entry 0 to the color RT surface state, KSP0 `0xC0`, SIMD8 PS
dispatch, zero scratch, zero samplers, and a writeable 32x32 B8G8R8A8_UNORM RT
surface. This rules out the PS binding-table-count mismatch as the first
missing SF-to-WM handoff bit. The useful new mismatch is that TRUEOS emits
`3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP` and
`3DSTATE_VIEWPORT_STATE_POINTERS_CC`, but not the generic
`3DSTATE_VIEWPORT_STATE_POINTERS` zero-pointer packet that appears in Mesa's
active sequence.

## Latest Screen-Space Trilist SF-Sane Bary8 Header Mesa-Raster Mesa-SF Mesa-SBE Probe

Measured on 2026-06-02 in
`bld/baremetal-logs/latest.log -> trueos-baremetal.2.log`:

```text
late-vf-screen-trilist-header-pos1-mesa-raster-mesa-sf-mesa-sbe-bary8-raster-wm-oa-probe
primary-single-raster-isolation-screen-trilist-mesa-raster-mesa-sf-mesa-sbe-bary8-header
```

This selected single-raster index 19 rung keeps the index 18 header+position
screen-space TRIANGLELIST Mesa-raster+Mesa-SF state and changes the SBE packet
body to Mesa's verified active draw value:
`sbe=[0x30200000,0,0,0xFFFFFFFF,0xFFFFFFFF]`.

Evidence:

```text
fragment-candidate-shape accepted=1 geometry=screen-space-oversized screen=v0[0.000,0.000] v1[64.000,0.000] v2[0.000,64.000]
wm-input-reference geometry=screen-space-oversized ... active_interpretation=pretransformed-screen-space expected_samples=256 bbox=0x0..15x15
probe-preclip-sbe-ps-state backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8-header-mesa-raster-mesa-sf-mesa-sbe emitted=1 sbe=[0x30200000,0x00000000,0x00000000,0xFFFFFFFF,0xFFFFFFFF] sbe_read_offset=0 sbe_read_length=0
probe-mesa-active-block backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8-header-mesa-raster-mesa-sf-mesa-sbe stamped=1 clip=[0x00000400,0xC0008300,0x00000020] sf=[0x00080402,0x00000000,0x02004808] raster=[0x04A11003,0x00000000,0x00000000,0x00000000] wm=0x80004040 ps_blend=0x518C6200
probe-sbe-decoded read_offset=0 read_length=0 attr_swizzle=1 emit_sbe_swiz=1 num_sf_outputs=0 force_offset=1 force_length=1 active_component_dw4=0xFFFFFFFF active_component_dw5=0xFFFFFFFF
probe-clip-decoded topo=trilist ... ClipMode=4(CLIPMODE_ACCEPT_ALL) PerspectiveDivideDisable=1 NonPerspectiveBarycentricEnable=1 EarlyCullEnable=0
probe-sf-decoded sf_dw=[0x00080402,0x00000000,0x02004808] ViewportTransformEnable=1 StatisticsEnable=1 DerefBlockSize=0(Block32) LineWidth=0x80 PointWidth=0x8 PointWidthSource=1 AALineDistanceMode=1 LastPixelEnable=0 TriangleFanProvokingVertexSelect=1
probe-raster-decoded ... front=ccw api_mode=2(DX10.1+) z_near_clip=1 z_far_clip=1 raster_scissor_legacy_bit=1 dx_ms_enable=1 sample_mask=0xFFFF
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=1 delta_ps=0
vf-proof accepted=1 ia_vtx_delta=3 ia_prim_delta=1
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=1 clip_counter_required=1
fragment-candidate-proof accepted=1 candidate_shape=screen-space-oversized clip_counter=1 raster_packet=1 ps_state_marker=1 fragment_observed=0
ps-dispatch-proof accepted=0 ps_delta=0 cps_delta=0 ps_depth_delta=0
render-target completed=1 any_change=0 triangle_change=0
scratch-rt-fragment-proof accepted=0 completed=1 changed=0 ps_delta=0
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
wm-boundary-regs reports_valid=1 wm_coverage=0 psd_dispatch=0 sc=0xFFFFFFFF row=0xFFFFFFFF sampler=0xFFFFFFFF
primary-single-raster-isolation-ladder stopped index=19 completed=1
```

Interpretation: Mesa's active SBE body is not the missing handoff bit on the
current header+Mesa-raster+Mesa-SF path. It changes SBE to Mesa's
read0/length0/attribute-swizzle/zero-output contract, but the same full-surface
CPU coverage candidate, header+position VUE, TRIANGLELIST topology, CL-positive
draw, Mesa SF/RASTER packets, bary8, and PS state still produce zero raster
samples, zero WM coverage, zero PSD dispatch, and no scratch render-target
change.

## Latest Screen-Space Trilist SF-Sane Bary8 Header Mesa-Raster Mesa-SF Probe

Measured on 2026-06-02 in
`bld/baremetal-logs/latest.log -> trueos-baremetal.1.log`:

```text
late-vf-screen-trilist-header-pos1-mesa-raster-mesa-sf-bary8-raster-wm-oa-probe
primary-single-raster-isolation-screen-trilist-mesa-raster-mesa-sf-bary8-header
```

This selected single-raster index 18 rung keeps the index 17 header+position
screen-space TRIANGLELIST Mesa-raster state and changes the SF packet body to
Mesa's verified active draw value: `sf=[0x00080402,0,0x02004808]`.

Evidence:

```text
fragment-candidate-shape accepted=1 geometry=screen-space-oversized screen=v0[0.000,0.000] v1[64.000,0.000] v2[0.000,64.000]
wm-input-reference geometry=screen-space-oversized ... active_interpretation=pretransformed-screen-space expected_samples=256 bbox=0x0..15x15
probe-vf-vue-contract vf_synthesized_vue=1 experiment=header+pos-slots01 ve_count=2 vue_contract=slot0=header slot1=position header_slot=source-offset0 position_slot=slot1=offset16-xyz+forced-w1 position_format=R32G32B32_FLOAT position_components=src,src,src,1.0 vertex_stride=32
probe-mesa-active-block backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8-header-mesa-raster-mesa-sf stamped=1 clip=[0x00000400,0xC0008300,0x00000020] sf=[0x00080402,0x00000000,0x02004808] raster=[0x04A11003,0x00000000,0x00000000,0x00000000] wm=0x80004040 ps_blend=0x518C6200
probe-3dprimitive-extended ... vf_topology=trilist primitive_dw1=0x00000000
probe-sbe-decoded read_offset=1 read_length=1 attr_swizzle=0 emit_sbe_swiz=1
probe-clip-decoded topo=trilist ... ClipMode=4(CLIPMODE_ACCEPT_ALL) PerspectiveDivideDisable=1 NonPerspectiveBarycentricEnable=1 EarlyCullEnable=0
probe-sf-decoded sf_dw=[0x00080402,0x00000000,0x02004808] ViewportTransformEnable=1 StatisticsEnable=1 DerefBlockSize=0(Block32) LineWidth=0x80 PointWidth=0x8 PointWidthSource=1 VertexSubPixelPrecisionSelect=_8Bit SmoothPointEnable=0 AALineDistanceMode=1 LastPixelEnable=0 TriangleFanProvokingVertexSelect=1
probe-raster-decoded ... front=ccw api_mode=2(DX10.1+) z_near_clip=1 z_far_clip=1 raster_scissor_legacy_bit=1 dx_ms_enable=1 sample_mask=0xFFFF
probe-wm-decoded backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8-header-mesa-raster-mesa-sf ... bary=0x08 ... ps_valid=1 ps_writes_rt=1
probe-ps-payload-decoded backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8-header-mesa-raster-mesa-sf ... wm_bary=0x8 ps_dispatch_bits=100
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=1 delta_ps=0
vf-proof accepted=1 ia_vtx_delta=3 ia_prim_delta=1
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=1 clip_counter_required=1
fragment-candidate-proof accepted=1 candidate_shape=screen-space-oversized clip_counter=1 raster_packet=1 ps_state_marker=1 fragment_observed=0
ps-dispatch-proof accepted=0 ps_delta=0 cps_delta=0 ps_depth_delta=0
render-target completed=1 any_change=0 triangle_change=0
scratch-rt-fragment-proof accepted=0 completed=1 changed=0 ps_delta=0
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
wm-boundary-regs reports_valid=1 wm_coverage=0 psd_dispatch=0 sc=0xFFFFFFFF row=0xFFFFFFFF sampler=0xFFFFFFFF
primary-single-raster-isolation-ladder stopped index=18 completed=1
```

Interpretation: Mesa's active SF body is not the missing handoff bit on the
current header+Mesa-raster path. It changes the decoded SF state to enable SF
viewport transform and Mesa-style AA line distance/fan provoking/last-pixel
bits, but the same full-surface CPU coverage candidate, header+position VUE,
TRIANGLELIST topology, CL-positive draw, Mesa RASTER packet, bary8, and PS
state still produce zero raster samples, zero WM coverage, zero PSD dispatch,
and no scratch render-target change.

## Latest Screen-Space Trilist SF-Sane Bary8 Header Mesa-Raster Probe

Measured on 2026-06-02 in
`bld/baremetal-logs/latest.log -> trueos-baremetal.0.log`:

```text
late-vf-screen-trilist-header-pos1-mesa-raster-sf-sane-bary8-raster-wm-oa-probe
primary-single-raster-isolation-screen-trilist-mesa-raster-sf-sane-bary8-header
```

This selected single-raster index 17 rung keeps the index 16 header+position
screen-space TRIANGLELIST state and changes only the RASTER packet body to
Mesa's verified active draw value: `raster=[0x04A11003,0,0,0]`.

Evidence:

```text
fragment-candidate-shape accepted=1 geometry=screen-space-oversized screen=v0[0.000,0.000] v1[64.000,0.000] v2[0.000,64.000]
wm-input-reference geometry=screen-space-oversized ... active_interpretation=pretransformed-screen-space expected_samples=256 bbox=0x0..15x15
probe-vf-vue-contract vf_synthesized_vue=1 experiment=header+pos-slots01 ve_count=2 vue_contract=slot0=header slot1=position header_slot=source-offset0 position_slot=slot1=offset16-xyz+forced-w1 position_format=R32G32B32_FLOAT position_components=src,src,src,1.0 vertex_stride=32
probe-mesa-active-block backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8-header-mesa-raster ... clip=[0x00000400,0xC0008300,0x00000020] sf=[0x00080400,0x00000000,0x80000808] raster=[0x04A11003,0x00000000,0x00000000,0x00000000] wm=0x80004040 ps_blend=0x518C6200
probe-3dprimitive-extended ... vf_topology=trilist primitive_dw1=0x00000000
probe-sbe-decoded read_offset=1 read_length=1 attr_swizzle=0 emit_sbe_swiz=1
probe-clip-decoded topo=trilist ... PerspectiveDivideDisable=1 NonPerspectiveBarycentricEnable=1
probe-sf-decoded sf_dw=[0x00080400,0x00000000,0x80000808] ViewportTransformEnable=0 LineWidth=0x80 PointWidth=0x8 PointWidthSource=1 LastPixelEnable=1
probe-raster-decoded ... front=ccw api_mode=2(DX10.1+) z_near_clip=1 z_far_clip=1 raster_scissor_legacy_bit=1 dx_ms_enable=1 sample_mask=0xFFFF
probe-wm-decoded backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8-header-mesa-raster ... bary=0x08 ... ps_valid=1 ps_writes_rt=1
probe-ps-payload-decoded backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8-header-mesa-raster ... wm_bary=0x8 ps_dispatch_bits=100
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=1 delta_ps=0
vf-proof accepted=1 ia_vtx_delta=3 ia_prim_delta=1
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=1
fragment-candidate-proof accepted=1 candidate_shape=screen-space-oversized clip_counter=1 raster_packet=1 ps_state_marker=1 fragment_observed=0
ps-dispatch-proof accepted=0 ps_delta=0 cps_delta=0 ps_depth_delta=0
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
wm-boundary-regs reports_valid=1 wm_coverage=0 psd_dispatch=0 sc=0xFFFFFFFF row=0xFFFFFFFF sampler=0xFFFFFFFF
scratch-rt-fragment-proof accepted=0 completed=1 changed=0 ps_delta=0
primary-single-raster-isolation-ladder stopped index=17 completed=1
```

Interpretation: Mesa's active RASTER dword is not the missing handoff bit. It
does change the decoded RASTER state from the previous header rung, but with
the same positive CPU coverage, header+position VUE, CL-positive draw, SBE
read offset/length 1, SF sane defaults, bary8, and PS state, the WM/PSD
boundary remains flat.

## Latest Screen-Space Trilist SF-Sane Bary8 Header Probe

Measured on 2026-06-02 in
`bld/baremetal-logs/latest.log -> trueos-baremetal.2.log`:

```text
late-vf-screen-trilist-header-pos1-mesa-active-block-swiz-sf-sane-bary8-raster-wm-oa-probe
primary-single-raster-isolation-screen-trilist-mesa-active-block-swiz-sf-sane-bary8-header
```

This selected single-raster index 16 rung keeps the index 15 screen-space
TRIANGLELIST SF-sane+bary8 state and changes the VF-synthesized VUE layout to
match a header+position shape: `slot0=header slot1=position`, position at
offset 16, forced W=1, and a 32-byte vertex stride.

Evidence:

```text
fragment-candidate-shape accepted=1 geometry=screen-space-oversized screen=v0[0.000,0.000] v1[64.000,0.000] v2[0.000,64.000]
wm-input-reference geometry=screen-space-oversized ... active_interpretation=pretransformed-screen-space expected_samples=256 bbox=0x0..15x15
probe-vf-vue-contract vf_synthesized_vue=1 experiment=header+pos-slots01 ve_count=2 vue_contract=slot0=header slot1=position header_slot=source-offset0 position_slot=slot1=offset16-xyz+forced-w1 position_format=R32G32B32_FLOAT position_components=src,src,src,1.0 vertex_stride=32
probe-mesa-active-block backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8-header ... clip=[0x00000400,0xC0008300,0x00000020] sf=[0x00080400,0x00000000,0x80000808] raster=[0x00810002,0x00000000,0x00000000,0x00000000] wm=0x80004040 ps_blend=0x518C6200
probe-3dprimitive-extended ... vf_topology=trilist primitive_dw1=0x00000000
probe-sbe-decoded read_offset=1 read_length=1 attr_swizzle=0 emit_sbe_swiz=1
probe-clip-decoded topo=trilist ... PerspectiveDivideDisable=1 NonPerspectiveBarycentricEnable=1
probe-sf-decoded sf_dw=[0x00080400,0x00000000,0x80000808] ViewportTransformEnable=0 LineWidth=0x80 PointWidth=0x8 PointWidthSource=1 LastPixelEnable=1
probe-wm-decoded backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8-header ... bary=0x08 ... ps_valid=1 ps_writes_rt=1
probe-ps-payload-decoded backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8-header ... wm_bary=0x8 ps_dispatch_bits=100
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=1 delta_ps=0
vf-proof accepted=1 ia_vtx_delta=3 ia_prim_delta=1
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=1
fragment-candidate-proof accepted=1 candidate_shape=screen-space-oversized clip_counter=1 raster_packet=1 ps_state_marker=1 fragment_observed=0
ps-dispatch-proof accepted=0 ps_delta=0 cps_delta=0 ps_depth_delta=0
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
wm-boundary-regs reports_valid=1 wm_coverage=0 psd_dispatch=0 sc=0xFFFFFFFF row=0xFFFFFFFF sampler=0xFFFFFFFF
scratch-rt-fragment-proof accepted=0 completed=1 changed=0 ps_delta=0
primary-single-raster-isolation-ladder stopped index=16 completed=1
```

Interpretation: the synthesized VUE header+position layout did not move
WM/PSD. This rules out the immediate theory that SF was rejecting the
slot0-position-only VUE because it expected a VUE header before position data.
The boundary remains SF object setup to WM scan conversion, with the strongest
remaining suspects now hidden/adjacent frontend state, real VS/VUE content, or
another state packet required for the SF-to-WM handoff.

## Latest Screen-Space Trilist SF-Sane Bary8 Probe

Measured on 2026-06-02 in
`bld/baremetal-logs/latest.log -> trueos-baremetal.1.log`:

```text
late-vf-screen-trilist-slot0-xyzw-mesa-active-block-swiz-sf-sane-bary8-raster-wm-oa-probe
primary-single-raster-isolation-screen-trilist-mesa-active-block-swiz-sf-sane-bary8
```

This selected single-raster index 15 rung keeps the index 14 SF-sane
screen-space TRIANGLELIST contract and adds the non-perspective barycentric
state that the previous strongest TRIANGLELIST lacked: CLIP
`NonPerspectiveBarycentricEnable=1`, WM barycentric mode `0x08`, and decoded
PS payload `wm_bary=0x8`.

Evidence:

```text
fragment-candidate-shape accepted=1 geometry=screen-space-oversized screen=v0[0.000,0.000] v1[64.000,0.000] v2[0.000,64.000]
wm-input-reference geometry=screen-space-oversized ... active_interpretation=pretransformed-screen-space expected_samples=256 bbox=0x0..15x15
probe-mesa-active-block backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8 ... clip=[0x00000400,0xC0008300,0x00000020] sf=[0x00080400,0x00000000,0x80000808] raster=[0x00810002,0x00000000,0x00000000,0x00000000] wm=0x80004040 ps_blend=0x518C6200
probe-3dprimitive-extended ... vf_topology=trilist primitive_dw1=0x00000000
probe-sbe-decoded read_offset=1 read_length=1 attr_swizzle=0 emit_sbe_swiz=1
probe-clip-decoded topo=trilist ... PerspectiveDivideDisable=1 NonPerspectiveBarycentricEnable=1
probe-sf-decoded sf_dw=[0x00080400,0x00000000,0x80000808] ViewportTransformEnable=0 LineWidth=0x80 PointWidth=0x8 PointWidthSource=1 LastPixelEnable=1
probe-wm-decoded backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8 ... bary=0x08 ... ps_valid=1 ps_writes_rt=1
probe-ps-payload-decoded backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane-bary8 ... wm_bary=0x8 ps_dispatch_bits=100
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=1 delta_ps=0
vf-proof accepted=1 ia_vtx_delta=3 ia_prim_delta=1
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=1
fragment-candidate-proof accepted=1 candidate_shape=screen-space-oversized clip_counter=1 raster_packet=1 ps_state_marker=1 fragment_observed=0
ps-dispatch-proof accepted=0 ps_delta=0 cps_delta=0 ps_depth_delta=0
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
wm-boundary-regs reports_valid=1 wm_coverage=0 psd_dispatch=0 sc=0xFFFFFFFF row=0xFFFFFFFF sampler=0xFFFFFFFF
scratch-rt-fragment-proof accepted=0 completed=1 changed=0 ps_delta=0
primary-single-raster-isolation-ladder stopped index=15 completed=1
```

Interpretation: TRIANGLELIST SF-sane plus bary8/non-perspective did not move
WM/PSD. This rules out missing barycentric mode on the strongest current
TRIANGLELIST probe; the boundary remains SF object/VUE/header interpretation,
hidden adjacent state, or a missing state packet required for SF-to-WM scan
conversion.

## Latest Screen-Space Trilist SF-Sane Mesa-Active Block Probe

Measured on 2026-06-02 in
`bld/baremetal-logs/latest.log -> trueos-baremetal.0.log`:

```text
late-vf-screen-trilist-slot0-xyzw-mesa-active-block-swiz-sf-sane-raster-wm-oa-probe
primary-single-raster-isolation-screen-trilist-mesa-active-block-swiz-sf-sane
```

This selected single-raster index 14 rung keeps the index 13 screen-space
TRIANGLELIST contract but restores the SF sane-default body shape. The delta
from the previous screen-space TRIANGLELIST is SF only: `sf` changes from
`[0x00000400,0x00000000,0x00000000]` to
`[0x00080400,0x00000000,0x80000808]`, giving nonzero line/point defaults and
`LastPixelEnable=1` while keeping SF viewport transform disabled.

Evidence:

```text
fragment-candidate-shape accepted=1 geometry=screen-space-oversized screen=v0[0.000,0.000] v1[64.000,0.000] v2[0.000,64.000]
wm-input-reference geometry=screen-space-oversized ... active_interpretation=pretransformed-screen-space expected_samples=256 bbox=0x0..15x15
probe-mesa-active-block backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz-sf-sane ... clip=[0x00000400,0xC0008200,0x00000020] sf=[0x00080400,0x00000000,0x80000808] raster=[0x00810002,0x00000000,0x00000000,0x00000000] wm=0x80000040 ps_blend=0x518C6200
probe-3dprimitive-extended ... vf_topology=trilist primitive_dw1=0x00000000
probe-sbe-decoded read_offset=1 read_length=1 attr_swizzle=0 emit_sbe_swiz=1
probe-clip-decoded topo=trilist ... ClipMode=4(CLIPMODE_ACCEPT_ALL) PerspectiveDivideDisable=1 EarlyCullEnable=0
probe-sf-decoded sf_dw=[0x00080400,0x00000000,0x80000808] ViewportTransformEnable=0 LineWidth=0x80 PointWidth=0x8 PointWidthSource=1 LastPixelEnable=1
probe-raster-decoded ... front=cw api_mode=2(DX10.1+) z_near_clip=0 z_far_clip=0 raster_scissor_legacy_bit=1 sample_mask=0xFFFF
stage-frontier completed=1 ... counter_frontier=clipper-thread
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=1 delta_ps=0
vf-proof accepted=1 ia_vtx_delta=3 ia_prim_delta=1
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=1
fragment-candidate-proof accepted=1 candidate_shape=screen-space-oversized clip_counter=1 raster_packet=1 ps_state_marker=1 fragment_observed=0
ps-dispatch-proof accepted=0 ps_delta=0 cps_delta=0 ps_depth_delta=0
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
wm-boundary-regs reports_valid=1 wm_coverage=0 psd_dispatch=0 sc=0xFFFFFFFF row=0xFFFFFFFF sampler=0xFFFFFFFF
render-target completed=1 any_change=0 triangle_change=0
scratch-rt-fragment-proof accepted=0 completed=1 changed=0 ps_delta=0
primary-single-raster-isolation-ladder stopped index=14 completed=1
```

Interpretation: the base screen-space TRIANGLELIST did not fail because the SF
packet was too minimal. With the SF sane-default packet shape restored, TRUEOS
still gets a CL-positive positive-coverage triangle and still observes zero
raster samples, WM coverage, PSD dispatch, PS counters, or scratch RT writes.
The current suspect remains an SF object/VUE/header interpretation issue,
hidden adjacent frontend state, or another state packet needed for SF-to-WM
scan conversion.

## Latest Screen-Space Trilist Mesa-Active Block Probe

Measured on 2026-06-02 in
`bld/baremetal-logs/latest.log -> trueos-baremetal.2.log`:

```text
late-vf-screen-trilist-slot0-xyzw-mesa-active-block-swiz-raster-wm-oa-probe
primary-single-raster-isolation-screen-trilist-mesa-active-block-swiz
```

This selected single-raster index 13 rung tests an oversized positive-coverage
screen-space TRIANGLELIST using slot0 XYZW, the host simple-triangle
pretransformed coordinate contract (`PerspectiveDivideDisable=1`, SF viewport
transform disabled), Mesa's active backend block, explicit SBE_SWIZ, SBE read
offset/length 1, VF topology `trilist`, and Mesa's active extended
3DPRIMITIVE `dw1=0`.

Evidence:

```text
fragment-candidate-shape accepted=1 geometry=screen-space-oversized screen=v0[0.000,0.000] v1[64.000,0.000] v2[0.000,64.000]
wm-input-reference geometry=screen-space-oversized ... active_interpretation=pretransformed-screen-space expected_samples=256 bbox=0x0..15x15
probe-mesa-active-block backend=raster-wm-input-oa-screen-space-trilist-mesa-active-block-swiz ... clip=[0x00000400,0xC0008200,0x00000020] sf=[0x00000400,0x00000000,0x00000000] raster=[0x00810002,0x00000000,0x00000000,0x00000000] wm=0x80000040 ps_blend=0x518C6200
probe-3dprimitive-extended ... vf_topology=trilist primitive_dw1=0x00000000
probe-sbe-decoded read_offset=1 read_length=1 attr_swizzle=0 emit_sbe_swiz=1
probe-clip-decoded topo=trilist ... ClipMode=4(CLIPMODE_ACCEPT_ALL) PerspectiveDivideDisable=1 EarlyCullEnable=0
probe-sf-decoded sf_dw=[0x00000400,0x00000000,0x00000000] ViewportTransformEnable=0
probe-raster-decoded ... front=cw api_mode=2(DX10.1+) z_near_clip=0 z_far_clip=0 raster_scissor_legacy_bit=1 sample_mask=0xFFFF
stage-frontier completed=1 ... counter_frontier=clipper-thread
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=1 delta_ps=0
vf-proof accepted=1 ia_vtx_delta=3 ia_prim_delta=1
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=1
fragment-candidate-proof accepted=1 candidate_shape=screen-space-oversized clip_counter=1 raster_packet=1 ps_state_marker=1 fragment_observed=0
ps-dispatch-proof accepted=0 ps_delta=0 cps_delta=0 ps_depth_delta=0
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
wm-boundary-regs reports_valid=1 wm_coverage=0 psd_dispatch=0 sc=0xFFFFFFFF row=0xFFFFFFFF sampler=0xFFFFFFFF
render-target completed=1 any_change=0 triangle_change=0
scratch-rt-fragment-proof accepted=0 completed=1 changed=0 ps_delta=0
primary-single-raster-isolation-ladder stopped index=13 completed=1
```

Interpretation: this removes the latest obvious host-oracle delta from the NDC
TRIANGLELIST proof. TRUEOS can present a positive-coverage TRIANGLELIST through
VF and CL using either NDC+viewport (`PDD=0`, SF viewport transform enabled) or
pretransformed screen-space (`PDD=1`, SF viewport transform disabled), and both
produce zero raster samples and no WM/PSD handoff. The remaining suspect is not
the coordinate-space/PDD contract; it is still an SF object/VUE/header
interpretation issue, hidden adjacent frontend state, or another state packet
needed for SF-to-WM scan conversion.

## Latest NDC Trilist Mesa-Active Block Probe

Measured on 2026-06-02 in
`bld/baremetal-logs/latest.log -> trueos-baremetal.1.log`:

```text
late-vf-ndc-trilist-slot0-xyzw-mesa-active-block-swiz-raster-wm-oa-probe
primary-single-raster-isolation-ndc-trilist-mesa-active-block-swiz
```

This selected single-raster index 12 rung tests an oversized positive-coverage
NDC TRIANGLELIST using slot0 XYZW, SF viewport transform, perspective divide,
Mesa's active CLIP/SF/RASTER/backend block, explicit SBE_SWIZ, SBE read
offset/length 1, VF topology `trilist`, and Mesa's active extended
3DPRIMITIVE `dw1=0`.

Evidence:

```text
fragment-candidate-shape accepted=1 geometry=oversized screen_bbox=[0,0..31,31]
wm-input-reference geometry=oversized ... expected_samples=256 bbox=0x0..15x15
probe-mesa-active-block backend=raster-wm-input-oa-ndc-trilist-mesa-active-block-swiz ... wm=0x80000040 ps_blend=0x518C6200
probe-3dprimitive-extended ... vf_topology=trilist primitive_dw1=0x00000000
probe-sbe-decoded read_offset=1 read_length=1 attr_swizzle=0 emit_sbe_swiz=1
probe-clip-decoded topo=trilist ... PerspectiveDivideDisable=0 EarlyCullEnable=1
probe-sf-decoded sf_dw=[0x00080402,0x00000000,0x02004808] ViewportTransformEnable=1
probe-raster-decoded ... z_near_clip=1 z_far_clip=1 raster_scissor_legacy_bit=1 sample_mask=0xFFFF
stage-frontier completed=1 ... counter_frontier=clipper-thread
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=0 delta_ps=0
vf-proof accepted=1 ia_vtx_delta=3 ia_prim_delta=1
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=0
fragment-candidate-proof accepted=1 candidate_shape=oversized clip_counter=1 raster_packet=1 ps_state_marker=1 fragment_observed=0
ps-dispatch-proof accepted=0 ps_delta=0 cps_delta=0 ps_depth_delta=0
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
render-target completed=1 any_change=0 triangle_change=0
scratch-rt-fragment-proof accepted=0 completed=1 changed=0 ps_delta=0
primary-single-raster-isolation-ladder stopped index=12 completed=1
```

Interpretation: this removes the two visible weaknesses left by the NDC
RECTLIST rung: the draw now uses TRIANGLELIST topology and the extended
primitive packet uses the Mesa active `dw1=0`. The hardware still stops after
CL input and before raster samples, WM coverage, PSD dispatch, PS counters, or
scratch render-target writes. The remaining VF path suspect is therefore not
RECTLIST topology or primitive-DW1 shape; it is deeper in SF object/VUE/header
interpretation, hidden fixed-function state, or another state packet needed for
SF-to-WM scan conversion.

## Latest NDC Rect Mesa-Active Block Probe

Measured on 2026-06-02 in `bld/baremetal-logs/latest.log`:

```text
late-vf-ndc-rectlist-slot0-xyzw-mesa-active-block-swiz-raster-wm-oa-probe
primary-single-raster-isolation-ndc-rectlist-mesa-active-block-swiz
```

This selected single-raster index 11 rung tests a positive-coverage NDC
RECTLIST using slot0 XYZW, SF viewport transform, perspective divide, Mesa's
active CLIP/SF/RASTER/backend block, explicit SBE_SWIZ, SBE read offset/length
1, and extended 3DPRIMITIVE.

Evidence:

```text
wm-input-reference ... expected_samples=136 bbox=0x0..15x15
probe-mesa-active-block backend=raster-wm-input-oa-ndc-rectlist-mesa-active-block-swiz ... wm=0x80000040 ps_blend=0x518C6200
probe-3dprimitive-extended ... vf_topology=rectlist primitive_dw1=0x0000000F
probe-sbe-decoded read_offset=1 read_length=1 attr_swizzle=0 emit_sbe_swiz=1
probe-clip-decoded topo=rectlist ... PerspectiveDivideDisable=0 EarlyCullEnable=1
probe-sf-decoded sf_dw=[0x00080402,0x00000000,0x02004808] ViewportTransformEnable=1
probe-raster-decoded ... z_near_clip=1 z_far_clip=1 raster_scissor_legacy_bit=1 sample_mask=0xFFFF
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=0 delta_ps=0
vf-proof accepted=1 ia_vtx_delta=3 ia_prim_delta=1
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=0
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
render-target completed=1 any_change=0 triangle_change=0
scratch-rt-fragment-proof accepted=0 completed=1 changed=0 ps_delta=0
primary-single-raster-isolation-ladder stopped index=11 completed=1
```

Interpretation: this rules out the previous line probe's zero expected sample
coverage and invalid/weak proof concerns. Even a positive NDC RECTLIST, with
Mesa's active fixed-function block and valid OA reports, stops after CL and
before raster sample generation/WM coverage.

## Latest NDC Line Mesa-Active Block Probe

Measured on 2026-06-02 in `bld/baremetal-logs/latest.log`:

```text
late-vf-ndc-linelist-slot0-xyzw-mesa-active-block-swiz-raster-wm-oa-probe
primary-single-raster-isolation-ndc-linelist-mesa-active-block-swiz
```

This selected single-raster index 10 rung tests a CL-producing VF line using
clip-space/NDC coordinates and Mesa's active CLIP/SF/RASTER/backend block before
the real-VS probes. It uses slot0 XYZW, LINELIST topology, SF viewport transform
enabled, perspective divide enabled, zero-length SBE reads, an explicit zeroed
SBE_SWIZ packet, Mesa-active sample mask/depth/WM/PS_BLEND dwords, and Mesa's
active extended 3DPRIMITIVE shape.

Evidence:

```text
fragment-candidate-shape accepted=1 geometry=ndc-line-center-32
wm-input-reference ... expected_samples=0 bbox=0x0..0x0
probe-mesa-active-block ... clip=[0x00040400,0xD4000001,0x0003FFE0] sf=[0x00080402,0x00000000,0x02004808] raster=[0x04A11003,0x00000000,0x00000000,0x00000000] sample_mask=0x0000FFFF wm=0x80000040 ps_blend=0x518C6200
probe-3dprimitive-extended ... vf_topology=linelist primitive_dw1=0x00000002
probe-sbe-decoded read_offset=0 read_length=0 attr_swizzle=0 emit_sbe_swiz=1 num_sf_outputs=0
probe-clip-decoded topo=linelist ... ClipMode=0(CLIPMODE_NORMAL) PerspectiveDivideDisable=0 EarlyCullEnable=1
probe-sf-decoded sf_dw=[0x00080402,0x00000000,0x02004808] ViewportTransformEnable=1
probe-raster-decoded ... z_near_clip=1 z_far_clip=1 raster_scissor_legacy_bit=1 sample_mask=0xFFFF
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=0 delta_ps=0
vf-proof accepted=1 ia_vtx_delta=2 ia_prim_delta=1
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=0
ps-dispatch-proof accepted=0 ps_delta=0 cps_delta=0 ps_depth_delta=0
render-target completed=1 any_change=0 triangle_change=0
scratch-rt-fragment-proof accepted=0 completed=1 changed=0 ps_delta=0
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
```

Interpretation: the Mesa-active fixed-function block did not turn a CL-positive
VF NDC line into WM/PSD work. The repaired OA ordering makes this a valid
counter-window negative, but the CPU reference for this exact line shape still
predicts zero covered samples, so the positive-coverage RECTLIST rung above is
the stronger CL-to-WM frontier proof.

## Latest Line Bary8 Swiz Probe

Measured on 2026-06-02 in `bld/baremetal-logs/latest.log`:

```text
late-vf-screen-linelist-slot0-xyzw-mesa-backend-open-bounds-sample1-onpixel-mesa-sf-point-sbe-read0-bary8-swiz-raster-wm-oa-probe
primary-single-raster-isolation-linelist-mesa-backend-open-bounds-sample1-onpixel-mesa-sf-point-sbe-read0-bary8-swiz
```

This selected single-raster index 14 rung keeps the previous strongest Bary8
line probe and changes only the SBE attribute-swizzle contract to match Mesa's
successful active SBE packet shape: `VertexURBEntryReadOffset=0`,
`VertexURBEntryReadLength=0`, `AttributeSwizzleEnable=1`, plus an explicit
zeroed `SBE_SWIZ` packet.

Evidence:

```text
wm-input-reference ... expected_samples=60 bbox=0x6..15x9
probe-mesa-active-block backend=raster-wm-input-oa-screen-space-linelist-mesa-backend-open-bounds-sample1-onpixel-mesa-sf-point-sbe-read0-bary8-swiz ... wm=0x80004040
probe-sbe-decoded read_offset=0 read_length=0 attr_swizzle=1 emit_sbe_swiz=1
probe-clip-decoded topo=linelist ... PerspectiveDivideDisable=1 NonPerspectiveBarycentricEnable=1
probe-sf-decoded sf_dw=[0x00400400,0x00000000,0x02004808] ... LineWidth=0x400 PointWidth=0x8 PointWidthSource=1
probe-raster-decoded ... dx_ms_enable=1 dx_ms_mode=2(on-pixel) force_multisampling=1 forced_samples=1(NUMRASTSAMPLES_1) rt_independent_raster=1 sample_mask=0xFFFF
probe-wm-decoded backend=raster-wm-input-oa-screen-space-linelist-mesa-backend-open-bounds-sample1-onpixel-mesa-sf-point-sbe-read0-bary8-swiz ... bary=0x08 ... ps_valid=1 ps_writes_rt=1
probe-ps-payload-decoded ... wm_bary=0x8 ps_dispatch_bits=100
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=1 delta_ps=0
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=1
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
render-target completed=1 any_change=0 triangle_change=0
primary-single-raster-isolation-ladder stopped index=14 completed=1
```

Interpretation: the Mesa-oracle SBE attr-swizzle bit was truly programmed and
decoded, but it still produced no raster samples, WM coverage, PSD dispatch, or
render-target change. This rules out the visible SBE/SBE_SWIZ contract as the
missing handoff bit for the strongest line probe.

## Latest Line Bary8 Probe

Measured on 2026-06-02 in `bld/baremetal-logs/latest.log`:

```text
late-vf-screen-linelist-slot0-xyzw-mesa-backend-open-bounds-sample1-onpixel-mesa-sf-point-sbe-read0-bary8-raster-wm-oa-probe
primary-single-raster-isolation-linelist-mesa-backend-open-bounds-sample1-onpixel-mesa-sf-point-sbe-read0-bary8
```

This selected single-raster index 13 rung keeps the strongest CL-producing
screen-space LINELIST path: Mesa backend gate state, `NUMRASTSAMPLES_1`,
on-pixel WM_INT/SF_INT raster mode, Mesa SF point `dw3`, and SBE read length
zero. It changes only the rect-style barycentric contract: CLIP
`NonPerspectiveBarycentricEnable=1` and WM barycentric mode `0x08`.

Evidence:

```text
wm-input-reference ... expected_samples=60 bbox=0x6..15x9
probe-mesa-active-block backend=raster-wm-input-oa-screen-space-linelist-mesa-backend-open-bounds-sample1-onpixel-mesa-sf-point-sbe-read0-bary8 ... wm=0x80004040
probe-sbe-decoded read_offset=0 read_length=0 attr_swizzle=0 emit_sbe_swiz=1
probe-clip-decoded topo=linelist ... PerspectiveDivideDisable=1 NonPerspectiveBarycentricEnable=1
probe-sf-decoded sf_dw=[0x00400400,0x00000000,0x02004808] ... LineWidth=0x400 PointWidth=0x8 PointWidthSource=1
probe-raster-decoded ... dx_ms_enable=1 dx_ms_mode=2(on-pixel) force_multisampling=1 forced_samples=1(NUMRASTSAMPLES_1) rt_independent_raster=1 sample_mask=0xFFFF
probe-wm-decoded backend=raster-wm-input-oa-screen-space-linelist-mesa-backend-open-bounds-sample1-onpixel-mesa-sf-point-sbe-read0-bary8 ... bary=0x08 ... ps_valid=1 ps_writes_rt=1
probe-ps-payload-decoded ... wm_bary=0x8 ps_dispatch_bits=100
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=1 delta_ps=0
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=1
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
render-target completed=1 any_change=0 triangle_change=0
primary-single-raster-isolation-ladder stopped index=13 completed=1
```

Interpretation: unlike the first attempted bary8 run, this capture really did
program and decode the barycentric contract. It still produced no raster
samples, WM coverage, PSD dispatch, or render-target change. An all-zero WM
barycentric mode was not the missing handoff bit for the strongest line probe.

## Latest NDC Viewport Rect Probe

Measured on 2026-06-02 in `bld/baremetal-logs/latest.log`:

```text
late-vf-ndc-rectlist-slot0-xyzw-sbe1-mesa-order-clip-on-early-backend-raster-wm-oa-probe
primary-single-raster-isolation-ndc-viewport
```

This selected single-raster index 12 rung tests whether the earlier
pretransformed-screen/PerspectiveDivideDisable contract was blocking SF to WM
scan conversion. It uses a fullscreen NDC RECTLIST, SF viewport transform
enabled, perspective divide enabled, SBE read offset/length 1, barycentric mode
`0x08`, and the same OA/scratch-RT proof window.

Evidence:

```text
fragment-candidate-shape accepted=1 geometry=ndc-rect-fullscreen coverage_contract=ndc-viewport-transform
wm-input-reference ... expected_samples=136 bbox=0x0..15x15
probe-sbe-decoded read_offset=1 read_length=1 attr_swizzle=0 emit_sbe_swiz=0 num_sf_outputs=0 force_offset=1 force_length=1
probe-sf-decoded sf_dw=[0x00080402,0x00000000,0x80000808] ViewportTransformEnable=1 ... LineWidth=0x80 PointWidth=0x8
probe-raster-decoded ... front=cw api_mode=0(DX9/OGL) z_near_clip=0 z_far_clip=0 raster_scissor_legacy_bit=1 sample_mask=0x1
probe-wm-decoded backend=raster-wm-input-oa-ndc-rectlist-slot0-xyzw-sbe1-mesa-order-clip-on-early-backend ... bary=0x08 ... ps_valid=1 ps_writes_rt=1
probe-sf-wm-contract topo=rectlist vertex_count=3 ... coord_contract=clip-space-with-sf-viewport-transform clip_mode=4(CLIPMODE_ACCEPT_ALL) pdd=0 nonpersp_bary=1 sf_viewport_transform=1 ... first_unproven=sf_object_setup_to_wm_scan_conversion
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=1 delta_ps=0
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=1
raster-packet-proof accepted=1 ... does_not_prove=fragment_samples_or_ps
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
render-target completed=1 any_change=0 triangle_change=0
primary-single-raster-isolation-ladder stopped index=12 completed=1
```

Interpretation: a positive CPU coverage candidate in NDC+viewport space still
reaches CL and retires raster packet markers, but it produces no hardware
raster samples, WM coverage, PSD dispatch, or render-target change. The
screen-space/PDD ambiguity is not the missing handoff bit.

## Latest NDC Pointlist Probe

Measured on 2026-06-02 in `bld/baremetal-logs/trueos-baremetal.1.log`:

```text
late-vf-ndc-pointlist-slot0-xyzw-open-bounds-raster-wm-oa-probe
primary-single-raster-isolation-ndc-pointlist-open-bounds
```

This selected single-raster index 11 rung moved the pointlist/open-bounds probe
to NDC with SF viewport transform enabled and perspective divide enabled.

Evidence:

```text
probe-sbe-decoded read_offset=0 read_length=1 attr_swizzle=0 emit_sbe_swiz=0
probe-sf-decoded sf_dw=[0x00080402,0x00000000,0x80000FFF] ViewportTransformEnable=1 ... PointWidth=0x7FF
probe-raster-decoded ... front=ccw api_mode=2(DX10.1+) z_near_clip=0 z_far_clip=0 raster_scissor_legacy_bit=0 sample_mask=0x1
probe-wm-decoded backend=raster-wm-input-oa-ndc-pointlist-open-bounds-walk16-sf-viewport ... bary=0x00 force_thread_dispatch=2(ForceON) ps_valid=1 ps_writes_rt=1
probe-sf-wm-contract topo=pointlist vertex_count=1 coord_contract=clip-space-with-sf-viewport-transform ... pdd=0 sf_viewport_transform=1 ... first_unproven=sf_object_setup_to_wm_scan_conversion
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=1 delta_ps=0
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=1
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
render-target completed=1 any_change=0 triangle_change=0
```

Interpretation: the NDC point path also dies after CL. Its CPU reference is a
weaker positive-coverage candidate than the index 12 NDC rect, but it still
rules out SF viewport transform/perspective-divide as a simple fix for the
pointlist path.

## Latest SBE-Read0 Probe

Measured on 2026-06-02 in `bld/baremetal-logs/latest.log`:

```text
late-vf-screen-linelist-slot0-xyzw-mesa-backend-open-bounds-sample1-onpixel-mesa-sf-point-sbe-read0-raster-wm-oa-probe
primary-single-raster-isolation-linelist-mesa-backend-open-bounds-sample1-onpixel-mesa-sf-point-sbe-read0
```

This selected single-raster index 9 rung keeps the previous CL-producing
screen-space LINELIST + Mesa backend-gate + `NUMRASTSAMPLES_1` + on-pixel
WM_INT/SF_INT + Mesa-SF-point `dw3` contract. It changes only SBE
`VertexURBEntryReadLength` from 1 to 0 while keeping forced SBE read
offset/length enabled.

Evidence:

```text
probe-preclip-sbe-ps-state ... sbe_read_offset=0 sbe_read_length=0
probe-mesa-active-block backend=raster-wm-input-oa-screen-space-linelist-mesa-backend-open-bounds-sample1-onpixel-mesa-sf-point-sbe-read0 ... sf=[0x00400400,0x00000000,0x02004808] raster=[0x00855800,0x00000000,0x00000000,0x00000000] sample_mask=0x0000FFFF
probe-sbe-decoded read_offset=0 read_length=0 attr_swizzle=0 emit_sbe_swiz=1 num_sf_outputs=0 force_offset=1 force_length=1
probe-sf-decoded sf_dw=[0x00400400,0x00000000,0x02004808] ... LineWidth=0x400 PointWidth=0x8 PointWidthSource=1 AALineDistanceMode=1 LastPixelEnable=0 TriangleFanProvokingVertexSelect=1
probe-raster-decoded ... dx_ms_enable=1 dx_ms_mode=2(on-pixel) force_multisampling=1 forced_samples=1(NUMRASTSAMPLES_1) rt_independent_raster=1 wm_int_ms_raster_mode=on-pixel sample_mask=0xFFFF
probe-sf-wm-contract topo=linelist vertex_count=2 ... first_unproven=sf_object_setup_to_wm_scan_conversion
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=1 delta_ps=0
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=1
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
render-target completed=1 any_change=0 triangle_change=0
primary-single-raster-isolation-ladder stopped index=9 completed=1
```

Interpretation: SBE read-length zero was encoded before CLIP and decoded in the
active state, but it did not produce raster samples, WM coverage, PSD dispatch,
or render-target change. The no-attribute SBE contract is not the missing
handoff bit for the current strongest no-VS line probe.

## Latest Mesa-SF-Point Probe

Measured on 2026-06-02 in `bld/baremetal-logs/latest.log`:

```text
late-vf-screen-linelist-slot0-xyzw-mesa-backend-open-bounds-sample1-onpixel-mesa-sf-point-raster-wm-oa-probe
primary-single-raster-isolation-linelist-mesa-backend-open-bounds-sample1-onpixel-mesa-sf-point
```

This selected single-raster index 8 rung keeps the previous CL-producing
screen-space LINELIST + Mesa backend-gate + `NUMRASTSAMPLES_1` + on-pixel
WM_INT/SF_INT raster-mode contract. It changes only SF `dw3` to Mesa's
fixed-function point-width/source/provoking value while leaving line width at
8 pixels.

Evidence:

```text
probe-mesa-active-block backend=raster-wm-input-oa-screen-space-linelist-mesa-backend-open-bounds-sample1-onpixel-mesa-sf-point ... sf=[0x00400400,0x00000000,0x02004808] raster=[0x00855800,0x00000000,0x00000000,0x00000000] sample_mask=0x0000FFFF
probe-sf-decoded sf_dw=[0x00400400,0x00000000,0x02004808] ... LineWidth=0x400 PointWidth=0x8 PointWidthSource=1 AALineDistanceMode=1 LastPixelEnable=0 TriangleFanProvokingVertexSelect=1
probe-raster-decoded ... dx_ms_enable=1 dx_ms_mode=2(on-pixel) force_multisampling=1 forced_samples=1(NUMRASTSAMPLES_1) rt_independent_raster=1 wm_int_ms_raster_mode=on-pixel sample_mask=0xFFFF
probe-sf-wm-contract topo=linelist vertex_count=2 ... first_unproven=sf_object_setup_to_wm_scan_conversion
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=1 delta_ps=0
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=1
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
render-target completed=1 any_change=0 triangle_change=0
primary-single-raster-isolation-ladder stopped index=8 completed=1
```

Interpretation: Mesa-like SF `dw3` was correctly encoded and decoded, but it
did not produce raster samples, WM coverage, PSD dispatch, or render-target
change. The strongest no-VS line probe still dies after CL accepts the primitive
and before SF/WM scan conversion reports work.

## Latest On-Pixel MS Raster Probe

Measured on 2026-06-02 in `bld/baremetal-logs/latest.log`:

```text
late-vf-screen-linelist-slot0-xyzw-mesa-backend-open-bounds-sample1-onpixel-raster-wm-oa-probe
primary-single-raster-isolation-linelist-mesa-backend-open-bounds-sample1-onpixel
```

This selected single-raster index 7 rung keeps the CL-producing screen-space
LINELIST + Mesa backend-gate path, keeps `NUMRASTSAMPLES_1`, and additionally
forces the WM_INT/SF_INT multisample raster mode to on-pixel.

Evidence:

```text
probe-preclip-raster-state ... raster_dw=[0x00855800,0x00000000,0x00000000,0x00000000]
probe-mesa-active-block backend=raster-wm-input-oa-screen-space-linelist-mesa-backend-open-bounds-sample1-onpixel ... raster=[0x00855800,0x00000000,0x00000000,0x00000000] sample_mask=0x0000FFFF
probe-raster-decoded ... dx_ms_enable=1 dx_ms_mode=2(on-pixel) force_multisampling=1 forced_samples=1(NUMRASTSAMPLES_1) rt_independent_raster=1 wm_int_ms_raster_mode=on-pixel sample_mask=0xFFFF
probe-sf-wm-contract topo=linelist vertex_count=2 ... first_unproven=sf_object_setup_to_wm_scan_conversion
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=1 delta_ps=0
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=1
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
render-target completed=1 any_change=0 triangle_change=0
```

Interpretation: the on-pixel MS raster mode was correctly encoded and decoded,
but it also did not hand work into WM/PSD. The first-bad boundary remains after
CL accepts the primitive and before hardware reports raster samples or WM
coverage.

## Latest Sample-Count Probe

Measured on 2026-06-02 in `bld/baremetal-logs/latest.log`:

```text
late-vf-screen-linelist-slot0-xyzw-mesa-backend-open-bounds-sample1-raster-wm-oa-probe
primary-single-raster-isolation-linelist-mesa-backend-open-bounds-sample1
```

This selected single-raster index 6 rung keeps the CL-producing screen-space
LINELIST + Mesa backend-gate path, then forces the raster sample count from
`NUMRASTSAMPLES_0` to `NUMRASTSAMPLES_1`.

Evidence:

```text
probe-mesa-active-block backend=raster-wm-input-oa-screen-space-linelist-mesa-backend-open-bounds-sample1 ... raster=[0x00850000,0x00000000,0x00000000,0x00000000] sample_mask=0x0000FFFF wm=0x80000040 ps_blend=0x518C6200
probe-sbe-decoded read_offset=0 read_length=1 ... emit_sbe_swiz=1
probe-raster-decoded ... forced_samples=1(NUMRASTSAMPLES_1) rt_independent_raster=1 sample_mask=0xFFFF
probe-sf-wm-contract topo=linelist vertex_count=2 ... draw_rect=[0,0..65535,65535] first_unproven=sf_object_setup_to_wm_scan_conversion
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=1 delta_ps=0
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=1
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
render-target completed=1 any_change=0 triangle_change=0
```

Interpretation: forcing a real raster sample count was correctly encoded and
decoded, but it did not hand work into WM/PSD. The strongest frontier remains
SF/object setup to WM scan conversion after CL accepts the line primitive.

## Latest Real-VS Probe

Measured on 2026-06-02 in
`bld/baremetal-logs/latest.log -> trueos-baremetal.2.log`:

```text
real-vs-ndc-mesa-active-block-swiz-vs-grf2-urbdf8-vb0204400c-prim0-raster-wm-oa-probe
primary-single-raster-isolation-real-vs-ndc-mesa-active-block-swiz-vs-grf2-urbdf8-vb0204400c-prim0
```

This selected single-raster index 10 rung combines the real VS NDC
oversized-triangle path with Mesa-successful fixed-function/backend state while
intentionally emitting explicit `3DSTATE_SBE_SWIZ`. It also programs the VS
dispatch GRF-start field as `2`, matching the Mesa oracle active `3DSTATE_VS`
`dw6=0x00200800` packet instead of trusting the baked shader metadata value
`0`. The latest variant additionally matches Mesa's active VS
`URB_ALLOC_VS` entry count (`0x0DF8`, logged as `programmed_urb_entries=3576`)
and Mesa's active VS `dw8=0x00000000`. It also overrides the visible frontend
draw packet shape to match Mesa's active vertex-buffer `dw1=0x0204400C` and
extended `3DPRIMITIVE` `dw1=0x00000000`.

Evidence:

```text
fragment-candidate-shape accepted=1 geometry=oversized ... coverage_contract=oversized-triangle
probe-mesa-active-block backend=raster-wm-input-oa-ndc-mesa-active-block-swiz ... sample_mask=0x0000FFFF wm=0x80000040 ps_blend=0x518C6200
probe-sbe-decoded read_offset=1 read_length=1 ... emit_sbe_swiz=1
probe-vs ... dw6=0x00200800 dw7=0x88400405 dw8=0x00000000 ... programmed_urb_entries=3576 baked_grf_start=0 applied_grf_start=2
probe-frontend-dwords contract=mesa-vs-grf2-urbdf8-vb0204400c-prim0 vb=[0x78080003,0x0204400C,0x00870000,0x00000000,0x00000024] ve0=[0x78090001,0x02400000,0x11130000] vf_topology=[0x784B0000,0x00000004] primitive=[0x7B000808,0x00000000,0x00000003,0x00000000,0x00000001,0x00000000,...] mesa_active_vb_dw1=0x0204400C mesa_active_ve0=[0x78090001,0x02400000,0x11130000] mesa_active_vf_topology=0x00000004 mesa_active_primitive_ext=[0x7B000808,0x00000000,0x00000003,0x00000000,0x00000001,0x00000000,...]
probe-handoff-decoded ... programmed_vs_urb_entries=3576 baked_vs_grf_start=0 programmed_vs_grf_start=2 sbe_read_offset=1 sbe_read_len=1
probe-sf-wm-contract topo=trilist ... coord_contract=clip-space-with-sf-viewport-transform clip_mode=0(CLIPMODE_NORMAL) pdd=0 sf_viewport_transform=1
stage-diagnosis completed=1 verdict=completed delta_vs=3 delta_cl=0 delta_cl_prim=0 delta_ps=0
vf-proof accepted=1 ia_vtx_delta=3 ia_prim_delta=1
vs-proof accepted=1 vs_delta=3
clip-raster-proof accepted=0 cl_delta=0 cl_prim_delta=0 clip_counter_required=1
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
```

Interpretation: the visible Mesa-oracle VS and frontend dwords tested here are
not the missing handoff. The real-VS path proves VF and VS work with these
Mesa-matching active VS fields and draw packet dwords, but still does not
create CL input or raster/WM coverage. The next real-VS suspect is VS output
VUE/URB content or hidden/adjacent frontend state, not the visible
vertex-buffer `dw1`, first vertex element, VF topology, or extended
`3DPRIMITIVE` first dwords. The VF NDC TRIANGLELIST path remains the strongest
SF/raster frontier probe because it uses full positive CPU coverage, Mesa-active
fixed-function state, `vf_topology=trilist`, `primitive_dw1=0`, and still goes
flat at raster samples/WM coverage after CL input.

## Latest Real-VS Streamout Probe

Measured on 2026-06-02 in
`bld/baremetal-logs/latest.log -> trueos-baremetal.2.log`.

This probe tried to use SOL/streamout as a VS-output oracle before the selected
real-VS raster rung. The streamout batch now accepts the same frontend-contract
overrides as the raster batch, so the proof can test both the default Mesa-like
VS contract and the selected GRF2/URBDF8 contract.

Evidence:

```text
vs-streamout-proof contract experiment=header+pos-slots01 ... urb_vs[alloc_len=1 start=4 entries=192] ... topo=pointlist
vs-streamout-proof stage-frontier completed=0 ... clip_raster_packets=0 ... counter_frontier=vs-only-counters note=draw_not_retired_before_light_sync
vs-streamout-proof stage-diagnosis completed=0 verdict=vs-progress-no-sol-write-or-offset delta_vs=3 delta_cl=0 delta_so0=0 delta_so_write0=0
vs-streamout-proof soft-accept accepted=0 ... delta_ia_vtx=3 delta_ia_prim=3 delta_vs=3 delta_so0=0 delta_so_write0=0 min_streamout_bytes=96
vs-streamout-proof v0 experiment=header+pos-slots01 completed=0 hdr=[0,0,0,0] pos=[0,0,0,0] pos_f=[0.000,0.000,0.000,0.000]
primary-real-vs-output-baseline experiment=header+pos-slots01 accepted=0 contract=mesa-like geometry=oversized

vs-streamout-proof contract experiment=pos-slot0-xyzw ... urb_vs[alloc_len=1 start=4 entries=3576] ... topo=pointlist
vs-streamout-proof stage-diagnosis completed=0 verdict=vs-progress-no-sol-write-or-offset delta_vs=3 delta_cl=0 delta_so0=0 delta_so_write0=0
vs-streamout-proof soft-accept accepted=0 ... delta_ia_vtx=3 delta_ia_prim=3 delta_vs=3 delta_so0=0 delta_so_write0=0 min_streamout_bytes=48
vs-streamout-proof v0 experiment=pos-slot0-xyzw completed=0 raw=[0,0,0,0] pos=[0.000,0.000,0.000,0.000]
primary-real-vs-output-proof experiment=pos-slot0-xyzw accepted=0 contract=mesa-vs-grf2-urbdf8-vb0204400c-prim0 geometry=oversized

vs-streamout-proof contract experiment=header+pos-slots01 ... urb_vs[alloc_len=1 start=4 entries=3576] ... topo=pointlist
vs-streamout-proof stage-diagnosis completed=0 verdict=vs-progress-no-sol-write-or-offset delta_vs=3 delta_cl=0 delta_so0=0 delta_so_write0=0
primary-real-vs-output-proof experiment=header+pos-slots01 accepted=0 contract=mesa-vs-grf2-urbdf8-vb0204400c-prim0 geometry=oversized
```

Interpretation: this is not proof that the VS wrote zero VUE bytes. The
baseline Mesa-like streamout attempt fails in the same shape as the selected
GRF2/URBDF8 attempts: IA and VS counters advance, but the draw does not retire
past the light sync marker, SOL counters stay flat, and the streamout buffer
remains zero. Treat the current SOL proof as a failed oracle. It still narrows
the real-VS track: the next useful proof must either repair/baseline streamout
itself or use a different way to inspect/infer the VUE handed from VS to CL.

## Strongest Current Probe

The strongest isolation probe tried so far is:

```text
late-vf-ndc-trilist-slot0-xyzw-mesa-active-block-swiz-raster-wm-oa-probe
```

It uses:

- VF-synthesized VUE.
- Position in VUE slot 0 as full XYZW.
- No fake header slot.
- TRIANGLELIST topology with an oversized NDC triangle.
- SF viewport transform enabled and perspective divide enabled.
- CLIP normal classification with guardband and viewport XY clip tests enabled.
- Mesa's active CLIP/SF/RASTER fixed-function block stamped before draw.
- Mesa's active backend gate values stamped after raster:
  `sample_mask=0x0000FFFF`, `wm_depth=[0x00000010,0,0]`,
  `primitive_replication=0x00010000`, `wm=0x80000040`,
  `ps_blend=0x518C6200`.
- Explicit zeroed `3DSTATE_SBE_SWIZ` emitted (`emit_sbe_swiz=1`).
- Extended 3DPRIMITIVE with `vf_topology=trilist` and `primitive_dw1=0`.
- PS state armed and KSP validated.
- OA begin/end reports valid.

This probe is important because it removes point/line coverage quirks, the
RECTLIST topology mismatch, and the extended primitive-DW1 mismatch from the
first-bad set. The CPU reference expects full diagnostic-grid coverage
(`expected_samples=256`). CL consumes the primitive (`cl_delta=1`), but hardware
still reports zero raster samples, zero WM coverage, and zero PSD dispatch.

## Additional 2026-06-02 Ruled-Out Probes

The NDC pointlist/open-bounds variant:

```text
late-vf-ndc-pointlist-slot0-xyzw-open-bounds-raster-wm-oa-probe
```

decoded with SF viewport transform enabled, perspective divide enabled, and
POINTLIST topology. It still completed with CL primitive input but reported:

```text
wm_coverage=0 psd_dispatch=0 fragment_observed=0
raster_samples_delta=0 ps_threads_delta=0
```

The screen-space pointlist/open-bounds `CLIPMODE_NORMAL` variant:

```text
late-vf-screen-pointlist-slot0-xyzw-clip-normal-open-bounds-raster-wm-oa-probe
```

cleanly changed only ClipMode from ACCEPT_ALL to NORMAL and still reported:

```text
ClipMode=0(CLIPMODE_NORMAL)
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps
wm_coverage=0 psd_dispatch=0 fragment_observed=0
```

The screen-space linelist/open-bounds variant:

```text
late-vf-screen-linelist-slot0-xyzw-open-bounds-raster-wm-oa-probe
```

decoded the intended line contract:

```text
topo=linelist vertex_count=2
LineWidth=0x400
wm-input-reference expected_samples=60
vf-proof accepted=1 ia_vtx_delta=2 ia_prim_delta=1
clip-raster-proof accepted=1 cl_delta=1 cl_prim_delta=1
```

but still failed at the same handoff:

```text
ps-dispatch-proof accepted=0 ps_delta=0 cps_delta=0 ps_depth_delta=0
raster-wm-input-proof accepted=0 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw any_delta=0 nonzero_count=0
```

The screen-space linelist plus Mesa-successful backend-gate variant:

```text
late-vf-screen-linelist-slot0-xyzw-mesa-backend-open-bounds-raster-wm-oa-probe
```

kept the same VF/no-VS line object setup but stamped the successful Mesa
backend values after raster and emitted explicit SBE_SWIZ:

```text
probe-mesa-active-block ... sample_mask=0x0000FFFF wm_depth=[0x00000010,0,0] prim_repl=0x00010000 wm=0x80000040 ps_blend=0x518C6200
probe-sbe-decoded read_offset=0 read_length=1 attr_swizzle=0 emit_sbe_swiz=1
probe-backend-gate ... thread_dispatch_enable=1 reason=writeable-rt
probe-sf-wm-contract topo=linelist vertex_count=2 primitive_objects_expected=1 ... first_unproven=sf_object_setup_to_wm_scan_conversion
```

The corrected hardware result is still negative:

```text
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps delta_cl=1 delta_cl_prim=1 delta_ps=0
scratch-rt-fragment-proof accepted=0 completed=1 ... changed=0 ps_delta=0 cps_delta=0 ps_depth_delta=0
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
```

## Evidence From The Pointlist Open-Bounds Probe

Useful positive evidence:

```text
fragment-candidate-shape accepted=1
probe-vf-vue-contract experiment=pos-slot0-xyzw ... header_slot=none position_slot=slot0=xyzw
probe-clip-decoded topo=pointlist ... ClipMode=ACCEPT_ALL ... ClipEnable=1 PerspectiveDivideDisable=1
probe-sf-decoded ... PointWidth=0x40 ... DerefBlockSize=0(Block32)
probe-raster-decoded ... raster_scissor_legacy_bit=0 ... sample_mask=0x1
probe-wm-decoded ... force_thread_dispatch=2(ForceON) ... wm_hz_op=0 wm_hz_scissor=0
probe-sf-wm-contract topo=pointlist vertex_count=3 primitive_objects_expected=3 ... draw_rect=[0,0..65535,65535]
stage-diagnosis completed=1 verdict=stops-between-clipper-and-ps ... delta_cl=3 delta_cl_prim=3 delta_ps=0
vf-proof accepted=1 ia_vtx_delta=3 ia_prim_delta=3
clip-raster-proof accepted=1 cl_delta=3 cl_prim_delta=3
raster-packet-proof accepted=1 post_clip=0xC0DE7726 post_raster=0xC0DE7727
```

Negative evidence:

```text
raster-wm-input-proof accepted=0 completed=1 reports_valid=1 wm_coverage=0 psd_dispatch=0 raster_samples_delta=0 ps_threads_delta=0
raster-wm-oa-raw reports_valid=1 any_delta=0 nonzero_count=0
ps-dispatch-proof accepted=0 ps_delta=0 cps_delta=0 ps_depth_delta=0
scratch-rt-fragment-proof accepted=0 ... changed=0
render-target completed=1 any_change=0 triangle_change=0
```

The engine is not just failing to write the render target. The OA window sees
no raster samples and no PS thread dispatch.

## Ruled Out For This Frontier

The following are no longer good first suspects for the current track:

- Missing VS execution: VF-synthesized VUE path intentionally has `vs=0`; that
  is expected and logged.
- HS, DS, GS thread counters: also expected to be zero on this path.
- Fake VUE header slot: pointlist slot0 XYZW path has no fake header.
- Position slot ambiguity: position is now explicitly slot0 XYZW.
- Triangle edge equations: pointlist avoids triangle/rect edge setup.
- Point coverage/object setup: linelist with nonzero expected CPU coverage
  still fails.
- RECTLIST topology or extended 3DPRIMITIVE `dw1` mismatch: the selected NDC
  oversized TRIANGLELIST rung uses `vf_topology=trilist`,
  `primitive_dw1=0x00000000`, `expected_samples=256`, and valid OA reports, but
  still has `raster_samples_delta=0`, `wm_coverage=0`, and `psd_dispatch=0`.
- PDD/SF viewport-transform coordinate contract: the selected screen-space
  oversized TRIANGLELIST rung keeps `vf_topology=trilist`,
  `primitive_dw1=0x00000000`, and `expected_samples=256`, but switches to
  `PerspectiveDivideDisable=1` with SF viewport transform disabled; it reaches
  `cl_prim_delta=1` and still reports zero raster samples, WM coverage, and PSD
  dispatch.
- Over-minimized SF packet shape on the screen-space TRIANGLELIST: the SF-sane
  selected rung restores `sf=[0x00080400,0x00000000,0x80000808]`,
  `LineWidth=0x80`, `PointWidth=0x8`, and `LastPixelEnable=1`; it still reaches
  `cl_prim_delta=1` with `expected_samples=256`, but reports zero raster
  samples, WM coverage, and PSD dispatch.
- Missing TRIANGLELIST barycentric mode: the selected SF-sane+bary8 rung sets
  `NonPerspectiveBarycentricEnable=1`, WM `bary=0x08`, and PS payload
  `wm_bary=0x8` while keeping `expected_samples=256` and `cl_prim_delta=1`; it
  still reports zero raster samples, WM coverage, and PSD dispatch.
- Missing VF-synthesized VUE header/position layout: the selected header+pos1
  rung uses `HeaderAndPositionSlots01` / `slot0=header slot1=position`, with
  position at offset 16, forced W=1, `expected_samples=256`, and
  `cl_prim_delta=1`; it still reports zero raster samples, WM coverage, and PSD
  dispatch.
- Missing Mesa active RASTER `dw1`: the selected header+pos1 Mesa-raster rung
  programs `raster=[0x04A11003,0,0,0]`, decoded as `front=ccw`,
  `z_near_clip=1`, `z_far_clip=1`, and `dx_ms_enable=1`, while keeping
  `expected_samples=256` and `cl_prim_delta=1`; it still reports zero raster
  samples, WM coverage, and PSD dispatch.
- Missing Mesa active SF body: the selected header+pos1
  Mesa-raster+Mesa-SF rung programs `sf=[0x00080402,0,0x02004808]` with
  `expected_samples=256` and `cl_prim_delta=1`; it still reports zero raster
  samples, WM coverage, and PSD dispatch.
- Missing Mesa active SBE body: the selected header+pos1
  Mesa-raster+Mesa-SF+Mesa-SBE rung programs
  `sbe=[0x30200000,0,0,0xFFFFFFFF,0xFFFFFFFF]`, decoded as read offset/length
  0/0 with attribute swizzle enabled, while keeping `expected_samples=256` and
  `cl_prim_delta=1`; it still reports zero raster samples, WM coverage, and
  PSD dispatch.
- Mesa active PS binding-table entry count on the selected Mesa-SBE path: the
  PS-BT0 contract snapshot programs/logs `ps[... bt_count=0 ...]` and
  `trueos_ps[... binding_table_entry_count=0 ...]`, matching the host Mesa
  oracle, but still reports zero raster samples, WM coverage, and PSD dispatch.
- Missing SBE_SWIZ packet: the corrected linelist/Mesa-backend probe emits
  SBE_SWIZ and still fails.
- Missing simple WM backend gate state: Mesa-successful sample mask,
  WM_DEPTH_STENCIL, PRIMITIVE_REPLICATION, WM, and PS_BLEND values are present
  and the decoded backend gate says `thread_dispatch_enable=1`.
- VS dispatch GRF-start, VS `dw8`, VS URB entry-count, visible vertex-buffer
  `dw1`, first vertex element, VF topology, or extended `3DPRIMITIVE` first
  dword mismatch versus Mesa: the real-VS GRF2/URBDF8/VB0204400C/PRIM0 probe
  programs `programmed_vs_grf_start=2`, `dw6=0x00200800`,
  `dw8=0x00000000`, `programmed_vs_urb_entries=3576`,
  `vb_dw1=0x0204400C`, `ve0=[0x78090001,0x02400000,0x11130000]`,
  `vf_topology=0x00000004`, and `3DPRIMITIVE dw1=0`, but still reports
  `delta_vs=3`, `delta_cl=0`, `wm_coverage=0`, and `psd_dispatch=0`.
- The current streamout proof as a VUE-byte oracle: default Mesa-like and
  selected GRF2/URBDF8 streamout probes both report `delta_vs=3` with
  `delta_so0=0`, `delta_so_write0=0`, and zero output bytes, so the proof path
  itself must be repaired or independently baselined before its buffer contents
  can be used to judge VS output.
- SF viewport transform / clip-space coordinates: NDC pointlist still fails, and
  the screen-space TRIANGLELIST Mesa-active block fails with the host-style
  PDD/screen-space contract.
- Tight scissor or tight draw rectangle: open-bounds pointlist still fails with
  WM scissor disabled and a full drawing rectangle.
- Basic CLIP pass-through: CL counters report point and line primitives.
- ACCEPT_ALL versus NORMAL ClipMode: both pointlist variants fail identically
  at WM/PSD handoff.
- PS kernel pointer or PS state absence: KSP and PS state markers are present.
- RT visibility alone: no WM coverage or PSD dispatch precedes any RT write.

## Current First-Bad Name

Use this phrase consistently:

```text
SF object setup to WM scan-conversion state-packet mismatch
```

This means the software-visible packets are sufficient to get VF and CL input
and to retire the raster packet markers, but the hidden SF-to-WM object payload
does not appear to produce WM coverage.

## Current Code Anchors

The current focused probe is added in:

```text
src/intel/render/primary.rs
```

Backend mode and submit-name classification live in:

```text
src/intel/render/state.rs
```

Packet construction and decoded proof logging live in:

```text
src/intel/render/pipeline.rs
src/intel/render/submit.rs
```

The active screen-space TRIANGLELIST SF-sane+bary8 header
Mesa-raster+Mesa-SF+Mesa-SBE mode is:

```text
BackendProbeMode::RasterWmInputOaScreenSpaceTriListMesaActiveBlockSwizSfSaneBary8HeaderMesaRasterSfMesaSbe
```

That selected mode currently also forces the PS binding-table entry count to
zero through `force_ps_binding_table_count_zero()` so the visible PS state
matches the Mesa active oracle for this field.

The submit label is:

```text
late-vf-screen-trilist-header-pos1-mesa-raster-mesa-sf-mesa-sbe-bary8-raster-wm-oa-probe
```

## Host Oracle Track

There are two different host-side tracks. Keep them separate.

The sane track is a host Mesa render oracle: run a known-good Intel Vulkan
triangle under the i915 ioctl tracer, preserve its execbuffer BO dumps, and
compare Mesa's render packet/state setup against TRUEOS. This is the right next
tool for the WM frontier because the current suspect is an SF-to-WM state
contract, not arbitrary ring replay.

The command is:

```sh
bash tools/intel_userland_oracle/run_render_triangle_oracle.sh
```

The default output directory is:

```text
.codex_tmp/intel_userland_oracle/render-simple-triangle
```

The useful files are:

```text
simple_triangle_dump.log
pipeline_exec/host_state_reference.txt
pipeline_exec/*GEN_Assembly.txt
log.txt
replay_manifest.json
summary.txt
```

The host triangle must say `simple_triangle_dump: verified=1` before treating
the captured packets as a good oracle. If it does not verify, discard that
capture for WM comparison.

The insane track is the replay path:

```sh
python3 tools/intel_userland_oracle/extract_replay_manifest.py TRACE/log.txt --hash > TRACE/replay_manifest.json
python3 tools/intel_userland_oracle/emit_replay_artifact.py TRACE/replay_manifest.json --out src/intel/replay_*.rs --name NAME
```

That preserves concrete BO GPU VAs and batches for baremetal replay. It is
useful later, but it should not be the first WM experiment because replaying a
captured ring submission drags in arbitrary host driver state and does not
explain which visible packet contract TRUEOS is missing.

Existing historical evidence:

```text
crates/trueos-shader/host_shader_validation/simple_triangle_dump.log
.codex_tmp/intel_userland_oracle/adapterlibgfx-rotating-intel/log.txt
```

The first file is the older verified Intel simple-triangle host proof. The
second is a noisier render-ish execbuffer capture suitable for replay/autopsy,
not a clean first oracle for the current WM boundary.

## Next Useful Experiments

Do not start by changing only VF geometry again. The better next probes should
test whether the real-VS output/VUE contract or hidden/adjacent frontend state
is the missing ingredient, now that the selected real-VS draw's visible
VB/VE/VF topology/primitive packet dwords match Mesa's active shape.

Good next candidates:

1. Emit the generic `3DSTATE_VIEWPORT_STATE_POINTERS` zero-pointer packet on
   the selected Mesa-SBE screen-space TRIANGLELIST path, matching Mesa's active
   `780D0000 00000000` packet. The latest contract snapshot shows TRUEOS emits
   SF_CLIP and CC viewport pointer packets, but not this generic packet.
2. Repair or replace the VS-output oracle. The current SOL/streamout proof
   advances VS but never writes streamout bytes even under the default Mesa-like
   contract, so it cannot yet prove VUE contents.
3. Compare TRUEOS against the verified Mesa oracle above the visible packet
   values: VUE/URB contents, VF format, shader-provided position contract, and
   any hidden frontend state that Mesa establishes before `CLIP/SF/RASTER`.
4. Split the selected real-VS contract into GRF-start versus URB-entry-count
   probes only after the VS-output oracle is trustworthy, because the current
   streamout result cannot distinguish "bad GRF/URB contract" from "bad SOL
   proof shape".
5. Try the same line+Mesa backend with an alternate SBE read offset only if
   the real-VS path still fails; the current slot0 XYZW contract is otherwise
   already well exercised, and `read_offset=0/read_length=0/attr_swizzle=1`
   is now ruled out.
6. Instrument more SF/WM-related live registers around the screen-space and NDC
   TRIANGLELIST Mesa-active submits,
   especially anything that can distinguish "SF produced object setup" from
   "WM refused scan conversion".
7. Capture or refresh the host render oracle above, then compare any packet
   that affects SF output contracts, not just visible raster/scissor state.

Bad next candidates:

- More changes to VUE slot/header layout without a new Mesa/VUE hypothesis.
- Treating the current all-zero streamout buffer as proof that VS emitted zero
  positions.
- More PS binding-table-count toggles on the selected Mesa-SBE path; the Mesa
  oracle value 0 is now tested and still negative.
- More scissor/draw-rectangle probes; open-bounds did not move coverage.
- Treating `completed=1` or post-draw markers as a fence for WM coverage.
- PS payload spectrum before `wm_coverage=1` or `psd_dispatch=1`.

## Run Loop

The standard runtime loop for this frontier remains:

```sh
make iso
sleep 25
rg -n "late-vf-screen-trilist-slot0-xyzw-mesa-active-block-swiz|probe-3dprimitive-extended|probe-sbe-decoded|probe-mesa-active-block|raster-wm-input-proof|raster-wm-oa-raw|wm-boundary-regs|primary-single-raster-isolation-screen-trilist" bld/baremetal-logs/latest.log
```

If `latest.log` is stale or empty, inspect all three maintained rotating logs:

```sh
rg -n "late-vf-screen-trilist-slot0-xyzw-mesa-active-block-swiz|probe-3dprimitive-extended|probe-sbe-decoded|probe-mesa-active-block|primary-single-raster-isolation-screen-trilist" bld/baremetal-logs/trueos-baremetal.*.log
```
