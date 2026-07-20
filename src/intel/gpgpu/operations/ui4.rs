/// Queue one UI4 blend without waiting for its post marker.  Every mutable GPU
/// object used here is compositor-private: LRC/ring, batch, result page,
/// descriptor page, PPGTT root, vGPU device, and timeline.
pub(crate) fn queue_ui4_compositor_layers(
    base: Option<GpgpuRgba8Surface>,
    dst: GpgpuRgba8Surface,
    layers: &[GpgpuUi4ComposeLayer],
    damage: GpgpuRect,
    flags: u32,
) -> Result<Ui4CompositorSubmission, Ui4CompositorSubmitError> {
    if !dst.is_valid()
        || damage.x < 0
        || damage.y < 0
        || damage.width == 0
        || damage.height == 0
        || damage.x as u32 >= dst.width
        || damage.y as u32 >= dst.height
        || layers.len() > UI4_COMPOSE_LAYERS_MAX_LAYERS
    {
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }
    let base = base.unwrap_or(dst);
    if !base.is_valid()
        || ((flags & UI4_COMPOSE_FLAG_BASE_XRGB) != 0
            && (base.width != dst.width || base.height != dst.height))
        || layers
            .iter()
            .any(|layer| !layer.src.is_valid() || layer.dst_width == 0 || layer.dst_height == 0)
    {
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }

    let damage_x = damage.x as u32;
    let damage_y = damage.y as u32;
    let damage_width = damage.width.min(dst.width - damage_x);
    let damage_height = damage.height.min(dst.height - damage_y);
    let mut runtime = UI4_COMPOSITOR_RUNTIME.lock();
    if runtime.pending.is_some() {
        return Err(Ui4CompositorSubmitError::Busy);
    }
    let dev = super::claimed_device().ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let upload =
        upload_ui4_compose_layers_rgba8_kernel().ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let state = ui4_compositor_rcs_state_once(dev).ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let desc = ui4_compositor_sprite_quad_desc_buffer_once()
        .ok_or(Ui4CompositorSubmitError::Unavailable)?;

    unsafe {
        core::ptr::write_bytes(desc.virt, 0, desc.bytes);
        let out = desc.virt as *mut GpgpuUi4ComposeLayerDesc;
        for (index, layer) in layers.iter().enumerate() {
            core::ptr::write_volatile(
                out.add(index),
                GpgpuUi4ComposeLayerDesc {
                    src_gpu_lo: layer.src.gpu as u32,
                    src_gpu_hi: (layer.src.gpu >> 32) as u32,
                    src_pitch_bytes: layer.src.pitch_bytes,
                    src_width: layer.src.width,
                    src_height: layer.src.height,
                    dst_x: layer.dst_x,
                    dst_y: layer.dst_y,
                    dst_width: layer.dst_width,
                    dst_height: layer.dst_height,
                    opacity: layer.opacity as u32,
                    flags: 0,
                    reserved: 0,
                },
            );
        }
    }
    super::dma_flush(desc.virt, desc.bytes);

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && (runtime.state_mapped || direct_rcs_map_state(dev, state));
    if mapped_ok {
        runtime.state_mapped = true;
    }
    let ppgtt_ok = mapped_ok && (runtime.ppgtt_initialized || direct_rcs_init_ppgtt(state));
    if ppgtt_ok {
        runtime.ppgtt_initialized = true;
    }
    let kernel_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let base_ok = kernel_ok && direct_rcs_map_ppgtt_kernel(state, base.gpu, base.phys, base.bytes);
    // This allocation transfers directly to a display plane after GuC
    // retirement. Sources and descriptors remain PAT0/WB, while the exact
    // destination follows the proven PAT3/UC scanout contract used by the
    // native-video, Draw3D, Gridpaper, and preview paths.
    let dst_ok = base_ok && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes);
    let desc_ok = dst_ok && direct_rcs_map_ppgtt_kernel(state, desc.gpu, desc.phys, desc.bytes);
    let mut sources_ok = desc_ok;
    for layer in layers {
        if sources_ok
            && !direct_rcs_map_ppgtt_kernel(state, layer.src.gpu, layer.src.phys, layer.src.bytes)
        {
            sources_ok = false;
        }
    }
    let params = Ui4ComposeLayersParams {
        base_gpu: base.gpu,
        dst_gpu: dst.gpu,
        layers_gpu: desc.gpu,
        base_pitch_bytes: base.pitch_bytes,
        dst_pitch_bytes: dst.pitch_bytes,
        dst_width: dst.width,
        dst_height: dst.height,
        damage_x,
        damage_y,
        damage_width,
        damage_height,
        layer_count: layers.len() as u32,
        flags,
    };
    let batch_ok = sources_ok
        && direct_rcs_encode_ui4_compose_layers_batch(
            state, upload, params, base.bytes, dst.bytes, desc.bytes,
        );
    if !batch_ok {
        crate::log_error!(target: "ui4";
            "ui4/guc-compositor: layer queue rejected forcewake={} mapped={} ppgtt={} kernel={} base={} dst={} desc={} sources={} layers={} damage={}x{}@{},{}\n",
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ok as u8,
            base_ok as u8,
            dst_ok as u8,
            desc_ok as u8,
            sources_ok as u8,
            layers.len(),
            damage_width,
            damage_height,
            damage_x,
            damage_y,
        );
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }

    let started_tick = direct_rcs_now_tick();
    if !direct_rcs_submit_batch_for(
        dev,
        state,
        &mut runtime.submit,
        crate::gpu::vgpu::KernelClient::Ui4Compositor,
    ) {
        return Err(Ui4CompositorSubmitError::SubmissionRejected);
    }
    runtime.next_serial = runtime.next_serial.wrapping_add(1).max(1);
    let serial = runtime.next_serial;
    let gpu = runtime
        .submit
        .pending
        .expect("accepted UI4 submission must have an executor token");
    let submission = Ui4CompositorSubmission { serial, gpu };
    runtime.last_completion = None;
    runtime.pending = Some(Ui4CompositorPending {
        submission,
        started_tick,
        marker_slot: SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        marker_value: UI4_COMPOSE_LAYERS_POST_MARKER,
        kernel: "ui4-compose-layers",
        stats: GpgpuWorklistSubmitStats {
            descs: layers.len(),
            walkers: 1,
            submits: 1,
            submit_ms: 0,
        },
        overdue_logged: false,
    });
    crate::log_trace!(target: "ui4";
        "ui4/guc-compositor: queued serial={} kernel=ui4-compose-layers layers={} walkers=1 damage={}x{}@{},{} dst_gpu=0x{:X} ppgtt=base-pat0-wb,dst-pat3-uc,desc-pat0-wb,sources-pat0-wb context=isolated persistent=1 wait=none\n",
        serial,
        layers.len(),
        damage_width,
        damage_height,
        damage_x,
        damage_y,
        dst.gpu,
    );
    Ok(submission)
}

pub(crate) fn queue_ui4_compositor_sprite_quad_runs(
    dst: GpgpuRgba8Surface,
    runs: &[GpgpuSpriteQuadWorklistRun<'_>],
) -> Result<Ui4CompositorSubmission, Ui4CompositorSubmitError> {
    // Do not call `sprite_quad_worklist_ready()` here. That helper runs the
    // legacy synchronous smoke probe and polls its marker on the caller's CPU.
    // UI4 calls this entry point from an Embassy task and owns an asynchronous
    // GuC completion path below; making admission depend on the synchronous
    // probe can time out a successfully admitted GuC request, poison the
    // one-shot readiness flag, and prevent the real compositor request from
    // ever being queued. Preparing and admitting this request is the capability
    // check; its marker is validated by `poll_ui4_compositor_submission()`.
    if !dst.is_valid() {
        return Err(Ui4CompositorSubmitError::Unavailable);
    }
    let total_descs = runs
        .iter()
        .try_fold(0usize, |total, run| total.checked_add(run.descs.len()))
        .ok_or(Ui4CompositorSubmitError::InvalidWorklist)?;
    if runs.is_empty()
        || total_descs == 0
        || total_descs > SPRITE_QUAD_WORKLIST_MAX_DESCS
        || runs.iter().any(|run| run.descs.is_empty())
    {
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }

    let mut runtime = UI4_COMPOSITOR_RUNTIME.lock();
    if runtime.pending.is_some() {
        return Err(Ui4CompositorSubmitError::Busy);
    }
    let dev = super::claimed_device().ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let upload =
        upload_sprite_quad_worklist_rgba8_kernel().ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let state = ui4_compositor_rcs_state_once(dev).ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let desc = ui4_compositor_sprite_quad_desc_buffer_once()
        .ok_or(Ui4CompositorSubmitError::Unavailable)?;

    unsafe {
        core::ptr::write_bytes(desc.virt, 0, desc.bytes);
        let out = desc.virt as *mut GpgpuSpriteQuadWorklistDesc;
        let mut index = 0usize;
        for run in runs {
            for descriptor in run.descs.iter().copied() {
                core::ptr::write_volatile(out.add(index), descriptor);
                index = index.saturating_add(1);
            }
        }
    }
    super::dma_flush(desc.virt, desc.bytes);

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && (runtime.state_mapped || direct_rcs_map_state(dev, state));
    if mapped_ok {
        runtime.state_mapped = true;
    }
    let ppgtt_ok = mapped_ok && (runtime.ppgtt_initialized || direct_rcs_init_ppgtt(state));
    if ppgtt_ok {
        runtime.ppgtt_initialized = true;
    }
    let kernel_ppgtt_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let dst_ppgtt_ok =
        kernel_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes);
    let desc_ppgtt_ok =
        dst_ppgtt_ok && direct_rcs_map_ppgtt_kernel(state, desc.gpu, desc.phys, desc.bytes);
    let mut src_ppgtt_ok = desc_ppgtt_ok;
    if src_ppgtt_ok {
        for run in runs {
            if !direct_rcs_map_ppgtt_kernel(state, run.src.gpu, run.src.phys, run.src.bytes) {
                src_ppgtt_ok = false;
                break;
            }
        }
    }
    let batch_ok = src_ppgtt_ok
        && direct_rcs_encode_sprite_quad_worklist_runs_batch(state, upload, dst, desc, runs);
    if !batch_ok {
        crate::log_error!(target: "ui4";
            "ui4/guc-compositor: queue rejected stage=prepare forcewake={} mapped={} ppgtt={} kernel={} dst={} desc={} src={} batch={} descs={}\n",
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ppgtt_ok as u8,
            dst_ppgtt_ok as u8,
            desc_ppgtt_ok as u8,
            src_ppgtt_ok as u8,
            batch_ok as u8,
            total_descs,
        );
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }
    let started_tick = direct_rcs_now_tick();
    if !direct_rcs_submit_batch_for(
        dev,
        state,
        &mut runtime.submit,
        crate::gpu::vgpu::KernelClient::Ui4Compositor,
    ) {
        return Err(Ui4CompositorSubmitError::SubmissionRejected);
    }
    runtime.next_serial = runtime.next_serial.wrapping_add(1).max(1);
    let serial = runtime.next_serial;
    let gpu = runtime
        .submit
        .pending
        .expect("accepted UI4 submission must have an executor token");
    let submission = Ui4CompositorSubmission { serial, gpu };
    runtime.last_completion = None;
    runtime.pending = Some(Ui4CompositorPending {
        submission,
        started_tick,
        marker_slot: SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        marker_value: SPRITE_QUAD_WORKLIST_POST_MARKER,
        kernel: "sprite-quad-runs",
        stats: GpgpuWorklistSubmitStats {
            descs: total_descs,
            walkers: total_descs,
            submits: 1,
            submit_ms: 0,
        },
        overdue_logged: false,
    });
    crate::log_trace!(target: "ui4";
        "ui4/guc-compositor: queued serial={} descs={} dst_gpu=0x{:X} context=isolated persistent=1 wait=none\n",
        serial,
        total_descs,
        dst.gpu,
    );
    Ok(submission)
}

/// Queue the proven native-video primary rebuild as one GuC-owned RCS job.
/// Every primary output pixel is written: native NV12 inside the viewport and
/// the immutable XRGB desktop base outside it.
pub(crate) fn queue_ui4_compositor_nv12_tile64_to_primary(
    source: GpgpuNv12Tile64Surface,
    base: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    content_dst_x: u32,
    content_dst_y: u32,
    content_width: u32,
    content_height: u32,
    source_x: u32,
    source_y: u32,
) -> Result<Ui4CompositorSubmission, Ui4CompositorSubmitError> {
    let destination_valid = content_width != 0
        && content_height != 0
        && content_dst_x
            .checked_add(content_width)
            .is_some_and(|right| right <= dst.width)
        && content_dst_y
            .checked_add(content_height)
            .is_some_and(|bottom| bottom <= dst.height);
    let source_valid = source_x
        .checked_add(content_width)
        .is_some_and(|right| right <= source.width)
        && source_y
            .checked_add(content_height)
            .is_some_and(|bottom| bottom <= source.height);
    let layouts_match = base.is_valid()
        && dst.is_valid()
        && source.is_valid()
        && base.width == dst.width
        && base.height == dst.height;
    let ranges_distinct = !gpu_ranges_overlap(source.gpu, source.bytes, base.gpu, base.bytes)
        && !gpu_ranges_overlap(source.gpu, source.bytes, dst.gpu, dst.bytes)
        && !gpu_ranges_overlap(base.gpu, base.bytes, dst.gpu, dst.bytes)
        && !gpu_ranges_overlap(source.phys, source.bytes, base.phys, base.bytes)
        && !gpu_ranges_overlap(source.phys, source.bytes, dst.phys, dst.bytes)
        && !gpu_ranges_overlap(base.phys, base.bytes, dst.phys, dst.bytes);
    if !destination_valid || !source_valid || !layouts_match || !ranges_distinct {
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }
    let params = Ui4Nv12Tile64ToRgba8FrameParams {
        nv12_gpu: source.gpu,
        base_gpu: base.gpu,
        dst_gpu: dst.gpu,
        src_pitch_bytes: source.pitch_bytes,
        src_uv_offset: source.uv_offset,
        base_pitch_bytes: base.pitch_bytes,
        dst_pitch_bytes: dst.pitch_bytes,
        output_width: dst.width,
        output_height: dst.height,
        content_dst_x,
        content_dst_y,
        content_width,
        content_height,
        source_x,
        source_y,
    };

    let mut runtime = UI4_COMPOSITOR_RUNTIME.lock();
    if runtime.pending.is_some() {
        return Err(Ui4CompositorSubmitError::Busy);
    }
    let dev = super::claimed_device().ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let upload = upload_ui4_nv12_ytile_to_primary_xrgb_kernel()
        .ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let state = ui4_compositor_rcs_state_once(dev).ok_or(Ui4CompositorSubmitError::Unavailable)?;

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && (runtime.state_mapped || direct_rcs_map_state(dev, state));
    if mapped_ok {
        runtime.state_mapped = true;
    }
    let ppgtt_ok = mapped_ok && (runtime.ppgtt_initialized || direct_rcs_init_ppgtt(state));
    if ppgtt_ok {
        runtime.ppgtt_initialized = true;
    }
    let kernel_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let source_ok =
        kernel_ok && direct_rcs_map_ppgtt_kernel(state, source.gpu, source.phys, source.bytes);
    let base_ok = source_ok && direct_rcs_map_ppgtt_kernel(state, base.gpu, base.phys, base.bytes);
    let dst_ok = base_ok && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok = dst_ok
        && direct_rcs_encode_ui4_nv12_tile64_to_primary_batch(
            state,
            upload,
            params,
            source.bytes,
            base.bytes,
            dst.bytes,
        );
    if !batch_ok {
        crate::log_error!(target: "ui4";
            "ui4/guc-video-compositor: queue rejected forcewake={} state={} ppgtt={} kernel={} source={} base={} dst={} batch={} source_gpu=0x{:X} base_gpu=0x{:X} dst_gpu=0x{:X}\n",
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ok as u8,
            source_ok as u8,
            base_ok as u8,
            dst_ok as u8,
            batch_ok as u8,
            source.gpu,
            base.gpu,
            dst.gpu,
        );
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }
    let started_tick = direct_rcs_now_tick();
    if !direct_rcs_submit_batch_for(
        dev,
        state,
        &mut runtime.submit,
        crate::gpu::vgpu::KernelClient::Ui4Compositor,
    ) {
        return Err(Ui4CompositorSubmitError::SubmissionRejected);
    }
    runtime.next_serial = runtime.next_serial.wrapping_add(1).max(1);
    let serial = runtime.next_serial;
    let gpu = runtime
        .submit
        .pending
        .expect("accepted UI4 submission must have an executor token");
    let submission = Ui4CompositorSubmission { serial, gpu };
    runtime.last_completion = None;
    runtime.pending = Some(Ui4CompositorPending {
        submission,
        started_tick,
        marker_slot: SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        marker_value: SPRITE_QUAD_WORKLIST_POST_MARKER,
        kernel: "nv12-tile64-primary",
        stats: GpgpuWorklistSubmitStats {
            descs: 1,
            walkers: 1,
            submits: 1,
            submit_ms: 0,
        },
        overdue_logged: false,
    });
    crate::log_trace!(target: "ui4";
        "ui4/guc-video-compositor: queued serial={} native=tile64-nv12 output={}x{} content={}x{}@{},{} source={},{} dst_gpu=0x{:X} ppgtt=source-pat0-wb,base-pat0-wb,dst-pat3-uc bindings=3 display_plane_writes=0\n",
        serial,
        dst.width,
        dst.height,
        content_width,
        content_height,
        content_dst_x,
        content_dst_y,
        source_x,
        source_y,
        dst.gpu,
    );
    Ok(submission)
}

/// Convert one decoder-retired Tile64 NV12 picture directly into the exact
/// leased UI4 RGBA allocation. The accepted submission owns `dst` until its
/// completion marker retires; display programming is deliberately absent.
pub(crate) fn queue_ui4_video_frame_nv12_tile64_to_rgba8(
    source: GpgpuNv12Tile64Surface,
    dst: GpgpuRgba8Surface,
    content_dst_x: u32,
    content_dst_y: u32,
    content_width: u32,
    content_height: u32,
    source_x: u32,
    source_y: u32,
) -> Result<Ui4CompositorSubmission, Ui4CompositorSubmitError> {
    let destination_valid = content_width != 0
        && content_height != 0
        && content_dst_x
            .checked_add(content_width)
            .is_some_and(|right| right <= dst.width)
        && content_dst_y
            .checked_add(content_height)
            .is_some_and(|bottom| bottom <= dst.height);
    let source_valid = source_x
        .checked_add(content_width)
        .is_some_and(|right| right <= source.width)
        && source_y
            .checked_add(content_height)
            .is_some_and(|bottom| bottom <= source.height);
    let layouts_valid = dst.is_valid() && source.is_valid();
    let ranges_distinct = !gpu_ranges_overlap(source.gpu, source.bytes, dst.gpu, dst.bytes)
        && !gpu_ranges_overlap(source.phys, source.bytes, dst.phys, dst.bytes);
    if !destination_valid || !source_valid || !layouts_valid || !ranges_distinct {
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }
    let params = Ui4Nv12Tile64ToRgba8FrameParams {
        nv12_gpu: source.gpu,
        // The rebuilt binary keeps this legacy stateless pointer slot solely
        // so all following scalar offsets remain stable. It is never read and
        // deliberately aliases neither source nor destination.
        base_gpu: 0,
        dst_gpu: dst.gpu,
        src_pitch_bytes: source.pitch_bytes,
        src_uv_offset: source.uv_offset,
        base_pitch_bytes: 0,
        dst_pitch_bytes: dst.pitch_bytes,
        output_width: dst.width,
        output_height: dst.height,
        content_dst_x,
        content_dst_y,
        content_width,
        content_height,
        source_x,
        source_y,
    };

    let mut runtime = UI4_COMPOSITOR_RUNTIME.lock();
    if runtime.pending.is_some() {
        return Err(Ui4CompositorSubmitError::Busy);
    }
    let dev = super::claimed_device().ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let upload = upload_ui4_nv12_tile64_to_rgba8_frame_kernel()
        .ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let state = ui4_compositor_rcs_state_once(dev).ok_or(Ui4CompositorSubmitError::Unavailable)?;

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && (runtime.state_mapped || direct_rcs_map_state(dev, state));
    if mapped_ok {
        runtime.state_mapped = true;
    }
    let ppgtt_ok = mapped_ok && (runtime.ppgtt_initialized || direct_rcs_init_ppgtt(state));
    if ppgtt_ok {
        runtime.ppgtt_initialized = true;
    }
    let kernel_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let source_ok =
        kernel_ok && direct_rcs_map_ppgtt_kernel(state, source.gpu, source.phys, source.bytes);
    let dst_ok = source_ok && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok = dst_ok
        && direct_rcs_encode_ui4_nv12_tile64_to_rgba8_frame_batch(
            state,
            upload,
            params,
            source.bytes,
            dst.bytes,
        );
    if !batch_ok {
        crate::log_error!(target: "ui4";
            "ui4/guc-video-frame: queue rejected forcewake={} state={} ppgtt={} kernel={} source={} dst={} batch={} source_gpu=0x{:X} dst_gpu=0x{:X}\n",
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ok as u8,
            source_ok as u8,
            dst_ok as u8,
            batch_ok as u8,
            source.gpu,
            dst.gpu,
        );
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }
    let started_tick = direct_rcs_now_tick();
    if !direct_rcs_submit_batch_for(
        dev,
        state,
        &mut runtime.submit,
        crate::gpu::vgpu::KernelClient::Ui4Compositor,
    ) {
        return Err(Ui4CompositorSubmitError::SubmissionRejected);
    }
    runtime.next_serial = runtime.next_serial.wrapping_add(1).max(1);
    let serial = runtime.next_serial;
    let gpu = runtime
        .submit
        .pending
        .expect("accepted UI4 submission must have an executor token");
    let submission = Ui4CompositorSubmission { serial, gpu };
    runtime.last_completion = None;
    runtime.pending = Some(Ui4CompositorPending {
        submission,
        started_tick,
        marker_slot: SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        marker_value: SPRITE_QUAD_WORKLIST_POST_MARKER,
        kernel: "nv12-tile64-rgba8-frame",
        stats: GpgpuWorklistSubmitStats {
            descs: 1,
            walkers: 1,
            submits: 1,
            submit_ms: 0,
        },
        overdue_logged: false,
    });
    crate::log_trace!(target: "ui4";
        "ui4/guc-video-frame: queued serial={} native=tile64-nv12 output={}x{} content={}x{}@{},{} source={},{} dst_gpu=0x{:X} ppgtt=source-pat0-wb,dst-pat3-uc bindings=2 legacy_base_pointer=zero-unbound display_plane_writes=0\n",
        serial,
        dst.width,
        dst.height,
        content_width,
        content_height,
        content_dst_x,
        content_dst_y,
        source_x,
        source_y,
        dst.gpu,
    );
    Ok(submission)
}

fn gpu_ranges_overlap(left: u64, left_bytes: usize, right: u64, right_bytes: usize) -> bool {
    let Some(left_end) = left.checked_add(left_bytes as u64) else {
        return true;
    };
    let Some(right_end) = right.checked_add(right_bytes as u64) else {
        return true;
    };
    left < right_end && right < left_end
}

/// Observe one compositor marker exactly once.  This function never spins.
pub(crate) fn poll_ui4_compositor_submission(
    submission: Ui4CompositorSubmission,
) -> Ui4CompositorCompletion {
    const FAILURE_TIMEOUT_MS: u64 = 1_000;

    let mut runtime = UI4_COMPOSITOR_RUNTIME.lock();
    let Some(mut pending) = runtime.pending else {
        if let Some((retired, completion)) = runtime.last_completion
            && retired == submission
        {
            return completion;
        }
        return Ui4CompositorCompletion::InvalidSubmission;
    };
    if pending.submission != submission {
        return Ui4CompositorCompletion::InvalidSubmission;
    }
    let Some(state) = *UI4_COMPOSITOR_RCS_STATE.lock() else {
        runtime.pending = None;
        runtime.submit.pending = None;
        let completion = Ui4CompositorCompletion::Failed;
        runtime.last_completion = Some((submission, completion));
        drop(runtime);
        let _ = crate::gpu::executor::complete_kernel_submission(submission.gpu, false);
        return completion;
    };
    let observed = direct_rcs_read_result_slot(state, pending.marker_slot);
    if observed == pending.marker_value {
        pending.stats.submit_ms = direct_rcs_elapsed_ms_since(pending.started_tick);
        runtime.pending = None;
        runtime.submit.pending = None;
        let completion = Ui4CompositorCompletion::Complete(pending.stats);
        runtime.last_completion = Some((submission, completion));
        drop(runtime);
        let _ = crate::gpu::executor::complete_kernel_submission(submission.gpu, true);
        crate::log_trace!(target: "ui4";
            "ui4/guc-compositor: complete serial={} kernel={} descs={} walkers={} elapsed_ms={} poll=single\n",
            pending.submission.serial,
            pending.kernel,
            pending.stats.descs,
            pending.stats.walkers,
            pending.stats.submit_ms,
        );
        return completion;
    }
    if direct_rcs_elapsed_ms_since(pending.started_tick) >= FAILURE_TIMEOUT_MS {
        // A software timeout is not a GuC cancellation. Releasing this token
        // would let the next request overwrite the same batch/result storage
        // while the old context can still execute, and its shared marker could
        // then falsely retire the replacement request. Keep ownership pinned
        // until the marker arrives or a future real context-reset path proves
        // that execution stopped.
        if !pending.overdue_logged {
            pending.overdue_logged = true;
            runtime.pending = Some(pending);
            drop(runtime);
            crate::log_error!(target: "ui4";
                "ui4/guc-compositor: completion overdue serial={} observed=0x{:08X} want=0x{:08X} threshold_ms={} action=keep-pending-no-reuse cancellation=unavailable log=once\n",
                pending.submission.serial,
                observed,
                pending.marker_value,
                FAILURE_TIMEOUT_MS,
            );
        }
        return Ui4CompositorCompletion::Pending;
    }
    Ui4CompositorCompletion::Pending
}

/// Retire the video conversion and mint the exact-allocation producer release
/// only after the shared GuC completion packet has been observed.
pub(crate) fn poll_ui4_video_frame_submission(
    submission: Ui4CompositorSubmission,
    dst: GpgpuRgba8Surface,
) -> Ui4VideoFrameCompletion {
    match poll_ui4_compositor_submission(submission) {
        Ui4CompositorCompletion::Pending => Ui4VideoFrameCompletion::Pending,
        Ui4CompositorCompletion::Complete(stats) => Ui4VideoFrameCompletion::Complete {
            stats,
            release: gpgpu_rgba8_release(dst),
        },
        Ui4CompositorCompletion::Failed | Ui4CompositorCompletion::InvalidSubmission => {
            Ui4VideoFrameCompletion::Failed
        }
    }
}

/// Backend completion driver for awaitable UI4 GPU fences. Polling remains
/// dormant while there is no in-flight compositor job. The reaper itself owns
/// a fence waiter, proving the same wake path that future UI callers consume.
#[embassy_executor::task]
pub(crate) async fn gpu_completion_reaper_task() {
    use core::future::{Future, poll_fn};
    use core::pin::Pin;
    use core::task::Poll;
    use embassy_time::{Duration, Timer};

    let mut active: Option<(Ui4CompositorSubmission, crate::gpu::executor::GpuFence)> = None;
    loop {
        let pending = UI4_COMPOSITOR_RUNTIME
            .lock()
            .pending
            .map(|pending| pending.submission);
        let Some(submission) = pending else {
            active = None;
            Timer::after(Duration::from_millis(4)).await;
            continue;
        };
        if active
            .as_ref()
            .is_none_or(|(current, _)| *current != submission)
        {
            active = Some((submission, submission.fence()));
        }
        if let Some((_, fence)) = active.as_mut() {
            // Poll exactly once to register this task's waker without blocking
            // the backend marker probe that is responsible for completing it.
            let _ready = poll_fn(|cx| Poll::Ready(Pin::new(&mut *fence).poll(cx).is_ready())).await;
        }
        if !matches!(poll_ui4_compositor_submission(submission), Ui4CompositorCompletion::Pending) {
            active = None;
        }
        Timer::after(Duration::from_millis(1)).await;
    }
}
