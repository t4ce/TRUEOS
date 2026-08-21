const COPY_RECT_PROBE_CASE_COUNT: usize = 4;
const COPY_RECT_PROBE_HALF_BYTES: usize = CLEAR_RECT_TEST_BYTES / 2;
const COPY_RECT_PROBE_HALF_PIXELS: usize = COPY_RECT_PROBE_HALF_BYTES / core::mem::size_of::<u32>();
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

#[derive(Copy, Clone, Debug)]
struct CopyRectProbeCase {
    label: &'static str,
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: u32,
    dst_width: u32,
    dst_height: u32,
    dst_pitch_bytes: u32,
    src_x: u32,
    src_y: u32,
    dst_x: u32,
    dst_y: u32,
    width: u32,
    height: u32,
}

const COPY_RECT_PROBE_CASES: [CopyRectProbeCase; COPY_RECT_PROBE_CASE_COUNT] = [
    CopyRectProbeCase {
        label: "even-small",
        src_width: 27,
        src_height: 13,
        src_pitch_bytes: 128,
        dst_width: 25,
        dst_height: 13,
        dst_pitch_bytes: 112,
        src_x: 3,
        src_y: 2,
        dst_x: 5,
        dst_y: 4,
        width: 8,
        height: 3,
    },
    CopyRectProbeCase {
        label: "odd-small",
        src_width: 23,
        src_height: 14,
        src_pitch_bytes: 112,
        dst_width: 29,
        dst_height: 14,
        dst_pitch_bytes: 128,
        src_x: 4,
        src_y: 3,
        dst_x: 7,
        dst_y: 5,
        width: 7,
        height: 4,
    },
    CopyRectProbeCase {
        label: "even-multigroup",
        src_width: 48,
        src_height: 12,
        src_pitch_bytes: 208,
        dst_width: 46,
        dst_height: 12,
        dst_pitch_bytes: 192,
        src_x: 7,
        src_y: 2,
        dst_x: 5,
        dst_y: 3,
        width: 34,
        height: 2,
    },
    CopyRectProbeCase {
        label: "odd-multigroup",
        src_width: 44,
        src_height: 12,
        src_pitch_bytes: 192,
        dst_width: 45,
        dst_height: 12,
        dst_pitch_bytes: 208,
        src_x: 6,
        src_y: 3,
        dst_x: 7,
        dst_y: 4,
        width: 33,
        height: 3,
    },
];

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuCopyRectProbeCaseResult {
    pub(crate) label: &'static str,
    pub(crate) attempted: bool,
    pub(crate) submitted: bool,
    pub(crate) retired: bool,
    pub(crate) ok: bool,
    pub(crate) src_width: u32,
    pub(crate) src_height: u32,
    pub(crate) src_pitch_bytes: u32,
    pub(crate) dst_width: u32,
    pub(crate) dst_height: u32,
    pub(crate) dst_pitch_bytes: u32,
    pub(crate) src_x: u32,
    pub(crate) src_y: u32,
    pub(crate) dst_x: u32,
    pub(crate) dst_y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) copied_pixels_checked: usize,
    pub(crate) guard_pixels_checked: usize,
    pub(crate) source_pixels_checked: usize,
    pub(crate) pre_marker: u32,
    pub(crate) post_marker: u32,
    pub(crate) retire_ms: u64,
    pub(crate) first_failure: &'static str,
    pub(crate) failure_byte_offset: Option<usize>,
    pub(crate) expected: u32,
    pub(crate) observed: u32,
}

impl GpgpuCopyRectProbeCaseResult {
    const EMPTY: Self = Self {
        label: "not-configured",
        attempted: false,
        submitted: false,
        retired: false,
        ok: false,
        src_width: 0,
        src_height: 0,
        src_pitch_bytes: 0,
        dst_width: 0,
        dst_height: 0,
        dst_pitch_bytes: 0,
        src_x: 0,
        src_y: 0,
        dst_x: 0,
        dst_y: 0,
        width: 0,
        height: 0,
        copied_pixels_checked: 0,
        guard_pixels_checked: 0,
        source_pixels_checked: 0,
        pre_marker: 0,
        post_marker: 0,
        retire_ms: 0,
        first_failure: "not-run",
        failure_byte_offset: None,
        expected: 0,
        observed: 0,
    };

    const fn from_case(case: CopyRectProbeCase) -> Self {
        Self {
            label: case.label,
            src_width: case.src_width,
            src_height: case.src_height,
            src_pitch_bytes: case.src_pitch_bytes,
            dst_width: case.dst_width,
            dst_height: case.dst_height,
            dst_pitch_bytes: case.dst_pitch_bytes,
            src_x: case.src_x,
            src_y: case.src_y,
            dst_x: case.dst_x,
            dst_y: case.dst_y,
            width: case.width,
            height: case.height,
            ..Self::EMPTY
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuCopyRectProbeResult {
    pub(crate) ok: bool,
    pub(crate) reboot_required: bool,
    pub(crate) frontend: &'static str,
    pub(crate) feature: &'static str,
    pub(crate) feature_enabled: bool,
    pub(crate) artifact: &'static str,
    pub(crate) artifact_source: &'static str,
    pub(crate) artifact_target: &'static str,
    pub(crate) artifact_verified: bool,
    pub(crate) artifact_sha256: [u8; 32],
    pub(crate) pci_bus: u8,
    pub(crate) pci_slot: u8,
    pub(crate) pci_function: u8,
    pub(crate) device_id: u16,
    pub(crate) revision_id: u8,
    pub(crate) case_count: usize,
    pub(crate) attempted_cases: usize,
    pub(crate) retired_cases: usize,
    pub(crate) passed_cases: usize,
    pub(crate) first_failure_case: &'static str,
    pub(crate) first_failure: &'static str,
    pub(crate) cases: [GpgpuCopyRectProbeCaseResult; COPY_RECT_PROBE_CASE_COUNT],
}

impl GpgpuCopyRectProbeResult {
    fn new() -> Self {
        let mut cases = [GpgpuCopyRectProbeCaseResult::EMPTY; COPY_RECT_PROBE_CASE_COUNT];
        let mut index = 0;
        while index < COPY_RECT_PROBE_CASE_COUNT {
            cases[index] = GpgpuCopyRectProbeCaseResult::from_case(COPY_RECT_PROBE_CASES[index]);
            index += 1;
        }
        Self {
            ok: false,
            reboot_required: false,
            frontend: COPY_RECT_RGBA8_ARTIFACT_FRONTEND,
            feature: "cpp-for-opencl-built-in",
            feature_enabled: true,
            artifact: COPY_RECT_RGBA8_ADLS_ARTIFACT.name,
            artifact_source: "unavailable",
            artifact_target: COPY_RECT_RGBA8_ADLS_ARTIFACT.target,
            artifact_verified: false,
            artifact_sha256: COPY_RECT_RGBA8_ADLS_ARTIFACT.bin_sha256,
            pci_bus: 0,
            pci_slot: 0,
            pci_function: 0,
            device_id: 0,
            revision_id: 0,
            case_count: COPY_RECT_PROBE_CASE_COUNT,
            attempted_cases: 0,
            retired_cases: 0,
            passed_cases: 0,
            first_failure_case: "setup",
            first_failure: "none",
            cases,
        }
    }

    fn fail_setup(&mut self, failure: &'static str) {
        if self.first_failure == "none" {
            self.first_failure = failure;
        }
    }

    fn observe_case(&mut self, case: GpgpuCopyRectProbeCaseResult) {
        self.attempted_cases += case.attempted as usize;
        self.retired_cases += case.retired as usize;
        self.passed_cases += case.ok as usize;
        if self.first_failure == "none" && !case.ok {
            self.first_failure_case = case.label;
            self.first_failure = case.first_failure;
        }
    }
}

/// Exercise the selected `copy_rect_rgba8` artifact on the claimed Intel GPU.
///
/// The probe deliberately borrows the already-reserved 16 KiB direct-RCS
/// clear-test VA window and splits it into two page-aligned halves. Holding the
/// ordinary direct-RCS submit lock from CPU initialization through retirement
/// and readback ensures no batch can remap those PPGTT leaves while the probe
/// owns them. The full destination half is guard-checked, including row
/// padding and bytes beyond the logical surface.
pub(crate) fn shell_copy_rect_rgba8_probe() -> GpgpuCopyRectProbeResult {
    let mut result = GpgpuCopyRectProbeResult::new();
    if !DIRECT_RCS_ENABLED {
        result.fail_setup("direct-rcs-disabled");
        return result;
    }

    let Some(dev) = super::claimed_device() else {
        result.fail_setup("no-claimed-device");
        return result;
    };
    result.pci_bus = dev.bus;
    result.pci_slot = dev.slot;
    result.pci_function = dev.function;
    result.device_id = dev.device_id;
    result.revision_id = dev.revision_id;

    let Some(upload) = upload_copy_rect_rgba8_kernel() else {
        result.fail_setup("artifact-upload-rejected");
        return result;
    };
    result.artifact = upload.name;
    result.artifact_source = upload.source;
    result.artifact_target = upload.target;
    result.artifact_verified = upload.verified;
    result.artifact_sha256 = upload.bin_sha256;

    let _submit_guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    if direct_rcs_context_is_quarantined() {
        result.reboot_required = true;
        result.fail_setup("direct-rcs-quarantined-reboot-required");
        return result;
    }
    let Some(state) = direct_rcs_state_once(dev) else {
        result.fail_setup("direct-rcs-state-allocation");
        return result;
    };

    for (index, case) in COPY_RECT_PROBE_CASES.iter().copied().enumerate() {
        let case_result = run_copy_rect_probe_case(dev, state, upload, case, index);
        result.cases[index] = case_result;
        result.observe_case(case_result);

        // A submitted batch without a retired post marker may still own the
        // scratch and batch allocations. The poll path has quarantined shared
        // direct-RCS state until reboot; stop this invocation before rewriting
        // it for another case. Pre-submit setup failures are deterministic too.
        if !case_result.retired && !case_result.ok {
            break;
        }
    }

    result.ok = result.attempted_cases == result.case_count
        && result.retired_cases == result.case_count
        && result.passed_cases == result.case_count;
    result.reboot_required = direct_rcs_context_is_quarantined();
    if result.ok {
        result.first_failure_case = "none";
        result.first_failure = "none";
    }

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: copy-rect probe ok={} reboot_required={} frontend={} feature={} feature_enabled={} artifact={} source={} target={} verified={} device={:02X}:{:02X}.{}-0x{:04X}-r{:02X} cases={}/{} retired={} passed={} first_failure_case={} first_failure={}\n",
        result.ok as u8,
        result.reboot_required as u8,
        result.frontend,
        result.feature,
        result.feature_enabled as u8,
        result.artifact,
        result.artifact_source,
        result.artifact_target,
        result.artifact_verified as u8,
        result.pci_bus,
        result.pci_slot,
        result.pci_function,
        result.device_id,
        result.revision_id,
        result.attempted_cases,
        result.case_count,
        result.retired_cases,
        result.passed_cases,
        result.first_failure_case,
        result.first_failure,
    );
    result
}

fn run_copy_rect_probe_case(
    dev: super::Dev,
    state: DirectRcsState,
    upload: UploadedKernelArtifact,
    case: CopyRectProbeCase,
    case_index: usize,
) -> GpgpuCopyRectProbeCaseResult {
    let mut result = GpgpuCopyRectProbeCaseResult::from_case(case);
    result.attempted = true;
    result.first_failure = "none";

    let Some(src_phys) = state.clear_test_phys.checked_add(0) else {
        result.first_failure = "source-physical-overflow";
        return result;
    };
    let Some(dst_phys) = state
        .clear_test_phys
        .checked_add(COPY_RECT_PROBE_HALF_BYTES as u64)
    else {
        result.first_failure = "destination-physical-overflow";
        return result;
    };
    let Some(src) = GpgpuRgba8Surface::new(
        src_phys,
        COPY_RECT_PROBE_SRC_GPU,
        COPY_RECT_PROBE_HALF_BYTES,
        case.src_width,
        case.src_height,
        case.src_pitch_bytes,
    ) else {
        result.first_failure = "source-surface-invalid";
        return result;
    };
    let Some(dst) = GpgpuRgba8Surface::new(
        dst_phys,
        COPY_RECT_PROBE_DST_GPU,
        COPY_RECT_PROBE_HALF_BYTES,
        case.dst_width,
        case.dst_height,
        case.dst_pitch_bytes,
    ) else {
        result.first_failure = "destination-surface-invalid";
        return result;
    };
    if !copy_rect_probe_case_in_bounds(case) {
        result.first_failure = "case-out-of-bounds";
        return result;
    }

    let src_virt = state.clear_test_virt as *mut u32;
    let dst_virt = unsafe { state.clear_test_virt.add(COPY_RECT_PROBE_HALF_BYTES) as *mut u32 };
    unsafe {
        for pixel in 0..COPY_RECT_PROBE_HALF_PIXELS {
            core::ptr::write_volatile(
                src_virt.add(pixel),
                copy_rect_probe_source(pixel, case_index),
            );
            core::ptr::write_volatile(
                dst_virt.add(pixel),
                copy_rect_probe_destination_guard(pixel, case_index),
            );
        }
    }
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);

    let params = CopyRectRgba8Params {
        src_gpu: src.gpu,
        dst_gpu: dst.gpu,
        src_pitch_bytes: src.pitch_bytes,
        dst_pitch_bytes: dst.pitch_bytes,
        src_x: case.src_x,
        src_y: case.src_y,
        dst_x: case.dst_x,
        dst_y: case.dst_y,
        width: case.width,
        height: case.height,
    };
    if !direct_rcs_forcewake(dev) {
        result.first_failure = "forcewake";
        return result;
    }
    if !direct_rcs_map_state(dev, state) {
        result.first_failure = "state-map";
        return result;
    }
    if !direct_rcs_init_ppgtt(state) {
        result.first_failure = "ppgtt-init";
        return result;
    }
    if !direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes) {
        result.first_failure = "kernel-map";
        return result;
    }
    if !direct_rcs_map_ppgtt_kernel(state, src.gpu, src.phys, src.bytes) {
        result.first_failure = "source-map";
        return result;
    }
    if !direct_rcs_map_ppgtt_kernel(state, dst.gpu, dst.phys, dst.bytes) {
        result.first_failure = "destination-map";
        return result;
    }
    if !direct_rcs_encode_copy_rect_2d_batch(state, upload, params, src.bytes, dst.bytes) {
        result.first_failure = "batch-encode";
        return result;
    }

    let retire_started = direct_rcs_now_tick();
    result.submitted = direct_rcs_submit_batch(dev, state);
    if !result.submitted {
        result.first_failure = "guc-submit";
        return result;
    }
    result.post_marker = direct_rcs_poll_result_slot_timeout_ms(
        state,
        COPY_RECT_POST_MARKER_SLOT,
        COPY_RECT_POST_MARKER,
        COPY_RECT_2D_COMPLETION_TIMEOUT_MS,
    );
    result.retire_ms = direct_rcs_elapsed_ms_since(retire_started);
    result.pre_marker = direct_rcs_read_result_slot(state, COPY_RECT_PRE_MARKER_SLOT);
    result.retired = result.post_marker == COPY_RECT_POST_MARKER;
    if !result.retired {
        result.first_failure = if result.pre_marker == COPY_RECT_PRE_MARKER {
            "walker-not-retired-reboot-required"
        } else {
            "batch-not-started-reboot-required"
        };
        return result;
    }

    // CLFLUSH is the established TRUEOS DMA readback primitive: after the
    // ordered post marker it invalidates CPU cache lines before volatile reads.
    super::dma_flush(state.clear_test_virt, CLEAR_RECT_TEST_BYTES);

    let copied_pixels = (case.width as usize).saturating_mul(case.height as usize);
    result.copied_pixels_checked = copied_pixels;
    result.guard_pixels_checked = COPY_RECT_PROBE_HALF_PIXELS.saturating_sub(copied_pixels);
    result.source_pixels_checked = COPY_RECT_PROBE_HALF_PIXELS;

    for pixel in 0..COPY_RECT_PROBE_HALF_PIXELS {
        let copied_source = copy_rect_probe_copied_source_index(case, pixel);
        let expected = copied_source.map_or_else(
            || copy_rect_probe_destination_guard(pixel, case_index),
            |source| copy_rect_probe_source(source, case_index),
        );
        let observed = unsafe { core::ptr::read_volatile(dst_virt.add(pixel)) };
        if observed != expected {
            result.first_failure = if copied_source.is_some() {
                "copy-pixel-mismatch"
            } else {
                "guard-pixel-modified"
            };
            result.failure_byte_offset = Some(pixel * core::mem::size_of::<u32>());
            result.expected = expected;
            result.observed = observed;
            return result;
        }
    }
    for pixel in 0..COPY_RECT_PROBE_HALF_PIXELS {
        let expected = copy_rect_probe_source(pixel, case_index);
        let observed = unsafe { core::ptr::read_volatile(src_virt.add(pixel)) };
        if observed != expected {
            result.first_failure = "source-pixel-modified";
            result.failure_byte_offset = Some(pixel * core::mem::size_of::<u32>());
            result.expected = expected;
            result.observed = observed;
            return result;
        }
    }

    result.ok = true;
    result
}

fn copy_rect_probe_case_in_bounds(case: CopyRectProbeCase) -> bool {
    case.width != 0
        && case.height != 0
        && case
            .src_pitch_bytes
            .is_multiple_of(core::mem::size_of::<u32>() as u32)
        && case
            .dst_pitch_bytes
            .is_multiple_of(core::mem::size_of::<u32>() as u32)
        && case
            .src_x
            .checked_add(case.width)
            .is_some_and(|end| end <= case.src_width)
        && case
            .src_y
            .checked_add(case.height)
            .is_some_and(|end| end <= case.src_height)
        && case
            .dst_x
            .checked_add(case.width)
            .is_some_and(|end| end <= case.dst_width)
        && case
            .dst_y
            .checked_add(case.height)
            .is_some_and(|end| end <= case.dst_height)
}

fn copy_rect_probe_copied_source_index(
    case: CopyRectProbeCase,
    destination_index: usize,
) -> Option<usize> {
    let dst_pitch_pixels = case.dst_pitch_bytes as usize / core::mem::size_of::<u32>();
    let src_pitch_pixels = case.src_pitch_bytes as usize / core::mem::size_of::<u32>();
    let dst_y = destination_index / dst_pitch_pixels;
    let dst_x = destination_index % dst_pitch_pixels;
    let copy_x = dst_x.checked_sub(case.dst_x as usize)?;
    let copy_y = dst_y.checked_sub(case.dst_y as usize)?;
    if copy_x >= case.width as usize || copy_y >= case.height as usize {
        return None;
    }
    (case.src_y as usize)
        .checked_add(copy_y)?
        .checked_mul(src_pitch_pixels)?
        .checked_add(case.src_x as usize)?
        .checked_add(copy_x)
        .filter(|index| *index < COPY_RECT_PROBE_HALF_PIXELS)
}

fn copy_rect_probe_source(pixel: usize, case_index: usize) -> u32 {
    0x5100_0000u32
        ^ (case_index as u32).wrapping_mul(0x0110_0001)
        ^ (pixel as u32).wrapping_mul(0x0001_0101)
}

fn copy_rect_probe_destination_guard(pixel: usize, case_index: usize) -> u32 {
    0xA700_0000u32
        ^ (case_index as u32).wrapping_mul(0x0011_1001)
        ^ (pixel as u32).wrapping_mul(0x0001_0001)
}

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
    if direct_rcs_context_is_quarantined() {
        return false;
    }
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
    let submitted =
        submit_fill_rect_worklist(surface, desc, params, false) == GpgpuSubmissionOutcome::Complete;
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
    // Readiness may be queried by the synchronous display fallback before
    // Intel device claim completes. That is a transient ordering condition,
    // not a completed probe: consuming the one-shot here permanently left
    // later Helio/UI4 users at `gpu-unavailable` even after the device existed.
    let Some(dev) = super::claimed_device() else {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: sprite-quad-worklist-rgba8 deferred reason=no-claimed-device probe_consumed=0 retryable=1\n"
        );
        return false;
    };
    if !force && SPRITE_QUAD_WORKLIST_RAN.swap(true, Ordering::AcqRel) {
        crate::log_debug!(
            target: "gpgpu";
            "intel/gpgpu: sprite-quad-worklist-rgba8 readiness cached ran=1 ok={} quarantined={}\n",
            SPRITE_QUAD_WORKLIST_OK.load(Ordering::Acquire) as u8,
            direct_rcs_context_is_quarantined() as u8,
        );
        return false;
    }
    SPRITE_QUAD_WORKLIST_RAN.store(true, Ordering::Release);
    SPRITE_QUAD_WORKLIST_OK.store(false, Ordering::Release);

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
    if direct_rcs_context_is_quarantined() {
        crate::log_warn!(
            target: "gpgpu";
            "intel/gpgpu: sprite-quad-worklist-rgba8 failed rung=submit-lock reason=system-service-lane-quarantined\n"
        );
        return false;
    }
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
    let pre_ok = pre_marker == SPRITE_QUAD_WORKLIST_PRE_MARKER;
    let post_ok = post_marker == SPRITE_QUAD_WORKLIST_POST_MARKER;
    let pixels_ok = row1[0] == src00 && row1[1] == src01 && row2[0] == src10 && row2[1] == src11;
    let ok = submitted && pre_ok && post_ok && pixels_ok;

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: sprite-quad-worklist-rgba8 forcewake=1 ggtt=1 ppgtt=1 kernel_ppgtt=1 src_ppgtt=1 dst_ppgtt=1 desc_ppgtt=1 batch=1 submitted={} ok={} pre_ok={} post_ok={} pixels_ok={} submit_ms={} descs=1 walkers={} pre_marker=0x{:08X} expected_pre=0x{:08X} post_marker=0x{:08X} expected_post=0x{:08X} kernel_gpu=0x{:X} kernel_text_gpu=0x{:X} src_gpu=0x{:X} dst_gpu=0x{:X} desc_gpu=0x{:X} row1=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] row2=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] artifact={}\n",
        submitted as u8,
        ok as u8,
        pre_ok as u8,
        post_ok as u8,
        pixels_ok as u8,
        submit_ms,
        sprite_quad_worklist_walker_count(1),
        pre_marker,
        SPRITE_QUAD_WORKLIST_PRE_MARKER,
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
