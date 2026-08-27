const CPS_STATE_DWORDS_PER_VIEWPORT: usize = 8;
const CPS_STATE_VIEWPORTS: usize = 16;
const CPS_STATE_DWORDS: usize = CPS_STATE_DWORDS_PER_VIEWPORT * CPS_STATE_VIEWPORTS;
static CHURN_NATIVE_SURFACE_STATE_LOGGED: AtomicBool = AtomicBool::new(false);
static CHURN_NATIVE_BINDING_COMMAND_LOGGED: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct PixelShaderDispatchContract {
    dispatch_8: u32,
    dispatch_16: u32,
    dispatch_32: u32,
    vector_mask_enable: bool,
    grf_start_dw: u32,
    ksp: [u32; 3],
}

/// First payload DWORD of 3DSTATE_SBE_SWIZ.
///
/// Each attribute detail is 16 bits wide. Native Churn has two fragment
/// inputs, so identity routing is source 0 in the low half and source 1 in
/// the high half. Compatibility probes retain their historical all-zero
/// payload.
const fn sbe_swiz_payload(artifact_native_fixed_function: bool, native_sampled: bool) -> [u32; 2] {
    if artifact_native_fixed_function && native_sampled {
        // The first retained texture rung exports only UV at attribute 0.
        [0, 0]
    } else if artifact_native_fixed_function {
        [0x0001_0000, 0]
    } else {
        [0, 0]
    }
}

/// 3DSTATE_WM barycentric interpolation payload required by a fragment
/// executable with user varyings.
///
/// This follows the pixel-shader ABI, not the draw-source kind. HelioV's
/// sampled shader is an ordinary VF-fed draw, but it consumes one perspective
/// UV varying just as the artifact-native Churn shader consumes its inputs.
const fn wm_barycentric_mode(num_varying_inputs: u8, force_barycentric_planes: bool) -> u32 {
    if num_varying_inputs != 0 || force_barycentric_planes {
        1 << 11
    } else {
        0
    }
}

#[cfg(test)]
mod churn_sbe_swiz_tests {
    use super::{sbe_swiz_payload, wm_barycentric_mode};

    #[test]
    fn native_churn_routes_normal_and_material_identity() {
        assert_eq!(sbe_swiz_payload(true, false), [0x0001_0000, 0]);
        assert_eq!(sbe_swiz_payload(true, true), [0, 0]);
        assert_eq!(sbe_swiz_payload(false, false), [0, 0]);
    }

    #[test]
    fn sampled_fragment_varying_enables_perspective_pixel_payload() {
        assert_eq!(wm_barycentric_mode(1, false), 1 << 11);
        assert_eq!(wm_barycentric_mode(0, false), 0);
        assert_eq!(wm_barycentric_mode(0, true), 1 << 11);
    }
}

/// Resolve the gfx12 variable-pixel-dispatch mapping into the exact
/// 3DSTATE_PS fields we emit.
///
/// A single enabled SIMD16 executable belongs in KSP0. With SIMD8+SIMD16,
/// variable pixel dispatch maps SIMD8 to KSP0 and SIMD16 to KSP2. Keep the
/// captured Mesa pair when the base pipeline supplies both executables, but do
/// not accidentally turn every MesaLike fixed-function draw into a mixed-width
/// fragment launch.
fn pixel_shader_dispatch_contract(
    backend_probe_mode: BackendProbeMode,
    dispatch_mode: crate::intel::shader::DispatchMode,
    uses_vmask: bool,
    ps_ksp_base: u32,
    grf_start: u8,
    simd16_pair: Option<(u32, u8)>,
) -> PixelShaderDispatchContract {
    if matches!(backend_probe_mode, BackendProbeMode::MesaLike)
        && matches!(dispatch_mode, crate::intel::shader::DispatchMode::Simd8)
    {
        if let Some((simd16_ksp, simd16_grf_start)) = simd16_pair {
            return PixelShaderDispatchContract {
                dispatch_8: 1,
                dispatch_16: 1,
                dispatch_32: 0,
                vector_mask_enable: true,
                grf_start_dw: (u32::from(simd16_grf_start) << 16) | u32::from(grf_start),
                // KSP1 has no active width in an 8+16 pair. Mesa writes the
                // program base there; its GRF-start field remains zero.
                ksp: [ps_ksp_base, ps_ksp_base, simd16_ksp],
            };
        }
    }

    let stage_dispatch = match dispatch_mode {
        crate::intel::shader::DispatchMode::Simd8 => (1, 0, 0),
        crate::intel::shader::DispatchMode::Simd16 => (0, 1, 0),
        crate::intel::shader::DispatchMode::Simd32 => (0, 0, 1),
    };
    let (dispatch_8, dispatch_16, dispatch_32) = match backend_probe_mode {
        BackendProbeMode::PsDispatchSlot0 => (1, 0, 0),
        BackendProbeMode::PsDispatchSlot1 => (0, 1, 0),
        BackendProbeMode::PsDispatchSlot2 => (0, 0, 1),
        BackendProbeMode::PsDispatchAllKspSlots => (1, 1, 1),
        _ => stage_dispatch,
    };
    let ksp0 = if matches!(backend_probe_mode.ps_dispatch_slot(), Some(1 | 2)) {
        0
    } else {
        ps_ksp_base
    };
    let ksp1 = if matches!(
        backend_probe_mode,
        BackendProbeMode::PsDispatchSlot1 | BackendProbeMode::PsDispatchAllKspSlots
    ) {
        ps_ksp_base
    } else {
        0
    };
    let ksp2 = if matches!(
        backend_probe_mode,
        BackendProbeMode::PsDispatchSlot2 | BackendProbeMode::PsDispatchAllKspSlots
    ) {
        ps_ksp_base
    } else {
        0
    };
    let grf_start_dw = (u32::from(ksp0 != 0) * u32::from(grf_start))
        | ((u32::from(ksp1 != 0) * u32::from(grf_start)) << 8)
        | ((u32::from(ksp2 != 0) * u32::from(grf_start)) << 16);

    PixelShaderDispatchContract {
        dispatch_8,
        dispatch_16,
        dispatch_32,
        vector_mask_enable: uses_vmask,
        grf_start_dw,
        ksp: [ksp0, ksp1, ksp2],
    }
}

#[cfg(test)]
mod pixel_shader_dispatch_contract_tests {
    use super::{BackendProbeMode, PixelShaderDispatchContract, pixel_shader_dispatch_contract};
    use crate::intel::shader::DispatchMode;

    #[test]
    fn diagnostic_simd16_is_exclusive_and_uses_ksp0() {
        assert_eq!(
            pixel_shader_dispatch_contract(
                BackendProbeMode::MesaLike,
                DispatchMode::Simd16,
                true,
                0xC0,
                2,
                None,
            ),
            PixelShaderDispatchContract {
                dispatch_8: 0,
                dispatch_16: 1,
                dispatch_32: 0,
                vector_mask_enable: true,
                grf_start_dw: 0x0000_0002,
                ksp: [0xC0, 0, 0],
            }
        );
    }

    #[test]
    fn explicit_simd16_probe_uses_the_same_single_width_mapping() {
        let contract = pixel_shader_dispatch_contract(
            BackendProbeMode::PsSimd16,
            DispatchMode::Simd16,
            true,
            0xC0,
            2,
            None,
        );
        assert_eq!((contract.dispatch_8, contract.dispatch_16, contract.dispatch_32), (0, 1, 0));
        assert_eq!(contract.ksp, [0xC0, 0, 0]);
    }

    #[test]
    fn captured_simd8_simd16_pair_keeps_its_variable_dispatch_mapping() {
        assert_eq!(
            pixel_shader_dispatch_contract(
                BackendProbeMode::MesaLike,
                DispatchMode::Simd8,
                false,
                0xC0,
                2,
                Some((0x100, 2)),
            ),
            PixelShaderDispatchContract {
                dispatch_8: 1,
                dispatch_16: 1,
                dispatch_32: 0,
                vector_mask_enable: true,
                grf_start_dw: 0x0002_0002,
                ksp: [0xC0, 0xC0, 0x100],
            }
        );
    }
}

fn log_render_buffer_layout(warm: RenderWarmState, rt_gpu_addr: Option<u64>) {
    if !crate::log_os::flags::INTEL_RENDER_NGIN_LOGS || crate::log_os::flags::INTEL_STAGE1_LOGS {
        return;
    }
    let rt_gpu_addr = rt_gpu_addr.unwrap_or(0);
    intel_render_verbose_log!(
        "buffers ring phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} context phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} batch phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} result phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} streamout phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} state phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} vertex phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rt_ggtt=0x{:X}\n",
        warm.ring_phys,
        GPU_VA_RING_BASE,
        warm.ring_len,
        warm.context_phys,
        GPU_VA_CONTEXT_BASE,
        warm.context_len,
        warm.batch_phys,
        GPU_VA_BATCH_BASE,
        warm.batch_len,
        warm.result_phys,
        GPU_VA_RESULT_BASE,
        warm.result_len,
        warm.streamout_phys,
        GPU_VA_STREAMOUT_BASE,
        warm.streamout_len,
        warm.draw_state_phys,
        GPU_VA_DRAW_STATE_BASE,
        warm.draw_state_len,
        warm.vertex_phys,
        GPU_VA_VERTEX_BASE,
        warm.vertex_len,
        rt_gpu_addr
    );
}

fn log_render_packet_encodings() {
    if !crate::log_os::flags::INTEL_RENDER_NGIN_LOGS || crate::log_os::flags::INTEL_STAGE1_LOGS {
        return;
    }
    let (ctx_desc_lo, ctx_desc_hi) = build_guc_context_descriptor(GPU_VA_CONTEXT_BASE);
    intel_render_verbose_log!(
        "encodings mi_store_data_imm=0x{:08X} guc_hwlrca=0x{:08X}:0x{:08X} state_base_address=0x{:08X} pipe_control=0x{:08X} pc_post_sync_immediate=0x{:08X} pc_dest_ggtt=0x{:08X}\n",
        MI_STORE_DATA_IMM_GGTT_DW1,
        ctx_desc_hi,
        ctx_desc_lo,
        STATE_BASE_ADDRESS_CMD,
        PIPE_CONTROL_CMD,
        PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE,
        PIPE_CONTROL_DEST_GGTT
    );
}
fn log_triangle_probe_state(
    warm: RenderWarmState,
    shader_layout: TriangleShaderLayout,
    probe_state: TriangleProbeStateLayout,
) {
    if !crate::log_os::flags::INTEL_RENDER_NGIN_LOGS || crate::log_os::flags::INTEL_STAGE1_LOGS {
        return;
    }
    let dwords = unsafe {
        core::slice::from_raw_parts(warm.draw_state_virt as *const u32, warm.draw_state_len / 4)
    };
    let bt_ptr = if device_is_gfx125(warm.device_id) {
        probe_state.binding_table_offset_bytes
    } else {
        probe_state
            .binding_table_offset_bytes
            .saturating_sub(shader_layout.state_region_offset_bytes)
    };
    let bt_entry = dwords[probe_state.binding_table_offset_bytes as usize / 4];
    let surface = &dwords[probe_state.surface_state_offset_bytes as usize / 4
        ..probe_state.surface_state_offset_bytes as usize / 4 + 16];
    let blend = &dwords[probe_state.blend_state_offset_bytes as usize / 4
        ..probe_state.blend_state_offset_bytes as usize / 4 + 16];
    let color_calc = &dwords[probe_state.color_calc_state_offset_bytes as usize / 4
        ..probe_state.color_calc_state_offset_bytes as usize / 4 + 16];
    intel_render_verbose_log!(
        "probe-state bt_off=0x{:X} bt_entry0=0x{:08X} surf_off=0x{:X} ps_ptr=bt:0x{:X} blend_ptr=0x{:X} cc_ptr=0x{:X}\n",
        probe_state.binding_table_offset_bytes,
        bt_entry,
        probe_state.surface_state_offset_bytes,
        bt_ptr,
        probe_state.blend_state_offset_bytes | 1,
        probe_state.color_calc_state_offset_bytes | 1
    );
    intel_render_verbose_log!(
        "probe-surface d0=0x{:08X} d1=0x{:08X} d2=0x{:08X} d3=0x{:08X} d4=0x{:08X} d5=0x{:08X} d6=0x{:08X} d7=0x{:08X}\n",
        surface[0],
        surface[1],
        surface[2],
        surface[3],
        surface[4],
        surface[5],
        surface[6],
        surface[7]
    );
    intel_render_verbose_log!(
        "probe-surface d8=0x{:08X} d9=0x{:08X} d10=0x{:08X} d11=0x{:08X} d12=0x{:08X} d13=0x{:08X} d14=0x{:08X} d15=0x{:08X}\n",
        surface[8],
        surface[9],
        surface[10],
        surface[11],
        surface[12],
        surface[13],
        surface[14],
        surface[15]
    );
    intel_render_verbose_log!(
        "probe-blend d0=0x{:08X} d1=0x{:08X} d2=0x{:08X} d3=0x{:08X} d4=0x{:08X} d5=0x{:08X} d6=0x{:08X} d7=0x{:08X}\n",
        blend[0],
        blend[1],
        blend[2],
        blend[3],
        blend[4],
        blend[5],
        blend[6],
        blend[7]
    );
    intel_render_verbose_log!(
        "probe-blend d8=0x{:08X} d9=0x{:08X} d10=0x{:08X} d11=0x{:08X} d12=0x{:08X} d13=0x{:08X} d14=0x{:08X} d15=0x{:08X}\n",
        blend[8],
        blend[9],
        blend[10],
        blend[11],
        blend[12],
        blend[13],
        blend[14],
        blend[15]
    );
    intel_render_verbose_log!(
        "probe-cc d0=0x{:08X} d1=0x{:08X} d2=0x{:08X} d3=0x{:08X} d4=0x{:08X} d5=0x{:08X} d6=0x{:08X} d7=0x{:08X}\n",
        color_calc[0],
        color_calc[1],
        color_calc[2],
        color_calc[3],
        color_calc[4],
        color_calc[5],
        color_calc[6],
        color_calc[7]
    );
    intel_render_verbose_log!(
        "probe-cc d8=0x{:08X} d9=0x{:08X} d10=0x{:08X} d11=0x{:08X} d12=0x{:08X} d13=0x{:08X} d14=0x{:08X} d15=0x{:08X}\n",
        color_calc[8],
        color_calc[9],
        color_calc[10],
        color_calc[11],
        color_calc[12],
        color_calc[13],
        color_calc[14],
        color_calc[15]
    );
}

fn write_triangle_probe_state(
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
    shader_layout: TriangleShaderLayout,
    blend_mode: TriangleBlendProbeMode,
    backend_probe_mode: BackendProbeMode,
    viewport_translation_px: [f32; 2],
) -> Result<TriangleProbeStateLayout, &'static str> {
    write_triangle_probe_state_with_flush(
        warm,
        draw,
        shader_layout,
        blend_mode,
        backend_probe_mode,
        viewport_translation_px,
        true,
    )
}

fn write_triangle_probe_state_unflushed(
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
    shader_layout: TriangleShaderLayout,
    blend_mode: TriangleBlendProbeMode,
    backend_probe_mode: BackendProbeMode,
    viewport_translation_px: [f32; 2],
) -> Result<TriangleProbeStateLayout, &'static str> {
    write_triangle_probe_state_with_flush(
        warm,
        draw,
        shader_layout,
        blend_mode,
        backend_probe_mode,
        viewport_translation_px,
        false,
    )
}

fn write_triangle_probe_state_with_flush(
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
    shader_layout: TriangleShaderLayout,
    blend_mode: TriangleBlendProbeMode,
    backend_probe_mode: BackendProbeMode,
    viewport_translation_px: [f32; 2],
    flush_state: bool,
) -> Result<TriangleProbeStateLayout, &'static str> {
    const BLEND_FACTOR_ONE: u32 = 0x01;
    const BLEND_FACTOR_SRC_ALPHA: u32 = 0x03;
    const BLEND_FACTOR_INV_SRC_ALPHA: u32 = 0x13;
    const BLEND_FUNCTION_ADD: u32 = 0x00;
    if viewport_translation_px
        .iter()
        .any(|component| !component.is_finite())
    {
        return Err("probe-viewport-translation");
    }
    let native_sampled = draw.native.is_some() && draw.sampled_texture.is_some();
    let binding_table_entries = if draw.native.is_some() {
        4usize
    } else if draw.sampled_texture.is_some() {
        3usize
    } else {
        1usize
    };
    let ps_binding_table_entries = if native_sampled { 3usize } else { 0usize };
    let surface_state_count = if native_sampled {
        5usize
    } else {
        binding_table_entries
    };
    let mut cursor = shader_layout.state_region_offset_bytes as usize;
    let binding_table_offset = cursor;
    let ps_binding_table_offset = if native_sampled {
        binding_table_offset
            .checked_add(binding_table_entries * core::mem::size_of::<u32>())
            .ok_or("probe-state-overflow")?
    } else {
        binding_table_offset
    };
    cursor = crate::intel::align_up(
        ps_binding_table_offset
            .checked_add(
                ps_binding_table_entries
                    .max(if native_sampled {
                        0
                    } else {
                        binding_table_entries
                    })
                    .checked_mul(core::mem::size_of::<u32>())
                    .ok_or("probe-state-overflow")?,
            )
            .ok_or("probe-state-overflow")?,
        64,
    )
    .ok_or("probe-state-align")?;
    let surface_state_offset = cursor;
    cursor = crate::intel::align_up(
        surface_state_offset
            .checked_add(
                surface_state_count
                    .checked_mul(64)
                    .ok_or("probe-state-overflow")?,
            )
            .ok_or("probe-state-overflow")?,
        32,
    )
    .ok_or("probe-state-align")?;
    let sampler_state_offset = cursor;
    cursor = crate::intel::align_up(sampler_state_offset + 16, 64).ok_or("probe-state-align")?;
    let blend_state_offset = cursor;
    cursor = crate::intel::align_up(blend_state_offset + 64, 64).ok_or("probe-state-align")?;
    let color_calc_state_offset = cursor;
    cursor = crate::intel::align_up(color_calc_state_offset + 64, 64).ok_or("probe-state-align")?;
    let cc_viewport_offset = cursor;
    cursor = crate::intel::align_up(cc_viewport_offset + 8, 64).ok_or("probe-state-align")?;
    let sf_clip_viewport_offset = cursor;
    cursor = crate::intel::align_up(sf_clip_viewport_offset + 64, 64).ok_or("probe-state-align")?;
    let scissor_rect_offset = cursor;
    cursor = scissor_rect_offset
        .checked_add(8)
        .ok_or("probe-state-overflow")?;
    let cps_state_offset = crate::intel::align_up(cursor, 32).ok_or("probe-state-align")?;
    cursor = cps_state_offset
        .checked_add(CPS_STATE_DWORDS * core::mem::size_of::<u32>())
        .ok_or("probe-state-overflow")?;
    let slice_hash_table_offset = if device_is_gfx125(warm.device_id) {
        let offset = crate::intel::align_up(cursor, 64).ok_or("probe-state-align")?;
        cursor = offset
            .checked_add(GFX125_SLICE_HASH_TABLE_BYTES)
            .ok_or("probe-state-overflow")?;
        offset
    } else {
        0
    };
    let end_offset = cursor;
    if end_offset > warm.draw_state_len {
        return Err("probe-state-exceeds-state-bo");
    }

    let dwords = unsafe {
        core::slice::from_raw_parts_mut(warm.draw_state_virt as *mut u32, warm.draw_state_len / 4)
    };
    // gfx11 through gfx12.0 address binding tables through Surface State Base
    // Address.  Point that base directly at this dedicated state region, so a
    // zero binding-table pointer is both valid and cannot alias shader ISA.
    // gfx12.5 keeps its existing draw-BO-relative binding-table pool contract.
    let binding_table_entry_base_offset = if device_is_gfx125(warm.device_id) {
        0usize
    } else {
        shader_layout.state_region_offset_bytes as usize
    };
    for entry in 0..binding_table_entries {
        let entry_offset = surface_state_offset
            .checked_add(entry * 64)
            .and_then(|offset| offset.checked_sub(binding_table_entry_base_offset))
            .ok_or("probe-state-overflow")?;
        dwords[binding_table_offset / 4 + entry] =
            u32::try_from(entry_offset).map_err(|_| "probe-state-overflow")?;
    }
    if native_sampled {
        // VS consumes BTI0..3 as RT placeholder + camera/instances/compacted.
        // The separately compiled PS consumes RT at BTI0 and the sampled
        // image at BTI2; stage-specific binding-table pointers make those
        // layouts coexist without aliasing the VS instance surface.
        for (entry, surface_index) in [0usize, 0, 4].into_iter().enumerate() {
            let entry_offset = surface_state_offset
                .checked_add(surface_index * 64)
                .and_then(|offset| offset.checked_sub(binding_table_entry_base_offset))
                .ok_or("probe-state-overflow")?;
            dwords[ps_binding_table_offset / 4 + entry] =
                u32::try_from(entry_offset).map_err(|_| "probe-state-overflow")?;
        }
    }

    let surface = &mut dwords[surface_state_offset / 4..surface_state_offset / 4 + 16];
    surface.fill(0);
    let resident_msaa4 = draw.uses_resident_scene_msaa4();
    if resident_msaa4 && !device_is_gfx125(warm.device_id) {
        return Err("probe-msaa4-device");
    }
    let surface_halign = if resident_msaa4 {
        3
    } else {
        backend_probe_mode.surface_halign_raw(warm.device_id)
    };
    surface[0] = (SURFTYPE_2D << 29)
        | (draw.rt_surface_format << 18)
        | (surface_halign << 14)
        | (SURFACE_VALIGN_4 << 16)
        | if resident_msaa4 {
            (1 << 12) | (1 << 28)
        } else {
            0
        };
    surface[1] = (RENDER_MOCS << 24)
        // TGL/ADL PRM: EnableUnormPathInColorPipe must never be zero on
        // gfx11 through gfx12.0 render surfaces.
        | if device_is_gfx12(warm.device_id) && !device_is_gfx125(warm.device_id) {
            1 << 31
        } else {
            0
        }
        | if resident_msaa4 {
            let aligned_height = crate::intel::align_up(draw.target_h as usize, 64)
                .ok_or("probe-msaa4-shape")?;
            u32::try_from(aligned_height / 4).map_err(|_| "probe-msaa4-shape")?
        } else {
            0
        };
    surface[2] = draw.target_w.saturating_sub(1) | (draw.target_h.saturating_sub(1) << 16);
    surface[3] = draw.rt_pitch.saturating_sub(1);
    surface[4] = if resident_msaa4 { 2 << 3 } else { 0 };
    surface[7] = (SHADER_CHANNEL_ALPHA << 16)
        | (SHADER_CHANNEL_BLUE << 19)
        | (SHADER_CHANNEL_GREEN << 22)
        | (SHADER_CHANNEL_RED << 25);
    surface[8] = draw.rt_gpu_addr as u32;
    surface[9] = (draw.rt_gpu_addr >> 32) as u32;
    intel_render_verbose_log!(
        "probe-surface-rt backend={} surf0=0x{:08X} format={} halign_raw={} valign_raw={} size={}x{} pitch=0x{:X} rt_gpu=0x{:X} samples={} tiling={} note=render-target-descriptor\n",
        backend_probe_mode.label(),
        surface[0],
        (surface[0] >> 18) & 0x1FF,
        (surface[0] >> 14) & 0x3,
        (surface[0] >> 16) & 0x3,
        draw.target_w,
        draw.target_h,
        draw.rt_pitch,
        draw.rt_gpu_addr,
        if resident_msaa4 { 4 } else { 1 },
        if resident_msaa4 { "tile64" } else { "linear" },
    );

    if let Some(native) = draw.native {
        for (binding_index, binding) in native.vs_storage_bindings.into_iter().enumerate() {
            let start = surface_state_offset / 4 + (binding_index + 1) * 16;
            write_triangle_raw_buffer_surface_state(&mut dwords[start..start + 16], binding)?;
        }
        if !CHURN_NATIVE_SURFACE_STATE_LOGGED.swap(true, Ordering::AcqRel) {
            let binding_table =
                &dwords[binding_table_offset / 4..binding_table_offset / 4 + binding_table_entries];
            let raw0 = &dwords[surface_state_offset / 4 + 16..surface_state_offset / 4 + 32];
            let raw1 = &dwords[surface_state_offset / 4 + 32..surface_state_offset / 4 + 48];
            let raw2 = &dwords[surface_state_offset / 4 + 48..surface_state_offset / 4 + 64];
            crate::log_info!(
                target: "gpgpu";
                "helio-churn: native-state gpu=0x{:X} phys=0x{:X} state_region=0x{:X} binding_table=0x{:X} surface_state=0x{:X} bt=[0x{:X},0x{:X},0x{:X},0x{:X}] bindings=[0x{:X}/0x{:X},0x{:X}/0x{:X},0x{:X}/0x{:X}] raw1=[{:08X},{:08X},{:08X},{:08X},{:08X},{:08X},{:08X},{:08X}] raw2=[{:08X},{:08X},{:08X},{:08X},{:08X},{:08X},{:08X},{:08X}] raw3=[{:08X},{:08X},{:08X},{:08X},{:08X},{:08X},{:08X},{:08X}] address_order=gpu/bytes raw_order=dw0,dw1,dw2,dw3,dw7,dw8,dw9,dw11\n",
                draw.state_gpu_addr,
                warm.draw_state_phys,
                shader_layout.state_region_offset_bytes,
                binding_table_offset,
                surface_state_offset,
                binding_table[0],
                binding_table[1],
                binding_table[2],
                binding_table[3],
                native.vs_storage_bindings[0].gpu_addr,
                native.vs_storage_bindings[0].byte_len,
                native.vs_storage_bindings[1].gpu_addr,
                native.vs_storage_bindings[1].byte_len,
                native.vs_storage_bindings[2].gpu_addr,
                native.vs_storage_bindings[2].byte_len,
                raw0[0], raw0[1], raw0[2], raw0[3], raw0[7], raw0[8], raw0[9], raw0[11],
                raw1[0], raw1[1], raw1[2], raw1[3], raw1[7], raw1[8], raw1[9], raw1[11],
                raw2[0], raw2[1], raw2[2], raw2[3], raw2[7], raw2[8], raw2[9], raw2[11],
            );
        }
        if let Some(texture) = draw.sampled_texture {
            let start = surface_state_offset / 4 + 4 * 16;
            write_triangle_sampled_rgba8_surface_state(&mut dwords[start..start + 16], texture)?;
        }
    } else if let Some(texture) = draw.sampled_texture {
        let start = surface_state_offset / 4 + 2 * 16;
        write_triangle_sampled_rgba8_surface_state(&mut dwords[start..start + 16], texture)?;
    }

    let sampler = &mut dwords[sampler_state_offset / 4..sampler_state_offset / 4 + 4];
    sampler.fill(0);
    if let Some(texture) = draw.sampled_texture
        && texture.sampler_flags
            != (crate::gpu::vgpu::SAMPLER_ADDRESS_U_REPEAT
                | crate::gpu::vgpu::SAMPLER_ADDRESS_V_REPEAT)
    {
        return Err("probe-sampler-mode");
    }

    let blend = &mut dwords[blend_state_offset / 4..blend_state_offset / 4 + 16];
    blend.fill(0);
    match blend_mode {
        // Keep the existing explicit RT0 setup as the baseline attempt.
        TriangleBlendProbeMode::ExplicitRt0 => {
            blend[0] = 0;
            blend[1] = (1 << 0) | (1 << 1) | (2 << 2);
        }
        TriangleBlendProbeMode::StraightAlpha => {
            // Gen12 blend state: straight-alpha color over, with separate
            // alpha accumulation (src=ONE, dst=INV_SRC_ALPHA).  The common
            // dword enables independent alpha; entry dword 0 carries the
            // blend factors/function and entry dword 1 keeps RGBA writable.
            blend[0] = 1 << 30;
            blend[1] = (1 << 31)
                | (BLEND_FACTOR_SRC_ALPHA << 26)
                | (BLEND_FACTOR_INV_SRC_ALPHA << 21)
                | (BLEND_FUNCTION_ADD << 18)
                | (BLEND_FACTOR_ONE << 13)
                | (BLEND_FACTOR_INV_SRC_ALPHA << 8)
                | (BLEND_FUNCTION_ADD << 5);
            blend[2] = 0;
        }
        // Mesa's trivial path mainly relies on PS_BLEND HasWriteableRT with a
        // boring zeroed blend-state payload.
        TriangleBlendProbeMode::MesaZeroedState
        | TriangleBlendProbeMode::MesaZeroedNoBlendPointer => {}
    }

    let color_calc = &mut dwords[color_calc_state_offset / 4..color_calc_state_offset / 4 + 16];
    color_calc.fill(0);

    let cc_viewport = &mut dwords[cc_viewport_offset / 4..cc_viewport_offset / 4 + 2];
    cc_viewport[0] = 0.0f32.to_bits();
    cc_viewport[1] = 1.0f32.to_bits();

    let sf_clip_viewport =
        &mut dwords[sf_clip_viewport_offset / 4..sf_clip_viewport_offset / 4 + 16];
    sf_clip_viewport.fill(0);
    sf_clip_viewport[0] = (draw.target_w as f32 * 0.5).to_bits();
    sf_clip_viewport[1] = (-(draw.target_h as f32) * 0.5).to_bits();
    sf_clip_viewport[2] = 1.0f32.to_bits();
    sf_clip_viewport[3] = (draw.target_w as f32 * 0.5 + viewport_translation_px[0]).to_bits();
    sf_clip_viewport[4] = (draw.target_h as f32 * 0.5 + viewport_translation_px[1]).to_bits();
    sf_clip_viewport[5] = 0.0f32.to_bits();
    sf_clip_viewport[8] = (-32768.0f32).to_bits();
    sf_clip_viewport[9] = 32768.0f32.to_bits();
    sf_clip_viewport[10] = (-32768.0f32).to_bits();
    sf_clip_viewport[11] = 32768.0f32.to_bits();
    sf_clip_viewport[12] = 0.0f32.to_bits();
    sf_clip_viewport[13] = (draw.target_w as f32).to_bits();
    sf_clip_viewport[14] = 0.0f32.to_bits();
    sf_clip_viewport[15] = (draw.target_h as f32).to_bits();
    intel_render_focus_log!(
        "sf-clip-viewport-extents target={}x{} xmin=0.000 xmax={:.3} ymin=0.000 ymax={:.3} translate_px={:.3},{:.3} prm=viewport-transform-final-clip-rectangle\n",
        draw.target_w,
        draw.target_h,
        draw.target_w as f32,
        draw.target_h as f32,
        viewport_translation_px[0],
        viewport_translation_px[1],
    );

    let scissor_rect = &mut dwords[scissor_rect_offset / 4..scissor_rect_offset / 4 + 2];
    scissor_rect[0] = 0;
    scissor_rect[1] = draw.target_w.saturating_sub(1) | (draw.target_h.saturating_sub(1) << 16);

    let cps_state = &mut dwords[cps_state_offset / 4..cps_state_offset / 4 + CPS_STATE_DWORDS];
    cps_state.fill(0);

    if slice_hash_table_offset != 0 {
        let slice_hash = &mut dwords[slice_hash_table_offset / 4
            ..slice_hash_table_offset / 4 + GFX125_SLICE_HASH_TABLE_DWORDS];
        let mut packed = [0u32; GFX125_SLICE_HASH_TABLE_DWORDS];
        gfx125_pack_slice_hash_tables(gfx125_slice_hash_config(warm), &mut packed);
        slice_hash.copy_from_slice(&packed);
    }

    if flush_state {
        let flush_ptr = unsafe {
            warm.draw_state_virt
                .add(shader_layout.state_region_offset_bytes as usize)
        };
        crate::intel::dma_flush(
            flush_ptr,
            end_offset - shader_layout.state_region_offset_bytes as usize,
        );
    }

    Ok(TriangleProbeStateLayout {
        binding_table_offset_bytes: binding_table_offset as u32,
        ps_binding_table_offset_bytes: ps_binding_table_offset as u32,
        surface_state_offset_bytes: surface_state_offset as u32,
        sampler_state_offset_bytes: sampler_state_offset as u32,
        blend_state_offset_bytes: blend_state_offset as u32,
        color_calc_state_offset_bytes: color_calc_state_offset as u32,
        cc_viewport_offset_bytes: cc_viewport_offset as u32,
        sf_clip_viewport_offset_bytes: sf_clip_viewport_offset as u32,
        scissor_rect_offset_bytes: scissor_rect_offset as u32,
        push_constant_offset_bytes: 0,
        cps_state_offset_bytes: cps_state_offset as u32,
        slice_hash_table_offset_bytes: slice_hash_table_offset as u32,
        used_bytes: end_offset as u32,
    })
}

/// Encode the Gen12 RAW buffer surface used by Churn's read-only storage
/// bindings. The extent is the exact bound byte range, not the page-rounded
/// allocation size, so an out-of-contract shader index still faults at the
/// narrowest surface boundary the hardware supports.
fn write_triangle_raw_buffer_surface_state(
    surface: &mut [u32],
    binding: TriangleStorageBufferBinding,
) -> Result<(), &'static str> {
    if surface.len() != 16 || binding.byte_len == 0 {
        return Err("probe-native-buffer-surface");
    }
    surface.fill(0);
    let extent = u64::from(binding.byte_len - 1);
    let width_minus_1 = (extent & 0x7f) as u32;
    let height_minus_1 = ((extent >> 7) & 0x3fff) as u32;
    let depth_minus_1 = ((extent >> 21) & 0x7ff) as u32;
    surface[0] = (SURFTYPE_BUFFER << 29)
        | (SURFACE_FORMAT_RAW << 18)
        | (SURFACE_HALIGN_4 << 14)
        | (SURFACE_VALIGN_4 << 16);
    surface[1] = RENDER_MOCS << 24;
    surface[2] = (height_minus_1 << 16) | width_minus_1;
    surface[3] = depth_minus_1 << 21;
    // Mesa/ISL composes RAW's missing color channels with the identity view
    // swizzle: RGB select zero while alpha selects one.  The alpha field is
    // still part of the exact RENDER_SURFACE_STATE contract even though the
    // shader reaches this buffer through untyped HDC reads.
    surface[7] = SHADER_CHANNEL_ONE << 16;
    surface[8] = binding.gpu_addr as u32;
    surface[9] = (binding.gpu_addr >> 32) as u32;
    // gfx12 ISL carries the exact byte range in the high auxiliary-address
    // DWORD when buffer_length_in_aux_addr is enabled.
    surface[11] = binding.byte_len;
    Ok(())
}

fn write_triangle_sampled_rgba8_surface_state(
    surface: &mut [u32],
    texture: TriangleSampledTextureBinding,
) -> Result<(), &'static str> {
    if surface.len() != 16
        || texture.width == 0
        || texture.height == 0
        || texture.pitch < texture.width.saturating_mul(4)
    {
        return Err("probe-sampled-texture-surface");
    }
    surface.fill(0);
    surface[0] = (SURFTYPE_2D << 29)
        | (SURFACE_FORMAT_R8G8B8A8_UNORM << 18)
        | (SURFACE_HALIGN_4 << 14)
        | (SURFACE_VALIGN_4 << 16);
    surface[1] = (RENDER_MOCS << 24)
        // TGL/ADL requires this for UNORM surfaces, including sampler reads.
        // Omitting it is not a legal way to describe a linear RGBA8 texture.
        | (1 << 31);
    surface[2] = texture.width.saturating_sub(1) | (texture.height.saturating_sub(1) << 16);
    surface[3] = texture.pitch.saturating_sub(1);
    surface[7] = (SHADER_CHANNEL_ALPHA << 16)
        | (SHADER_CHANNEL_BLUE << 19)
        | (SHADER_CHANNEL_GREEN << 22)
        | (SHADER_CHANNEL_RED << 25);
    surface[8] = texture.gpu_addr as u32;
    surface[9] = (texture.gpu_addr >> 32) as u32;
    Ok(())
}

#[cfg(test)]
mod churn_raw_surface_tests {
    use super::{
        RENDER_MOCS, SURFACE_FORMAT_RAW, SURFTYPE_BUFFER, TriangleStorageBufferBinding,
        write_triangle_raw_buffer_surface_state,
    };

    #[test]
    fn encodes_exact_raw_extent_and_address() {
        let mut surface = [0xFFFF_FFFF; 16];
        let binding = TriangleStorageBufferBinding {
            gpu_addr: 0x0000_1234_5678_9000,
            byte_len: 368,
        };
        write_triangle_raw_buffer_surface_state(&mut surface, binding).unwrap();
        let extent = 367u32;
        assert_eq!(
            surface[0],
            (SURFTYPE_BUFFER << 29)
                | (SURFACE_FORMAT_RAW << 18)
                | (super::SURFACE_HALIGN_4 << 14)
                | (super::SURFACE_VALIGN_4 << 16)
        );
        assert_eq!(surface[1], RENDER_MOCS << 24);
        assert_eq!(surface[2], (((extent >> 7) & 0x3fff) << 16) | (extent & 0x7f));
        assert_eq!(surface[3], ((extent >> 21) & 0x7ff) << 21);
        assert_eq!(surface[8], 0x5678_9000);
        assert_eq!(surface[9], 0x0000_1234);
        assert_eq!(surface[7], super::SHADER_CHANNEL_ONE << 16);
        assert_eq!(surface[10], 0);
        assert_eq!(surface[11], binding.byte_len);
        assert!(
            surface[4..7]
                .iter()
                .chain(&surface[12..])
                .all(|&word| word == 0)
        );
    }

    #[test]
    fn rejects_zero_sized_or_wrong_sized_surface() {
        let mut surface = [0u32; 16];
        assert_eq!(
            write_triangle_raw_buffer_surface_state(
                &mut surface,
                TriangleStorageBufferBinding {
                    gpu_addr: 0x1000,
                    byte_len: 0,
                },
            ),
            Err("probe-native-buffer-surface")
        );
        assert_eq!(
            write_triangle_raw_buffer_surface_state(
                &mut surface[..15],
                TriangleStorageBufferBinding {
                    gpu_addr: 0x1000,
                    byte_len: 4,
                },
            ),
            Err("probe-native-buffer-surface")
        );
    }
}

#[cfg(test)]
mod sampled_rgba8_surface_tests {
    use super::{
        RENDER_MOCS, TriangleSampledTextureBinding, write_triangle_sampled_rgba8_surface_state,
    };

    #[test]
    fn enables_the_adls_unorm_sampler_path() {
        let mut surface = [0u32; 16];
        write_triangle_sampled_rgba8_surface_state(
            &mut surface,
            TriangleSampledTextureBinding {
                gpu_addr: 0x1234_5000,
                width: 16,
                height: 16,
                pitch: 64,
                sampler_flags: 0,
            },
        )
        .unwrap();
        assert_eq!(surface[1], (RENDER_MOCS << 24) | (1 << 31));
        assert_eq!(surface[8], 0x1234_5000);
        assert_eq!(surface[9], 0);
    }
}

/// Lower Helio's exact five-DWORD DrawIndexedIndirectArgs ABI to the Intel
/// auto-draw registers. Keeping this as a small standalone encoder makes the
/// ABI testable without a device and leaves the record GPU-writable for the
/// next culling step.
fn encode_draw_indexed_indirect_register_loads(
    batch_dwords: &mut [u32],
    cursor: &mut usize,
    args_gpu_addr: u64,
) -> Result<(), &'static str> {
    if args_gpu_addr & 3 != 0 {
        return Err("probe-indirect-address");
    }
    let fields = [
        (RCS_3DPRIM_VERTEX_COUNT, 0u64),
        (RCS_3DPRIM_INSTANCE_COUNT, 4),
        (RCS_3DPRIM_START_VERTEX, 8),
        (RCS_3DPRIM_BASE_VERTEX, 12),
        (RCS_3DPRIM_START_INSTANCE, 16),
        // Gen11+ routes gl_BaseVertex through XP0 when extended parameters
        // are present. The current clip-space VS does not consume it. This
        // prepares XP0; future Helio shader metadata must also enable its
        // 3DSTATE_VF_SGVS_2 slot before the builtin is delivered.
        (RCS_3DPRIM_XP_BASE_VERTEX, 12),
    ];
    let required = fields
        .len()
        .checked_mul(4)
        .and_then(|dwords| dwords.checked_add(3))
        .ok_or("probe-indirect-capacity")?;
    if cursor.saturating_add(required) > batch_dwords.len() {
        return Err("probe-batch-exhausted");
    }
    for (register, byte_offset) in fields {
        let address = args_gpu_addr
            .checked_add(byte_offset)
            .ok_or("probe-indirect-address")?;
        // Gfx11+ addresses engine-relative CS MMIO registers through the
        // AddCSMMIOStartOffset bit. Mesa's mi_adjust_reg_num() applies the
        // same 0x2000 subtraction before emitting MI_LOAD_REGISTER_MEM.
        batch_dwords[*cursor] = MI_LOAD_REGISTER_MEM | MI_LRI_CS_MMIO;
        batch_dwords[*cursor + 1] = register - RCS_RING_BASE as u32;
        batch_dwords[*cursor + 2] = address as u32;
        batch_dwords[*cursor + 3] = (address >> 32) as u32;
        *cursor += 4;
    }
    // Gen11+ indirect draws source all three extended parameters from the
    // 3DPRIM_XP registers when ExtendedParametersPresent is set.  XP1 is
    // implicit StartInstanceLocation, while XP0 and XP2 must be initialized
    // explicitly.  Churn is one logical draw per packet, so its DrawID is 0.
    // Match ANV's exact CS-MMIO LRI form (no ForcePosted bit).
    batch_dwords[*cursor] = mi_lri_cmd(1, 0);
    batch_dwords[*cursor + 1] = RCS_3DPRIM_XP_DRAW_ID - RCS_RING_BASE as u32;
    batch_dwords[*cursor + 2] = 0;
    *cursor += 3;
    Ok(())
}

#[cfg(test)]
mod draw_indexed_indirect_encoder_tests {
    use super::{
        MI_LOAD_REGISTER_MEM, MI_LRI_CS_MMIO, RCS_3DPRIM_BASE_VERTEX, RCS_3DPRIM_INSTANCE_COUNT,
        RCS_3DPRIM_START_INSTANCE, RCS_3DPRIM_START_VERTEX, RCS_3DPRIM_VERTEX_COUNT,
        RCS_3DPRIM_XP_BASE_VERTEX, RCS_3DPRIM_XP_DRAW_ID, RCS_RING_BASE,
        encode_draw_indexed_indirect_register_loads, mi_lri_cmd,
    };

    #[test]
    fn lowers_the_exact_twenty_byte_helio_record_to_rcs_registers() {
        let base = 0x0000_1234_5678_9000u64;
        let mut batch = [0u32; 27];
        let mut cursor = 0usize;
        encode_draw_indexed_indirect_register_loads(&mut batch, &mut cursor, base).unwrap();
        assert_eq!(cursor, batch.len());

        let expected = [
            (RCS_3DPRIM_VERTEX_COUNT, 0u64),
            (RCS_3DPRIM_INSTANCE_COUNT, 4),
            (RCS_3DPRIM_START_VERTEX, 8),
            (RCS_3DPRIM_BASE_VERTEX, 12),
            (RCS_3DPRIM_START_INSTANCE, 16),
            (RCS_3DPRIM_XP_BASE_VERTEX, 12),
        ];
        for (packet, (register, byte_offset)) in batch[..24].chunks_exact(4).zip(expected) {
            let address = base + byte_offset;
            assert_eq!(packet[0], MI_LOAD_REGISTER_MEM | MI_LRI_CS_MMIO);
            assert_eq!(packet[1], register - RCS_RING_BASE as u32);
            assert_eq!(packet[2], address as u32);
            assert_eq!(packet[3], (address >> 32) as u32);
        }
        assert_eq!(
            &batch[24..],
            &[
                mi_lri_cmd(1, 0),
                RCS_3DPRIM_XP_DRAW_ID - RCS_RING_BASE as u32,
                0,
            ]
        );
    }

    #[test]
    fn rejects_unaligned_or_truncated_indirect_packets() {
        let mut batch = [0u32; 27];
        let mut cursor = 0usize;
        assert_eq!(
            encode_draw_indexed_indirect_register_loads(&mut batch, &mut cursor, 0x1002),
            Err("probe-indirect-address")
        );
        assert_eq!(cursor, 0);

        let mut short = [0u32; 26];
        assert_eq!(
            encode_draw_indexed_indirect_register_loads(&mut short, &mut cursor, 0x1000),
            Err("probe-batch-exhausted")
        );
        assert_eq!(cursor, 0);
    }
}

fn validate_triangle_native_draw_contract(
    draw: TriangleDrawPrep,
    native: TriangleNativeDrawContract,
) -> Result<(), &'static str> {
    let [camera, instances, compacted] = native.vs_storage_bindings;
    let instance_count =
        instances.byte_len / trueos_helio_runtime::churn::GpuInstanceData::BYTE_LEN as u32;
    let compacted_count = compacted.byte_len / core::mem::size_of::<u32>() as u32;
    let expected_instancing = [
        TriangleVfInstancingState {
            element_index: 0,
            enabled: false,
            step_rate: 0,
        },
        TriangleVfInstancingState {
            element_index: 1,
            enabled: false,
            step_rate: 0,
        },
        TriangleVfInstancingState {
            element_index: 2,
            enabled: false,
            step_rate: 0,
        },
        TriangleVfInstancingState {
            element_index: 3,
            enabled: false,
            step_rate: 0,
        },
    ];
    let pos_normal = draw.vertex_format == TriangleVertexFormat::PosNormal
        && draw.vertex_stride == trueos_helio_artifact::churn_forward::VERTEX_STRIDE
        && draw.sampled_texture.is_none()
        && native.vertex_element_count == 3
        && native.vf_sgvs_dw1 == 0xE002_4002
        && native.vf_sgvs_2_dw1 == 0xB002_0002
        && native.vf_component_packing == [0x0000_0A77, 0, 0, 0];
    let pos_normal_uv = draw.vertex_format == TriangleVertexFormat::PosNormalUv
        && draw.vertex_stride == 32
        && draw.sampled_texture.is_some()
        && native.vertex_element_count == 4
        && native.vf_sgvs_dw1 == 0xE003_4002
        && native.vf_sgvs_2_dw1 == 0xB003_0002
        && native.vf_component_packing == [0x0000_A377, 0, 0, 0];
    if !(pos_normal || pos_normal_uv)
        || draw.index_buffer.is_none()
        || draw.indirect_args_gpu_addr.is_none()
        || camera.gpu_addr == 0
        || instances.gpu_addr == 0
        || compacted.gpu_addr == 0
        || camera.byte_len != trueos_helio_runtime::churn::GpuCameraUniforms::BYTE_LEN as u32
        || instances.byte_len == 0
        || instances.byte_len % trueos_helio_runtime::churn::GpuInstanceData::BYTE_LEN as u32 != 0
        || compacted.byte_len == 0
        || compacted.byte_len % core::mem::size_of::<u32>() as u32 != 0
        || instance_count != compacted_count
        || native.vf_sgvs_2_dw2 != 3
        || native.vf_instancing != expected_instancing
    {
        return Err("probe-native-vf-contract");
    }
    Ok(())
}

#[cfg(test)]
mod retained_native_matrix_draw_contract_tests {
    use super::{
        ChurnHardwareAdmission, TriangleDrawPrep, TriangleIndexBufferPrep,
        TriangleNativeDrawContract, TriangleSampledTextureBinding, TriangleStorageBufferBinding,
        TriangleVertexFormat, TriangleVfInstancingState, validate_triangle_native_draw_contract,
    };

    const CAMERA_GPU: u64 = 0x2000_0000;
    const INSTANCES_GPU: u64 = 0x2100_0000;
    const COMPACTED_GPU: u64 = 0x2200_0000;
    const INDIRECT_GPU: u64 = 0x2300_0028;
    const ROWS: u32 = 337;

    fn native_contract() -> TriangleNativeDrawContract {
        TriangleNativeDrawContract {
            hardware_admission: ChurnHardwareAdmission::ValidatedProduction,
            vs_storage_bindings: [
                TriangleStorageBufferBinding {
                    gpu_addr: CAMERA_GPU,
                    byte_len: trueos_helio_runtime::churn::GpuCameraUniforms::BYTE_LEN as u32,
                },
                TriangleStorageBufferBinding {
                    gpu_addr: INSTANCES_GPU,
                    byte_len: ROWS * trueos_helio_runtime::churn::GpuInstanceData::BYTE_LEN as u32,
                },
                TriangleStorageBufferBinding {
                    gpu_addr: COMPACTED_GPU,
                    byte_len: ROWS * core::mem::size_of::<u32>() as u32,
                },
            ],
            vf_sgvs_dw1: 0xE002_4002,
            vf_sgvs_2_dw1: 0xB002_0002,
            vf_sgvs_2_dw2: 3,
            vertex_element_count: 3,
            vf_component_packing: [0x0000_0A77, 0, 0, 0],
            vf_instancing: core::array::from_fn(|element_index| TriangleVfInstancingState {
                element_index: element_index as u8,
                enabled: false,
                step_rate: 0,
            }),
        }
    }

    fn native_draw(native: TriangleNativeDrawContract) -> TriangleDrawPrep {
        TriangleDrawPrep {
            vertex_count: 108,
            vertex_stride: trueos_helio_artifact::churn_forward::VERTEX_STRIDE,
            vertex_buffer_bytes: 108 * trueos_helio_artifact::churn_forward::VERTEX_STRIDE,
            vertex_format: TriangleVertexFormat::PosNormal,
            vertex_gpu_addr: 0x2400_0000,
            index_buffer: Some(TriangleIndexBufferPrep {
                index_count: 108,
                byte_len: 108 * core::mem::size_of::<u32>() as u32,
                gpu_addr: 0x2500_0000,
            }),
            indirect_args_gpu_addr: Some(INDIRECT_GPU),
            native: Some(native),
            sampled_texture: None,
            state_gpu_addr: 0x2600_0000,
            rt_gpu_addr: 0x2700_0000,
            rt_surface_format: 0,
            rt_pitch: 4096,
            target_w: 1024,
            target_h: 768,
        }
    }

    #[test]
    fn gpu_authored_rows_feed_the_native_indexed_indirect_contract() {
        let native = native_contract();
        let draw = native_draw(native);
        assert_eq!(trueos_helio_runtime::churn::GpuInstanceData::BYTE_LEN, 208);
        assert_eq!(trueos_helio_runtime::DrawIndexedIndirectArgs::BYTE_LEN, 20);
        assert_eq!(draw.indirect_args_gpu_addr, Some(INDIRECT_GPU));
        assert_eq!(draw.native.unwrap().vs_storage_bindings, native.vs_storage_bindings);
        assert_eq!(
            native.vs_storage_bindings.map(|binding| binding.gpu_addr),
            [CAMERA_GPU, INSTANCES_GPU, COMPACTED_GPU]
        );
        assert_eq!(validate_triangle_native_draw_contract(draw, native), Ok(()));
    }

    #[test]
    fn matrix_handoff_rejects_misaligned_instance_and_compaction_capacities() {
        let mut native = native_contract();
        native.vs_storage_bindings[1].byte_len -= 1;
        assert_eq!(
            validate_triangle_native_draw_contract(native_draw(native), native),
            Err("probe-native-vf-contract")
        );

        let mut native = native_contract();
        native.vs_storage_bindings[2].byte_len -= core::mem::size_of::<u32>() as u32;
        assert_eq!(
            validate_triangle_native_draw_contract(native_draw(native), native),
            Err("probe-native-vf-contract")
        );
    }

    #[test]
    fn textured_matrix_handoff_requires_uv_vertices_and_a_sampled_surface() {
        let mut native = native_contract();
        native.vf_sgvs_dw1 = 0xE003_4002;
        native.vf_sgvs_2_dw1 = 0xB003_0002;
        native.vertex_element_count = 4;
        native.vf_component_packing = [0x0000_A377, 0, 0, 0];

        let mut draw = native_draw(native);
        draw.vertex_stride = 32;
        draw.vertex_buffer_bytes = draw.vertex_count * 32;
        draw.vertex_format = TriangleVertexFormat::PosNormalUv;
        draw.sampled_texture = Some(TriangleSampledTextureBinding {
            gpu_addr: 0x2800_0000,
            width: 1024,
            height: 1024,
            pitch: 4096,
            sampler_flags: 0,
        });
        assert_eq!(validate_triangle_native_draw_contract(draw, native), Ok(()));

        draw.sampled_texture = None;
        assert_eq!(
            validate_triangle_native_draw_contract(draw, native),
            Err("probe-native-vf-contract")
        );
    }
}

fn encode_triangle_probe_batch(
    submit_name: &'static str,
    batch_dwords: &mut [u32],
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
    blend_mode: TriangleBlendProbeMode,
    depth_config: Option<TriangleDepthConfig>,
    pipeline: &crate::intel::shader::TrianglePipeline,
    shader_layout: TriangleShaderLayout,
    probe_state: TriangleProbeStateLayout,
    result_gpu_addr: u64,
    pre3d_value: u32,
    post3d_value: u32,
    done_value: u32,
    batch_mode: TriangleBatchMode,
    streamout_experiment: StreamoutProofExperiment,
    front_end_contract: TriangleFrontEndContract,
    viewport_translation_px: [f32; 2],
    backend_probe_mode: BackendProbeMode,
    post_draw_sync_variant: PostDrawSyncVariant,
) -> Result<usize, &'static str> {
    let mut cursor = 0usize;
    if let Some(native) = draw.native {
        validate_triangle_native_draw_contract(draw, native)?;
        if !device_admits_churn_forward_native(
            native.hardware_admission,
            warm.device_id,
            warm.revision_id,
        ) {
            return Err("probe-native-device-mismatch");
        }
    }
    let resident_msaa4 = draw.uses_resident_scene_msaa4();
    if resident_msaa4 && !device_is_gfx125(warm.device_id) {
        return Err("probe-msaa4-device");
    }
    let multisample_dw1 = if resident_msaa4 {
        2 << 1
    } else {
        backend_probe_mode.multisample_dw1()
    };
    let sample_mask_dw = if resident_msaa4 {
        0xF
    } else {
        backend_probe_mode.sample_mask_dw()
    };
    if let Some(depth) = depth_config
        && (depth.gpu_addr & 0xFFF != 0
            || depth.pitch_bytes == 0
            || depth.pitch_bytes > (1 << 18)
            || !depth.pitch_bytes.is_multiple_of(128)
            || depth.width == 0
            || depth.width > (1 << 14)
            || depth.height == 0
            || depth.height > (1 << 14)
            || depth.qpitch_rows_div4 > 0x7FFF
            || depth.compare_function > 7)
    {
        return Err("probe-depth-shape");
    }
    let vf_synthesized_vue = batch_mode.vf_synthesized_vue();
    let force_vs_with_vf_synthesized_vue =
        vf_synthesized_vue && front_end_contract.force_vs_with_vf_synthesized_vue;
    if draw.indirect_args_gpu_addr.is_some() && draw.index_buffer.is_none() {
        return Err("probe-indirect-requires-index-buffer");
    }
    if draw.indirect_args_gpu_addr.is_some() && !device_is_gfx12(warm.device_id) {
        // The CS-MMIO-relative MLRM encoding and XP0 register used below are
        // the gfx11+ contract. TRUEOS's production targets are gfx12; reject
        // older devices instead of pairing those loads with the legacy draw
        // packet and pretending it is portable.
        return Err("probe-indirect-device");
    }

    fn log_batch_offset(cursor: usize, label: &str) {
        intel_render_batch_log!(
            "batch-off 0x{:03X} {}\n",
            cursor * core::mem::size_of::<u32>(),
            label
        );
    }

    fn push(batch_dwords: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), &'static str> {
        if *cursor >= batch_dwords.len() {
            return Err("probe-batch-exhausted");
        }
        batch_dwords[*cursor] = value;
        *cursor += 1;
        Ok(())
    }

    fn push_addr(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        value: u64,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, value as u32)?;
        push(batch_dwords, cursor, (value >> 32) as u32)
    }

    fn sampler_count_encoding(count: u8) -> u32 {
        match count {
            0 => 0,
            1..=4 => 1,
            5..=8 => 2,
            9..=12 => 3,
            _ => 4,
        }
    }

    fn binding_table_entry_count_encoding(count: u8) -> u32 {
        count as u32
    }

    fn push_pipe_control(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags: u32,
    ) -> Result<(), &'static str> {
        push_pipe_control_full(batch_dwords, cursor, 0, flags)
    }

    fn push_pipe_control_full(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags_dw0: u32,
        flags_dw1: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD)?;
        push(batch_dwords, cursor, flags_dw1)?;
        if let Some(slot) = batch_dwords.get_mut(cursor.saturating_sub(2)) {
            *slot |= flags_dw0;
        } else {
            return Err("probe-pipe-control-header");
        }
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)
    }

    fn push_pipe_control_post_sync_imm(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags_dw0: u32,
        flags_dw1: u32,
        address: u64,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD)?;
        push(batch_dwords, cursor, flags_dw1)?;
        if let Some(slot) = batch_dwords.get_mut(cursor.saturating_sub(2)) {
            *slot |= flags_dw0;
        } else {
            return Err("probe-pipe-control-header");
        }
        push(batch_dwords, cursor, address as u32)?;
        push(batch_dwords, cursor, (address >> 32) as u32)?;
        push(batch_dwords, cursor, value)?;
        push(batch_dwords, cursor, 0)
    }

    fn push_store_data_imm(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        address: u64,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, MI_STORE_DATA_IMM_GGTT_DW1)?;
        push_addr(batch_dwords, cursor, address)?;
        push(batch_dwords, cursor, value)
    }

    fn push_load_register_imm(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        reg: usize,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, mi_lri_cmd(1, MI_LRI_FORCE_POSTED))?;
        push(batch_dwords, cursor, reg as u32)?;
        push(batch_dwords, cursor, value)
    }

    fn push_mi_report_perf_count(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        address: u64,
        report_id: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, MI_REPORT_PERF_COUNT_CMD)?;
        push(batch_dwords, cursor, (address as u32) | MI_REPORT_PERF_COUNT_USE_GLOBAL_GTT)?;
        push(batch_dwords, cursor, (address >> 32) as u32)?;
        push(batch_dwords, cursor, report_id)
    }

    fn push_wm_hz_op(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        device_id: u16,
        dw1: u32,
        dw2: u32,
        dw3: u32,
        dw4: u32,
    ) -> Result<(), &'static str> {
        if device_is_gfx125(device_id) {
            push(batch_dwords, cursor, CMD_3DSTATE_WM_HZ_OP_GFX125)?;
            push(batch_dwords, cursor, dw1)?;
            push(batch_dwords, cursor, dw2)?;
            push(batch_dwords, cursor, dw3)?;
            push(batch_dwords, cursor, dw4)?;
            push(batch_dwords, cursor, 0)
        } else {
            push(batch_dwords, cursor, CMD_3DSTATE_WM_HZ_OP_GEN12)?;
            push(batch_dwords, cursor, dw1)?;
            push(batch_dwords, cursor, dw2)?;
            push(batch_dwords, cursor, dw3)?;
            push(batch_dwords, cursor, dw4)
        }
    }

    fn push_vertex_element_state(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        vertex_buffer_index: u32,
        source_offset: u32,
        source_format: u32,
        component0: u32,
        component1: u32,
        component2: u32,
        component3: u32,
    ) -> Result<(), &'static str> {
        push(
            batch_dwords,
            cursor,
            (source_offset & 0xFFF)
                | (source_format << 16)
                | (1 << 25)
                | (vertex_buffer_index << 26),
        )?;
        push(
            batch_dwords,
            cursor,
            (component0 << 28) | (component1 << 24) | (component2 << 20) | (component3 << 16),
        )
    }

    fn cmd_3dstate_vertex_elements(count: usize) -> Result<u32, &'static str> {
        let body_dwords = count
            .checked_mul(2)
            .and_then(|n| n.checked_sub(1))
            .ok_or("ve-count-overflow")?;
        let body_dwords = u32::try_from(body_dwords).map_err(|_| "ve-count-convert")?;
        Ok(body_dwords | (9 << 16) | (3 << 27) | (3 << 29))
    }

    fn push_raster_wm_oa_config(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
    ) -> Result<(), &'static str> {
        if enable {
            // Mesa's ACMGT3 Ext1010 set uses these OAG selector defaults for
            // rasterizer_sample_output/pixel_write/pixel_blend A counters.
            push_load_register_imm(batch_dwords, cursor, OAG_OASTARTTRIG1, 0)?;
            push_load_register_imm(batch_dwords, cursor, OAG_OASTARTTRIG2, 0x0080_0000)?;
            push_load_register_imm(batch_dwords, cursor, OAG_OASTARTTRIG3, 0)?;
            push_load_register_imm(batch_dwords, cursor, OAG_OASTARTTRIG4, 0x0080_0000)?;
            push_load_register_imm(batch_dwords, cursor, OAG_OAREPORTTRIG1, 0)?;
            push_load_register_imm(batch_dwords, cursor, OAG_SPCTR_CNF, 0)?;
            push_load_register_imm(batch_dwords, cursor, OAA_LENABLE_REG, 0)?;
            push_load_register_imm(batch_dwords, cursor, OAG_OA_PESS, 0)?;
        }
        push_load_register_imm(
            batch_dwords,
            cursor,
            RCS_OACTXCONTROL,
            if enable {
                OACTXCONTROL_COUNTER_RESUME
            } else {
                0
            },
        )?;
        push_load_register_imm(
            batch_dwords,
            cursor,
            OAR_OACONTROL,
            if enable {
                OAR_OACONTROL_FORMAT_A24_A14_B8_C8 | OAR_OACONTROL_COUNTER_ENABLE
            } else {
                0
            },
        )?;
        push_load_register_imm(
            batch_dwords,
            cursor,
            RCS_RING_CONTEXT_CONTROL,
            masked_bits_update(
                if enable {
                    CTX_CTRL_OAC_CONTEXT_ENABLE
                } else {
                    0
                },
                if enable {
                    0
                } else {
                    CTX_CTRL_OAC_CONTEXT_ENABLE
                },
            ),
        )
    }

    fn push_sba_address(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        mocs: u32,
        address: u64,
    ) -> Result<(), &'static str> {
        let low = ((address as u32) & 0xFFFF_F000) | (mocs << 4) | u32::from(enable);
        push(batch_dwords, cursor, low)?;
        push(batch_dwords, cursor, (address >> 32) as u32)
    }

    fn push_sba_size(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        size_bytes: usize,
    ) -> Result<(), &'static str> {
        let size_bytes = crate::intel::align_up(size_bytes, 4096).ok_or("probe-sba-size-align")?;
        let size_bytes = u32::try_from(size_bytes).map_err(|_| "probe-sba-size-convert")?;
        push(batch_dwords, cursor, (size_bytes & 0xFFFF_F000) | u32::from(enable))
    }

    fn binding_table_pool_base_dword(device_id: u16, base: u64) -> u32 {
        let base = (base as u32) & BINDING_TABLE_POOL_BASE_MASK;
        let mocs = RENDER_MOCS & BINDING_TABLE_POOL_MOCS_MASK;
        if device_is_gfx125(device_id) {
            base | mocs
        } else {
            base | BINDING_TABLE_POOL_ENABLE | mocs
        }
    }

    let mesa_host_fixed_function = matches!(backend_probe_mode, BackendProbeMode::MesaLike);
    let artifact_native_fixed_function = draw.native.is_some();
    // Mesa disables BINDING_TABLE_POOL_ALLOC on gfx11 through gfx12.0 because
    // this state can leak/corrupt across contexts.  Keep every pre-gfx12.5
    // draw on the ordinary Surface State Base Address contract, independent
    // of which diagnostic/fixed-function profile selected the draw.
    //
    // The old non-host pool path used `state_region_gpu_addr` even though the
    // packet drops its low twelve bits, then subtracted that same unaligned
    // offset from the binding-table pointer.  Remove that latent zero-pointer
    // contract altogether.  The physical ADL fault still has a precise
    // fingerprint: if the first ISA DWORD (0xA1370040) is consumed as BT entry
    // zero, 0x30006000 + 0xA1370040 resolves to the captured 0xD1376040.  The
    // fresh Native Churn call site requested host-style addressing already,
    // so post-encode packet logging below remains the deciding discriminator.
    // The gfx12.5 pool path is page-based instead.
    let surface_base_relative_binding_table = !device_is_gfx125(warm.device_id);
    let binding_table_pool_size = warm.draw_state_len;
    let surface_state_base_offset_bytes = if surface_base_relative_binding_table {
        shader_layout.state_region_offset_bytes
    } else {
        0
    };
    let surface_state_base_gpu_addr = if surface_base_relative_binding_table {
        shader_layout.state_region_gpu_addr
    } else {
        draw.state_gpu_addr
    };
    let binding_table_pointer_offset = probe_state
        .binding_table_offset_bytes
        .checked_sub(surface_state_base_offset_bytes)
        .ok_or("probe-binding-table-base")?;
    let ps_binding_table_pointer_offset = probe_state
        .ps_binding_table_offset_bytes
        .checked_sub(surface_state_base_offset_bytes)
        .ok_or("probe-binding-table-base")?;
    let surface_state_pointer_offset = probe_state
        .surface_state_offset_bytes
        .checked_sub(surface_state_base_offset_bytes)
        .ok_or("probe-surface-state-base")?;
    let binding_table_pool_base_dw = if surface_base_relative_binding_table {
        RENDER_MOCS & BINDING_TABLE_POOL_MOCS_MASK
    } else {
        binding_table_pool_base_dword(warm.device_id, draw.state_gpu_addr)
    };
    let binding_table_pool_base_hi = if surface_base_relative_binding_table {
        0
    } else {
        (draw.state_gpu_addr >> 32) as u32
    };
    let binding_table_pool_size_dw = if surface_base_relative_binding_table {
        0
    } else {
        u32::try_from(
            crate::intel::align_up(binding_table_pool_size, 4096)
                .ok_or("probe-binding-pool-align")?,
        )
        .map_err(|_| "probe-binding-pool-convert")?
            & 0xFFFF_F000
    };
    let binding_table_gpu_addr = surface_state_base_gpu_addr + binding_table_pointer_offset as u64;
    let binding_table_entry0_gpu_addr =
        surface_state_base_gpu_addr + surface_state_pointer_offset as u64;
    let binding_table_pool_enable = if surface_base_relative_binding_table {
        "disabled-host-style"
    } else if device_is_gfx125(warm.device_id) {
        "implicit-gfx125"
    } else {
        "bit11"
    };
    if artifact_native_fixed_function
        && !CHURN_NATIVE_BINDING_COMMAND_LOGGED.swap(true, Ordering::AcqRel)
    {
        crate::log_info!(
            target: "gpgpu";
            "helio-churn: native-binding-command device=0x{:04X} state_base=0x{:X} state_region=0x{:X} bt_pointer=0x{:X} bt_gpu=0x{:X} surface0_gpu=0x{:X} pool_base_dw=0x{:08X} pool_base_hi=0x{:08X} pool_size_dw=0x{:08X} pool={} contract={} expected_bti1_gpu=0x{:X}\n",
            warm.device_id,
            draw.state_gpu_addr,
            shader_layout.state_region_gpu_addr,
            binding_table_pointer_offset,
            binding_table_gpu_addr,
            binding_table_entry0_gpu_addr,
            binding_table_pool_base_dw,
            binding_table_pool_base_hi,
            binding_table_pool_size_dw,
            binding_table_pool_enable,
            if surface_base_relative_binding_table {
                "surface-base-relative"
            } else {
                "page-pool-relative"
            },
            binding_table_entry0_gpu_addr + 64,
        );
    }
    let vs_ksp_offset = shader_layout.vs.code_offset_bytes + shader_layout.vs.ksp_offset_bytes;
    let ps_ksp_offset = shader_layout.ps.code_offset_bytes + shader_layout.ps.ksp_offset_bytes;
    let host_pipeline_has_no_varyings = mesa_host_fixed_function
        && !artifact_native_fixed_function
        && pipeline.ps.meta.num_varying_inputs == 0;
    let sbe_vertex_read_offset =
        if host_pipeline_has_no_varyings || backend_probe_mode.force_sbe_read0() {
            0
        } else {
            front_end_contract.sbe_read_offset as u32
        };
    let sbe_vertex_read_length =
        if host_pipeline_has_no_varyings || backend_probe_mode.force_sbe_read0() {
            0
        } else {
            front_end_contract.sbe_read_length as u32
        };
    let sbe_num_sf_attrs = if backend_probe_mode.force_one_sbe_attribute() {
        pipeline.ps.meta.num_varying_inputs.max(1)
    } else {
        pipeline.ps.meta.num_varying_inputs
    };
    let sbe_attr_swizzle_enable = !backend_probe_mode.disable_sbe_attr_swizzle();
    // The native Churn PS consumes two contiguous VUE attributes after the
    // position/header slots: world normal (source attribute 0) and material
    // id (source attribute 1).  Xe-LP's enabled SBE swizzle packet must spell
    // that identity routing out; an all-zero payload aliases both inputs to
    // attribute 0.
    let sbe_swiz = sbe_swiz_payload(artifact_native_fixed_function, draw.sampled_texture.is_some());
    let sbe_dw1 = (sbe_vertex_read_offset << 5)
        | (u32::from(sbe_attr_swizzle_enable) << 21)
        | ((sbe_num_sf_attrs as u32) << 22)
        | (u32::from(front_end_contract.force_sbe_read_offset) << 28)
        | (u32::from(front_end_contract.force_sbe_read_length) << 29)
        | (sbe_vertex_read_length << 11);
    let mesa_simple_rect_stack = backend_probe_mode.mesa_simple_rect_stack();
    // The MesaLike probe is the direct replay contract for the verified host
    // Vulkan draw.  These are the last effective CLIP/SF/RASTER payloads in
    // that batch, not values inferred from packet names or intermediate
    // state.  In particular, use the normal clipper with perspective divide;
    // the old ACCEPT_ALL/no-divide combination had no working reference and
    // consistently stopped after VS on ADL.
    // The first-pixel path uses Mesa's valid simple-render CLIP bypass.  VS
    // still exports real VUEs, which are handed directly to SF; only the
    // fixed-function unit at the currently stalled boundary is bypassed.
    let mesa_clip_bypass = backend_probe_mode.mesa_clip_bypass();
    // SF viewport translation happens after fixed-function clipping. Bypass
    // canonical clip rejection for a translated resident scene and rely on
    // the target scissor for final bounds; otherwise offscreen primitives can
    // never become visible when the consumer pans toward them.
    let translated_viewport_clip_bypass = mesa_host_fixed_function
        && viewport_translation_px
            .iter()
            .any(|component| *component != 0.0);
    let clip_dw1 = if translated_viewport_clip_bypass {
        0
    } else if mesa_host_fixed_function {
        0x0004_0400
    } else if mesa_clip_bypass {
        0
    } else {
        (1 << 10)
            | if backend_probe_mode.force_clip_mode() {
                CLIP_FORCE_CLIP_MODE
            } else {
                0
            }
    };
    let clip_dw2 = if translated_viewport_clip_bypass {
        CLIP_PERSPECTIVE_DIVIDE_DISABLE
    } else if mesa_host_fixed_function {
        0xD400_0001
    } else if mesa_simple_rect_stack || mesa_clip_bypass {
        CLIP_PERSPECTIVE_DIVIDE_DISABLE
    } else {
        (if backend_probe_mode.enable_perspective_divide() {
            0
        } else {
            CLIP_PERSPECTIVE_DIVIDE_DISABLE
        }) | if backend_probe_mode.clip_accept_all() {
            CLIP_MODE_ACCEPT_ALL
        } else {
            0
        } | if backend_probe_mode.clip_api_d3d() {
            1 << 30
        } else {
            0
        } | if backend_probe_mode.enable_viewport_xy_clip() {
            1 << 28
        } else {
            0
        } | if backend_probe_mode.disable_clip_unit() {
            0
        } else {
            1 << 31
        }
    };
    let point_width_raw = if batch_mode.point_raster() {
        backend_probe_mode
            .point_width_raw_override()
            .unwrap_or(0x200)
    } else {
        0
    };
    let clip_max_point_width_raw = if batch_mode.point_raster() {
        backend_probe_mode
            .clip_max_point_width_raw_override()
            .unwrap_or(0)
    } else {
        0
    };
    let clip_dw3 = if translated_viewport_clip_bypass {
        0
    } else if mesa_host_fixed_function {
        0x0003_FFE0
    } else if mesa_clip_bypass {
        0
    } else {
        (1 << 5) | ((clip_max_point_width_raw & 0x7FF) << 6)
    };
    // Mesa selects per-poly dereference only when VS has fewer than 192 URB
    // handles.  This ADL configuration has 3576, so the matching value is the
    // default 32-handle block mode (zero).
    let sf_viewport_transform_enable =
        !(batch_mode.screen_space_raster() || backend_probe_mode.disable_sf_viewport_transform());
    let sf_dw1 = if mesa_host_fixed_function {
        0x0008_0402
    } else {
        (u32::from(sf_viewport_transform_enable) << 1) | (1 << 10)
    };
    let sf_dw2 = if backend_probe_mode.sf_deref_block_zero() || TRIANGLE_VS_URB_ENTRIES >= 192 {
        0
    } else {
        1 << 29
    };
    // SF.DW3[10:0] is PointWidth, and DW3[11] selects state-sourced width.
    // Use a deliberately large U8.3-ish value for point-list raster smoke
    // tests so a single center point should cover visible samples if SF/raster
    // is alive.
    let sf_dw3 = if mesa_host_fixed_function {
        0x0200_4808
    } else if batch_mode.point_raster() {
        let point_width_source_state = if backend_probe_mode.point_width_from_vertex() {
            0
        } else {
            1 << 11
        };
        let smooth_point_enable = if backend_probe_mode.smooth_point_raster() {
            1 << 13
        } else {
            0
        };
        point_width_source_state | smooth_point_enable | point_width_raw
    } else {
        0
    };
    // Mirror Mesa's simple-shader path here as literally as possible: cull
    // none, and otherwise leave raster defaults boring until we have visual
    // proof that a more opinionated packet is required.
    let raster_dw1 = if artifact_native_fixed_function {
        // Exact validated gfx12 Xe-LP state for CCW front faces, BACK culling,
        // solid fill, one raster sample, near/far clip and scissor enabled.
        0x04A3_1003
    } else if mesa_host_fixed_function {
        if resident_msaa4 {
            0x04A1_1C03
        } else {
            0x04A1_1003
        }
    } else {
        (1 << 16)
            | if backend_probe_mode.smooth_point_raster() {
                1 << 13
            } else {
                0
            }
            | if backend_probe_mode.dx_multisample_raster() {
                (1 << 12) | (2 << 10)
            } else {
                0
            }
            | if backend_probe_mode.force_multisample_raster() {
                1 << 14
            } else {
                0
            }
            | ((backend_probe_mode.forced_raster_sample_count() & 0x7) << 18)
            | if backend_probe_mode.front_ccw() {
                1 << 21
            } else {
                0
            }
            | if backend_probe_mode.enable_raster_scissor() {
                1 << 1
            } else {
                0
            }
    };
    let raster_dw2 = 0;
    let raster_dw3 = 0;
    let raster_dw4 = 0;
    let primitive_replication_dw1 = if backend_probe_mode.disable_primitive_replication() {
        0
    } else {
        1 << 16
    };
    // Mesa's simple-shader path emits a nearly all-default WM packet here.
    // Keep this dedicated triangle path equally boring rather than forcing
    // point-rule / line-AA bits that the host reference never asked for.
    // Program interpolation from the fragment executable ABI. HelioV's
    // sampled shader is not an artifact-native Churn draw, but it still reads
    // a perspective UV varying and therefore needs the same pixel payload.
    let wm_barycentric_mode = wm_barycentric_mode(
        pipeline.ps.meta.num_varying_inputs,
        backend_probe_mode.force_ps_bary_planes(),
    );
    let force_wm_thread_dispatch = (matches!(backend_probe_mode, BackendProbeMode::WmLateReemit)
        || (batch_mode.vf_synthesized_vue()
            && !mesa_simple_rect_stack
            && !matches!(backend_probe_mode, BackendProbeMode::WmNormalDispatch)))
        && !backend_probe_mode.suppress_forced_wm_thread_dispatch();
    let wm_dw1 = (1 << 31)
        | if batch_mode.point_raster() && !backend_probe_mode.suppress_wm_point_rule() {
            1 << 2
        } else {
            0
        }
        | if backend_probe_mode.force_kill_pixel_off() {
            WM_FORCE_KILL_PIXEL_OFF
        } else {
            0
        }
        | if force_wm_thread_dispatch {
            // The VF-fed draw path is our backend isolation probe, so make the
            // fragment launch condition explicit instead of inferring it from
            // the minimal Mesa-like defaults.
            2 << 19
        } else {
            0
        }
        | wm_barycentric_mode;
    let wm_depth_stencil_dw1 = depth_config.map_or(0, |depth| {
        u32::from(depth.write_enabled) | (1 << 1) | (u32::from(depth.compare_function) << 5)
    });
    let wm_depth_stencil_dw2 = 0;
    let wm_depth_stencil_dw3 = 0;
    let wm_chroma_key_dw1 = 0;
    const BLEND_FACTOR_ONE: u32 = 0x01;
    const BLEND_FACTOR_SRC_ALPHA: u32 = 0x03;
    const BLEND_FACTOR_INV_SRC_ALPHA: u32 = 0x13;
    let ps_blend_dw1 = if backend_probe_mode.disable_ps_contract()
        || backend_probe_mode.disable_ps_blend_writeable_rt()
    {
        0
    } else if matches!(blend_mode, TriangleBlendProbeMode::StraightAlpha) {
        // 3DSTATE_PS_BLEND: writable RT + color blending enabled, with
        // straight-alpha RGB and independent alpha factors.
        (1 << 30)
            | (1 << 29)
            | (BLEND_FACTOR_ONE << 24)
            | (BLEND_FACTOR_INV_SRC_ALPHA << 19)
            | (BLEND_FACTOR_SRC_ALPHA << 14)
            | (BLEND_FACTOR_INV_SRC_ALPHA << 9)
            | (1 << 7)
    } else {
        1 << 30
    };
    let streamout_dw1 = (1 << 25) | (1 << 30) | (1 << 31);
    let streamout_dw2 = streamout_experiment.vertex_read_length();
    let streamout_dw3 = streamout_experiment.vertex_bytes() as u32;
    let streamout_dw4 = 0;
    let streamout_surface_size_dwords = (warm.streamout_len / 4).saturating_sub(1) as u32;
    let so_buffer_index_dw1 = (RENDER_MOCS << 22) | (1 << 20) | (1 << 21) | (1 << 31);
    let so_buffer_stream_offset_dw = 0u32;
    // Mesa zeros this packet during init to clear inherited clear/resolve
    // overrides. The wm-hz probe keeps those op bits clear but arms the
    // packet-local sample mask to test whether WM treats zero as no samples.
    let wm_hz_op_dw1 = 0;
    let wm_hz_op_dw2 = 0;
    let wm_hz_op_dw3 = 0;
    let wm_hz_op_dw4 = if matches!(backend_probe_mode, BackendProbeMode::WmHzSampleMask)
        || matches!(backend_probe_mode, BackendProbeMode::WmLateReemit)
        || (backend_probe_mode.uses_raster_wm_oa()
            && !backend_probe_mode.suppress_wm_hz_op_sample_mask())
    {
        backend_probe_mode.wm_hz_op_sample_mask() & 0xFFFF
    } else {
        0
    };
    let gfx125_sample_pattern_dwords = if resident_msaa4 {
        [
            0x8888_8888,
            0x8888_8888,
            0x8888_8888,
            0x8888_8888,
            0x8888_8888,
            0x8888_8888,
            0xAE2A_E662,
            0x0088_44CC,
        ]
    } else {
        [0x8888_8888; 8]
    };
    let gfx125_slice_hash =
        device_is_gfx125(warm.device_id).then(|| gfx125_slice_hash_config(warm));
    let gfx125_3d_mode_dw1 = gfx125_slice_hash.map(gfx125_3d_mode_dw1).unwrap_or(0);
    let gfx125_3d_mode_dw2 = 0;
    let gfx125_3d_mode_dw3 = gfx125_3d_mode_dw3();
    let ps_binding_table_entry_count = match backend_probe_mode {
        BackendProbeMode::MesaLike
        | BackendProbeMode::PsBindingTableCountZero
        | BackendProbeMode::WmNormalDispatch
        | BackendProbeMode::PsWmNormalBt0
        | BackendProbeMode::PsWmNormalBt0CpDep
        | BackendProbeMode::PsPrmEarlyRasterGate
        | BackendProbeMode::PsPrmRasterGateSbeBeforeSf
        | BackendProbeMode::PsPrmSbeNoAttrSwizzle
        | BackendProbeMode::PsPrmNoPrimitiveReplication
        | BackendProbeMode::PsExtraBeforePs
        | BackendProbeMode::PsWmReemitAfterPsExtra
        | BackendProbeMode::PsOmitWmHzOp
        | BackendProbeMode::PsSampleAll
        | BackendProbeMode::PsSbeRead0
        | BackendProbeMode::PsSbeBeforeSf
        | BackendProbeMode::PsNoPrimitiveReplication
        | BackendProbeMode::PsNoWriteableRt
        | BackendProbeMode::PsNoCcPointer
        | BackendProbeMode::PsDispatchSlot0
        | BackendProbeMode::PsDispatchSlot1
        | BackendProbeMode::PsDispatchSlot2
        | BackendProbeMode::PsDispatchAllKspSlots
        | BackendProbeMode::PsSimd16
        | BackendProbeMode::PsEotOnly
        | BackendProbeMode::PsCpsDisabled
        | BackendProbeMode::PsPayloadPushConstant
        | BackendProbeMode::PsPayloadAttributeEnable
        | BackendProbeMode::PsPayloadSimpleHint
        | BackendProbeMode::PsPayloadSourceDepthW
        | BackendProbeMode::PsPayloadBaryPlanes
        | BackendProbeMode::PsGrfStartR1
        | BackendProbeMode::PsGrfStartR2
        | BackendProbeMode::PsGrfStartR4
        | BackendProbeMode::PsGrfMaxThreads31
        | BackendProbeMode::PsGrfMaxThreads15
        | BackendProbeMode::WmHzSampleMask
        | BackendProbeMode::WmLateReemit
        | BackendProbeMode::RasterWmInputOa
        | BackendProbeMode::RasterWmInputOaSurfaceHalign128
        | BackendProbeMode::RasterWmInputOaKillOff
        | BackendProbeMode::RasterWmInputOaSmoothPoint
        | BackendProbeMode::RasterWmInputOaMsRaster
        | BackendProbeMode::RasterWmInputOaMsRasterForced
        | BackendProbeMode::RasterWmInputOaDerefBlock0
        | BackendProbeMode::RasterWmInputOaNoHzOp
        | BackendProbeMode::RasterWmInputOaWmNormalDispatch
        | BackendProbeMode::RasterWmInputOaWmReemitAfterPsExtra
        | BackendProbeMode::RasterWmInputOaOmitHzOp
        | BackendProbeMode::RasterWmInputOaPsDisabled
        | BackendProbeMode::RasterWmInputOaBtCountOne
        | BackendProbeMode::RasterWmInputOaScissorOnly
        | BackendProbeMode::RasterWmInputOaMesaSimpleRect
        | BackendProbeMode::RasterWmInputOaMesaSimpleRectEarly
        | BackendProbeMode::RasterWmInputOaMesaSimpleRectArtificial
        | BackendProbeMode::RasterWmInputOaMesaSimpleRectNoSrcHeader
        | BackendProbeMode::RasterWmInputOaEarlySample
        | BackendProbeMode::RasterWmInputOaEarlyKillOff
        | BackendProbeMode::RasterWmInputOaClipNormal
        | BackendProbeMode::RasterWmInputOaClipPerspective
        | BackendProbeMode::RasterWmInputOaClipDisabled
        | BackendProbeMode::RasterWmInputOaClipDisabledArtificial
        | BackendProbeMode::RasterWmInputOaClipForceMode
        | BackendProbeMode::RasterWmInputOaClipApiD3d
        | BackendProbeMode::RasterWmInputOaClipViewportXy
        | BackendProbeMode::RasterWmInputOaEarlyClipViewportXy
        | BackendProbeMode::RasterWmInputOaEarlyPointWidth1023
        | BackendProbeMode::RasterWmInputOaEarlyMsRasterForced
        | BackendProbeMode::RasterWmInputOaSbeBeforeClip
        | BackendProbeMode::RasterWmInputOaSbeBeforeSf
        | BackendProbeMode::RasterWmInputOaSbeRead0
        | BackendProbeMode::RasterWmInputOaDrawRectEarlyOnly
        | BackendProbeMode::RasterWmInputOaSampleMaskEarlyOnly
        | BackendProbeMode::RasterWmInputOaPipeControlClipSf
        | BackendProbeMode::RasterWmInputOaWmHzOpBeforeWm
        | BackendProbeMode::RasterWmInputOaWmHzOpAfterPsExtra
        | BackendProbeMode::RasterWmInputOaPayloadAttributeEnable
        | BackendProbeMode::RasterWmInputOaPayloadSourceDepthW
        | BackendProbeMode::RasterWmInputOaPayloadBaryPlanes
        | BackendProbeMode::RasterWmInputOaSampleAll
        | BackendProbeMode::RasterWmInputOaWmHandoff
        | BackendProbeMode::RasterWmInputOaSampleAllWmHandoff
        | BackendProbeMode::RasterWmInputOaFrontCcw
        | BackendProbeMode::RasterWmInputOaNoPrimitiveReplication
        | BackendProbeMode::RasterWmInputOaVfGeometryDistribution
        | BackendProbeMode::RasterWmInputOaPointWidth8
        | BackendProbeMode::RasterWmInputOaPointWidth8ClipMax
        | BackendProbeMode::RasterWmInputOaPointWidth64
        | BackendProbeMode::RasterWmInputOaPointWidth64SurfaceHalign128
        | BackendProbeMode::RasterWmInputOaPointWidth64ClipMax
        | BackendProbeMode::RasterWmInputOaPointWidth64Early
        | BackendProbeMode::RasterWmInputOaPointWidth64EarlyScissor
        | BackendProbeMode::RasterWmInputOaPointWidth64Screen
        | BackendProbeMode::RasterWmInputOaPointWidth64Artificial
        | BackendProbeMode::RasterWmInputOaPointWidth64WmNormalDispatch
        | BackendProbeMode::RasterWmInputOaPointWidth64WmReemitAfterPsExtra
        | BackendProbeMode::RasterWmInputOaPointWidth64OmitHzOp
        | BackendProbeMode::RasterWmInputOaPointWidth64PsDisabled
        | BackendProbeMode::RasterWmInputOaPointWidth64PayloadAttributeEnable
        | BackendProbeMode::RasterWmInputOaPointWidth64PayloadSourceDepthW
        | BackendProbeMode::RasterWmInputOaPointWidth64PayloadBaryPlanes
        | BackendProbeMode::RasterWmInputOaPointWidth64SbeBeforeClip
        | BackendProbeMode::RasterWmInputOaPointWidth64SbeBeforeSf
        | BackendProbeMode::RasterWmInputOaPointWidth1023
        | BackendProbeMode::RasterWmInputOaPointWidth1023NoWmPoint
        | BackendProbeMode::RasterWmInputOaPointWidth1023Scissor
        | BackendProbeMode::RasterWmInputOaPointWidthVertex
        | BackendProbeMode::RasterWmInputOaHammer
        | BackendProbeMode::RasterWmInputOaScreenHammer => {
            pipeline.ps.meta.kernel.binding_table_entry_count
        }
        BackendProbeMode::PsBindingTableCountOne => {
            pipeline.ps.meta.kernel.binding_table_entry_count.max(1)
        }
    };
    let ps_binding_table_entry_count =
        if matches!(backend_probe_mode, BackendProbeMode::PsBindingTableCountZero)
            || matches!(backend_probe_mode, BackendProbeMode::PsWmNormalBt0)
            || matches!(backend_probe_mode, BackendProbeMode::PsWmNormalBt0CpDep)
            || matches!(backend_probe_mode, BackendProbeMode::PsPrmEarlyRasterGate)
            || matches!(backend_probe_mode, BackendProbeMode::PsPrmRasterGateSbeBeforeSf)
            || matches!(backend_probe_mode, BackendProbeMode::PsPrmSbeNoAttrSwizzle)
            || matches!(backend_probe_mode, BackendProbeMode::PsPrmNoPrimitiveReplication)
            || (backend_probe_mode.uses_raster_wm_oa()
                && !backend_probe_mode.keep_ps_binding_table_count())
        {
            0
        } else {
            ps_binding_table_entry_count
        };
    let ps_push_constant_enable = pipeline.ps.meta.kernel.push_constant_bytes > 0
        || matches!(backend_probe_mode, BackendProbeMode::PsPayloadPushConstant);
    let ps_max_threads_per_psd = backend_probe_mode
        .ps_max_threads_override()
        .unwrap_or(TRIANGLE_PS_MAX_THREADS);
    let ps_grf_start = backend_probe_mode
        .ps_grf_start_override()
        .unwrap_or(pipeline.ps.meta.kernel.grf_start_register);
    let ps_ksp_base = ps_ksp_offset & !0x3F;
    let host_simd16_pipeline = crate::intel::shader::triangle_pipeline_simd16();
    let host_simd16_pair = (pipeline.ps.code.as_ptr()
        == crate::intel::shader::triangle_pipeline().ps.code.as_ptr())
    .then_some((
        host_simd16_pipeline.ps.meta.kernel.code_offset_bytes & !0x3F,
        host_simd16_pipeline.ps.meta.kernel.grf_start_register,
    ));
    let ps_dispatch_contract = pixel_shader_dispatch_contract(
        backend_probe_mode,
        pipeline.ps.meta.kernel.dispatch_mode,
        pipeline.ps.meta.uses_vmask,
        ps_ksp_base,
        ps_grf_start,
        host_simd16_pair,
    );
    let (ps_dispatch_8, ps_dispatch_16, ps_dispatch_32) = (
        ps_dispatch_contract.dispatch_8,
        ps_dispatch_contract.dispatch_16,
        ps_dispatch_contract.dispatch_32,
    );
    let ps_dw3 = (binding_table_entry_count_encoding(ps_binding_table_entry_count) << 18)
        | (sampler_count_encoding(pipeline.ps.meta.kernel.sampler_count) << 27)
        | (u32::from(ps_dispatch_contract.vector_mask_enable) * PS_VECTOR_MASK_ENABLE);
    let ps_dw6 = ps_dispatch_8
        | (ps_dispatch_16 << 1)
        | (ps_dispatch_32 << 2)
        | (u32::from(ps_push_constant_enable) * PS_PUSH_CONSTANT_ENABLE)
        | (ps_max_threads_per_psd << PS_MAX_THREADS_SHIFT);
    let ps_dw7 = if artifact_native_fixed_function
        && matches!(
            pipeline.ps.meta.kernel.dispatch_mode,
            crate::intel::shader::DispatchMode::Simd8
        ) {
        // Churn's authenticated SIMD8 binary is KSP0.  The captured ANV
        // packet pairs KSP0 with Constant/Setup Data 0 (DW7[22:16]); the
        // generic diagnostic mapper predates this artifact-native contract
        // and keeps its historical variable-dispatch slot layout.
        u32::from(ps_grf_start) << 16
    } else {
        ps_dispatch_contract.grf_start_dw
    };
    let [ps_ksp0, ps_ksp1, ps_ksp2] = ps_dispatch_contract.ksp;
    let ps_scratch_space_buffer = 0u32;
    let ps_extra_attribute_enable =
        pipeline.ps.meta.num_varying_inputs > 0 || backend_probe_mode.force_ps_attribute_payload();
    let ps_extra_dw1 = (u32::from(pipeline.ps.meta.computed_stencil)
        * PS_EXTRA_PIXEL_SHADER_COMPUTES_STENCIL)
        | (u32::from(pipeline.ps.meta.persample_dispatch) * PS_EXTRA_PIXEL_SHADER_IS_PER_SAMPLE)
        | (u32::from(ps_extra_attribute_enable) * PS_EXTRA_ATTRIBUTE_ENABLE)
        | (u32::from(matches!(backend_probe_mode, BackendProbeMode::PsPayloadSimpleHint))
            * PS_EXTRA_SIMPLE_PS_HINT)
        | (u32::from(backend_probe_mode.force_ps_dependency_on_cpsize_change())
            * PS_EXTRA_ENABLE_PS_DEPENDENCY_ON_CPSIZE_CHANGE)
        | (u32::from(backend_probe_mode.force_ps_source_depth_w())
            * (PS_EXTRA_REQUIRES_SOURCE_DEPTH_W_PLANE
                | PS_EXTRA_USES_SOURCE_W
                | PS_EXTRA_USES_SOURCE_DEPTH))
        | (u32::from(backend_probe_mode.force_ps_bary_planes())
            * (PS_EXTRA_REQUIRES_NONPERSPECTIVE_BARY_PLANE
                | PS_EXTRA_REQUIRES_PERSPECTIVE_BARY_PLANE))
        | ((pipeline.ps.meta.computed_depth_mode as u32) << 26)
        | PS_EXTRA_PIXEL_SHADER_VALID;
    let ps_extra_dw1 = if backend_probe_mode.disable_ps_contract() {
        ps_extra_dw1 & !PS_EXTRA_PIXEL_SHADER_VALID
    } else {
        ps_extra_dw1
    };

    batch_dwords.fill(0);

    log_batch_offset(cursor, "PIPE_CONTROL flush");
    push_pipe_control_full(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER,
        PIPE_CONTROL_FLUSH_BITS,
    )?;
    log_batch_offset(cursor, "PIPE_CONTROL invalidate");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS)?;

    log_batch_offset(cursor, "PIPELINE_SELECT");
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_3D)?;
    log_batch_offset(cursor, "MI_STORE_DATA_IMM batch-entry");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_BATCH_ENTRY_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_BATCH_ENTRY,
    )?;

    if device_is_gfx12(warm.device_id) {
        let l3alloc = if device_is_gfx125(warm.device_id) {
            GFX125_L3ALLOC_FULL_WAYS
        } else {
            GEN12_L3ALLOC_ADL_DEFAULT
        };
        log_batch_offset(cursor, "MI_LOAD_REGISTER_IMM L3ALLOC");
        push_load_register_imm(batch_dwords, &mut cursor, GEN12_L3ALLOC, l3alloc)?;
        intel_render_verbose_log!(
            "l3alloc-init device=0x{:04X} value=0x{:08X} profile={}\n",
            warm.device_id,
            l3alloc,
            if device_is_gfx125(warm.device_id) {
                "gfx125-full-ways"
            } else {
                "adl-gfx12-default"
            },
        );
    }

    log_batch_offset(cursor, "STATE_BASE_ADDRESS");
    push(batch_dwords, &mut cursor, STATE_BASE_ADDRESS_CMD)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, draw.state_gpu_addr)?;
    // Stateless Data Port Access MOCS is explicitly non-zero in the Gen12
    // packet contract, even when this draw has no stateless shader access.
    push(batch_dwords, &mut cursor, RENDER_MOCS << 16)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, surface_state_base_gpu_addr)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, draw.state_gpu_addr)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, INDIRECT_OBJECT_SBA_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, draw.state_gpu_addr)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, INDIRECT_OBJECT_SBA_SIZE_BYTES)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    // Complete the 22-DWORD Gen12 SBA.  The host keeps a valid bindless
    // surface range and an explicitly modified null bindless-sampler base;
    // leaving all six DWORDs zero leaves non-zero-MOCS state unspecified.
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, draw.state_gpu_addr)?;
    push(batch_dwords, &mut cursor, BINDLESS_SURFACE_STATE_SIZE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    if device_is_gfx12(warm.device_id) {
        let cps_ptr = probe_state.cps_state_offset_bytes & !0x1F;
        log_batch_offset(cursor, "3DSTATE_CPS_POINTERS disabled");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_CPS_POINTERS)?;
        push(batch_dwords, &mut cursor, cps_ptr)?;
        intel_render_verbose_log!(
            "cps-pointers-init device=0x{:04X} cps_ptr=0x{:X} cps_gpu=0x{:X} state_dwords={} mode=none source=mesa-gen12-init\n",
            warm.device_id,
            cps_ptr,
            draw.state_gpu_addr + cps_ptr as u64,
            CPS_STATE_DWORDS,
        );
    }

    log_batch_offset(cursor, "3DSTATE_AA_LINE_PARAMETERS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_AA_LINE_PARAMETERS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    if device_is_gfx12(warm.device_id) {
        log_batch_offset(cursor, "3DSTATE_SAMPLE_PATTERN");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLE_PATTERN)?;
        for pattern_dw in gfx125_sample_pattern_dwords {
            push(batch_dwords, &mut cursor, pattern_dw)?;
        }
    }

    if device_is_gfx125(warm.device_id) {
        log_batch_offset(cursor, "3DSTATE_SLICE_TABLE_STATE_POINTERS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SLICE_TABLE_STATE_POINTERS)?;
        push(batch_dwords, &mut cursor, probe_state.slice_hash_table_offset_bytes | 1)?;

        log_batch_offset(cursor, "3DSTATE_3D_MODE");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_3D_MODE)?;
        push(batch_dwords, &mut cursor, gfx125_3d_mode_dw1)?;
        push(batch_dwords, &mut cursor, gfx125_3d_mode_dw2)?;
        push(batch_dwords, &mut cursor, gfx125_3d_mode_dw3)?;
        let slice_hash = gfx125_slice_hash.expect("gfx125 slice hash config");
        intel_render_verbose_log!(
            "gfx125-svl-init sample_pattern={} slice_hash_ptr=0x{:X} geom_dss=0x{:08X} ppipe_dss={}/{}/{} mask1=0x{:X} mask2=0x{:X} mode_dw1=0x{:08X} mode_dw3=0x{:08X} cross_slice_mode={}({}) rhwo_disable=1\n",
            if resident_msaa4 {
                "standard-4x"
            } else {
                "center"
            },
            probe_state.slice_hash_table_offset_bytes,
            slice_hash.geometry_dss_enable,
            slice_hash.ppipe_subslices[0],
            slice_hash.ppipe_subslices[1],
            slice_hash.ppipe_subslices[2],
            slice_hash.ppipe_mask1,
            slice_hash.ppipe_mask2,
            gfx125_3d_mode_dw1,
            gfx125_3d_mode_dw3,
            slice_hash.cross_slice_hashing_mode,
            if slice_hash.cross_slice_hashing_mode == GFX125_3D_MODE_CROSS_SLICE_HASHING_32X32 {
                "hashing32x32"
            } else {
                "normal"
            },
        );
    }

    log_batch_offset(cursor, "PIPE_CONTROL pre-binding-table-pool");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;
    log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POOL_ALLOC");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POOL_ALLOC)?;
    push(batch_dwords, &mut cursor, binding_table_pool_base_dw)?;
    push(batch_dwords, &mut cursor, binding_table_pool_base_hi)?;
    push(batch_dwords, &mut cursor, binding_table_pool_size_dw)?;
    log_batch_offset(cursor, "PIPE_CONTROL post-binding-table-pool");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS)?;

    log_batch_offset(cursor, "3DSTATE_SAMPLER_STATE_POINTERS_VS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLER_STATE_POINTERS_VS)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_SAMPLER_STATE_POINTERS_PS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLER_STATE_POINTERS_PS)?;
    push(batch_dwords, &mut cursor, probe_state.sampler_state_offset_bytes)?;

    log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POINTERS_VS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_VS)?;
    push(
        batch_dwords,
        &mut cursor,
        if artifact_native_fixed_function {
            binding_table_pointer_offset
        } else {
            0
        },
    )?;
    log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POINTERS_HS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_HS)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POINTERS_DS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_DS)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POINTERS_GS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_GS)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POINTERS_PS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_PS)?;
    push(batch_dwords, &mut cursor, ps_binding_table_pointer_offset)?;

    if device_is_gfx12(warm.device_id) {
        // ADL has 32 KiB of constant URB space.  Mesa partitions it between
        // every active graphics stage even when the shaders consume no push
        // data.  Our VS URB allocation starts at address 4 (4 * 8 KiB), so
        // these packets are the missing definition of that reserved region.
        log_batch_offset(cursor, "3DSTATE_PUSH_CONSTANT_ALLOC_VS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PUSH_CONSTANT_ALLOC_VS)?;
        push(batch_dwords, &mut cursor, 16)?;
        log_batch_offset(cursor, "3DSTATE_PUSH_CONSTANT_ALLOC_HS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PUSH_CONSTANT_ALLOC_HS)?;
        push(batch_dwords, &mut cursor, 0)?;
        log_batch_offset(cursor, "3DSTATE_PUSH_CONSTANT_ALLOC_DS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PUSH_CONSTANT_ALLOC_DS)?;
        push(batch_dwords, &mut cursor, 0)?;
        log_batch_offset(cursor, "3DSTATE_PUSH_CONSTANT_ALLOC_GS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PUSH_CONSTANT_ALLOC_GS)?;
        push(batch_dwords, &mut cursor, 0)?;
        log_batch_offset(cursor, "3DSTATE_PUSH_CONSTANT_ALLOC_PS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PUSH_CONSTANT_ALLOC_PS)?;
        push(batch_dwords, &mut cursor, (16 << 16) | 16)?;
        intel_render_focus_log!(
            "probe-push-constant-urb-partition total_kb=32 vs[offset_kb=0 size_kb=16] hs=0 ds=0 gs=0 ps[offset_kb=16 size_kb=16] following_urb_start_8kb={} source=mesa-adl-gfx12\n",
            TRIANGLE_VS_URB_START,
        );

        log_batch_offset(cursor, "3DSTATE_CONSTANT_ALL empty-all-stages pre-ps");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_CONSTANT_ALL_EMPTY_ALL_STAGES)?;
        push(batch_dwords, &mut cursor, RENDER_MOCS)?;
    }

    log_batch_offset(cursor, "3DSTATE_VIEWPORT_STATE_POINTERS_CC");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VIEWPORT_STATE_POINTERS_CC)?;
    push(batch_dwords, &mut cursor, probe_state.cc_viewport_offset_bytes)?;
    log_batch_offset(cursor, "3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP)?;
    push(batch_dwords, &mut cursor, probe_state.sf_clip_viewport_offset_bytes)?;
    log_batch_offset(cursor, "3DSTATE_SCISSOR_STATE_POINTERS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SCISSOR_STATE_POINTERS)?;
    push(batch_dwords, &mut cursor, probe_state.scissor_rect_offset_bytes)?;

    log_batch_offset(cursor, "3DSTATE_VERTEX_BUFFERS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VERTEX_BUFFERS_1)?;
    let vertex_buffer_dw1 = draw.vertex_stride
        | (1 << 14)
        | (VERTEX_BUFFER_MOCS << 16)
        | if device_is_gfx12(warm.device_id) {
            VERTEX_BUFFER_L3_BYPASS_DISABLE
        } else {
            0
        };
    push(batch_dwords, &mut cursor, vertex_buffer_dw1)?;
    push_addr(batch_dwords, &mut cursor, draw.vertex_gpu_addr)?;
    push(batch_dwords, &mut cursor, draw.vertex_buffer_bytes)?;
    intel_render_verbose_log!(
        "probe-vb-state dw1=0x{:08X} pitch={} format={:?} mocs={} address_modify=1 l3_bypass_disable={} gpu=0x{:X} bytes={} source=mesa-gen12-verified-draw\n",
        vertex_buffer_dw1,
        draw.vertex_stride,
        draw.vertex_format,
        VERTEX_BUFFER_MOCS,
        (vertex_buffer_dw1 >> 25) & 0x1,
        draw.vertex_gpu_addr,
        draw.vertex_buffer_bytes,
    );

    log_batch_offset(cursor, "3DSTATE_VERTEX_ELEMENTS");
    let vf_vertex_element_count = if let Some(native) = draw.native {
        native.vertex_element_count
    } else if mesa_simple_rect_stack && vf_synthesized_vue {
        2
    } else {
        streamout_experiment.vf_vertex_element_count()
    };
    push(
        batch_dwords,
        &mut cursor,
        if artifact_native_fixed_function || vf_synthesized_vue {
            cmd_3dstate_vertex_elements(vf_vertex_element_count)?
        } else {
            CMD_3DSTATE_VERTEX_ELEMENTS_1
        },
    )?;
    if mesa_simple_rect_stack && vf_synthesized_vue {
        // Mesa's simple-shader / BLORP path deliberately builds a synthetic
        // VUE header element and then lets SGVS overwrite header DW1 with the
        // primitive instance id. Position remains the only real vertex buffer.
        let header_component0 = if backend_probe_mode.mesa_simple_rect_no_src_header() {
            VFCOMP_STORE_0
        } else {
            VFCOMP_STORE_SRC
        };
        push_vertex_element_state(
            batch_dwords,
            &mut cursor,
            1,
            0,
            SURFACE_FORMAT_R32G32B32A32_FLOAT,
            header_component0,
            VFCOMP_STORE_0,
            VFCOMP_STORE_0,
            VFCOMP_STORE_0,
        )?;
        push_vertex_element_state(
            batch_dwords,
            &mut cursor,
            0,
            0,
            SURFACE_FORMAT_R32G32B32_FLOAT,
            VFCOMP_STORE_SRC,
            VFCOMP_STORE_SRC,
            VFCOMP_STORE_SRC,
            VFCOMP_STORE_1_FP,
        )?;
    } else if vf_synthesized_vue {
        match streamout_experiment {
            StreamoutProofExperiment::PositionSlot0 => {
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    0,
                    SURFACE_FORMAT_R32G32B32_FLOAT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_1_FP,
                )?;
            }
            StreamoutProofExperiment::PositionSlot1 => {
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    0,
                    SURFACE_FORMAT_R32G32B32A32_FLOAT,
                    VFCOMP_STORE_0,
                    VFCOMP_STORE_0,
                    VFCOMP_STORE_0,
                    VFCOMP_STORE_0,
                )?;
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    0,
                    SURFACE_FORMAT_R32G32B32_FLOAT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_1_FP,
                )?;
            }
            StreamoutProofExperiment::PrmVueHeaderPositionSlots01
            | StreamoutProofExperiment::PrmVueHeaderPositionXywzSlots01
            | StreamoutProofExperiment::HeaderAndPositionSlots01 => {
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    0,
                    SURFACE_FORMAT_R32G32B32A32_FLOAT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                )?;
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    16,
                    SURFACE_FORMAT_R32G32B32A32_FLOAT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                )?;
            }
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1 => {
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    16,
                    SURFACE_FORMAT_R32G32B32A32_FLOAT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                )?;
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    0,
                    SURFACE_FORMAT_R32G32B32A32_FLOAT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                )?;
            }
        }
    } else {
        match draw.vertex_format {
            TriangleVertexFormat::Float2 => push_vertex_element_state(
                batch_dwords,
                &mut cursor,
                0,
                0,
                SURFACE_FORMAT_R32G32_FLOAT,
                VFCOMP_STORE_SRC,
                VFCOMP_STORE_SRC,
                VFCOMP_STORE_0,
                VFCOMP_STORE_1_FP,
            )?,
            TriangleVertexFormat::Float3 => push_vertex_element_state(
                batch_dwords,
                &mut cursor,
                0,
                0,
                SURFACE_FORMAT_R32G32B32_FLOAT,
                VFCOMP_STORE_SRC,
                VFCOMP_STORE_SRC,
                VFCOMP_STORE_SRC,
                VFCOMP_STORE_1_FP,
            )?,
            TriangleVertexFormat::PosUv => {
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    0,
                    SURFACE_FORMAT_R32G32B32_FLOAT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_1_FP,
                )?;
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    12,
                    SURFACE_FORMAT_R32G32_FLOAT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_0,
                    VFCOMP_STORE_1_FP,
                )?;
            }
            TriangleVertexFormat::PosNormal => {
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    0,
                    SURFACE_FORMAT_R32G32B32_FLOAT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_1_FP,
                )?;
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    12,
                    SURFACE_FORMAT_R32G32B32_FLOAT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_1_FP,
                )?;
                // ANV appends one non-memory synthetic element. SGVS writes
                // StartingInstance into Y and InstanceID into W; all fetched
                // components are zero so VB31 needs no backing allocation.
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    31,
                    0,
                    SURFACE_FORMAT_R32G32_UINT,
                    VFCOMP_STORE_0,
                    VFCOMP_STORE_0,
                    VFCOMP_STORE_0,
                    VFCOMP_STORE_0,
                )?;
            }
            TriangleVertexFormat::PosNormalUv => {
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    0,
                    SURFACE_FORMAT_R32G32B32_FLOAT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_1_FP,
                )?;
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    12,
                    SURFACE_FORMAT_R32G32B32_FLOAT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_1_FP,
                )?;
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    24,
                    SURFACE_FORMAT_R32G32_FLOAT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_0,
                    VFCOMP_STORE_1_FP,
                )?;
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    31,
                    0,
                    SURFACE_FORMAT_R32G32_UINT,
                    VFCOMP_STORE_0,
                    VFCOMP_STORE_0,
                    VFCOMP_STORE_0,
                    VFCOMP_STORE_0,
                )?;
            }
        }
    }

    log_batch_offset(cursor, "3DSTATE_VF_STATISTICS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_STATISTICS | 1)?;
    if device_is_gfx125(warm.device_id) {
        // Reset gfx125 vertex distribution state explicitly before the real
        // VS path. Mesa emits this packet in the gfx state stream, and leaving
        // it inherited makes the VS front-end path less deterministic than the
        // otherwise identical VF-fed probe.
        log_batch_offset(cursor, "3DSTATE_VFG");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VFG)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_VF");
    let vf_geometry_distribution_enable = if (mesa_simple_rect_stack
        || backend_probe_mode.force_vf_geometry_distribution())
        && device_is_gfx125(warm.device_id)
    {
        1 << 12
    } else {
        0
    };
    let vf_component_packing_enable = if mesa_host_fixed_function || artifact_native_fixed_function
    {
        1 << 9
    } else {
        0
    };
    push(
        batch_dwords,
        &mut cursor,
        CMD_3DSTATE_VF | vf_geometry_distribution_enable | vf_component_packing_enable,
    )?;
    // Keep the disabled cut-index value identical to the verified gfx12 Mesa
    // draw.  It is architecturally ignored for this non-indexed triangle list,
    // but is part of the exact SVL state accompanying component packing.
    push(batch_dwords, &mut cursor, if mesa_host_fixed_function { 0xFFFF } else { 0 })?;
    if let Some(index_buffer) = draw.index_buffer {
        log_batch_offset(cursor, "3DSTATE_INDEX_BUFFER");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_INDEX_BUFFER)?;
        let index_buffer_dw1 = (RENDER_MOCS & 0x7F)
            | (INDEX_BUFFER_FORMAT_DWORD << 8)
            | if device_is_gfx12(warm.device_id) {
                INDEX_BUFFER_L3_BYPASS_DISABLE
            } else {
                0
            };
        push(batch_dwords, &mut cursor, index_buffer_dw1)?;
        push_addr(batch_dwords, &mut cursor, index_buffer.gpu_addr)?;
        push(batch_dwords, &mut cursor, index_buffer.byte_len)?;
        intel_render_focus_log!(
            "{} index-buffer-state accepted=1 format=u32 count={} bytes={} gpu=0x{:X} mocs={} l3_bypass_disable={} primitive_access=random retained=1 does_not_prove=index_fetch\n",
            submit_name,
            index_buffer.index_count,
            index_buffer.byte_len,
            index_buffer.gpu_addr,
            RENDER_MOCS,
            (index_buffer_dw1 >> 11) & 0x1,
        );
    }
    log_batch_offset(cursor, "3DSTATE_VF_SGVS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_SGVS)?;
    let vf_sgvs_dw1 = if let Some(native) = draw.native {
        native.vf_sgvs_dw1
    } else if mesa_host_fixed_function {
        0x6001_4001
    } else if mesa_simple_rect_stack && vf_synthesized_vue {
        (1 << 31) | (1 << 29)
    } else {
        0
    };
    push(batch_dwords, &mut cursor, vf_sgvs_dw1)?;
    log_batch_offset(cursor, "3DSTATE_VF_SGVS_2");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_SGVS_2)?;
    push(
        batch_dwords,
        &mut cursor,
        draw.native.map_or_else(
            || {
                if mesa_host_fixed_function {
                    0x3001_0001
                } else {
                    0
                }
            },
            |native| native.vf_sgvs_2_dw1,
        ),
    )?;
    push(
        batch_dwords,
        &mut cursor,
        draw.native.map_or_else(
            || if mesa_host_fixed_function { 2 } else { 0 },
            |native| native.vf_sgvs_2_dw2,
        ),
    )?;
    if let Some(native) = draw.native {
        for instancing in native.vf_instancing {
            log_batch_offset(cursor, "3DSTATE_VF_INSTANCING artifact-native");
            push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_INSTANCING)?;
            push(
                batch_dwords,
                &mut cursor,
                u32::from(instancing.element_index) | (u32::from(instancing.enabled) << 8),
            )?;
            push(batch_dwords, &mut cursor, instancing.step_rate)?;
        }
    } else if mesa_simple_rect_stack && vf_synthesized_vue {
        for element_index in 0..2 {
            log_batch_offset(cursor, "3DSTATE_VF_INSTANCING mesa-simple");
            push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_INSTANCING)?;
            push(batch_dwords, &mut cursor, element_index)?;
            push(batch_dwords, &mut cursor, 0)?;
        }
    } else {
        log_batch_offset(cursor, "3DSTATE_VF_INSTANCING");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_INSTANCING)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_VF_TOPOLOGY");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_TOPOLOGY)?;
    push(batch_dwords, &mut cursor, batch_mode.topology())?;
    log_batch_offset(cursor, "MI_STORE_DATA_IMM packet-marker after-VF-state");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_VF_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_VF,
    )?;

    let baked_vs_urb_output_length = pipeline.vs.meta.urb_entry_output_length;
    let programmed_vs_urb_output_length = front_end_contract
        .vs_urb_output_length_override
        .or(TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE)
        .unwrap_or(baked_vs_urb_output_length);

    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_VS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_VS)?;
    push(
        batch_dwords,
        &mut cursor,
        // Gfx12 encodes URB allocation size as "size in 64B units minus 1".
        // A position-only VUE is one 64B slot, so the programmed value must
        // be 0 rather than 1 or clipper sees the wrong VS allocation contract.
        (programmed_vs_urb_output_length.saturating_sub(1) as u32)
            | (TRIANGLE_VS_URB_START << 10)
            | (TRIANGLE_VS_URB_START << 21),
    )?;
    push(batch_dwords, &mut cursor, TRIANGLE_VS_URB_ENTRIES | (TRIANGLE_VS_URB_ENTRIES << 16))?;

    // Match Mesa's gfx12 allocation sequence exactly.  Disabled stages still
    // receive the first valid URB address; zero is outside this configuration's
    // push-constant reservation and must not be used as an inherited default.
    let disabled_urb_dw1 = (TRIANGLE_VS_URB_START << 10) | (TRIANGLE_VS_URB_START << 21);
    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_HS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_HS)?;
    push(batch_dwords, &mut cursor, disabled_urb_dw1)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_DS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_DS)?;
    push(batch_dwords, &mut cursor, disabled_urb_dw1)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_GS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_GS)?;
    push(batch_dwords, &mut cursor, disabled_urb_dw1)?;
    push(batch_dwords, &mut cursor, 0)?;
    intel_render_verbose_log!(
        "probe-urb-config order=vs-hs-ds-gs vs_start={} vs_entries={} vs_entry_64b={} disabled_stage_start={} sf_deref={} source=mesa-adl-gt1\n",
        TRIANGLE_VS_URB_START,
        TRIANGLE_VS_URB_ENTRIES,
        programmed_vs_urb_output_length,
        TRIANGLE_VS_URB_START,
        (sf_dw2 >> 29) & 0x3,
    );

    if mesa_host_fixed_function || artifact_native_fixed_function {
        log_batch_offset(cursor, "3DSTATE_VF_COMPONENT_PACKING");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_COMPONENT_PACKING)?;
        let packing = draw
            .native
            .map_or([0x0000_0007, 0, 0, 0], |native| native.vf_component_packing);
        for dword in packing {
            push(batch_dwords, &mut cursor, dword)?;
        }
        intel_render_verbose_log!(
            "probe-vf-component-packing enabled=1 masks=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] source={}\n",
            packing[0],
            packing[1],
            packing[2],
            packing[3],
            if artifact_native_fixed_function {
                "artifact-native"
            } else {
                "verified-host-vs"
            },
        );
    }

    if vf_synthesized_vue && !force_vs_with_vf_synthesized_vue {
        log_batch_offset(cursor, "3DSTATE_VS disabled");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VS)?;
        for _ in 0..8 {
            push(batch_dwords, &mut cursor, 0)?;
        }
    } else {
        let vs_dw3 = ((pipeline.vs.meta.kernel.binding_table_entry_count as u32) << 18)
            | (sampler_count_encoding(pipeline.vs.meta.kernel.sampler_count) << 27);
        let applied_vs_grf_start =
            triangle_vs_dispatch_grf_start_register(pipeline.vs.meta.kernel.grf_start_register);
        let vs_dw6 = (1 << 11) | (applied_vs_grf_start << 20);
        let vs_dw7 = 1
            | (1 << 2)
            | (1 << 10)
            | (triangle_vs_max_threads_field(warm.device_id, pipeline.vs.meta.max_threads) << 22);
        // Mesa's gfx12 VS packet leaves VertexURBEntryOutputLength at zero.
        // The 64-byte entry size is programmed independently through
        // 3DSTATE_URB_ALLOC_VS; mirroring that allocation size here changes a
        // separate fixed-function output-read contract.
        let vs_state_urb_output_length = if device_is_gfx12(warm.device_id) {
            0
        } else {
            programmed_vs_urb_output_length
        };
        let vs_dw8 = (vs_state_urb_output_length as u32) << 16;
        log_batch_offset(cursor, "3DSTATE_VS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VS)?;
        push(batch_dwords, &mut cursor, vs_ksp_offset & !0x3F)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, vs_dw3)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, vs_dw6)?;
        push(batch_dwords, &mut cursor, vs_dw7)?;
        push(batch_dwords, &mut cursor, vs_dw8)?;
        intel_render_verbose_log!(
            "probe-vs ksp=0x{:08X} dw3=0x{:08X} dw6=0x{:08X} dw7=0x{:08X} dw8=0x{:08X} baked_max_threads={} applied_max_threads_field={} urb_alloc_64b={} vs_state_urb_out_len={} baked_grf_start={} applied_grf_start={} dispatch={:?}\n",
            vs_ksp_offset & !0x3F,
            vs_dw3,
            vs_dw6,
            vs_dw7,
            vs_dw8,
            pipeline.vs.meta.max_threads,
            triangle_vs_max_threads_field(warm.device_id, pipeline.vs.meta.max_threads),
            programmed_vs_urb_output_length,
            vs_state_urb_output_length,
            pipeline.vs.meta.kernel.grf_start_register,
            applied_vs_grf_start,
            pipeline.vs.meta.kernel.dispatch_mode,
        );
        intel_render_verbose_log!(
            "probe-vs-export note={} position_only={} generic_attrs=0 baked_urb_bytes={} programmed_urb_bytes={} expected_vue=header+position-only\n",
            crate::intel::shader::triangle_pipeline_note(),
            (pipeline.ps.meta.num_varying_inputs == 0) as u8,
            (baked_vs_urb_output_length as u32) * 64,
            (programmed_vs_urb_output_length as u32) * 64,
        );
    }
    log_batch_offset(cursor, "MI_STORE_DATA_IMM packet-marker after-VS-state");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_VS_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_VS,
    )?;

    log_batch_offset(cursor, "3DSTATE_HS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_HS)?;
    for _ in 0..8 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_TE");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_TE)?;
    for _ in 0..4 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_DS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_DS)?;
    for _ in 0..10 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_STREAMOUT");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_STREAMOUT)?;
    if batch_mode.streamout_enabled() {
        push(batch_dwords, &mut cursor, streamout_dw1)?;
        push(batch_dwords, &mut cursor, streamout_dw2)?;
        push(batch_dwords, &mut cursor, streamout_dw3)?;
        push(batch_dwords, &mut cursor, streamout_dw4)?;

        log_batch_offset(cursor, "PIPE_CONTROL pre-so-buffer");
        push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;
        log_batch_offset(cursor, "3DSTATE_SO_BUFFER_INDEX_0");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SO_BUFFER_INDEX_0)?;
        push(batch_dwords, &mut cursor, so_buffer_index_dw1)?;
        push_addr(batch_dwords, &mut cursor, GPU_VA_STREAMOUT_BASE)?;
        push(batch_dwords, &mut cursor, streamout_surface_size_dwords)?;
        push_addr(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, so_buffer_stream_offset_dw)?;
        log_batch_offset(cursor, "PIPE_CONTROL post-so-buffer");
        push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;

        log_batch_offset(cursor, "3DSTATE_SO_DECL_LIST");
        let streamout_decl_dword0 = streamout_experiment.so_decl_buffer_selects();
        let streamout_decl_dword1 = streamout_experiment.so_decl_num_entries();
        let [
            streamout_decl_dword2,
            streamout_decl_dword3,
            streamout_decl_dword4,
            streamout_decl_dword5,
        ] = streamout_experiment.so_decl_entry_dwords();
        push(batch_dwords, &mut cursor, streamout_experiment.so_decl_header())?;
        push(batch_dwords, &mut cursor, streamout_decl_dword0)?;
        push(batch_dwords, &mut cursor, streamout_decl_dword1)?;
        push(batch_dwords, &mut cursor, streamout_decl_dword2)?;
        push(batch_dwords, &mut cursor, streamout_decl_dword3)?;
        if matches!(
            streamout_experiment,
            StreamoutProofExperiment::PrmVueHeaderPositionSlots01
                | StreamoutProofExperiment::PrmVueHeaderPositionXywzSlots01
                | StreamoutProofExperiment::HeaderAndPositionSlots01
        ) {
            push(batch_dwords, &mut cursor, streamout_decl_dword4)?;
            push(batch_dwords, &mut cursor, streamout_decl_dword5)?;
        }
        crate::log!(
            "probe-streamout-decl experiment={} read_len={} so_pitch={} decl=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] vs_position_only={} ps_varyings={} generic_attrs=0 compatible={}\n",
            streamout_experiment.label(),
            streamout_experiment.vertex_read_length(),
            streamout_experiment.vertex_bytes(),
            streamout_decl_dword0,
            streamout_decl_dword1,
            streamout_decl_dword2,
            streamout_decl_dword3,
            streamout_decl_dword4,
            streamout_decl_dword5,
            (pipeline.ps.meta.num_varying_inputs == 0) as u8,
            pipeline.ps.meta.num_varying_inputs,
            streamout_experiment.compatible() as u8,
        );
        crate::log!(
            "probe-streamout-config experiment={} so[function_enable={} statistics_enable={} rendering_disable={} render_stream={} reorder={} read_offset={} read_length_field={} buffer0_pitch={}] sobuf0[enable={} write_enable={} offset_addr_enable={} offset_mode={} mocs=0x{:X} surface=0x{:X} size_dwords=0x{:X} stream_offset=0x{:08X}] slot_contract={}\n",
            streamout_experiment.label(),
            (streamout_dw1 >> 31) & 0x1,
            (streamout_dw1 >> 25) & 0x1,
            (streamout_dw1 >> 30) & 0x1,
            (streamout_dw1 >> 27) & 0x3,
            (streamout_dw1 >> 26) & 0x1,
            (streamout_dw2 >> 5) & 0x1,
            streamout_dw2 & 0x1F,
            streamout_dw3 & 0xFFF,
            (so_buffer_index_dw1 >> 31) & 0x1,
            (so_buffer_index_dw1 >> 21) & 0x1,
            (so_buffer_index_dw1 >> 20) & 0x1,
            decode_streamout_offset_mode_name(
                (so_buffer_index_dw1 >> 21) & 0x1,
                (so_buffer_index_dw1 >> 20) & 0x1,
            ),
            (so_buffer_index_dw1 >> 22) & 0x7F,
            GPU_VA_STREAMOUT_BASE,
            streamout_surface_size_dwords,
            so_buffer_stream_offset_dw,
            streamout_experiment.vf_slot_contract(),
        );
        log_batch_offset(cursor, "PIPE_CONTROL post-so-decl");
        push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;
    } else {
        for _ in 0..4 {
            push(batch_dwords, &mut cursor, 0)?;
        }
    }
    log_batch_offset(cursor, "3DSTATE_GS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_GS)?;
    for _ in 0..9 {
        push(batch_dwords, &mut cursor, 0)?;
    }

    if matches!(backend_probe_mode, BackendProbeMode::PsCpsDisabled) {
        log_batch_offset(cursor, "PIPE_CONTROL pre-cps-pointers");
        push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;
        log_batch_offset(cursor, "3DSTATE_CPS_POINTERS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_CPS_POINTERS)?;
        push(batch_dwords, &mut cursor, probe_state.cps_state_offset_bytes & !0x1F)?;
        intel_render_focus_log!(
            "probe-cps-disabled backend={} cps_ptr=0x{:X} cps_gpu=0x{:X} state_dwords={} mode=none source=mesa-gen12-cps-pointers does_not_prove=ps_thread_launch\n",
            backend_probe_mode.label(),
            probe_state.cps_state_offset_bytes & !0x1F,
            draw.state_gpu_addr + (probe_state.cps_state_offset_bytes as u64 & !0x1F),
            CPS_STATE_DWORDS,
        );
    }

    // Bind a real tiled D32 surface only for the Resident-scene visibility contract.
    // Every other consumer retains the explicit null state proven during the
    // render bring-up. SurfacePitch, Width and Height are encoded minus one;
    // gfx12.5 replaces implicit Y0 with explicit Tile4 (encoding 3).
    let (
        depth_buffer_dw1,
        depth_buffer_addr,
        depth_buffer_dw4,
        depth_buffer_dw5,
        depth_buffer_dw6,
        depth_buffer_dw7,
    ) = if let Some(depth) = depth_config {
        (
            (depth.pitch_bytes - 1)
                | (DEPTH_SURFACE_FORMAT_D32_FLOAT << 24)
                | (1 << 28)
                | (SURFTYPE_2D << 29),
            depth.gpu_addr,
            ((depth.width - 1) << 1) | ((depth.height - 1) << 17),
            RENDER_MOCS,
            if device_is_gfx125(warm.device_id) {
                if resident_msaa4 { 1 << 30 } else { 3 << 30 }
            } else {
                0
            },
            depth.qpitch_rows_div4,
        )
    } else {
        ((DEPTH_SURFACE_FORMAT_D32_FLOAT << 24) | (SURFTYPE_NULL << 29), 0, 0, RENDER_MOCS, 0, 0)
    };
    log_batch_offset(cursor, "3DSTATE_CLEAR_PARAMS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_CLEAR_PARAMS)?;
    push(
        batch_dwords,
        &mut cursor,
        if depth_config.is_some() {
            1.0f32.to_bits()
        } else {
            0.0f32.to_bits()
        },
    )?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "3DSTATE_DEPTH_BUFFER");
    let depth_buffer_cmd = if device_is_gfx125(warm.device_id) {
        CMD_3DSTATE_DEPTH_BUFFER_GFX125
    } else {
        CMD_3DSTATE_DEPTH_BUFFER_GEN12
    };
    push(batch_dwords, &mut cursor, depth_buffer_cmd)?;
    push(batch_dwords, &mut cursor, depth_buffer_dw1)?;
    push_addr(batch_dwords, &mut cursor, depth_buffer_addr)?;
    push(batch_dwords, &mut cursor, depth_buffer_dw4)?;
    push(batch_dwords, &mut cursor, depth_buffer_dw5)?;
    push(batch_dwords, &mut cursor, depth_buffer_dw6)?;
    push(batch_dwords, &mut cursor, depth_buffer_dw7)?;
    if device_is_gfx125(warm.device_id) {
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
    }

    log_batch_offset(cursor, "3DSTATE_STENCIL_BUFFER");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_STENCIL_BUFFER)?;
    push(batch_dwords, &mut cursor, SURFTYPE_NULL << 29)?;
    push_addr(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, RENDER_MOCS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "3DSTATE_HIER_DEPTH_BUFFER");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_HIER_DEPTH_BUFFER)?;
    push(batch_dwords, &mut cursor, RENDER_MOCS << 25)?;
    push_addr(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    if backend_probe_mode.sample_mask_before_clip() {
        log_batch_offset(cursor, "3DSTATE_MULTISAMPLE early-raster-gate");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_MULTISAMPLE)?;
        push(batch_dwords, &mut cursor, multisample_dw1)?;
        log_batch_offset(cursor, "3DSTATE_SAMPLE_MASK early-raster-gate");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLE_MASK)?;
        push(batch_dwords, &mut cursor, sample_mask_dw)?;
    }

    if backend_probe_mode.draw_rect_before_clip() {
        log_batch_offset(cursor, "3DSTATE_DRAWING_RECTANGLE early-raster-gate");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_DRAWING_RECTANGLE)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(
            batch_dwords,
            &mut cursor,
            draw.target_w.saturating_sub(1) | (draw.target_h.saturating_sub(1) << 16),
        )?;
        push(batch_dwords, &mut cursor, 0)?;
    }

    if backend_probe_mode.sample_mask_before_clip() || backend_probe_mode.draw_rect_before_clip() {
        intel_render_focus_log!(
            "probe-early-raster-gate backend={} sample_mask_early={} drawing_rect_early={} drawing_rect=[0,0..{},{}] order=before-clip-sf-raster-wm does_not_prove=raster_samples_or_ps\n",
            backend_probe_mode.label(),
            backend_probe_mode.sample_mask_before_clip() as u8,
            backend_probe_mode.draw_rect_before_clip() as u8,
            draw.target_w.saturating_sub(1),
            draw.target_h.saturating_sub(1),
        );
    }

    if backend_probe_mode.sbe_before_clip() {
        log_batch_offset(cursor, "3DSTATE_SBE pre-clip");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE)?;
        push(batch_dwords, &mut cursor, sbe_dw1)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, pipeline.ps.meta.flat_inputs)?;
        push(batch_dwords, &mut cursor, SBE_ACTIVE_COMPONENT_XYZW_MASK_DWORD)?;
        push(batch_dwords, &mut cursor, SBE_ACTIVE_COMPONENT_XYZW_MASK_DWORD)?;

        log_batch_offset(cursor, "3DSTATE_SBE_SWIZ pre-clip");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE_SWIZ)?;
        push(batch_dwords, &mut cursor, sbe_swiz[0])?;
        push(batch_dwords, &mut cursor, sbe_swiz[1])?;
        for _ in 2..10 {
            push(batch_dwords, &mut cursor, 0)?;
        }
        intel_render_focus_log!(
            "probe-sbe-order backend={} order=sbe-swiz-before-clip does_not_prove=raster_samples_or_ps\n",
            backend_probe_mode.label(),
        );
    }

    log_batch_offset(cursor, "3DSTATE_CLIP");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_CLIP)?;
    push(batch_dwords, &mut cursor, clip_dw1)?;
    push(batch_dwords, &mut cursor, clip_dw2)?;
    push(batch_dwords, &mut cursor, clip_dw3)?;
    log_batch_offset(cursor, "MI_STORE_DATA_IMM packet-marker after-CLIP-state");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_CLIP_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_CLIP,
    )?;

    if backend_probe_mode.pipe_control_between_clip_sf() {
        log_batch_offset(cursor, "PIPE_CONTROL clip-to-sf-cs-stall");
        push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;
        intel_render_focus_log!(
            "probe-clip-sf-sync backend={} flags=cs-stall order=after-clip-before-sf does_not_prove=raster_samples_or_ps\n",
            backend_probe_mode.label(),
        );
    }

    if backend_probe_mode.sbe_before_sf() {
        log_batch_offset(cursor, "3DSTATE_SBE pre-sf");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE)?;
        push(batch_dwords, &mut cursor, sbe_dw1)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, pipeline.ps.meta.flat_inputs)?;
        push(batch_dwords, &mut cursor, SBE_ACTIVE_COMPONENT_XYZW_MASK_DWORD)?;
        push(batch_dwords, &mut cursor, SBE_ACTIVE_COMPONENT_XYZW_MASK_DWORD)?;

        log_batch_offset(cursor, "3DSTATE_SBE_SWIZ pre-sf");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE_SWIZ)?;
        push(batch_dwords, &mut cursor, sbe_swiz[0])?;
        push(batch_dwords, &mut cursor, sbe_swiz[1])?;
        for _ in 2..10 {
            push(batch_dwords, &mut cursor, 0)?;
        }
        intel_render_focus_log!(
            "probe-sbe-order backend={} order=sbe-swiz-before-sf does_not_prove=raster_samples_or_ps\n",
            backend_probe_mode.label(),
        );
    }

    log_batch_offset(cursor, "3DSTATE_SF");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SF)?;
    push(batch_dwords, &mut cursor, sf_dw1)?;
    push(batch_dwords, &mut cursor, sf_dw2)?;
    push(batch_dwords, &mut cursor, sf_dw3)?;

    log_batch_offset(cursor, "3DSTATE_RASTER");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_RASTER)?;
    push(batch_dwords, &mut cursor, raster_dw1)?;
    push(batch_dwords, &mut cursor, raster_dw2)?;
    push(batch_dwords, &mut cursor, raster_dw3)?;
    push(batch_dwords, &mut cursor, raster_dw4)?;
    log_batch_offset(cursor, "3DSTATE_PRIMITIVE_REPLICATION");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_PRIMITIVE_REPLICATION)?;
    push(batch_dwords, &mut cursor, primitive_replication_dw1)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "MI_STORE_DATA_IMM packet-marker after-RASTER-state");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_RASTER_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_RASTER,
    )?;

    if !(backend_probe_mode.sbe_before_clip() || backend_probe_mode.sbe_before_sf()) {
        log_batch_offset(cursor, "3DSTATE_SBE");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE)?;
        push(batch_dwords, &mut cursor, sbe_dw1)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, pipeline.ps.meta.flat_inputs)?;
        push(batch_dwords, &mut cursor, SBE_ACTIVE_COMPONENT_XYZW_MASK_DWORD)?;
        push(batch_dwords, &mut cursor, SBE_ACTIVE_COMPONENT_XYZW_MASK_DWORD)?;

        // Gen12/Xe-LP keeps attribute swizzle state in a separate packet.
        log_batch_offset(cursor, "3DSTATE_SBE_SWIZ");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE_SWIZ)?;
        push(batch_dwords, &mut cursor, sbe_swiz[0])?;
        push(batch_dwords, &mut cursor, sbe_swiz[1])?;
        for _ in 2..10 {
            push(batch_dwords, &mut cursor, 0)?;
        }
    }

    if backend_probe_mode.wm_hz_op_before_wm() {
        log_batch_offset(cursor, "3DSTATE_WM_HZ_OP before-wm");
        push_wm_hz_op(
            batch_dwords,
            &mut cursor,
            warm.device_id,
            wm_hz_op_dw1,
            wm_hz_op_dw2,
            wm_hz_op_dw3,
            wm_hz_op_dw4,
        )?;
        intel_render_focus_log!(
            "probe-wm-hz-op-order backend={} order=before-wm does_not_prove=raster_samples_or_ps\n",
            backend_probe_mode.label(),
        );
    }

    log_batch_offset(cursor, "3DSTATE_WM");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_WM)?;
    push(batch_dwords, &mut cursor, wm_dw1)?;

    log_batch_offset(cursor, "3DSTATE_WM_DEPTH_STENCIL");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_WM_DEPTH_STENCIL)?;
    push(batch_dwords, &mut cursor, wm_depth_stencil_dw1)?;
    push(batch_dwords, &mut cursor, wm_depth_stencil_dw2)?;
    push(batch_dwords, &mut cursor, wm_depth_stencil_dw3)?;

    log_batch_offset(cursor, "3DSTATE_WM_CHROMA_KEY");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_WM_CHROMA_KEY)?;
    push(batch_dwords, &mut cursor, wm_chroma_key_dw1)?;

    // Match Mesa's gfx12 trivial path and avoid relying on inherited depth
    // bounds state from earlier firmware or display bring-up.
    log_batch_offset(cursor, "3DSTATE_DEPTH_BOUNDS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_DEPTH_BOUNDS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0.0f32.to_bits())?;
    push(batch_dwords, &mut cursor, 1.0f32.to_bits())?;

    log_batch_offset(cursor, "3DSTATE_CC_STATE_POINTERS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_CC_STATE_POINTERS)?;
    push(
        batch_dwords,
        &mut cursor,
        if backend_probe_mode.zero_cc_state_pointer() {
            0
        } else {
            probe_state.color_calc_state_offset_bytes | 1
        },
    )?;

    log_batch_offset(cursor, "3DSTATE_BLEND_STATE_POINTERS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_BLEND_STATE_POINTERS)?;
    push(
        batch_dwords,
        &mut cursor,
        blend_mode.blend_state_pointer_dword(probe_state.blend_state_offset_bytes),
    )?;

    log_batch_offset(cursor, "3DSTATE_PS_BLEND");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_PS_BLEND)?;
    push(batch_dwords, &mut cursor, ps_blend_dw1)?;

    if !backend_probe_mode.sample_mask_before_clip() {
        log_batch_offset(cursor, "3DSTATE_MULTISAMPLE");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_MULTISAMPLE)?;
        push(batch_dwords, &mut cursor, multisample_dw1)?;
        log_batch_offset(cursor, "3DSTATE_SAMPLE_MASK");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLE_MASK)?;
        push(batch_dwords, &mut cursor, sample_mask_dw)?;
    }

    if !backend_probe_mode.draw_rect_before_clip() {
        log_batch_offset(cursor, "3DSTATE_DRAWING_RECTANGLE");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_DRAWING_RECTANGLE)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(
            batch_dwords,
            &mut cursor,
            draw.target_w.saturating_sub(1) | (draw.target_h.saturating_sub(1) << 16),
        )?;
        push(batch_dwords, &mut cursor, 0)?;
    }

    // Clear inherited WM_HZ_OP clear/resolve overrides so PS dispatch only
    // depends on the explicit probe state we log below.
    if !mesa_simple_rect_stack
        && !backend_probe_mode.wm_hz_op_before_wm()
        && !backend_probe_mode.wm_hz_op_after_ps_extra()
        && !backend_probe_mode.omit_wm_hz_op()
    {
        log_batch_offset(cursor, "3DSTATE_WM_HZ_OP");
        push_wm_hz_op(
            batch_dwords,
            &mut cursor,
            warm.device_id,
            wm_hz_op_dw1,
            wm_hz_op_dw2,
            wm_hz_op_dw3,
            wm_hz_op_dw4,
        )?;
    }

    let ps_extra_before_ps = backend_probe_mode.ps_extra_before_ps();
    if ps_extra_before_ps {
        log_batch_offset(cursor, "3DSTATE_PS_EXTRA before-ps");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PS_EXTRA)?;
        push(batch_dwords, &mut cursor, ps_extra_dw1)?;
        intel_render_focus_log!(
            "probe-ps-extra-order backend={} order=before-ps does_not_prove=ps_thread_launch\n",
            backend_probe_mode.label(),
        );
    }

    if backend_probe_mode.disable_ps_contract() {
        log_batch_offset(cursor, "3DSTATE_PS disabled");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PS)?;
        for _ in 0..11 {
            push(batch_dwords, &mut cursor, 0)?;
        }
    } else {
        log_batch_offset(cursor, "3DSTATE_PS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PS)?;
        push(batch_dwords, &mut cursor, ps_ksp0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, ps_dw3)?;
        push(batch_dwords, &mut cursor, ps_scratch_space_buffer)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, ps_dw6)?;
        push(batch_dwords, &mut cursor, ps_dw7)?;
        push(batch_dwords, &mut cursor, ps_ksp1)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, ps_ksp2)?;
        push(batch_dwords, &mut cursor, 0)?;
    }

    if !ps_extra_before_ps {
        log_batch_offset(cursor, "3DSTATE_PS_EXTRA");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PS_EXTRA)?;
        push(batch_dwords, &mut cursor, ps_extra_dw1)?;
    }
    if backend_probe_mode.reemit_wm_after_ps_extra() {
        log_batch_offset(cursor, "3DSTATE_WM after-ps-extra-reemit");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_WM)?;
        push(batch_dwords, &mut cursor, wm_dw1)?;
        intel_render_focus_log!(
            "probe-wm-reemit backend={} order=after-ps-extra does_not_prove=raster_samples_or_ps\n",
            backend_probe_mode.label(),
        );
    }
    if backend_probe_mode.wm_hz_op_after_ps_extra() {
        log_batch_offset(cursor, "3DSTATE_WM_HZ_OP after-ps-extra");
        push_wm_hz_op(
            batch_dwords,
            &mut cursor,
            warm.device_id,
            wm_hz_op_dw1,
            wm_hz_op_dw2,
            wm_hz_op_dw3,
            wm_hz_op_dw4,
        )?;
        intel_render_focus_log!(
            "probe-wm-hz-op-order backend={} order=after-ps-extra does_not_prove=raster_samples_or_ps\n",
            backend_probe_mode.label(),
        );
    }
    log_batch_offset(cursor, "MI_STORE_DATA_IMM packet-marker after-PS-state");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_PS_STATE_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_PS_STATE,
    )?;

    log_batch_offset(cursor, "MI_STORE_DATA_IMM pre-3d");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_PRE3D_DWORD as u64) * 4,
        pre3d_value,
    )?;

    if depth_config.is_some() && !matches!(backend_probe_mode, BackendProbeMode::WmLateReemit) {
        log_batch_offset(cursor, "PIPE_CONTROL resident-scene-depth-pre-draw");
        push_pipe_control_full(
            batch_dwords,
            &mut cursor,
            PIPE_CONTROL_BIG_PRE_DRAW_HEADER_BITS,
            PIPE_CONTROL_BIG_PRE_DRAW_BITS,
        )?;
    }

    if matches!(backend_probe_mode, BackendProbeMode::WmLateReemit) {
        log_batch_offset(cursor, "PIPE_CONTROL big-pre-draw-flush");
        push_pipe_control_full(
            batch_dwords,
            &mut cursor,
            PIPE_CONTROL_BIG_PRE_DRAW_HEADER_BITS,
            PIPE_CONTROL_BIG_PRE_DRAW_BITS,
        )?;

        log_batch_offset(cursor, "3DSTATE_SBE_SWIZ late-reemit");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE_SWIZ)?;
        push(batch_dwords, &mut cursor, sbe_swiz[0])?;
        push(batch_dwords, &mut cursor, sbe_swiz[1])?;
        for _ in 2..10 {
            push(batch_dwords, &mut cursor, 0)?;
        }

        log_batch_offset(cursor, "3DSTATE_WM late-reemit");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_WM)?;
        push(batch_dwords, &mut cursor, wm_dw1)?;

        log_batch_offset(cursor, "3DSTATE_WM_HZ_OP late-reemit");
        push_wm_hz_op(
            batch_dwords,
            &mut cursor,
            warm.device_id,
            wm_hz_op_dw1,
            wm_hz_op_dw2,
            wm_hz_op_dw3,
            wm_hz_op_dw4,
        )?;

        log_batch_offset(cursor, "3DSTATE_PS late-reemit");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PS)?;
        push(batch_dwords, &mut cursor, ps_ksp0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, ps_dw3)?;
        push(batch_dwords, &mut cursor, ps_scratch_space_buffer)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, ps_dw6)?;
        push(batch_dwords, &mut cursor, ps_dw7)?;
        push(batch_dwords, &mut cursor, ps_ksp1)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, ps_ksp2)?;
        push(batch_dwords, &mut cursor, 0)?;

        log_batch_offset(cursor, "3DSTATE_PS_EXTRA late-reemit");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PS_EXTRA)?;
        push(batch_dwords, &mut cursor, ps_extra_dw1)?;

        log_batch_offset(cursor, "3DSTATE_CPS_POINTERS late-null");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_CPS_POINTERS)?;
        push(batch_dwords, &mut cursor, 0)?;

        intel_render_focus_log!(
            "probe-late-reemit backend={} pc_header=0x{:08X} pc_dw1=0x{:08X} wm_hz_sample_mask=0x{:X} cps_ptr=null does_not_prove=ps_thread_launch\n",
            backend_probe_mode.label(),
            PIPE_CONTROL_BIG_PRE_DRAW_HEADER_BITS,
            PIPE_CONTROL_BIG_PRE_DRAW_BITS,
            wm_hz_op_dw4 & 0xFFFF,
        );
    }

    let artificial_marker_x = draw.target_w / 2;
    let artificial_marker_y = draw.target_h / 2;
    let artificial_marker_pre_offset = artificial_marker_y
        .checked_mul(draw.rt_pitch)
        .and_then(|row| row.checked_add(artificial_marker_x.saturating_mul(4)))
        .ok_or("probe-artificial-marker-offset")?;
    let artificial_marker_post_offset = artificial_marker_pre_offset
        .checked_add(if artificial_marker_x + 1 < draw.target_w {
            4
        } else {
            0
        })
        .ok_or("probe-artificial-marker-post-offset")?;
    if backend_probe_mode.artificial_fragment_markers() {
        log_batch_offset(cursor, "MI_STORE_DATA_IMM artificial-fragment-pre");
        push_store_data_imm(
            batch_dwords,
            &mut cursor,
            draw.rt_gpu_addr + artificial_marker_pre_offset as u64,
            RCS_ARTIFICIAL_FRAGMENT_PRE_COLOR,
        )?;
        intel_render_focus_log!(
            "probe-artificial-fragment-arm backend={} pre_gpu=0x{:X} post_gpu=0x{:X} pre_color=0x{:08X} post_color=0x{:08X} center=[{},{}] meaning=artificial-fragment-not-wm\n",
            backend_probe_mode.label(),
            draw.rt_gpu_addr + artificial_marker_pre_offset as u64,
            draw.rt_gpu_addr + artificial_marker_post_offset as u64,
            RCS_ARTIFICIAL_FRAGMENT_PRE_COLOR,
            RCS_ARTIFICIAL_FRAGMENT_POST_COLOR,
            artificial_marker_x,
            artificial_marker_y,
        );
    }

    if backend_probe_mode.uses_raster_wm_oa() {
        log_batch_offset(cursor, "OA raster-wm enable");
        push_raster_wm_oa_config(batch_dwords, &mut cursor, true)?;
        log_batch_offset(cursor, "MI_REPORT_PERF_COUNT raster-wm begin");
        push_mi_report_perf_count(
            batch_dwords,
            &mut cursor,
            result_gpu_addr + (RESULT_OA_BEGIN_DWORD as u64) * 4,
            RESULT_OA_RASTER_WM_BEGIN_ID,
        )?;
    }

    if surface_base_relative_binding_table && !artifact_native_fixed_function {
        // The verified gfx12 Mesa draw re-arms this state as one stalled tail
        // immediately before 3DPRIMITIVE.  Keep the order and payloads exact:
        // this is the final SVL/URB admission block, not a second draw.
        log_batch_offset(cursor, "PIPE_CONTROL verified-host-pre-draw-tail");
        push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;

        log_batch_offset(cursor, "3DSTATE_VF verified-host-tail");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VF | (1 << 9))?;
        push(batch_dwords, &mut cursor, 0xFFFF)?;

        log_batch_offset(cursor, "3DSTATE_PRIMITIVE_REPLICATION verified-host-tail");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PRIMITIVE_REPLICATION)?;
        push(batch_dwords, &mut cursor, 0x0001_0000)?;
        for _ in 0..4 {
            push(batch_dwords, &mut cursor, 0)?;
        }

        log_batch_offset(cursor, "3DSTATE_WM verified-host-tail");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_WM)?;
        // This late verified-host packet supersedes the earlier WM state.
        // Preserve the shader-derived barycentric mode instead of silently
        // reverting VF-fed sampled shaders to a no-varying payload.
        push(batch_dwords, &mut cursor, 0x8000_0040 | wm_barycentric_mode)?;
        log_batch_offset(cursor, "3DSTATE_PS_BLEND verified-host-tail");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PS_BLEND)?;
        push(batch_dwords, &mut cursor, 0x518C_6200)?;
        log_batch_offset(cursor, "3DSTATE_BLEND_STATE_POINTERS verified-host-tail");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_BLEND_STATE_POINTERS)?;
        push(batch_dwords, &mut cursor, probe_state.blend_state_offset_bytes | 1)?;

        for (command, value) in [
            (CMD_3DSTATE_PUSH_CONSTANT_ALLOC_VS, 16),
            (CMD_3DSTATE_PUSH_CONSTANT_ALLOC_HS, 0),
            (CMD_3DSTATE_PUSH_CONSTANT_ALLOC_DS, 0),
            (CMD_3DSTATE_PUSH_CONSTANT_ALLOC_GS, 0),
            (CMD_3DSTATE_PUSH_CONSTANT_ALLOC_PS, (16 << 16) | 16),
        ] {
            push(batch_dwords, &mut cursor, command)?;
            push(batch_dwords, &mut cursor, value)?;
        }
        push(batch_dwords, &mut cursor, CMD_3DSTATE_CONSTANT_ALL_EMPTY_VS_PS)?;
        push(batch_dwords, &mut cursor, RENDER_MOCS)?;
        push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_VS)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_PS)?;
        push(batch_dwords, &mut cursor, ps_binding_table_pointer_offset)?;
    }

    if surface_base_relative_binding_table && artifact_native_fixed_function {
        // On SKL+ the binding-table pointer packets only take effect after the
        // corresponding push-constant/state sequence.  The artifact-native
        // path emitted valid pointers during initial state setup, but the
        // later PUSH_CONSTANT_ALLOC/CONSTANT_ALL and shader-stage packets made
        // those early values stale.  Re-arm both native stages at the final
        // draw frontier, after all 3D state and before the indirect loads.
        log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POINTERS_VS native-pre-draw");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_VS)?;
        push(batch_dwords, &mut cursor, binding_table_pointer_offset)?;
        log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POINTERS_PS native-pre-draw");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_PS)?;
        push(batch_dwords, &mut cursor, ps_binding_table_pointer_offset)?;
    }

    if let Some(args_gpu_addr) = draw.indirect_args_gpu_addr {
        log_batch_offset(cursor, "Helio DrawIndexedIndirectArgs -> 3DPRIM registers");
        encode_draw_indexed_indirect_register_loads(batch_dwords, &mut cursor, args_gpu_addr)?;
    }

    log_batch_offset(cursor, "3DPRIMITIVE");
    if device_is_gfx12(warm.device_id) {
        // Gfx11+ direct draws use the extended ten-DWORD form.  Topology is
        // supplied by 3DSTATE_VF_TOPOLOGY, while the three extended values
        // repeat firstVertex, firstInstance, and baseVertex.  The verified
        // host packet is 0x7B000808 followed by nine zero/default fields apart
        // from vertex and instance counts.
        push(
            batch_dwords,
            &mut cursor,
            CMD_3DPRIMITIVE_EXTENDED
                | if draw.indirect_args_gpu_addr.is_some() {
                    PRIMITIVE_INDIRECT_PARAMETER_ENABLE
                } else {
                    0
                },
        )?;
        push(
            batch_dwords,
            &mut cursor,
            if draw.index_buffer.is_some() {
                PRIMITIVE_VERTEX_ACCESS_RANDOM
            } else {
                0
            },
        )?;
        push(batch_dwords, &mut cursor, draw.vertex_count)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 1)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
    } else {
        push(
            batch_dwords,
            &mut cursor,
            CMD_3DPRIMITIVE
                | if draw.indirect_args_gpu_addr.is_some() {
                    PRIMITIVE_INDIRECT_PARAMETER_ENABLE
                } else {
                    0
                },
        )?;
        push(
            batch_dwords,
            &mut cursor,
            batch_mode.topology()
                | if draw.index_buffer.is_some() {
                    PRIMITIVE_VERTEX_ACCESS_RANDOM
                } else {
                    0
                },
        )?;
        push(batch_dwords, &mut cursor, draw.vertex_count)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 1)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
    }

    if backend_probe_mode.uses_raster_wm_oa() {
        log_batch_offset(cursor, "MI_REPORT_PERF_COUNT raster-wm end");
        push_mi_report_perf_count(
            batch_dwords,
            &mut cursor,
            result_gpu_addr + (RESULT_OA_END_DWORD as u64) * 4,
            RESULT_OA_RASTER_WM_END_ID,
        )?;
        log_batch_offset(cursor, "OA raster-wm disable");
        push_raster_wm_oa_config(batch_dwords, &mut cursor, false)?;
    }

    if backend_probe_mode.artificial_fragment_markers() {
        log_batch_offset(cursor, "MI_STORE_DATA_IMM artificial-fragment-post");
        push_store_data_imm(
            batch_dwords,
            &mut cursor,
            draw.rt_gpu_addr + artificial_marker_post_offset as u64,
            RCS_ARTIFICIAL_FRAGMENT_POST_COLOR,
        )?;
    }

    log_batch_offset(cursor, "MI_STORE_DATA_IMM pre-light-pipe-control");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_PRE_LIGHT_PC_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_PRE_LIGHT_PC,
    )?;

    if post_draw_sync_variant == PostDrawSyncVariant::HeavyAll {
        // Resident-scene reuses one color/depth target across separately scheduled
        // mesh contexts.  The Mesa-shaped completion packet below drains the
        // render-target cache, but it does not drain D32, Tile, HDC, or the L3
        // fabric.  Its marker can therefore become visible before all pixels
        // needed by the next mesh (or the present copy) are globally visible.
        //
        // Use the already-proven gfx12 full drain packet first, including its
        // required DW0 HDC/untyped-dataport bits.  The following post-sync PC
        // remains the sole completion fence, so observing its value proves
        // that this drain retired as well.
        log_batch_offset(cursor, "PIPE_CONTROL post-3d-full-cache-drain");
        push_pipe_control_full(
            batch_dwords,
            &mut cursor,
            PIPE_CONTROL_BIG_PRE_DRAW_HEADER_BITS,
            PIPE_CONTROL_BIG_PRE_DRAW_BITS,
        )?;
    }

    log_batch_offset(cursor, "PIPE_CONTROL post-3d-light-marker");
    let light_sync_flags = post_draw_sync_variant.light_sync_flags();
    if post_draw_sync_variant.light_post_sync_enabled() {
        push_pipe_control_post_sync_imm(
            batch_dwords,
            &mut cursor,
            0,
            light_sync_flags,
            result_gpu_addr + (RESULT_SLOT_POST3D_LIGHT_PIPE_CONTROL_LO_DWORD as u64) * 4,
            post3d_value,
        )?;
    } else {
        push_pipe_control(batch_dwords, &mut cursor, light_sync_flags)?;
    }

    log_batch_offset(cursor, "MI_STORE_DATA_IMM final-after-light");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_FINAL_AFTER_LIGHT_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_FINAL_AFTER_LIGHT,
    )?;

    if let Some(heavy_sync_flags) = post_draw_sync_variant.heavy_sync_flags() {
        let heavy_sync_flags = heavy_sync_flags
            | if depth_config.is_some() {
                PIPE_CONTROL_DEPTH_CACHE_FLUSH | PIPE_CONTROL_DEPTH_STALL
            } else {
                0
            };
        log_batch_offset(cursor, "PIPE_CONTROL post-3d-heavy-sync");
        push_pipe_control_post_sync_imm(
            batch_dwords,
            &mut cursor,
            0,
            heavy_sync_flags,
            result_gpu_addr + (RESULT_SLOT_POST3D_PIPE_CONTROL_LO_DWORD as u64) * 4,
            post3d_value,
        )?;
    }

    log_batch_offset(cursor, "MI_STORE_DATA_IMM final");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_FINAL_DWORD as u64) * 4,
        done_value,
    )?;
    log_batch_offset(cursor, "MI_BATCH_BUFFER_END");
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;

    intel_render_verbose_log!(
        "probe-3d mode={} topology={} backend={} ps_bt_count={} ps_ksp=[0x{:X},0x{:X},0x{:X}] ps_scratch=0x{:X} ps_dispatch_bits={}{}{} sbe=0x{:08X} clip=[0x{:08X},0x{:08X},0x{:08X}] sf=[0x{:08X},0x{:08X},0x{:08X}] raster=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] wm=0x{:08X} ps3=0x{:08X} ps6=0x{:08X} ps7=0x{:08X} ps_extra=0x{:08X}\n",
        batch_mode.label(),
        batch_mode.topology(),
        backend_probe_mode.label(),
        ps_binding_table_entry_count,
        ps_ksp0,
        ps_ksp1,
        ps_ksp2,
        ps_scratch_space_buffer,
        ps_dispatch_8,
        ps_dispatch_16,
        ps_dispatch_32,
        sbe_dw1,
        clip_dw1,
        clip_dw2,
        clip_dw3,
        sf_dw1,
        sf_dw2,
        sf_dw3,
        raster_dw1,
        raster_dw2,
        raster_dw3,
        raster_dw4,
        wm_dw1,
        ps_dw3,
        ps_dw6,
        ps_dw7,
        ps_extra_dw1
    );
    intel_render_verbose_log!(
        "probe-backend ps_blend=0x{:08X} wm_depth=[0x{:08X},0x{:08X},0x{:08X}] wm_hz_op=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
        ps_blend_dw1,
        wm_depth_stencil_dw1,
        wm_depth_stencil_dw2,
        wm_depth_stencil_dw3,
        wm_hz_op_dw1,
        wm_hz_op_dw2,
        wm_hz_op_dw3,
        wm_hz_op_dw4,
    );
    intel_render_verbose_log!(
        "probe-binding-table-pool base=0x{:X} base_dw=0x{:08X} size_dw=0x{:08X} mocs=0x{:X} enable={} ps_bt_ptr=0x{:X} bt_gpu=0x{:X} bt_entry0=0x{:08X} surf_gpu=0x{:X} contract={}\n",
        surface_state_base_gpu_addr,
        binding_table_pool_base_dw,
        binding_table_pool_size_dw,
        RENDER_MOCS & BINDING_TABLE_POOL_MOCS_MASK,
        binding_table_pool_enable,
        binding_table_pointer_offset,
        binding_table_gpu_addr,
        surface_state_pointer_offset,
        binding_table_entry0_gpu_addr,
        if surface_base_relative_binding_table {
            "surface-base-relative"
        } else {
            "pool-relative"
        },
    );
    log_mesa_spec_cross_compare(
        warm,
        pipeline,
        sbe_dw1,
        baked_vs_urb_output_length,
        programmed_vs_urb_output_length,
        clip_dw1,
        clip_dw2,
        sf_dw1,
        sf_dw2,
        raster_dw1,
        batch_mode.topology(),
        primitive_replication_dw1,
        ps_dw3,
        ps_dw6,
        ps_extra_dw1,
    );
    log_backend_dispatch_contract(
        wm_dw1,
        ps_blend_dw1,
        wm_depth_stencil_dw1,
        wm_depth_stencil_dw2,
        wm_depth_stencil_dw3,
        wm_hz_op_dw1,
        wm_hz_op_dw2,
        wm_hz_op_dw3,
        wm_hz_op_dw4,
        ps_extra_dw1,
    );
    let ps_valid = (ps_extra_dw1 >> 31) & 0x1;
    let ps_has_uav = (ps_extra_dw1 >> 2) & 0x1;
    let ps_computes_stencil = (ps_extra_dw1 >> 5) & 0x1;
    let ps_attribute_enable = (ps_extra_dw1 >> 8) & 0x1;
    let ps_computed_depth = (ps_extra_dw1 >> 26) & 0x3;
    let ps_kills = (ps_extra_dw1 >> 28) & 0x1;
    let ps_blend_has_writeable_rt = (ps_blend_dw1 >> 30) & 0x1;
    let wm_force_thread_dispatch = (wm_dw1 >> 19) & 0x3;
    let wm_hz_op_active = ((wm_hz_op_dw1 | wm_hz_op_dw2 | wm_hz_op_dw3 | wm_hz_op_dw4) != 0) as u32;
    let wm_depth_test_enable = (wm_depth_stencil_dw1 >> 1) & 0x1;
    let wm_stencil_test_enable = (wm_depth_stencil_dw1 >> 3) & 0x1;
    let wm_depth_write_enable = wm_depth_stencil_dw1 & 0x1;
    let dispatch_reason = if wm_force_thread_dispatch == 1 {
        "force-thread-dispatch-off"
    } else if ps_valid == 0 {
        "ps-invalid"
    } else if wm_force_thread_dispatch == 2 {
        "force-thread-dispatch-on"
    } else if wm_hz_op_active != 0 {
        "wm-hz-op-active"
    } else if ps_blend_has_writeable_rt != 0 {
        "writeable-rt"
    } else if ps_has_uav != 0 {
        "ps-uav"
    } else if ps_kills != 0 {
        "ps-kill"
    } else if ps_computed_depth != 0 && (wm_depth_test_enable != 0 || wm_depth_write_enable != 0) {
        "computed-depth"
    } else if ps_computes_stencil != 0 && wm_stencil_test_enable != 0 {
        "computed-stencil"
    } else {
        "no-ps-dispatch-qualifier"
    };
    let dispatch_armed = matches!(
        dispatch_reason,
        "force-thread-dispatch-on"
            | "writeable-rt"
            | "ps-uav"
            | "ps-kill"
            | "computed-depth"
            | "computed-stencil"
    ) as u32;
    let ps_bary_coeffs = ((ps_extra_dw1
        & (PS_EXTRA_REQUIRES_NONPERSPECTIVE_BARY_PLANE | PS_EXTRA_REQUIRES_PERSPECTIVE_BARY_PLANE))
        != 0) as u8;
    let ps_source_depth_w = ((ps_extra_dw1 & PS_EXTRA_REQUIRES_SOURCE_DEPTH_W_PLANE) != 0) as u8;
    let no_varying_payload = (pipeline.ps.meta.num_varying_inputs == 0
        && !ps_push_constant_enable
        && ps_attribute_enable == 0
        && ps_bary_coeffs == 0
        && ps_source_depth_w == 0) as u8;
    intel_render_focus_log!(
        "{} pre-ps-contract backend={} topo={} sbe[dw1=0x{:08X} read_offset={} read_len={} attrs={} force_off={} force_len={}] vue[vs_baked={} vs_prog={} vf_synth={} vf_synth_vs_on={}] raster[dw1=0x{:08X} msaa={} forced_ms={} forced_samples={} scissor={} sample_mask=0x{:X}] wm[dw1=0x{:08X} stats={} bary=0x{:X} force_dispatch={} hz_active={} hz_sample_mask=0x{:X}] ps[dw3=0x{:08X} dw6=0x{:08X} dw7=0x{:08X} extra=0x{:08X} valid={} bt_count={} dispatch_bits={}{}{} push={} attr={} cpdep={} bary={} src_depth_w={} grf_start={} max_threads={}] rt[writeable={} bt_ptr=0x{:X} surf=0x{:X}] launch_qualifier={} dispatch_armed={} no_varying_payload={} note=pre-ps-frontier\n",
        submit_name,
        backend_probe_mode.label(),
        primitive_topology_label(batch_mode.topology()),
        sbe_dw1,
        sbe_vertex_read_offset,
        sbe_vertex_read_length,
        sbe_num_sf_attrs,
        front_end_contract.force_sbe_read_offset as u8,
        front_end_contract.force_sbe_read_length as u8,
        baked_vs_urb_output_length,
        programmed_vs_urb_output_length,
        vf_synthesized_vue as u8,
        force_vs_with_vf_synthesized_vue as u8,
        raster_dw1,
        (raster_dw1 >> 14) & 0x1,
        (raster_dw1 >> 12) & 0x1,
        (raster_dw1 >> 18) & 0x7,
        (raster_dw1 >> 1) & 0x1,
        backend_probe_mode.sample_mask_dw(),
        wm_dw1,
        (wm_dw1 >> 31) & 0x1,
        (wm_dw1 >> 11) & 0x3F,
        wm_force_thread_dispatch,
        wm_hz_op_active,
        wm_hz_op_dw4 & 0xFFFF,
        ps_dw3,
        ps_dw6,
        ps_dw7,
        ps_extra_dw1,
        ps_valid,
        ps_binding_table_entry_count,
        ps_dispatch_8,
        ps_dispatch_16,
        ps_dispatch_32,
        ps_push_constant_enable as u8,
        ps_attribute_enable,
        ((ps_extra_dw1 & PS_EXTRA_ENABLE_PS_DEPENDENCY_ON_CPSIZE_CHANGE) != 0) as u8,
        ps_bary_coeffs,
        ps_source_depth_w,
        ps_grf_start,
        ps_max_threads_per_psd,
        ps_blend_has_writeable_rt,
        binding_table_pointer_offset,
        surface_state_pointer_offset,
        dispatch_reason,
        dispatch_armed,
        no_varying_payload,
    );
    let state_words = unsafe {
        core::slice::from_raw_parts(warm.draw_state_virt as *const u32, warm.draw_state_len / 4)
    };
    let sf_vp = &state_words[probe_state.sf_clip_viewport_offset_bytes as usize / 4
        ..probe_state.sf_clip_viewport_offset_bytes as usize / 4 + 16];
    let scissor = &state_words[probe_state.scissor_rect_offset_bytes as usize / 4
        ..probe_state.scissor_rect_offset_bytes as usize / 4 + 2];
    let surface = &state_words[probe_state.surface_state_offset_bytes as usize / 4
        ..probe_state.surface_state_offset_bytes as usize / 4 + 16];
    let bt_entry0 = state_words[probe_state.binding_table_offset_bytes as usize / 4];
    let vp_xmin = f32::from_bits(sf_vp[12]);
    let vp_xmax = f32::from_bits(sf_vp[13]);
    let vp_ymin = f32::from_bits(sf_vp[14]);
    let vp_ymax = f32::from_bits(sf_vp[15]);
    let cc_min_depth =
        f32::from_bits(state_words[probe_state.cc_viewport_offset_bytes as usize / 4]);
    let cc_max_depth =
        f32::from_bits(state_words[probe_state.cc_viewport_offset_bytes as usize / 4 + 1]);
    let scissor_xmin = scissor[0] & 0xFFFF;
    let scissor_ymin = (scissor[0] >> 16) & 0xFFFF;
    let scissor_xmax = scissor[1] & 0xFFFF;
    let scissor_ymax = (scissor[1] >> 16) & 0xFFFF;
    let primitive_replication_count = primitive_replication_dw1 & 0xF;
    let primitive_replication_mask = (primitive_replication_dw1 >> 16) & 0xFFFF;
    let vp_extents_ok = vp_xmin <= vp_xmax && vp_ymin <= vp_ymax && vp_xmax > 0.0 && vp_ymax > 0.0;
    let draw_rect_ok = draw.target_w != 0 && draw.target_h != 0;
    let sample_mask_ok = backend_probe_mode.sample_mask_dw() != 0;
    let cull_none = ((raster_dw1 >> 16) & 0x3) == 1;
    let checkbook_clip_enable = (clip_dw2 >> 31) & 0x1;
    let prim_repl_ok = primitive_replication_count == 0 || primitive_replication_mask != 0;
    let fixed_admit_ok = vp_extents_ok
        && draw_rect_ok
        && sample_mask_ok
        && cull_none
        && prim_repl_ok
        && checkbook_clip_enable != 0
        && ((sf_dw1 >> 1) & 0x1) != 0;
    // RENDER_SURFACE_STATE DW2 stores width in bits 13:0 and height in
    // bits 29:16.  The base address is the DW9:DW8 pair, not DW8:DW1.
    let rt_width = (surface[2] & 0x3FFF) + 1;
    let rt_height = ((surface[2] >> 16) & 0x3FFF) + 1;
    let surface_base = ((surface[9] as u64) << 32) | u64::from(surface[8]);
    let sbe_ps_attr_match = (sbe_num_sf_attrs != 0) == (ps_attribute_enable != 0);
    let sbe_read_valid = sbe_vertex_read_length != 0 || sbe_num_sf_attrs == 0;
    let ps_dispatch_bits_ok = (ps_dispatch_8 | ps_dispatch_16 | ps_dispatch_32) != 0;
    let ps_reserved_mbz_ok = (ps_extra_dw1 & (1 << 17)) == 0;
    let rt_binding_ok = ps_blend_has_writeable_rt != 0
        && bt_entry0 == surface_state_pointer_offset
        && rt_width == draw.target_w
        && rt_height == draw.target_h;
    let expected_depth_test = u32::from(depth_config.is_some());
    let expected_depth_write = depth_config.map_or(0, |depth| u32::from(depth.write_enabled));
    let depth_state_ok = wm_depth_test_enable == expected_depth_test
        && wm_depth_write_enable == expected_depth_write
        && wm_stencil_test_enable == 0;
    let ps_admit_ok = ps_valid != 0
        && dispatch_armed != 0
        && ps_dispatch_bits_ok
        && sbe_ps_attr_match
        && sbe_read_valid
        && ps_reserved_mbz_ok
        && rt_binding_ok
        && wm_force_thread_dispatch != 1
        && wm_hz_op_active == 0
        && depth_state_ok;
    intel_render_focus_log!(
        "{} launch-checkbook-fixed accepted={} vf_vue={} clip_enable={} sf_vp_transform={} vp_extents_ok={} vp=[{:.1},{:.1}..{:.1},{:.1}] draw_rect_ok={} draw_rect=[0,0..{},{}] scissor_enable={} scissor=[{},{}..{},{}] sample_mask_ok={} sample_mask=0x{:X} cull_none={} prim_repl_ok={} prim_repl_count={} prim_repl_mask=0x{:X} note=fixed-function-admission\n",
        submit_name,
        fixed_admit_ok as u8,
        vf_synthesized_vue as u8,
        checkbook_clip_enable,
        (sf_dw1 >> 1) & 0x1,
        vp_extents_ok as u8,
        vp_xmin,
        vp_ymin,
        vp_xmax,
        vp_ymax,
        draw_rect_ok as u8,
        draw.target_w.saturating_sub(1),
        draw.target_h.saturating_sub(1),
        (raster_dw1 >> 1) & 0x1,
        scissor_xmin,
        scissor_ymin,
        scissor_xmax,
        scissor_ymax,
        sample_mask_ok as u8,
        backend_probe_mode.sample_mask_dw(),
        cull_none as u8,
        prim_repl_ok as u8,
        primitive_replication_count,
        primitive_replication_mask,
    );
    intel_render_focus_log!(
        "{} launch-checkbook-ps accepted={} ps_valid={} dispatch_armed={} dispatch_bits={}{}{} wm_force={} wm_hz_op_active={} wm_hz_op_inactive={} reject_wm_hz_op_active={} depth_state_ok={} depth_test={} depth_write={} sbe_read_valid={} sbe_attr_ps_match={} sbe_attrs={} ps_attr={} ps_reserved_mbz={} rt_binding_ok={} bt0=0x{:X} surf_off=0x{:X} rt={}x{} rt_gpu=0x{:X} cc_depth=[{:.1},{:.1}] note=ps-dispatch-admission\n",
        submit_name,
        ps_admit_ok as u8,
        ps_valid,
        dispatch_armed,
        ps_dispatch_8,
        ps_dispatch_16,
        ps_dispatch_32,
        wm_force_thread_dispatch,
        wm_hz_op_active,
        (wm_hz_op_active == 0) as u8,
        (wm_hz_op_active != 0) as u8,
        depth_state_ok as u8,
        wm_depth_test_enable,
        wm_depth_write_enable,
        sbe_read_valid as u8,
        sbe_ps_attr_match as u8,
        sbe_num_sf_attrs,
        ps_attribute_enable,
        ps_reserved_mbz_ok as u8,
        rt_binding_ok as u8,
        bt_entry0,
        surface_state_pointer_offset,
        rt_width,
        rt_height,
        surface_base,
        cc_min_depth,
        cc_max_depth,
    );
    if post_draw_sync_variant == PostDrawSyncVariant::LightPostSyncNoCs {
        intel_render_focus_log!(
            "{} postdraw-marker-contract variant={} post_sync_write=1 cs_stall=0 if_write_missing_and_following_mi_retires=sync-marker-failure-not-ps-stop\n",
            submit_name,
            post_draw_sync_variant.label(),
        );
    }
    let clip_mode = (clip_dw2 >> 13) & 0x7;
    let api_mode = (clip_dw2 >> 30) & 0x1;
    let provoking_tri_fan = clip_dw2 & 0x3;
    let provoking_line = (clip_dw2 >> 2) & 0x3;
    let provoking_tri_strip = (clip_dw2 >> 4) & 0x3;
    let guardband_enable = (clip_dw2 >> 26) & 0x1;
    let viewport_xy_clip_enable = (clip_dw2 >> 28) & 0x1;
    let clip_enable = (clip_dw2 >> 31) & 0x1;
    let force_clip_mode = ((clip_dw1 & CLIP_FORCE_CLIP_MODE) != 0) as u8;
    let early_cull_enable = (clip_dw1 >> 18) & 0x1;
    let statistics_enable = (clip_dw1 >> 10) & 0x1;
    let vertex_subpixel_precision = (clip_dw1 >> 19) & 0x1;
    let max_vp_idx = clip_dw3 & 0xF;
    let force_zero_rta_index = (clip_dw3 >> 5) & 0x1;
    let max_point_width_raw = (clip_dw3 >> 6) & 0x7FF;
    intel_render_verbose_log!(
        "probe-clip-decoded topo={} patchlist=0 gs_active=0 ClipMode={}({}) APIMode={}({}) GuardbandClipTestEnable={} ViewportXYClipTestEnable={} ClipEnable={} PerspectiveDivideDisable={} ForceClipMode={} EarlyCullEnable={} StatisticsEnable={} VertexSubPixelPrecisionSelect={} TriangleFanProvokingVertexSelect={} LineStripListProvokingVertexSelect={} TriangleStripListProvokingVertexSelect={} MaximumVPIndex={} ForceZeroRTAIndexEnable={} MaximumPointWidthRaw=0x{:X}\n",
        primitive_topology_label(batch_mode.topology()),
        clip_mode,
        decode_clip_mode_name(clip_mode),
        api_mode,
        decode_api_mode_name(api_mode),
        guardband_enable,
        viewport_xy_clip_enable,
        clip_enable,
        ((clip_dw2 & CLIP_PERSPECTIVE_DIVIDE_DISABLE) != 0) as u8,
        force_clip_mode,
        early_cull_enable,
        statistics_enable,
        decode_vertex_subpixel_precision_name(vertex_subpixel_precision),
        provoking_tri_fan,
        provoking_line,
        provoking_tri_strip,
        max_vp_idx,
        force_zero_rta_index,
        max_point_width_raw,
    );
    intel_render_verbose_log!(
        "probe-sf-decoded ViewportTransformEnable={} StatisticsEnable={} LegacyGlobalDepthBiasEnable={} DerefBlockSize={}({}) LineWidth=0x{:X} PointWidth=0x{:X} PointWidthSource={} SmoothPointEnable={} LastPixelEnable={} TriangleStripListProvokingVertexSelect={} LineStripListProvokingVertexSelect={} TriangleFanProvokingVertexSelect={}\n",
        (sf_dw1 >> 1) & 0x1,
        (sf_dw1 >> 10) & 0x1,
        (sf_dw1 >> 11) & 0x1,
        (sf_dw2 >> 29) & 0x3,
        decode_deref_block_size_name((sf_dw2 >> 29) & 0x3),
        (sf_dw1 >> 12) & 0x3FFFF,
        sf_dw3 & 0x7FF,
        if (sf_dw3 & (1 << 11)) != 0 {
            "state"
        } else {
            "vertex"
        },
        (sf_dw3 >> 13) & 0x1,
        (sf_dw3 >> 31) & 0x1,
        (sf_dw3 >> 29) & 0x3,
        (sf_dw3 >> 27) & 0x3,
        (sf_dw3 >> 25) & 0x3,
    );
    intel_render_verbose_log!(
        "probe-raster-decoded sf_viewport=0x{:X} cc_viewport=0x{:X} scissor_ptr=0x{:X} cull={} fill_front={} fill_back={} front={} scissor_enable={} aa_enable={} smooth_point={} msaa_rast_enable={} msaa_rast_mode={} force_msaa={} forced_samples={} sample_mask=0x1\n",
        probe_state.sf_clip_viewport_offset_bytes,
        probe_state.cc_viewport_offset_bytes,
        probe_state.scissor_rect_offset_bytes,
        decode_cull_mode_name((raster_dw1 >> 16) & 0x3),
        decode_fill_mode_name((raster_dw1 >> 5) & 0x3),
        decode_fill_mode_name((raster_dw1 >> 3) & 0x3),
        decode_front_winding_name((raster_dw1 >> 21) & 0x1),
        (raster_dw1 >> 1) & 0x1,
        (raster_dw1 >> 2) & 0x1,
        (raster_dw1 >> 13) & 0x1,
        (raster_dw1 >> 12) & 0x1,
        (raster_dw1 >> 10) & 0x3,
        (raster_dw1 >> 14) & 0x1,
        (raster_dw1 >> 18) & 0x7,
    );
    if backend_probe_mode.raster_hammer() {
        intel_render_verbose_log!(
            "probe-raster-hammer backend={} early_sample={} early_draw_rect={} scissor={} kill_pixel_off={} forced_ms={} dx_ms={} ps_bt_count={} sf_viewport_transform={} point_width_raw=0x{:X} clip_max_point_width_raw=0x{:X} wm_hz_sample_mask={}\n",
            backend_probe_mode.label(),
            backend_probe_mode.sample_mask_before_clip() as u8,
            backend_probe_mode.draw_rect_before_clip() as u8,
            backend_probe_mode.enable_raster_scissor() as u8,
            backend_probe_mode.force_kill_pixel_off() as u8,
            backend_probe_mode.force_multisample_raster() as u8,
            backend_probe_mode.dx_multisample_raster() as u8,
            ps_binding_table_entry_count,
            ((sf_dw1 >> 1) & 0x1),
            sf_dw3 & 0x7FF,
            max_point_width_raw,
            wm_hz_op_dw4 & 0xFFFF,
        );
    }
    intel_render_verbose_log!(
        "probe-prim-repl-decoded replication_count={} replica_mask=0x{:X} rtai0={}\n",
        primitive_replication_dw1 & 0xF,
        (primitive_replication_dw1 >> 16) & 0xFFFF,
        0,
    );
    intel_render_verbose_log!(
        "probe-handoff-decoded clip_out=sf vue_in_urb=1 baked_vs_urb_out_len={} programmed_vs_urb_out_len={} sbe_read_offset={} sbe_read_len={} ps_varyings={} streamout={}\n",
        baked_vs_urb_output_length,
        programmed_vs_urb_output_length,
        sbe_vertex_read_offset,
        sbe_vertex_read_length,
        sbe_num_sf_attrs,
        batch_mode.streamout_enabled() as u8,
    );
    intel_render_verbose_log!(
        "probe-ps-payload-decoded backend={} push_constant_enable={} push_constant_bytes={} scratch=0x{:X} grf_start={} grf_used={} ps_extra=0x{:08X} attr_enable={} simple_hint={} cpdep={} src_depth={} src_w={} src_depth_w_coeff={} bary_coeffs={} wm_bary=0x{:X} ps_dispatch_bits={}{}{} does_not_prove=ps_thread_launch\n",
        backend_probe_mode.label(),
        ps_push_constant_enable as u8,
        pipeline.ps.meta.kernel.push_constant_bytes,
        ps_scratch_space_buffer,
        ps_grf_start,
        pipeline.ps.meta.kernel.grf_used,
        ps_extra_dw1,
        ((ps_extra_dw1 & PS_EXTRA_ATTRIBUTE_ENABLE) != 0) as u8,
        ((ps_extra_dw1 & PS_EXTRA_SIMPLE_PS_HINT) != 0) as u8,
        ((ps_extra_dw1 & PS_EXTRA_ENABLE_PS_DEPENDENCY_ON_CPSIZE_CHANGE) != 0) as u8,
        ((ps_extra_dw1 & PS_EXTRA_USES_SOURCE_DEPTH) != 0) as u8,
        ((ps_extra_dw1 & PS_EXTRA_USES_SOURCE_W) != 0) as u8,
        ((ps_extra_dw1 & PS_EXTRA_REQUIRES_SOURCE_DEPTH_W_PLANE) != 0) as u8,
        ((ps_extra_dw1
            & (PS_EXTRA_REQUIRES_NONPERSPECTIVE_BARY_PLANE
                | PS_EXTRA_REQUIRES_PERSPECTIVE_BARY_PLANE))
            != 0) as u8,
        (wm_dw1 >> 11) & 0x3F,
        ps_dispatch_8,
        ps_dispatch_16,
        ps_dispatch_32,
    );
    intel_render_verbose_log!(
        "probe-ps-grf-decoded backend={} baked_grf_start={} programmed_grf_start={} grf_used={} register_blocks_16={} max_threads_per_psd={} ps_dw6=0x{:08X} ps_dw7=0x{:08X} dispatch_bits={}{}{} does_not_prove=ps_thread_launch\n",
        backend_probe_mode.label(),
        pipeline.ps.meta.kernel.grf_start_register,
        ps_grf_start,
        pipeline.ps.meta.kernel.grf_used,
        (u32::from(pipeline.ps.meta.kernel.grf_used) + 15) / 16,
        ps_max_threads_per_psd,
        ps_dw6,
        ps_dw7,
        ps_dispatch_8,
        ps_dispatch_16,
        ps_dispatch_32,
    );
    intel_render_verbose_log!(
        "3dprimitive-setup mode={:?} topo={} vertices={} start_vertex=0 instances={} start_instance=0 base_vertex=0 indirect_gpu=0x{:X} indirect_stride={} command_owner={} vb=0x{:X} stride={} rt=0x{:X} pitch=0x{:X} rect={}x{} postdraw_sync={} light_flags=0x{:08X}\n",
        batch_mode,
        primitive_topology_label(batch_mode.topology()),
        draw.vertex_count,
        1,
        draw.indirect_args_gpu_addr.unwrap_or(0),
        if draw.indirect_args_gpu_addr.is_some() {
            DRAW_INDEXED_INDIRECT_BYTES
        } else {
            0
        },
        if draw.indirect_args_gpu_addr.is_some() {
            "helio-gpu-record"
        } else {
            "trueos-direct"
        },
        draw.vertex_gpu_addr,
        draw.vertex_stride,
        draw.rt_gpu_addr,
        draw.rt_pitch,
        draw.target_w,
        draw.target_h,
        post_draw_sync_variant.label(),
        post_draw_sync_variant.light_sync_flags(),
    );

    Ok(cursor * core::mem::size_of::<u32>())
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn encode_minimal_streamout_proof_batch(
    batch_dwords: &mut [u32],
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
    result_gpu_addr: u64,
    pre3d_value: u32,
    post3d_value: u32,
    done_value: u32,
    streamout_experiment: StreamoutProofExperiment,
    slice_hash_table_offset_bytes: u32,
    vs_config: Option<VsStreamoutProofConfig>,
) -> Result<usize, &'static str> {
    let mut cursor = 0usize;
    let batch_mode = if vs_config.is_some() {
        TriangleBatchMode::VsStreamoutProof
    } else {
        TriangleBatchMode::VfStreamoutProof
    };
    let submit_label = if vs_config.is_some() {
        "vs-streamout-proof"
    } else {
        "vf-streamout-proof"
    };

    fn push(batch_dwords: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), &'static str> {
        if *cursor >= batch_dwords.len() {
            return Err("vf-streamout-batch-exhausted");
        }
        batch_dwords[*cursor] = value;
        *cursor += 1;
        Ok(())
    }

    fn push_addr(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        value: u64,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, value as u32)?;
        push(batch_dwords, cursor, (value >> 32) as u32)
    }

    fn push_store_data_imm(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        address: u64,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, MI_STORE_DATA_IMM_GGTT_DW1)?;
        push_addr(batch_dwords, cursor, address)?;
        push(batch_dwords, cursor, value)
    }

    fn push_pipe_control(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD)?;
        push(batch_dwords, cursor, flags)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)
    }

    fn push_pipe_control_post_sync_imm(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags: u32,
        address: u64,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD)?;
        push(batch_dwords, cursor, flags)?;
        push(batch_dwords, cursor, address as u32)?;
        push(batch_dwords, cursor, (address >> 32) as u32)?;
        push(batch_dwords, cursor, value)?;
        push(batch_dwords, cursor, 0)
    }

    fn push_load_register_imm(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        reg: usize,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, mi_lri_cmd(1, MI_LRI_FORCE_POSTED))?;
        push(batch_dwords, cursor, reg as u32)?;
        push(batch_dwords, cursor, value)
    }

    fn push_sba_address(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        mocs: u32,
        address: u64,
    ) -> Result<(), &'static str> {
        let low = ((address as u32) & 0xFFFF_F000) | (mocs << 4) | u32::from(enable);
        push(batch_dwords, cursor, low)?;
        push(batch_dwords, cursor, (address >> 32) as u32)
    }

    fn push_sba_size(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        size_bytes: usize,
    ) -> Result<(), &'static str> {
        let size_bytes =
            crate::intel::align_up(size_bytes, 4096).ok_or("vf-streamout-sba-align")?;
        let size_bytes = u32::try_from(size_bytes).map_err(|_| "vf-streamout-sba-convert")?;
        push(batch_dwords, cursor, (size_bytes & 0xFFFF_F000) | u32::from(enable))
    }

    fn sampler_count_encoding(count: u8) -> u32 {
        match count {
            0 => 0,
            1..=4 => 1,
            5..=8 => 2,
            9..=12 => 3,
            _ => 4,
        }
    }

    fn log_batch_offset(cursor: usize, label: &str) {
        intel_render_batch_log!(
            "batch-off 0x{:03X} {}\n",
            cursor * core::mem::size_of::<u32>(),
            label
        );
    }

    fn cmd_3dstate_vertex_buffers(count: usize) -> Result<u32, &'static str> {
        let body_dwords = count
            .checked_mul(4)
            .and_then(|n| n.checked_sub(1))
            .ok_or("vf-streamout-vb-count-overflow")?;
        let body_dwords =
            u32::try_from(body_dwords).map_err(|_| "vf-streamout-vb-count-convert")?;
        Ok(body_dwords | (8 << 16) | (3 << 27) | (3 << 29))
    }

    fn cmd_3dstate_vertex_elements(count: usize) -> Result<u32, &'static str> {
        let body_dwords = count
            .checked_mul(2)
            .and_then(|n| n.checked_sub(1))
            .ok_or("vf-streamout-ve-count-overflow")?;
        let body_dwords =
            u32::try_from(body_dwords).map_err(|_| "vf-streamout-ve-count-convert")?;
        Ok(body_dwords | (9 << 16) | (3 << 27) | (3 << 29))
    }

    fn push_vertex_buffer_state(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        vertex_buffer_index: u32,
        pitch: u32,
        start_addr: u64,
        size_bytes: u32,
    ) -> Result<(), &'static str> {
        push(
            batch_dwords,
            cursor,
            (pitch & 0xFFF)
                | (1 << 14)
                | (VERTEX_BUFFER_MOCS << 16)
                | VERTEX_BUFFER_L3_BYPASS_DISABLE
                | (vertex_buffer_index << 26),
        )?;
        push_addr(batch_dwords, cursor, start_addr)?;
        push(batch_dwords, cursor, size_bytes)
    }

    fn push_vertex_element_state(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        vertex_buffer_index: u32,
        source_offset: u32,
        source_format: u32,
        component0: u32,
        component1: u32,
        component2: u32,
        component3: u32,
    ) -> Result<(), &'static str> {
        push(
            batch_dwords,
            cursor,
            (source_offset & 0xFFF)
                | (source_format << 16)
                | (1 << 25)
                | (vertex_buffer_index << 26),
        )?;
        push(
            batch_dwords,
            cursor,
            (component0 << 28) | (component1 << 24) | (component2 << 20) | (component3 << 16),
        )
    }

    let streamout_surface_size_dwords = (warm.streamout_len / 4).saturating_sub(1) as u32;
    let streamout_dw1 = (1 << 25) | (1 << 30) | (1 << 31);
    let streamout_dw2 = streamout_experiment.vertex_read_length();
    let streamout_dw3 = streamout_experiment.vertex_bytes() as u32;
    let streamout_dw4 = 0u32;
    let so_buffer_index_dw1 = (RENDER_MOCS << 22) | (1 << 20) | (1 << 21) | (1 << 31);
    let sbe_dw1 = (1 << 5) | (1 << 11) | (1 << 21) | (1 << 22) | (1 << 28) | (1 << 29);
    let programmed_vs_urb_output_length = vs_config
        .map(|config| {
            TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE
                .unwrap_or(config.pipeline.vs.meta.urb_entry_output_length)
        })
        .unwrap_or(1);
    let urb_vs_alloc_dw1 = (programmed_vs_urb_output_length.saturating_sub(1) as u32)
        | (TRIANGLE_VS_URB_START << 10)
        | (TRIANGLE_VS_URB_START << 21);
    let urb_vs_alloc_dw2 = TRIANGLE_VS_URB_ENTRIES | (TRIANGLE_VS_URB_ENTRIES << 16);
    let gfx125_sample_pattern_dw = 0x8888_8888;
    let gfx125_slice_hash =
        device_is_gfx125(warm.device_id).then(|| gfx125_slice_hash_config(warm));
    let gfx125_3d_mode_dw1 = gfx125_slice_hash.map(gfx125_3d_mode_dw1).unwrap_or(0);
    let gfx125_3d_mode_dw3 = gfx125_3d_mode_dw3();
    let vb_size_bytes = draw.vertex_buffer_bytes;
    let vb_cmd = cmd_3dstate_vertex_buffers(1)?;
    let ve_cmd = cmd_3dstate_vertex_elements(if vs_config.is_some() {
        1
    } else {
        streamout_experiment.vf_vertex_element_count()
    })?;

    batch_dwords.fill(0);

    log_batch_offset(cursor, "PIPE_CONTROL flush");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_FLUSH_BITS)?;
    log_batch_offset(cursor, "PIPE_CONTROL invalidate");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS)?;

    log_batch_offset(cursor, "PIPELINE_SELECT");
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_3D)?;
    log_batch_offset(cursor, "MI_STORE_DATA_IMM batch-entry");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_BATCH_ENTRY_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_BATCH_ENTRY,
    )?;

    if device_is_gfx12(warm.device_id) {
        let l3alloc = if device_is_gfx125(warm.device_id) {
            GFX125_L3ALLOC_FULL_WAYS
        } else {
            GEN12_L3ALLOC_ADL_DEFAULT
        };
        log_batch_offset(cursor, "MI_LOAD_REGISTER_IMM L3ALLOC");
        push_load_register_imm(batch_dwords, &mut cursor, GEN12_L3ALLOC, l3alloc)?;
        intel_render_verbose_log!(
            "streamout-l3alloc-init device=0x{:04X} value=0x{:08X} profile={}\n",
            warm.device_id,
            l3alloc,
            if device_is_gfx125(warm.device_id) {
                "gfx125-full-ways"
            } else {
                "adl-gfx12-default"
            },
        );
    }

    log_batch_offset(cursor, "STATE_BASE_ADDRESS");
    push(batch_dwords, &mut cursor, STATE_BASE_ADDRESS_CMD)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_VERTEX_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.vertex_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    for _ in 0..6 {
        push(batch_dwords, &mut cursor, 0)?;
    }

    if device_is_gfx12(warm.device_id) {
        log_batch_offset(cursor, "3DSTATE_SAMPLE_PATTERN");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLE_PATTERN)?;
        for _ in 0..8 {
            push(batch_dwords, &mut cursor, gfx125_sample_pattern_dw)?;
        }
    }

    if device_is_gfx125(warm.device_id) {
        log_batch_offset(cursor, "3DSTATE_SLICE_TABLE_STATE_POINTERS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SLICE_TABLE_STATE_POINTERS)?;
        push(
            batch_dwords,
            &mut cursor,
            slice_hash_table_offset_bytes | u32::from(slice_hash_table_offset_bytes != 0),
        )?;

        log_batch_offset(cursor, "3DSTATE_3D_MODE");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_3D_MODE)?;
        push(batch_dwords, &mut cursor, gfx125_3d_mode_dw1)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, gfx125_3d_mode_dw3)?;
        let slice_hash = gfx125_slice_hash.expect("gfx125 slice hash config");
        intel_render_verbose_log!(
            "gfx125-svl-init sample_pattern=center slice_hash_ptr=0x{:X} geom_dss=0x{:08X} ppipe_dss={}/{}/{} mask1=0x{:X} mask2=0x{:X} mode_dw1=0x{:08X} mode_dw3=0x{:08X} cross_slice_mode={}({}) rhwo_disable=1\n",
            slice_hash_table_offset_bytes,
            slice_hash.geometry_dss_enable,
            slice_hash.ppipe_subslices[0],
            slice_hash.ppipe_subslices[1],
            slice_hash.ppipe_subslices[2],
            slice_hash.ppipe_mask1,
            slice_hash.ppipe_mask2,
            gfx125_3d_mode_dw1,
            gfx125_3d_mode_dw3,
            slice_hash.cross_slice_hashing_mode,
            if slice_hash.cross_slice_hashing_mode == GFX125_3D_MODE_CROSS_SLICE_HASHING_32X32 {
                "hashing32x32"
            } else {
                "normal"
            },
        );
    }

    log_batch_offset(cursor, "3DSTATE_VF_INSTANCING");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_INSTANCING)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_VF_STATISTICS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_STATISTICS | 1)?;
    if device_is_gfx125(warm.device_id) {
        log_batch_offset(cursor, "3DSTATE_VFG");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VFG)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_VF");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_VF_SGVS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_SGVS)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_VF_SGVS_2");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_SGVS_2)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "3DSTATE_VERTEX_BUFFERS");
    push(batch_dwords, &mut cursor, vb_cmd)?;
    push_vertex_buffer_state(
        batch_dwords,
        &mut cursor,
        0,
        draw.vertex_stride,
        draw.vertex_gpu_addr,
        vb_size_bytes,
    )?;

    log_batch_offset(cursor, "3DSTATE_VERTEX_ELEMENTS");
    push(batch_dwords, &mut cursor, ve_cmd)?;
    if vs_config.is_some() {
        push_vertex_element_state(
            batch_dwords,
            &mut cursor,
            0,
            0,
            SURFACE_FORMAT_R32G32B32_FLOAT,
            VFCOMP_STORE_SRC,
            VFCOMP_STORE_SRC,
            VFCOMP_STORE_SRC,
            VFCOMP_STORE_1_FP,
        )?;
    } else {
        match streamout_experiment {
            StreamoutProofExperiment::PositionSlot0 => {
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    0,
                    SURFACE_FORMAT_R32G32B32A32_UINT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                )?;
            }
            StreamoutProofExperiment::PositionSlot1 => {
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    0,
                    SURFACE_FORMAT_R32G32B32A32_UINT,
                    VFCOMP_STORE_0,
                    VFCOMP_STORE_0,
                    VFCOMP_STORE_0,
                    VFCOMP_STORE_0,
                )?;
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    0,
                    SURFACE_FORMAT_R32G32B32A32_UINT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                )?;
            }
            StreamoutProofExperiment::PrmVueHeaderPositionSlots01
            | StreamoutProofExperiment::PrmVueHeaderPositionXywzSlots01
            | StreamoutProofExperiment::HeaderAndPositionSlots01 => {
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    0,
                    SURFACE_FORMAT_R32G32B32A32_UINT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                )?;
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    16,
                    SURFACE_FORMAT_R32G32B32A32_UINT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                )?;
            }
            StreamoutProofExperiment::PointSizeSlot0PositionSlot1 => {
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    16,
                    SURFACE_FORMAT_R32G32B32A32_UINT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                )?;
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    0,
                    0,
                    SURFACE_FORMAT_R32G32B32A32_UINT,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                    VFCOMP_STORE_SRC,
                )?;
            }
        }
    }

    log_batch_offset(cursor, "3DSTATE_VF_TOPOLOGY");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_TOPOLOGY)?;
    push(batch_dwords, &mut cursor, batch_mode.topology())?;
    log_batch_offset(cursor, "MI_STORE_DATA_IMM packet-marker after-VF-state");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_VF_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_VF,
    )?;

    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_HS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_HS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_DS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_DS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_GS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_GS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_VS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_VS)?;
    push(batch_dwords, &mut cursor, urb_vs_alloc_dw1)?;
    push(batch_dwords, &mut cursor, urb_vs_alloc_dw2)?;

    if let Some(config) = vs_config {
        let pipeline = config.pipeline;
        let shader_layout = config.shader_layout;
        let vs_ksp_offset = shader_layout.vs.code_offset_bytes + shader_layout.vs.ksp_offset_bytes;
        let baked_vs_urb_output_length = pipeline.vs.meta.urb_entry_output_length;
        let vs_dw3 = ((pipeline.vs.meta.kernel.binding_table_entry_count as u32) << 18)
            | (sampler_count_encoding(pipeline.vs.meta.kernel.sampler_count) << 27);
        let applied_vs_grf_start =
            triangle_vs_dispatch_grf_start_register(pipeline.vs.meta.kernel.grf_start_register);
        let vs_dw6 = (1 << 11) | (applied_vs_grf_start << 20);
        let vs_dw7 = 1
            | (1 << 2)
            | (1 << 10)
            | (triangle_vs_max_threads_field(warm.device_id, pipeline.vs.meta.max_threads) << 22);
        let vs_state_urb_output_length = if device_is_gfx12(warm.device_id) {
            0
        } else {
            programmed_vs_urb_output_length
        };
        let vs_dw8 = (vs_state_urb_output_length as u32) << 16;
        log_batch_offset(cursor, "3DSTATE_VS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VS)?;
        push(batch_dwords, &mut cursor, vs_ksp_offset & !0x3F)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, vs_dw3)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, vs_dw6)?;
        push(batch_dwords, &mut cursor, vs_dw7)?;
        push(batch_dwords, &mut cursor, vs_dw8)?;
        intel_render_verbose_log!(
            "probe-vs ksp=0x{:08X} dw3=0x{:08X} dw6=0x{:08X} dw7=0x{:08X} dw8=0x{:08X} baked_max_threads={} applied_max_threads_field={} urb_alloc_64b={} vs_state_urb_out_len={} baked_grf_start={} applied_grf_start={} dispatch={:?}\n",
            vs_ksp_offset & !0x3F,
            vs_dw3,
            vs_dw6,
            vs_dw7,
            vs_dw8,
            pipeline.vs.meta.max_threads,
            triangle_vs_max_threads_field(warm.device_id, pipeline.vs.meta.max_threads),
            programmed_vs_urb_output_length,
            vs_state_urb_output_length,
            pipeline.vs.meta.kernel.grf_start_register,
            applied_vs_grf_start,
            pipeline.vs.meta.kernel.dispatch_mode,
        );
        intel_render_verbose_log!(
            "probe-vs-export note={} position_only={} generic_attrs=0 baked_urb_bytes={} programmed_urb_bytes={} expected_vue=header+position-only\n",
            crate::intel::shader::triangle_pipeline_note(),
            (pipeline.ps.meta.num_varying_inputs == 0) as u8,
            (baked_vs_urb_output_length as u32) * 64,
            (programmed_vs_urb_output_length as u32) * 64,
        );
    } else {
        log_batch_offset(cursor, "3DSTATE_VS disabled");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VS)?;
        for _ in 0..8 {
            push(batch_dwords, &mut cursor, 0)?;
        }
    }
    log_batch_offset(cursor, "MI_STORE_DATA_IMM packet-marker after-VS-state");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_VS_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_VS,
    )?;

    log_batch_offset(cursor, "3DSTATE_HS disabled");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_HS)?;
    for _ in 0..8 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_TE disabled");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_TE)?;
    for _ in 0..4 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_DS disabled");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_DS)?;
    for _ in 0..10 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_GS disabled");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_GS)?;
    for _ in 0..9 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_PS disabled");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_PS)?;
    for _ in 0..11 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "MI_STORE_DATA_IMM packet-marker after-PS-state");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_PS_STATE_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_PS_STATE,
    )?;

    log_batch_offset(cursor, "3DSTATE_SBE");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE)?;
    push(batch_dwords, &mut cursor, sbe_dw1)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, SBE_ACTIVE_COMPONENT_XYZW_MASK_DWORD)?;
    push(batch_dwords, &mut cursor, SBE_ACTIVE_COMPONENT_XYZW_MASK_DWORD)?;

    log_batch_offset(cursor, "3DSTATE_STREAMOUT");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_STREAMOUT)?;
    push(batch_dwords, &mut cursor, streamout_dw1)?;
    push(batch_dwords, &mut cursor, streamout_dw2)?;
    push(batch_dwords, &mut cursor, streamout_dw3)?;
    push(batch_dwords, &mut cursor, streamout_dw4)?;

    log_batch_offset(cursor, "PIPE_CONTROL pre-so-buffer");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;
    log_batch_offset(cursor, "3DSTATE_SO_BUFFER_INDEX_0");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SO_BUFFER_INDEX_0)?;
    push(batch_dwords, &mut cursor, so_buffer_index_dw1)?;
    push_addr(batch_dwords, &mut cursor, GPU_VA_STREAMOUT_BASE)?;
    push(batch_dwords, &mut cursor, streamout_surface_size_dwords)?;
    push_addr(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "PIPE_CONTROL post-so-buffer");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;

    log_batch_offset(cursor, "3DSTATE_SO_DECL_LIST");
    let streamout_decl_dword0 = streamout_experiment.so_decl_buffer_selects();
    let streamout_decl_dword1 = streamout_experiment.so_decl_num_entries();
    let [
        streamout_decl_dword2,
        streamout_decl_dword3,
        streamout_decl_dword4,
        streamout_decl_dword5,
    ] = streamout_experiment.so_decl_entry_dwords();
    push(batch_dwords, &mut cursor, streamout_experiment.so_decl_header())?;
    push(batch_dwords, &mut cursor, streamout_decl_dword0)?;
    push(batch_dwords, &mut cursor, streamout_decl_dword1)?;
    push(batch_dwords, &mut cursor, streamout_decl_dword2)?;
    push(batch_dwords, &mut cursor, streamout_decl_dword3)?;
    if matches!(
        streamout_experiment,
        StreamoutProofExperiment::PrmVueHeaderPositionSlots01
            | StreamoutProofExperiment::PrmVueHeaderPositionXywzSlots01
            | StreamoutProofExperiment::HeaderAndPositionSlots01
    ) {
        push(batch_dwords, &mut cursor, streamout_decl_dword4)?;
        push(batch_dwords, &mut cursor, streamout_decl_dword5)?;
    }
    crate::log!(
        "{} decl experiment={} read_len={} so_pitch={} decl=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] slot_contract={}\n",
        submit_label,
        streamout_experiment.label(),
        streamout_experiment.vertex_read_length(),
        streamout_experiment.vertex_bytes(),
        streamout_decl_dword0,
        streamout_decl_dword1,
        streamout_decl_dword2,
        streamout_decl_dword3,
        streamout_decl_dword4,
        streamout_decl_dword5,
        streamout_experiment.vf_slot_contract(),
    );
    crate::log!(
        "{} contract experiment={} stages_disabled={} sbe[read_offset=1 read_length=1 num_sf_attrs=1 force_offset=1 force_length=1] urb_vs[alloc_len={} start={} entries={}] vb[index=0 pitch={} size=0x{:X}] streamout[read_offset=0 read_length_field={} rendering_disable={} stats_enable={} pitch={} so_gpu=0x{:X} size_dwords=0x{:X}] topo={}\n",
        submit_label,
        streamout_experiment.label(),
        if vs_config.is_some() {
            "hs|te|ds|gs|ps"
        } else {
            "vs|hs|te|ds|gs|ps"
        },
        programmed_vs_urb_output_length,
        TRIANGLE_VS_URB_START,
        TRIANGLE_VS_URB_ENTRIES,
        draw.vertex_stride,
        vb_size_bytes,
        streamout_dw2 & 0x1F,
        (streamout_dw1 >> 30) & 0x1,
        (streamout_dw1 >> 25) & 0x1,
        streamout_dw3 & 0xFFF,
        GPU_VA_STREAMOUT_BASE,
        streamout_surface_size_dwords,
        primitive_topology_label(batch_mode.topology()),
    );
    log_batch_offset(cursor, "PIPE_CONTROL post-so-decl");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;

    log_batch_offset(cursor, "MI_STORE_DATA_IMM pre-3d");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_PRE3D_DWORD as u64) * 4,
        pre3d_value,
    )?;

    log_batch_offset(cursor, "3DPRIMITIVE");
    push(batch_dwords, &mut cursor, CMD_3DPRIMITIVE)?;
    push(batch_dwords, &mut cursor, batch_mode.topology())?;
    push(batch_dwords, &mut cursor, draw.vertex_count)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 1)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "MI_STORE_DATA_IMM pre-light-pipe-control");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_PRE_LIGHT_PC_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_PRE_LIGHT_PC,
    )?;

    log_batch_offset(cursor, "PIPE_CONTROL post-3d-light-marker");
    push_pipe_control_post_sync_imm(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_POST_DRAW_LIGHT_SYNC_BITS,
        result_gpu_addr + (RESULT_SLOT_POST3D_LIGHT_PIPE_CONTROL_LO_DWORD as u64) * 4,
        post3d_value,
    )?;

    log_batch_offset(cursor, "MI_STORE_DATA_IMM final-after-light");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_FINAL_AFTER_LIGHT_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_FINAL_AFTER_LIGHT,
    )?;

    log_batch_offset(cursor, "PIPE_CONTROL post-3d-heavy-sync");
    push_pipe_control_post_sync_imm(
        batch_dwords,
        &mut cursor,
        PIPE_CONTROL_POST_DRAW_SYNC_BITS,
        result_gpu_addr + (RESULT_SLOT_POST3D_PIPE_CONTROL_LO_DWORD as u64) * 4,
        post3d_value,
    )?;

    log_batch_offset(cursor, "MI_STORE_DATA_IMM final");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_FINAL_DWORD as u64) * 4,
        done_value,
    )?;
    log_batch_offset(cursor, "MI_BATCH_BUFFER_END");
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;

    intel_render_verbose_log!(
        "3dprimitive-setup mode={:?} topo={} vertices={} start_vertex=0 instances=1 start_instance=0 base_vertex=0 vb=0x{:X} stride={} rt=0x{:X} pitch=0x{:X} rect={}x{} postdraw_sync={} light_flags=0x{:08X}\n",
        batch_mode,
        primitive_topology_label(batch_mode.topology()),
        draw.vertex_count,
        draw.vertex_gpu_addr,
        draw.vertex_stride,
        draw.rt_gpu_addr,
        draw.rt_pitch,
        draw.target_w,
        draw.target_h,
        PostDrawSyncVariant::HeavyAll.label(),
        PostDrawSyncVariant::HeavyAll.light_sync_flags(),
    );

    Ok(cursor * core::mem::size_of::<u32>())
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn encode_vf_streamout_proof_batch(
    batch_dwords: &mut [u32],
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
    result_gpu_addr: u64,
    pre3d_value: u32,
    post3d_value: u32,
    done_value: u32,
    streamout_experiment: StreamoutProofExperiment,
    slice_hash_table_offset_bytes: u32,
) -> Result<usize, &'static str> {
    encode_minimal_streamout_proof_batch(
        batch_dwords,
        warm,
        draw,
        result_gpu_addr,
        pre3d_value,
        post3d_value,
        done_value,
        streamout_experiment,
        slice_hash_table_offset_bytes,
        None,
    )
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn encode_vs_streamout_proof_batch(
    batch_dwords: &mut [u32],
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
    result_gpu_addr: u64,
    pre3d_value: u32,
    post3d_value: u32,
    done_value: u32,
    streamout_experiment: StreamoutProofExperiment,
    slice_hash_table_offset_bytes: u32,
    vs_config: VsStreamoutProofConfig,
) -> Result<usize, &'static str> {
    encode_minimal_streamout_proof_batch(
        batch_dwords,
        warm,
        draw,
        result_gpu_addr,
        pre3d_value,
        post3d_value,
        done_value,
        streamout_experiment,
        slice_hash_table_offset_bytes,
        Some(vs_config),
    )
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn encode_3d_no_draw_probe_batch(
    batch_dwords: &mut [u32],
    warm: RenderWarmState,
    result_gpu_addr: u64,
    done_value: u32,
    extra_store: Option<(u64, u32)>,
) -> Result<usize, &'static str> {
    let mut cursor = 0usize;

    fn push(batch_dwords: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), &'static str> {
        if *cursor >= batch_dwords.len() {
            return Err("3d-no-draw-batch-exhausted");
        }
        batch_dwords[*cursor] = value;
        *cursor += 1;
        Ok(())
    }

    fn push_addr(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        value: u64,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, value as u32)?;
        push(batch_dwords, cursor, (value >> 32) as u32)
    }

    fn push_store_data_imm(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        address: u64,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, MI_STORE_DATA_IMM_GGTT_DW1)?;
        push_addr(batch_dwords, cursor, address)?;
        push(batch_dwords, cursor, value)
    }

    fn push_pipe_control_full(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags_dw0: u32,
        flags_dw1: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD)?;
        push(batch_dwords, cursor, flags_dw1)?;
        if let Some(slot) = batch_dwords.get_mut(cursor.saturating_sub(2)) {
            *slot |= flags_dw0;
        } else {
            return Err("3d-no-draw-pipe-control-header");
        }
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)
    }

    fn push_sba_address(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        mocs: u32,
        address: u64,
    ) -> Result<(), &'static str> {
        let low = ((address as u32) & 0xFFFF_F000) | (mocs << 4) | u32::from(enable);
        push(batch_dwords, cursor, low)?;
        push(batch_dwords, cursor, (address >> 32) as u32)
    }

    fn push_sba_size(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        enable: bool,
        size_bytes: usize,
    ) -> Result<(), &'static str> {
        let size_bytes = crate::intel::align_up(size_bytes, 4096).ok_or("3d-no-draw-sba-align")?;
        let size_bytes = u32::try_from(size_bytes).map_err(|_| "3d-no-draw-sba-convert")?;
        push(batch_dwords, cursor, (size_bytes & 0xFFFF_F000) | u32::from(enable))
    }

    batch_dwords.fill(0);
    push_pipe_control_full(batch_dwords, &mut cursor, 0, PIPE_CONTROL_FLUSH_BITS)?;
    push_pipe_control_full(batch_dwords, &mut cursor, 0, PIPE_CONTROL_INVALIDATE_BITS)?;
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_3D)?;
    push(batch_dwords, &mut cursor, STATE_BASE_ADDRESS_CMD)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push(batch_dwords, &mut cursor, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_VERTEX_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.vertex_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    for _ in 0..6 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    if let Some((dst_gpu_addr, value)) = extra_store {
        push_store_data_imm(batch_dwords, &mut cursor, dst_gpu_addr, value)?;
    }
    push_store_data_imm(batch_dwords, &mut cursor, result_gpu_addr, done_value)?;
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;
    Ok(cursor * core::mem::size_of::<u32>())
}
