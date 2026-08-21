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
    let base_is_dst = base.gpu == dst.gpu
        && base.phys == dst.phys
        && base.bytes == dst.bytes
        && base.pitch_bytes == dst.pitch_bytes;
    if !base.is_valid()
        || ((flags & UI4_COMPOSE_FLAG_BASE_XRGB) != 0
            && (base.width != dst.width || base.height != dst.height))
        || (!base_is_dst
            && (gpu_ranges_overlap(base.gpu, base.bytes, dst.gpu, dst.bytes)
                || gpu_ranges_overlap(base.phys, base.bytes, dst.phys, dst.bytes)))
        || layers.iter().any(|layer| {
            !layer.src.is_valid()
                || layer.dst_width == 0
                || layer.dst_height == 0
                || gpu_ranges_overlap(layer.src.gpu, layer.src.bytes, dst.gpu, dst.bytes)
                || gpu_ranges_overlap(layer.src.phys, layer.src.bytes, dst.phys, dst.bytes)
        })
    {
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }

    let damage_x = damage.x as u32;
    let damage_y = damage.y as u32;
    let damage_width = damage.width.min(dst.width - damage_x);
    let damage_height = damage.height.min(dst.height - damage_y);
    let mut runtime = UI4_COMPOSITOR_RUNTIME.lock();
    if !runtime.pending.is_empty() {
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
    // Overlay composition uses the destination itself as the preserved base.
    // Map that exact VA once as PAT3/UC: mapping it first as PAT0/WB and then
    // rewriting the same PTEs for the destination recreated the cache-policy
    // transition which corrupted earlier producer/display handoffs.
    let base_ok = kernel_ok
        && if base_is_dst {
            direct_rcs_map_ppgtt_scanout(state, base.gpu, base.phys, base.bytes)
        } else {
            direct_rcs_map_ppgtt_kernel(state, base.gpu, base.phys, base.bytes)
        };
    // This allocation transfers directly to a display plane after GuC
    // retirement. Sources and descriptors remain PAT0/WB, while the exact
    // destination follows the proven PAT3/UC scanout contract used by the
    // native-video, resident-scene, Gridpaper, and preview paths.
    let dst_ok = base_ok
        && (base_is_dst || direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes));
    let desc_ok = dst_ok && direct_rcs_map_ppgtt_kernel(state, desc.gpu, desc.phys, desc.bytes);
    let mut sources_ok = desc_ok;
    for layer in layers {
        if !sources_ok {
            break;
        }
        let mapped = if layer.src_scanout_cache {
            direct_rcs_map_ppgtt_scanout(state, layer.src.gpu, layer.src.phys, layer.src.bytes)
        } else {
            direct_rcs_map_ppgtt_kernel(state, layer.src.gpu, layer.src.phys, layer.src.bytes)
        };
        if !mapped {
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
    let Some(gpu) = direct_rcs_submit_batch_with_runtime(
        dev,
        state,
        &mut runtime.submit,
        crate::gpu::vgpu::KernelClient::Ui4Compositor,
        true,
    ) else {
        return Err(Ui4CompositorSubmitError::SubmissionRejected);
    };
    let admitted_tick = direct_rcs_now_tick();
    runtime.next_serial = runtime.next_serial.wrapping_add(1).max(1);
    let serial = runtime.next_serial;
    let submission = Ui4CompositorSubmission { serial, gpu };
    runtime.pending.push_back(Ui4CompositorPending {
        submission,
        job_slot: 0,
        queue_depth_at_admission: 1,
        started_tick,
        admitted_tick,
        marker_slot: SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        marker_value: UI4_COMPOSE_LAYERS_POST_MARKER,
        kernel: "ui4-compose-layers",
        stats: GpgpuWorklistSubmitStats {
            descs: layers.len(),
            walkers: 1,
            submits: 1,
            submit_ms: 0,
            probe: GpgpuSubmissionProbe::default(),
        },
        overdue_logged: false,
    });
    crate::log_trace!(target: "ui4";
        "ui4/guc-compositor: queued serial={} kernel=ui4-compose-layers layers={} walkers=1 damage={}x{}@{},{} dst_gpu=0x{:X} ppgtt_base={} ppgtt_dst=pat3-uc desc=pat0-wb sources=producer-stable-pat same_va_cache_remap=0 context=isolated persistent=1 wait=none\n",
        serial,
        layers.len(),
        damage_width,
        damage_height,
        damage_x,
        damage_y,
        dst.gpu,
        if base_is_dst { "dst-pat3-uc" } else { "pat0-wb" },
    );
    Ok(submission)
}

pub(crate) fn queue_ui4_compositor_sprite_quad_runs(
    dst: GpgpuRgba8Surface,
    runs: &[GpgpuSpriteQuadWorklistRun<'_>],
) -> Result<Ui4CompositorSubmission, Ui4CompositorSubmitError> {
    queue_ui4_sprite_quad_runs(dst, runs, false, "sprite-quad-runs")
}

/// Queue an arbitrary-quad Blueprint scene into its exact UI4 write lease.
///
/// This is deliberately a UI4 producer entry point rather than a mode on the
/// shared direct-RCS helper. It keeps the sprite ABI while giving the request
/// the compositor-private context, descriptor page, PPGTT, timeline, and
/// asynchronous retirement contract. The destination is mapped with UI4's
/// scanout cache policy because a successful final marker is published
/// directly as the producer release for this exact allocation.
pub(crate) fn queue_ui4_blueprint_sprite_scene(
    dst: GpgpuRgba8Surface,
    runs: &[GpgpuSpriteQuadWorklistRun<'_>],
) -> Result<Ui4CompositorSubmission, Ui4CompositorSubmitError> {
    queue_ui4_sprite_quad_runs(dst, runs, true, "blueprint-sprite-scene")
}

/// Queue one ordered set of 1:1 Blueprint sprite rectangles.  This uses the
/// proven descriptor alpha compositor: sixteen descriptors per walker, no 3D
/// draws or triangles, and the exact UI4 lease as its scanout-safe destination.
pub(crate) fn queue_ui4_blueprint_alpha_rects(
    src: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    descriptors: &[GpgpuAlphaBlendWorklistDesc],
) -> Result<Ui4CompositorSubmission, Ui4CompositorSubmitError> {
    const VALID_FLAGS: u32 = ALPHA_BLEND_WORKLIST_FLAG_COPY
        | ALPHA_BLEND_WORKLIST_FLAG_SRC_OVER
        | ALPHA_BLEND_WORKLIST_FLAG_TINT_RGB
        | ALPHA_BLEND_WORKLIST_FLAG_TINT_ALPHA
        | ALPHA_BLEND_WORKLIST_FLAG_PREMUL_SRC;

    if !src.is_valid()
        || !dst.is_valid()
        || descriptors.is_empty()
        || descriptors.len() > ALPHA_BLEND_WORKLIST_MAX_DESCS
        || gpu_ranges_overlap(src.gpu, src.bytes, dst.gpu, dst.bytes)
        || gpu_ranges_overlap(src.phys, src.bytes, dst.phys, dst.bytes)
        || descriptors.iter().any(|descriptor| {
            let src_x = descriptor.src_xy & 0xFFFF;
            let src_y = descriptor.src_xy >> 16;
            let dst_x = (descriptor.dst_xy as u16 as i16) as i32;
            let dst_y = ((descriptor.dst_xy >> 16) as u16 as i16) as i32;
            let width = descriptor.size & 0xFFFF;
            let height = descriptor.size >> 16;
            let copy = descriptor.flags & ALPHA_BLEND_WORKLIST_FLAG_COPY != 0;
            let source_over = descriptor.flags & ALPHA_BLEND_WORKLIST_FLAG_SRC_OVER != 0;
            width == 0
                || height == 0
                || dst_x < 0
                || dst_y < 0
                || descriptor.flags & !VALID_FLAGS != 0
                || copy == source_over
                || src_x
                    .checked_add(width)
                    .is_none_or(|right| right > src.width)
                || src_y
                    .checked_add(height)
                    .is_none_or(|bottom| bottom > src.height)
                || (dst_x as u32)
                    .checked_add(width)
                    .is_none_or(|right| right > dst.width)
                || (dst_y as u32)
                    .checked_add(height)
                    .is_none_or(|bottom| bottom > dst.height)
        })
    {
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }

    let mut runtime = UI4_COMPOSITOR_RUNTIME.lock();
    if !runtime.pending.is_empty() {
        return Err(Ui4CompositorSubmitError::Busy);
    }
    let dev = super::claimed_device().ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let upload =
        upload_alpha_blend_worklist_rgba8_kernel().ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let state = ui4_compositor_rcs_state_once(dev).ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let desc = ui4_compositor_sprite_quad_desc_buffer_once()
        .ok_or(Ui4CompositorSubmitError::Unavailable)?;
    if desc.bytes < ALPHA_BLEND_WORKLIST_DESC_BYTES {
        return Err(Ui4CompositorSubmitError::Unavailable);
    }

    unsafe {
        core::ptr::write_bytes(desc.virt, 0, desc.bytes);
        let out = desc.virt as *mut GpgpuAlphaBlendWorklistDesc;
        for (index, descriptor) in descriptors.iter().copied().enumerate() {
            core::ptr::write_volatile(out.add(index), descriptor);
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
    let src_ok = kernel_ok && direct_rcs_map_ppgtt_kernel(state, src.gpu, src.phys, src.bytes);
    let dst_ok = src_ok && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes);
    let desc_ok = dst_ok && direct_rcs_map_ppgtt_kernel(state, desc.gpu, desc.phys, desc.bytes);
    let params = AlphaBlendWorklistRgba8Params {
        src_gpu: src.gpu,
        dst_gpu: dst.gpu,
        desc_gpu: desc.gpu,
        src_pitch_bytes: src.pitch_bytes,
        dst_pitch_bytes: dst.pitch_bytes,
        desc_base: 0,
        desc_count: descriptors.len() as u32,
    };
    let batch_ok = desc_ok
        && direct_rcs_encode_alpha_blend_worklist_batch(
            state, upload, params, src.bytes, dst.bytes, desc.bytes,
        );
    if !batch_ok {
        crate::log_error!(target: "ui4";
            "ui4/guc-compositor: queue rejected stage=prepare request=blueprint-alpha-rects forcewake={} mapped={} ppgtt={} kernel={} src={} dst={} desc={} batch={} rects={}\n",
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ok as u8,
            src_ok as u8,
            dst_ok as u8,
            desc_ok as u8,
            batch_ok as u8,
            descriptors.len(),
        );
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }

    let started_tick = direct_rcs_now_tick();
    let Some(gpu) = direct_rcs_submit_batch_with_runtime(
        dev,
        state,
        &mut runtime.submit,
        crate::gpu::vgpu::KernelClient::Ui4Compositor,
        true,
    ) else {
        return Err(Ui4CompositorSubmitError::SubmissionRejected);
    };
    let admitted_tick = direct_rcs_now_tick();
    runtime.next_serial = runtime.next_serial.wrapping_add(1).max(1);
    let serial = runtime.next_serial;
    let submission = Ui4CompositorSubmission { serial, gpu };
    runtime.pending.push_back(Ui4CompositorPending {
        submission,
        job_slot: 0,
        queue_depth_at_admission: 1,
        started_tick,
        admitted_tick,
        marker_slot: RECT_WORKLIST_POST_MARKER_SLOT,
        marker_value: ALPHA_BLEND_WORKLIST_POST_MARKER,
        kernel: "blueprint-alpha-rects",
        stats: GpgpuWorklistSubmitStats {
            descs: descriptors.len(),
            walkers: rect_worklist_walker_count(descriptors.len()),
            submits: 1,
            submit_ms: 0,
            probe: GpgpuSubmissionProbe::default(),
        },
        overdue_logged: false,
    });
    crate::log_trace!(target: "ui4";
        "ui4/guc-compositor: queued serial={} kernel=blueprint-alpha-rects rects={} walkers={} src_gpu=0x{:X} dst_gpu=0x{:X} dst_cache=pat3-uc context=isolated persistent=1 wait=none\n",
        serial,
        descriptors.len(),
        rect_worklist_walker_count(descriptors.len()),
        src.gpu,
        dst.gpu,
    );
    Ok(submission)
}

fn queue_ui4_sprite_quad_runs(
    dst: GpgpuRgba8Surface,
    runs: &[GpgpuSpriteQuadWorklistRun<'_>],
    direct_scanout: bool,
    kernel: &'static str,
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
    if !runtime.pending.is_empty() {
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
    let dst_ppgtt_ok = kernel_ppgtt_ok
        && if direct_scanout {
            direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes)
        } else {
            direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes)
        };
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
            "ui4/guc-compositor: queue rejected stage=prepare request={} forcewake={} mapped={} ppgtt={} kernel={} dst={} dst_cache={} desc={} src={} batch={} descs={}\n",
            kernel,
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ppgtt_ok as u8,
            dst_ppgtt_ok as u8,
            if direct_scanout { "pat3-uc" } else { "pat0-wb" },
            desc_ppgtt_ok as u8,
            src_ppgtt_ok as u8,
            batch_ok as u8,
            total_descs,
        );
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }
    let started_tick = direct_rcs_now_tick();
    let Some(gpu) = direct_rcs_submit_batch_with_runtime(
        dev,
        state,
        &mut runtime.submit,
        crate::gpu::vgpu::KernelClient::Ui4Compositor,
        true,
    ) else {
        return Err(Ui4CompositorSubmitError::SubmissionRejected);
    };
    let admitted_tick = direct_rcs_now_tick();
    runtime.next_serial = runtime.next_serial.wrapping_add(1).max(1);
    let serial = runtime.next_serial;
    let submission = Ui4CompositorSubmission { serial, gpu };
    runtime.pending.push_back(Ui4CompositorPending {
        submission,
        job_slot: 0,
        queue_depth_at_admission: 1,
        started_tick,
        admitted_tick,
        marker_slot: SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        marker_value: SPRITE_QUAD_WORKLIST_POST_MARKER,
        kernel,
        stats: GpgpuWorklistSubmitStats {
            descs: total_descs,
            walkers: total_descs,
            submits: 1,
            submit_ms: 0,
            probe: GpgpuSubmissionProbe::default(),
        },
        overdue_logged: false,
    });
    crate::log_trace!(target: "ui4";
        "ui4/guc-compositor: queued serial={} kernel={} descs={} dst_gpu=0x{:X} dst_cache={} context=isolated persistent=1 wait=none\n",
        serial,
        kernel,
        total_descs,
        dst.gpu,
        if direct_scanout { "pat3-uc" } else { "pat0-wb" },
    );
    Ok(submission)
}

/// Convert one decoder-retired media-Y-tiled NV12 picture directly into the
/// exact leased UI4 RGBA allocation. The older `Tile64` symbol names are kept
/// for artifact ABI compatibility. The accepted submission owns `dst` until
/// its completion marker retires; display programming is deliberately absent.
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
    let queue_started_tick = direct_rcs_now_tick();
    let mut probe = GpgpuSubmissionProbe::default();
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
    if !destination_valid
        || !source_valid
        || !layouts_valid
        || source.bytes > UI4_COMPOSITOR_NV12_SOURCE_MAX_BYTES
        || gpu_ranges_overlap(source.phys, source.bytes, dst.phys, dst.bytes)
    {
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }

    let mut runtime = UI4_COMPOSITOR_RUNTIME.lock();
    if runtime.pending.len() >= UI4_COMPOSITOR_RCS_JOB_SLOTS {
        return Err(Ui4CompositorSubmitError::Busy);
    }
    let Some(job_slot) = (0..UI4_COMPOSITOR_RCS_JOB_SLOTS).find(|slot| {
        runtime
            .pending
            .iter()
            .all(|pending| pending.job_slot != *slot)
    }) else {
        return Err(Ui4CompositorSubmitError::Busy);
    };
    let source_gpu = UI4_COMPOSITOR_NV12_SOURCE_GPU_BASE
        .checked_add((job_slot * UI4_COMPOSITOR_NV12_SOURCE_MAX_BYTES) as u64)
        .ok_or(Ui4CompositorSubmitError::InvalidWorklist)?;
    if gpu_ranges_overlap(source_gpu, source.bytes, dst.gpu, dst.bytes) {
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }
    let params = Ui4Nv12Tile64ToRgba8FrameParams {
        nv12_gpu: source_gpu,
        // Keep the known-good three-binding video ABI, but do not revive the
        // full-primary desktop copy. Outside the native viewport each work
        // item reads and rewrites only its own pixel in this exact lease.
        base_gpu: dst.gpu,
        dst_gpu: dst.gpu,
        src_pitch_bytes: source.pitch_bytes,
        src_uv_offset: source.uv_offset,
        base_pitch_bytes: dst.pitch_bytes,
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
    let dev = super::claimed_device().ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let upload = upload_ui4_nv12_tile64_to_rgba8_frame_kernel()
        .ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let shared_state =
        ui4_compositor_rcs_state_once(dev).ok_or(Ui4CompositorSubmitError::Unavailable)?;
    let state =
        direct_rcs_job_slot(shared_state, job_slot).ok_or(Ui4CompositorSubmitError::Unavailable)?;

    let phase_started_tick = direct_rcs_now_tick();
    let forcewake_ok = direct_rcs_forcewake(dev);
    probe.forcewake_us = direct_rcs_elapsed_us_since(phase_started_tick);
    if forcewake_ok {
        probe.gpu_timestamp_frequency_hz = u64::from(direct_rcs_timestamp_frequency_hz(dev));
    }
    let phase_started_tick = direct_rcs_now_tick();
    let mapped_ok =
        forcewake_ok && (runtime.state_mapped || direct_rcs_map_state(dev, shared_state));
    probe.state_map_us = direct_rcs_elapsed_us_since(phase_started_tick);
    if mapped_ok {
        runtime.state_mapped = true;
    }
    let phase_started_tick = direct_rcs_now_tick();
    let ppgtt_ok = mapped_ok && (runtime.ppgtt_initialized || direct_rcs_init_ppgtt(shared_state));
    probe.ppgtt_init_us = direct_rcs_elapsed_us_since(phase_started_tick);
    if ppgtt_ok {
        runtime.ppgtt_initialized = true;
    }
    let phase_started_tick = direct_rcs_now_tick();
    let kernel_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    probe.kernel_map_us = direct_rcs_elapsed_us_since(phase_started_tick);
    let phase_started_tick = direct_rcs_now_tick();
    let source_ok =
        kernel_ok && direct_rcs_map_ppgtt_kernel(state, source_gpu, source.phys, source.bytes);
    probe.source_map_us = direct_rcs_elapsed_us_since(phase_started_tick);
    let phase_started_tick = direct_rcs_now_tick();
    let dst_ok = source_ok && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes);
    probe.destination_map_us = direct_rcs_elapsed_us_since(phase_started_tick);
    let phase_started_tick = direct_rcs_now_tick();
    let batch_ok = dst_ok
        && direct_rcs_encode_ui4_nv12_tile64_to_rgba8_frame_batch(
            state,
            upload,
            params,
            source.bytes,
            dst.bytes,
        );
    probe.batch_encode_us = direct_rcs_elapsed_us_since(phase_started_tick);
    if !batch_ok {
        crate::log_error!(target: "ui4";
            "ui4/guc-video-frame: queue rejected forcewake={} state={} ppgtt={} kernel={} source={} dst={} batch={} source_gpu=0x{:X} media_gpu=0x{:X} dst_gpu=0x{:X}\n",
            forcewake_ok as u8,
            mapped_ok as u8,
            ppgtt_ok as u8,
            kernel_ok as u8,
            source_ok as u8,
            dst_ok as u8,
            batch_ok as u8,
            source_gpu,
            source.gpu,
            dst.gpu,
        );
        return Err(Ui4CompositorSubmitError::InvalidWorklist);
    }
    probe.queue_prepare_us = direct_rcs_elapsed_us_since(queue_started_tick);
    let submit_attempt = UI4_VIDEO_FRAME_SUBMIT_ATTEMPTS
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    let log_submit_boundary = submit_attempt == 1 || submit_attempt.is_power_of_two();
    if log_submit_boundary {
        crate::log_info!(target: "ui4";
            "ui4/guc-video-frame: submit-boundary attempt={} action=enter-guc-submit ppgtt_root=0x{:X} source_gpu=0x{:X} media_gpu=0x{:X} source_phys=0x{:X} source_bytes=0x{:X} source_pat=0 source_alias=compositor-owned destination_gpu=0x{:X} destination_phys=0x{:X} destination_bytes=0x{:X} destination_pat=3 bindings=3 base_alias=exact-destination pte_preflight=complete batch_ready=1 display_plane_writes=0 cpu_pixel_copy=0\n",
            submit_attempt,
            shared_state.ppgtt_phys,
            source_gpu,
            source.gpu,
            source.phys,
            source.bytes,
            dst.gpu,
            dst.phys,
            dst.bytes,
        );
    }
    probe.gpu_host_pre_submit_timestamp = direct_rcs_read_render_timestamp(dev);
    let started_tick = direct_rcs_now_tick();
    let Some(gpu) = direct_rcs_submit_batch_with_runtime(
        dev,
        state,
        &mut runtime.submit,
        crate::gpu::vgpu::KernelClient::Ui4Compositor,
        true,
    ) else {
        return Err(Ui4CompositorSubmitError::SubmissionRejected);
    };
    let admitted_tick = direct_rcs_now_tick();
    probe.admission_us = direct_rcs_elapsed_us_since(started_tick);
    probe.queue_total_us = direct_rcs_elapsed_us_since(queue_started_tick);
    runtime.next_serial = runtime.next_serial.wrapping_add(1).max(1);
    let serial = runtime.next_serial;
    probe.guc_h2g_publish_sequence = gpu.physical_publish_sequence();
    if crate::intel::guc_ctb::h2g_sequence_consumed(probe.guc_h2g_publish_sequence) {
        probe.gpu_h2g_consumed_observe_timestamp = direct_rcs_read_render_timestamp(dev);
    }
    let submission = Ui4CompositorSubmission { serial, gpu };
    let queue_depth_at_admission = runtime.pending.len().saturating_add(1);
    runtime.pending.push_back(Ui4CompositorPending {
        submission,
        job_slot,
        queue_depth_at_admission,
        started_tick,
        admitted_tick,
        marker_slot: SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT,
        marker_value: SPRITE_QUAD_WORKLIST_POST_MARKER,
        kernel: "nv12-media-ytile-rgba8-frame",
        stats: GpgpuWorklistSubmitStats {
            descs: 1,
            walkers: 1,
            submits: 1,
            submit_ms: 0,
            probe,
        },
        overdue_logged: false,
    });
    if log_submit_boundary {
        crate::log_info!(target: "ui4";
            "ui4/guc-video-frame: submit-boundary attempt={} action=guc-submit-accepted serial={} job_slot={} queue_depth={} scheduler_priority=kmd-high priority_abi=0 guc_policy_enqueued=1 next=completion-marker\n",
            submit_attempt,
            serial,
            job_slot,
            queue_depth_at_admission,
        );
    }
    crate::log_trace!(target: "ui4";
        "ui4/guc-video-frame: queued serial={} job_slot={} queue_depth={} queue_capacity={} native=media-ytile-nv12 output={}x{} content={}x{}@{},{} source={},{} source_gpu=0x{:X} media_gpu=0x{:X} dst_gpu=0x{:X} ppgtt=source-slot-private-alias-pat0-wb,dst-base-pat3-uc bindings=3 base_alias=exact-dst-same-pte display_plane_writes=0\n",
        serial,
        job_slot,
        queue_depth_at_admission,
        UI4_COMPOSITOR_RCS_JOB_SLOTS,
        dst.width,
        dst.height,
        content_width,
        content_height,
        content_dst_x,
        content_dst_y,
        source_x,
        source_y,
        source_gpu,
        source.gpu,
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

fn remember_ui4_compositor_completion(
    runtime: &mut Ui4CompositorRuntime,
    submission: Ui4CompositorSubmission,
    completion: Ui4CompositorCompletion,
) {
    runtime
        .completions
        .retain(|(retired, _)| *retired != submission);
    runtime.completions.push_back((submission, completion));
    while runtime.completions.len() > UI4_COMPOSITOR_RCS_JOB_SLOTS * 2 {
        let _ = runtime.completions.pop_front();
    }
}

fn observe_ui4_pending_h2g_consumption(pending: &mut Ui4CompositorPending) {
    if pending.stats.probe.gpu_h2g_consumed_observe_timestamp == 0
        && crate::intel::guc_ctb::h2g_sequence_consumed(
            pending.stats.probe.guc_h2g_publish_sequence,
        )
    {
        pending.stats.probe.gpu_h2g_consumed_observe_timestamp = super::claimed_device()
            .map(direct_rcs_read_render_timestamp)
            .unwrap_or(0);
    }
}

/// Observe one compositor marker exactly once.  This function never spins.
pub(crate) fn poll_ui4_compositor_submission(
    submission: Ui4CompositorSubmission,
) -> Ui4CompositorCompletion {
    const FAILURE_TIMEOUT_MS: u64 = 1_000;

    let mut runtime = UI4_COMPOSITOR_RUNTIME.lock();
    if let Some((_, completion)) = runtime
        .completions
        .iter()
        .find(|(retired, _)| *retired == submission)
    {
        return *completion;
    }
    let Some(position) = runtime
        .pending
        .iter()
        .position(|pending| pending.submission == submission)
    else {
        return Ui4CompositorCompletion::InvalidSubmission;
    };
    if position != 0 {
        let pending = &mut runtime.pending[position];
        pending.stats.probe.completion_polls =
            pending.stats.probe.completion_polls.saturating_add(1);
        // Keep the H2G split meaningful for a queued follower. Retirement is
        // ordered, but observing its exact CTB stream position is independent
        // and must not wait for the predecessor's marker.
        observe_ui4_pending_h2g_consumption(pending);
        return Ui4CompositorCompletion::Pending;
    }
    let mut pending = runtime
        .pending
        .front()
        .copied()
        .expect("position zero requires a front submission");
    let Some(shared_state) = *UI4_COMPOSITOR_RCS_STATE.lock() else {
        drop(runtime);
        quarantine_ui4_compositor_rcs_context("pending-submission-state-missing");
        return Ui4CompositorCompletion::Pending;
    };
    let Some(state) = direct_rcs_job_slot(shared_state, pending.job_slot) else {
        drop(runtime);
        quarantine_ui4_compositor_rcs_context("pending-submission-job-slot-invalid");
        return Ui4CompositorCompletion::Pending;
    };
    pending.stats.probe.completion_polls = pending.stats.probe.completion_polls.saturating_add(1);
    observe_ui4_pending_h2g_consumption(&mut pending);
    let observed = direct_rcs_read_result_slot(state, pending.marker_slot);
    // UI4 can queue several persistent-ring entries. Requiring the saved HEAD
    // to equal the latest published tail is conservative for the front entry,
    // and proves every accepted predecessor/follower has passed BBE and the
    // context save boundary before any job slot or PPGTT mapping is reusable.
    let proof = DirectRcsRetirementProof {
        marker_observed: observed == pending.marker_value,
        saved_head_bytes: direct_rcs_read_lrc_ring_head(state) & (DIRECT_RCS_RING_BYTES as u32 - 1),
        published_tail_bytes: runtime.submit.ring_tail_bytes,
    };
    if proof.complete() {
        let host_observe = super::claimed_device()
            .map(direct_rcs_read_render_timestamp)
            .unwrap_or(0);
        pending.stats.submit_ms = direct_rcs_elapsed_ms_since(pending.started_tick);
        pending.stats.probe.submit_to_marker_us =
            direct_rcs_elapsed_us_since(pending.admitted_tick);
        if matches!(pending.kernel, "nv12-media-ytile-rgba8-frame" | "rgba8-linear-nv12-encode") {
            let batch_enter =
                direct_rcs_read_result_qword(state, UI4_VIDEO_FRAME_GPU_BATCH_ENTER_TIMESTAMP_SLOT);
            let pre =
                direct_rcs_read_result_qword(state, UI4_VIDEO_FRAME_GPU_PRE_WALKER_TIMESTAMP_SLOT);
            let post =
                direct_rcs_read_result_qword(state, UI4_VIDEO_FRAME_GPU_POST_WALKER_TIMESTAMP_SLOT);
            let post_release = direct_rcs_read_result_qword(
                state,
                UI4_VIDEO_FRAME_GPU_POST_RELEASE_TIMESTAMP_SLOT,
            );
            pending.stats.probe.gpu_batch_enter_timestamp = batch_enter;
            pending.stats.probe.gpu_pre_walker_timestamp = pre;
            pending.stats.probe.gpu_post_walker_timestamp = post;
            pending.stats.probe.gpu_post_release_timestamp = post_release;
            pending.stats.probe.gpu_host_observe_timestamp = host_observe;
            let frequency_hz = pending.stats.probe.gpu_timestamp_frequency_hz;
            if let Some((ticks, us)) = direct_rcs_timestamp_interval_us(pre, post, frequency_hz) {
                pending.stats.probe.gpu_walker_ticks = ticks;
                pending.stats.probe.gpu_walker_us = us;
                pending.stats.probe.gpu_walker_timestamp_valid = true;
            }
            let submit_to_batch = direct_rcs_timestamp_interval_us(
                pending.stats.probe.gpu_host_pre_submit_timestamp,
                batch_enter,
                frequency_hz,
            );
            let batch_to_walker = direct_rcs_timestamp_interval_us(batch_enter, pre, frequency_hz);
            let walker_to_release =
                direct_rcs_timestamp_interval_us(post, post_release, frequency_hz);
            let release_to_observe =
                direct_rcs_timestamp_interval_us(post_release, host_observe, frequency_hz);
            let submit_to_observe = direct_rcs_timestamp_interval_us(
                pending.stats.probe.gpu_host_pre_submit_timestamp,
                host_observe,
                frequency_hz,
            );
            if let (
                Some((submit_to_batch_ticks, submit_to_batch_us)),
                Some((batch_to_walker_ticks, batch_to_walker_us)),
                Some((walker_to_release_ticks, walker_to_release_us)),
                Some((release_to_observe_ticks, release_to_observe_us)),
                Some((submit_to_observe_ticks, submit_to_observe_us)),
            ) = (
                submit_to_batch,
                batch_to_walker,
                walker_to_release,
                release_to_observe,
                submit_to_observe,
            ) {
                let phase_sum_ticks = submit_to_batch_ticks
                    .saturating_add(batch_to_walker_ticks)
                    .saturating_add(pending.stats.probe.gpu_walker_ticks)
                    .saturating_add(walker_to_release_ticks)
                    .saturating_add(release_to_observe_ticks);
                if pending.stats.probe.gpu_walker_timestamp_valid
                    && phase_sum_ticks == submit_to_observe_ticks
                {
                    pending.stats.probe.gpu_pre_submit_to_batch_ticks = submit_to_batch_ticks;
                    pending.stats.probe.gpu_pre_submit_to_batch_us = submit_to_batch_us;
                    let submit_to_h2g = direct_rcs_timestamp_interval_us(
                        pending.stats.probe.gpu_host_pre_submit_timestamp,
                        pending.stats.probe.gpu_h2g_consumed_observe_timestamp,
                        frequency_hz,
                    );
                    let h2g_to_batch = direct_rcs_timestamp_interval_us(
                        pending.stats.probe.gpu_h2g_consumed_observe_timestamp,
                        batch_enter,
                        frequency_hz,
                    );
                    if let (
                        Some((submit_to_h2g_ticks, submit_to_h2g_us)),
                        Some((h2g_to_batch_ticks, h2g_to_batch_us)),
                    ) = (submit_to_h2g, h2g_to_batch)
                        && submit_to_h2g_ticks.saturating_add(h2g_to_batch_ticks)
                            == submit_to_batch_ticks
                    {
                        pending.stats.probe.gpu_pre_submit_to_h2g_consumed_ticks =
                            submit_to_h2g_ticks;
                        pending.stats.probe.gpu_pre_submit_to_h2g_consumed_us = submit_to_h2g_us;
                        pending.stats.probe.gpu_h2g_consumed_to_batch_ticks = h2g_to_batch_ticks;
                        pending.stats.probe.gpu_h2g_consumed_to_batch_us = h2g_to_batch_us;
                        pending.stats.probe.gpu_h2g_split_valid = true;
                    }
                    pending.stats.probe.gpu_batch_to_walker_ticks = batch_to_walker_ticks;
                    pending.stats.probe.gpu_batch_to_walker_us = batch_to_walker_us;
                    pending.stats.probe.gpu_walker_to_release_ticks = walker_to_release_ticks;
                    pending.stats.probe.gpu_walker_to_release_us = walker_to_release_us;
                    pending.stats.probe.gpu_release_to_observe_ticks = release_to_observe_ticks;
                    pending.stats.probe.gpu_release_to_observe_us = release_to_observe_us;
                    pending.stats.probe.gpu_pre_submit_to_observe_ticks = submit_to_observe_ticks;
                    pending.stats.probe.gpu_pre_submit_to_observe_us = submit_to_observe_us;
                    pending.stats.probe.gpu_phase_timestamps_valid = true;
                }
            }
        }
        let _ = runtime.pending.pop_front();
        let completion = Ui4CompositorCompletion::Complete(pending.stats);
        remember_ui4_compositor_completion(&mut runtime, submission, completion);
        let remaining_depth = runtime.pending.len();
        drop(runtime);
        let _ = crate::gpu::executor::complete_kernel_submission(submission.gpu, true);
        crate::log_trace!(target: "ui4";
            "ui4/guc-compositor: complete serial={} job_slot={} admission_queue_depth={} remaining_queue_depth={} saved_head={} published_tail={} retirement=marker-plus-context-save kernel={} descs={} walkers={} elapsed_ms={} gpu_phase_us=pre_submit_to_batch:{},pre_submit_to_h2g_consumed_observe:{},h2g_consumed_observe_to_batch:{},batch_to_walker:{},walker:{},walker_to_release:{},release_to_observe:{},pre_submit_to_observe:{} gpu_phase_ticks=pre_submit_to_batch:{},pre_submit_to_h2g_consumed_observe:{},h2g_consumed_observe_to_batch:{},batch_to_walker:{},walker:{},walker_to_release:{},release_to_observe:{},pre_submit_to_observe:{} gpu_timestamps=host_pre_submit:{},h2g_consumed_observe:{},batch_enter:{},pre_walker:{},post_walker:{},post_release:{},host_observe:{} guc_h2g_publish_sequence={} gpu_timestamp_hz={} gpu_walker_valid={} gpu_phase_valid={} gpu_h2g_split_valid={} h2g_split_bounds=consume-upper+dispatch-lower poll=ordered-ring\n",
            pending.submission.serial,
            pending.job_slot,
            pending.queue_depth_at_admission,
            remaining_depth,
            proof.saved_head_bytes,
            proof.published_tail_bytes,
            pending.kernel,
            pending.stats.descs,
            pending.stats.walkers,
            pending.stats.submit_ms,
            pending.stats.probe.gpu_pre_submit_to_batch_us,
            pending.stats.probe.gpu_pre_submit_to_h2g_consumed_us,
            pending.stats.probe.gpu_h2g_consumed_to_batch_us,
            pending.stats.probe.gpu_batch_to_walker_us,
            pending.stats.probe.gpu_walker_us,
            pending.stats.probe.gpu_walker_to_release_us,
            pending.stats.probe.gpu_release_to_observe_us,
            pending.stats.probe.gpu_pre_submit_to_observe_us,
            pending.stats.probe.gpu_pre_submit_to_batch_ticks,
            pending.stats.probe.gpu_pre_submit_to_h2g_consumed_ticks,
            pending.stats.probe.gpu_h2g_consumed_to_batch_ticks,
            pending.stats.probe.gpu_batch_to_walker_ticks,
            pending.stats.probe.gpu_walker_ticks,
            pending.stats.probe.gpu_walker_to_release_ticks,
            pending.stats.probe.gpu_release_to_observe_ticks,
            pending.stats.probe.gpu_pre_submit_to_observe_ticks,
            pending.stats.probe.gpu_host_pre_submit_timestamp,
            pending.stats.probe.gpu_h2g_consumed_observe_timestamp,
            pending.stats.probe.gpu_batch_enter_timestamp,
            pending.stats.probe.gpu_pre_walker_timestamp,
            pending.stats.probe.gpu_post_walker_timestamp,
            pending.stats.probe.gpu_post_release_timestamp,
            pending.stats.probe.gpu_host_observe_timestamp,
            pending.stats.probe.guc_h2g_publish_sequence,
            pending.stats.probe.gpu_timestamp_frequency_hz,
            pending.stats.probe.gpu_walker_timestamp_valid as u8,
            pending.stats.probe.gpu_phase_timestamps_valid as u8,
            pending.stats.probe.gpu_h2g_split_valid as u8,
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
            if let Some(front) = runtime.pending.front_mut() {
                *front = pending;
            }
            drop(runtime);
            let reason = if proof.marker_observed {
                "completion-marker-observed-context-save-timeout"
            } else {
                "completion-marker-timeout"
            };
            quarantine_ui4_compositor_rcs_context(reason);
            crate::log_error!(target: "ui4";
                "ui4/guc-compositor: completion overdue serial={} observed=0x{:08X} want=0x{:08X} marker_observed={} saved_head={} published_tail={} threshold_ms={} action=quarantine-keep-pending-no-reuse cancellation=unavailable log=once\n",
                pending.submission.serial,
                observed,
                pending.marker_value,
                proof.marker_observed as u8,
                proof.saved_head_bytes,
                proof.published_tail_bytes,
                FAILURE_TIMEOUT_MS,
            );
        }
        return Ui4CompositorCompletion::Pending;
    }
    if let Some(front) = runtime.pending.front_mut() {
        *front = pending;
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

/// Retire the final Blueprint sprite batch and mint the producer release for
/// the exact UI4 write lease. Intermediate alpha or arbitrary-quad batches use
/// `poll_ui4_compositor_submission()` and therefore cannot manufacture a
/// publishable fence before the complete ordered scene has retired.
pub(crate) fn poll_ui4_blueprint_sprite_scene(
    submission: Ui4CompositorSubmission,
    dst: GpgpuRgba8Surface,
) -> Ui4SpriteSceneCompletion {
    match poll_ui4_compositor_submission(submission) {
        Ui4CompositorCompletion::Pending => Ui4SpriteSceneCompletion::Pending,
        Ui4CompositorCompletion::Complete(stats) => Ui4SpriteSceneCompletion::Complete {
            stats,
            release: gpgpu_rgba8_release(dst),
        },
        Ui4CompositorCompletion::Failed | Ui4CompositorCompletion::InvalidSubmission => {
            Ui4SpriteSceneCompletion::Failed
        }
    }
}

/// Backend completion driver for awaitable UI4 GPU fences. Polling remains
/// dormant while there is no in-flight compositor job. The reaper itself owns
/// a fence waiter, proving the same wake path that future UI callers consume.
#[trueos_executor::task]
pub(crate) async fn gpu_completion_reaper_task() {
    use core::future::{Future, poll_fn};
    use core::pin::Pin;
    use core::task::Poll;
    use trueos_time::{Duration, Timer};

    let mut active: Option<(Ui4CompositorSubmission, crate::gpu::executor::GpuFence)> = None;
    loop {
        let pending = UI4_COMPOSITOR_RUNTIME
            .lock()
            .pending
            .front()
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
        // Completion belongs to the task which queued the exact request.  In
        // particular, consuming a video conversion here can let a following
        // compositor job retire before the video task observes its release.
        // Keep this task observer-only; every current submission class has a
        // persistent owner which polls and retires it.
        if UI4_COMPOSITOR_RUNTIME
            .lock()
            .pending
            .front()
            .is_none_or(|pending| pending.submission != submission)
        {
            active = None;
        }
        Timer::after(Duration::from_millis(1)).await;
    }
}
