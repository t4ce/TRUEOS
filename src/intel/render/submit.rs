fn submit_warm_render_batch(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    expected_result: u32,
    expected_result_slot_dword: usize,
    submit_name: &'static str,
) -> bool {
    let stats_before = capture_triangle_stage_stats(dev);
    let surface_samples_before = if is_surface_draw_submit_name(submit_name) {
        crate::intel::display::capture_primary_surface_samples()
    } else {
        None
    };
    if is_triangle_debug_submit_name(submit_name) {
        log_triangle_stage_stats(submit_name, "before-submit", true, stats_before, None);
    }
    let ring_tail_bytes = build_ring_batch_start(warm, GPU_VA_BATCH_BASE);
    let Some(ring_ctl) = ring_ctl_value(warm.ring_len) else {
        return false;
    };
    if !init_gen12_lrc_context_image(
        warm,
        GPU_VA_RING_BASE as u32,
        ring_tail_bytes as u32,
        ring_ctl,
    ) {
        return false;
    }
    let (context_desc_lo, context_desc_hi) = build_execlist_context_descriptor(GPU_VA_CONTEXT_BASE);
    write_lrc_ring_tail(warm, ring_tail_bytes as u32);
    let pphwsp_gpu = (GPU_VA_CONTEXT_BASE & !0xFFF) as u32;

    crate::intel::mmio_write(
        dev,
        RCS_RING_MODE_GEN7,
        masked_bit_enable(GFX_RUN_LIST_ENABLE | GEN11_GFX_DISABLE_LEGACY_MODE),
    );
    let ctx_ctl_after = masked_bits_update(
        CTX_CTRL_RS_CTX_ENABLE,
        CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT
            | CTX_CTRL_ENGINE_CTX_SAVE_INHIBIT
            | CTX_CTRL_INHIBIT_SYN_CTX_SWITCH,
    );
    crate::intel::mmio_write(dev, RCS_RING_CONTEXT_CONTROL, ctx_ctl_after);
    crate::intel::mmio_write(dev, RCS_RING_CONTEXT_CONTROL_REF, ctx_ctl_after);
    crate::intel::mmio_write(dev, RCS_RING_HWS_PGA, pphwsp_gpu);
    let hws_after = crate::intel::mmio_read(dev, RCS_RING_HWS_PGA);

    core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    execlist_submit_port_push(dev, context_desc_lo, context_desc_hi, 0, 0);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_CONTROL, EL_CTRL_LOAD);

    if should_log_primary_probe_detail() {
        crate::log!(
            "intel/render: {} execlist-start desc=0x{:08X}:0x{:08X} hws=0x{:08X} sq=0x{:08X}:0x{:08X} ctx_ctl=0x{:08X}\n",
            submit_name,
            context_desc_hi,
            context_desc_lo,
            hws_after,
            crate::intel::mmio_read(dev, RCS_RING_EXECLIST_SQ_HI),
            crate::intel::mmio_read(dev, RCS_RING_EXECLIST_SQ_LO),
            crate::intel::mmio_read(dev, RCS_RING_CONTEXT_CONTROL)
        );
    }

    let mut completed = false;
    let mut iter = 0usize;
    while iter < 4096 {
        let result0 = read_result_dword(warm, RESULT_SLOT_PRE3D_DWORD);
        let result1 = read_result_dword(warm, RESULT_SLOT_POST3D_DWORD);
        let result2 = read_result_dword(warm, RESULT_SLOT_FINAL_DWORD);
        let result3 = read_result_dword(warm, RESULT_SLOT_POST_VF_DWORD);
        let result4 = read_result_dword(warm, RESULT_SLOT_POST_VS_DWORD);
        let result5 = read_result_dword(warm, RESULT_SLOT_POST_PS_STATE_DWORD);
        let result6 = read_result_dword(warm, RESULT_SLOT_POST_CLIP_DWORD);
        let result7 = read_result_dword(warm, RESULT_SLOT_POST_RASTER_DWORD);
        let result_post3d_eop = read_result_dword(warm, RESULT_SLOT_POST3D_PIPE_CONTROL_LO_DWORD);
        let result_post3d_eop_hi =
            read_result_dword(warm, RESULT_SLOT_POST3D_PIPE_CONTROL_HI_DWORD);
        let result_post3d_light =
            read_result_dword(warm, RESULT_SLOT_POST3D_LIGHT_PIPE_CONTROL_LO_DWORD);
        let result_post3d_light_hi =
            read_result_dword(warm, RESULT_SLOT_POST3D_LIGHT_PIPE_CONTROL_HI_DWORD);
        let result_final_after_light = read_result_dword(warm, RESULT_SLOT_FINAL_AFTER_LIGHT_DWORD);
        let result_pre_light_pc = read_result_dword(warm, RESULT_SLOT_PRE_LIGHT_PC_DWORD);
        let observed = match expected_result_slot_dword {
            RESULT_SLOT_PRE3D_DWORD => result0,
            RESULT_SLOT_POST3D_DWORD => result1,
            RESULT_SLOT_FINAL_DWORD => result2,
            RESULT_SLOT_POST_VF_DWORD => result3,
            RESULT_SLOT_POST_VS_DWORD => result4,
            RESULT_SLOT_POST_PS_STATE_DWORD => result5,
            RESULT_SLOT_POST_CLIP_DWORD => result6,
            RESULT_SLOT_POST_RASTER_DWORD => result7,
            RESULT_SLOT_POST3D_PIPE_CONTROL_LO_DWORD => result_post3d_eop,
            RESULT_SLOT_POST3D_PIPE_CONTROL_HI_DWORD => result_post3d_eop_hi,
            RESULT_SLOT_POST3D_LIGHT_PIPE_CONTROL_LO_DWORD => result_post3d_light,
            RESULT_SLOT_POST3D_LIGHT_PIPE_CONTROL_HI_DWORD => result_post3d_light_hi,
            RESULT_SLOT_FINAL_AFTER_LIGHT_DWORD => result_final_after_light,
            RESULT_SLOT_PRE_LIGHT_PC_DWORD => result_pre_light_pc,
            RESULT_SLOT_GPGPU_PREFLIGHT_MARKER_DWORD => read_result_dword(
                warm,
                RESULT_SLOT_GPGPU_PREFLIGHT_MARKER_DWORD,
            ),
            _ => result0,
        };
        if observed == expected_result {
            completed = true;
            break;
        }
        if should_log_primary_probe_detail()
            && (iter == 0 || iter == 256 || iter == 1024 || iter == 4095)
        {
            let poll_stats = capture_triangle_stage_stats(dev);
            crate::log!(
                "intel/render: {} poll iter={} head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X} execlist_lo=0x{:08X} execlist_hi=0x{:08X} result0=0x{:08X} result1=0x{:08X} result2=0x{:08X}\n",
                submit_name,
                iter,
                crate::intel::mmio_read(dev, RCS_RING_HEAD),
                crate::intel::mmio_read(dev, RCS_RING_TAIL),
                crate::intel::mmio_read(dev, RCS_RING_ACTHD),
                crate::intel::mmio_read(dev, RCS_RING_IPEIR),
                crate::intel::mmio_read(dev, RCS_RING_IPEHR),
                crate::intel::mmio_read(dev, RCS_RING_EIR),
                crate::intel::mmio_read(dev, RCS_RING_EXECLIST_STATUS_LO),
                crate::intel::mmio_read(dev, RCS_RING_EXECLIST_STATUS_HI),
                result0,
                result1,
                result2
            );
            intel_render_verbose_log!(
                "intel/render: {} poll-stage iter={} post_vf=0x{:08X} post_vs=0x{:08X} post_ps_state=0x{:08X} post_clip=0x{:08X} post_raster=0x{:08X} pre_light_pc=0x{:08X} post3d_light=0x{:08X} post3d_light_hi=0x{:08X} final_after_light=0x{:08X} post3d_eop=0x{:08X} post3d_hi=0x{:08X}\n",
                submit_name,
                iter,
                result3,
                result4,
                result5,
                result6,
                result7,
                result_pre_light_pc,
                result_post3d_light,
                result_post3d_light_hi,
                result_final_after_light,
                result_post3d_eop,
                result_post3d_eop_hi
            );
            intel_render_verbose_log!(
                "intel/render: {} poll-counters iter={} ia_vtx={} ia_prim={} vs={} hs={} ds={} gs={} gs_prim={} cl={} cl_prim={} ps={} cps={} ps_depth={} so0={} so_write0={}\n",
                submit_name,
                iter,
                poll_stats.ia_vertices,
                poll_stats.ia_primitives,
                poll_stats.vs_invocations,
                poll_stats.hs_invocations,
                poll_stats.ds_invocations,
                poll_stats.gs_invocations,
                poll_stats.gs_primitives,
                poll_stats.cl_invocations,
                poll_stats.cl_primitives,
                poll_stats.ps_invocations,
                poll_stats.cps_invocations,
                poll_stats.ps_depth,
                poll_stats.so_prims_written_0,
                poll_stats.so_write_offset_0,
            );
        }
        core::hint::spin_loop();
        iter += 1;
    }

    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    let result0 = read_result_dword(warm, RESULT_SLOT_PRE3D_DWORD);
    let result1 = read_result_dword(warm, RESULT_SLOT_POST3D_DWORD);
    let result2 = read_result_dword(warm, RESULT_SLOT_FINAL_DWORD);
    let result3 = read_result_dword(warm, RESULT_SLOT_POST_VF_DWORD);
    let result4 = read_result_dword(warm, RESULT_SLOT_POST_VS_DWORD);
    let result5 = read_result_dword(warm, RESULT_SLOT_POST_PS_STATE_DWORD);
    let result6 = read_result_dword(warm, RESULT_SLOT_POST_CLIP_DWORD);
    let result7 = read_result_dword(warm, RESULT_SLOT_POST_RASTER_DWORD);
    let result_post3d_eop = read_result_dword(warm, RESULT_SLOT_POST3D_PIPE_CONTROL_LO_DWORD);
    let result_post3d_eop_hi = read_result_dword(warm, RESULT_SLOT_POST3D_PIPE_CONTROL_HI_DWORD);
    let result_post3d_light =
        read_result_dword(warm, RESULT_SLOT_POST3D_LIGHT_PIPE_CONTROL_LO_DWORD);
    let result_post3d_light_hi =
        read_result_dword(warm, RESULT_SLOT_POST3D_LIGHT_PIPE_CONTROL_HI_DWORD);
    let result_final_after_light = read_result_dword(warm, RESULT_SLOT_FINAL_AFTER_LIGHT_DWORD);
    let result_pre_light_pc = read_result_dword(warm, RESULT_SLOT_PRE_LIGHT_PC_DWORD);
    if should_log_primary_probe_detail() {
        crate::log!(
            "intel/render: {} complete={} result0=0x{:08X} result1=0x{:08X} result2=0x{:08X} post_vf=0x{:08X} post_vs=0x{:08X} post_ps_state=0x{:08X} post_clip=0x{:08X} post_raster=0x{:08X} pre_light_pc=0x{:08X} post3d_light=0x{:08X} post3d_light_hi=0x{:08X} final_after_light=0x{:08X} post3d_eop=0x{:08X} post3d_hi=0x{:08X} ctl=0x{:08X} instdone=0x{:08X}\n",
            submit_name,
            completed as u8,
            result0,
            result1,
            result2,
            result3,
            result4,
            result5,
            result6,
            result7,
            result_pre_light_pc,
            result_post3d_light,
            result_post3d_light_hi,
            result_final_after_light,
            result_post3d_eop,
            result_post3d_eop_hi,
            crate::intel::mmio_read(dev, RCS_RING_CTL),
            crate::intel::mmio_read(dev, RCS_RING_INSTDONE)
        );
        crate::intel::display::log_primary_surface_samples("post-render");
    }
    if is_triangle_debug_submit_name(submit_name) {
        intel_render_focus_log!(
            "intel/render: {} batch-submit-proof completed={} start_marker={} pre_light_pc_marker={} post3d_light_marker={} final_after_light_marker={} post3d_marker={} final_marker={} expected_slot={} expected=0x{:08X} acthd=0x{:08X} ipehr=0x{:08X} does_not_prove=3d_stage_progress\n",
            submit_name,
            completed as u8,
            (result0 == RCS_EXEC_RESULT_DRAW_PRE3D) as u8,
            (result_pre_light_pc == RCS_EXEC_RESULT_DRAW_PRE_LIGHT_PC) as u8,
            (result_post3d_light == RCS_EXEC_RESULT_DRAW_POST3D && result_post3d_light_hi == 0)
                as u8,
            (result_final_after_light == RCS_EXEC_RESULT_DRAW_FINAL_AFTER_LIGHT) as u8,
            (result_post3d_eop == RCS_EXEC_RESULT_DRAW_POST3D) as u8,
            (result2 == RCS_EXEC_RESULT_DONE) as u8,
            expected_result_slot_dword,
            expected_result,
            crate::intel::mmio_read(dev, RCS_RING_ACTHD),
            crate::intel::mmio_read(dev, RCS_RING_IPEHR)
        );
        intel_render_focus_log!(
            "intel/render: 3dprimitive-result completed={} pre3d={} pre_light_pc={} post3d_light={} final_after_light={} post3d_heavy={} final={} vf={} vs={} ps_state={} clip={} raster={} pre_draw_packet_markers={} clip_raster_packet_markers={} post_draw_pre_light_markers={} post_draw_light_markers={} post_draw_final_after_light_markers={} post_draw_heavy_markers={} post_draw_retire_markers={} acthd=0x{:08X} ipehr=0x{:08X}\n",
            completed as u8,
            result0 == RCS_EXEC_RESULT_DRAW_PRE3D,
            result_pre_light_pc == RCS_EXEC_RESULT_DRAW_PRE_LIGHT_PC,
            result_post3d_light == RCS_EXEC_RESULT_DRAW_POST3D && result_post3d_light_hi == 0,
            result_final_after_light == RCS_EXEC_RESULT_DRAW_FINAL_AFTER_LIGHT,
            result_post3d_eop == RCS_EXEC_RESULT_DRAW_POST3D && result_post3d_eop_hi == 0,
            result2 == RCS_EXEC_RESULT_DONE,
            result3 == RCS_EXEC_RESULT_DRAW_POST_VF,
            result4 == RCS_EXEC_RESULT_DRAW_POST_VS,
            result5 == RCS_EXEC_RESULT_DRAW_POST_PS_STATE,
            result6 == RCS_EXEC_RESULT_DRAW_POST_CLIP,
            result7 == RCS_EXEC_RESULT_DRAW_POST_RASTER,
            ((result3 == RCS_EXEC_RESULT_DRAW_POST_VF) && (result4 == RCS_EXEC_RESULT_DRAW_POST_VS))
                as u8,
            ((result6 == RCS_EXEC_RESULT_DRAW_POST_CLIP)
                && (result7 == RCS_EXEC_RESULT_DRAW_POST_RASTER)) as u8,
            (result_pre_light_pc == RCS_EXEC_RESULT_DRAW_PRE_LIGHT_PC) as u8,
            (result_post3d_light == RCS_EXEC_RESULT_DRAW_POST3D && result_post3d_light_hi == 0)
                as u8,
            (result_final_after_light == RCS_EXEC_RESULT_DRAW_FINAL_AFTER_LIGHT) as u8,
            (result_post3d_eop == RCS_EXEC_RESULT_DRAW_POST3D && result_post3d_eop_hi == 0) as u8,
            (result2 == RCS_EXEC_RESULT_DONE) as u8,
            crate::intel::mmio_read(dev, RCS_RING_ACTHD),
            crate::intel::mmio_read(dev, RCS_RING_IPEHR)
        );
        if let Some(postdraw_variant) = PostDrawSyncVariant::from_submit_name(submit_name) {
            let pre_light_pc_ok = result_pre_light_pc == RCS_EXEC_RESULT_DRAW_PRE_LIGHT_PC;
            let final_after_light_ok =
                result_final_after_light == RCS_EXEC_RESULT_DRAW_FINAL_AFTER_LIGHT;
            intel_render_focus_log!(
                "intel/render: {} postdraw-flush-spectrum-proof accepted={} variant={} heavy_flags=0x{:08X} post3d_light={} final_after_light={} post3d_heavy={} final={} acthd=0x{:08X} ipehr=0x{:08X} does_not_prove=rt_write\n",
                submit_name,
                (result2 == RCS_EXEC_RESULT_DONE) as u8,
                postdraw_variant.label(),
                postdraw_variant.heavy_sync_flags().unwrap_or(0),
                (result_post3d_light == RCS_EXEC_RESULT_DRAW_POST3D && result_post3d_light_hi == 0)
                    as u8,
                (result_final_after_light == RCS_EXEC_RESULT_DRAW_FINAL_AFTER_LIGHT) as u8,
                (result_post3d_eop == RCS_EXEC_RESULT_DRAW_POST3D && result_post3d_eop_hi == 0)
                    as u8,
                (result2 == RCS_EXEC_RESULT_DONE) as u8,
                crate::intel::mmio_read(dev, RCS_RING_ACTHD),
                crate::intel::mmio_read(dev, RCS_RING_IPEHR)
            );
            intel_render_focus_log!(
                "intel/render: {} pc-retire-triad-proof accepted={} variant={} light_flags=0x{:08X} light_postsync={} light_cs_stall={} before_light={} post3d_light={} final_after_light={} post3d_heavy={} final={} acthd=0x{:08X} ipehr=0x{:08X} does_not_prove=rt_write\n",
                submit_name,
                (pre_light_pc_ok && final_after_light_ok) as u8,
                postdraw_variant.label(),
                postdraw_variant.light_sync_flags(),
                postdraw_variant.light_post_sync_enabled() as u8,
                postdraw_variant.light_cs_stall_enabled() as u8,
                pre_light_pc_ok as u8,
                (result_post3d_light == RCS_EXEC_RESULT_DRAW_POST3D && result_post3d_light_hi == 0)
                    as u8,
                final_after_light_ok as u8,
                (result_post3d_eop == RCS_EXEC_RESULT_DRAW_POST3D && result_post3d_eop_hi == 0)
                    as u8,
                (result2 == RCS_EXEC_RESULT_DONE) as u8,
                crate::intel::mmio_read(dev, RCS_RING_ACTHD),
                crate::intel::mmio_read(dev, RCS_RING_IPEHR)
            );
        }
    }
    if !completed && is_triangle_debug_submit_name(submit_name) {
        let acthd = crate::intel::mmio_read(dev, RCS_RING_ACTHD);
        let acthd_batch_off = acthd.saturating_sub(GPU_VA_BATCH_BASE as u32);
        let instdone_geom = crate::intel::mmio_read(dev, INSTDONE_GEOM);
        let sc_instdone = crate::intel::mmio_read(dev, SC_INSTDONE);
        let sc_extra = crate::intel::mmio_read(dev, SC_INSTDONE_EXTRA);
        let sc_extra2 = crate::intel::mmio_read(dev, SC_INSTDONE_EXTRA2);
        intel_render_focus_log!(
            "intel/render: {} stall-detail acthd_batch_off=0x{:08X} ipehr=0x{:08X} instdone_geom=0x{:08X} sc_instdone=0x{:08X} sc_extra=0x{:08X} sc_extra2=0x{:08X}\n",
            submit_name,
            acthd_batch_off,
            crate::intel::mmio_read(dev, RCS_RING_IPEHR),
            instdone_geom,
            sc_instdone,
            sc_extra,
            sc_extra2,
        );
        log_not_done_units(submit_name, "stall-geom-not-done", instdone_geom, GEOM_INSTDONE_BITS);
        log_not_done_units(submit_name, "stall-sc-not-done", sc_instdone, SC_INSTDONE_BITS);
        log_not_done_units(
            submit_name,
            "stall-sc-extra-not-done",
            sc_extra,
            SC_INSTDONE_EXTRA_BITS,
        );
        log_not_done_units(
            submit_name,
            "stall-sc-extra2-not-done",
            sc_extra2,
            SC_INSTDONE_EXTRA2_BITS,
        );
    }
    if is_triangle_debug_submit_name(submit_name) {
        let stats_after = capture_triangle_stage_stats(dev);
        log_triangle_stage_stats(
            submit_name,
            "after-submit",
            completed,
            stats_after,
            Some(stats_before),
        );
        log_triangle_stage_frontier(
            submit_name,
            completed,
            stats_before,
            stats_after,
            result_pre_light_pc,
            result_post3d_light,
            result_post3d_light_hi,
            result_final_after_light,
            result_post3d_eop,
            result_post3d_eop_hi,
            result2,
            result3,
            result4,
            result5,
            result6,
            result7,
        );
        log_triangle_stage_diagnosis(submit_name, completed, stats_before, stats_after);
        log_triangle_named_proofs(
            dev,
            submit_name,
            completed,
            stats_before,
            stats_after,
            result3,
            result4,
            result5,
            result6,
            result7,
        );
    }
    if is_surface_draw_submit_name(submit_name) {
        if let (Some(before), Some(after)) =
            (surface_samples_before, crate::intel::display::capture_primary_surface_samples())
        {
            let stats_after = capture_triangle_stage_stats(dev);
            let delta = stats_after.delta_since(stats_before);
            let any_change = after.any_changed_since(before);
            let triangle_change = after.triangle_points_changed_since(before);
            intel_render_focus_log!(
                "intel/render: {} ps-rt-proof accepted={} ps_delta={} rt_any_change={} rt_triangle_change={} does_not_prove=display_scanout\n",
                submit_name,
                (delta.ps_invocations > 0 && any_change) as u8,
                delta.ps_invocations,
                any_change as u8,
                triangle_change as u8,
            );
            intel_render_focus_log!(
                "intel/render: {} render-target completed={} any_change={} triangle_change={} apex={}=>{} centroid={}=>{} left={}=>{} right={}=>{} center={}=>{}\n",
                submit_name,
                completed as u8,
                any_change as u8,
                triangle_change as u8,
                before.apex,
                after.apex,
                before.centroid,
                after.centroid,
                before.left,
                after.left,
                before.right,
                after.right,
                before.center,
                after.center,
            );
        }
    }
    if is_surface_draw_submit_name(submit_name) {
        log_triangle_demo_stats(dev, completed);
    }
    if completed && is_surface_draw_submit_name(submit_name) {
        let label = match submit_name {
            "vf-draw-path" => "post-vf-draw-path",
            "vs-draw-frontier" => "post-vs-draw-frontier",
            _ => "post-draw-path",
        };
        let kicked = crate::intel::display::kick_primary_surface_scanout(label);
        intel_render_verbose_log!("intel/render: {} scanout-kick={}\n", submit_name, kicked as u8);
        crate::intel::display::log_pipe_live_scanout_state(label);
    }
    completed
}

fn log_triangle_demo_stats(dev: crate::intel::Dev, completed: bool) {
    let mut values = [0u32; TRIANGLE_STATS_LOG.len()];
    for (idx, stat) in TRIANGLE_STATS_LOG.iter().copied().enumerate() {
        let Some(offset) = stat.mmio_offset() else {
            continue;
        };
        values[idx] = crate::intel::mmio_read(dev, offset);
    }

    intel_render_verbose_log!(
        "intel/render: triangle-stats completed={} {}={} {}={} {}={}\n",
        completed as u8,
        TRIANGLE_STATS_LOG[0].symbol(),
        values[0],
        TRIANGLE_STATS_LOG[1].symbol(),
        values[1],
        TRIANGLE_STATS_LOG[2].symbol(),
        values[2]
    );
}

fn triangle_vs_max_threads_field(device_id: u16, baked_max_threads: u16) -> u32 {
    if device_is_gfx125(device_id) {
        // Mesa advertises ADL-S gfx12 max_vs_threads = 546 and programs the
        // packet with max_vs_threads - 1.
        545
    } else {
        baked_max_threads.saturating_sub(1) as u32
    }
}

fn triangle_vs_dispatch_grf_start_register(baked_grf_start: u8) -> u32 {
    // Trust the compiler-exported packet metadata here. The disassembly reads
    // payload from g1..g4, but the packet field is not a raw visible-GRF index
    // dump; forcing an extra +1 made this path less Mesa-like without proving
    // that the compiler metadata was wrong.
    baked_grf_start as u32
}

fn device_is_gfx125(device_id: u16) -> bool {
    matches!(device_id, 0x4680 | 0x4682 | 0x4688 | 0x468A | 0x468B | 0x4690 | 0x4692 | 0x4693)
}

fn decode_clip_mode_name(mode: u32) -> &'static str {
    match mode {
        0 => "CLIPMODE_NORMAL",
        3 => "CLIPMODE_REJECT_ALL",
        4 => "CLIPMODE_ACCEPT_ALL",
        _ => "unknown",
    }
}

fn decode_api_mode_name(mode: u32) -> &'static str {
    match mode {
        0 => "APIMODE_OGL",
        1 => "APIMODE_D3D",
        _ => "unknown",
    }
}

fn decode_vertex_subpixel_precision_name(bit: u32) -> &'static str {
    match bit {
        0 => "_8Bit",
        1 => "_4Bit",
        _ => "unknown",
    }
}

fn decode_deref_block_size_name(mode: u32) -> &'static str {
    match mode {
        0 => "Block32",
        1 => "PerPoly",
        2 => "Block8",
        _ => "unknown",
    }
}

fn decode_cull_mode_name(mode: u32) -> &'static str {
    match mode {
        0 => "both",
        1 => "none",
        2 => "front",
        3 => "back",
        _ => "unknown",
    }
}

fn decode_fill_mode_name(mode: u32) -> &'static str {
    match mode {
        0 => "solid",
        1 => "wireframe",
        2 => "point",
        _ => "unknown",
    }
}

fn decode_front_winding_name(bit: u32) -> &'static str {
    match bit {
        0 => "cw",
        1 => "ccw",
        _ => "unknown",
    }
}

fn decode_wm_force_thread_dispatch_name(mode: u32) -> &'static str {
    match mode {
        0 => "normal",
        1 => "force-off",
        2 => "force-on",
        _ => "reserved",
    }
}

fn decode_wm_early_depth_stencil_control_name(mode: u32) -> &'static str {
    match mode {
        0 => "EDSC_NORMAL",
        1 => "EDSC_PSEXEC",
        2 => "EDSC_PREPS",
        _ => "reserved",
    }
}

const GEOM_INSTDONE_BITS: &[(u32, &str)] = &[
    (1 << 1, "VFL"),
    (1 << 2, "VS"),
    (1 << 3, "HS"),
    (1 << 4, "TE"),
    (1 << 5, "DS"),
    (1 << 6, "GS"),
    (1 << 7, "SOL"),
    (1 << 8, "CL"),
    (1 << 9, "SF"),
    (1 << 11, "TDG1"),
    (1 << 13, "URBM"),
    (1 << 14, "SVG"),
    (1 << 17, "TSG0"),
    (1 << 22, "SDE"),
];

const SC_INSTDONE_BITS: &[(u32, &str)] = &[
    (1 << 0, "SVL"),
    (1 << 1, "WMFE"),
    (1 << 2, "WMBE"),
    (1 << 3, "HIZ"),
    (1 << 5, "IZFE"),
    (1 << 6, "SBE"),
    (1 << 9, "RCC"),
    (1 << 10, "RCPBE"),
    (1 << 11, "RCPFE"),
    (1 << 12, "DAPB"),
    (1 << 13, "DAPRBE"),
    (1 << 15, "SARB"),
    (1 << 16, "DC0"),
    (1 << 17, "DC1"),
    (1 << 18, "DC2"),
    (1 << 19, "DC3"),
    (1 << 20, "GW0"),
    (1 << 21, "GW1"),
    (1 << 22, "GW2"),
    (1 << 23, "GW3"),
    (1 << 24, "TDC"),
    (1 << 25, "SFBE"),
    (1 << 26, "PSS"),
    (1 << 27, "AMFS"),
];

const SC_INSTDONE_EXTRA_BITS: &[(u32, &str)] = &[
    (1 << 9, "RCC1"),
    (1 << 10, "RCPBE1"),
    (1 << 11, "RCPFE1"),
    (1 << 12, "DAPB1"),
    (1 << 13, "DAPRBE1"),
    (1 << 16, "DC4"),
    (1 << 17, "DC5"),
    (1 << 18, "DC6"),
    (1 << 19, "DC7"),
    (1 << 20, "GW4"),
    (1 << 21, "GW5"),
    (1 << 22, "GW6"),
    (1 << 23, "GW7"),
    (1 << 24, "TDC1"),
    (1 << 26, "PSS1"),
];

const SC_INSTDONE_EXTRA2_BITS: &[(u32, &str)] = &[
    (1 << 9, "RCC2"),
    (1 << 10, "RCPBE2"),
    (1 << 11, "RCPFE2"),
    (1 << 12, "DAPB2"),
    (1 << 13, "DAPRBE2"),
];

fn log_not_done_units(submit_name: &str, label: &str, value: u32, bits: &[(u32, &'static str)]) {
    intel_render_focus_log!("intel/render: {} {}=", submit_name, label);
    let mut any = false;
    for &(mask, name) in bits {
        if (value & mask) == 0 {
            if any {
                intel_render_focus_log!("|");
            }
            intel_render_focus_log!("{}", name);
            any = true;
        }
    }
    if !any {
        intel_render_focus_log!("none");
    }
    intel_render_focus_log!("\n");
}

fn log_backend_dispatch_contract(
    wm_dw1: u32,
    ps_blend_dw1: u32,
    wm_depth_stencil_dw1: u32,
    _wm_depth_stencil_dw2: u32,
    _wm_depth_stencil_dw3: u32,
    wm_hz_op_dw1: u32,
    wm_hz_op_dw2: u32,
    wm_hz_op_dw3: u32,
    wm_hz_op_dw4: u32,
    ps_extra_dw1: u32,
) {
    let wm_statistics_enable = (wm_dw1 >> 31) & 0x1;
    let wm_force_thread_dispatch = (wm_dw1 >> 19) & 0x3;
    let wm_edsc = (wm_dw1 >> 21) & 0x3;
    let ps_blend_alpha_test = (ps_blend_dw1 >> 8) & 0x1;
    let ps_blend_color_enable = (ps_blend_dw1 >> 29) & 0x1;
    let ps_blend_has_writeable_rt = (ps_blend_dw1 >> 30) & 0x1;
    let ps_blend_alpha_to_coverage = (ps_blend_dw1 >> 31) & 0x1;
    let wm_depth_test_enable = (wm_depth_stencil_dw1 >> 1) & 0x1;
    let wm_stencil_write_enable = (wm_depth_stencil_dw1 >> 2) & 0x1;
    let wm_stencil_test_enable = (wm_depth_stencil_dw1 >> 3) & 0x1;
    let wm_double_sided_stencil = (wm_depth_stencil_dw1 >> 4) & 0x1;
    let wm_depth_write_enable = (wm_depth_stencil_dw1 >> 28) & 0x1;
    let wm_hz_partial_resolve = (wm_hz_op_dw1 >> 9) & 0x1;
    let wm_hz_samples = (wm_hz_op_dw1 >> 13) & 0x7;
    let wm_hz_stencil_resolve = (wm_hz_op_dw1 >> 24) & 0x1;
    let wm_hz_full_surface_clear = (wm_hz_op_dw1 >> 25) & 0x1;
    let wm_hz_hier_resolve = (wm_hz_op_dw1 >> 27) & 0x1;
    let wm_hz_depth_resolve = (wm_hz_op_dw1 >> 28) & 0x1;
    let wm_hz_scissor = (wm_hz_op_dw1 >> 29) & 0x1;
    let wm_hz_depth_clear = (wm_hz_op_dw1 >> 30) & 0x1;
    let wm_hz_stencil_clear = (wm_hz_op_dw1 >> 31) & 0x1;
    let wm_hz_sample_mask = wm_hz_op_dw4 & 0xFFFF;
    let wm_hz_op_active = ((wm_hz_op_dw1 | wm_hz_op_dw2 | wm_hz_op_dw3 | wm_hz_op_dw4) != 0) as u32;
    let ps_valid = (ps_extra_dw1 >> 31) & 0x1;
    let ps_has_uav = (ps_extra_dw1 >> 2) & 0x1;
    let ps_computes_stencil = (ps_extra_dw1 >> 5) & 0x1;
    let ps_per_sample = (ps_extra_dw1 >> 6) & 0x1;
    let ps_attribute_enable = (ps_extra_dw1 >> 8) & 0x1;
    let ps_computed_depth = (ps_extra_dw1 >> 26) & 0x3;
    let ps_kills = (ps_extra_dw1 >> 28) & 0x1;
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

    intel_render_focus_log!(
        "intel/render: probe-backend-decoded wm[stats={} force_thread_dispatch={}({}) edsc={}({})] ps_blend[writeable_rt={} blend_enable={} alpha_test={} alpha_to_coverage={}] wm_depth_stencil[depth_test={} depth_write={} stencil_test={} stencil_write={} double_sided_stencil={}]\n",
        wm_statistics_enable,
        wm_force_thread_dispatch,
        decode_wm_force_thread_dispatch_name(wm_force_thread_dispatch),
        wm_edsc,
        decode_wm_early_depth_stencil_control_name(wm_edsc),
        ps_blend_has_writeable_rt,
        ps_blend_color_enable,
        ps_blend_alpha_test,
        ps_blend_alpha_to_coverage,
        wm_depth_test_enable,
        wm_depth_write_enable,
        wm_stencil_test_enable,
        wm_stencil_write_enable,
        wm_double_sided_stencil,
    );
    intel_render_focus_log!(
        "intel/render: probe-backend-gate wm_hz_op[active={} depth_clear={} depth_resolve={} hier_resolve={} stencil_clear={} stencil_resolve={} full_surface_clear={} partial_resolve={} scissor={} samples={} sample_mask=0x{:X}] ps_extra[valid={} attribute_enable={} per_sample={} has_uav={} kills={} computed_depth={} computes_stencil={}] dispatch_armed={} reason={}\n",
        wm_hz_op_active,
        wm_hz_depth_clear,
        wm_hz_depth_resolve,
        wm_hz_hier_resolve,
        wm_hz_stencil_clear,
        wm_hz_stencil_resolve,
        wm_hz_full_surface_clear,
        wm_hz_partial_resolve,
        wm_hz_scissor,
        wm_hz_samples,
        wm_hz_sample_mask,
        ps_valid,
        ps_attribute_enable,
        ps_per_sample,
        ps_has_uav,
        ps_kills,
        ps_computed_depth,
        ps_computes_stencil,
        dispatch_armed,
        dispatch_reason,
    );
}

fn log_mesa_spec_cross_compare(
    warm: RenderWarmState,
    pipeline: &'static crate::intel::shader::TrianglePipeline,
    sbe_dw1: u32,
    baked_vs_urb_output_length: u8,
    programmed_vs_urb_output_length: u8,
    clip_dw1: u32,
    clip_dw2: u32,
    sf_dw1: u32,
    raster_dw1: u32,
    ps_dw3: u32,
    ps_dw6: u32,
    ps_extra_dw1: u32,
) {
    let trueos_sbe_read_offset = (sbe_dw1 >> 5) & 0x3F;
    let trueos_sbe_read_length = (sbe_dw1 >> 11) & 0x1F;
    let trueos_sbe_force_read_offset = (sbe_dw1 >> 28) & 0x1;
    let trueos_sbe_force_read_length = (sbe_dw1 >> 29) & 0x1;
    let trueos_sbe_num_sf_attrs = (sbe_dw1 >> 22) & 0x3F;
    let trueos_ps_vector_mask = (ps_dw3 >> 30) & 0x1;
    let trueos_ps_binding_table_entry_count = (ps_dw3 >> 18) & 0x1F;
    let trueos_ps_push_constants = (ps_dw6 >> 11) & 0x1;
    let trueos_ps_max_threads_per_psd = (ps_dw6 >> PS_MAX_THREADS_SHIFT) & 0x7F;
    let trueos_clip_perspective_divide_disable =
        ((clip_dw2 & CLIP_PERSPECTIVE_DIVIDE_DISABLE) != 0) as u32;
    let trueos_clip_mode = (clip_dw2 >> 13) & 0x7;
    let trueos_clip_enable = (clip_dw2 >> 31) & 0x1;
    let trueos_clip_stats = (clip_dw1 >> 10) & 0x1;
    let trueos_sf_stats = (sf_dw1 >> 10) & 0x1;
    let trueos_raster_cull_mode = (raster_dw1 >> 16) & 0x3;
    let trueos_ps_attribute_enable = (ps_extra_dw1 >> 8) & 0x1;
    let trueos_ps_per_sample = (ps_extra_dw1 >> 6) & 0x1;
    let trueos_ps_computed_depth = (ps_extra_dw1 >> 26) & 0x3;
    let trueos_ps_computes_stencil = (ps_extra_dw1 >> 5) & 0x1;
    let trueos_baked_vs_urb_out_len = baked_vs_urb_output_length as u32;
    let trueos_programmed_vs_urb_out_len = programmed_vs_urb_output_length as u32;
    let trueos_ps_dispatch = match pipeline.ps.meta.kernel.dispatch_mode {
        crate::intel::shader::DispatchMode::Simd8 => "simd8",
        crate::intel::shader::DispatchMode::Simd16 => "simd16",
        crate::intel::shader::DispatchMode::Simd32 => "simd32",
    };

    intel_render_focus_log!(
        "intel/render: mesa-compare target=device=0x{:04X} note={} host_sbe[read_offset=1 read_length=1 force_read_offset=1 force_read_length=1 num_sf_attrs=0] trueos_sbe[read_offset={} read_length={} force_read_offset={} force_read_length={} num_sf_attrs={}] host_clip[perspective_divide_disable=1] trueos_clip[perspective_divide_disable={} clip_mode={}({}) clip_enable={} statistics={}] debug_sf[statistics={}] host_raster[cull_mode=none sample_mask=0x1] trueos_raster[cull_mode={}({}) sample_mask=1]\n",
        warm.device_id,
        crate::intel::shader::triangle_pipeline_note(),
        trueos_sbe_read_offset,
        trueos_sbe_read_length,
        trueos_sbe_force_read_offset,
        trueos_sbe_force_read_length,
        trueos_sbe_num_sf_attrs,
        trueos_clip_perspective_divide_disable,
        trueos_clip_mode,
        decode_clip_mode_name(trueos_clip_mode),
        trueos_clip_enable,
        trueos_clip_stats,
        trueos_sf_stats,
        trueos_raster_cull_mode,
        decode_cull_mode_name(trueos_raster_cull_mode),
    );
    intel_render_focus_log!(
        "intel/render: mesa-compare host_ps[vector_mask=0 binding_table_entry_count=0 push_constants=0 dispatch=simd8 max_threads_per_psd=63] trueos_ps[vector_mask={} binding_table_entry_count={} push_constants={} dispatch={} max_threads_per_psd={}] host_ps_extra[attribute_enable=0 per_sample=0 computed_depth=0 computes_stencil=0] trueos_ps_extra[attribute_enable={} per_sample={} computed_depth={} computes_stencil={}] spec_pre_raster[baked_vs_urb_output_len={} programmed_vs_urb_output_len={} sbe_read_offset={} sbe_read_length={}]\n",
        trueos_ps_vector_mask,
        trueos_ps_binding_table_entry_count,
        trueos_ps_push_constants,
        trueos_ps_dispatch,
        trueos_ps_max_threads_per_psd,
        trueos_ps_attribute_enable,
        trueos_ps_per_sample,
        trueos_ps_computed_depth,
        trueos_ps_computes_stencil,
        trueos_baked_vs_urb_out_len,
        trueos_programmed_vs_urb_out_len,
        trueos_sbe_read_offset,
        trueos_sbe_read_length,
    );
}

fn read_stat_counter64(dev: crate::intel::Dev, reg: usize) -> u64 {
    let low = crate::intel::mmio_read(dev, reg) as u64;
    let high = crate::intel::mmio_read(dev, reg + 4) as u64;
    low | (high << 32)
}

fn capture_triangle_stage_stats(dev: crate::intel::Dev) -> TriangleStageStats {
    TriangleStageStats {
        ia_vertices: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::IaVerticesCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        ia_primitives: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::IaPrimitivesCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        vs_invocations: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::VsInvocationCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        hs_invocations: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::HsInvocationCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        ds_invocations: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::DsInvocationCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        gs_invocations: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::GsInvocationCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        gs_primitives: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::GsPrimitivesCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        cl_invocations: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::ClInvocationCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        cl_primitives: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::ClPrimitivesCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        ps_invocations: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::PsInvocationCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        cps_invocations: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::CpsInvocationCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        ps_depth: read_stat_counter64(
            dev,
            crate::intel::stats::RenderStat::PsDepthCount
                .mmio_offset()
                .unwrap_or(0),
        ),
        so_prims_written_0: read_stat_counter64(dev, SO_NUM_PRIMS_WRITTEN_0),
        so_write_offset_0: crate::intel::mmio_read(dev, SO_WRITE_OFFSET_0) as u64,
    }
}

fn log_triangle_stage_stats(
    submit_name: &str,
    label: &str,
    completed: bool,
    stats: TriangleStageStats,
    before: Option<TriangleStageStats>,
) {
    if crate::logflag::INTEL_STAGE1_LOGS {
        return;
    }
    if let Some(before) = before {
        let delta = stats.delta_since(before);
        intel_render_verbose_log!(
            "intel/render: {} stage-stats label={} completed={} ia_vtx={} ia_prim={} vs={} hs={} ds={} gs={} gs_prim={} cl={} cl_prim={} ps={} cps={} ps_depth={} so0={} so_write0={} delta_ia_vtx={} delta_ia_prim={} delta_vs={} delta_hs={} delta_ds={} delta_gs={} delta_gs_prim={} delta_cl={} delta_cl_prim={} delta_ps={} delta_cps={} delta_ps_depth={} delta_so0={} delta_so_write0={}\n",
            submit_name,
            label,
            completed as u8,
            stats.ia_vertices,
            stats.ia_primitives,
            stats.vs_invocations,
            stats.hs_invocations,
            stats.ds_invocations,
            stats.gs_invocations,
            stats.gs_primitives,
            stats.cl_invocations,
            stats.cl_primitives,
            stats.ps_invocations,
            stats.cps_invocations,
            stats.ps_depth,
            stats.so_prims_written_0,
            stats.so_write_offset_0,
            delta.ia_vertices,
            delta.ia_primitives,
            delta.vs_invocations,
            delta.hs_invocations,
            delta.ds_invocations,
            delta.gs_invocations,
            delta.gs_primitives,
            delta.cl_invocations,
            delta.cl_primitives,
            delta.ps_invocations,
            delta.cps_invocations,
            delta.ps_depth,
            delta.so_prims_written_0,
            delta.so_write_offset_0
        );
    } else {
        intel_render_verbose_log!(
            "intel/render: {} stage-stats label={} completed={} ia_vtx={} ia_prim={} vs={} hs={} ds={} gs={} gs_prim={} cl={} cl_prim={} ps={} cps={} ps_depth={} so0={} so_write0={}\n",
            submit_name,
            label,
            completed as u8,
            stats.ia_vertices,
            stats.ia_primitives,
            stats.vs_invocations,
            stats.hs_invocations,
            stats.ds_invocations,
            stats.gs_invocations,
            stats.gs_primitives,
            stats.cl_invocations,
            stats.cl_primitives,
            stats.ps_invocations,
            stats.cps_invocations,
            stats.ps_depth,
            stats.so_prims_written_0,
            stats.so_write_offset_0
        );
    }
}

fn log_triangle_stage_diagnosis(
    submit_name: &str,
    completed: bool,
    before: TriangleStageStats,
    after: TriangleStageStats,
) {
    let delta = after.delta_since(before);
    let verdict =
        if is_vf_streamout_submit_name(submit_name) && delta.vs_invocations > 0 && !completed {
            "vf-proof-unexpected-vs-counters"
        } else if is_vf_streamout_submit_name(submit_name)
            && (delta.ia_vertices > 0 || delta.ia_primitives > 0)
            && delta.vs_invocations == 0
            && delta.so_prims_written_0 == 0
            && delta.so_write_offset_0 == 0
            && !completed
        {
            "vf-progress-no-sol-write-or-offset"
        } else if delta.ia_vertices == 0 && delta.vs_invocations == 0 {
            "no-front-end-progress"
        } else if is_streamout_submit_name(submit_name)
            && delta.vs_invocations > 0
            && delta.so_prims_written_0 == 0
            && delta.so_write_offset_0 == 0
            && !completed
        {
            "vs-progress-no-sol-write-or-offset"
        } else if delta.cl_primitives > 0
            && delta.ps_invocations == 0
            && delta.cps_invocations == 0
            && delta.ps_depth == 0
            && !completed
        {
            "clip-stage-produced-primitives-no-retire-or-ps-observable"
        } else if delta.vs_invocations > 0
            && delta.cl_invocations == 0
            && delta.cl_primitives == 0
            && delta.ps_invocations == 0
            && !completed
        {
            "vs-progress-no-clipper-or-ps-counters"
        } else if delta.cl_invocations > 0 && delta.ps_invocations == 0 {
            "stops-between-clipper-and-ps"
        } else if delta.ps_invocations > 0
            && delta.ps_depth > 0
            && delta.so_prims_written_0 == 0
            && delta.so_write_offset_0 == 0
            && !completed
        {
            "ps-depth-ran-no-streamout-or-retire"
        } else if delta.ps_invocations > 0 && delta.so_prims_written_0 == 0 && !completed {
            "ps-ran-no-retire-or-export"
        } else if (delta.so_prims_written_0 > 0 || delta.so_write_offset_0 > 0) && !completed {
            "streamout-wrote-no-retire"
        } else if completed {
            "completed"
        } else {
            "late-backend-stall"
        };
    intel_render_focus_log!(
        "intel/render: {} stage-diagnosis completed={} verdict={} delta_vs={} delta_hs={} delta_ds={} delta_gs={} delta_gs_prim={} delta_cl={} delta_cl_prim={} delta_ps={} delta_cps={} delta_ps_depth={} delta_so0={} delta_so_write0={}\n",
        submit_name,
        completed as u8,
        verdict,
        delta.vs_invocations,
        delta.hs_invocations,
        delta.ds_invocations,
        delta.gs_invocations,
        delta.gs_primitives,
        delta.cl_invocations,
        delta.cl_primitives,
        delta.ps_invocations,
        delta.cps_invocations,
        delta.ps_depth,
        delta.so_prims_written_0,
        delta.so_write_offset_0
    );
}

fn log_triangle_named_proofs(
    dev: crate::intel::Dev,
    submit_name: &str,
    completed: bool,
    before: TriangleStageStats,
    after: TriangleStageStats,
    post_vf_marker: u32,
    post_vs_marker: u32,
    post_ps_state_marker: u32,
    post_clip_marker: u32,
    post_raster_marker: u32,
) {
    let delta = after.delta_since(before);
    let vf_marker_ok = post_vf_marker == RCS_EXEC_RESULT_DRAW_POST_VF;
    let vs_marker_ok = post_vs_marker == RCS_EXEC_RESULT_DRAW_POST_VS;
    let ps_state_marker_ok = post_ps_state_marker == RCS_EXEC_RESULT_DRAW_POST_PS_STATE;
    let clip_marker_ok = post_clip_marker == RCS_EXEC_RESULT_DRAW_POST_CLIP;
    let raster_marker_ok = post_raster_marker == RCS_EXEC_RESULT_DRAW_POST_RASTER;
    let vf_accept = delta.ia_vertices > 0 || delta.ia_primitives > 0;
    let vs_accept = delta.vs_invocations > 0;
    let clip_raster_accept = delta.cl_invocations > 0 || delta.cl_primitives > 0;
    let ps_accept = delta.ps_invocations > 0 || delta.cps_invocations > 0 || delta.ps_depth > 0;
    let clip_accept = delta.cl_invocations > 0 || delta.cl_primitives > 0;
    let raster_packet_accept = clip_marker_ok && raster_marker_ok;
    let ps_launch_input_ready = ps_state_marker_ok && raster_packet_accept && clip_accept;
    let sc_instdone = crate::intel::mmio_read(dev, SC_INSTDONE);
    let sc_extra = crate::intel::mmio_read(dev, SC_INSTDONE_EXTRA);
    let sc_extra2 = crate::intel::mmio_read(dev, SC_INSTDONE_EXTRA2);

    intel_render_focus_log!(
        "intel/render: {} vf-proof accepted={} ia_vtx_delta={} ia_prim_delta={} post_vf=0x{:08X} post_vf_marker={} does_not_prove=vs_or_pixels\n",
        submit_name,
        vf_accept as u8,
        delta.ia_vertices,
        delta.ia_primitives,
        post_vf_marker,
        vf_marker_ok as u8,
    );
    intel_render_focus_log!(
        "intel/render: {} vs-proof accepted={} vs_delta={} post_vs=0x{:08X} post_vs_marker={} does_not_prove=clip_raster_or_pixels\n",
        submit_name,
        vs_accept as u8,
        delta.vs_invocations,
        post_vs_marker,
        vs_marker_ok as u8,
    );
    intel_render_focus_log!(
        "intel/render: {} clip-raster-proof accepted={} cl_delta={} cl_prim_delta={} post_clip=0x{:08X} post_raster=0x{:08X} packet_markers={} does_not_prove=ps_or_rt_write\n",
        submit_name,
        clip_raster_accept as u8,
        delta.cl_invocations,
        delta.cl_primitives,
        post_clip_marker,
        post_raster_marker,
        (clip_marker_ok && raster_marker_ok) as u8,
    );
    intel_render_focus_log!(
        "intel/render: {} clip-counter-proof accepted={} cl_delta={} cl_prim_delta={} does_not_prove=raster_samples_or_ps\n",
        submit_name,
        clip_accept as u8,
        delta.cl_invocations,
        delta.cl_primitives,
    );
    intel_render_focus_log!(
        "intel/render: {} raster-packet-proof accepted={} post_clip=0x{:08X} post_raster=0x{:08X} clip_counter={} sc_instdone=0x{:08X} sc_extra=0x{:08X} sc_extra2=0x{:08X} does_not_prove=fragment_samples_or_ps\n",
        submit_name,
        raster_packet_accept as u8,
        post_clip_marker,
        post_raster_marker,
        clip_accept as u8,
        sc_instdone,
        sc_extra,
        sc_extra2,
    );
    if is_fragment_candidate_submit_name(submit_name) {
        let candidate_ready = clip_accept
            && raster_packet_accept
            && delta.ps_invocations == 0
            && delta.cps_invocations == 0
            && delta.ps_depth == 0;
        let fragment_observed =
            delta.ps_invocations > 0 || delta.cps_invocations > 0 || delta.ps_depth > 0;
        record_fragment_boundary_probe(candidate_ready, fragment_observed);
        intel_render_focus_log!(
            "intel/render: {} fragment-candidate-proof accepted={} candidate_ready={} oversized=1 clip_counter={} raster_packet={} ps_state_marker={} fragment_observed={} ps_delta={} cps_delta={} ps_depth_delta={} observable=no_dedicated_fragment_counter_yet does_not_prove=rt_write\n",
            submit_name,
            candidate_ready as u8,
            candidate_ready as u8,
            clip_accept as u8,
            raster_packet_accept as u8,
            ps_state_marker_ok as u8,
            fragment_observed as u8,
            delta.ps_invocations,
            delta.cps_invocations,
            delta.ps_depth,
        );
    }
    intel_render_focus_log!(
        "intel/render: {} ps-launch-frontier-proof accepted={} input_ready={} ps_state_marker={} raster_packet={} clip_counter={} ps_delta={} cps_delta={} ps_depth_delta={} sc_instdone=0x{:08X} does_not_prove=rt_write\n",
        submit_name,
        ps_accept as u8,
        ps_launch_input_ready as u8,
        ps_state_marker_ok as u8,
        raster_packet_accept as u8,
        clip_accept as u8,
        delta.ps_invocations,
        delta.cps_invocations,
        delta.ps_depth,
        sc_instdone,
    );
    if submit_name == "ps-launch-big-primitive" {
        intel_render_focus_log!(
            "intel/render: ps-launch-big-primitive-proof accepted={} input_ready={} oversized=1 ps_state_marker={} raster_packet={} clip_counter={} ps_delta={} cps_delta={} ps_depth_delta={} does_not_prove=rt_write\n",
            ps_accept as u8,
            ps_launch_input_ready as u8,
            ps_state_marker_ok as u8,
            raster_packet_accept as u8,
            clip_accept as u8,
            delta.ps_invocations,
            delta.cps_invocations,
            delta.ps_depth,
        );
    }
    if submit_name == "ps-bt1-big-primitive" {
        intel_render_focus_log!(
            "intel/render: ps-bt1-big-primitive-proof accepted={} input_ready={} oversized=1 ps_bt_count=1 ps_state_marker={} raster_packet={} clip_counter={} ps_delta={} cps_delta={} ps_depth_delta={} does_not_prove=rt_write\n",
            ps_accept as u8,
            ps_launch_input_ready as u8,
            ps_state_marker_ok as u8,
            raster_packet_accept as u8,
            clip_accept as u8,
            delta.ps_invocations,
            delta.cps_invocations,
            delta.ps_depth,
        );
    }
    if submit_name == "ps-bt0-scratch-rt" {
        intel_render_focus_log!(
            "intel/render: ps-bt0-scratch-rt-frontier-proof accepted={} input_ready={} oversized=1 ps_bt_count=0 scratch_rt=1 ps_state_marker={} raster_packet={} clip_counter={} ps_delta={} cps_delta={} ps_depth_delta={} does_not_prove=scratch_rt_write\n",
            ps_accept as u8,
            ps_launch_input_ready as u8,
            ps_state_marker_ok as u8,
            raster_packet_accept as u8,
            clip_accept as u8,
            delta.ps_invocations,
            delta.cps_invocations,
            delta.ps_depth,
        );
    }
    if submit_name == "ps-wm-normal-big-primitive" {
        intel_render_focus_log!(
            "intel/render: ps-wm-normal-big-primitive-proof accepted={} input_ready={} oversized=1 wm_force=normal dispatch_qualifier=writeable_rt ps_state_marker={} raster_packet={} clip_counter={} ps_delta={} cps_delta={} ps_depth_delta={} does_not_prove=rt_write\n",
            ps_accept as u8,
            ps_launch_input_ready as u8,
            ps_state_marker_ok as u8,
            raster_packet_accept as u8,
            clip_accept as u8,
            delta.ps_invocations,
            delta.cps_invocations,
            delta.ps_depth,
        );
    }
    if submit_name == "ps-dispatch-all-big-primitive" {
        intel_render_focus_log!(
            "intel/render: ps-dispatch-width-proof accepted={} input_ready={} oversized=1 dispatch=all ksp_slots=all-same ps_state_marker={} raster_packet={} clip_counter={} ps_delta={} cps_delta={} ps_depth_delta={} does_not_prove=rt_write\n",
            ps_accept as u8,
            ps_launch_input_ready as u8,
            ps_state_marker_ok as u8,
            raster_packet_accept as u8,
            clip_accept as u8,
            delta.ps_invocations,
            delta.cps_invocations,
            delta.ps_depth,
        );
    }
    let dispatch_slot = match submit_name {
        "ps-dispatch-slot0-big-primitive" => Some(0),
        "ps-dispatch-slot1-big-primitive" => Some(1),
        "ps-dispatch-slot2-big-primitive" => Some(2),
        _ => None,
    };
    if let Some(slot) = dispatch_slot {
        intel_render_focus_log!(
            "intel/render: ps-dispatch-slot-proof accepted={} input_ready={} oversized=1 dispatch_slot={} ksp_slot={} ps_state_marker={} raster_packet={} clip_counter={} ps_delta={} cps_delta={} ps_depth_delta={} does_not_prove=rt_write\n",
            ps_accept as u8,
            ps_launch_input_ready as u8,
            slot,
            slot,
            ps_state_marker_ok as u8,
            raster_packet_accept as u8,
            clip_accept as u8,
            delta.ps_invocations,
            delta.cps_invocations,
            delta.ps_depth,
        );
    }
    let payload_variant = match submit_name {
        "ps-payload-push-big-primitive" => Some("push-constant-enable"),
        "ps-payload-attr-big-primitive" => Some("attribute-enable"),
        "ps-payload-simple-big-primitive" => Some("simple-ps-hint"),
        "ps-payload-source-depth-w-big-primitive" => Some("source-depth-w"),
        "ps-payload-bary-big-primitive" => Some("bary-plane-coeffs"),
        _ => None,
    };
    if let Some(payload_variant) = payload_variant {
        intel_render_focus_log!(
            "intel/render: ps-payload-proof accepted={} input_ready={} oversized=1 payload_variant={} ps_state_marker={} raster_packet={} clip_counter={} ps_delta={} cps_delta={} ps_depth_delta={} does_not_prove=rt_write\n",
            ps_accept as u8,
            ps_launch_input_ready as u8,
            payload_variant,
            ps_state_marker_ok as u8,
            raster_packet_accept as u8,
            clip_accept as u8,
            delta.ps_invocations,
            delta.cps_invocations,
            delta.ps_depth,
        );
    }
    let grf_variant = match submit_name {
        "ps-grf-start-r1-big-primitive" => Some("grf-start-r1"),
        "ps-grf-start-r2-big-primitive" => Some("grf-start-r2"),
        "ps-grf-start-r4-big-primitive" => Some("grf-start-r4"),
        "ps-grf-maxthreads-31-big-primitive" => Some("maxthreads-31"),
        "ps-grf-maxthreads-15-big-primitive" => Some("maxthreads-15"),
        _ => None,
    };
    if let Some(grf_variant) = grf_variant {
        intel_render_focus_log!(
            "intel/render: ps-grf-proof accepted={} input_ready={} oversized=1 grf_variant={} ps_state_marker={} raster_packet={} clip_counter={} ps_delta={} cps_delta={} ps_depth_delta={} does_not_prove=rt_write\n",
            ps_accept as u8,
            ps_launch_input_ready as u8,
            grf_variant,
            ps_state_marker_ok as u8,
            raster_packet_accept as u8,
            clip_accept as u8,
            delta.ps_invocations,
            delta.cps_invocations,
            delta.ps_depth,
        );
    }
    intel_render_focus_log!(
        "intel/render: {} ps-dispatch-proof accepted={} ps_delta={} cps_delta={} ps_depth_delta={} ps_state_marker={} completed={} does_not_prove=rt_write_or_display\n",
        submit_name,
        ps_accept as u8,
        delta.ps_invocations,
        delta.cps_invocations,
        delta.ps_depth,
        ps_state_marker_ok as u8,
        completed as u8,
    );
}

fn maybe_soft_accept_streamout_submit(
    submit_name: &'static str,
    warm: RenderWarmState,
    before: TriangleStageStats,
    after: TriangleStageStats,
    require_vs: bool,
    min_streamout_bytes: usize,
) -> bool {
    let delta = after.delta_since(before);
    let post3d_light = read_result_dword(warm, RESULT_SLOT_POST3D_LIGHT_PIPE_CONTROL_LO_DWORD);
    let post3d_light_hi = read_result_dword(warm, RESULT_SLOT_POST3D_LIGHT_PIPE_CONTROL_HI_DWORD);
    let final_after_light = read_result_dword(warm, RESULT_SLOT_FINAL_AFTER_LIGHT_DWORD);
    let pre_light_pc = read_result_dword(warm, RESULT_SLOT_PRE_LIGHT_PC_DWORD);
    let post3d_eop = read_result_dword(warm, RESULT_SLOT_POST3D_PIPE_CONTROL_LO_DWORD);
    let post3d_hi = read_result_dword(warm, RESULT_SLOT_POST3D_PIPE_CONTROL_HI_DWORD);
    let post3d_light_ok = post3d_light == RCS_EXEC_RESULT_DRAW_POST3D && post3d_light_hi == 0;
    let post3d_heavy_ok = post3d_eop == RCS_EXEC_RESULT_DRAW_POST3D && post3d_hi == 0;
    let expected_reason = if require_vs && post3d_heavy_ok {
        "post3d-heavy-eop+vs+streamout-counters"
    } else if require_vs {
        "post3d-light-eop+vs+streamout-counters"
    } else if post3d_heavy_ok {
        "post3d-heavy-eop+vf-streamout-counters"
    } else {
        "post3d-light-eop+vf-streamout-counters"
    };
    let accept = (post3d_light_ok || post3d_heavy_ok)
        && (delta.so_prims_written_0 > 0
            || usize::try_from(delta.so_write_offset_0).ok().unwrap_or(0) >= min_streamout_bytes)
        && if require_vs {
            delta.vs_invocations > 0
        } else {
            delta.vs_invocations == 0 && (delta.ia_vertices > 0 || delta.ia_primitives > 0)
        };
    intel_render_focus_log!(
        "intel/render: {} soft-accept accepted={} reason={} pre_light_pc=0x{:08X} post3d_light=0x{:08X} post3d_light_hi=0x{:08X} final_after_light=0x{:08X} post3d_eop=0x{:08X} post3d_hi=0x{:08X} delta_ia_vtx={} delta_ia_prim={} delta_vs={} delta_so0={} delta_so_write0={} min_streamout_bytes={}\n",
        submit_name,
        accept as u8,
        expected_reason,
        pre_light_pc,
        post3d_light,
        post3d_light_hi,
        final_after_light,
        post3d_eop,
        post3d_hi,
        delta.ia_vertices,
        delta.ia_primitives,
        delta.vs_invocations,
        delta.so_prims_written_0,
        delta.so_write_offset_0,
        min_streamout_bytes,
    );
    accept
}

fn log_triangle_stage_frontier(
    submit_name: &str,
    completed: bool,
    before: TriangleStageStats,
    after: TriangleStageStats,
    result_pre_light_pc: u32,
    result_post3d_light: u32,
    result_post3d_light_hi: u32,
    result_final_after_light: u32,
    result_post3d_eop: u32,
    result_post3d_eop_hi: u32,
    result2: u32,
    result3: u32,
    result4: u32,
    result5: u32,
    result6: u32,
    result7: u32,
) {
    let delta = after.delta_since(before);
    let pre_raster_packets = ((result3 == RCS_EXEC_RESULT_DRAW_POST_VF)
        && (result4 == RCS_EXEC_RESULT_DRAW_POST_VS)) as u8;
    let ps_state_packet = (result5 == RCS_EXEC_RESULT_DRAW_POST_PS_STATE) as u8;
    let clip_raster_packets = ((result6 == RCS_EXEC_RESULT_DRAW_POST_CLIP)
        && (result7 == RCS_EXEC_RESULT_DRAW_POST_RASTER)) as u8;
    let post_draw_before_light = (result_pre_light_pc == RCS_EXEC_RESULT_DRAW_PRE_LIGHT_PC) as u8;
    let post_draw_light = ((result_post3d_light == RCS_EXEC_RESULT_DRAW_POST3D)
        && (result_post3d_light_hi == 0)) as u8;
    let post_draw_final_after_light =
        (result_final_after_light == RCS_EXEC_RESULT_DRAW_FINAL_AFTER_LIGHT) as u8;
    let post_draw_heavy =
        ((result_post3d_eop == RCS_EXEC_RESULT_DRAW_POST3D) && (result_post3d_eop_hi == 0)) as u8;
    let post_draw_retire = (result2 == RCS_EXEC_RESULT_DONE) as u8;
    let light_post_sync_expected = PostDrawSyncVariant::from_submit_name(submit_name)
        .map(|variant| variant.light_post_sync_enabled())
        .unwrap_or(true);
    let counter_frontier =
        if delta.ps_invocations > 0 || delta.cps_invocations > 0 || delta.ps_depth > 0 {
            "ps-thread"
        } else if delta.cl_invocations > 0 || delta.cl_primitives > 0 {
            "clipper-thread"
        } else if is_vf_streamout_submit_name(submit_name)
            && (delta.ia_vertices > 0 || delta.ia_primitives > 0)
            && delta.vs_invocations == 0
        {
            "vf-only-counters"
        } else if delta.gs_invocations > 0
            || delta.ds_invocations > 0
            || delta.hs_invocations > 0
            || delta.gs_primitives > 0
        {
            "pre-raster-shader-thread"
        } else if delta.vs_invocations > 0 || delta.ia_vertices > 0 || delta.ia_primitives > 0 {
            "vs-only-counters"
        } else {
            "no-draw-counters"
        };
    let note = if post_draw_before_light == 0 {
        "draw_did_not_reach_pre_light_pc_marker"
    } else if light_post_sync_expected
        && post_draw_light == 0
        && post_draw_final_after_light != 0
    {
        "tail_retired_without_light_postsync_write"
    } else if light_post_sync_expected && post_draw_light == 0 {
        "draw_not_retired_before_light_sync"
    } else if post_draw_final_after_light == 0 {
        if light_post_sync_expected {
            "draw_light_sync_wrote_but_tail_mi_store_did_not_retire"
        } else {
            "draw_light_pc_did_not_retire_tail_mi_store"
        }
    } else if post_draw_heavy == 0 {
        "draw_reached_light_sync_not_heavy_flush"
    } else if post_draw_retire == 0 {
        "post_draw_heavy_sync_wrote_but_did_not_retire"
    } else if clip_raster_packets != 0
        && delta.cl_invocations == 0
        && delta.cl_primitives == 0
        && delta.ps_invocations == 0
        && delta.cps_invocations == 0
        && delta.ps_depth == 0
    {
        "state_packets_retired_through_raster_counters_only_show_no_clipper_or_ps_threads"
    } else if ps_state_packet != 0
        && delta.ps_invocations == 0
        && delta.cps_invocations == 0
        && delta.ps_depth == 0
    {
        "ps_state_programmed_but_no_ps_threads"
    } else {
        "draw_retired"
    };
    intel_render_focus_log!(
        "intel/render: {} stage-frontier completed={} pre_raster_packets={} ps_state_packet={} clip_raster_packets={} post_draw_before_light={} post_draw_light={} post_draw_final_after_light={} post_draw_heavy={} post_draw_retire={} counter_frontier={} note={}\n",
        submit_name,
        completed as u8,
        pre_raster_packets,
        ps_state_packet,
        clip_raster_packets,
        post_draw_before_light,
        post_draw_light,
        post_draw_final_after_light,
        post_draw_heavy,
        post_draw_retire,
        counter_frontier,
        note,
    );
}

fn log_streamout_proof_result(
    submit_name: &str,
    warm: RenderWarmState,
    completed: bool,
    vertex_count: usize,
    experiment: StreamoutProofExperiment,
) {
    let flush_bytes = experiment
        .vertex_bytes()
        .saturating_mul(vertex_count)
        .min(warm.streamout_len);
    if flush_bytes != 0 {
        crate::intel::dma_flush(warm.streamout_virt, flush_bytes);
    }
    let base = warm.streamout_virt as *const u32;
    let count = core::cmp::min(vertex_count, 3);
    let stride_words = experiment.vertex_bytes() / 4;
    for idx in 0..count {
        let words =
            unsafe { core::slice::from_raw_parts(base.add(idx * stride_words), stride_words) };
        match experiment {
            StreamoutProofExperiment::PositionSlot0 | StreamoutProofExperiment::PositionSlot1 => {
                intel_render_verbose_log!(
                    "intel/render: {} v{} experiment={} completed={} raw=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pos=[{:.3},{:.3},{:.3},{:.3}]\n",
                    submit_name,
                    idx,
                    experiment.label(),
                    completed as u8,
                    words[0],
                    words[1],
                    words[2],
                    words[3],
                    f32::from_bits(words[0]),
                    f32::from_bits(words[1]),
                    f32::from_bits(words[2]),
                    f32::from_bits(words[3])
                );
            }
            StreamoutProofExperiment::HeaderAndPositionSlots01 => {
                intel_render_verbose_log!(
                    "intel/render: {} v{} experiment={} completed={} hdr=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pos=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] pos_f=[{:.3},{:.3},{:.3},{:.3}]\n",
                    submit_name,
                    idx,
                    experiment.label(),
                    completed as u8,
                    words[0],
                    words[1],
                    words[2],
                    words[3],
                    words[4],
                    words[5],
                    words[6],
                    words[7],
                    f32::from_bits(words[4]),
                    f32::from_bits(words[5]),
                    f32::from_bits(words[6]),
                    f32::from_bits(words[7])
                );
            }
        }
    }
}

fn primitive_topology_label(topology: u32) -> &'static str {
    match topology {
        0x01 => "pointlist",
        0x02 => "linelist",
        0x03 => "linestrip",
        0x04 => "trilist",
        0x05 => "tristrip",
        0x06 => "trifan",
        0x09 => "linelist_adj",
        _ => "unknown",
    }
}

fn decode_streamout_offset_mode_name(
    stream_offset_write_enable: u32,
    offset_addr_enable: u32,
) -> &'static str {
    match ((stream_offset_write_enable & 0x1) << 1) | (offset_addr_enable & 0x1) {
        0 => "legacy-mmio-only",
        1 => "store-mmio-to-memory",
        2 => "load-from-immediate-or-address",
        3 => "load-and-store",
        _ => "unknown",
    }
}

fn read_result_dword(warm: RenderWarmState, index: usize) -> u32 {
    unsafe { core::ptr::read_volatile((warm.result_virt as *const u32).add(index)) }
}

fn seed_result_debug_slots(warm: RenderWarmState) {
    unsafe {
        for i in 0..RESULT_DEBUG_DWORD_COUNT {
            core::ptr::write_volatile((warm.result_virt as *mut u32).add(i), RESULT_DEBUG_SENTINEL);
        }
    }
}

fn recover_render_engine_after_nonretired_submit(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    submit_name: &'static str,
) {
    let el_pre = crate::intel::mmio_read(dev, RCS_RING_EXECLIST_STATUS_LO);
    let mi_mode_pre = crate::intel::mmio_read(dev, RCS_RING_MI_MODE);
    let acthd_pre = crate::intel::mmio_read(dev, RCS_RING_ACTHD);
    intel_render_focus_log!(
        "intel/render: {} recovery begin execlist_lo=0x{:08X} mi_mode=0x{:08X} acthd=0x{:08X}\n",
        submit_name,
        el_pre,
        mi_mode_pre,
        acthd_pre,
    );

    for _ in 0..200_000u32 {
        let el = crate::intel::mmio_read(dev, RCS_RING_EXECLIST_STATUS_LO);
        if (el >> 30) == 0 {
            break;
        }
        core::hint::spin_loop();
    }

    crate::intel::mmio_write(
        dev,
        RCS_RING_MI_MODE,
        RING_MI_MODE_STOP_RING | (RING_MI_MODE_STOP_RING << 16),
    );
    for _ in 0..50_000u32 {
        if crate::intel::mmio_read(dev, RCS_RING_MI_MODE) & MODE_IDLE != 0 {
            break;
        }
        core::hint::spin_loop();
    }

    crate::intel::mmio_write(dev, GDRST, GRDOM_RENDER);
    for _ in 0..500_000u32 {
        if crate::intel::mmio_read(dev, GDRST) & GRDOM_RENDER == 0 {
            break;
        }
        core::hint::spin_loop();
    }

    crate::intel::mmio_write(dev, RCS_RING_MI_MODE, RING_MI_MODE_STOP_RING << 16);
    crate::intel::ggtt_invalidate(dev);

    let mode_bits = GFX_RUN_LIST_ENABLE | GEN11_GFX_DISABLE_LEGACY_MODE;
    crate::intel::mmio_write(dev, RCS_RING_MODE_GEN7, masked_bit_enable(mode_bits));
    let forcewake_ok = forcewake_render_acquire(warm);

    intel_render_focus_log!(
        "intel/render: {} recovery end gdrst=0x{:08X} execlist_lo=0x{:08X} mi_mode=0x{:08X} mode=0x{:08X} forcewake_ok={}\n",
        submit_name,
        crate::intel::mmio_read(dev, GDRST),
        crate::intel::mmio_read(dev, RCS_RING_EXECLIST_STATUS_LO),
        crate::intel::mmio_read(dev, RCS_RING_MI_MODE),
        crate::intel::mmio_read(dev, RCS_RING_MODE_GEN7),
        forcewake_ok as u8,
    );
}

fn build_ring_batch_start(warm: RenderWarmState, batch_gpu_addr: u64) -> usize {
    let dwords =
        unsafe { core::slice::from_raw_parts_mut(warm.ring_virt as *mut u32, BLT_RING_DWORDS) };
    dwords[0] = MI_BATCH_BUFFER_START_GEN8 | MI_BATCH_GTT;
    dwords[1] = batch_gpu_addr as u32;
    dwords[2] = (batch_gpu_addr >> 32) as u32;
    dwords[3] = MI_NOOP;
    crate::intel::dma_flush(warm.ring_virt, BLT_RING_TAIL_BYTES);
    BLT_RING_TAIL_BYTES
}

fn ring_ctl_value(size: usize) -> Option<u32> {
    let size = u32::try_from(size).ok()?;
    Some(size.checked_sub(4096)? | RING_VALID)
}

fn masked_bit_enable(bit: u32) -> u32 {
    bit | (bit << 16)
}

fn masked_bit_disable(bit: u32) -> u32 {
    bit << 16
}

fn masked_bits_update(set_bits: u32, clear_bits: u32) -> u32 {
    let update = set_bits | clear_bits;
    set_bits | (update << 16)
}

fn build_execlist_context_descriptor(context_gpu_addr: u64) -> (u32, u32) {
    let base = (context_gpu_addr as u32) & 0xFFFF_F000;
    let desc = base
        | GEN8_CTX_VALID
        | CTX_DESC_FORCE_RESTORE
        | GEN8_CTX_PRIVILEGE
        | GEN12_CTX_PRIORITY_NORMAL
        | (INTEL_LEGACY_64B_CONTEXT << GEN8_CTX_ADDRESSING_MODE_SHIFT);
    (desc, (context_gpu_addr >> 32) as u32)
}

fn execlist_submit_port_push(
    dev: crate::intel::Dev,
    context0_lo: u32,
    context0_hi: u32,
    context1_lo: u32,
    context1_hi: u32,
) {
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SQ_LO, context0_lo);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SQ_HI, context0_hi);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SUBMIT_PORT, context0_lo);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SUBMIT_PORT, context0_hi);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SUBMIT_PORT, context1_lo);
    crate::intel::mmio_write(dev, RCS_RING_EXECLIST_SUBMIT_PORT, context1_hi);
}

fn write_lrc_ring_tail(warm: RenderWarmState, ring_tail: u32) {
    let total_dwords = warm.context_len / core::mem::size_of::<u32>();
    if total_dwords <= LRC_STATE_OFFSET_DWORDS + 3 {
        return;
    }

    let dwords =
        unsafe { core::slice::from_raw_parts_mut(warm.context_virt as *mut u32, total_dwords) };
    dwords[LRC_STATE_OFFSET_DWORDS + 3] = ring_tail;
    crate::intel::dma_flush(warm.context_virt, warm.context_len);
}

fn mi_lri_num_regs(num_regs: u32) -> u32 {
    num_regs.saturating_mul(2).saturating_sub(1)
}

fn mi_lri_cmd(num_regs: u32, flags: u32) -> u32 {
    MI_LOAD_REGISTER_IMM | MI_LRI_CS_MMIO | flags | mi_lri_num_regs(num_regs)
}

fn push_mi_nops(state: &mut [u32], idx: &mut usize, count: usize) {
    for _ in 0..count {
        state[*idx] = MI_NOOP;
        *idx += 1;
    }
}
