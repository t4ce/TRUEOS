fn direct_rcs_write_copy_rect_interface_descriptor_at(
    state: DirectRcsState,
    idd_offset: usize,
    binding_table_offset: usize,
    text_offset_bytes: u64,
) -> bool {
    direct_rcs_write_copy_rect_interface_descriptor_at_with_cross_thread_grfs(
        state,
        idd_offset,
        binding_table_offset,
        text_offset_bytes,
        3,
    )
}

fn direct_rcs_write_copy_rect_interface_descriptor_at_with_cross_thread_grfs(
    state: DirectRcsState,
    idd_offset: usize,
    binding_table_offset: usize,
    text_offset_bytes: u64,
    cross_thread_grfs: u32,
) -> bool {
    direct_rcs_write_interface_descriptor_at(
        state,
        idd_offset,
        binding_table_offset,
        text_offset_bytes,
        2,
        cross_thread_grfs,
    )
}

fn direct_rcs_write_interface_descriptor_at(
    state: DirectRcsState,
    idd_offset: usize,
    binding_table_offset: usize,
    text_offset_bytes: u64,
    binding_count: u32,
    cross_thread_grfs: u32,
) -> bool {
    if idd_offset + COPY_RECT_IDD_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let idd = unsafe { state.batch_virt.add(idd_offset) as *mut u32 };
    unsafe {
        core::ptr::write_volatile(idd, text_offset_bytes as u32);
        core::ptr::write_volatile(idd.add(1), 0);
        core::ptr::write_volatile(idd.add(2), IDD_THREAD_PREEMPTION_DISABLE);
        core::ptr::write_volatile(idd.add(3), 0);
        core::ptr::write_volatile(
            idd.add(4),
            (binding_table_offset as u32) | binding_count.min(31),
        );
        core::ptr::write_volatile(idd.add(5), 3 << 16);
        core::ptr::write_volatile(idd.add(6), GPGPU_WALKER_GROUP_THREADS);
        core::ptr::write_volatile(idd.add(7), cross_thread_grfs);
    }
    true
}

fn direct_rcs_write_copy_rect_surface_states_at(
    state: DirectRcsState,
    binding_table_offset: usize,
    src_surface_offset: usize,
    dst_surface_offset: usize,
    src_gpu: u64,
    src_bytes: usize,
    dst_gpu: u64,
    dst_bytes: usize,
) -> bool {
    let binding_end = binding_table_offset + 2 * core::mem::size_of::<u32>();
    let surface_bytes = COPY_RECT_SURFACE_STATE_DWORDS * core::mem::size_of::<u32>();
    let src_surface_end = src_surface_offset + surface_bytes;
    let dst_surface_end = dst_surface_offset + surface_bytes;
    if binding_end > DIRECT_RCS_BATCH_BYTES
        || src_surface_end > DIRECT_RCS_BATCH_BYTES
        || dst_surface_end > DIRECT_RCS_BATCH_BYTES
    {
        return false;
    }

    unsafe {
        let binding = state.batch_virt.add(binding_table_offset) as *mut u32;
        core::ptr::write_volatile(binding, src_surface_offset as u32);
        core::ptr::write_volatile(binding.add(1), dst_surface_offset as u32);
    }
    direct_rcs_write_buffer_surface_state(state, src_surface_offset, src_gpu, src_bytes)
        && direct_rcs_write_buffer_surface_state(state, dst_surface_offset, dst_gpu, dst_bytes)
}

fn direct_rcs_write_buffer_surface_state(
    state: DirectRcsState,
    surface_offset: usize,
    gpu: u64,
    target_bytes: usize,
) -> bool {
    let surface_bytes = COPY_RECT_SURFACE_STATE_DWORDS * core::mem::size_of::<u32>();
    let surface_end = surface_offset + surface_bytes;
    if surface_end > DIRECT_RCS_BATCH_BYTES || target_bytes == 0 {
        return false;
    }

    let extent = target_bytes.saturating_sub(1);
    let surface_width_minus1 = (extent & 0x7F) as u32;
    let surface_height_minus1 = ((extent >> 7) & 0x3FFF) as u32;
    let surface_depth_minus1 = ((extent >> 21) & 0x7FF) as u32;
    let surface_dword0 = (SURFTYPE_BUFFER << 29) | (SURFACE_FORMAT_RAW << 18);
    let surface_dword2 = (surface_height_minus1 << 16) | surface_width_minus1;
    let surface_dword3 = surface_depth_minus1 << 21;

    unsafe {
        let surface = state.batch_virt.add(surface_offset) as *mut u32;
        for index in 0..COPY_RECT_SURFACE_STATE_DWORDS {
            core::ptr::write_volatile(surface.add(index), 0);
        }
        core::ptr::write_volatile(surface, surface_dword0);
        core::ptr::write_volatile(surface.add(1), RENDER_MOCS << 24);
        core::ptr::write_volatile(surface.add(2), surface_dword2);
        core::ptr::write_volatile(surface.add(3), surface_dword3);
        core::ptr::write_volatile(surface.add(8), gpu as u32);
        core::ptr::write_volatile(surface.add(9), (gpu >> 32) as u32);
    }
    true
}

fn direct_rcs_write_ui4_nv12_frame_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: Ui4Nv12Tile64ToRgba8FrameParams,
) -> bool {
    if payload_offset + UI4_NV12_PRIMARY_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, UI4_NV12_PRIMARY_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        // The SIMD16 artifact's .ze_info assigns bytes 0..12 to
        // `global_id_offset`, not to global size.  Keep all three components
        // zero: the walker supplies the group dimensions, while the explicit
        // output_width/output_height kernel arguments live at bytes 88/92
        // below.  Feeding the output extent here starts every invocation at
        // (width, height), so the kernel's first bounds check retires every
        // lane without touching the destination even though its completion
        // marker still succeeds.
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.nv12_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.nv12_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.base_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.base_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(17), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(18), params.src_pitch_bytes);
        core::ptr::write_volatile(dwords.add(19), params.src_uv_offset);
        core::ptr::write_volatile(dwords.add(20), params.base_pitch_bytes);
        core::ptr::write_volatile(dwords.add(21), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(22), params.output_width);
        core::ptr::write_volatile(dwords.add(23), params.output_height);
        core::ptr::write_volatile(dwords.add(24), params.content_dst_x);
        core::ptr::write_volatile(dwords.add(25), params.content_dst_y);
        core::ptr::write_volatile(dwords.add(26), params.content_width);
        core::ptr::write_volatile(dwords.add(27), params.content_height);
        core::ptr::write_volatile(dwords.add(28), params.source_x);
        core::ptr::write_volatile(dwords.add(29), params.source_y);

        let local_ids = payload.add(UI4_NV12_PRIMARY_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_ui4_rgba8_to_nv12_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: Ui4Rgba8ToNv12LinearParams,
) -> bool {
    if payload_offset + UI4_RGBA8_TO_NV12_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, UI4_RGBA8_TO_NV12_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.src_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.src_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.src_pitch_bytes);
        core::ptr::write_volatile(dwords.add(17), params.src_width);
        core::ptr::write_volatile(dwords.add(18), params.src_height);
        core::ptr::write_volatile(dwords.add(19), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(20), params.dst_width);
        core::ptr::write_volatile(dwords.add(21), params.dst_height);
        core::ptr::write_volatile(dwords.add(22), params.active_top);
        core::ptr::write_volatile(dwords.add(23), params.active_height);
        let local_ids = payload.add(UI4_RGBA8_TO_NV12_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_copy_rect_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: CopyRectRgba8Params,
) -> bool {
    if payload_offset + COPY_RECT_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, COPY_RECT_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.src_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.src_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.src_pitch_bytes);
        core::ptr::write_volatile(dwords.add(17), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(18), params.src_x);
        core::ptr::write_volatile(dwords.add(19), params.src_y);
        core::ptr::write_volatile(dwords.add(20), params.dst_x);
        core::ptr::write_volatile(dwords.add(21), params.dst_y);
        core::ptr::write_volatile(dwords.add(22), params.width);
        core::ptr::write_volatile(dwords.add(23), params.height);

        let local_ids = payload.add(COPY_RECT_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_glyph_mask_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: CopyRectRgba8Params,
    color_rgba: u32,
) -> bool {
    if payload_offset + GLYPH_MASK_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, GLYPH_MASK_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.src_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.src_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.src_pitch_bytes);
        core::ptr::write_volatile(dwords.add(17), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(18), params.src_x);
        core::ptr::write_volatile(dwords.add(19), params.src_y);
        core::ptr::write_volatile(dwords.add(20), params.dst_x);
        core::ptr::write_volatile(dwords.add(21), params.dst_y);
        core::ptr::write_volatile(dwords.add(22), params.width);
        core::ptr::write_volatile(dwords.add(23), params.height);
        core::ptr::write_volatile(dwords.add(24), color_rgba);

        let local_ids = payload.add(GLYPH_MASK_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_font_instance_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    layer: GpgpuFontInstanceLayer,
    descriptor_gpu: u64,
    dst: GpgpuRgba8Surface,
    dispatch: GpgpuRect,
    time_seconds: f32,
) -> bool {
    if payload_offset + FONT_INSTANCE_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES
        || dispatch.x < 0
        || dispatch.y < 0
    {
        return false;
    }
    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, FONT_INSTANCE_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), layer.mask.gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (layer.mask.gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), dst.gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (dst.gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), descriptor_gpu as u32);
        core::ptr::write_volatile(dwords.add(17), (descriptor_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(18), dst.pitch_bytes);
        core::ptr::write_volatile(dwords.add(19), dispatch.x as u32);
        core::ptr::write_volatile(dwords.add(20), dispatch.y as u32);
        core::ptr::write_volatile(dwords.add(21), dispatch.width);
        core::ptr::write_volatile(dwords.add(22), dispatch.height);
        core::ptr::write_volatile(dwords.add(23), layer.dst_center[0].to_bits());
        core::ptr::write_volatile(dwords.add(24), layer.dst_center[1].to_bits());
        core::ptr::write_volatile(dwords.add(25), time_seconds.to_bits());

        let local_ids = payload.add(FONT_INSTANCE_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_skybox_sample_rgb565_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: SkyboxSampleRgb565Params,
) -> bool {
    if payload_offset + SKYBOX_SAMPLE_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, SKYBOX_SAMPLE_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.sky_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.sky_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.dst_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(16), params.sky_pitch_bytes);
        core::ptr::write_volatile(dwords.add(17), params.sky_width);
        core::ptr::write_volatile(dwords.add(18), params.sky_height);
        core::ptr::write_volatile(dwords.add(19), params.dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(20), params.dst_width);
        core::ptr::write_volatile(dwords.add(21), params.dst_height);
        core::ptr::write_volatile(dwords.add(22), params.rect_x);
        core::ptr::write_volatile(dwords.add(23), params.rect_y);
        core::ptr::write_volatile(dwords.add(24), params.rect_width);
        core::ptr::write_volatile(dwords.add(25), params.rect_height);
        core::ptr::write_volatile(dwords.add(26), params.right_x.to_bits());
        core::ptr::write_volatile(dwords.add(27), params.right_y.to_bits());
        core::ptr::write_volatile(dwords.add(28), params.right_z.to_bits());
        core::ptr::write_volatile(dwords.add(29), params.up_x.to_bits());
        core::ptr::write_volatile(dwords.add(30), params.up_y.to_bits());
        core::ptr::write_volatile(dwords.add(31), params.up_z.to_bits());
        core::ptr::write_volatile(dwords.add(32), params.forward_x.to_bits());
        core::ptr::write_volatile(dwords.add(33), params.forward_y.to_bits());
        core::ptr::write_volatile(dwords.add(34), params.forward_z.to_bits());
        core::ptr::write_volatile(dwords.add(35), params.aspect_tan_half_fov_y.to_bits());
        core::ptr::write_volatile(dwords.add(36), params.tan_half_fov_y.to_bits());

        let local_ids = payload.add(SKYBOX_SAMPLE_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_chart_sine_rgba8_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: ChartSineRgba8Params,
) -> bool {
    if payload_offset + CHART_SINE_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let Some(known) = super::opencl::registry::known_aot_kernel(CHART_SINE_RGBA8_KERNEL_NAME)
    else {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: chart-sine-rgba8 payload rejected reason=missing-opencl-contract\n"
        );
        return false;
    };

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, CHART_SINE_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.dst_gpu >> 32) as u32);

        let cross_thread = core::slice::from_raw_parts_mut(payload, CHART_SINE_CROSS_THREAD_BYTES);
        let values = (|| {
            let mut writer = super::opencl::KernelValueWriter::new(known.contract, cross_thread)?;
            writer.set_u32(1, params.dst_pitch_bytes)?;
            writer.set_u32(2, params.dst_width)?;
            writer.set_u32(3, params.dst_height)?;
            writer.set_u32(4, params.rect_x)?;
            writer.set_u32(5, params.rect_y)?;
            writer.set_u32(6, params.rect_width)?;
            writer.set_u32(7, params.rect_height)?;
            writer.set_f32(8, params.phase)?;
            writer.set_f32(9, params.cycles)?;
            writer.set_f32(10, params.amplitude)?;
            writer.set_f32(11, params.line_width_px)?;
            writer.set_u32(12, params.background_rgba)?;
            writer.set_u32(13, params.minor_grid_rgba)?;
            writer.set_u32(14, params.major_grid_rgba)?;
            writer.set_u32(15, params.axis_rgba)?;
            writer.set_u32(16, params.line_rgba)?;
            writer.set_u32(17, params.glow_rgba)?;
            writer.set_u32(18, params.flags)?;
            writer.finish()?;
            Ok::<(), super::opencl::KernelValueError>(())
        })();
        if let Err(err) = values {
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: chart-sine-rgba8 payload rejected reason=value-contract error={:?}\n",
                err
            );
            return false;
        }

        let local_ids = payload.add(CHART_SINE_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_pixel_plasma_rgba8_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: PixelPlasmaRgba8Params,
) -> bool {
    if payload_offset + PIXEL_PLASMA_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let Some(known) = super::opencl::registry::known_aot_kernel(PIXEL_PLASMA_RGBA8_KERNEL_NAME)
    else {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: pixel-plasma-rgba8 payload rejected reason=missing-opencl-contract\n"
        );
        return false;
    };

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, PIXEL_PLASMA_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.dst_gpu >> 32) as u32);

        let cross_thread =
            core::slice::from_raw_parts_mut(payload, PIXEL_PLASMA_CROSS_THREAD_BYTES);
        let values = (|| {
            let mut writer = super::opencl::KernelValueWriter::new(known.contract, cross_thread)?;
            writer.set_u32(1, params.dst_pitch_bytes)?;
            writer.set_u32(2, params.dst_width)?;
            writer.set_u32(3, params.dst_height)?;
            writer.set_u32(4, params.rect_x)?;
            writer.set_u32(5, params.rect_y)?;
            writer.set_u32(6, params.rect_width)?;
            writer.set_u32(7, params.rect_height)?;
            writer.set_f32(8, params.time)?;
            writer.set_f32(9, params.spatial_scale)?;
            writer.set_f32(10, params.intensity)?;
            writer.set_u32(11, params.low_rgba)?;
            writer.set_u32(12, params.mid_rgba)?;
            writer.set_u32(13, params.high_rgba)?;
            writer.set_u32(14, params.flags)?;
            writer.finish()?;
            Ok::<(), super::opencl::KernelValueError>(())
        })();
        if let Err(err) = values {
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: pixel-plasma-rgba8 payload rejected reason=value-contract error={:?}\n",
                err
            );
            return false;
        }

        let local_ids = payload.add(PIXEL_PLASMA_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_font_outline_coverage_r8_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: FontOutlineCoverageR8Params,
) -> bool {
    if payload_offset + FONT_OUTLINE_COVERAGE_R8_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let Some(known) =
        super::opencl::registry::known_aot_kernel(FONT_OUTLINE_COVERAGE_R8_KERNEL_NAME)
    else {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: font-outline-coverage-r8 payload rejected reason=missing-opencl-contract\n"
        );
        return false;
    };
    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, FONT_OUTLINE_COVERAGE_R8_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.ops_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.ops_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.mask_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.mask_gpu >> 32) as u32);

        let cross_thread =
            core::slice::from_raw_parts_mut(payload, FONT_OUTLINE_COVERAGE_R8_CROSS_THREAD_BYTES);
        let values = (|| {
            let mut writer = super::opencl::KernelValueWriter::new(known.contract, cross_thread)?;
            writer.set_u32(2, params.op_count)?;
            writer.set_u32(3, params.subdivisions)?;
            writer.set_u32(4, params.mask_pitch_bytes)?;
            writer.set_u32(5, params.mask_width)?;
            writer.set_u32(6, params.mask_height)?;
            writer.set_u32(7, params.rect_x)?;
            writer.set_u32(8, params.rect_y)?;
            writer.set_u32(9, params.rect_width)?;
            writer.set_u32(10, params.rect_height)?;
            writer.set_f32(11, params.optical_bias_px)?;
            writer.finish()?;
            Ok::<(), super::opencl::KernelValueError>(())
        })();
        if let Err(err) = values {
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: font-outline-coverage-r8 payload rejected reason=value-contract error={:?}\n",
                err
            );
            return false;
        }
        let local_ids = payload.add(FONT_OUTLINE_COVERAGE_R8_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}

fn direct_rcs_write_font_outline_mesh_payload_at(
    state: DirectRcsState,
    payload_offset: usize,
    params: FontOutlineMeshParams,
) -> bool {
    if payload_offset + FONT_OUTLINE_MESH_INDIRECT_BYTES > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    let Some(known) = super::opencl::registry::known_aot_kernel(FONT_OUTLINE_MESH_KERNEL_NAME)
    else {
        crate::log_error!(
            target: "gpgpu";
            "intel/gpgpu: font-outline-mesh payload rejected reason=missing-opencl-contract\n"
        );
        return false;
    };

    unsafe {
        let payload = state.batch_virt.add(payload_offset);
        core::ptr::write_bytes(payload, 0, FONT_OUTLINE_MESH_INDIRECT_BYTES);
        let dwords = payload as *mut u32;
        core::ptr::write_volatile(dwords.add(3), 16);
        core::ptr::write_volatile(dwords.add(4), 1);
        core::ptr::write_volatile(dwords.add(5), 1);
        core::ptr::write_volatile(dwords.add(8), 16);
        core::ptr::write_volatile(dwords.add(9), 1);
        core::ptr::write_volatile(dwords.add(10), 1);
        core::ptr::write_volatile(dwords.add(12), params.src_gpu as u32);
        core::ptr::write_volatile(dwords.add(13), (params.src_gpu >> 32) as u32);
        core::ptr::write_volatile(dwords.add(14), params.dst_gpu as u32);
        core::ptr::write_volatile(dwords.add(15), (params.dst_gpu >> 32) as u32);

        let cross_thread =
            core::slice::from_raw_parts_mut(payload, FONT_OUTLINE_MESH_CROSS_THREAD_BYTES);
        let values = (|| {
            let mut writer = super::opencl::KernelValueWriter::new(known.contract, cross_thread)?;
            writer.set_u32(2, params.op_count)?;
            writer.set_u32(3, params.stage)?;
            writer.set_u32(4, params.subdivisions)?;
            writer.set_u32(5, params.max_vertices)?;
            writer.set_u32(6, params.max_indices)?;
            writer.set_f32(7, params.scale)?;
            writer.set_f32(8, params.origin_x)?;
            writer.set_f32(9, params.origin_y)?;
            writer.set_f32(10, params.stroke_half_width)?;
            writer.finish()?;
            Ok::<(), super::opencl::KernelValueError>(())
        })();
        if let Err(err) = values {
            crate::log_error!(
                target: "gpgpu";
                "intel/gpgpu: font-outline-mesh payload rejected reason=value-contract error={:?}\n",
                err
            );
            return false;
        }

        let local_ids = payload.add(FONT_OUTLINE_MESH_CROSS_THREAD_BYTES) as *mut u16;
        for lane in 0..16usize {
            core::ptr::write_volatile(local_ids.add(lane), lane as u16);
            core::ptr::write_volatile(local_ids.add(16 + lane), 0);
            core::ptr::write_volatile(local_ids.add(32 + lane), 0);
        }
    }
    true
}
