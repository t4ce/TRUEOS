const RECT_DISPATCH_WIDTH: u32 = 64;
const RECT_DISPATCH_LEFT: u32 = 8;
const RECT_DISPATCH_BAND: u32 = 16;
const RECT_DISPATCH_BACKGROUND: u32 = 0xFF11_2233;
const RECT_DISPATCH_COLOR: u32 = 0xFFCC_8844;
const RECT_DISPATCH_PAIRS: usize = 24;
const RECT_DISPATCH_BUFFER_BYTES: usize = 8192;
// This is a SystemService PPGTT range following the existing clear-test
// allocation. The diagnostic's backing stays owned for this boot, including
// after an uncertain retirement.
const RECT_DISPATCH_GPU_BASE: u64 =
    DIRECT_RCS_GPU_VA_CLEAR_TEST_BASE + CLEAR_RECT_TEST_BYTES as u64;
const _: () = {
    assert!(RECT_DISPATCH_GPU_BASE.is_multiple_of(4096));
    assert!(
        RECT_DISPATCH_GPU_BASE + 2 * RECT_DISPATCH_BUFFER_BYTES as u64
            <= DIRECT_RCS_PPGTT_LIMIT_BYTES
    );
    assert!(RECT_DISPATCH_WIDTH as usize * 32 * 4 == RECT_DISPATCH_BUFFER_BYTES);
    assert!(RECT_DISPATCH_BAND as usize <= RECT_WORKLIST_DESCS_PER_WALKER);
};

pub(crate) struct RectDispatchReport {
    pub(crate) physical_device: u16,
    pub(crate) physical_revision: u8,
    pub(crate) effective_device: u16,
    pub(crate) timestamp_hz: u32,
    pub(crate) artifact_sha256: String,
    pub(crate) empty_lanes: u64,
    pub(crate) strips_lanes: u64,
    pub(crate) merged_lanes: u64,
    pub(crate) pairs: usize,
    pub(crate) rows: u32,
    pub(crate) strips_gpu_p50_ticks: u64,
    pub(crate) strips_gpu_p95_ticks: u64,
    pub(crate) merged_gpu_p50_ticks: u64,
    pub(crate) merged_gpu_p95_ticks: u64,
    pub(crate) strips_host_p50_us: u64,
    pub(crate) strips_host_p95_us: u64,
    pub(crate) merged_host_p50_us: u64,
    pub(crate) merged_host_p95_us: u64,
}

pub(crate) fn probe_rectangle_dispatch() -> Result<RectDispatchReport, String> {
    static RUN: Mutex<()> = Mutex::new(());
    static SCRATCH_PHYS: spin::Once<u64> = spin::Once::new();
    let _run = RUN.try_lock().ok_or("compute-dispatch-already-running")?;
    let dev = super::claimed_device().ok_or("no-device")?;
    let pci = crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 0);
    let physical_device = (pci >> 16) as u16;
    let physical_revision = crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 8) as u8;
    if pci as u16 != 0x8086
        || physical_device != super::PCI_DEVICE_ALDER_LAKE_S_GT1
        || physical_revision != 0x0C
        || (dev.device_id, dev.revision_id) != (physical_device, physical_revision)
    {
        return Err(alloc::format!(
            "unsupported-target {:04X}:{physical_device:04X} rev={physical_revision:02X}",
            pci as u16,
        ));
    }
    if !DIRECT_RCS_ENABLED || !direct_rcs_forcewake(dev) || direct_rcs_context_is_quarantined() {
        return Err("system-rcs-unavailable".into());
    }
    let upload =
        upload_alpha_blend_worklist_rgba8_kernel().ok_or("worklist-artifact-unavailable")?;
    let timestamp_hz = direct_rcs_timestamp_frequency_hz(dev);
    if timestamp_hz == 0 {
        return Err("gpu-timestamp-clock-unavailable".into());
    }
    let scratch_phys = *SCRATCH_PHYS.try_call_once(|| {
        crate::dma::alloc_ppgtt(2 * RECT_DISPATCH_BUFFER_BYTES, super::WARM_ALIGN)
            .map(|(phys, _)| phys)
            .ok_or("dispatch-scratch-allocation")
    })?;

    let sample = |backing, rows, rects: &[GpgpuSolidRect], probe| {
        sample_rectangle_dispatch(dev, upload, scratch_phys, backing, rows, rects, probe)
    };
    let (strips, merged) = rectangle_dispatch_variants(4);
    let empty_lanes = sample(
        0,
        4,
        &merged,
        RectDispatchProbe::Counter { skip_walker: true },
    )?
    .lanes()?;
    let counter = RectDispatchProbe::Counter { skip_walker: false };
    let merged_lanes = sample(1, 4, &merged, counter)?.lanes()?;
    let strips_lanes = sample(0, 4, &strips, counter)?.lanes()?;
    if (empty_lanes, strips_lanes, merged_lanes) != (0, 256, 16) {
        return Err(alloc::format!(
            "counter-calibration empty={empty_lanes}/0 strips={strips_lanes}/256 merged={merged_lanes}/16"
        ));
    }

    // The warm pair also checks whether the smallest workload resolves on the
    // timestamp clock. Both timed variants use the same extent.
    let warm_strips = sample(0, 4, &strips, RectDispatchProbe::Timing)?.gpu_ticks()?;
    let warm_merged = sample(1, 4, &merged, RectDispatchProbe::Timing)?.gpu_ticks()?;
    let rows = if warm_strips == 0 || warm_merged == 0 {
        32
    } else {
        4
    };
    let (strips, merged) = rectangle_dispatch_variants(rows);
    if rows == 32 {
        sample(0, rows, &strips, RectDispatchProbe::Timing)?.gpu_ticks()?;
        sample(1, rows, &merged, RectDispatchProbe::Timing)?.gpu_ticks()?;
    }
    let lists: [&[GpgpuSolidRect]; 2] = [&strips, &merged];
    let mut gpu_ticks = [[0; RECT_DISPATCH_PAIRS]; 2];
    let mut host_us = [[0; RECT_DISPATCH_PAIRS]; 2];
    for pair in 0..RECT_DISPATCH_PAIRS {
        let first = pair % 2;
        let swap_backing = (pair / 2) % 2;
        for variant in [first, 1 - first] {
            let measured = sample(
                variant ^ swap_backing,
                rows,
                lists[variant],
                RectDispatchProbe::Timing,
            )?;
            gpu_ticks[variant][pair] = measured.gpu_ticks()?;
            host_us[variant][pair] = measured.host_us;
        }
    }
    let [
        (strips_gpu_p50_ticks, strips_gpu_p95_ticks),
        (merged_gpu_p50_ticks, merged_gpu_p95_ticks),
    ] = gpu_ticks.map(rectangle_dispatch_percentiles);
    let [
        (strips_host_p50_us, strips_host_p95_us),
        (merged_host_p50_us, merged_host_p95_us),
    ] = host_us.map(rectangle_dispatch_percentiles);
    Ok(RectDispatchReport {
        physical_device,
        physical_revision,
        effective_device: dev.device_id,
        timestamp_hz,
        artifact_sha256: digest_hex(&upload.bin_sha256),
        empty_lanes,
        strips_lanes,
        merged_lanes,
        pairs: RECT_DISPATCH_PAIRS,
        rows,
        strips_gpu_p50_ticks,
        strips_gpu_p95_ticks,
        merged_gpu_p50_ticks,
        merged_gpu_p95_ticks,
        strips_host_p50_us,
        strips_host_p95_us,
        merged_host_p50_us,
        merged_host_p95_us,
    })
}

struct RectDispatchSample {
    before: u64,
    after: u64,
    host_us: u64,
}

impl RectDispatchSample {
    fn lanes(&self) -> Result<u64, &'static str> {
        // The command writer fills each DWORD with all ones before submission.
        for word in [self.before, self.after] {
            if word as u32 == u32::MAX || (word >> 32) as u32 == u32::MAX {
                return Err("counter-snapshot-incomplete");
            }
        }
        Ok(self.after.wrapping_sub(self.before))
    }

    fn gpu_ticks(&self) -> Result<u64, &'static str> {
        let before = self.before & DIRECT_RCS_TIMESTAMP_MASK;
        let after = self.after & DIRECT_RCS_TIMESTAMP_MASK;
        if before != 0 && before == after {
            return Ok(0);
        }
        direct_rcs_timestamp_delta_ticks(before, after).ok_or("timestamp-snapshot-invalid")
    }
}

fn sample_rectangle_dispatch(
    dev: super::Dev,
    upload: UploadedKernelArtifact,
    scratch_phys: u64,
    backing: usize,
    rows: u32,
    rects: &[GpgpuSolidRect],
    probe: RectDispatchProbe,
) -> Result<RectDispatchSample, &'static str> {
    let offset = (backing * RECT_DISPATCH_BUFFER_BYTES) as u64;
    let phys = scratch_phys + offset;
    let dst = GpgpuRgba8Surface::new(
        phys,
        RECT_DISPATCH_GPU_BASE + offset,
        RECT_DISPATCH_BUFFER_BYTES,
        RECT_DISPATCH_WIDTH,
        rows,
        RECT_DISPATCH_WIDTH * 4,
    )
    .ok_or("dispatch-surface-layout")?;
    let desc = rect_worklist_desc_buffer_once().ok_or("dispatch-descriptor-allocation")?;
    let _desc_guard = RECT_WORKLIST_DESC_SUBMIT_LOCK.lock();
    let _submit_guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    // State acquisition checks quarantine before any writes to the shared
    // batch, descriptor buffer or result page.
    let state = direct_rcs_state_once(dev).ok_or("system-rcs-state-unavailable")?;
    let output = crate::phys::phys_to_virt(phys as usize) as *mut u32;
    let words = RECT_DISPATCH_BUFFER_BYTES / core::mem::size_of::<u32>();
    unsafe { core::slice::from_raw_parts_mut(output, words) }.fill(RECT_DISPATCH_BACKGROUND);
    super::dma_flush(output.cast(), RECT_DISPATCH_BUFFER_BYTES);

    let started_ns = crate::chronos::monotonic_nanos();
    let descriptors = lower_solid_rects(dst, rects).ok_or("dispatch-descriptor-lowering")?;
    if descriptors.len() != rects.len()
        || !(1..=RECT_WORKLIST_DESCS_PER_WALKER).contains(&descriptors.len())
    {
        return Err("dispatch-descriptor-count");
    }
    let params = stage_solid_rect_worklist(desc, dst, &descriptors);
    match submit_solid_rect_worklist_locked(dev, state, upload, dst, desc, params, false, probe) {
        GpgpuSubmissionOutcome::Complete => {}
        GpgpuSubmissionOutcome::SubmittedIncomplete => return Err("dispatch-retirement-unproven"),
        GpgpuSubmissionOutcome::Unavailable => return Err("dispatch-rejected"),
    }
    let host_us = crate::chronos::monotonic_nanos().saturating_sub(started_ns) / 1_000;
    if direct_rcs_read_result_slot(state, RECT_WORKLIST_PRE_MARKER_SLOT)
        != ALPHA_BLEND_WORKLIST_PRE_MARKER
    {
        return Err("dispatch-entry-marker");
    }
    super::dma_flush(output.cast(), RECT_DISPATCH_BUFFER_BYTES);
    let empty = matches!(probe, RectDispatchProbe::Counter { skip_walker: true });
    for index in 0..words {
        let actual = unsafe { core::ptr::read_volatile(output.add(index)) };
        if actual != rectangle_dispatch_reference(index as u32, rows, empty) {
            crate::log_error!(target: "gpgpu";
                "compute-dispatch: pixel mismatch backing={} rects={} rows={} index={} actual=0x{:08X}\n",
                backing, rects.len(), rows, index, actual);
            return Err("dispatch-pixel-mismatch");
        }
    }
    let read_u64 = |slot| {
        u64::from(direct_rcs_read_result_slot(state, slot))
            | u64::from(direct_rcs_read_result_slot(state, slot + 1)) << 32
    };
    Ok(RectDispatchSample {
        before: read_u64(RECT_DISPATCH_BEFORE_SLOT),
        after: read_u64(RECT_DISPATCH_AFTER_SLOT),
        host_us,
    })
}

fn rectangle_dispatch_variants(
    rows: u32,
) -> (
    [GpgpuSolidRect; RECT_DISPATCH_BAND as usize],
    [GpgpuSolidRect; 1],
) {
    let strips = core::array::from_fn(|index| GpgpuSolidRect {
        rect: GpgpuRect::new((RECT_DISPATCH_LEFT + index as u32) as i32, 0, 1, rows),
        color_rgba: RECT_DISPATCH_COLOR,
    });
    let merged = [GpgpuSolidRect {
        rect: GpgpuRect::new(RECT_DISPATCH_LEFT as i32, 0, RECT_DISPATCH_BAND, rows),
        color_rgba: RECT_DISPATCH_COLOR,
    }];
    (strips, merged)
}

fn rectangle_dispatch_reference(index: u32, rows: u32, empty: bool) -> u32 {
    let x = index % RECT_DISPATCH_WIDTH;
    if !empty
        && index / RECT_DISPATCH_WIDTH < rows
        && (RECT_DISPATCH_LEFT..RECT_DISPATCH_LEFT + RECT_DISPATCH_BAND).contains(&x)
    {
        RECT_DISPATCH_COLOR
    } else {
        RECT_DISPATCH_BACKGROUND
    }
}

fn rectangle_dispatch_percentiles(mut samples: [u64; RECT_DISPATCH_PAIRS]) -> (u64, u64) {
    samples.sort_unstable();
    let p50 = samples[(RECT_DISPATCH_PAIRS * 50).div_ceil(100) - 1];
    let p95 = samples[(RECT_DISPATCH_PAIRS * 95).div_ceil(100) - 1];
    (p50, p95)
}
