pub(crate) fn activity_snapshot() -> GpgpuActivitySnapshot {
    let submit_seq = DIRECT_RCS_SUBMIT_COUNTER.load(Ordering::Relaxed);
    let Some(dev) = super::claimed_device() else {
        return GpgpuActivitySnapshot {
            direct_rcs_enabled: DIRECT_RCS_ENABLED,
            submit_seq,
            ..GpgpuActivitySnapshot::default()
        };
    };

    GpgpuActivitySnapshot {
        available: true,
        direct_rcs_enabled: DIRECT_RCS_ENABLED,
        submit_seq,
        ring_head: super::mmio_read(dev, RCS_RING_HEAD),
        ring_tail: super::mmio_read(dev, RCS_RING_TAIL),
        acthd: super::mmio_read(dev, RCS_RING_ACTHD),
        ipeir: super::mmio_read(dev, RCS_RING_IPEIR),
        ipehr: super::mmio_read(dev, RCS_RING_IPEHR),
        eir: super::mmio_read(dev, RCS_RING_EIR),
    }
}

pub(crate) fn submit_fill_rect_worklist_rgba8_probe_now() -> bool {
    submit_fill_rect_worklist_rgba8_probe(true)
}

fn submit_fill_rect_worklist_rgba8_probe(force: bool) -> bool {
    if !DIRECT_RCS_ENABLED {
        if force {
            FILL_RECT_WORKLIST_OK.store(false, Ordering::Release);
        }
        return false;
    }
    if !force && FILL_RECT_WORKLIST_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }
    FILL_RECT_WORKLIST_RAN.store(true, Ordering::Release);
    FILL_RECT_WORKLIST_OK.store(false, Ordering::Release);

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: fill-rect-worklist-rgba8 skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: fill-rect-worklist-rgba8 failed rung=alloc\n"
        );
        return false;
    };
    let Some(desc) = rect_worklist_desc_buffer_once() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: fill-rect-worklist-rgba8 failed rung=desc-buffer\n"
        );
        return false;
    };
    let Some(surface) = GpgpuRgba8Surface::new(
        state.clear_test_phys,
        DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        CLEAR_RECT_TEST_BYTES,
        64,
        4,
        64 * core::mem::size_of::<u32>() as u32,
    ) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: fill-rect-worklist-rgba8 failed rung=surface\n"
        );
        return false;
    };

    let _desc_guard = RECT_WORKLIST_DESC_SUBMIT_LOCK.lock();
    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CLEAR_RECT_TEST_BYTES);
        core::ptr::write_bytes(desc.virt, 0, desc.bytes);
        let descs = desc.virt as *mut FillRectWorklistRgba8Desc;
        core::ptr::write_volatile(
            descs,
            FillRectWorklistRgba8Desc {
                dst_xy: pack_i16_pair_u32(0, 0),
                size: pack_u16_pair_u32(4, 1),
                color_rgba: 0xFFCC_8844,
            },
        );
        core::ptr::write_volatile(
            descs.add(1),
            FillRectWorklistRgba8Desc {
                dst_xy: pack_i16_pair_u32(8, 1),
                size: pack_u16_pair_u32(4, 2),
                color_rgba: 0xFF10_2030,
            },
        );
    }
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    super::dma_flush(desc.virt, desc.bytes);

    let params = FillRectWorklistRgba8Params {
        dst_gpu: surface.gpu,
        desc_gpu: desc.gpu,
        dst_pitch_bytes: surface.pitch_bytes,
        desc_base: 0,
        desc_count: 2,
    };
    let start_tick = direct_rcs_now_tick();
    let submitted = submit_fill_rect_worklist(surface, desc, params, false);
    let submit_ms = direct_rcs_elapsed_ms_since(start_tick);
    let pre_marker = direct_rcs_read_result_slot(state, RECT_WORKLIST_PRE_MARKER_SLOT);
    let post_marker = direct_rcs_read_result_slot(state, RECT_WORKLIST_POST_MARKER_SLOT);
    let row0 = direct_rcs_read_worklist_probe_span(state, 0, 0);
    let row1 = direct_rcs_read_worklist_probe_span(state, 1, 8);
    let row2 = direct_rcs_read_worklist_probe_span(state, 2, 8);
    let ok = submitted
        && pre_marker == FILL_RECT_WORKLIST_PRE_MARKER
        && post_marker == FILL_RECT_WORKLIST_POST_MARKER
        && row0 == [0xFFCC_8844; 4]
        && row1 == [0xFF10_2030; 4]
        && row2 == [0xFF10_2030; 4];

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: fill-rect-worklist-rgba8 forcewake=1 ggtt=1 ppgtt=1 kernel_ppgtt=1 dst_ppgtt=1 desc_ppgtt=1 batch=1 submitted={} ok={} submit_ms={} descs=2 walkers={} pre_marker=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} dst_gpu=0x{:X} desc_gpu=0x{:X} row0=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] row1=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] row2=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] artifact={}\n",
        submitted as u8,
        ok as u8,
        submit_ms,
        rect_worklist_walker_count(2),
        pre_marker,
        post_marker,
        FILL_RECT_WORKLIST_POST_MARKER,
        FILL_RECT_WORKLIST_RGBA8_ADLS_GPU,
        FILL_RECT_WORKLIST_RGBA8_ADLS_GPU + FILL_RECT_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        surface.gpu,
        desc.gpu,
        row0[0],
        row0[1],
        row0[2],
        row0[3],
        row1[0],
        row1[1],
        row1[2],
        row1[3],
        row2[0],
        row2[1],
        row2[2],
        row2[3],
        FILL_RECT_WORKLIST_RGBA8_KERNEL_NAME,
    );

    FILL_RECT_WORKLIST_OK.store(ok, Ordering::Release);
    ok
}

pub(crate) fn sprite_quad_worklist_ready() -> bool {
    if SPRITE_QUAD_WORKLIST_OK.load(Ordering::Acquire) {
        return true;
    }
    let _ = submit_sprite_quad_worklist_rgba8_probe_once();
    SPRITE_QUAD_WORKLIST_OK.load(Ordering::Acquire)
}

pub(crate) fn submit_sprite_quad_worklist_rgba8_probe_once() -> bool {
    submit_sprite_quad_worklist_rgba8_probe(false)
}

fn submit_sprite_quad_worklist_rgba8_probe(force: bool) -> bool {
    if !DIRECT_RCS_ENABLED {
        if force {
            SPRITE_QUAD_WORKLIST_OK.store(false, Ordering::Release);
        }
        return false;
    }
    if !force && SPRITE_QUAD_WORKLIST_RAN.swap(true, Ordering::AcqRel) {
        return false;
    }
    SPRITE_QUAD_WORKLIST_RAN.store(true, Ordering::Release);
    SPRITE_QUAD_WORKLIST_OK.store(false, Ordering::Release);

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: sprite-quad-worklist-rgba8 skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: sprite-quad-worklist-rgba8 failed rung=alloc\n"
        );
        return false;
    };
    let Some(desc) = sprite_quad_worklist_desc_buffer_once() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: sprite-quad-worklist-rgba8 failed rung=desc-buffer\n"
        );
        return false;
    };
    let Some(surface) = GpgpuRgba8Surface::new(
        state.clear_test_phys,
        DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE,
        CLEAR_RECT_TEST_BYTES,
        64,
        4,
        64 * core::mem::size_of::<u32>() as u32,
    ) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: sprite-quad-worklist-rgba8 failed rung=surface\n"
        );
        return false;
    };

    let _desc_guard = RECT_WORKLIST_DESC_SUBMIT_LOCK.lock();
    let src00 = 0xFF00_00FF;
    let src01 = 0xFF00_FF00;
    let src10 = 0xFFFF_0000;
    let src11 = 0xFFFF_FFFF;
    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CLEAR_RECT_TEST_BYTES);
        core::ptr::write_bytes(desc.virt, 0, desc.bytes);
        let pixels = state.clear_test_virt as *mut u32;
        core::ptr::write_volatile(pixels, src00);
        core::ptr::write_volatile(pixels.add(1), src01);
        core::ptr::write_volatile(pixels.add(64), src10);
        core::ptr::write_volatile(pixels.add(65), src11);
        let descs = desc.virt as *mut GpgpuSpriteQuadWorklistDesc;
        core::ptr::write_volatile(
            descs,
            GpgpuSpriteQuadWorklistDesc {
                c0_x: 10.0,
                c0_y: 1.0,
                c0_u: 0.0,
                c0_v: 0.0,
                c1_x: 12.0,
                c1_y: 1.0,
                c1_u: 2.0 / 64.0,
                c1_v: 0.0,
                c2_x: 12.0,
                c2_y: 3.0,
                c2_u: 2.0 / 64.0,
                c2_v: 2.0 / 4.0,
                c3_x: 10.0,
                c3_y: 3.0,
                c3_u: 0.0,
                c3_v: 2.0 / 4.0,
                color_rgba: 0xFFFF_FFFF,
                flags: SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER,
            },
        );
    }
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    super::dma_flush(desc.virt, desc.bytes);

    let params = SpriteQuadWorklistRgba8Params {
        src_gpu: surface.gpu,
        dst_gpu: surface.gpu,
        desc_gpu: desc.gpu,
        src_pitch_bytes: surface.pitch_bytes,
        dst_pitch_bytes: surface.pitch_bytes,
        src_width: surface.width,
        src_height: surface.height,
        dst_width: surface.width,
        dst_height: surface.height,
        desc_base: 0,
        desc_count: 1,
    };
    let start_tick = direct_rcs_now_tick();
    let submitted = submit_sprite_quad_worklist(surface, surface, desc, params);
    let submit_ms = direct_rcs_elapsed_ms_since(start_tick);
    let pre_marker = direct_rcs_read_result_slot(state, SPRITE_QUAD_WORKLIST_PRE_MARKER_SLOT);
    let post_marker = direct_rcs_read_result_slot(state, SPRITE_QUAD_WORKLIST_POST_MARKER_SLOT);
    let row1 = direct_rcs_read_worklist_probe_span(state, 1, 10);
    let row2 = direct_rcs_read_worklist_probe_span(state, 2, 10);
    let ok = submitted
        && pre_marker == SPRITE_QUAD_WORKLIST_PRE_MARKER
        && post_marker == SPRITE_QUAD_WORKLIST_POST_MARKER
        && row1[0] == src00
        && row1[1] == src01
        && row2[0] == src10
        && row2[1] == src11;

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: sprite-quad-worklist-rgba8 forcewake=1 ggtt=1 ppgtt=1 kernel_ppgtt=1 src_ppgtt=1 dst_ppgtt=1 desc_ppgtt=1 batch=1 submitted={} ok={} submit_ms={} descs=1 walkers={} pre_marker=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} src_gpu=0x{:X} dst_gpu=0x{:X} desc_gpu=0x{:X} row1=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] row2=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] artifact={}\n",
        submitted as u8,
        ok as u8,
        submit_ms,
        sprite_quad_worklist_walker_count(1),
        pre_marker,
        post_marker,
        SPRITE_QUAD_WORKLIST_POST_MARKER,
        SPRITE_QUAD_WORKLIST_RGBA8_ADLS_GPU,
        SPRITE_QUAD_WORKLIST_RGBA8_ADLS_GPU + SPRITE_QUAD_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        surface.gpu,
        surface.gpu,
        desc.gpu,
        row1[0],
        row1[1],
        row1[2],
        row1[3],
        row2[0],
        row2[1],
        row2[2],
        row2[3],
        SPRITE_QUAD_WORKLIST_RGBA8_KERNEL_NAME,
    );

    SPRITE_QUAD_WORKLIST_OK.store(ok, Ordering::Release);
    ok
}

fn rect_worklist_desc_buffer_once() -> Option<GpgpuRectWorklistDescBuffer> {
    let mut guard = GPGPU_RECT_WORKLIST_DESC.lock();
    if let Some(buffer) = *guard {
        return Some(buffer);
    }

    let bytes = align_up(RECT_WORKLIST_DESC_BYTES, super::WARM_ALIGN)?;
    let (phys, virt) = crate::dma::alloc(bytes, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    super::dma_flush(virt, bytes);

    let buffer = GpgpuRectWorklistDescBuffer {
        phys,
        gpu: RECT_WORKLIST_DESC_GPU,
        virt,
        bytes,
    };
    *guard = Some(buffer);
    Some(buffer)
}

fn sprite_quad_worklist_desc_buffer_once() -> Option<GpgpuRectWorklistDescBuffer> {
    let mut guard = GPGPU_SPRITE_QUAD_WORKLIST_DESC.lock();
    if let Some(buffer) = *guard {
        return Some(buffer);
    }

    let bytes = align_up(SPRITE_QUAD_WORKLIST_DESC_BYTES, super::WARM_ALIGN)?;
    let (phys, virt) = crate::dma::alloc(bytes, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    super::dma_flush(virt, bytes);

    let buffer = GpgpuRectWorklistDescBuffer {
        phys,
        gpu: SPRITE_QUAD_WORKLIST_DESC_GPU,
        virt,
        bytes,
    };
    *guard = Some(buffer);
    Some(buffer)
}

fn ui4_compositor_sprite_quad_desc_buffer_once() -> Option<GpgpuRectWorklistDescBuffer> {
    let mut guard = UI4_COMPOSITOR_SPRITE_QUAD_DESC.lock();
    if let Some(buffer) = *guard {
        return Some(buffer);
    }

    let bytes = align_up(SPRITE_QUAD_WORKLIST_DESC_BYTES, super::WARM_ALIGN)?;
    let (phys, virt) = crate::dma::alloc(bytes, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    super::dma_flush(virt, bytes);

    // This numeric VA may match the ordinary descriptor VA because the UI4
    // compositor owns a distinct PPGTT root.  The physical page is separate
    // so an ordinary GPGPU submission cannot overwrite an in-flight frame.
    let buffer = GpgpuRectWorklistDescBuffer {
        phys,
        gpu: SPRITE_QUAD_WORKLIST_DESC_GPU,
        virt,
        bytes,
    };
    *guard = Some(buffer);
    Some(buffer)
}

fn mandel64_worklist_desc_buffer_once() -> Option<GpgpuRectWorklistDescBuffer> {
    let mut guard = GPGPU_MANDEL64_WORKLIST_DESC.lock();
    if let Some(buffer) = *guard {
        return Some(buffer);
    }

    let bytes = align_up(RECT_WORKLIST_DESC_BYTES, super::WARM_ALIGN)?;
    let (phys, virt) = crate::dma::alloc(bytes, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    super::dma_flush(virt, bytes);

    let buffer = GpgpuRectWorklistDescBuffer {
        phys,
        gpu: MANDEL64_WORKLIST_DESC_GPU,
        virt,
        bytes,
    };
    *guard = Some(buffer);
    Some(buffer)
}

fn rect_is_inside_mask(surface: GpgpuMask8Surface, rect: GpgpuRect) -> bool {
    if rect.is_empty() || rect.x < 0 || rect.y < 0 {
        return false;
    }
    let x2 = rect.x as i64 + rect.width as i64;
    let y2 = rect.y as i64 + rect.height as i64;
    x2 <= surface.width as i64 && y2 <= surface.height as i64
}

