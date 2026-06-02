fn log_render_buffer_layout(warm: RenderWarmState, rt_gpu_addr: Option<u64>) {
    if !crate::logflag::INTEL_RENDER_NGIN_LOGS || crate::logflag::INTEL_STAGE1_LOGS {
        return;
    }
    let rt_gpu_addr = rt_gpu_addr.unwrap_or(0);
    intel_render_verbose_log!(
        "intel/render: buffers ring phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} context phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} batch phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} result phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} streamout phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} state phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} vertex phys=0x{:X} ggtt=0x{:X} bytes=0x{:X} rt_ggtt=0x{:X}\n",
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
    if !crate::logflag::INTEL_RENDER_NGIN_LOGS || crate::logflag::INTEL_STAGE1_LOGS {
        return;
    }
    let (ctx_desc_lo, ctx_desc_hi) = build_execlist_context_descriptor(GPU_VA_CONTEXT_BASE);
    intel_render_verbose_log!(
        "intel/render: encodings mi_store_data_imm=0x{:08X} ctx_desc=0x{:08X}:0x{:08X} state_base_address=0x{:08X} pipe_control=0x{:08X} pc_post_sync_immediate=0x{:08X} pc_dest_ggtt=0x{:08X}\n",
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
    if !crate::logflag::INTEL_RENDER_NGIN_LOGS || crate::logflag::INTEL_STAGE1_LOGS {
        return;
    }
    let dwords = unsafe {
        core::slice::from_raw_parts(warm.draw_state_virt as *const u32, warm.draw_state_len / 4)
    };
    let bt_ptr = probe_state
        .binding_table_offset_bytes
        .saturating_sub(shader_layout.state_region_offset_bytes);
    let bt_entry = dwords[probe_state.binding_table_offset_bytes as usize / 4];
    let surface = &dwords[probe_state.surface_state_offset_bytes as usize / 4
        ..probe_state.surface_state_offset_bytes as usize / 4 + 16];
    let blend = &dwords[probe_state.blend_state_offset_bytes as usize / 4
        ..probe_state.blend_state_offset_bytes as usize / 4 + 16];
    let color_calc = &dwords[probe_state.color_calc_state_offset_bytes as usize / 4
        ..probe_state.color_calc_state_offset_bytes as usize / 4 + 16];
    intel_render_verbose_log!(
        "intel/render: probe-state bt_off=0x{:X} bt_entry0=0x{:08X} surf_off=0x{:X} ps_ptr=bt:0x{:X} blend_ptr=0x{:X} cc_ptr=0x{:X}\n",
        probe_state.binding_table_offset_bytes,
        bt_entry,
        probe_state.surface_state_offset_bytes,
        bt_ptr,
        probe_state.blend_state_offset_bytes | 1,
        probe_state.color_calc_state_offset_bytes | 1
    );
    intel_render_verbose_log!(
        "intel/render: probe-surface d0=0x{:08X} d1=0x{:08X} d2=0x{:08X} d3=0x{:08X} d4=0x{:08X} d5=0x{:08X} d6=0x{:08X} d7=0x{:08X}\n",
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
        "intel/render: probe-surface d8=0x{:08X} d9=0x{:08X} d10=0x{:08X} d11=0x{:08X} d12=0x{:08X} d13=0x{:08X} d14=0x{:08X} d15=0x{:08X}\n",
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
        "intel/render: probe-blend d0=0x{:08X} d1=0x{:08X} d2=0x{:08X} d3=0x{:08X} d4=0x{:08X} d5=0x{:08X} d6=0x{:08X} d7=0x{:08X}\n",
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
        "intel/render: probe-blend d8=0x{:08X} d9=0x{:08X} d10=0x{:08X} d11=0x{:08X} d12=0x{:08X} d13=0x{:08X} d14=0x{:08X} d15=0x{:08X}\n",
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
        "intel/render: probe-cc d0=0x{:08X} d1=0x{:08X} d2=0x{:08X} d3=0x{:08X} d4=0x{:08X} d5=0x{:08X} d6=0x{:08X} d7=0x{:08X}\n",
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
        "intel/render: probe-cc d8=0x{:08X} d9=0x{:08X} d10=0x{:08X} d11=0x{:08X} d12=0x{:08X} d13=0x{:08X} d14=0x{:08X} d15=0x{:08X}\n",
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
) -> Result<TriangleProbeStateLayout, &'static str> {
    let mut cursor = shader_layout.state_region_offset_bytes as usize;
    let binding_table_offset = cursor;
    cursor = crate::intel::align_up(binding_table_offset + 4, 64).ok_or("probe-state-align")?;
    let surface_state_offset = cursor;
    cursor = crate::intel::align_up(surface_state_offset + 64, 32).ok_or("probe-state-align")?;
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
    dwords[binding_table_offset / 4] = surface_state_offset as u32;
    dwords[(binding_table_offset + 0x20) / 4] = surface_state_offset as u32;

    let surface = &mut dwords[surface_state_offset / 4..surface_state_offset / 4 + 16];
    surface.fill(0);
    surface[0] = (SURFTYPE_2D << 29)
        | (SURFACE_FORMAT_B8G8R8A8_UNORM << 18)
        | (SURFACE_HALIGN_4 << 14)
        | (SURFACE_VALIGN_4 << 16);
    surface[1] = RENDER_MOCS << 24;
    surface[2] = draw.target_w.saturating_sub(1) | (draw.target_h.saturating_sub(1) << 16);
    surface[3] = draw.rt_pitch.saturating_sub(1);
    surface[7] = (SHADER_CHANNEL_ALPHA << 16)
        | (SHADER_CHANNEL_BLUE << 19)
        | (SHADER_CHANNEL_GREEN << 22)
        | (SHADER_CHANNEL_RED << 25);
    surface[8] = draw.rt_gpu_addr as u32;
    surface[9] = (draw.rt_gpu_addr >> 32) as u32;

    let sampler = &mut dwords[sampler_state_offset / 4..sampler_state_offset / 4 + 4];
    sampler.fill(0);

    let blend = &mut dwords[blend_state_offset / 4..blend_state_offset / 4 + 16];
    blend.fill(0);
    match blend_mode {
        // Keep the existing explicit RT0 setup as the baseline attempt.
        TriangleBlendProbeMode::ExplicitRt0 => {
            blend[0] = 0;
            blend[1] = (1 << 0) | (1 << 1) | (2 << 2);
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
    sf_clip_viewport[3] = (draw.target_w as f32 * 0.5).to_bits();
    sf_clip_viewport[4] = (draw.target_h as f32 * 0.5).to_bits();
    sf_clip_viewport[5] = 0.0f32.to_bits();
    let clip_guardband =
        if matches!(backend_probe_mode, BackendProbeMode::RasterWmInputOaNdcClipPreconditions) {
            1.0f32
        } else {
            32768.0f32
        };
    sf_clip_viewport[8] = (-clip_guardband).to_bits();
    sf_clip_viewport[9] = clip_guardband.to_bits();
    sf_clip_viewport[10] = (-clip_guardband).to_bits();
    sf_clip_viewport[11] = clip_guardband.to_bits();
    sf_clip_viewport[12] = 0.0f32.to_bits();
    sf_clip_viewport[13] = (draw.target_w.saturating_sub(1) as f32).to_bits();
    sf_clip_viewport[14] = 0.0f32.to_bits();
    sf_clip_viewport[15] = (draw.target_h.saturating_sub(1) as f32).to_bits();
    intel_render_focus_log!(
        "intel/render: probe-sf-viewport-decoded backend={} m00={:.3} m11={:.3} m22={:.3} m30={:.3} m31={:.3} m32={:.3} guardband=[{:.3},{:.3}..{:.3},{:.3}] viewport=[{:.3},{:.3}..{:.3},{:.3}]\n",
        backend_probe_mode.label(),
        f32::from_bits(sf_clip_viewport[0]),
        f32::from_bits(sf_clip_viewport[1]),
        f32::from_bits(sf_clip_viewport[2]),
        f32::from_bits(sf_clip_viewport[3]),
        f32::from_bits(sf_clip_viewport[4]),
        f32::from_bits(sf_clip_viewport[5]),
        f32::from_bits(sf_clip_viewport[8]),
        f32::from_bits(sf_clip_viewport[10]),
        f32::from_bits(sf_clip_viewport[9]),
        f32::from_bits(sf_clip_viewport[11]),
        f32::from_bits(sf_clip_viewport[12]),
        f32::from_bits(sf_clip_viewport[14]),
        f32::from_bits(sf_clip_viewport[13]),
        f32::from_bits(sf_clip_viewport[15]),
    );

    let scissor_rect = &mut dwords[scissor_rect_offset / 4..scissor_rect_offset / 4 + 2];
    scissor_rect[0] = 0;
    scissor_rect[1] = draw.target_w.saturating_sub(1) | (draw.target_h.saturating_sub(1) << 16);

    if slice_hash_table_offset != 0 {
        let slice_hash = &mut dwords[slice_hash_table_offset / 4
            ..slice_hash_table_offset / 4 + GFX125_SLICE_HASH_TABLE_DWORDS];
        let mut packed = [0u32; GFX125_SLICE_HASH_TABLE_DWORDS];
        gfx125_pack_slice_hash_tables(gfx125_slice_hash_config(warm), &mut packed);
        slice_hash.copy_from_slice(&packed);
    }

    let flush_ptr = unsafe {
        warm.draw_state_virt
            .add(shader_layout.state_region_offset_bytes as usize)
    };
    crate::intel::dma_flush(
        flush_ptr,
        end_offset - shader_layout.state_region_offset_bytes as usize,
    );

    Ok(TriangleProbeStateLayout {
        binding_table_offset_bytes: binding_table_offset as u32,
        surface_state_offset_bytes: surface_state_offset as u32,
        sampler_state_offset_bytes: sampler_state_offset as u32,
        blend_state_offset_bytes: blend_state_offset as u32,
        color_calc_state_offset_bytes: color_calc_state_offset as u32,
        cc_viewport_offset_bytes: cc_viewport_offset as u32,
        sf_clip_viewport_offset_bytes: sf_clip_viewport_offset as u32,
        scissor_rect_offset_bytes: scissor_rect_offset as u32,
        slice_hash_table_offset_bytes: slice_hash_table_offset as u32,
    })
}

fn encode_triangle_probe_batch(
    batch_dwords: &mut [u32],
    warm: RenderWarmState,
    draw: TriangleDrawPrep,
    blend_mode: TriangleBlendProbeMode,
    pipeline: &'static crate::intel::shader::TrianglePipeline,
    shader_layout: TriangleShaderLayout,
    probe_state: TriangleProbeStateLayout,
    result_gpu_addr: u64,
    pre3d_value: u32,
    post3d_value: u32,
    done_value: u32,
    batch_mode: TriangleBatchMode,
    streamout_experiment: StreamoutProofExperiment,
    front_end_contract: TriangleFrontEndContract,
    backend_probe_mode: BackendProbeMode,
    post_draw_sync_variant: PostDrawSyncVariant,
) -> Result<usize, &'static str> {
    let mut cursor = 0usize;
    let vf_synthesized_vue = matches!(batch_mode, TriangleBatchMode::VfDraw);
    let primitive_topology = backend_probe_mode
        .primitive_topology_override()
        .unwrap_or(batch_mode.topology());
    let defer_raster_wm_oa_end = backend_probe_mode.defer_raster_wm_oa_end_after_fence();
    let vs_thread_request_expected = !vf_synthesized_vue;
    let ps_thread_request_expected =
        matches!(batch_mode, TriangleBatchMode::Draw | TriangleBatchMode::VfDraw);
    intel_render_focus_log!(
        "intel/render: thread-request-contract mode={:?} vs={} hs=0 ds=0 gs=0 ps={} vf_synthesized_vue={} note=zero_vs_counter_expected_when_vs_disabled zero_hs_ds_gs_expected\n",
        batch_mode,
        vs_thread_request_expected as u8,
        ps_thread_request_expected as u8,
        vf_synthesized_vue as u8,
    );

    fn log_batch_offset(cursor: usize, label: &str) {
        intel_render_batch_log!(
            "intel/render: batch-off 0x{:03X} {}\n",
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

    fn stage_dispatch_bits(mode: crate::intel::shader::DispatchMode) -> (u32, u32, u32) {
        match mode {
            crate::intel::shader::DispatchMode::Simd8 => (1, 0, 0),
            crate::intel::shader::DispatchMode::Simd16 => (0, 1, 0),
            crate::intel::shader::DispatchMode::Simd32 => (0, 0, 1),
        }
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
                CTX_CTRL_OAC_CONTEXT_ENABLE,
                if enable {
                    CTX_CTRL_OAC_CONTEXT_ENABLE
                } else {
                    0
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

    let binding_table_pool_size = warm
        .draw_state_len
        .saturating_sub(shader_layout.state_region_offset_bytes as usize);
    let binding_table_pointer_offset = probe_state
        .binding_table_offset_bytes
        .saturating_sub(shader_layout.state_region_offset_bytes);
    let binding_table_pool_base_dw =
        binding_table_pool_base_dword(warm.device_id, shader_layout.state_region_gpu_addr);
    let binding_table_pool_size_dw = u32::try_from(
        crate::intel::align_up(binding_table_pool_size, 4096).ok_or("probe-binding-pool-align")?,
    )
    .map_err(|_| "probe-binding-pool-convert")?
        & 0xFFFF_F000;
    let binding_table_gpu_addr =
        shader_layout.state_region_gpu_addr + binding_table_pointer_offset as u64;
    let binding_table_entry0_gpu_addr =
        GPU_VA_DRAW_STATE_BASE + probe_state.surface_state_offset_bytes as u64;
    let binding_table_pool_enable = if device_is_gfx125(warm.device_id) {
        "implicit-gfx125"
    } else {
        "bit11"
    };
    let vs_ksp_offset = shader_layout.vs.code_offset_bytes + shader_layout.vs.ksp_offset_bytes;
    let ps_ksp_offset = shader_layout.ps.code_offset_bytes + shader_layout.ps.ksp_offset_bytes;
    let mut sbe_vertex_read_offset = backend_probe_mode
        .sbe_read_offset_override()
        .unwrap_or(front_end_contract.sbe_read_offset) as u32;
    let mut sbe_vertex_read_length = backend_probe_mode
        .sbe_read_length_override()
        .unwrap_or(front_end_contract.sbe_read_length) as u32;
    if vf_synthesized_vue {
        match streamout_experiment {
            StreamoutProofExperiment::PositionSlot0 | StreamoutProofExperiment::PositionSlot0Xyzw => {
                sbe_vertex_read_offset = 0;
                sbe_vertex_read_length = 1;
            }
            StreamoutProofExperiment::PositionSlot1
            | StreamoutProofExperiment::HeaderAndPositionSlots01
            | StreamoutProofExperiment::MesaNoVsRectlist => {
                sbe_vertex_read_offset = 1;
                sbe_vertex_read_length = 1;
            }
        }
    }
    let sbe_attribute_swizzle_enable = backend_probe_mode.enable_sbe_attribute_swizzle();
    // SBE_SWIZ is a separate packet on Xe-LP. Emit an explicit zeroed packet
    // even when attribute swizzle is disabled so the SF/WM handoff never
    // inherits stale swizzle state from earlier firmware or probes.
    let emit_sbe_swiz_packet = !backend_probe_mode.skip_sbe_swiz_packet();
    let mut sbe_dw1 = (sbe_vertex_read_offset << 5)
        | (u32::from(sbe_attribute_swizzle_enable) << 21)
        | ((pipeline.ps.meta.num_varying_inputs as u32) << 22)
        | (u32::from(front_end_contract.force_sbe_read_offset) << 28)
        | (u32::from(front_end_contract.force_sbe_read_length) << 29)
        | (sbe_vertex_read_length << 11);
    let mut sbe_dw2 = 0;
    let mut sbe_dw3 = pipeline.ps.meta.flat_inputs;
    let mut sbe_dw4 = front_end_contract
        .sbe_active_component_override
        .unwrap_or(SBE_ACTIVE_COMPONENT_XYZW_MASK_DWORD);
    let mut sbe_dw5 = sbe_dw4;
    let (ps_dispatch_8, ps_dispatch_16, ps_dispatch_32) = match backend_probe_mode {
        BackendProbeMode::PsDispatchSlot0 => (1, 0, 0),
        BackendProbeMode::PsDispatchSlot1 => (0, 1, 0),
        BackendProbeMode::PsDispatchSlot2 => (0, 0, 1),
        BackendProbeMode::PsDispatchAllKspSlots => (1, 1, 1),
        _ => stage_dispatch_bits(pipeline.ps.meta.kernel.dispatch_mode),
    };
    // Keep CLIP close to Mesa's trivial path. Most probes leave the CLIP stage
    // enabled for counters; the clip-bypass probe mirrors simple-shader state
    // by clearing ClipEnable while using screen-space coordinates.
    let mesa_simple_order = backend_probe_mode.mesa_simple_order();
    let mesa_order_emit_wm_hz_op = backend_probe_mode.mesa_order_with_early_backend();
    let mesa_simple_emit_wm_hz_op_after_raster =
        backend_probe_mode.mesa_simple_emit_wm_hz_op_after_raster();
    let early_backend_before_clip_sf_raster = !mesa_simple_order || mesa_order_emit_wm_hz_op;
    let oracle_active_reference_order = matches!(
        backend_probe_mode,
        BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl
            | BackendProbeMode::RasterWmInputOaNdcTriListMesaActiveBlockSwiz
    );
    let skip_tbimr_tile_pass_info_after_raster = oracle_active_reference_order
        || backend_probe_mode.skip_tbimr_tile_pass_info_after_raster();
    let clip_enable = !backend_probe_mode.disable_clip_enable()
        && !backend_probe_mode.mesa_simple_clip_defaults();
    let (mut clip_dw1, mut clip_dw2, mut clip_dw3) =
        if backend_probe_mode.mesa_simple_clip_defaults() {
            (0, CLIP_PERSPECTIVE_DIVIDE_DISABLE, 0)
        } else {
            (
                if clip_enable { 1 << 10 } else { 0 }
                    | if backend_probe_mode.force_clip_mode() {
                        CLIP_FORCE_CLIP_MODE
                    } else {
                        0
                    }
                    | if backend_probe_mode.enable_clip_preconditions() {
                        (1 << 17) | (1 << 18)
                    } else {
                        0
                    },
                backend_probe_mode.clip_mode_bits()
                    | (u32::from(clip_enable) << 31)
                    | (u32::from(backend_probe_mode.clip_api_mode_d3d()) << 30)
                    | if backend_probe_mode.enable_clip_preconditions() {
                        (1 << 26) | (1 << 28)
                    } else {
                        0
                    }
                    | if backend_probe_mode.enable_clip_perspective_divide() {
                        0
                    } else {
                        CLIP_PERSPECTIVE_DIVIDE_DISABLE
                    }
                    | if backend_probe_mode.enable_clip_non_perspective_barycentric() {
                        CLIP_NON_PERSPECTIVE_BARYCENTRIC_ENABLE
                    } else {
                        0
                    },
                1 << 5,
            )
        };
    // Keep SF close to the trivial host path by default.  The screen-space WM
    // coverage probe disables the SF viewport transform so the coordinate
    // contract matches PerspectiveDivideDisable instead of applying a second
    // hidden transform before scan conversion.
    let sf_line_width_u11_7 = if matches!(
        backend_probe_mode,
        BackendProbeMode::RasterWmInputOaScreenSpaceLineListOpenBounds
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBounds
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixel
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixelMesaSfPoint
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixelMesaSfPointSbeRead0
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixelMesaSfPointSbeRead0Bary8
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixelMesaSfPointSbeRead0Bary8Swiz
    ) {
        8 << 7
    } else if backend_probe_mode.enable_sf_sane_defaults() {
        1 << 7
    } else {
        0
    };
    let sf_point_width_u8_3 = if matches!(
        backend_probe_mode,
        BackendProbeMode::RasterWmInputOaScreenSpacePointList
            | BackendProbeMode::RasterWmInputOaScreenSpacePointListOpenBounds
            | BackendProbeMode::RasterWmInputOaScreenSpacePointListClipNormalOpenBounds
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListOpenBounds
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBounds
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixel
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixelMesaSfPoint
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixelMesaSfPointSbeRead0
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixelMesaSfPointSbeRead0Bary8
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixelMesaSfPointSbeRead0Bary8Swiz
            | BackendProbeMode::RasterWmInputOaNdcPointListOpenBounds
    ) {
        0x7FF
    } else if backend_probe_mode.enable_sf_sane_defaults() {
        1 << 3
    } else {
        0
    };
    let mut sf_dw1 = (u32::from(!backend_probe_mode.disable_sf_viewport_transform()) << 1)
        | if clip_enable { 1 << 10 } else { 0 }
        | (sf_line_width_u11_7 << 12);
    let sf_deref_block_size = backend_probe_mode
        .sf_deref_block_size_override()
        .unwrap_or(1);
    let mut sf_dw2 = sf_deref_block_size << 29;
    let sf_body_dw2_prm_default = if backend_probe_mode.enable_sf_sane_defaults() {
        1 << 11
    } else {
        0
    };
    let mut sf_dw3 = sf_point_width_u8_3
        | sf_body_dw2_prm_default
        | (u32::from(backend_probe_mode.enable_sf_sane_defaults()) << 31);
    if backend_probe_mode.use_mesa_sf_point_state() {
        sf_dw3 = 0x0200_4808;
    }
    // Mirror Mesa's simple-shader path by default: cull none, and otherwise
    // leave raster defaults boring. The forced-MSAA raster probe deliberately
    // overrides only the WM_INT/SF_INT multisample-rasterization mode.
    let forced_ms_raster_mode = backend_probe_mode.forced_ms_raster_mode();
    let forced_raster_sample_count = backend_probe_mode.forced_raster_sample_count();
    let raster_viewport_z_clip_tests = backend_probe_mode.enable_raster_viewport_z_clip_tests();
    let raster_api_mode = backend_probe_mode.raster_api_mode_override().unwrap_or(0);
    let raster_dw1 = (1 << 16)
        | u32::from(raster_viewport_z_clip_tests)
        | (u32::from(backend_probe_mode.enable_wm_scissor()) << 1)
        | (u32::from(backend_probe_mode.raster_front_counter_clockwise()) << 21)
        | ((raster_api_mode & 0x3) << 22)
        | (u32::from(raster_viewport_z_clip_tests) << 26)
        | forced_ms_raster_mode
            .map_or(0, |mode| (1 << 14) | ((mode & 0x3) << 10) | (u32::from(mode >= 2) << 12));
    let mut raster_dw1 =
        raster_dw1 | forced_raster_sample_count.map_or(0, |samples| (samples & 0x7) << 18);
    let mut raster_dw2 = 0;
    let mut raster_dw3 = 0;
    let mut raster_dw4 = 0;
    // Keep primitive replication explicit on the visible Xe-LP triangle path.
    // The userland oracle's simple triangle emitted mask bit 0 set
    // (0x00010000); a zero-count packet leaves the SF-to-WM object contract
    // ambiguous exactly where this frontier is stuck.
    let mut primitive_replication_dw1 = 0;
    // Mesa's simple-shader path emits a nearly all-default WM packet here.
    // Keep this dedicated triangle path equally boring rather than forcing
    // point-rule / line-AA bits that the host reference never asked for.
    let wm_barycentric_mode = (backend_probe_mode
        .wm_barycentric_mode_override()
        .unwrap_or_else(|| {
            u32::from(matches!(backend_probe_mode, BackendProbeMode::PsPayloadBaryPlanes))
        })
        & 0x3f)
        << 11;
    let wm_walk_granularity = backend_probe_mode
        .wm_walk_granularity_override()
        .unwrap_or(2);
    let wm_reserved_bit29 = 0u32;
    let force_wm_dispatch = backend_probe_mode.force_wm_thread_dispatch()
        || (matches!(batch_mode, TriangleBatchMode::VfDraw)
            && !matches!(
                backend_probe_mode,
                BackendProbeMode::WmNormalDispatch
                    | BackendProbeMode::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
                    | BackendProbeMode::RasterWmInputOaScreenSpaceRectListMesaNoVsEarlyBackend
                    | BackendProbeMode::RasterWmInputOaNdcRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
                    | BackendProbeMode::RasterWmInputOaScreenSpaceTriListMesaActiveBlockSwizSfSaneBary8HeaderMesaRasterSfMesaSbe
            ));
    let mut wm_dw1 = (1 << 31)
        | (wm_reserved_bit29 << 29)
        | ((wm_walk_granularity & 0x3) << 24)
        | if force_wm_dispatch {
            // The VF-fed draw path is our backend isolation probe, so make the
            // fragment launch condition explicit instead of inferring it from
            // the minimal Mesa-like defaults. The focused real-VS WM probe can
            // also force this bit to separate scan conversion from dispatch
            // formula gating.
            2 << 19
        } else {
            0
        }
        | wm_barycentric_mode;
    let mut wm_depth_stencil_dw1 = 0;
    let mut wm_depth_stencil_dw2 = 0;
    let mut wm_depth_stencil_dw3 = 0;
    let wm_chroma_key_dw1 = 0;
    let mut ps_blend_dw1 = 1 << 30;
    let streamout_dw1 = (1 << 8) | (1 << 25) | (1 << 31);
    let streamout_dw2 = streamout_experiment.vertex_read_length();
    let streamout_dw3 = streamout_experiment.vertex_bytes() as u32;
    let streamout_dw4 = 0;
    let streamout_surface_size_dwords = (warm.streamout_len / 4).saturating_sub(1) as u32;
    let so_buffer_index_dw1 = (RENDER_MOCS << 22) | (1 << 21) | (1 << 31);
    let so_buffer_stream_offset_dw = 0u32;
    let mut sample_mask_dw1 = 1u32;
    if backend_probe_mode.mesa_active_fixed_function_state() {
        clip_dw1 = 0x0004_0400;
        clip_dw2 = 0xD400_0001;
        if backend_probe_mode.mesa_success_disable_primitive_replication()
            && !matches!(
                backend_probe_mode,
                BackendProbeMode::RasterWmInputOaNdcTriListMesaActiveBlockSwiz
                    | BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl
            )
        {
            clip_dw2 |= CLIP_PERSPECTIVE_DIVIDE_DISABLE;
        }
        clip_dw3 = 0x0003_FFE0;
        sf_dw1 = 0x0008_0402;
        sf_dw2 = sf_deref_block_size << 29;
        sf_dw3 = 0x0200_4808;
        raster_dw1 = 0x04A1_1003;
        raster_dw2 = 0;
        raster_dw3 = 0;
        raster_dw4 = 0;
        sample_mask_dw1 = 0x0000_FFFF;
        wm_depth_stencil_dw1 = 0x0000_0010;
        wm_depth_stencil_dw2 = 0;
        wm_depth_stencil_dw3 = 0;
    }
    if backend_probe_mode.mesa_active_block_state() {
        sample_mask_dw1 = 0x0000_FFFF;
        wm_depth_stencil_dw1 = 0x0000_0010;
        wm_depth_stencil_dw2 = 0;
        wm_depth_stencil_dw3 = 0;
        primitive_replication_dw1 = if matches!(
            backend_probe_mode,
            BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl
        ) {
            0x0001_0000
        } else if backend_probe_mode.mesa_success_disable_primitive_replication() {
            0
        } else {
            0x0001_0000
        };
        wm_dw1 = 0x8000_0040 | wm_barycentric_mode;
        ps_blend_dw1 = 0x518C_6200;
    }
    if backend_probe_mode.mesa_active_raster_state() {
        raster_dw1 = 0x04A1_1003;
        raster_dw2 = 0;
        raster_dw3 = 0;
        raster_dw4 = 0;
    }
    if backend_probe_mode.mesa_active_sf_state() {
        sf_dw1 = 0x0008_0402;
        sf_dw2 = sf_deref_block_size << 29;
        sf_dw3 = 0x0200_4808;
    }
    if backend_probe_mode.mesa_active_sbe_state() {
        sbe_dw1 = 0x3020_0000;
        sbe_dw2 = 0;
        sbe_dw3 = 0;
        sbe_dw4 = 0xFFFF_FFFF;
        sbe_dw5 = 0xFFFF_FFFF;
    }
    let pdoane_minimal_sf_probe = backend_probe_mode.pdoane_minimal_sf_probe();
    if pdoane_minimal_sf_probe {
        clip_dw1 = 0;
        clip_dw2 = CLIP_PERSPECTIVE_DIVIDE_DISABLE | CLIP_NON_PERSPECTIVE_BARYCENTRIC_ENABLE;
        clip_dw3 = 1 << 5;
        sf_dw1 = 1 << 10;
        sf_dw2 = 0;
        sf_dw3 = 0;
        sample_mask_dw1 = 0x0000_FFFF;
        intel_render_focus_log!(
            "intel/render: probe-pdoane-minimal-sf backend={} enabled=1 source=igt-rendercopy-gen7-inspired clip=[0x{:08X},0x{:08X},0x{:08X}] sf=[0x{:08X},0x{:08X},0x{:08X}] sample_mask=0x{:08X} note=active-no-vs-rectlist-clears-sf-point-line-viewport-and-clip-thread-gates\n",
            backend_probe_mode.label(),
            clip_dw1,
            clip_dw2,
            clip_dw3,
            sf_dw1,
            sf_dw2,
            sf_dw3,
            sample_mask_dw1,
        );
    }
    if backend_probe_mode.disable_sf_viewport_transform() {
        sf_dw1 &= !(1 << 1);
    }
    if backend_probe_mode.disable_clip_enable() {
        clip_dw1 &= !(1 << 10);
        clip_dw2 &= !(1 << 31);
        sf_dw1 &= !(1 << 10);
    }
    if force_wm_dispatch {
        wm_dw1 = (wm_dw1 & !(0x3 << 19)) | (2 << 19);
    }
    if vf_synthesized_vue
        && !matches!(
            backend_probe_mode,
            BackendProbeMode::RasterWmInputOaScreenSpaceTriListMesaActiveBlockSwiz
                | BackendProbeMode::RasterWmInputOaScreenSpaceTriListMesaActiveBlockSwizSfSaneBary8HeaderMesaRasterSfMesaSbe
        )
    {
        sample_mask_dw1 = 0x0000_0001;
    }
    if matches!(
        backend_probe_mode,
        BackendProbeMode::RasterWmInputOaScreenSpaceTriListMesaActiveBlockSwiz
    ) {
        clip_dw1 = 0x0000_0400;
        clip_dw2 = 0xC000_8200;
        clip_dw3 = 0x0000_0020;
        sf_dw1 = 0x0000_0400;
        sf_dw2 = 0;
        sf_dw3 = 0;
        sbe_dw1 = 0x3000_0820;
        sbe_dw2 = 0;
        sbe_dw3 = 0;
        sbe_dw4 = 0xFFFF_FFFF;
        sbe_dw5 = 0xFFFF_FFFF;
        sample_mask_dw1 = 0x0000_FFFF;
        wm_dw1 = 0x8000_0040;
    }
    let sbe_decoded_read_offset = (sbe_dw1 >> 5) & 0x3F;
    let sbe_decoded_read_length = (sbe_dw1 >> 11) & 0x1F;
    // Mesa zeros this packet during init to clear any inherited clear/resolve
    // overrides. The focused scissor probe intentionally sets the WM_HZ_OP
    // scissor bit, so keep the packet's own sample mask live with SAMPLE_MASK.
    let wm_hz_op_scissor = backend_probe_mode.enable_wm_hz_op_scissor();
    let wm_hz_op_dw1 = u32::from(wm_hz_op_scissor) << 29;
    let wm_hz_op_dw2 = 0;
    let wm_hz_op_dw3 = if wm_hz_op_scissor {
        draw.target_w.saturating_sub(1) | (draw.target_h.saturating_sub(1) << 16)
    } else {
        0
    };
    let wm_hz_op_dw4 = if wm_hz_op_scissor { sample_mask_dw1 } else { 0 };
    let wm_hz_op_dw5 = 0;
    let gfx125_sample_pattern_dw = 0x8888_8888;
    let multisample_dw1 = 0u32;
    let gfx125_slice_hash =
        device_is_gfx125(warm.device_id).then(|| gfx125_slice_hash_config(warm));
    let gfx125_3d_mode_dw1 = gfx125_slice_hash.map(gfx125_3d_mode_dw1).unwrap_or(0);
    let gfx125_3d_mode_dw2 = 0;
    let (draw_rect_max_x, draw_rect_max_y) = if backend_probe_mode.use_full_drawing_rectangle() {
        (u16::MAX as u32, u16::MAX as u32)
    } else {
        (draw.target_w.saturating_sub(1), draw.target_h.saturating_sub(1))
    };
    let drawing_rectangle_dw2 = draw_rect_max_x | (draw_rect_max_y << 16);
    let gfx125_3d_mode_dw3 = gfx125_3d_mode_dw3();
    let ps_binding_table_entry_count = match backend_probe_mode {
        BackendProbeMode::MesaLike
        | BackendProbeMode::PsBindingTableCountZero
        | BackendProbeMode::WmNormalDispatch
        | BackendProbeMode::PsDispatchSlot0
        | BackendProbeMode::PsDispatchSlot1
        | BackendProbeMode::PsDispatchSlot2
        | BackendProbeMode::PsDispatchAllKspSlots
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
        | BackendProbeMode::RasterWmInputOa
        | BackendProbeMode::RasterWmInputOaNdcBlock32
        | BackendProbeMode::RasterWmInputOaNdcMesaActiveBlock
        | BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockSwiz
        | BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl
        | BackendProbeMode::RasterWmInputOaNdcPerPoly
        | BackendProbeMode::RasterWmInputOaNdcWalk16
        | BackendProbeMode::RasterWmInputOaNdcClipPreconditions
        | BackendProbeMode::RasterWmInputOaNdcNoWmScissor
        | BackendProbeMode::RasterWmInputOaScreenSpace
        | BackendProbeMode::RasterWmInputOaScreenSpaceNoWmHzOpPacket
        | BackendProbeMode::RasterWmInputOaScreenSpaceForceThreadDispatch
        | BackendProbeMode::RasterWmInputOaScreenSpaceSfSaneDefaults
        | BackendProbeMode::RasterWmInputOaScreenSpaceClipBypass
        | BackendProbeMode::RasterWmInputOaScreenSpaceClipBypassRasterPreconditions
        | BackendProbeMode::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBounds
        | BackendProbeMode::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBoundsSbe1NoSwiz
        | BackendProbeMode::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsSbe1NoSwiz
        | BackendProbeMode::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderSbe1NoSwiz
        | BackendProbeMode::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz
        | BackendProbeMode::RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz
        | BackendProbeMode::RasterWmInputOaScreenSpaceSlot0TightPreClipRasterSbePsNoSwiz
        | BackendProbeMode::RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz
        | BackendProbeMode::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPerPolySbe1NoSwiz
        | BackendProbeMode::RasterWmInputOaScreenSpaceRectListClipBypass
        | BackendProbeMode::RasterWmInputOaScreenSpaceRectListClipBypassSfSane
        | BackendProbeMode::RasterWmInputOaScreenSpaceRectListBlorpLike
        | BackendProbeMode::RasterWmInputOaScreenSpaceRectListSlot0PerPoly
        | BackendProbeMode::RasterWmInputOaScreenSpaceRectListSlot0XyzwTightPreClipPerPoly
        | BackendProbeMode::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly
        | BackendProbeMode::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder
        | BackendProbeMode::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOn
        | BackendProbeMode::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
        | BackendProbeMode::RasterWmInputOaScreenSpaceRectListMesaNoVsEarlyBackend
        | BackendProbeMode::RasterWmInputOaNdcRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
        | BackendProbeMode::RasterWmInputOaScreenSpaceClipPreconditions
        | BackendProbeMode::RasterWmInputOaScreenSpaceRasterClipPreconditions
        | BackendProbeMode::RasterWmInputOaScreenSpaceRasterClipPreconditionsHardFence
        | BackendProbeMode::RasterWmInputOaScreenSpaceD3dRasterPreconditionsNoHz
        | BackendProbeMode::RasterWmInputOaScreenSpaceD3dPerPolyNoHz
        | BackendProbeMode::RasterWmInputOaScreenSpacePerPoly
        | BackendProbeMode::RasterWmInputOaScreenSpaceUrb128PerPoly
        | BackendProbeMode::RasterWmInputOaScreenSpaceMesaSimpleOrder
        | BackendProbeMode::RasterWmInputOaScreenSpaceMesaSimpleNoSwizNoScissor
        | BackendProbeMode::RasterWmInputOaScreenSpacePointList
        | BackendProbeMode::RasterWmInputOaScreenSpacePointListOpenBounds
        | BackendProbeMode::RasterWmInputOaScreenSpacePointListClipNormalOpenBounds
        | BackendProbeMode::RasterWmInputOaScreenSpaceLineListOpenBounds
        | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBounds
        | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1
        | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixel
        | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixelMesaSfPoint
        | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixelMesaSfPointSbeRead0
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixelMesaSfPointSbeRead0Bary8
        | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixelMesaSfPointSbeRead0Bary8Swiz
        | BackendProbeMode::RasterWmInputOaNdcLineListMesaActiveBlockSwiz
        | BackendProbeMode::RasterWmInputOaNdcRectListMesaActiveBlockSwiz
        | BackendProbeMode::RasterWmInputOaNdcTriListMesaActiveBlockSwiz
        | BackendProbeMode::RasterWmInputOaScreenSpaceTriListMesaActiveBlockSwiz
        | BackendProbeMode::RasterWmInputOaScreenSpaceTriListMesaActiveBlockSwizSfSane
        | BackendProbeMode::RasterWmInputOaScreenSpaceTriListMesaActiveBlockSwizSfSaneBary8
        | BackendProbeMode::RasterWmInputOaScreenSpaceTriListMesaActiveBlockSwizSfSaneBary8Header
        | BackendProbeMode::RasterWmInputOaScreenSpaceTriListMesaActiveBlockSwizSfSaneBary8HeaderMesaRaster
        | BackendProbeMode::RasterWmInputOaScreenSpaceTriListMesaActiveBlockSwizSfSaneBary8HeaderMesaRasterSf
        | BackendProbeMode::RasterWmInputOaScreenSpaceTriListMesaActiveBlockSwizSfSaneBary8HeaderMesaRasterSfMesaSbe
        | BackendProbeMode::RasterWmInputOaNdcPointListOpenBounds
        | BackendProbeMode::RasterWmInputOaScreenSpaceRtIndependent
        | BackendProbeMode::RasterWmInputOaNdcForceOnPattern
        | BackendProbeMode::RasterWmInputOaForceOnPattern
        | BackendProbeMode::RasterWmInputOaForceOffPixel => {
            pipeline.ps.meta.kernel.binding_table_entry_count
        }
        BackendProbeMode::PsBindingTableCountOne => {
            pipeline.ps.meta.kernel.binding_table_entry_count.max(1)
        }
    };
    let ps_binding_table_entry_count = if backend_probe_mode.force_ps_binding_table_count_zero()
        || matches!(
            backend_probe_mode,
            BackendProbeMode::PsBindingTableCountZero
            | BackendProbeMode::RasterWmInputOa
            | BackendProbeMode::RasterWmInputOaNdcBlock32
            | BackendProbeMode::RasterWmInputOaNdcPerPoly
            | BackendProbeMode::RasterWmInputOaNdcWalk16
            | BackendProbeMode::RasterWmInputOaNdcClipPreconditions
            | BackendProbeMode::RasterWmInputOaNdcNoWmScissor
            | BackendProbeMode::RasterWmInputOaScreenSpace
            | BackendProbeMode::RasterWmInputOaScreenSpaceNoWmHzOpPacket
            | BackendProbeMode::RasterWmInputOaScreenSpaceForceThreadDispatch
            | BackendProbeMode::RasterWmInputOaScreenSpaceSfSaneDefaults
            | BackendProbeMode::RasterWmInputOaScreenSpaceClipBypass
            | BackendProbeMode::RasterWmInputOaScreenSpaceClipBypassRasterPreconditions
            | BackendProbeMode::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBounds
            | BackendProbeMode::RasterWmInputOaScreenSpaceClipBypassRasterPreconditionsOpenBoundsSbe1NoSwiz
            | BackendProbeMode::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsSbe1NoSwiz
            | BackendProbeMode::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderSbe1NoSwiz
            | BackendProbeMode::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPreClipSbePsSbe1NoSwiz
            | BackendProbeMode::RasterWmInputOaScreenSpaceSlot0PreClipSbePsNoSwiz
            | BackendProbeMode::RasterWmInputOaScreenSpaceSlot0TightPreClipRasterSbePsNoSwiz
            | BackendProbeMode::RasterWmInputOaScreenSpaceSlot0XyzwTightPreClipRasterSbePsNoSwiz
            | BackendProbeMode::RasterWmInputOaScreenSpaceAcceptAllOpenBoundsHeaderPerPolySbe1NoSwiz
            | BackendProbeMode::RasterWmInputOaScreenSpaceRectListClipBypass
            | BackendProbeMode::RasterWmInputOaScreenSpaceRectListClipBypassSfSane
            | BackendProbeMode::RasterWmInputOaScreenSpaceRectListBlorpLike
            | BackendProbeMode::RasterWmInputOaScreenSpaceRectListSlot0PerPoly
            | BackendProbeMode::RasterWmInputOaScreenSpaceRectListSlot0XyzwTightPreClipPerPoly
            | BackendProbeMode::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1TightPreClipPerPoly
            | BackendProbeMode::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrder
            | BackendProbeMode::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOn
            | BackendProbeMode::RasterWmInputOaScreenSpaceRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
            | BackendProbeMode::RasterWmInputOaScreenSpaceRectListMesaNoVsEarlyBackend
            | BackendProbeMode::RasterWmInputOaNdcRectListSlot0XyzwSbe1MesaOrderClipOnEarlyBackend
            | BackendProbeMode::RasterWmInputOaScreenSpaceClipPreconditions
            | BackendProbeMode::RasterWmInputOaScreenSpaceRasterClipPreconditions
            | BackendProbeMode::RasterWmInputOaScreenSpaceRasterClipPreconditionsHardFence
            | BackendProbeMode::RasterWmInputOaScreenSpaceD3dRasterPreconditionsNoHz
            | BackendProbeMode::RasterWmInputOaScreenSpaceD3dPerPolyNoHz
            | BackendProbeMode::RasterWmInputOaScreenSpacePerPoly
            | BackendProbeMode::RasterWmInputOaScreenSpaceUrb128PerPoly
            | BackendProbeMode::RasterWmInputOaScreenSpaceMesaSimpleOrder
            | BackendProbeMode::RasterWmInputOaScreenSpaceMesaSimpleNoSwizNoScissor
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListOpenBounds
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBounds
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixel
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixelMesaSfPoint
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixelMesaSfPointSbeRead0
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixelMesaSfPointSbeRead0Bary8
            | BackendProbeMode::RasterWmInputOaScreenSpaceLineListMesaBackendOpenBoundsSample1OnPixelMesaSfPointSbeRead0Bary8Swiz
            | BackendProbeMode::RasterWmInputOaScreenSpaceRtIndependent
            | BackendProbeMode::RasterWmInputOaNdcForceOnPattern
            | BackendProbeMode::RasterWmInputOaForceOnPattern
            | BackendProbeMode::RasterWmInputOaForceOffPixel
        )
    {
        0
    } else {
        ps_binding_table_entry_count
    };
    let mut ps_dw3 = (binding_table_entry_count_encoding(ps_binding_table_entry_count) << 18)
        | (sampler_count_encoding(pipeline.ps.meta.kernel.sampler_count) << 27)
        | (u32::from(pipeline.ps.meta.uses_vmask) * PS_VECTOR_MASK_ENABLE);
    let ps_push_constant_enable = pipeline.ps.meta.kernel.push_constant_bytes > 0
        || matches!(backend_probe_mode, BackendProbeMode::PsPayloadPushConstant);
    let ps_max_threads_per_psd = backend_probe_mode
        .ps_max_threads_override()
        .unwrap_or(TRIANGLE_PS_MAX_THREADS);
    let ps_grf_start = backend_probe_mode
        .ps_grf_start_override()
        .unwrap_or(pipeline.ps.meta.kernel.grf_start_register);
    let mut ps_dw6 = ps_dispatch_8
        | (ps_dispatch_16 << 1)
        | (ps_dispatch_32 << 2)
        | (u32::from(ps_push_constant_enable) * PS_PUSH_CONSTANT_ENABLE)
        | (ps_max_threads_per_psd << PS_MAX_THREADS_SHIFT);
    let mut ps_dw7 =
        (ps_grf_start as u32) | ((ps_grf_start as u32) << 8) | ((ps_grf_start as u32) << 16);
    let ps_ksp_base = ps_ksp_offset & !0x3F;
    let ps_ksp0 = if matches!(backend_probe_mode.ps_dispatch_slot(), Some(1 | 2)) {
        0
    } else {
        ps_ksp_base
    };
    let mut ps_ksp1 = if matches!(
        backend_probe_mode,
        BackendProbeMode::PsDispatchSlot1 | BackendProbeMode::PsDispatchAllKspSlots
    ) {
        ps_ksp_base
    } else {
        0
    };
    let mut ps_ksp2 = if matches!(
        backend_probe_mode,
        BackendProbeMode::PsDispatchSlot2 | BackendProbeMode::PsDispatchAllKspSlots
    ) {
        ps_ksp_base
    } else {
        0
    };
    let ps_scratch_space_buffer = 0u32;
    let ps_extra_attribute_enable = pipeline.ps.meta.num_varying_inputs > 0
        || matches!(backend_probe_mode, BackendProbeMode::PsPayloadAttributeEnable);
    let mut ps_extra_dw1 = (u32::from(pipeline.ps.meta.computed_stencil)
        * PS_EXTRA_PIXEL_SHADER_COMPUTES_STENCIL)
        | (u32::from(pipeline.ps.meta.persample_dispatch) * PS_EXTRA_PIXEL_SHADER_IS_PER_SAMPLE)
        | (u32::from(ps_extra_attribute_enable) * PS_EXTRA_ATTRIBUTE_ENABLE)
        | (u32::from(matches!(backend_probe_mode, BackendProbeMode::PsPayloadSimpleHint))
            * PS_EXTRA_SIMPLE_PS_HINT)
        | (u32::from(matches!(backend_probe_mode, BackendProbeMode::PsPayloadSourceDepthW))
            * (PS_EXTRA_REQUIRES_SOURCE_DEPTH_W_PLANE
                | PS_EXTRA_USES_SOURCE_W
                | PS_EXTRA_USES_SOURCE_DEPTH))
        | (u32::from(matches!(backend_probe_mode, BackendProbeMode::PsPayloadBaryPlanes))
            * (PS_EXTRA_REQUIRES_NONPERSPECTIVE_BARY_PLANE
                | PS_EXTRA_REQUIRES_PERSPECTIVE_BARY_PLANE))
        | ((pipeline.ps.meta.computed_depth_mode as u32) << 26)
        | PS_EXTRA_PIXEL_SHADER_VALID;
    let oracle_active_ps_packet = oracle_active_reference_order
        || matches!(
            backend_probe_mode,
            BackendProbeMode::RasterWmInputOaNdcTriListMesaActiveBlockSwiz
                | BackendProbeMode::RasterWmInputOaScreenSpaceTriListMesaActiveBlockSwizSfSaneBary8HeaderMesaRasterSfMesaSbe
        );
    if oracle_active_ps_packet {
        ps_dw3 = 0x4004_0000;
        ps_dw6 = 0x1F80_0003;
        ps_dw7 = 0x0002_0002;
        ps_extra_dw1 = 0x8000_0000;
        ps_ksp1 = ps_ksp_base;
        ps_ksp2 = ps_ksp_base + 0x40;
        intel_render_focus_log!(
            "intel/render: probe-oracle-ps-packet-override backend={} emitted=1 ps_dw3=0x{:08X} ps_dw6=0x{:08X} ps_dw7=0x{:08X} ps_extra=0x{:08X} ksp1=0x{:X} ksp2=0x{:X} source=host-rendercopy-gen12-active-draw-segment note=simd16-ksp1-mirrors-local-simd8-kernel\n",
            backend_probe_mode.label(),
            ps_dw3,
            ps_dw6,
            ps_dw7,
            ps_extra_dw1,
            ps_ksp1,
            ps_ksp2,
        );
    }

    batch_dwords.fill(0);

    log_batch_offset(cursor, "PIPE_CONTROL flush");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_FLUSH_BITS)?;
    log_batch_offset(cursor, "PIPE_CONTROL invalidate");
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS)?;

    log_batch_offset(cursor, "PIPELINE_SELECT");
    push(batch_dwords, &mut cursor, PIPELINE_SELECT_3D)?;

    if device_is_gfx125(warm.device_id) {
        let disable_tbimr = backend_probe_mode.disable_gfx125_tbimr_raster_wa();
        let chicken_raster_2_value = if disable_tbimr {
            gfx125_chicken_raster_2_disable_value()
        } else {
            gfx125_chicken_raster_2_value()
        };
        log_batch_offset(cursor, "MI_LOAD_REGISTER_IMM MCR_SELECTOR multicast");
        push_load_register_imm(batch_dwords, &mut cursor, MCR_SELECTOR, MCR_MULTICAST)?;
        log_batch_offset(cursor, "MI_LOAD_REGISTER_IMM CHICKEN_RASTER_2");
        push_load_register_imm(
            batch_dwords,
            &mut cursor,
            CHICKEN_RASTER_2,
            chicken_raster_2_value,
        )?;
        log_batch_offset(cursor, "MI_LOAD_REGISTER_IMM MCR_SELECTOR multicast restore");
        push_load_register_imm(batch_dwords, &mut cursor, MCR_SELECTOR, MCR_MULTICAST)?;
        intel_render_verbose_log!(
            "intel/render: gfx125-raster-wa-batch chicken_raster_2=0x{:08X} mcr_selector=0x{:08X} tbimr_fast_clip={} mode={} source=linux-xe-wa-14021567978 hypothesis=tbimr-fast-clip-global-control-without-primitive-tile-pass-may-block-wm\n",
            chicken_raster_2_value,
            MCR_MULTICAST,
            (!disable_tbimr) as u8,
            if disable_tbimr { "disable" } else { "enable" },
        );
    }

    log_batch_offset(cursor, "STATE_BASE_ADDRESS");
    push(batch_dwords, &mut cursor, STATE_BASE_ADDRESS_CMD)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push(batch_dwords, &mut cursor, RENDER_MOCS << 16)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.vertex_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "3DSTATE_AA_LINE_PARAMETERS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_AA_LINE_PARAMETERS)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "3DSTATE_SAMPLE_PATTERN");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLE_PATTERN)?;
    for _ in 0..8 {
        push(batch_dwords, &mut cursor, gfx125_sample_pattern_dw)?;
    }
    intel_render_focus_log!(
        "intel/render: probe-sample-pattern-state emitted=1 device=0x{:04X} pattern=center_8_16th includes_1x_sample=1 placement=post-state-base-before-raster\n",
        warm.device_id,
    );

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
            "intel/render: gfx125-svl-init sample_pattern=center slice_hash_ptr=0x{:X} geom_dss=0x{:08X} ppipe_dss={}/{}/{} mask1=0x{:X} mask2=0x{:X} mode_dw1=0x{:08X} mode_dw3=0x{:08X} cross_slice_mode={}({}) rhwo_disable=1\n",
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
    push(batch_dwords, &mut cursor, (shader_layout.state_region_gpu_addr >> 32) as u32)?;
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
    push(batch_dwords, &mut cursor, 0)?;
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
    push(batch_dwords, &mut cursor, binding_table_pointer_offset)?;

    log_batch_offset(cursor, "3DSTATE_VIEWPORT_STATE_POINTERS_CC");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VIEWPORT_STATE_POINTERS_CC)?;
    push(batch_dwords, &mut cursor, probe_state.cc_viewport_offset_bytes)?;
    log_batch_offset(cursor, "3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP)?;
    push(batch_dwords, &mut cursor, probe_state.sf_clip_viewport_offset_bytes)?;
    log_batch_offset(cursor, "3DSTATE_VIEWPORT_STATE_POINTERS generic-zero");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VIEWPORT_STATE_POINTERS)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_SCISSOR_STATE_POINTERS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SCISSOR_STATE_POINTERS)?;
    push(batch_dwords, &mut cursor, probe_state.scissor_rect_offset_bytes)?;

    let vertex_buffer_dw1_default = draw.vertex_stride | (1 << 14) | (RENDER_MOCS << 16);
    let mesa_no_vs_rectlist_vb1 = vf_synthesized_vue
        && matches!(
            streamout_experiment,
            StreamoutProofExperiment::MesaNoVsRectlist
        );
    let active_i16_rectlist_vb = vf_synthesized_vue
        && matches!(
            (streamout_experiment, backend_probe_mode),
            (
                StreamoutProofExperiment::PositionSlot1,
                BackendProbeMode::RasterWmInputOaScreenSpaceRectListMesaNoVsEarlyBackend
            )
        );
    let mut vertex_buffer_dw1 = front_end_contract
        .vertex_buffer_dw1_override
        .unwrap_or(vertex_buffer_dw1_default);
    if mesa_no_vs_rectlist_vb1 || active_i16_rectlist_vb {
        vertex_buffer_dw1 = (vertex_buffer_dw1 & !0xFFF) | (draw.vertex_stride & 0xFFF);
    }
    let vertex_buffer1_dw1 = (vertex_buffer_dw1 & !(0x3F << 26)) | (1 << 26);
    let vertex_buffer_addr_lo = draw.vertex_gpu_addr as u32;
    let vertex_buffer_addr_hi = (draw.vertex_gpu_addr >> 32) as u32;
    let vertex_buffer_bytes = draw.vertex_count.saturating_mul(draw.vertex_stride);
    log_batch_offset(cursor, "3DSTATE_VERTEX_BUFFERS");
    push(
        batch_dwords,
        &mut cursor,
        if mesa_no_vs_rectlist_vb1 {
            CMD_3DSTATE_VERTEX_BUFFERS_2
        } else {
            CMD_3DSTATE_VERTEX_BUFFERS_1
        },
    )?;
    push(batch_dwords, &mut cursor, vertex_buffer_dw1)?;
    push_addr(batch_dwords, &mut cursor, draw.vertex_gpu_addr)?;
    push(batch_dwords, &mut cursor, vertex_buffer_bytes)?;
    if mesa_no_vs_rectlist_vb1 {
        push(batch_dwords, &mut cursor, vertex_buffer1_dw1)?;
        push_addr(batch_dwords, &mut cursor, draw.vertex_gpu_addr)?;
        push(batch_dwords, &mut cursor, vertex_buffer_bytes)?;
        intel_render_focus_log!(
            "intel/render: probe-vf-vb1-fix active=1 experiment={} vb0_dw1=0x{:08X} vb1_dw1=0x{:08X} addr=0x{:X} bytes={} reason=mesa-no-vs-rectlist-ve0-references-vb1\n",
            streamout_experiment.label(),
            vertex_buffer_dw1,
            vertex_buffer1_dw1,
            draw.vertex_gpu_addr,
            vertex_buffer_bytes,
        );
    }

    let vf_vertex_element_count = if vf_synthesized_vue {
        streamout_experiment.vf_vertex_element_count()
    } else {
        1
    };
    let vertex_elements_cmd = if vf_vertex_element_count == 3 {
        CMD_3DSTATE_VERTEX_ELEMENTS_3
    } else if vf_vertex_element_count == 2 {
        CMD_3DSTATE_VERTEX_ELEMENTS_2
    } else {
        CMD_3DSTATE_VERTEX_ELEMENTS_1
    };
    let vertex_element0_dw0;
    let vertex_element0_dw1;
    log_batch_offset(cursor, "3DSTATE_VERTEX_ELEMENTS");
    push(batch_dwords, &mut cursor, vertex_elements_cmd)?;
    if vf_synthesized_vue {
        match streamout_experiment {
            StreamoutProofExperiment::PositionSlot0 => {
                vertex_element0_dw0 = (SURFACE_FORMAT_R32G32B32_FLOAT << 16) | (1 << 25);
                vertex_element0_dw1 = (VFCOMP_STORE_SRC << 28)
                    | (VFCOMP_STORE_SRC << 24)
                    | (VFCOMP_STORE_SRC << 20)
                    | (VFCOMP_STORE_1_FP << 16);
                push(batch_dwords, &mut cursor, vertex_element0_dw0)?;
                push(batch_dwords, &mut cursor, vertex_element0_dw1)?;
            }
            StreamoutProofExperiment::PositionSlot0Xyzw => {
                vertex_element0_dw0 = (SURFACE_FORMAT_R32G32B32A32_FLOAT << 16) | (1 << 25);
                vertex_element0_dw1 = (VFCOMP_STORE_SRC << 28)
                    | (VFCOMP_STORE_SRC << 24)
                    | (VFCOMP_STORE_SRC << 20)
                    | (VFCOMP_STORE_SRC << 16);
                push(batch_dwords, &mut cursor, vertex_element0_dw0)?;
                push(batch_dwords, &mut cursor, vertex_element0_dw1)?;
            }
            StreamoutProofExperiment::PositionSlot1 => {
                vertex_element0_dw0 = (SURFACE_FORMAT_R32G32B32A32_FLOAT << 16) | (1 << 25);
                vertex_element0_dw1 = (VFCOMP_STORE_0 << 28)
                    | (VFCOMP_STORE_0 << 24)
                    | (VFCOMP_STORE_0 << 20)
                    | (VFCOMP_STORE_0 << 16);
                push(batch_dwords, &mut cursor, vertex_element0_dw0)?;
                push(batch_dwords, &mut cursor, vertex_element0_dw1)?;
                let active_rectlist_i16 = matches!(
                    backend_probe_mode,
                    BackendProbeMode::RasterWmInputOaScreenSpaceRectListMesaNoVsEarlyBackend
                );
                push(
                    batch_dwords,
                    &mut cursor,
                    (if active_rectlist_i16 {
                        SURFACE_FORMAT_R16G16_SSCALED
                    } else {
                        SURFACE_FORMAT_R32G32B32_FLOAT
                    } << 16)
                        | (1 << 25),
                )?;
                push(
                    batch_dwords,
                    &mut cursor,
                    (VFCOMP_STORE_SRC << 28)
                        | (VFCOMP_STORE_SRC << 24)
                        | (if active_rectlist_i16 {
                            VFCOMP_STORE_0
                        } else {
                            VFCOMP_STORE_SRC
                        } << 20)
                        | (VFCOMP_STORE_1_FP << 16),
                )?;
            }
            StreamoutProofExperiment::HeaderAndPositionSlots01 => {
                vertex_element0_dw0 = (SURFACE_FORMAT_R32G32B32A32_UINT << 16) | (1 << 25);
                vertex_element0_dw1 = (VFCOMP_STORE_SRC << 28)
                    | (VFCOMP_STORE_SRC << 24)
                    | (VFCOMP_STORE_SRC << 20)
                    | (VFCOMP_STORE_SRC << 16);
                push(batch_dwords, &mut cursor, vertex_element0_dw0)?;
                push(batch_dwords, &mut cursor, vertex_element0_dw1)?;
                push(
                    batch_dwords,
                    &mut cursor,
                    16 | (SURFACE_FORMAT_R32G32B32_FLOAT << 16) | (1 << 25),
                )?;
                push(
                    batch_dwords,
                    &mut cursor,
                    (VFCOMP_STORE_SRC << 28)
                        | (VFCOMP_STORE_SRC << 24)
                        | (VFCOMP_STORE_SRC << 20)
                        | (VFCOMP_STORE_1_FP << 16),
                )?;
            }
            StreamoutProofExperiment::MesaNoVsRectlist => {
                vertex_element0_dw0 =
                    (SURFACE_FORMAT_R32G32B32A32_FLOAT << 16) | (1 << 25) | (1 << 26);
                vertex_element0_dw1 = (VFCOMP_STORE_SRC << 28)
                    | (VFCOMP_STORE_0 << 24)
                    | (VFCOMP_STORE_0 << 20)
                    | (VFCOMP_STORE_0 << 16);
                push(batch_dwords, &mut cursor, vertex_element0_dw0)?;
                push(batch_dwords, &mut cursor, vertex_element0_dw1)?;
                push(
                    batch_dwords,
                    &mut cursor,
                    (SURFACE_FORMAT_R32G32B32_FLOAT << 16) | (1 << 25),
                )?;
                push(
                    batch_dwords,
                    &mut cursor,
                    (VFCOMP_STORE_SRC << 28)
                        | (VFCOMP_STORE_SRC << 24)
                        | (VFCOMP_STORE_SRC << 20)
                        | (VFCOMP_STORE_1_FP << 16),
                )?;
                push(
                    batch_dwords,
                    &mut cursor,
                    16 | (SURFACE_FORMAT_R32G32B32A32_FLOAT << 16) | (1 << 25) | (1 << 26),
                )?;
                push(
                    batch_dwords,
                    &mut cursor,
                    (VFCOMP_STORE_SRC << 28)
                        | (VFCOMP_STORE_SRC << 24)
                        | (VFCOMP_STORE_SRC << 20)
                        | (VFCOMP_STORE_SRC << 16),
                )?;
            }
        }
    } else {
        let vertex_element_format = front_end_contract
            .vertex_element_format_override
            .unwrap_or(SURFACE_FORMAT_R32G32B32_FLOAT);
        vertex_element0_dw0 = (vertex_element_format << 16) | (1 << 25);
        vertex_element0_dw1 = if vertex_element_format == SURFACE_FORMAT_R32G32B32A32_FLOAT {
            (VFCOMP_STORE_SRC << 28)
                | (VFCOMP_STORE_SRC << 24)
                | (VFCOMP_STORE_SRC << 20)
                | (VFCOMP_STORE_SRC << 16)
        } else {
            (VFCOMP_STORE_SRC << 28)
                | (VFCOMP_STORE_SRC << 24)
                | (VFCOMP_STORE_SRC << 20)
                | (VFCOMP_STORE_1_FP << 16)
        };
        push(batch_dwords, &mut cursor, vertex_element0_dw0)?;
        push(batch_dwords, &mut cursor, vertex_element0_dw1)?;
    }
    let real_vs_vertex_element_format = front_end_contract
        .vertex_element_format_override
        .unwrap_or(SURFACE_FORMAT_R32G32B32_FLOAT);
    intel_render_focus_log!(
        "intel/render: probe-vf-vue-contract vf_synthesized_vue={} experiment={} ve_count={} vue_contract={} header_slot={} position_slot={} position_format={} position_components={} vertex_stride={} note=clip_sf_requires_valid_xyzw_position\n",
        vf_synthesized_vue as u8,
        streamout_experiment.label(),
        vf_vertex_element_count,
        if vf_synthesized_vue {
            streamout_experiment.vf_slot_contract()
        } else {
            "shader-output"
        },
        if vf_synthesized_vue {
            match streamout_experiment {
                StreamoutProofExperiment::PositionSlot0 => "none",
                StreamoutProofExperiment::PositionSlot0Xyzw => "none",
                StreamoutProofExperiment::PositionSlot1 => "zero4",
                StreamoutProofExperiment::HeaderAndPositionSlots01 => "source-offset0",
                StreamoutProofExperiment::MesaNoVsRectlist => "mesa-sgvs-slot0",
            }
        } else {
            "shader-output"
        },
        if vf_synthesized_vue {
            match streamout_experiment {
                StreamoutProofExperiment::PositionSlot0 => "slot0=xyz+forced-w1",
                StreamoutProofExperiment::PositionSlot0Xyzw => "slot0=xyzw",
                StreamoutProofExperiment::PositionSlot1 => "slot1=xyz+forced-w1",
                StreamoutProofExperiment::HeaderAndPositionSlots01 => {
                    "slot1=offset16-xyz+forced-w1"
                }
                StreamoutProofExperiment::MesaNoVsRectlist => "slot1=xyz+forced-w1",
            }
        } else {
            "vertex-shader"
        },
        if !vf_synthesized_vue && real_vs_vertex_element_format == SURFACE_FORMAT_R32G32B32A32_FLOAT
        {
            "R32G32B32A32_FLOAT"
        } else if matches!(
            (streamout_experiment, backend_probe_mode),
            (
                StreamoutProofExperiment::PositionSlot1,
                BackendProbeMode::RasterWmInputOaScreenSpaceRectListMesaNoVsEarlyBackend
            )
        ) {
            "R16G16_SSCALED"
        } else if matches!(streamout_experiment, StreamoutProofExperiment::PositionSlot0Xyzw) {
            "R32G32B32A32_FLOAT"
        } else {
            "R32G32B32_FLOAT"
        },
        if !vf_synthesized_vue && real_vs_vertex_element_format == SURFACE_FORMAT_R32G32B32A32_FLOAT
        {
            "src,src,src,src"
        } else if matches!(
            (streamout_experiment, backend_probe_mode),
            (
                StreamoutProofExperiment::PositionSlot1,
                BackendProbeMode::RasterWmInputOaScreenSpaceRectListMesaNoVsEarlyBackend
            )
        ) {
            "src,src,0,1.0"
        } else if matches!(streamout_experiment, StreamoutProofExperiment::PositionSlot0Xyzw) {
            "src,src,src,src"
        } else {
            "src,src,src,1.0"
        },
        draw.vertex_stride,
    );

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
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_MESA_ACTIVE)?;
    push(batch_dwords, &mut cursor, MESA_ACTIVE_VF_CUT_INDEX)?;
    intel_render_focus_log!(
        "intel/render: probe-vf-state emit=initial cmd=0x{:08X} cut_index=0x{:08X} component_packing=1 source=mesa-active-triangle\n",
        CMD_3DSTATE_VF_MESA_ACTIVE,
        MESA_ACTIVE_VF_CUT_INDEX,
    );
    let vf_sgvs_dw1 = if vf_synthesized_vue
        && matches!(streamout_experiment, StreamoutProofExperiment::MesaNoVsRectlist)
    {
        0xA000_0000
    } else {
        0
    };
    log_batch_offset(cursor, "3DSTATE_VF_SGVS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_SGVS)?;
    push(batch_dwords, &mut cursor, vf_sgvs_dw1)?;
    log_batch_offset(cursor, "3DSTATE_VF_SGVS_2");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_SGVS_2)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    for vertex_element_index in 0..vf_vertex_element_count {
        log_batch_offset(cursor, "3DSTATE_VF_INSTANCING");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_INSTANCING)?;
        push(batch_dwords, &mut cursor, vertex_element_index as u32)?;
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "3DSTATE_VF_TOPOLOGY");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_TOPOLOGY)?;
    push(batch_dwords, &mut cursor, primitive_topology)?;
    log_batch_offset(cursor, "MI_STORE_DATA_IMM post-vf");
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
    let programmed_vs_urb_entries = front_end_contract
        .vs_urb_entries_override
        .or_else(|| backend_probe_mode.vs_urb_entries_override())
        .unwrap_or(TRIANGLE_VS_URB_ENTRIES);
    let programmed_vs_urb_start = front_end_contract
        .vs_urb_start_override
        .unwrap_or(TRIANGLE_VS_URB_START);
    let disabled_stage_urb_alloc_dw1 =
        (programmed_vs_urb_start << 10) | (programmed_vs_urb_start << 21);

    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_VS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_VS)?;
    push(
        batch_dwords,
        &mut cursor,
        // Gfx12 encodes URB allocation size as "size in 64B units minus 1".
        // A position-only VUE is one 64B slot, so the programmed value must
        // be 0 rather than 1 or clipper sees the wrong VS allocation contract.
        (programmed_vs_urb_output_length.saturating_sub(1) as u32)
            | (programmed_vs_urb_start << 10)
            | (programmed_vs_urb_start << 21),
    )?;
    push(batch_dwords, &mut cursor, programmed_vs_urb_entries | (programmed_vs_urb_entries << 16))?;

    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_HS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_HS)?;
    push(batch_dwords, &mut cursor, disabled_stage_urb_alloc_dw1)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_DS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_DS)?;
    push(batch_dwords, &mut cursor, disabled_stage_urb_alloc_dw1)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_GS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_GS)?;
    push(batch_dwords, &mut cursor, disabled_stage_urb_alloc_dw1)?;
    push(batch_dwords, &mut cursor, 0)?;

    if vf_synthesized_vue {
        log_batch_offset(cursor, "3DSTATE_VS disabled");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VS)?;
        for _ in 0..8 {
            push(batch_dwords, &mut cursor, 0)?;
        }
    } else {
        let vs_accesses_uav = shader_layout.vs.accesses_uav;
        let vs_dw3 = ((pipeline.vs.meta.kernel.binding_table_entry_count as u32) << 18)
            | ((vs_accesses_uav as u32) << 12)
            | (sampler_count_encoding(pipeline.vs.meta.kernel.sampler_count) << 27);
        let applied_vs_grf_start = front_end_contract
            .vs_dispatch_grf_start_override
            .map(u32::from)
            .unwrap_or_else(|| {
                triangle_vs_dispatch_grf_start_register(pipeline.vs.meta.kernel.grf_start_register)
            });
        let vs_dw6 = ((front_end_contract.vs_urb_read_offset as u32) << 4)
            | ((front_end_contract.vs_urb_read_length as u32) << 11)
            | (applied_vs_grf_start << 20);
        let applied_vs_max_threads_field = front_end_contract
            .vs_max_threads_field_override
            .unwrap_or_else(|| {
                triangle_vs_max_threads_field(warm.device_id, pipeline.vs.meta.max_threads)
            });
        let vs_dw7 = 1
            | (1 << 2)
            | (1 << 10)
            | ((front_end_contract.vs_simd8_single_instance_dispatch as u32) << 9)
            | (applied_vs_max_threads_field << 22);
        let vs_dw8 = front_end_contract
            .vs_dw8_override
            .unwrap_or((programmed_vs_urb_output_length.saturating_sub(1) as u32) << 16);
        let vs_output_read_offset = (vs_dw8 >> 21) & 0x3F;
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
        intel_render_focus_log!(
            "intel/render: probe-vs ksp=0x{:08X} dw3=0x{:08X} accesses_uav={} simd8_single={} dw6=0x{:08X} dw7=0x{:08X} dw8=0x{:08X} dw8_override={} vs_output_read_offset={} baked_max_threads={} applied_max_threads_field={} baked_urb_out_len={} programmed_urb_out_len={} programmed_urb_entries={} programmed_urb_start={} vs_urb_read_offset={} vs_urb_read_length={} baked_grf_start={} applied_grf_start={} dispatch={:?} note=vs_dw8_contract_or_urb_output_length_minus_one\n",
            vs_ksp_offset & !0x3F,
            vs_dw3,
            vs_accesses_uav as u8,
            front_end_contract.vs_simd8_single_instance_dispatch as u8,
            vs_dw6,
            vs_dw7,
            vs_dw8,
            front_end_contract.vs_dw8_override.is_some() as u8,
            vs_output_read_offset,
            pipeline.vs.meta.max_threads,
            applied_vs_max_threads_field,
            baked_vs_urb_output_length,
            programmed_vs_urb_output_length,
            programmed_vs_urb_entries,
            programmed_vs_urb_start,
            front_end_contract.vs_urb_read_offset,
            front_end_contract.vs_urb_read_length,
            pipeline.vs.meta.kernel.grf_start_register,
            applied_vs_grf_start,
            pipeline.vs.meta.kernel.dispatch_mode,
        );
        intel_render_verbose_log!(
            "intel/render: probe-vs-export note={} position_only={} generic_attrs=0 baked_urb_bytes={} programmed_urb_bytes={} expected_vue=header+position-only\n",
            crate::intel::shader::triangle_pipeline_note(),
            (pipeline.ps.meta.num_varying_inputs == 0) as u8,
            (baked_vs_urb_output_length as u32) * 64,
            (programmed_vs_urb_output_length as u32) * 64,
        );
    }
    log_batch_offset(cursor, "MI_STORE_DATA_IMM post-vs");
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
    log_batch_offset(cursor, "3DSTATE_GS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_GS)?;
    for _ in 0..9 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    intel_render_focus_log!(
        "intel/render: probe-thread-stage-order vs=programmed hs=disabled te=disabled ds=disabled gs=disabled sol={} clip=next sf=after-clip note=packet-order-matches-main-pipe-before-raster\n",
        batch_mode.streamout_enabled() as u8,
    );
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
        let streamout_decl_dword0 = streamout_experiment.so_decl_stream_offsets();
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
        if matches!(streamout_experiment, StreamoutProofExperiment::HeaderAndPositionSlots01) {
            push(batch_dwords, &mut cursor, streamout_decl_dword4)?;
            push(batch_dwords, &mut cursor, streamout_decl_dword5)?;
        }
        crate::log!(
            "intel/render: probe-streamout-decl experiment={} read_offset={} read_length={} so_pitch={} decl=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] vs_position_only={} ps_varyings={} generic_attrs=0 compatible={}\n",
            streamout_experiment.label(),
            streamout_experiment.vertex_read_offset(),
            streamout_experiment.vertex_read_length_field(),
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
            "intel/render: probe-streamout-config experiment={} so[function_enable={} statistics_enable={} rendering_disable={} sol_write_enable={} buffer0_enable={} render_stream={} reorder={} read_offset={} read_length_field={} buffer0_pitch={}] sobuf0[enable={} write_enable={} offset_addr_enable={} offset_mode={} mocs=0x{:X} surface=0x{:X} size_dwords=0x{:X} stream_offset=0x{:08X}] slot_contract={}\n",
            streamout_experiment.label(),
            (streamout_dw1 >> 31) & 0x1,
            (streamout_dw1 >> 25) & 0x1,
            (streamout_dw1 >> 30) & 0x1,
            ((streamout_dw1 >> 31) & 0x1) & (1 ^ ((streamout_dw1 >> 30) & 0x1)),
            (streamout_dw1 >> 8) & 0x1,
            (streamout_dw1 >> 27) & 0x3,
            (streamout_dw1 >> 26) & 0x1,
            (streamout_dw2 >> 5) & 0x1F,
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

    // Program explicit null depth/stencil state instead of relying on any
    // inherited render context defaults before the first primitive launches.
    let depth_buffer_dw1 = (DEPTH_SURFACE_FORMAT_D32_FLOAT << 24) | (SURFTYPE_NULL << 29);
    let depth_buffer_dw5 = RENDER_MOCS;
    log_batch_offset(cursor, "3DSTATE_CLEAR_PARAMS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_CLEAR_PARAMS)?;
    push(batch_dwords, &mut cursor, 0.0f32.to_bits())?;
    push(batch_dwords, &mut cursor, 0)?;

    log_batch_offset(cursor, "3DSTATE_DEPTH_BUFFER");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_DEPTH_BUFFER)?;
    push(batch_dwords, &mut cursor, depth_buffer_dw1)?;
    push_addr(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, depth_buffer_dw5)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, 0)?;

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

    // Drawing rectangle participates in the fixed-function raster/clip window.
    // Program it before CLIP/SF/RASTER so the main pipe cannot consume stale
    // inherited bounds while setting up the primitive.
    log_batch_offset(cursor, "3DSTATE_DRAWING_RECTANGLE pre-raster");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_DRAWING_RECTANGLE)?;
    push(batch_dwords, &mut cursor, 0)?;
    push(batch_dwords, &mut cursor, drawing_rectangle_dw2)?;
    push(batch_dwords, &mut cursor, 0)?;

    // Raster setup consumes the multisample/sample-mask contract, so establish
    // it before CLIP/SF/RASTER instead of relying on later state writes.
    log_batch_offset(cursor, "3DSTATE_MULTISAMPLE pre-raster");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_MULTISAMPLE)?;
    push(batch_dwords, &mut cursor, multisample_dw1)?;
    log_batch_offset(cursor, "3DSTATE_SAMPLE_MASK pre-raster");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLE_MASK)?;
    push(batch_dwords, &mut cursor, sample_mask_dw1)?;

    // Several WM/PSD inputs are backend state packets, but SF/RASTER may
    // consume their derived contract while preparing the object for WM. Mirror
    // Mesa's simple path by front-loading the boring backend gates before the
    // fixed-function front-end, then keep the later writes as an idempotent
    // refresh before PS.
    if early_backend_before_clip_sf_raster {
        log_batch_offset(cursor, "3DSTATE_PS_BLEND pre-raster");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PS_BLEND)?;
        push(batch_dwords, &mut cursor, ps_blend_dw1)?;
        log_batch_offset(cursor, "3DSTATE_WM_DEPTH_STENCIL pre-raster");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_WM_DEPTH_STENCIL)?;
        push(batch_dwords, &mut cursor, wm_depth_stencil_dw1)?;
        push(batch_dwords, &mut cursor, wm_depth_stencil_dw2)?;
        push(batch_dwords, &mut cursor, wm_depth_stencil_dw3)?;
        log_batch_offset(cursor, "3DSTATE_DEPTH_BOUNDS pre-raster");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_DEPTH_BOUNDS)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0.0f32.to_bits())?;
        push(batch_dwords, &mut cursor, 1.0f32.to_bits())?;
        // WM_HZ_OP derives WM_INT state from raster/scissor/sample inputs. Keep it
        // out of the pre-raster block so the later post-raster write is the first
        // authoritative programming point for that contract.
        log_batch_offset(cursor, "3DSTATE_WM pre-raster");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_WM)?;
        push(batch_dwords, &mut cursor, wm_dw1)?;
    }
    intel_render_focus_log!(
        "intel/render: probe-state-order backend={} early_backend_before_clip_sf_raster={} early_ps_blend=0x{:08X} early_wm_depth_stencil=[0x{:08X},0x{:08X},0x{:08X}] early_wm_hz_op=deferred-until-after-raster wm_hz_op_late=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] wm_hz_op_packet={} wm_hz_op_len_dw=4 wm_hz_op_total_dwords=6 early_wm=0x{:08X} later_refresh_before_ps={} hypothesis=wm_int_raster_scissor_sample_ordering\n",
        backend_probe_mode.label(),
        early_backend_before_clip_sf_raster as u8,
        ps_blend_dw1,
        wm_depth_stencil_dw1,
        wm_depth_stencil_dw2,
        wm_depth_stencil_dw3,
        wm_hz_op_dw1,
        wm_hz_op_dw2,
        wm_hz_op_dw3,
        wm_hz_op_dw4,
        wm_hz_op_dw5,
        if mesa_simple_order && (mesa_order_emit_wm_hz_op || mesa_simple_emit_wm_hz_op_after_raster)
        {
            "emitted"
        } else if backend_probe_mode.skip_wm_hz_op_packet() {
            "skipped"
        } else if mesa_simple_order {
            "skipped-mesa-simple"
        } else {
            "emitted"
        },
        wm_dw1,
        ((!mesa_simple_order) || mesa_order_emit_wm_hz_op) as u8,
    );

    if backend_probe_mode.pre_clip_sbe_ps_state() {
        log_batch_offset(cursor, "3DSTATE_SBE pre-clip");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE)?;
        push(batch_dwords, &mut cursor, sbe_dw1)?;
        push(batch_dwords, &mut cursor, sbe_dw2)?;
        push(batch_dwords, &mut cursor, sbe_dw3)?;
        push(batch_dwords, &mut cursor, sbe_dw4)?;
        push(batch_dwords, &mut cursor, sbe_dw5)?;

        if emit_sbe_swiz_packet {
            log_batch_offset(cursor, "3DSTATE_SBE_SWIZ pre-clip");
            push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE_SWIZ)?;
            for _ in 0..10 {
                push(batch_dwords, &mut cursor, 0)?;
            }
        }

        log_batch_offset(cursor, "3DSTATE_PS pre-clip");
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

        log_batch_offset(cursor, "3DSTATE_PS_EXTRA pre-clip");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PS_EXTRA)?;
        push(batch_dwords, &mut cursor, ps_extra_dw1)?;

        intel_render_focus_log!(
            "intel/render: probe-preclip-sbe-ps-state backend={} emitted=1 sbe=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] sbe_read_offset={} sbe_read_length={} ps_dw6=0x{:08X} ps_extra=0x{:08X} hypothesis=sf_wm_scan_conversion_needs_backend_contract_before_clip_sf_raster\n",
            backend_probe_mode.label(),
            sbe_dw1,
            sbe_dw2,
            sbe_dw3,
            sbe_dw4,
            sbe_dw5,
            sbe_decoded_read_offset,
            sbe_decoded_read_length,
            ps_dw6,
            ps_extra_dw1,
        );
    }

    if backend_probe_mode.pre_clip_raster_state() {
        log_batch_offset(cursor, "3DSTATE_RASTER pre-clip");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_RASTER)?;
        push(batch_dwords, &mut cursor, raster_dw1)?;
        push(batch_dwords, &mut cursor, raster_dw2)?;
        push(batch_dwords, &mut cursor, raster_dw3)?;
        push(batch_dwords, &mut cursor, raster_dw4)?;
        intel_render_focus_log!(
            "intel/render: probe-preclip-raster-state backend={} emitted=1 raster_dw=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] hypothesis=clip_and_sf_may_need_raster_scissor_winding_z_state_before_clip_packet\n",
            backend_probe_mode.label(),
            raster_dw1,
            raster_dw2,
            raster_dw3,
            raster_dw4,
        );
    }

    log_batch_offset(cursor, "3DSTATE_CLIP");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_CLIP)?;
    push(batch_dwords, &mut cursor, clip_dw1)?;
    push(batch_dwords, &mut cursor, clip_dw2)?;
    push(batch_dwords, &mut cursor, clip_dw3)?;
    log_batch_offset(cursor, "MI_STORE_DATA_IMM post-clip");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_CLIP_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_CLIP,
    )?;

    if oracle_active_reference_order {
        log_batch_offset(cursor, "3DSTATE_STREAMOUT oracle-active-off-after-clip");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_STREAMOUT)?;
        for _ in 0..4 {
            push(batch_dwords, &mut cursor, 0)?;
        }
        log_batch_offset(cursor, "3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP oracle-active-after-clip");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP)?;
        push(batch_dwords, &mut cursor, probe_state.sf_clip_viewport_offset_bytes)?;
        log_batch_offset(cursor, "3DSTATE_VIEWPORT_STATE_POINTERS_CC oracle-active-after-clip");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VIEWPORT_STATE_POINTERS_CC)?;
        push(batch_dwords, &mut cursor, probe_state.cc_viewport_offset_bytes)?;
        log_batch_offset(cursor, "3DSTATE_VF_TOPOLOGY oracle-active-before-sf");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_TOPOLOGY)?;
        push(batch_dwords, &mut cursor, primitive_topology)?;
        log_batch_offset(cursor, "3DSTATE_VERTEX_ELEMENTS oracle-active-before-sf");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VERTEX_ELEMENTS_1)?;
        push(batch_dwords, &mut cursor, vertex_element0_dw0)?;
        push(batch_dwords, &mut cursor, vertex_element0_dw1)?;
        log_batch_offset(cursor, "3DSTATE_TE oracle-active-before-sf");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_TE)?;
        for _ in 0..4 {
            push(batch_dwords, &mut cursor, 0)?;
        }
        log_batch_offset(cursor, "3DSTATE_GS oracle-active-before-sf");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_GS)?;
        for _ in 0..9 {
            push(batch_dwords, &mut cursor, 0)?;
        }
        intel_render_focus_log!(
            "intel/render: probe-oracle-active-before-sf backend={} emitted=1 placement=after-clip-before-sf streamout_off=1 sf_clip_vp=0x{:X} cc_vp=0x{:X} vf_topology=0x{:08X} ve0=[0x{:08X},0x{:08X}] te_gs_disabled=1 source=mesa-rendercopy-active-order hypothesis=SF_consumes_late_CLIP_viewport_and_VF_contract\n",
            backend_probe_mode.label(),
            probe_state.sf_clip_viewport_offset_bytes,
            probe_state.cc_viewport_offset_bytes,
            primitive_topology,
            vertex_element0_dw0,
            vertex_element0_dw1,
        );
    }

    if backend_probe_mode.primitive_replication_before_sf() {
        log_batch_offset(cursor, "3DSTATE_PRIMITIVE_REPLICATION after-clip-before-sf");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PRIMITIVE_REPLICATION)?;
        push(batch_dwords, &mut cursor, primitive_replication_dw1)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        intel_render_focus_log!(
            "intel/render: probe-prim-repl-xe-svg-order backend={} emitted=1 placement=after-clip-before-sf dw1=0x{:08X} source=linux-xe-default-lrc-svg-state hypothesis=clear_replication_state_before_sf_builds_wm_object\n",
            backend_probe_mode.label(),
            primitive_replication_dw1,
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
    if oracle_active_reference_order {
        log_batch_offset(cursor, "3DSTATE_VIEWPORT_STATE_POINTERS oracle-active-after-raster");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VIEWPORT_STATE_POINTERS)?;
        push(batch_dwords, &mut cursor, 0)?;
        log_batch_offset(cursor, "3DSTATE_CC_STATE_POINTERS oracle-active-after-raster");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_CC_STATE_POINTERS)?;
        push(batch_dwords, &mut cursor, probe_state.color_calc_state_offset_bytes | 1)?;
        intel_render_focus_log!(
            "intel/render: probe-oracle-active-after-raster backend={} emitted=1 generic_viewport=0 cc_state=0x{:X} source=mesa-rendercopy-active-order hypothesis=WM_CC_state_lifetime_before_sample_mask_depth\n",
            backend_probe_mode.label(),
            probe_state.color_calc_state_offset_bytes | 1,
        );
    }
    if device_is_gfx125(warm.device_id) && !skip_tbimr_tile_pass_info_after_raster {
        log_batch_offset(cursor, "3DSTATE_TBIMR_TILE_PASS_INFO zero-disable");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_TBIMR_TILE_PASS_INFO)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        intel_render_focus_log!(
            "intel/render: probe-tbimr-tile-pass-info backend={} emitted=1 placement=after-raster-before-wm-hz-op dw=[0x{:08X},0x00000000,0x00000000,0x00000000] source=linux-xe-default-lrc-svg-state-and-mesa-gen125-xml hypothesis=clear_stale_tbimr_state_at_sf_raster_to_wm_boundary\n",
            backend_probe_mode.label(),
            CMD_3DSTATE_TBIMR_TILE_PASS_INFO,
        );
    } else if device_is_gfx125(warm.device_id) {
        intel_render_focus_log!(
            "intel/render: probe-tbimr-tile-pass-info backend={} emitted=0 reason=active-order-skips-xe-default-lrc-tbimr-packet-at-sf-raster-boundary\n",
            backend_probe_mode.label(),
        );
    }
    if !mesa_simple_order {
        log_batch_offset(cursor, "3DSTATE_PRIMITIVE_REPLICATION");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PRIMITIVE_REPLICATION)?;
        push(batch_dwords, &mut cursor, primitive_replication_dw1)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
    }
    log_batch_offset(cursor, "MI_STORE_DATA_IMM post-raster");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_RASTER_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_RASTER,
    )?;

    if mesa_simple_order && (mesa_order_emit_wm_hz_op || mesa_simple_emit_wm_hz_op_after_raster) {
        log_batch_offset(cursor, "3DSTATE_WM_HZ_OP mesa-order");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_WM_HZ_OP)?;
        push(batch_dwords, &mut cursor, wm_hz_op_dw1)?;
        push(batch_dwords, &mut cursor, wm_hz_op_dw2)?;
        push(batch_dwords, &mut cursor, wm_hz_op_dw3)?;
        push(batch_dwords, &mut cursor, wm_hz_op_dw4)?;
        push(batch_dwords, &mut cursor, wm_hz_op_dw5)?;
        intel_render_focus_log!(
            "intel/render: probe-wm-hz-op-packet backend={} emitted=1 placement=after-raster-before-sbe-wm forced_simple_emit={} len_dw=4 total_dwords=6 dw=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] hypothesis=gfx125_wm_int_reset_packet_length_and_post_raster_precondition\n",
            backend_probe_mode.label(),
            mesa_simple_emit_wm_hz_op_after_raster as u8,
            wm_hz_op_dw1,
            wm_hz_op_dw2,
            wm_hz_op_dw3,
            wm_hz_op_dw4,
            wm_hz_op_dw5,
        );
    }

    if backend_probe_mode.mesa_active_block_state() {
        log_batch_offset(cursor, "3DSTATE_SAMPLE_MASK mesa-active");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLE_MASK)?;
        push(batch_dwords, &mut cursor, sample_mask_dw1)?;

        log_batch_offset(cursor, "3DSTATE_WM_DEPTH_STENCIL mesa-active");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_WM_DEPTH_STENCIL)?;
        push(batch_dwords, &mut cursor, wm_depth_stencil_dw1)?;
        push(batch_dwords, &mut cursor, wm_depth_stencil_dw2)?;
        push(batch_dwords, &mut cursor, wm_depth_stencil_dw3)?;

        log_batch_offset(cursor, "PIPE_CONTROL mesa-active-handoff");
        push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;

        log_batch_offset(cursor, "3DSTATE_VF mesa-active-handoff");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_MESA_ACTIVE)?;
        push(batch_dwords, &mut cursor, MESA_ACTIVE_VF_CUT_INDEX)?;

        log_batch_offset(cursor, "3DSTATE_PRIMITIVE_REPLICATION mesa-active");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PRIMITIVE_REPLICATION)?;
        push(batch_dwords, &mut cursor, primitive_replication_dw1)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;

        log_batch_offset(cursor, "3DSTATE_WM mesa-active");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_WM)?;
        push(batch_dwords, &mut cursor, wm_dw1)?;

        log_batch_offset(cursor, "3DSTATE_PS_BLEND mesa-active");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PS_BLEND)?;
        push(batch_dwords, &mut cursor, ps_blend_dw1)?;

        log_batch_offset(cursor, "3DSTATE_BLEND_STATE_POINTERS mesa-active-late");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_BLEND_STATE_POINTERS)?;
        push(
            batch_dwords,
            &mut cursor,
            blend_mode.blend_state_pointer_dword(probe_state.blend_state_offset_bytes),
        )?;

        let active_binding_table_pointer_offset = 0x20u32;

        log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POINTERS_PS mesa-active-late");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_PS)?;
        push(batch_dwords, &mut cursor, active_binding_table_pointer_offset)?;

        let late_sbe_ps_refresh = backend_probe_mode.late_sbe_ps_refresh_after_mesa_active_block();
        intel_render_focus_log!(
            "intel/render: probe-mesa-active-block backend={} stamped=1 clip=[0x{:08X},0x{:08X},0x{:08X}] sf=[0x{:08X},0x{:08X},0x{:08X}] raster=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] sample_mask=0x{:08X} wm_depth=[0x{:08X},0x{:08X},0x{:08X}] prim_repl=0x{:08X} wm=0x{:08X} ps_blend=0x{:08X} late_blend_ptr=0x{:X} late_ps_bt=0x{:X} streamout_off_after_clip={} skipped_tbimr={} skipped_late_sbe_ps={} skipped_wm_hz_op=1 sequence=preclip-sbe-ps-ps_extra-clip{}-sf-raster-sample_mask-wm_depth_stencil-pipe_control-vf-primitive_replication-wm-ps_blend-blend_ptr-ps_bt{} hypothesis=mesa_active_block_state_lifetime_into_wm_int\n",
            backend_probe_mode.label(),
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
            sample_mask_dw1,
            wm_depth_stencil_dw1,
            wm_depth_stencil_dw2,
            wm_depth_stencil_dw3,
            primitive_replication_dw1,
            wm_dw1,
            ps_blend_dw1,
            blend_mode.blend_state_pointer_dword(probe_state.blend_state_offset_bytes),
            active_binding_table_pointer_offset,
            oracle_active_reference_order as u8,
            skip_tbimr_tile_pass_info_after_raster as u8,
            (!late_sbe_ps_refresh) as u8,
            if oracle_active_reference_order {
                "-streamout_off"
            } else {
                ""
            },
            if late_sbe_ps_refresh {
                "-late-sbe-ps-ps_extra"
            } else {
                ""
            },
        );

        if late_sbe_ps_refresh {
            log_batch_offset(cursor, "3DSTATE_SBE mesa-active-late-refresh");
            push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE)?;
            push(batch_dwords, &mut cursor, sbe_dw1)?;
            push(batch_dwords, &mut cursor, sbe_dw2)?;
            push(batch_dwords, &mut cursor, sbe_dw3)?;
            push(batch_dwords, &mut cursor, sbe_dw4)?;
            push(batch_dwords, &mut cursor, sbe_dw5)?;

            if emit_sbe_swiz_packet {
                log_batch_offset(cursor, "3DSTATE_SBE_SWIZ mesa-active-late-refresh");
                push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE_SWIZ)?;
                for _ in 0..10 {
                    push(batch_dwords, &mut cursor, 0)?;
                }
            }

            log_batch_offset(cursor, "3DSTATE_PS mesa-active-late-refresh");
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

            log_batch_offset(cursor, "3DSTATE_PS_EXTRA mesa-active-late-refresh");
            push(batch_dwords, &mut cursor, CMD_3DSTATE_PS_EXTRA)?;
            push(batch_dwords, &mut cursor, ps_extra_dw1)?;

            intel_render_focus_log!(
                "intel/render: probe-mesa-active-late-sbe-ps backend={} emitted=1 sbe=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] sbe_swiz={} ps_dw3=0x{:08X} ps_dw6=0x{:08X} ps_extra=0x{:08X} hypothesis=late_ps_state_lifetime_into_psd_launch_frontier\n",
                backend_probe_mode.label(),
                sbe_dw1,
                sbe_dw2,
                sbe_dw3,
                sbe_dw4,
                sbe_dw5,
                emit_sbe_swiz_packet as u8,
                ps_dw3,
                ps_dw6,
                ps_extra_dw1,
            );
        }
    } else {
        log_batch_offset(cursor, "3DSTATE_SBE");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE)?;
        push(batch_dwords, &mut cursor, sbe_dw1)?;
        push(batch_dwords, &mut cursor, sbe_dw2)?;
        push(batch_dwords, &mut cursor, sbe_dw3)?;
        push(batch_dwords, &mut cursor, sbe_dw4)?;
        push(batch_dwords, &mut cursor, sbe_dw5)?;

        if emit_sbe_swiz_packet {
            // Gen12/Xe-LP keeps attribute swizzle state in a separate packet.
            log_batch_offset(cursor, "3DSTATE_SBE_SWIZ");
            push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE_SWIZ)?;
            for _ in 0..10 {
                push(batch_dwords, &mut cursor, 0)?;
            }
        }

        log_batch_offset(cursor, "3DSTATE_WM");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_WM)?;
        push(batch_dwords, &mut cursor, wm_dw1)?;

        if mesa_simple_order {
            intel_render_focus_log!(
                "intel/render: probe-mesa-simple-order backend={} exact_frontier_order=1 skipped_late_backend_refresh=1 skipped_wm_hz_op={} sequence={}\n",
                backend_probe_mode.label(),
                (!mesa_order_emit_wm_hz_op) as u8,
                if mesa_order_emit_wm_hz_op {
                    "clip-sf-raster-wm_hz_op-sbe-wm-ps-ps_extra-primitive_replication"
                } else {
                    "clip-sf-raster-sbe-wm-ps-ps_extra-primitive_replication"
                },
            );
        } else {
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
            push(batch_dwords, &mut cursor, probe_state.color_calc_state_offset_bytes | 1)?;

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

            // Keep the WM/HZ side of the contract freshly programmed as well. SF/RASTER
            // consume these bounds early, but WM_INT also derives state from the same
            // rectangle/sample/scissor inputs below the raster packets.
            log_batch_offset(cursor, "3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP late-wm");
            push(batch_dwords, &mut cursor, CMD_3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP)?;
            push(batch_dwords, &mut cursor, probe_state.sf_clip_viewport_offset_bytes)?;
            log_batch_offset(cursor, "3DSTATE_SCISSOR_STATE_POINTERS late-wm");
            push(batch_dwords, &mut cursor, CMD_3DSTATE_SCISSOR_STATE_POINTERS)?;
            push(batch_dwords, &mut cursor, probe_state.scissor_rect_offset_bytes)?;
            log_batch_offset(cursor, "3DSTATE_MULTISAMPLE late-wm");
            push(batch_dwords, &mut cursor, CMD_3DSTATE_MULTISAMPLE)?;
            push(batch_dwords, &mut cursor, multisample_dw1)?;
            log_batch_offset(cursor, "3DSTATE_SAMPLE_MASK late-wm");
            push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLE_MASK)?;
            push(batch_dwords, &mut cursor, sample_mask_dw1)?;
            log_batch_offset(cursor, "3DSTATE_DRAWING_RECTANGLE late-wm");
            push(batch_dwords, &mut cursor, CMD_3DSTATE_DRAWING_RECTANGLE)?;
            push(batch_dwords, &mut cursor, 0)?;
            push(batch_dwords, &mut cursor, drawing_rectangle_dw2)?;
            push(batch_dwords, &mut cursor, 0)?;
            intel_render_focus_log!(
                "intel/render: probe-wm-late-bounds-refresh backend={} sf_viewport=0x{:X} scissor_ptr=0x{:X} draw_rect=[0,0..{},{}] sample_mask=0x{:X} multisample_dw=0x{:08X} placement=after-raster-before-wm-hz-op hypothesis=wm_int_bound_scissor_sample_contract\n",
                backend_probe_mode.label(),
                probe_state.sf_clip_viewport_offset_bytes,
                probe_state.scissor_rect_offset_bytes,
                draw_rect_max_x,
                draw_rect_max_y,
                sample_mask_dw1,
                multisample_dw1,
            );

            if backend_probe_mode.skip_wm_hz_op_packet() {
                intel_render_focus_log!(
                    "intel/render: probe-wm-hz-op-packet backend={} emitted=0 reason=normal-rendering-path-tests-whether-zero-hz-packet-itself-poisons-wm-int-boundary\n",
                    backend_probe_mode.label(),
                );
            } else {
                // Clear inherited WM_HZ_OP clear/resolve overrides so PS dispatch only
                // depends on the explicit probe state we log below.
                log_batch_offset(cursor, "3DSTATE_WM_HZ_OP");
                push(batch_dwords, &mut cursor, CMD_3DSTATE_WM_HZ_OP)?;
                push(batch_dwords, &mut cursor, wm_hz_op_dw1)?;
                push(batch_dwords, &mut cursor, wm_hz_op_dw2)?;
                push(batch_dwords, &mut cursor, wm_hz_op_dw3)?;
                push(batch_dwords, &mut cursor, wm_hz_op_dw4)?;
                push(batch_dwords, &mut cursor, wm_hz_op_dw5)?;
            }
        }

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

        log_batch_offset(cursor, "3DSTATE_PS_EXTRA");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PS_EXTRA)?;
        push(batch_dwords, &mut cursor, ps_extra_dw1)?;

        // Mesa emits the all-zero PRIMITIVE_REPLICATION disable in the late
        // pixel-state cluster. Keep our early clear, then restamp it here so SF/WM
        // does not inherit stale VP/RTAI replication state from packet ordering.
        log_batch_offset(cursor, "3DSTATE_PRIMITIVE_REPLICATION late-after-ps-extra");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PRIMITIVE_REPLICATION)?;
        push(batch_dwords, &mut cursor, primitive_replication_dw1)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        intel_render_focus_log!(
            "intel/render: probe-prim-repl-order backend={} early_after_raster={} late_after_ps_extra=1 late_dw1=0x{:08X} hypothesis=mesa_late_replication_disable_state_lifetime_into_wm\n",
            backend_probe_mode.label(),
            (!mesa_simple_order) as u8,
            primitive_replication_dw1,
        );
    }

    log_batch_offset(cursor, "MI_STORE_DATA_IMM post-ps-state");
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

    let skip_late_dummy_draw = matches!(
        backend_probe_mode,
        BackendProbeMode::RasterWmInputOaNdcMesaActiveBlockNoPrimRepl
    );
    if skip_late_dummy_draw {
        log_batch_offset(cursor, "skip 3DPRIMITIVE late-dummy");
        intel_render_focus_log!(
            "intel/render: probe-late-dummy-draw backend={} emitted=0 reason=oracle-active-path-has-no-dummy-between-state-and-real-primitive\n",
            backend_probe_mode.label(),
        );
    } else {
        // pdoane/osdev's Ivy Bridge bring-up emits a zero-vertex 3DPRIMITIVE after
        // switching to the 3D pipe/context and before the real triangle. This legacy
        // probe keeps the old priming draw for non-oracle variants.
        log_batch_offset(cursor, "3DPRIMITIVE dummy-after-pipeline-select");
        push(batch_dwords, &mut cursor, CMD_3DPRIMITIVE)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
    }

    log_batch_offset(cursor, "3DSTATE_VF late-mesa-active");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_MESA_ACTIVE)?;
    push(batch_dwords, &mut cursor, MESA_ACTIVE_VF_CUT_INDEX)?;
    intel_render_focus_log!(
        "intel/render: probe-vf-state emit=late-before-real-primitive cmd=0x{:08X} cut_index=0x{:08X} component_packing=1 source=mesa-active-triangle hypothesis=vf-component-packing-handoff-unblocks-post-vs-frontier\n",
        CMD_3DSTATE_VF_MESA_ACTIVE,
        MESA_ACTIVE_VF_CUT_INDEX,
    );

    if oracle_active_reference_order {
        let vs_accesses_uav = shader_layout.vs.accesses_uav;
        let vs_dw3 = ((pipeline.vs.meta.kernel.binding_table_entry_count as u32) << 18)
            | ((vs_accesses_uav as u32) << 12)
            | (sampler_count_encoding(pipeline.vs.meta.kernel.sampler_count) << 27);
        let applied_vs_grf_start = front_end_contract
            .vs_dispatch_grf_start_override
            .map(u32::from)
            .unwrap_or_else(|| {
                triangle_vs_dispatch_grf_start_register(pipeline.vs.meta.kernel.grf_start_register)
            });
        let vs_dw6 = ((front_end_contract.vs_urb_read_offset as u32) << 4)
            | ((front_end_contract.vs_urb_read_length as u32) << 11)
            | (applied_vs_grf_start << 20);
        let applied_vs_max_threads_field = front_end_contract
            .vs_max_threads_field_override
            .unwrap_or_else(|| {
                triangle_vs_max_threads_field(warm.device_id, pipeline.vs.meta.max_threads)
            });
        let vs_dw7 = 1 | (1 << 2) | (1 << 10) | (applied_vs_max_threads_field << 22);
        let vs_dw8 = front_end_contract
            .vs_dw8_override
            .unwrap_or((programmed_vs_urb_output_length.saturating_sub(1) as u32) << 16);
        let active_binding_table_pointer_offset = 0x20u32;

        log_batch_offset(cursor, "oracle-final 3DSTATE_VERTEX_BUFFERS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VERTEX_BUFFERS_1)?;
        push(batch_dwords, &mut cursor, vertex_buffer_dw1)?;
        push_addr(batch_dwords, &mut cursor, draw.vertex_gpu_addr)?;
        push(batch_dwords, &mut cursor, vertex_buffer_bytes)?;

        log_batch_offset(cursor, "oracle-final 3DSTATE_URB_ALLOC_VS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_VS)?;
        push(
            batch_dwords,
            &mut cursor,
            (programmed_vs_urb_output_length.saturating_sub(1) as u32)
                | (programmed_vs_urb_start << 10)
                | (programmed_vs_urb_start << 21),
        )?;
        push(batch_dwords, &mut cursor, programmed_vs_urb_entries | (programmed_vs_urb_entries << 16))?;

        log_batch_offset(cursor, "oracle-final 3DSTATE_URB_ALLOC_HS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_HS)?;
        push(batch_dwords, &mut cursor, disabled_stage_urb_alloc_dw1)?;
        push(batch_dwords, &mut cursor, 0)?;
        log_batch_offset(cursor, "oracle-final 3DSTATE_URB_ALLOC_DS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_DS)?;
        push(batch_dwords, &mut cursor, disabled_stage_urb_alloc_dw1)?;
        push(batch_dwords, &mut cursor, 0)?;
        log_batch_offset(cursor, "oracle-final 3DSTATE_URB_ALLOC_GS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_GS)?;
        push(batch_dwords, &mut cursor, disabled_stage_urb_alloc_dw1)?;
        push(batch_dwords, &mut cursor, 0)?;

        log_batch_offset(cursor, "oracle-final 3DSTATE_VS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VS)?;
        push(batch_dwords, &mut cursor, vs_ksp_offset & !0x3F)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, vs_dw3)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, vs_dw6)?;
        push(batch_dwords, &mut cursor, vs_dw7)?;
        push(batch_dwords, &mut cursor, vs_dw8)?;

        log_batch_offset(cursor, "oracle-final 3DSTATE_HS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_HS)?;
        for _ in 0..8 {
            push(batch_dwords, &mut cursor, 0)?;
        }
        log_batch_offset(cursor, "oracle-final 3DSTATE_DS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_DS)?;
        for _ in 0..10 {
            push(batch_dwords, &mut cursor, 0)?;
        }

        log_batch_offset(cursor, "oracle-final 3DSTATE_SBE");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE)?;
        push(batch_dwords, &mut cursor, sbe_dw1)?;
        push(batch_dwords, &mut cursor, sbe_dw2)?;
        push(batch_dwords, &mut cursor, sbe_dw3)?;
        push(batch_dwords, &mut cursor, sbe_dw4)?;
        push(batch_dwords, &mut cursor, sbe_dw5)?;
        if emit_sbe_swiz_packet {
            log_batch_offset(cursor, "oracle-final 3DSTATE_SBE_SWIZ");
            push(batch_dwords, &mut cursor, CMD_3DSTATE_SBE_SWIZ)?;
            for _ in 0..10 {
                push(batch_dwords, &mut cursor, 0)?;
            }
        }

        log_batch_offset(cursor, "oracle-final 3DSTATE_PS");
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
        log_batch_offset(cursor, "oracle-final 3DSTATE_PS_EXTRA");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PS_EXTRA)?;
        push(batch_dwords, &mut cursor, ps_extra_dw1)?;

        log_batch_offset(cursor, "oracle-final 3DSTATE_CLIP");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_CLIP)?;
        push(batch_dwords, &mut cursor, clip_dw1)?;
        push(batch_dwords, &mut cursor, clip_dw2)?;
        push(batch_dwords, &mut cursor, clip_dw3)?;
        log_batch_offset(cursor, "oracle-final 3DSTATE_STREAMOUT off");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_STREAMOUT)?;
        for _ in 0..4 {
            push(batch_dwords, &mut cursor, 0)?;
        }
        log_batch_offset(cursor, "oracle-final 3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP)?;
        push(batch_dwords, &mut cursor, probe_state.sf_clip_viewport_offset_bytes)?;
        log_batch_offset(cursor, "oracle-final 3DSTATE_VIEWPORT_STATE_POINTERS_CC");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VIEWPORT_STATE_POINTERS_CC)?;
        push(batch_dwords, &mut cursor, probe_state.cc_viewport_offset_bytes)?;
        log_batch_offset(cursor, "oracle-final 3DSTATE_VF_TOPOLOGY");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_TOPOLOGY)?;
        push(batch_dwords, &mut cursor, primitive_topology)?;
        log_batch_offset(cursor, "oracle-final 3DSTATE_VERTEX_ELEMENTS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VERTEX_ELEMENTS_1)?;
        push(batch_dwords, &mut cursor, vertex_element0_dw0)?;
        push(batch_dwords, &mut cursor, vertex_element0_dw1)?;
        log_batch_offset(cursor, "oracle-final 3DSTATE_TE");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_TE)?;
        for _ in 0..4 {
            push(batch_dwords, &mut cursor, 0)?;
        }
        log_batch_offset(cursor, "oracle-final 3DSTATE_GS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_GS)?;
        for _ in 0..9 {
            push(batch_dwords, &mut cursor, 0)?;
        }
        log_batch_offset(cursor, "oracle-final 3DSTATE_SF");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SF)?;
        push(batch_dwords, &mut cursor, sf_dw1)?;
        push(batch_dwords, &mut cursor, sf_dw2)?;
        push(batch_dwords, &mut cursor, sf_dw3)?;
        log_batch_offset(cursor, "oracle-final 3DSTATE_RASTER");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_RASTER)?;
        push(batch_dwords, &mut cursor, raster_dw1)?;
        push(batch_dwords, &mut cursor, raster_dw2)?;
        push(batch_dwords, &mut cursor, raster_dw3)?;
        push(batch_dwords, &mut cursor, raster_dw4)?;
        log_batch_offset(cursor, "oracle-final 3DSTATE_VIEWPORT_STATE_POINTERS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VIEWPORT_STATE_POINTERS)?;
        push(batch_dwords, &mut cursor, 0)?;
        log_batch_offset(cursor, "oracle-final 3DSTATE_CC_STATE_POINTERS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_CC_STATE_POINTERS)?;
        push(batch_dwords, &mut cursor, probe_state.color_calc_state_offset_bytes | 1)?;
        log_batch_offset(cursor, "oracle-final 3DSTATE_SAMPLE_MASK");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLE_MASK)?;
        push(batch_dwords, &mut cursor, sample_mask_dw1)?;
        log_batch_offset(cursor, "oracle-final 3DSTATE_WM_DEPTH_STENCIL");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_WM_DEPTH_STENCIL)?;
        push(batch_dwords, &mut cursor, wm_depth_stencil_dw1)?;
        push(batch_dwords, &mut cursor, wm_depth_stencil_dw2)?;
        push(batch_dwords, &mut cursor, wm_depth_stencil_dw3)?;
        log_batch_offset(cursor, "oracle-final PIPE_CONTROL handoff");
        push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;
        log_batch_offset(cursor, "oracle-final 3DSTATE_VF");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_MESA_ACTIVE)?;
        push(batch_dwords, &mut cursor, MESA_ACTIVE_VF_CUT_INDEX)?;
        log_batch_offset(cursor, "oracle-final 3DSTATE_PRIMITIVE_REPLICATION");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PRIMITIVE_REPLICATION)?;
        push(batch_dwords, &mut cursor, primitive_replication_dw1)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        log_batch_offset(cursor, "oracle-final 3DSTATE_WM");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_WM)?;
        push(batch_dwords, &mut cursor, wm_dw1)?;
        log_batch_offset(cursor, "oracle-final 3DSTATE_PS_BLEND");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_PS_BLEND)?;
        push(batch_dwords, &mut cursor, ps_blend_dw1)?;
        log_batch_offset(cursor, "oracle-final 3DSTATE_BLEND_STATE_POINTERS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_BLEND_STATE_POINTERS)?;
        push(
            batch_dwords,
            &mut cursor,
            blend_mode.blend_state_pointer_dword(probe_state.blend_state_offset_bytes),
        )?;
        log_batch_offset(cursor, "oracle-final 3DSTATE_BINDING_TABLE_POINTERS_PS");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_PS)?;
        push(batch_dwords, &mut cursor, active_binding_table_pointer_offset)?;
        intel_render_focus_log!(
            "intel/render: probe-oracle-final-refresh emitted=1 start_before_primitive=1 sequence=vb-urb-vs-disabled-stages-sbe-ps-clip-streamout-vp-vf-topology-ve-te-gs-sf-raster-cc-sample-depth-pipe-vf-primrepl-wm-psblend-blend-bt source=host-active-segment hypothesis=erase-earlier-state-order-noise-before-real-draw\n",
        );
    }

    log_batch_offset(cursor, "3DPRIMITIVE");
    let vf_legacy_primitive = vf_synthesized_vue
        && matches!(
            primitive_topology,
            TRIANGLE_TOPOLOGY_RECTLIST | TRIANGLE_TOPOLOGY_POINTLIST | TRIANGLE_TOPOLOGY_LINELIST
        );
    let use_3dprimitive_extended = !vf_legacy_primitive
        && (backend_probe_mode.use_3dprimitive_extended()
            || front_end_contract.primitive_extended_dw1_override.is_some());
    let primitive_cmd;
    let primitive_dw1;
    let primitive_dw2;
    let primitive_dw3;
    let primitive_dw4;
    let primitive_dw5;
    if use_3dprimitive_extended {
        primitive_cmd = CMD_3DPRIMITIVE_EXTENDED;
        primitive_dw1 = backend_probe_mode
            .primitive_extended_dw1_override()
            .or(front_end_contract.primitive_extended_dw1_override)
            .unwrap_or(primitive_topology);
        primitive_dw2 = draw.vertex_count;
        primitive_dw3 = 0;
        primitive_dw4 = 1;
        primitive_dw5 = 0;
        push(batch_dwords, &mut cursor, CMD_3DPRIMITIVE_EXTENDED)?;
        push(batch_dwords, &mut cursor, primitive_dw1)?;
        push(batch_dwords, &mut cursor, primitive_dw2)?;
        push(batch_dwords, &mut cursor, primitive_dw3)?;
        push(batch_dwords, &mut cursor, primitive_dw4)?;
        push(batch_dwords, &mut cursor, primitive_dw5)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, 0)?;
        intel_render_focus_log!(
            "intel/render: probe-3dprimitive-extended backend={} emitted=1 cmd=0x{:08X} vf_topology={} primitive_dw1=0x{:08X} dw=[0x{:08X},0x{:08X},0x00000000,0x00000001,0x00000000,0x00000000,0x00000000,0x00000000,0x00000000] source=mesa-gen125-xml-and-genX_cmd_draw hypothesis=extended-draw-keeps-topology-in-dw1-and-vf-topology-state\n",
            backend_probe_mode.label(),
            CMD_3DPRIMITIVE_EXTENDED,
            primitive_topology_label(primitive_topology),
            primitive_dw1,
            primitive_dw1,
            draw.vertex_count,
        );
    } else {
        primitive_cmd = CMD_3DPRIMITIVE;
        primitive_dw1 = primitive_topology;
        primitive_dw2 = draw.vertex_count;
        primitive_dw3 = 0;
        primitive_dw4 = 1;
        primitive_dw5 = 0;
        push(batch_dwords, &mut cursor, CMD_3DPRIMITIVE)?;
        push(batch_dwords, &mut cursor, primitive_dw1)?;
        push(batch_dwords, &mut cursor, primitive_dw2)?;
        push(batch_dwords, &mut cursor, primitive_dw3)?;
        push(batch_dwords, &mut cursor, primitive_dw4)?;
        push(batch_dwords, &mut cursor, primitive_dw5)?;
        push(batch_dwords, &mut cursor, 0)?;
    }
    intel_render_focus_log!(
        "intel/render: probe-frontend-dwords contract={} vb=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] ve0=[0x{:08X},0x{:08X},0x{:08X}] vf=[0x{:08X},0x{:08X}] vf_topology=[0x{:08X},0x{:08X}] primitive=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},...] mesa_active_vb_dw1=0x0204400C mesa_active_ve0=[0x78090001,0x02400000,0x11130000] mesa_active_vf=[0x780C0200,0x0000FFFF] mesa_active_vf_topology=0x00000004 mesa_active_primitive_ext=[0x7B000808,0x00000000,0x00000003,0x00000000,0x00000001,0x00000000,...] note=visible-vf-dword-comparison\n",
        front_end_contract.label,
        if mesa_no_vs_rectlist_vb1 {
            CMD_3DSTATE_VERTEX_BUFFERS_2
        } else {
            CMD_3DSTATE_VERTEX_BUFFERS_1
        },
        vertex_buffer_dw1,
        vertex_buffer_addr_lo,
        vertex_buffer_addr_hi,
        vertex_buffer_bytes,
        vertex_elements_cmd,
        vertex_element0_dw0,
        vertex_element0_dw1,
        CMD_3DSTATE_VF_MESA_ACTIVE,
        MESA_ACTIVE_VF_CUT_INDEX,
        CMD_3DSTATE_VF_TOPOLOGY,
        primitive_topology,
        primitive_cmd,
        primitive_dw1,
        primitive_dw2,
        primitive_dw3,
        primitive_dw4,
        primitive_dw5,
    );

    let state_dwords = unsafe {
        core::slice::from_raw_parts(warm.draw_state_virt as *const u32, warm.draw_state_len / 4)
    };
    let surface = &state_dwords[probe_state.surface_state_offset_bytes as usize / 4
        ..probe_state.surface_state_offset_bytes as usize / 4 + 16];
    let blend = &state_dwords[probe_state.blend_state_offset_bytes as usize / 4
        ..probe_state.blend_state_offset_bytes as usize / 4 + 2];
    let cc_viewport = &state_dwords[probe_state.cc_viewport_offset_bytes as usize / 4
        ..probe_state.cc_viewport_offset_bytes as usize / 4 + 2];
    let sf_clip_viewport = &state_dwords[probe_state.sf_clip_viewport_offset_bytes as usize / 4
        ..probe_state.sf_clip_viewport_offset_bytes as usize / 4 + 16];
    let scissor = &state_dwords[probe_state.scissor_rect_offset_bytes as usize / 4
        ..probe_state.scissor_rect_offset_bytes as usize / 4 + 2];
    let surface_base = (surface[8] as u64) | ((surface[9] as u64) << 32);
    let vs_enabled = !vf_synthesized_vue;
    let vs_ksp = if vs_enabled { vs_ksp_offset & !0x3F } else { 0 };
    let vs_sampler_count = if vs_enabled {
        pipeline.vs.meta.kernel.sampler_count
    } else {
        0
    };
    let vs_binding_table_entry_count = if vs_enabled {
        pipeline.vs.meta.kernel.binding_table_entry_count
    } else {
        0
    };
    let ps_simd_mode = if ps_dispatch_8 != 0 {
        "simd8"
    } else if ps_dispatch_16 != 0 {
        "simd16"
    } else if ps_dispatch_32 != 0 {
        "simd32"
    } else {
        "none"
    };
    let post_heavy_flags = post_draw_sync_variant.heavy_sync_flags().unwrap_or(0);
    intel_render_focus_log!(
        "intel/render: probe-state-contract backend={} vs[enabled={} ksp=0x{:X} scratch=0x0 sampler_count={} binding_table_count={}] urb_vs[entries={} start={} size_64b={} size_field={} dw1=0x{:08X} dw2=0x{:08X}] sf[dw=[0x{:08X},0x{:08X},0x{:08X}] output_attr_count={}] wm[dw=0x{:08X} stats={} force_dispatch={} ps_dispatch_formula_enable={}] ps[ksp=[0x{:X},0x{:X},0x{:X}] simd={} dispatch_bits={}{}{} bt_count={} sampler_count={} scratch=0x{:X} dw3=0x{:08X} dw6=0x{:08X} dw7=0x{:08X}] primitive[cmd=0x{:08X} topology={} count={} start={} instance_count={} base_vertex={}]\n",
        backend_probe_mode.label(),
        vs_enabled as u8,
        vs_ksp,
        vs_sampler_count,
        vs_binding_table_entry_count,
        programmed_vs_urb_entries,
        programmed_vs_urb_start,
        programmed_vs_urb_output_length,
        programmed_vs_urb_output_length.saturating_sub(1),
        (programmed_vs_urb_output_length.saturating_sub(1) as u32)
            | (programmed_vs_urb_start << 10)
            | (programmed_vs_urb_start << 21),
        programmed_vs_urb_entries | (programmed_vs_urb_entries << 16),
        sf_dw1,
        sf_dw2,
        sf_dw3,
        (sbe_dw1 >> 22) & 0x3F,
        wm_dw1,
        (wm_dw1 >> 31) & 0x1,
        (wm_dw1 >> 19) & 0x3,
        ((ps_extra_dw1 & PS_EXTRA_PIXEL_SHADER_VALID) != 0 && (ps_blend_dw1 & (1 << 30)) != 0)
            as u8,
        ps_ksp0,
        ps_ksp1,
        ps_ksp2,
        ps_simd_mode,
        ps_dispatch_8,
        ps_dispatch_16,
        ps_dispatch_32,
        ps_binding_table_entry_count,
        pipeline.ps.meta.kernel.sampler_count,
        ps_scratch_space_buffer,
        ps_dw3,
        ps_dw6,
        ps_dw7,
        primitive_cmd,
        primitive_topology_label(primitive_topology),
        primitive_dw2,
        primitive_dw3,
        primitive_dw4,
        primitive_dw5,
    );
    intel_render_focus_log!(
        "intel/render: probe-pointer-surface-contract backend={} binding_table_ps=0x{:X} bt_entry0=0x{:08X} surface_off=0x{:X} rt_surface[type={} format=0x{:X} width={} height={} pitch={} mocs=0x{:X} base=0x{:X} swizzle=0x{:08X}] viewport[generic_emitted=1 generic_ptr=0x0 sf_clip_ptr=0x{:X} cc_ptr=0x{:X} sf_m00={:.3} sf_m11={:.3} sf_m30={:.3} sf_m31={:.3} cc_min={:.3} cc_max={:.3}] scissor_ptr=0x{:X} scissor=[{},{}..{},{}] blend_ptr=0x{:X} blend_dwords=[0x{:08X},0x{:08X}] cc_state_ptr=0x{:X} depth_stencil_state_ptr=not-emitted wm_depth_packet=[0x{:08X},0x{:08X},0x{:08X}]\n",
        backend_probe_mode.label(),
        binding_table_pointer_offset,
        state_dwords[probe_state.binding_table_offset_bytes as usize / 4],
        probe_state.surface_state_offset_bytes,
        (surface[0] >> 29) & 0x7,
        (surface[0] >> 18) & 0x1FF,
        (surface[2] & 0x3FFF) + 1,
        ((surface[2] >> 16) & 0x3FFF) + 1,
        surface[3] + 1,
        (surface[1] >> 24) & 0x7F,
        surface_base,
        surface[7],
        probe_state.sf_clip_viewport_offset_bytes,
        probe_state.cc_viewport_offset_bytes,
        f32::from_bits(sf_clip_viewport[0]),
        f32::from_bits(sf_clip_viewport[1]),
        f32::from_bits(sf_clip_viewport[3]),
        f32::from_bits(sf_clip_viewport[4]),
        f32::from_bits(cc_viewport[0]),
        f32::from_bits(cc_viewport[1]),
        probe_state.scissor_rect_offset_bytes,
        scissor[0] & 0xFFFF,
        scissor[0] >> 16,
        scissor[1] & 0xFFFF,
        scissor[1] >> 16,
        blend_mode.blend_state_pointer_dword(probe_state.blend_state_offset_bytes),
        blend[0],
        blend[1],
        probe_state.color_calc_state_offset_bytes | 1,
        wm_depth_stencil_dw1,
        wm_depth_stencil_dw2,
        wm_depth_stencil_dw3,
    );
    intel_render_focus_log!(
        "intel/render: probe-pipe-control-contract backend={} before_draw=[flush_flags=0x{:08X},invalidate_flags=0x{:08X},binding_pool_pre_cs=0x{:08X},binding_pool_post_invalidate=0x{:08X}] after_draw=[light_flags=0x{:08X},light_post_sync={},heavy_flags=0x{:08X},heavy_post_sync={},oa_fence_flags=0x{:08X}] postdraw_variant={}\n",
        backend_probe_mode.label(),
        PIPE_CONTROL_FLUSH_BITS,
        PIPE_CONTROL_INVALIDATE_BITS,
        PIPE_CONTROL_CS_STALL,
        PIPE_CONTROL_INVALIDATE_BITS,
        post_draw_sync_variant.light_sync_flags(),
        post_draw_sync_variant.light_post_sync_enabled() as u8,
        post_heavy_flags,
        ((post_heavy_flags & PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE) != 0) as u8,
        if defer_raster_wm_oa_end {
            PIPE_CONTROL_POST_DRAW_SYNC_BITS
        } else {
            0
        },
        post_draw_sync_variant.label(),
    );

    if backend_probe_mode.uses_raster_wm_oa() && !defer_raster_wm_oa_end {
        log_batch_offset(cursor, "MI_REPORT_PERF_COUNT raster-wm end immediate");
        push_mi_report_perf_count(
            batch_dwords,
            &mut cursor,
            result_gpu_addr + (RESULT_OA_END_DWORD as u64) * 4,
            RESULT_OA_RASTER_WM_END_ID,
        )?;
        log_batch_offset(cursor, "OA raster-wm disable immediate");
        push_raster_wm_oa_config(batch_dwords, &mut cursor, false)?;
    }

    log_batch_offset(cursor, "MI_STORE_DATA_IMM pre-light-pipe-control");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_PRE_LIGHT_PC_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_PRE_LIGHT_PC,
    )?;

    if matches!(post_draw_sync_variant, PostDrawSyncVariant::NoPostDrawPipeControl) {
        log_batch_offset(cursor, "skip PIPE_CONTROL post-3d-light-marker");
        intel_render_focus_log!(
            "intel/render: probe-postdraw-sync backend={} variant={} action=skip-light-pipe-control source=pdoane-reference-style\n",
            backend_probe_mode.label(),
            post_draw_sync_variant.label(),
        );
    } else {
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
    }

    log_batch_offset(cursor, "MI_STORE_DATA_IMM final-after-light");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_FINAL_AFTER_LIGHT_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_FINAL_AFTER_LIGHT,
    )?;

    if let Some(heavy_sync_flags) = post_draw_sync_variant.heavy_sync_flags() {
        log_batch_offset(cursor, "PIPE_CONTROL post-3d-heavy-sync");
        push_pipe_control_post_sync_imm(
            batch_dwords,
            &mut cursor,
            post_draw_sync_variant.heavy_header_flags(),
            heavy_sync_flags,
            result_gpu_addr + (RESULT_SLOT_POST3D_PIPE_CONTROL_LO_DWORD as u64) * 4,
            post3d_value,
        )?;
    }

    if backend_probe_mode.uses_raster_wm_oa() && defer_raster_wm_oa_end {
        intel_render_focus_log!(
            "intel/render: probe-raster-wm-oa-end-order backend={} mode=after-hard-postdraw-fence fence_batch_off=0x{:04X} end_batch_off=after-fence flags=0x{:08X} note=normal-raster-oa-probes-use-immediate-end-for-valid-comparison\n",
            backend_probe_mode.label(),
            cursor * 4,
            PIPE_CONTROL_POST_DRAW_SYNC_BITS,
        );
        log_batch_offset(cursor, "PIPE_CONTROL raster-wm-oa-fence");
        push_pipe_control_post_sync_imm(
            batch_dwords,
            &mut cursor,
            0,
            PIPE_CONTROL_POST_DRAW_SYNC_BITS,
            result_gpu_addr + (RESULT_SLOT_POST3D_PIPE_CONTROL_LO_DWORD as u64) * 4,
            post3d_value,
        )?;
        log_batch_offset(cursor, "MI_REPORT_PERF_COUNT raster-wm end after-fence");
        push_mi_report_perf_count(
            batch_dwords,
            &mut cursor,
            result_gpu_addr + (RESULT_OA_END_DWORD as u64) * 4,
            RESULT_OA_RASTER_WM_END_ID,
        )?;
        log_batch_offset(cursor, "OA raster-wm disable after-fence");
        push_raster_wm_oa_config(batch_dwords, &mut cursor, false)?;
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
        "intel/render: probe-3d backend={} ps_bt_count={} ps_ksp=[0x{:X},0x{:X},0x{:X}] ps_scratch=0x{:X} ps_dispatch_bits={}{}{} sbe=0x{:08X} clip=[0x{:08X},0x{:08X},0x{:08X}] sf=[0x{:08X},0x{:08X},0x{:08X}] raster=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] wm=0x{:08X} ps3=0x{:08X} ps6=0x{:08X} ps7=0x{:08X} ps_extra=0x{:08X}\n",
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
        "intel/render: probe-backend ps_blend=0x{:08X} wm_depth=[0x{:08X},0x{:08X},0x{:08X}] wm_hz_op=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
        ps_blend_dw1,
        wm_depth_stencil_dw1,
        wm_depth_stencil_dw2,
        wm_depth_stencil_dw3,
        wm_hz_op_dw1,
        wm_hz_op_dw2,
        wm_hz_op_dw3,
        wm_hz_op_dw4,
        wm_hz_op_dw5,
    );
    intel_render_focus_log!(
        "intel/render: probe-binding-table-pool base=0x{:X} base_dw=0x{:08X} size_dw=0x{:08X} mocs=0x{:X} enable={} ps_bt_ptr=0x{:X} bt_gpu=0x{:X} bt_entry0=0x{:08X} surf_gpu=0x{:X} contract=pool-relative\n",
        shader_layout.state_region_gpu_addr,
        binding_table_pool_base_dw,
        binding_table_pool_size_dw,
        RENDER_MOCS & BINDING_TABLE_POOL_MOCS_MASK,
        binding_table_pool_enable,
        binding_table_pointer_offset,
        binding_table_gpu_addr,
        probe_state.surface_state_offset_bytes,
        binding_table_entry0_gpu_addr,
    );
    intel_render_focus_log!(
        "intel/render: probe-sbe-decoded read_offset={} read_length={} attr_swizzle={} emit_sbe_swiz={} num_sf_outputs={} force_offset={} force_length={} active_component_dw4=0x{:08X} active_component_dw5=0x{:08X} note=sbe-feeds-sf-to-wm-attribute-contract;attr-swizzle-gates-explicit-sbe-swiz\n",
        (sbe_dw1 >> 5) & 0x3F,
        (sbe_dw1 >> 11) & 0x1F,
        (sbe_dw1 >> 21) & 0x1,
        u32::from(emit_sbe_swiz_packet),
        (sbe_dw1 >> 22) & 0x3F,
        (sbe_dw1 >> 28) & 0x1,
        (sbe_dw1 >> 29) & 0x1,
        sbe_dw4,
        sbe_dw5,
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
        raster_dw1,
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
    let clip_mode = (clip_dw2 >> 13) & 0x7;
    let api_mode = (clip_dw2 >> 30) & 0x1;
    let provoking_tri_fan = clip_dw2 & 0x3;
    let provoking_line = (clip_dw2 >> 2) & 0x3;
    let provoking_tri_strip = (clip_dw2 >> 4) & 0x3;
    let guardband_enable = (clip_dw2 >> 26) & 0x1;
    let viewport_xy_clip_enable = (clip_dw2 >> 28) & 0x1;
    let clip_enable = (clip_dw2 >> 31) & 0x1;
    let non_perspective_bary_enable = (clip_dw2 >> 8) & 0x1;
    let force_clip_mode = ((clip_dw1 & CLIP_FORCE_CLIP_MODE) != 0) as u8;
    let early_cull_enable = (clip_dw1 >> 18) & 0x1;
    let statistics_enable = (clip_dw1 >> 10) & 0x1;
    let vertex_subpixel_precision = (clip_dw1 >> 19) & 0x1;
    let max_vp_idx = clip_dw3 & 0xF;
    let force_zero_rta_index = (clip_dw3 >> 5) & 0x1;
    intel_render_focus_log!(
        "intel/render: probe-clip-decoded topo={} patchlist=0 gs_active=0 ClipMode={}({}) clip_action={} clip_thread_expected={} APIMode={}({}) GuardbandClipTestEnable={} ViewportXYClipTestEnable={} ClipEnable={} PerspectiveDivideDisable={} NonPerspectiveBarycentricEnable={} ForceClipMode={} EarlyCullEnable={} StatisticsEnable={} VertexSubPixelPrecisionSelect={} TriangleFanProvokingVertexSelect={} LineStripListProvokingVertexSelect={} TriangleStripListProvokingVertexSelect={} MaximumVPIndex={} ForceZeroRTAIndexEnable={}\n",
        primitive_topology_label(primitive_topology),
        clip_mode,
        decode_clip_mode_name(clip_mode),
        decode_clip_mode_action(clip_mode),
        clip_mode_thread_expected(clip_mode) as u8,
        api_mode,
        decode_api_mode_name(api_mode),
        guardband_enable,
        viewport_xy_clip_enable,
        clip_enable,
        ((clip_dw2 & CLIP_PERSPECTIVE_DIVIDE_DISABLE) != 0) as u8,
        non_perspective_bary_enable,
        force_clip_mode,
        early_cull_enable,
        statistics_enable,
        decode_vertex_subpixel_precision_name(vertex_subpixel_precision),
        provoking_tri_fan,
        provoking_line,
        provoking_tri_strip,
        max_vp_idx,
        force_zero_rta_index,
    );
    intel_render_focus_log!(
        "intel/render: probe-sf-decoded sf_dw=[0x{:08X},0x{:08X},0x{:08X}] ViewportTransformEnable={} StatisticsEnable={} LegacyGlobalDepthBiasEnable={} DerefBlockSize={}({}) LineWidth=0x{:X} PointWidth=0x{:X} PointWidthSource={} VertexSubPixelPrecisionSelect={} SmoothPointEnable={} AALineDistanceMode={} LastPixelEnable={} TriangleStripListProvokingVertexSelect={} LineStripListProvokingVertexSelect={} TriangleFanProvokingVertexSelect={}\n",
        sf_dw1,
        sf_dw2,
        sf_dw3,
        (sf_dw1 >> 1) & 0x1,
        (sf_dw1 >> 10) & 0x1,
        (sf_dw1 >> 11) & 0x1,
        (sf_dw2 >> 29) & 0x3,
        decode_deref_block_size_name((sf_dw2 >> 29) & 0x3),
        (sf_dw1 >> 12) & 0x3FFFF,
        sf_dw3 & 0x7FF,
        (sf_dw3 >> 11) & 0x1,
        decode_vertex_subpixel_precision_name((sf_dw3 >> 12) & 0x1),
        (sf_dw3 >> 13) & 0x1,
        (sf_dw3 >> 14) & 0x1,
        (sf_dw3 >> 31) & 0x1,
        (sf_dw3 >> 29) & 0x3,
        (sf_dw3 >> 27) & 0x3,
        (sf_dw3 >> 25) & 0x3,
    );
    intel_render_focus_log!(
        "intel/render: probe-raster-decoded sf_viewport=0x{:X} cc_viewport=0x{:X} scissor_ptr=0x{:X} cull={} fill_front={} fill_back={} front={} api_mode={}({}) z_near_clip={} z_far_clip={} raster_scissor_legacy_bit={} aa_enable={} dx_ms_enable={} dx_ms_mode={}({}) force_multisampling={} forced_samples={}({}) rt_independent_raster={} wm_hz_op={} wm_int_ms_raster_mode={} sample_mask=0x{:X} multisample_dw=0x{:08X}\n",
        probe_state.sf_clip_viewport_offset_bytes,
        probe_state.cc_viewport_offset_bytes,
        probe_state.scissor_rect_offset_bytes,
        decode_cull_mode_name((raster_dw1 >> 16) & 0x3),
        decode_fill_mode_name((raster_dw1 >> 5) & 0x3),
        decode_fill_mode_name((raster_dw1 >> 3) & 0x3),
        decode_front_winding_name((raster_dw1 >> 21) & 0x1),
        (raster_dw1 >> 22) & 0x3,
        decode_raster_api_mode_name((raster_dw1 >> 22) & 0x3),
        raster_dw1 & 0x1,
        (raster_dw1 >> 26) & 0x1,
        (raster_dw1 >> 1) & 0x1,
        (raster_dw1 >> 2) & 0x1,
        (raster_dw1 >> 12) & 0x1,
        (raster_dw1 >> 10) & 0x3,
        decode_ms_raster_mode_name((raster_dw1 >> 10) & 0x3),
        (raster_dw1 >> 14) & 0x1,
        (raster_dw1 >> 18) & 0x7,
        decode_forced_sample_count_name((raster_dw1 >> 18) & 0x7),
        (((raster_dw1 >> 18) & 0x7) != 0
            && (wm_hz_op_dw1 | wm_hz_op_dw2 | wm_hz_op_dw3 | wm_hz_op_dw4) == 0) as u8,
        ((wm_hz_op_dw1 | wm_hz_op_dw2 | wm_hz_op_dw3 | wm_hz_op_dw4) != 0) as u8,
        decode_forced_wm_int_ms_raster_mode_name(
            ((raster_dw1 >> 14) & 0x1) != 0,
            (raster_dw1 >> 10) & 0x3,
        ),
        sample_mask_dw1,
        multisample_dw1,
    );
    let wm_force_kill = wm_dw1 & 0x3;
    let wm_barycentric = (wm_dw1 >> 11) & 0x3F;
    let wm_position_zw = (wm_dw1 >> 17) & 0x3;
    let wm_force_dispatch = (wm_dw1 >> 19) & 0x3;
    let wm_eds = (wm_dw1 >> 21) & 0x3;
    let wm_walk_direction = (wm_dw1 >> 23) & 0x1;
    let wm_walk_granularity = (wm_dw1 >> 24) & 0x3;
    intel_render_focus_log!(
        "intel/render: probe-wm-decoded backend={} force_kill={}({}) point_rule={} line_stipple={} poly_stipple={} bary=0x{:02X} pos_zw={}({}) force_thread_dispatch={}({}) eds={}({}) walk={} granularity={}({}) wm_reserved29={} legacy_depth_clear={} legacy_depth_resolve={} legacy_hiz_resolve={} stats={} thread_dispatch_formula_inputs ps_valid={} ps_writes_rt={} has_writeable_rt={} wm_hz_op={} wm_hz_scissor={} wm_hz_samples={} wm_hz_sample_mask=0x{:X} note=coverage_still_requires_sf_setup_and_scan_conversion\n",
        backend_probe_mode.label(),
        wm_force_kill,
        decode_wm_force_mode(wm_force_kill),
        (wm_dw1 >> 2) & 0x1,
        (wm_dw1 >> 3) & 0x1,
        (wm_dw1 >> 4) & 0x1,
        wm_barycentric,
        wm_position_zw,
        decode_wm_position_zw_mode(wm_position_zw),
        wm_force_dispatch,
        decode_wm_force_mode(wm_force_dispatch),
        wm_eds,
        decode_wm_eds_mode(wm_eds),
        if wm_walk_direction == 0 { "snake" } else { "z" },
        wm_walk_granularity,
        decode_wm_walk_granularity(wm_walk_granularity),
        (wm_dw1 >> 29) & 0x1,
        (wm_dw1 >> 30) & 0x1,
        (wm_dw1 >> 28) & 0x1,
        (wm_dw1 >> 27) & 0x1,
        (wm_dw1 >> 31) & 0x1,
        ((ps_extra_dw1 & PS_EXTRA_PIXEL_SHADER_VALID) != 0) as u8,
        ((ps_extra_dw1 & PS_EXTRA_PIXEL_SHADER_DOES_NOT_WRITE_RT) == 0) as u8,
        (ps_blend_dw1 >> 30) & 0x1,
        ((wm_hz_op_dw1 | wm_hz_op_dw2 | wm_hz_op_dw3 | wm_hz_op_dw4) != 0) as u8,
        (wm_hz_op_dw1 >> 29) & 0x1,
        (wm_hz_op_dw1 >> 13) & 0x7,
        wm_hz_op_dw4 & 0xFFFF,
    );
    intel_render_focus_log!(
        "intel/render: probe-sf-wm-contract topo={} vertex_count={} primitive_objects_expected={} coord_contract={} clip_mode={}({}) force_clip_mode={} clip_enable={} clip_action={} pdd={} nonpersp_bary={} sf_viewport_transform={} sf_deref_block={}({}) prm_vs_entries={} prm_expected_deref={} vp_index=0 rta_index_forced_zero={} draw_rect=[0,0..{},{}] scissor=[0,0..{},{}] sf_outputs_hidden=pue_handle|bbox|edge_equations|raster_start|orientation first_unproven=sf_object_setup_to_wm_scan_conversion\n",
        primitive_topology_label(primitive_topology),
        draw.vertex_count,
        if primitive_topology == TRIANGLE_TOPOLOGY_TRILIST {
            draw.vertex_count / 3
        } else if primitive_topology == TRIANGLE_TOPOLOGY_RECTLIST {
            draw.vertex_count / 3
        } else if primitive_topology == TRIANGLE_TOPOLOGY_LINELIST {
            draw.vertex_count / 2
        } else {
            draw.vertex_count
        },
        if (clip_dw2 & CLIP_PERSPECTIVE_DIVIDE_DISABLE) != 0
            && backend_probe_mode.disable_sf_viewport_transform()
        {
            "pretransformed-screen-space-no-sf-viewport"
        } else if (clip_dw2 & CLIP_PERSPECTIVE_DIVIDE_DISABLE) != 0 {
            "pdd-with-sf-viewport-transform"
        } else {
            "clip-space-with-sf-viewport-transform"
        },
        clip_mode,
        decode_clip_mode_name(clip_mode),
        force_clip_mode,
        clip_enable as u8,
        if clip_enable != 0 {
            decode_clip_mode_action(clip_mode)
        } else {
            "clip-stage-bypass"
        },
        ((clip_dw2 & CLIP_PERSPECTIVE_DIVIDE_DISABLE) != 0) as u8,
        non_perspective_bary_enable,
        (sf_dw1 >> 1) & 0x1,
        (sf_dw2 >> 29) & 0x3,
        decode_deref_block_size_name((sf_dw2 >> 29) & 0x3),
        programmed_vs_urb_entries,
        if programmed_vs_urb_entries < 192 {
            "PerPoly"
        } else {
            "Block32"
        },
        force_zero_rta_index,
        draw_rect_max_x,
        draw_rect_max_y,
        draw.target_w.saturating_sub(1),
        draw.target_h.saturating_sub(1),
    );
    intel_render_focus_log!(
        "intel/render: probe-prim-repl-decoded replication_count={} replica_mask=0x{:X} rtai0={} disabled_like_mesa_simple_shader={} hypothesis=remove_hidden_vp_rtai_replication_from_sf_wm_contract\n",
        primitive_replication_dw1 & 0xF,
        (primitive_replication_dw1 >> 16) & 0xFFFF,
        0,
        (primitive_replication_dw1 == 0) as u8,
    );
    intel_render_focus_log!(
        "intel/render: probe-handoff-decoded clip_out=sf vue_in_urb=1 baked_vs_urb_out_len={} programmed_vs_urb_out_len={} programmed_vs_urb_entries={} programmed_vs_urb_start={} baked_vs_grf_start={} programmed_vs_grf_start={} vs_output_read_offset={} sbe_read_offset={} sbe_read_len={} ps_varyings={} streamout={}\n",
        baked_vs_urb_output_length,
        programmed_vs_urb_output_length,
        programmed_vs_urb_entries,
        programmed_vs_urb_start,
        pipeline.vs.meta.kernel.grf_start_register,
        front_end_contract
            .vs_dispatch_grf_start_override
            .unwrap_or(pipeline.vs.meta.kernel.grf_start_register),
        (front_end_contract
            .vs_dw8_override
            .unwrap_or((programmed_vs_urb_output_length.saturating_sub(1) as u32) << 16)
            >> 21)
            & 0x3F,
        sbe_decoded_read_offset,
        sbe_decoded_read_length,
        pipeline.ps.meta.num_varying_inputs,
        batch_mode.streamout_enabled() as u8,
    );
    intel_render_focus_log!(
        "intel/render: probe-ps-payload-decoded backend={} push_constant_enable={} push_constant_bytes={} scratch=0x{:X} grf_start={} grf_used={} ps_extra=0x{:08X} attr_enable={} simple_hint={} src_depth={} src_w={} src_depth_w_coeff={} bary_coeffs={} wm_bary=0x{:X} ps_dispatch_bits={}{}{} does_not_prove=ps_thread_launch\n",
        backend_probe_mode.label(),
        ps_push_constant_enable as u8,
        pipeline.ps.meta.kernel.push_constant_bytes,
        ps_scratch_space_buffer,
        ps_grf_start,
        pipeline.ps.meta.kernel.grf_used,
        ps_extra_dw1,
        ((ps_extra_dw1 & PS_EXTRA_ATTRIBUTE_ENABLE) != 0) as u8,
        ((ps_extra_dw1 & PS_EXTRA_SIMPLE_PS_HINT) != 0) as u8,
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
    intel_render_focus_log!(
        "intel/render: probe-ps-grf-decoded backend={} baked_grf_start={} programmed_grf_start={} grf_used={} register_blocks_16={} max_threads_per_psd={} ps_dw6=0x{:08X} ps_dw7=0x{:08X} dispatch_bits={}{}{} does_not_prove=ps_thread_launch\n",
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
        "intel/render: 3dprimitive-setup mode={:?} topo={} vertices={} start_vertex=0 instances={} start_instance=0 base_vertex=0 vb=0x{:X} stride={} rt=0x{:X} pitch=0x{:X} rect={}x{} postdraw_sync={} light_flags=0x{:08X}\n",
        batch_mode,
        primitive_topology_label(primitive_topology),
        draw.vertex_count,
        1,
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
    let post_draw_sync_variant = vs_config
        .map(|config| config.post_draw_sync)
        .unwrap_or(PostDrawSyncVariant::NoPostDrawPipeControl);
    let vs_binding_table_entry_count = vs_config
        .map(|config| config.pipeline.vs.meta.kernel.binding_table_entry_count)
        .unwrap_or(0);
    let vs_binding_table_pointer_offset = if vs_binding_table_entry_count > 0 {
        0x3400
    } else {
        0
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
        header_flags: u32,
        flags: u32,
        address: u64,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD | header_flags)?;
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

    fn binding_table_pool_base_dword(device_id: u16, base: u64) -> u32 {
        let base = (base as u32) & BINDING_TABLE_POOL_BASE_MASK;
        let mocs = RENDER_MOCS & BINDING_TABLE_POOL_MOCS_MASK;
        if device_is_gfx125(device_id) {
            base | mocs
        } else {
            base | BINDING_TABLE_POOL_ENABLE | mocs
        }
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
            "intel/render: batch-off 0x{:03X} {}\n",
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
                | (RENDER_MOCS << 16)
                | (1 << 25)
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
    let streamout_dw1 = (1 << 8) | (1 << 25) | (1 << 31);
    let streamout_dw2 = streamout_experiment.vertex_read_length();
    let streamout_dw3 = streamout_experiment.vertex_bytes() as u32;
    let streamout_dw4 = 0u32;
    let so_buffer_index_dw1 = (RENDER_MOCS << 22) | (1 << 21) | (1 << 31);
    let sbe_vertex_read_offset = streamout_experiment.vertex_read_offset();
    let sbe_vertex_read_length = streamout_experiment.vertex_read_length_field() + 1;
    let sbe_dw1 = (sbe_vertex_read_offset << 5)
        | (sbe_vertex_read_length << 11)
        | (1 << 21)
        | (1 << 22)
        | (1 << 28)
        | (1 << 29);
    let front_end_contract = vs_config
        .map(|config| config.front_end_contract)
        .unwrap_or(TRIANGLE_DEFAULT_FRONT_END_CONTRACT);
    let programmed_vs_urb_output_length = vs_config
        .map(|config| {
            front_end_contract
                .vs_urb_output_length_override
                .or(TRIANGLE_VS_URB_OUTPUT_LENGTH_OVERRIDE)
                .unwrap_or(config.pipeline.vs.meta.urb_entry_output_length)
        })
        .unwrap_or(1);
    let programmed_vs_urb_entries = vs_config
        .map(|_| {
            front_end_contract
                .vs_urb_entries_override
                .unwrap_or(TRIANGLE_VS_URB_ENTRIES)
        })
        .unwrap_or(TRIANGLE_VS_URB_ENTRIES);
    let programmed_vs_urb_start = front_end_contract
        .vs_urb_start_override
        .unwrap_or(TRIANGLE_VS_URB_START);
    let urb_vs_alloc_dw1 = (programmed_vs_urb_output_length.saturating_sub(1) as u32)
        | (programmed_vs_urb_start << 10)
        | (programmed_vs_urb_start << 21);
    let urb_vs_alloc_dw2 = programmed_vs_urb_entries | (programmed_vs_urb_entries << 16);
    let disabled_stage_urb_alloc_dw1 =
        (programmed_vs_urb_start << 10) | (programmed_vs_urb_start << 21);
    let gfx125_sample_pattern_dw = 0x8888_8888;
    let gfx125_slice_hash =
        device_is_gfx125(warm.device_id).then(|| gfx125_slice_hash_config(warm));
    let gfx125_3d_mode_dw1 = gfx125_slice_hash.map(gfx125_3d_mode_dw1).unwrap_or(0);
    let gfx125_3d_mode_dw3 = gfx125_3d_mode_dw3();
    let vb_size_bytes = draw.vertex_count.saturating_mul(draw.vertex_stride);
    let mesa_no_vs_rectlist_vb1 = vs_config.is_none()
        && matches!(
            streamout_experiment,
            StreamoutProofExperiment::MesaNoVsRectlist
        );
    let vb_count = if mesa_no_vs_rectlist_vb1 { 2 } else { 1 };
    let vb_cmd = cmd_3dstate_vertex_buffers(vb_count)?;
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

    if device_is_gfx125(warm.device_id) {
        let chicken_raster_2_value = gfx125_chicken_raster_2_value();
        log_batch_offset(cursor, "MI_LOAD_REGISTER_IMM MCR_SELECTOR multicast");
        push_load_register_imm(batch_dwords, &mut cursor, MCR_SELECTOR, MCR_MULTICAST)?;
        log_batch_offset(cursor, "MI_LOAD_REGISTER_IMM CHICKEN_RASTER_2");
        push_load_register_imm(
            batch_dwords,
            &mut cursor,
            CHICKEN_RASTER_2,
            chicken_raster_2_value,
        )?;
        log_batch_offset(cursor, "MI_LOAD_REGISTER_IMM MCR_SELECTOR multicast restore");
        push_load_register_imm(batch_dwords, &mut cursor, MCR_SELECTOR, MCR_MULTICAST)?;
        intel_render_verbose_log!(
            "intel/render: gfx125-raster-wa-batch chicken_raster_2=0x{:08X} mcr_selector=0x{:08X} tbimr_fast_clip=1 source=linux-xe-wa-14021567978\n",
            chicken_raster_2_value,
            MCR_MULTICAST,
        );
    }

    log_batch_offset(cursor, "STATE_BASE_ADDRESS");
    push(batch_dwords, &mut cursor, STATE_BASE_ADDRESS_CMD)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push(batch_dwords, &mut cursor, RENDER_MOCS << 16)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.vertex_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    for _ in 0..6 {
        push(batch_dwords, &mut cursor, 0)?;
    }

    if vs_binding_table_entry_count > 0 {
        let binding_table_pool_size_dw = u32::try_from(
            crate::intel::align_up(warm.draw_state_len, 4096)
                .ok_or("streamout-proof-binding-pool-align")?,
        )
        .map_err(|_| "streamout-proof-binding-pool-convert")?
            & 0xFFFF_F000;
        log_batch_offset(cursor, "PIPE_CONTROL pre-vs-binding-table-pool");
        push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_CS_STALL)?;
        log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POOL_ALLOC vs-probe");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POOL_ALLOC)?;
        push(
            batch_dwords,
            &mut cursor,
            binding_table_pool_base_dword(warm.device_id, GPU_VA_DRAW_STATE_BASE),
        )?;
        push(batch_dwords, &mut cursor, 0)?;
        push(batch_dwords, &mut cursor, binding_table_pool_size_dw)?;
        log_batch_offset(cursor, "PIPE_CONTROL post-vs-binding-table-pool");
        push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS)?;
        log_batch_offset(cursor, "3DSTATE_BINDING_TABLE_POINTERS_VS vs-probe");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_BINDING_TABLE_POINTERS_VS)?;
        push(batch_dwords, &mut cursor, vs_binding_table_pointer_offset)?;
        intel_render_focus_log!(
            "intel/render: vs-streamout-proof-bti enabled=1 bt_ptr=0x{:X} bt_entries={} pool_base=0x{:X} pool_size=0x{:X} note=diagnostic-vs-dataport-bti34-store\n",
            vs_binding_table_pointer_offset,
            vs_binding_table_entry_count,
            GPU_VA_DRAW_STATE_BASE,
            binding_table_pool_size_dw,
        );
    }

    log_batch_offset(cursor, "3DSTATE_SAMPLE_PATTERN");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_SAMPLE_PATTERN)?;
    for _ in 0..8 {
        push(batch_dwords, &mut cursor, gfx125_sample_pattern_dw)?;
    }
    intel_render_focus_log!(
        "intel/render: probe-sample-pattern-state emitted=1 device=0x{:04X} pattern=center_8_16th includes_1x_sample=1 placement=post-state-base-before-raster\n",
        warm.device_id,
    );

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
            "intel/render: gfx125-svl-init sample_pattern=center slice_hash_ptr=0x{:X} geom_dss=0x{:08X} ppipe_dss={}/{}/{} mask1=0x{:X} mask2=0x{:X} mode_dw1=0x{:08X} mode_dw3=0x{:08X} cross_slice_mode={}({}) rhwo_disable=1\n",
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

    let vf_vertex_element_count = if vs_config.is_some() {
        1
    } else {
        streamout_experiment.vf_vertex_element_count()
    };
    for vertex_element_index in 0..vf_vertex_element_count {
        log_batch_offset(cursor, "3DSTATE_VF_INSTANCING");
        push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_INSTANCING)?;
        push(batch_dwords, &mut cursor, vertex_element_index as u32)?;
        push(batch_dwords, &mut cursor, 0)?;
    }
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
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_MESA_ACTIVE)?;
    push(batch_dwords, &mut cursor, MESA_ACTIVE_VF_CUT_INDEX)?;
    intel_render_focus_log!(
        "intel/render: probe-vf-state emit=streamout-initial cmd=0x{:08X} cut_index=0x{:08X} component_packing=1 source=mesa-active-triangle\n",
        CMD_3DSTATE_VF_MESA_ACTIVE,
        MESA_ACTIVE_VF_CUT_INDEX,
    );
    let vf_sgvs_dw1 = if vs_config.is_none()
        && matches!(streamout_experiment, StreamoutProofExperiment::MesaNoVsRectlist)
    {
        0xA000_0000
    } else {
        0
    };
    log_batch_offset(cursor, "3DSTATE_VF_SGVS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_SGVS)?;
    push(batch_dwords, &mut cursor, vf_sgvs_dw1)?;
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
    if mesa_no_vs_rectlist_vb1 {
        push_vertex_buffer_state(
            batch_dwords,
            &mut cursor,
            1,
            draw.vertex_stride,
            draw.vertex_gpu_addr,
            vb_size_bytes,
        )?;
        intel_render_focus_log!(
            "intel/render: probe-vf-vb1-fix streamout=1 experiment={} vb_count={} addr=0x{:X} stride={} bytes={} reason=mesa-no-vs-rectlist-ve0-references-vb1\n",
            streamout_experiment.label(),
            vb_count,
            draw.vertex_gpu_addr,
            draw.vertex_stride,
            vb_size_bytes,
        );
    }

    log_batch_offset(cursor, "3DSTATE_VERTEX_ELEMENTS");
    push(batch_dwords, &mut cursor, ve_cmd)?;
    if vs_config.is_some() {
        let real_vs_vertex_element_format = front_end_contract
            .vertex_element_format_override
            .unwrap_or(SURFACE_FORMAT_R32G32B32_FLOAT);
        let component3 = if real_vs_vertex_element_format == SURFACE_FORMAT_R32G32B32A32_FLOAT {
            VFCOMP_STORE_SRC
        } else {
            VFCOMP_STORE_1_FP
        };
        push_vertex_element_state(
            batch_dwords,
            &mut cursor,
            0,
            0,
            real_vs_vertex_element_format,
            VFCOMP_STORE_SRC,
            VFCOMP_STORE_SRC,
            VFCOMP_STORE_SRC,
            component3,
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
            StreamoutProofExperiment::PositionSlot0Xyzw => {
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
            StreamoutProofExperiment::HeaderAndPositionSlots01 => {
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
            StreamoutProofExperiment::MesaNoVsRectlist => {
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    1,
                    0,
                    SURFACE_FORMAT_R32G32B32A32_FLOAT,
                    VFCOMP_STORE_SRC,
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
                push_vertex_element_state(
                    batch_dwords,
                    &mut cursor,
                    1,
                    16,
                    SURFACE_FORMAT_R32G32B32A32_FLOAT,
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
    log_batch_offset(cursor, "MI_STORE_DATA_IMM post-vf");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_POST_VF_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_POST_VF,
    )?;

    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_VS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_VS)?;
    push(batch_dwords, &mut cursor, urb_vs_alloc_dw1)?;
    push(batch_dwords, &mut cursor, urb_vs_alloc_dw2)?;

    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_HS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_HS)?;
    push(batch_dwords, &mut cursor, disabled_stage_urb_alloc_dw1)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_DS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_DS)?;
    push(batch_dwords, &mut cursor, disabled_stage_urb_alloc_dw1)?;
    push(batch_dwords, &mut cursor, 0)?;
    log_batch_offset(cursor, "3DSTATE_URB_ALLOC_GS");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_URB_ALLOC_GS)?;
    push(batch_dwords, &mut cursor, disabled_stage_urb_alloc_dw1)?;
    push(batch_dwords, &mut cursor, 0)?;

    if let Some(config) = vs_config {
        let pipeline = config.pipeline;
        let shader_layout = config.shader_layout;
        let vs_ksp_offset = shader_layout.vs.code_offset_bytes + shader_layout.vs.ksp_offset_bytes;
        let baked_vs_urb_output_length = pipeline.vs.meta.urb_entry_output_length;
        let vs_accesses_uav = shader_layout.vs.accesses_uav;
        let vs_dw3 = ((pipeline.vs.meta.kernel.binding_table_entry_count as u32) << 18)
            | ((vs_accesses_uav as u32) << 12)
            | (sampler_count_encoding(pipeline.vs.meta.kernel.sampler_count) << 27);
        let programmed_vs_grf_start = front_end_contract
            .vs_dispatch_grf_start_override
            .unwrap_or(pipeline.vs.meta.kernel.grf_start_register);
        let applied_vs_grf_start = triangle_vs_dispatch_grf_start_register(programmed_vs_grf_start);
        let vs_dw6 = ((front_end_contract.vs_urb_read_offset as u32) << 4)
            | ((front_end_contract.vs_urb_read_length as u32) << 11)
            | (applied_vs_grf_start << 20);
        let applied_vs_max_threads_field = front_end_contract
            .vs_max_threads_field_override
            .unwrap_or_else(|| {
                triangle_vs_max_threads_field(warm.device_id, pipeline.vs.meta.max_threads)
            });
        let vs_dw7 = 1
            | (1 << 2)
            | (1 << 10)
            | ((front_end_contract.vs_simd8_single_instance_dispatch as u32) << 9)
            | (applied_vs_max_threads_field << 22);
        let vs_dw8 = front_end_contract
            .vs_dw8_override
            .unwrap_or((programmed_vs_urb_output_length.saturating_sub(1) as u32) << 16);
        let vs_output_read_offset = (vs_dw8 >> 21) & 0x3F;
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
        intel_render_focus_log!(
            "intel/render: probe-vs ksp=0x{:08X} dw3=0x{:08X} accesses_uav={} simd8_single={} dw6=0x{:08X} dw7=0x{:08X} dw8=0x{:08X} dw8_override={} vs_output_read_offset={} baked_max_threads={} applied_max_threads_field={} baked_urb_out_len={} programmed_urb_out_len={} programmed_urb_entries={} programmed_urb_start={} vs_urb_read_offset={} vs_urb_read_length={} baked_grf_start={} programmed_grf_start={} applied_grf_start={} dispatch={:?} contract={} note=vs-streamout-proof-dw8-contract-or-minus-one\n",
            vs_ksp_offset & !0x3F,
            vs_dw3,
            vs_accesses_uav as u8,
            front_end_contract.vs_simd8_single_instance_dispatch as u8,
            vs_dw6,
            vs_dw7,
            vs_dw8,
            front_end_contract.vs_dw8_override.is_some() as u8,
            vs_output_read_offset,
            pipeline.vs.meta.max_threads,
            applied_vs_max_threads_field,
            baked_vs_urb_output_length,
            programmed_vs_urb_output_length,
            programmed_vs_urb_entries,
            programmed_vs_urb_start,
            front_end_contract.vs_urb_read_offset,
            front_end_contract.vs_urb_read_length,
            pipeline.vs.meta.kernel.grf_start_register,
            programmed_vs_grf_start,
            applied_vs_grf_start,
            pipeline.vs.meta.kernel.dispatch_mode,
            front_end_contract.label,
        );
        if pipeline.vs.code.len() >= 4 {
            let send_base = pipeline.vs.code.len().saturating_sub(4);
            let send_dw0 = pipeline.vs.code[send_base];
            let send_dw1 = pipeline.vs.code[send_base + 1];
            let send_dw2 = pipeline.vs.code[send_base + 2];
            let send_dw3 = pipeline.vs.code[send_base + 3];
            let send_sfid = (send_dw2 >> 28) & 0xF;
            let send_src0_grf = (send_dw2 >> 8) & 0xFF;
            let send_src0_mlen = (send_dw2 >> 3) & 0xF;
            let send_src1_grf = (send_dw3 >> 8) & 0xFF;
            let send_ex_mlen = (send_dw3 >> 24) & 0x1F;
            intel_render_focus_log!(
                "intel/render: probe-vs-urb-send send=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] sfid={} src0_g={} src0_mlen={} src1_g={} ex_mlen={} first_payload=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] note=mesa-disasm-shape-is-src0-g127-mlen1-src1-g119-ex_mlen8-urb-write-eot\n",
                send_dw0,
                send_dw1,
                send_dw2,
                send_dw3,
                send_sfid,
                send_src0_grf,
                send_src0_mlen,
                send_src1_grf,
                send_ex_mlen,
                pipeline.vs.code[0],
                pipeline.vs.code.get(1).copied().unwrap_or(0),
                pipeline.vs.code.get(2).copied().unwrap_or(0),
                pipeline.vs.code.get(3).copied().unwrap_or(0),
            );
        }
        intel_render_verbose_log!(
            "intel/render: probe-vs-export note={} position_only={} generic_attrs=0 baked_urb_bytes={} programmed_urb_bytes={} expected_vue=header+position-only\n",
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
    log_batch_offset(cursor, "MI_STORE_DATA_IMM post-vs");
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
    log_batch_offset(cursor, "MI_STORE_DATA_IMM post-ps-state");
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
        let streamout_decl_dword0 = streamout_experiment.so_decl_stream_offsets();
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
    if matches!(streamout_experiment, StreamoutProofExperiment::HeaderAndPositionSlots01) {
        push(batch_dwords, &mut cursor, streamout_decl_dword4)?;
        push(batch_dwords, &mut cursor, streamout_decl_dword5)?;
    }
    crate::log!(
        "intel/render: {} decl experiment={} read_offset={} read_length={} so_pitch={} decl=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] slot_contract={}\n",
        submit_label,
        streamout_experiment.label(),
        streamout_experiment.vertex_read_offset(),
        streamout_experiment.vertex_read_length_field(),
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
        "intel/render: {} contract experiment={} stages_disabled={} sbe[read_offset={} read_length={} num_sf_attrs=1 force_offset=1 force_length=1] urb_vs[alloc_len={} start={} entries={}] vb[index=0 pitch={} size=0x{:X}] streamout[read_offset={} read_length_field={} rendering_disable={} sol_write_enable={} buffer0_enable={} stats_enable={} pitch={} so_gpu=0x{:X} size_dwords=0x{:X}] sobuf0[enable={} stream_offset_write_enable={} offset_addr_enable={} offset_mode={}] topo={}\n",
        submit_label,
        streamout_experiment.label(),
        if vs_config.is_some() {
            "hs|te|ds|gs|ps"
        } else {
            "vs|hs|te|ds|gs|ps"
        },
        sbe_vertex_read_offset,
        sbe_vertex_read_length,
        programmed_vs_urb_output_length,
        programmed_vs_urb_start,
        programmed_vs_urb_entries,
        draw.vertex_stride,
        vb_size_bytes,
        (streamout_dw2 >> 5) & 0x1F,
        streamout_dw2 & 0x1F,
        (streamout_dw1 >> 30) & 0x1,
        ((streamout_dw1 >> 31) & 0x1) & (1 ^ ((streamout_dw1 >> 30) & 0x1)),
        (streamout_dw1 >> 8) & 0x1,
        (streamout_dw1 >> 25) & 0x1,
        streamout_dw3 & 0xFFF,
        GPU_VA_STREAMOUT_BASE,
        streamout_surface_size_dwords,
        (so_buffer_index_dw1 >> 31) & 0x1,
        (so_buffer_index_dw1 >> 21) & 0x1,
        (so_buffer_index_dw1 >> 20) & 0x1,
        decode_streamout_offset_mode_name(
            (so_buffer_index_dw1 >> 21) & 0x1,
            (so_buffer_index_dw1 >> 20) & 0x1,
        ),
        primitive_topology_label(batch_mode.topology()),
    );
    log_batch_offset(cursor, "3DSTATE_STREAMOUT refresh-after-so-decl");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_STREAMOUT)?;
    push(batch_dwords, &mut cursor, streamout_dw1)?;
    push(batch_dwords, &mut cursor, streamout_dw2)?;
    push(batch_dwords, &mut cursor, streamout_dw3)?;
    push(batch_dwords, &mut cursor, streamout_dw4)?;
    intel_render_focus_log!(
        "intel/render: {} streamout-refresh-after-decl emitted=1 reason=sol-state-after-buffer-and-decl-order-probe\n",
        submit_label,
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

    log_batch_offset(cursor, "3DSTATE_VF streamout-late-mesa-active");
    push(batch_dwords, &mut cursor, CMD_3DSTATE_VF_MESA_ACTIVE)?;
    push(batch_dwords, &mut cursor, MESA_ACTIVE_VF_CUT_INDEX)?;
    intel_render_focus_log!(
        "intel/render: probe-vf-state emit=streamout-late-before-primitive cmd=0x{:08X} cut_index=0x{:08X} component_packing=1 source=mesa-active-triangle hypothesis=vf-component-packing-handoff-unblocks-urb-streamout\n",
        CMD_3DSTATE_VF_MESA_ACTIVE,
        MESA_ACTIVE_VF_CUT_INDEX,
    );

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

    if matches!(
        post_draw_sync_variant,
        PostDrawSyncVariant::NoPostDrawPipeControl
    ) {
        log_batch_offset(cursor, "skip PIPE_CONTROL post-3d-light-marker");
        intel_render_focus_log!(
            "intel/render: vs-streamout-proof-postdraw action=skip-light-pipe-control variant={} source=pdoane-reference-style reason=avoid-known-postdraw-pc-stall\n",
            post_draw_sync_variant.label(),
        );
    } else {
        let light_sync_flags = post_draw_sync_variant.light_sync_flags();
        log_batch_offset(cursor, "PIPE_CONTROL post-3d-light-marker");
        intel_render_focus_log!(
            "intel/render: vs-streamout-proof-postdraw action=emit-light-pipe-control variant={} light_flags=0x{:08X} light_post_sync={} light_cs_stall={}\n",
            post_draw_sync_variant.label(),
            light_sync_flags,
            post_draw_sync_variant.light_post_sync_enabled() as u8,
            post_draw_sync_variant.light_cs_stall_enabled() as u8,
        );
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
    }

    log_batch_offset(cursor, "MI_STORE_DATA_IMM final-after-light");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        result_gpu_addr + (RESULT_SLOT_FINAL_AFTER_LIGHT_DWORD as u64) * 4,
        RCS_EXEC_RESULT_DRAW_FINAL_AFTER_LIGHT,
    )?;
    const STREAMOUT_DEBUG_MARKER_OFFSET_BYTES: u64 = 0x80;
    log_batch_offset(cursor, "MI_STORE_DATA_IMM streamout-debug-marker");
    push_store_data_imm(
        batch_dwords,
        &mut cursor,
        GPU_VA_STREAMOUT_BASE + STREAMOUT_DEBUG_MARKER_OFFSET_BYTES,
        0xC0DE_F00D,
    )?;
    intel_render_focus_log!(
        "intel/render: {} streamout-debug-marker gpu=0x{:X} offset=0x{:X} value=0xC0DEF00D purpose=ggtt-readback-after-so\n",
        submit_label,
        GPU_VA_STREAMOUT_BASE + STREAMOUT_DEBUG_MARKER_OFFSET_BYTES,
        STREAMOUT_DEBUG_MARKER_OFFSET_BYTES,
    );

    if let Some(heavy_sync_flags) = post_draw_sync_variant.heavy_sync_flags() {
        let heavy_header_flags = post_draw_sync_variant.heavy_header_flags();
        log_batch_offset(cursor, "PIPE_CONTROL post-3d-heavy-sync");
        intel_render_focus_log!(
            "intel/render: vs-streamout-proof-postdraw action=emit-heavy-pipe-control variant={} header_flags=0x{:08X} heavy_flags=0x{:08X}\n",
            post_draw_sync_variant.label(),
            heavy_header_flags,
            heavy_sync_flags,
        );
        push_pipe_control_post_sync_imm(
            batch_dwords,
            &mut cursor,
            heavy_header_flags,
            heavy_sync_flags,
            result_gpu_addr + (RESULT_SLOT_POST3D_PIPE_CONTROL_LO_DWORD as u64) * 4,
            post3d_value,
        )?;
    } else {
        log_batch_offset(cursor, "skip PIPE_CONTROL post-3d-heavy-sync");
        intel_render_focus_log!(
            "intel/render: vs-streamout-proof-postdraw action=skip-heavy-pipe-control variant={} source=pdoane-reference-style reason=proof-only-final-marker-retire\n",
            post_draw_sync_variant.label(),
        );
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
        "intel/render: 3dprimitive-setup mode={:?} topo={} vertices={} start_vertex=0 instances=1 start_instance=0 base_vertex=0 vb=0x{:X} stride={} rt=0x{:X} pitch=0x{:X} rect={}x{} postdraw_sync={} light_flags=0x{:08X}\n",
        batch_mode,
        primitive_topology_label(batch_mode.topology()),
        draw.vertex_count,
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

fn encode_3d_no_draw_probe_batch(
    batch_dwords: &mut [u32],
    warm: RenderWarmState,
    result_gpu_addr: u64,
    done_value: u32,
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
    push(batch_dwords, &mut cursor, RENDER_MOCS << 16)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, 0)?;
    push_sba_address(batch_dwords, &mut cursor, true, RENDER_MOCS, GPU_VA_DRAW_STATE_BASE)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.vertex_len)?;
    push_sba_size(batch_dwords, &mut cursor, true, warm.draw_state_len)?;
    for _ in 0..6 {
        push(batch_dwords, &mut cursor, 0)?;
    }
    push_store_data_imm(batch_dwords, &mut cursor, result_gpu_addr, done_value)?;
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;
    Ok(cursor * core::mem::size_of::<u32>())
}
