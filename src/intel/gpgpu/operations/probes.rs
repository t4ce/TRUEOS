const COPY_RECT_PROBE_HALF_BYTES: usize = CLEAR_RECT_TEST_BYTES / 2;
const COPY_RECT_PROBE_SRC_GPU: u64 = DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE;
const COPY_RECT_PROBE_DST_GPU: u64 =
    DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE + COPY_RECT_PROBE_HALF_BYTES as u64;

const _: () = assert!(CLEAR_RECT_TEST_BYTES.is_multiple_of(2));
const _: () = assert!(COPY_RECT_PROBE_HALF_BYTES.is_multiple_of(4096));
const _: () = assert!(COPY_RECT_PROBE_SRC_GPU.is_multiple_of(4096));
const _: () = assert!(COPY_RECT_PROBE_DST_GPU.is_multiple_of(4096));
const _: () = assert!(
    COPY_RECT_PROBE_DST_GPU + COPY_RECT_PROBE_HALF_BYTES as u64
        <= DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE + CLEAR_RECT_TEST_BYTES as u64
);













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
        ring_start: super::mmio_read(dev, RCS_RING_START),
        ring_ctl: super::mmio_read(dev, RCS_RING_CTL),
        acthd: super::mmio_read(dev, RCS_RING_ACTHD),
        mi_mode: super::mmio_read(dev, RCS_RING_MI_MODE),
        mode: super::mmio_read(dev, RCS_RING_MODE_GEN7),
        context_control: super::mmio_read(dev, RCS_RING_CONTEXT_CONTROL),
        execlist_control: super::mmio_read(dev, RCS_RING_EXECLIST_CONTROL),
        execlist_status_lo: super::mmio_read(dev, RCS_RING_EXECLIST_STATUS_LO),
        execlist_status_hi: super::mmio_read(dev, RCS_RING_EXECLIST_STATUS_HI),
        ipeir: super::mmio_read(dev, RCS_RING_IPEIR),
        ipehr: super::mmio_read(dev, RCS_RING_IPEHR),
        eir: super::mmio_read(dev, RCS_RING_EIR),
        instdone: super::mmio_read(dev, RCS_RING_INSTDONE),
        instps: super::mmio_read(dev, RCS_RING_INSTPS),
    }
}

pub(crate) fn submit_solid_composite_worklist_rgba8_probe_now() -> bool {
    if !DIRECT_RCS_ENABLED {
        return false;
    }

    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: solid-composite-worklist-rgba8 skipped reason=no-claimed-device\n"
        );
        return false;
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: solid-composite-worklist-rgba8 failed rung=alloc\n"
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
            "intel/gpgpu: solid-composite-worklist-rgba8 failed rung=surface\n"
        );
        return false;
    };

    unsafe {
        core::ptr::write_bytes(state.clear_test_virt, 0, CLEAR_RECT_TEST_BYTES);
    }
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);
    let rects = [
        GpgpuSolidRect {
            rect: GpgpuRect::new(0, 0, 4, 1),
            color_rgba: 0xFFCC_8844,
        },
        GpgpuSolidRect {
            rect: GpgpuRect::new(8, 1, 4, 2),
            color_rgba: 0xFF10_2030,
        },
    ];
    let start_tick = direct_rcs_now_tick();
    let submitted = fill_solid_rects_rgba8_result(surface, rects.as_slice());
    let submit_ms = direct_rcs_elapsed_ms_since(start_tick);
    let pre_marker = direct_rcs_read_result_slot(state, RECT_WORKLIST_PRE_MARKER_SLOT);
    let post_marker = direct_rcs_read_result_slot(state, RECT_WORKLIST_POST_MARKER_SLOT);
    let row0 = direct_rcs_read_worklist_probe_span(state, 0, 0);
    let row1 = direct_rcs_read_worklist_probe_span(state, 1, 8);
    let row2 = direct_rcs_read_worklist_probe_span(state, 2, 8);
    let ok = submitted.outcome == GpgpuSubmissionOutcome::Complete
        && submitted.stats.descs == rects.len()
        && submitted.stats.submits == 1
        && pre_marker == ALPHA_BLEND_WORKLIST_PRE_MARKER
        && post_marker == ALPHA_BLEND_WORKLIST_POST_MARKER
        && row0 == [0xFFCC_8844; 4]
        && row1 == [0xFF10_2030; 4]
        && row2 == [0xFF10_2030; 4];

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: solid-composite-worklist-rgba8 forcewake=1 ggtt=1 ppgtt=1 kernel_ppgtt=1 dst_ppgtt=1 desc_ppgtt=1 batch=1 submitted={} ok={} submit_ms={} descs=2 walkers={} pre_marker=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} dst_gpu=0x{:X} row0=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] row1=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] row2=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] artifact={} mode=source-free-solid\n",
        (submitted.outcome == GpgpuSubmissionOutcome::Complete) as u8,
        ok as u8,
        submit_ms,
        rect_worklist_walker_count(2),
        pre_marker,
        post_marker,
        ALPHA_BLEND_WORKLIST_POST_MARKER,
        ALPHA_BLEND_WORKLIST_RGBA8_ADLS_GPU,
        ALPHA_BLEND_WORKLIST_RGBA8_ADLS_GPU + ALPHA_BLEND_WORKLIST_RGBA8_TEXT_OFFSET_BYTES,
        surface.gpu,
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
        ALPHA_BLEND_WORKLIST_RGBA8_KERNEL_NAME,
    );

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



fn ui4_compositor_desc_buffer_once() -> Option<GpgpuRectWorklistDescBuffer> {
    let mut guard = UI4_COMPOSITOR_DESC.lock();
    if let Some(buffer) = *guard {
        return Some(buffer);
    }

    let bytes = align_up(UI4_COMPOSITOR_DESC_BYTES, super::WARM_ALIGN)?;
    let (phys, virt) = crate::dma::alloc(bytes, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    super::dma_flush(virt, bytes);

    // The compositor owns this descriptor allocation and its PPGTT root.
    // Layer, alpha-rectangle and video work serialize through this service.
    let buffer = GpgpuRectWorklistDescBuffer {
        phys,
        gpu: UI4_COMPOSITOR_DESC_GPU,
        virt,
        bytes,
    };
    *guard = Some(buffer);
    Some(buffer)
}
