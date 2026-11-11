const SPIRIT_VFX_INITIAL_TRACE_FRAMES: u32 = 30;
const SPIRIT_VFX_PERIODIC_TRACE_FRAMES: u32 = 60;
const SPIRIT_VFX_ENCODING_LOG_LIMIT: u32 = 2;
/// One invariant footprint for every Spirit procedural background. Keeping
/// this at the control-page writer makes scale non-negotiable for all callers.
pub(crate) const SPIRIT_VFX_BACKGROUND_PRESENT_SCALE: f32 = 1.171875;

#[derive(Copy, Clone, Debug)]
pub(crate) struct SpiritVfxControl {
    pub(crate) revision: u64,
    pub(crate) background_mode: u32,
    pub(crate) clock_seconds_of_day: u32,
    pub(crate) background_phase_override: Option<f32>,
    pub(crate) background_opacity: f32,
    pub(crate) background_speed: f32,
    pub(crate) background_intensity: f32,
    pub(crate) background_color_a: u32,
    pub(crate) background_color_b: u32,
    pub(crate) position_x: f32,
    pub(crate) position_y: f32,
    pub(crate) rotation_radians: f32,
    pub(crate) alpha_cutoff: f32,
    pub(crate) edge_fade_pixels: f32,
    pub(crate) sampling: u32,
    pub(crate) shader_mode: u32,
    pub(crate) shader_parameters: [f32; 4],
    pub(crate) fx_color_a: u32,
    pub(crate) fx_color_b: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SpiritVfxSubmission {
    tag: u64,
    frame: u32,
}

impl SpiritVfxSubmission {
    pub(crate) const fn tag(self) -> u64 {
        self.tag
    }

    pub(crate) const fn frame(self) -> u32 {
        self.frame
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum SpiritVfxCompletion {
    Pending,
    Complete(GpgpuRgba8ReleaseFence),
    Failed,
    InvalidSubmission,
}

#[derive(Copy, Clone)]
struct SpiritVfxBuffer {
    phys: u64,
    gpu: u64,
    virt: *mut u8,
    bytes: usize,
}

unsafe impl Send for SpiritVfxBuffer {}
unsafe impl Sync for SpiritVfxBuffer {}

#[derive(Copy, Clone)]
struct SpiritVfxRuntime {
    control: SpiritVfxBuffer,
}

unsafe impl Send for SpiritVfxRuntime {}
unsafe impl Sync for SpiritVfxRuntime {}

#[derive(Copy, Clone)]
struct SpiritVfxSubmitted {
    state: DirectRcsState,
    first_kernel: UploadedKernelArtifact,
    control: SpiritVfxBuffer,
    source: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    submit_ambiguous: bool,
    frame: u32,
    revision: u64,
    background_mode: u32,
    shader_mode: u32,
    started_tick: u64,
    encoding_evidence_logged: bool,
}

#[derive(Copy, Clone)]
struct SpiritVfxPending {
    handle: SpiritVfxSubmission,
    submitted: SpiritVfxSubmitted,
    timeout_logged: bool,
}

#[derive(Copy, Clone, Debug)]
struct SpiritVfxPpgttEvidence {
    entries: [u64; 4],
    read_levels: u8,
    topology_match: bool,
    leaf_match: bool,
}

#[derive(Copy, Clone, Debug)]
struct SpiritVfxEncodingEvidence {
    ring_before: u32,
    ring_after: u32,
    pdp_hi_before: u32,
    pdp_lo: u32,
    pdp_hi_after: u32,
    preemption_20e4: u32,
    ff_slice_cs_chicken1: u32,
    l3alloc_b134: u32,
    global_mocs4_4010: u32,
    l3cc_pair_b028: u32,
    cs_debug_mode2_20d8: u32,
    background_idd: Option<[u32; 8]>,
    background_cross_thread: Option<[u32; 16]>,
    background_local_ids: Option<[u32; 24]>,
    sprite_idd: Option<[u32; 8]>,
    sprite_cross_thread: Option<[u32; 24]>,
    sprite_local_ids: Option<[u32; 24]>,
    control: Option<[u32; SPIRIT_VFX_CONTROL_DWORDS]>,
}

/// Read fixed words within a retained CPU allocation. In particular, payload
/// pointers are observations only and are never followed to another address.
fn spirit_vfx_owned_dwords<const N: usize>(
    virt: *mut u8,
    bytes: usize,
    offset: usize,
) -> Option<[u32; N]> {
    let address = (virt as usize).checked_add(offset)?;
    let end = offset.checked_add(N.checked_mul(core::mem::size_of::<u32>())?)?;
    if virt.is_null()
        || (virt as usize).checked_add(bytes).is_none()
        || !address.is_multiple_of(core::mem::align_of::<u32>())
        || end > bytes
    {
        return None;
    }
    // The encoder already flushed its writes. Invalidate these immutable
    // ranges again so the timeout snapshot does not reuse the first capture's
    // clean CPU cache lines if memory changed while the job was pending.
    super::dma_flush(unsafe { virt.add(offset) }, end - offset);
    Some(core::array::from_fn(|index| unsafe {
        core::ptr::read_volatile(virt.add(offset).cast::<u32>().add(index))
    }))
}

fn spirit_vfx_capture_encoding_evidence(
    dev: super::Dev,
    state: DirectRcsState,
    control: SpiritVfxBuffer,
    background: bool,
) -> Option<SpiritVfxEncodingEvidence> {
    if !crate::intel::render::tgl_spirit_compute_contract() {
        return None;
    }
    let ring_before = super::mmio_read(dev, RCS_RING_START);
    let pdp_hi_before = super::mmio_read(dev, 0x2274);
    let pdp_lo = super::mmio_read(dev, 0x2270);
    let pdp_hi_after = super::mmio_read(dev, 0x2274);
    let preemption_20e4 = super::mmio_read(dev, 0x20E4);
    let ff_slice_cs_chicken1 = super::mmio_read(dev, RCS_RING_BASE + 0xE0);
    let l3alloc_b134 = super::mmio_read(dev, 0xB134);
    let global_mocs4_4010 = super::mmio_read(dev, 0x4010);
    let l3cc_pair_b028 = super::mmio_read(dev, 0xB028);
    let cs_debug_mode2_20d8 = super::mmio_read(dev, 0x20D8);
    let batch = state.batch_virt;
    let background_idd = if background {
        spirit_vfx_owned_dwords(batch, DIRECT_RCS_BATCH_BYTES, SPIRIT_VFX_BACKGROUND_IDD_OFFSET_BYTES)
    } else { None };
    let background_cross_thread = if background {
        spirit_vfx_owned_dwords(batch, DIRECT_RCS_BATCH_BYTES, SPIRIT_VFX_BACKGROUND_PAYLOAD_OFFSET_BYTES)
    } else { None };
    let background_local_ids = if background {
        spirit_vfx_owned_dwords(batch, DIRECT_RCS_BATCH_BYTES,
            SPIRIT_VFX_BACKGROUND_PAYLOAD_OFFSET_BYTES + SPIRIT_VFX_BACKGROUND_CROSS_THREAD_BYTES)
    } else { None };
    let sprite_idd = spirit_vfx_owned_dwords(
        batch, DIRECT_RCS_BATCH_BYTES, SPIRIT_VFX_SPRITE_IDD_OFFSET_BYTES);
    let sprite_cross_thread = spirit_vfx_owned_dwords(
        batch, DIRECT_RCS_BATCH_BYTES, SPIRIT_VFX_SPRITE_PAYLOAD_OFFSET_BYTES);
    let sprite_local_ids = spirit_vfx_owned_dwords(batch, DIRECT_RCS_BATCH_BYTES,
        SPIRIT_VFX_SPRITE_PAYLOAD_OFFSET_BYTES + SPIRIT_VFX_SPRITE_CROSS_THREAD_BYTES);
    let control = spirit_vfx_owned_dwords(control.virt, control.bytes, 0);
    let ring_after = super::mmio_read(dev, RCS_RING_START);
    Some(SpiritVfxEncodingEvidence {
        ring_before, ring_after, pdp_hi_before, pdp_lo, pdp_hi_after,
        preemption_20e4, ff_slice_cs_chicken1, l3alloc_b134,
        global_mocs4_4010, l3cc_pair_b028, cs_debug_mode2_20d8,
        background_idd, background_cross_thread, background_local_ids,
        sprite_idd, sprite_cross_thread, sprite_local_ids, control,
    })
}

fn spirit_vfx_log_encoding_evidence(
    tag: u64,
    submitted: SpiritVfxSubmitted,
    phase: &'static str,
    evidence: SpiritVfxEncodingEvidence,
) {
    let state = submitted.state;
    let pdp0 = (u64::from(evidence.pdp_hi_after) << 32) | u64::from(evidence.pdp_lo);
    let pdp_stable = evidence.pdp_hi_before == evidence.pdp_hi_after;
    let spirit_ring = u64::from(evidence.ring_before) == state.gpu_va.ring
        && evidence.ring_after == evidence.ring_before;
    let root_matches = (spirit_ring && pdp_stable).then_some(pdp0 == state.ppgtt_phys);
    crate::log_info!(target: "gpgpu";
        "intel/gpgpu: spirit-vfx encoding-job tag={} frame={} phase={} background_present={} revision={} shader={} root=0x{:X} context_phys=0x{:X} context_gpu=0x{:X} batch_phys=0x{:X} batch_gpu=0x{:X} control_phys=0x{:X} control_gpu=0x{:X} first_kernel_gpu=0x{:X} source_gpu=0x{:X} dst_gpu=0x{:X} scope=owned-cpu-readback cache=writeback-invalidate-before-read\n",
        tag, submitted.frame, phase,
        submitted.first_kernel.gpu == SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_GPU,
        submitted.revision, submitted.shader_mode, state.ppgtt_phys,
        state.context_phys, state.gpu_va.context, state.batch_phys, state.gpu_va.batch,
        submitted.control.phys, submitted.control.gpu, submitted.first_kernel.gpu,
        submitted.source.gpu, submitted.dst.gpu,
    );
    crate::log_info!(target: "gpgpu";
        "intel/gpgpu: spirit-vfx encoding-live tag={} phase={} pdp0=0x{:016X} pdp_hi_before=0x{:08X} pdp_stable={} ring_before=0x{:08X} ring_after=0x{:08X} expected_ring=0x{:X} spirit_ring={} root_matches={:?} preemption_20e4=0x{:08X} ff_slice_cs_chicken1=0x{:08X} l3alloc_b134=0x{:08X} global_mocs4_4010=0x{:08X} l3cc_pair_b028=0x{:08X} cs_debug_mode2_20d8=0x{:08X} capture=observational\n",
        tag, phase, pdp0, evidence.pdp_hi_before, pdp_stable,
        evidence.ring_before, evidence.ring_after, state.gpu_va.ring, spirit_ring,
        root_matches, evidence.preemption_20e4, evidence.ff_slice_cs_chicken1,
        evidence.l3alloc_b134, evidence.global_mocs4_4010,
        evidence.l3cc_pair_b028, evidence.cs_debug_mode2_20d8,
    );
    crate::log_info!(target: "gpgpu";
        "intel/gpgpu: spirit-vfx encoding-owned tag={} phase={} resource=background-idd offset=0x{:X} dwords={:08X?}\n",
        tag, phase, SPIRIT_VFX_BACKGROUND_IDD_OFFSET_BYTES, evidence.background_idd);
    crate::log_info!(target: "gpgpu";
        "intel/gpgpu: spirit-vfx encoding-owned tag={} phase={} resource=background-cross-thread offset=0x{:X} dwords={:08X?}\n",
        tag, phase, SPIRIT_VFX_BACKGROUND_PAYLOAD_OFFSET_BYTES, evidence.background_cross_thread);
    crate::log_info!(target: "gpgpu";
        "intel/gpgpu: spirit-vfx encoding-owned tag={} phase={} resource=background-local-ids offset=0x{:X} dwords={:08X?}\n",
        tag, phase, SPIRIT_VFX_BACKGROUND_PAYLOAD_OFFSET_BYTES + SPIRIT_VFX_BACKGROUND_CROSS_THREAD_BYTES,
        evidence.background_local_ids);
    crate::log_info!(target: "gpgpu";
        "intel/gpgpu: spirit-vfx encoding-owned tag={} phase={} resource=sprite-idd offset=0x{:X} dwords={:08X?}\n",
        tag, phase, SPIRIT_VFX_SPRITE_IDD_OFFSET_BYTES, evidence.sprite_idd);
    crate::log_info!(target: "gpgpu";
        "intel/gpgpu: spirit-vfx encoding-owned tag={} phase={} resource=sprite-cross-thread offset=0x{:X} dwords={:08X?}\n",
        tag, phase, SPIRIT_VFX_SPRITE_PAYLOAD_OFFSET_BYTES, evidence.sprite_cross_thread);
    crate::log_info!(target: "gpgpu";
        "intel/gpgpu: spirit-vfx encoding-owned tag={} phase={} resource=sprite-local-ids offset=0x{:X} dwords={:08X?}\n",
        tag, phase, SPIRIT_VFX_SPRITE_PAYLOAD_OFFSET_BYTES + SPIRIT_VFX_SPRITE_CROSS_THREAD_BYTES,
        evidence.sprite_local_ids);
    crate::log_info!(target: "gpgpu";
        "intel/gpgpu: spirit-vfx encoding-owned tag={} phase={} resource=control offset=0x0 dwords={:08X?} layout=preview-v1-plus-clock\n",
        tag, phase, evidence.control);
}

/// Inspect the retained dense PPGTT through its owned CPU allocation only.
/// A corrupt parent is evidence, never permission to dereference its address.
fn spirit_vfx_ppgtt_evidence(
    state: DirectRcsState,
    gpu: u64,
    expected_phys: u64,
    expected_flags: u64,
) -> Option<SpiritVfxPpgttEvidence> {
    if state.ppgtt_virt.is_null()
        || !(state.ppgtt_virt as usize).is_multiple_of(core::mem::align_of::<u64>())
        || (state.ppgtt_virt as usize).checked_add(DIRECT_RCS_PPGTT_BYTES).is_none()
        || state.ppgtt_phys == 0
        || !state.ppgtt_phys.is_multiple_of(4096)
        || state.ppgtt_phys.checked_add(DIRECT_RCS_PPGTT_BYTES as u64).is_none()
        || gpu >= DIRECT_RCS_PPGTT_LIMIT_BYTES
        || gpu & 0xFFF != expected_phys & 0xFFF
    {
        return None;
    }
    let pd_index = usize::try_from(gpu >> 21).ok()?;
    let pdpt_index = usize::try_from(gpu >> 30).ok()?;
    let leaf_offset = exact_ppgtt_pte_offset(gpu & !0xFFF, 0)?;
    let offsets = [
        DIRECT_RCS_PPGTT_PML4_OFFSET,
        DIRECT_RCS_PPGTT_PDPT_OFFSET.checked_add(pdpt_index.checked_mul(8)?)?,
        DIRECT_RCS_PPGTT_PD_OFFSET.checked_add(pd_index.checked_mul(8)?)?,
        leaf_offset,
    ];
    if offsets.iter().any(|&offset| {
        offset.checked_add(core::mem::size_of::<u64>())
            .is_none_or(|end| end > DIRECT_RCS_PPGTT_BYTES)
    }) {
        return None;
    }
    let child_offsets = [
        DIRECT_RCS_PPGTT_PDPT_OFFSET,
        DIRECT_RCS_PPGTT_PD_OFFSET.checked_add(pdpt_index.checked_mul(4096)?)?,
        DIRECT_RCS_PPGTT_PT_OFFSET.checked_add(pd_index.checked_mul(4096)?)?,
    ];
    let mut evidence = SpiritVfxPpgttEvidence {
        entries: [0; 4], read_levels: 0, topology_match: false, leaf_match: false,
    };
    for (level, offset) in offsets.into_iter().enumerate() {
        let raw = unsafe {
            core::ptr::read_volatile(state.ppgtt_virt.add(offset).cast::<u64>())
        };
        evidence.entries[level] = raw;
        evidence.read_levels += 1;
        if level < 3 {
            let child_phys = state.ppgtt_phys.checked_add(child_offsets[level] as u64)?;
            // Ignore hardware Accessed bits, but require present, no large
            // page, and the exact next table inside this allocation.
            if raw & !0xFFF != child_phys || raw & 0x81 != 1 {
                return Some(evidence);
            }
        } else {
            evidence.topology_match = true;
            // Gen12 PAT2 occupies leaf bit7 (PDE bit7 means large page).
            // https://github.com/torvalds/linux/blob/v6.14/drivers/gpu/drm/i915/gt/intel_gtt.h#L85
            let policy = super::GEN8_PAGE_PRESENT | GEN8_PAGE_RW | GEN8_PAGE_PWT | GEN8_PAGE_PCD
                | (1 << 7);
            evidence.leaf_match = raw & !0xFFF == expected_phys & !0xFFF
                && raw & policy == expected_flags & policy;
        }
    }
    Some(evidence)
}

fn spirit_vfx_log_mapping_evidence(dev: super::Dev, submitted: SpiritVfxSubmitted) {
    // TGL PRM Vol2c, PDP0 p488: RCS 0x2270..0x2277 contains the PML4
    // pointer in legacy 64-bit mode. Vol9's preemption table identifies
    // 0x20E4 bits2:1; Vol2c L3ALLOCREG p1264 identifies 0xB134.
    const PDP0_LO: usize = 0x2270;
    const PDP0_HI: usize = 0x2274;
    let state = submitted.state;
    let ring_before = super::mmio_read(dev, RCS_RING_START);
    let pdp_hi_before = super::mmio_read(dev, PDP0_HI);
    let pdp_lo = super::mmio_read(dev, PDP0_LO);
    let pdp_hi_after = super::mmio_read(dev, PDP0_HI);
    let preemption_20e4 = super::mmio_read(dev, 0x20E4);
    let l3alloc_b134 = super::mmio_read(dev, 0xB134);
    let ring_after = super::mmio_read(dev, RCS_RING_START);
    let pdp0 = (u64::from(pdp_hi_after) << 32) | u64::from(pdp_lo);
    let pdp_stable = pdp_hi_before == pdp_hi_after;
    let spirit_ring = u64::from(ring_before) == state.gpu_va.ring && ring_after == ring_before;
    let root_matches = (spirit_ring && pdp_stable)
        .then_some(pdp0 == state.ppgtt_phys);
    crate::log_error!(target: "gpgpu";
        "intel/gpgpu: spirit-vfx timeout-mapping root=0x{:X} pdp0=0x{:016X} pdp_hi_before=0x{:08X} pdp_stable={} ring_before=0x{:08X} ring_after=0x{:08X} expected_ring=0x{:X} spirit_ring={} root_matches={:?} preemption_20e4=0x{:08X} l3alloc_b134=0x{:08X} capture=observational-before-context-disable\n",
        state.ppgtt_phys, pdp0, pdp_hi_before, pdp_stable, ring_before, ring_after,
        state.gpu_va.ring, spirit_ring, root_matches,
        preemption_20e4, l3alloc_b134,
    );
    let background = submitted.first_kernel.gpu == SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_GPU;
    let idd_offset = if background { SPIRIT_VFX_BACKGROUND_IDD_OFFSET_BYTES }
        else { SPIRIT_VFX_SPRITE_IDD_OFFSET_BYTES };
    let payload_offset = if background { SPIRIT_VFX_BACKGROUND_PAYLOAD_OFFSET_BYTES }
        else { SPIRIT_VFX_SPRITE_PAYLOAD_OFFSET_BYTES };
    let wb = super::GEN8_PAGE_PRESENT | GEN8_PAGE_RW;
    for (name, gpu, phys, flags) in [
        ("first-kernel", submitted.first_kernel.gpu, submitted.first_kernel.phys, wb),
        ("first-idd", state.gpu_va.batch + idd_offset as u64, state.batch_phys + idd_offset as u64, wb),
        ("first-payload", state.gpu_va.batch + payload_offset as u64, state.batch_phys + payload_offset as u64, wb),
        ("control", submitted.control.gpu, submitted.control.phys, wb),
        ("destination", submitted.dst.gpu, submitted.dst.phys, wb | GEN8_PAGE_PWT | GEN8_PAGE_PCD),
    ] {
        let evidence = spirit_vfx_ppgtt_evidence(state, gpu, phys, flags);
        crate::log_error!(target: "gpgpu";
            "intel/gpgpu: spirit-vfx timeout-pte resource={} gpu=0x{:X} expected_phys=0x{:X} expected_flags=0x{:X} evidence={:X?} scope=owned-ppgtt-only raw_entries=pml4,pdp,pde,pte unread-levels=zero\n",
            name, gpu, phys, flags, evidence,
        );
    }
}

static SPIRIT_VFX_RUNTIME: Mutex<Option<SpiritVfxRuntime>> = Mutex::new(None);
static SPIRIT_VFX_PENDING: Mutex<Option<SpiritVfxPending>> = Mutex::new(None);
static SPIRIT_VFX_NEXT_TAG: AtomicU64 = AtomicU64::new(1);
static SPIRIT_VFX_NEXT_FRAME: AtomicU32 = AtomicU32::new(0);
static SPIRIT_VFX_ENCODING_LOGS: AtomicU32 = AtomicU32::new(0);
static SPIRIT_VFX_SAVED_CONTEXT_LOGS: AtomicU32 = AtomicU32::new(0);

fn spirit_vfx_runtime_once() -> Option<SpiritVfxRuntime> {
    if let Some(runtime) = *SPIRIT_VFX_RUNTIME.lock() {
        return Some(runtime);
    }
    let (phys, virt) = crate::dma::alloc(SPIRIT_VFX_PAGE_BYTES, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(virt, 0, SPIRIT_VFX_PAGE_BYTES);
    }
    super::dma_flush(virt, SPIRIT_VFX_PAGE_BYTES);
    let runtime = SpiritVfxRuntime {
        control: SpiritVfxBuffer {
            phys,
            gpu: SPIRIT_VFX_CONTROL_GPU,
            virt,
            bytes: SPIRIT_VFX_PAGE_BYTES,
        },
    };
    *SPIRIT_VFX_RUNTIME.lock() = Some(runtime);
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: spirit-vfx control ready phys=0x{:X} gpu=0x{:X} bytes=0x{:X} contract=preview-v1 bounded=1\n",
        runtime.control.phys,
        runtime.control.gpu,
        runtime.control.bytes,
    );
    Some(runtime)
}

fn spirit_vfx_bounded(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

fn spirit_vfx_shader_defaults(mode: u32) -> [f32; 4] {
    match mode {
        1 => [12.0, 1.15, 1.2, 0.18],
        2 => [3.2, 1.35, 1.1, 0.12],
        3 => [3.1, 16.0, 1.7, 1.25],
        4 => [3.4, 1.2, 10.0, 0.28],
        5 => [95.0, 3.5, 1.4, 0.82],
        6 => [2.7, 8.0, 2.8, 0.36],
        7 => [0.42, 0.08, 9.5, 1.45],
        8 => [11.0, 0.8, 1.2, 0.68],
        9 => [4.1, 14.0, 2.4, 1.65],
        10 => [0.55, 5.5, 0.58, 2.2],
        11 => [0.82, 2.2, 4.5, 1.5],
        12 => [3.2, 11.0, 1.7, 7.0],
        13 => [6.0, 1.7, 1.18, 1.2],
        14 => [4.3, 7.5, 1.1, 1.6],
        15 => [13.0, 0.9, 0.65, 0.28],
        _ => [0.0; 4],
    }
}

fn spirit_vfx_write_control(
    control_buffer: SpiritVfxBuffer,
    frame: u32,
    dst_pitch_bytes: u32,
    source: GpgpuRgba8Surface,
    control: SpiritVfxControl,
    present_fps: u32,
) {
    let animation_time = frame as f32 * (1.0 / 60.0);
    let background_speed = match control.background_phase_override {
        Some(phase) if animation_time > 0.0 => phase / animation_time,
        Some(_) => 0.0,
        None => control.background_speed,
    };
    unsafe {
        core::ptr::write_bytes(control_buffer.virt, 0, control_buffer.bytes);
        let dwords = control_buffer.virt as *mut u32;
        core::ptr::write_volatile(dwords, SPIRIT_VFX_CONTROL_MAGIC);
        core::ptr::write_volatile(dwords.add(1), SPIRIT_VFX_CONTROL_VERSION);
        core::ptr::write_volatile(dwords.add(2), frame);
        core::ptr::write_volatile(
            dwords.add(3),
            matches!(control.background_mode, 2..=11)
                .then_some(control.background_mode)
                .unwrap_or(0),
        );
        // Keep the original smooth animation clock for the sprite walker and
        // every non-clock background. A changing-speed background supplies a
        // phase-equivalent coefficient in dword 7, preserving the stable
        // control ABI. MagicTimeCircle reads its quantized UTC clock from the
        // append-only dword 32 instead.
        core::ptr::write_volatile(dwords.add(4), animation_time.to_bits());
        core::ptr::write_volatile(
            dwords.add(5),
            spirit_vfx_bounded(control.background_opacity, 0.0, 1.0, 0.0).to_bits(),
        );
        core::ptr::write_volatile(dwords.add(6), SPIRIT_VFX_BACKGROUND_PRESENT_SCALE.to_bits());
        core::ptr::write_volatile(
            dwords.add(7),
            spirit_vfx_bounded(background_speed, 0.0, 4.0, 1.0).to_bits(),
        );
        core::ptr::write_volatile(
            dwords.add(8),
            spirit_vfx_bounded(control.background_intensity, 0.1, 4.0, 2.0).to_bits(),
        );
        core::ptr::write_volatile(dwords.add(9), control.background_color_a & 0x00FF_FFFF);
        core::ptr::write_volatile(dwords.add(10), control.background_color_b & 0x00FF_FFFF);
        core::ptr::write_volatile(
            dwords.add(11),
            spirit_vfx_bounded(control.position_x, -0.35, 0.35, 0.0).to_bits(),
        );
        core::ptr::write_volatile(
            dwords.add(12),
            spirit_vfx_bounded(control.position_y, -0.35, 0.35, 0.0).to_bits(),
        );
        // Spirit's Lilly presentation scale is architectural, not a mutable
        // VFX parameter. Keep dword 13 for the kernel ABI, but lock its value.
        core::ptr::write_volatile(dwords.add(13), 0.65f32.to_bits());
        core::ptr::write_volatile(
            dwords.add(14),
            spirit_vfx_bounded(
                control.rotation_radians,
                -core::f32::consts::TAU,
                core::f32::consts::TAU,
                core::f32::consts::PI,
            )
            .to_bits(),
        );
        core::ptr::write_volatile(
            dwords.add(15),
            spirit_vfx_bounded(control.alpha_cutoff, 0.0, 0.3, 0.02).to_bits(),
        );
        core::ptr::write_volatile(dwords.add(16), control.sampling.min(1));
        let shader_mode = matches!(control.shader_mode, 0..=15)
            .then_some(control.shader_mode)
            .unwrap_or(0);
        core::ptr::write_volatile(dwords.add(17), shader_mode);
        let shader_defaults = spirit_vfx_shader_defaults(shader_mode);
        for (index, value) in control.shader_parameters.iter().copied().enumerate() {
            core::ptr::write_volatile(
                dwords.add(18 + index),
                spirit_vfx_bounded(value, -128.0, 256.0, shader_defaults[index]).to_bits(),
            );
        }
        core::ptr::write_volatile(dwords.add(22), control.fx_color_a & 0x00FF_FFFF);
        core::ptr::write_volatile(dwords.add(23), control.fx_color_b & 0x00FF_FFFF);
        core::ptr::write_volatile(dwords.add(24), source.width);
        core::ptr::write_volatile(dwords.add(25), source.height);
        core::ptr::write_volatile(dwords.add(26), source.pitch_bytes);
        core::ptr::write_volatile(dwords.add(27), dst_pitch_bytes);
        core::ptr::write_volatile(dwords.add(28), control.revision as u32);
        core::ptr::write_volatile(dwords.add(29), (control.revision >> 32) as u32);
        core::ptr::write_volatile(dwords.add(30), present_fps.min(1_000));
        core::ptr::write_volatile(
            dwords.add(31),
            spirit_vfx_bounded(control.edge_fade_pixels, 0.0, 16.0, 12.0).to_bits(),
        );
        core::ptr::write_volatile(
            dwords.add(32),
            (control.clock_seconds_of_day.min(86_399) as f32).to_bits(),
        );
    }
    super::dma_flush(control_buffer.virt, control_buffer.bytes);
}

pub(crate) fn submit_spirit_vfx_frame(
    dst: GpgpuRgba8Surface,
    source: GpgpuRgba8Surface,
    control: SpiritVfxControl,
    present_fps: u32,
) -> Option<SpiritVfxSubmission> {
    let _submit_guard = EXECUTION_RCS_SUBMIT_LOCK.try_lock()?;
    if EXECUTION_RCS_DETACHED_TAG.load(Ordering::Acquire) != 0
        || SPIRIT_VFX_PENDING.lock().is_some()
    {
        return None;
    }
    let (mut submitted, encoding_evidence) =
        submit_spirit_vfx_batch(dst, source, control, present_fps)?;
    let tag = SPIRIT_VFX_NEXT_TAG
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1)
        .max(1);
    let handle = SpiritVfxSubmission {
        tag,
        frame: submitted.frame,
    };
    // Capture precedes publication; log only after accepted or ambiguous
    // admission, when this exact job has a tag. Busy/rejected attempts do not
    // consume either initial capture slot. The submit lock serializes
    // admission and the bounded counter update.
    if let Some(evidence) = encoding_evidence {
        if SPIRIT_VFX_ENCODING_LOGS.fetch_update(
            Ordering::AcqRel, Ordering::Acquire,
            |count| (count < SPIRIT_VFX_ENCODING_LOG_LIMIT).then_some(count + 1),
        ).is_ok() {
            submitted.encoding_evidence_logged = true;
            spirit_vfx_log_encoding_evidence(tag, submitted, "before-commit", evidence);
        }
    }
    EXECUTION_RCS_DETACHED_TAG.store(tag, Ordering::Release);
    *SPIRIT_VFX_PENDING.lock() = Some(SpiritVfxPending {
        handle,
        submitted,
        timeout_logged: false,
    });
    if submitted.submit_ambiguous || spirit_vfx_trace_frame(submitted.frame) {
        let walkers = 1 + u32::from(submitted.background_mode != 0);
        let dependency = if submitted.background_mode == 0 {
            "none/clean-lilly"
        } else {
            "hdc-flush+invalidate"
        };
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: spirit-vfx accepted tag={} frame={} revision={} background={} shader={} src={}x{}@0x{:X} dst=0x{:X} walkers={} dependency={} owner=spirit-worker admission={} wait=detached producer_lease=retained\n",
            tag,
            submitted.frame,
            submitted.revision,
            submitted.background_mode,
            submitted.shader_mode,
            submitted.source.width,
            submitted.source.height,
            submitted.source.gpu,
            submitted.dst.gpu,
            walkers,
            dependency,
            if submitted.submit_ambiguous {
                "ambiguous-quarantined"
            } else {
                "guc"
            },
        );
    }
    Some(handle)
}

pub(crate) fn poll_spirit_vfx_submission(handle: SpiritVfxSubmission) -> SpiritVfxCompletion {
    if EXECUTION_RCS_DETACHED_TAG.load(Ordering::Acquire) != handle.tag {
        return SpiritVfxCompletion::InvalidSubmission;
    }
    let mut pending_slot = SPIRIT_VFX_PENDING.lock();
    let Some(mut pending) = *pending_slot else {
        return SpiritVfxCompletion::InvalidSubmission;
    };
    if pending.handle != handle {
        return SpiritVfxCompletion::InvalidSubmission;
    }
    let marker = direct_rcs_read_result_slot(pending.submitted.state, SPIRIT_VFX_POST_MARKER_SLOT);
    let elapsed_ms = direct_rcs_elapsed_ms_since(pending.submitted.started_tick);
    let proof =
        execution_rcs_retirement_proof(pending.submitted.state, marker == SPIRIT_VFX_POST_MARKER);
    if !proof.complete() {
        if elapsed_ms < SPIRIT_VFX_COMPLETION_TIMEOUT_MS {
            return SpiritVfxCompletion::Pending;
        }
        if !pending.timeout_logged {
            let reason = if marker == SPIRIT_VFX_POST_MARKER {
                "spirit-vfx-marker-observed-context-save-timeout"
            } else {
                "spirit-vfx-marker-timeout"
            };
            // Render carriers are independent GuC consumers. Their lifecycle
            // cannot prove anything about Spirit's HWLRCA, destination, or
            // producer lease; contain only Spirit's own execution lane while
            // retaining the exact pending job for a possible late proof.
            let pre_marker = direct_rcs_read_result_slot(
                pending.submitted.state,
                SPIRIT_VFX_PRE_MARKER_SLOT,
            );
            if let Some(dev) = crate::intel::claimed_device() {
                // Capture before disabling the context can change which
                // carrier owns the live registers. This is one observation
                // per timed-out job, with no recovery or register writes.
                const CS_CHICKEN1: usize = RCS_RING_BASE + 0x580;
                const FF_SLICE_CS_CHICKEN1: usize = RCS_RING_BASE + 0xE0;
                let preemption = crate::intel::mmio_read(dev, CS_CHICKEN1);
                crate::log_error!(target: "gpgpu";
                    "intel/gpgpu: spirit-vfx timeout-frontier tag={} pre_marker=0x{:08X} pre_marker_observed={:?} post_marker=0x{:08X} ring_start=0x{:08X} ring_head=0x{:08X} ring_tail=0x{:08X} cs_chicken1=0x{:08X} compute_preemption={} ff_slice_cs_chicken1=0x{:08X} capture=before-context-disable\n",
                    handle.tag,
                    pre_marker,
                    // The QWord POST store overwrites the neighboring PRE
                    // slot. Interpret PRE only while POST is absent.
                    (marker != SPIRIT_VFX_POST_MARKER)
                        .then_some(pre_marker == SPIRIT_VFX_PRE_MARKER),
                    marker,
                    crate::intel::mmio_read(dev, RCS_RING_START),
                    crate::intel::mmio_read(dev, RCS_RING_HEAD),
                    crate::intel::mmio_read(dev, RCS_RING_TAIL),
                    preemption,
                    (preemption >> 1) & 3,
                    crate::intel::mmio_read(dev, FF_SLICE_CS_CHICKEN1),
                );
                if pending.submitted.encoding_evidence_logged {
                    if let Some(evidence) = spirit_vfx_capture_encoding_evidence(
                        dev, pending.submitted.state, pending.submitted.control,
                        pending.submitted.first_kernel.gpu == SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_GPU,
                    ) {
                        spirit_vfx_log_encoding_evidence(handle.tag, pending.submitted, "first-timeout", evidence);
                    }
                }
                spirit_vfx_log_mapping_evidence(dev, pending.submitted);
            }
            quarantine_execution_rcs_context(reason);
            crate::log_error!(target: "gpgpu";
                "intel/gpgpu: spirit-vfx retirement timeout tag={} frame={} marker=0x{:08X} marker_observed={} saved_head={} published_tail={} pending=retained detached_tag=retained timeline=retained producer_lease=retained backing=retained render_peer_dependency=none action=quarantine-execution-lane-and-keep-polling\n",
                handle.tag,
                pending.submitted.frame,
                marker,
                proof.marker_observed as u8,
                proof.saved_head_bytes,
                proof.published_tail_bytes,
            );
            pending.timeout_logged = true;
            *pending_slot = Some(pending);
        }
        // Returning Failed would make Spirit cancel the producer lease while
        // a late GuC request may still write it. Keep ownership pinned and
        // permit only a later marker+saved-HEAD proof to retire this handle.
        return SpiritVfxCompletion::Pending;
    }
    if crate::intel::render::tgl_spirit_compute_contract() {
        if let Ok(retired_index) = SPIRIT_VFX_SAVED_CONTEXT_LOGS.fetch_update(
            Ordering::Relaxed, Ordering::Relaxed, |count| (count < 2).then_some(count + 1),
        ) {
            let saved_control = spirit_vfx_owned_dwords::<1>(
                pending.submitted.state.context_virt,
                DIRECT_RCS_CONTEXT_BYTES,
                (DIRECT_RCS_LRC_STATE_OFFSET_DWORDS + 3) * core::mem::size_of::<u32>(),
            ).map(|words| words[0]);
            crate::log_important!(target: "gpgpu";
                "intel/gpgpu: spirit-vfx saved-context tag={} frame={} retired_index={} control={:08X?} engine_restore_inhibit={:?} next_restore={} capture=owned-hwlrca-after-marker-and-saved-head writes=none\n",
                handle.tag, pending.submitted.frame, retired_index, saved_control,
                saved_control.map(|control| control & 1 != 0),
                match saved_control {
                    Some(control) if control & 1 == 0 => "enabled-observed",
                    _ => "unproven",
                });
        }
    }
    *pending_slot = None;
    drop(pending_slot);

    complete_execution_rcs_submission();
    EXECUTION_RCS_DETACHED_TAG.store(0, Ordering::Release);
    if pending.submitted.submit_ambiguous || spirit_vfx_trace_frame(pending.submitted.frame) {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: spirit-vfx complete frame={} ok=1 marker=0x{:08X} saved_head={} published_tail={} revision={} background={} shader={} submit_ms={} admission={} producer-release=marker-plus-guc-context-save\n",
            pending.submitted.frame,
            marker,
            proof.saved_head_bytes,
            proof.published_tail_bytes,
            pending.submitted.revision,
            pending.submitted.background_mode,
            pending.submitted.shader_mode,
            direct_rcs_elapsed_ms_since(pending.submitted.started_tick),
            if pending.submitted.submit_ambiguous {
                "ambiguous-late-accepted"
            } else {
                "guc"
            },
        );
    }
    SpiritVfxCompletion::Complete(gpgpu_rgba8_release(pending.submitted.dst))
}

fn submit_spirit_vfx_batch(
    dst: GpgpuRgba8Surface,
    source: GpgpuRgba8Surface,
    control: SpiritVfxControl,
    present_fps: u32,
) -> Option<(SpiritVfxSubmitted, Option<SpiritVfxEncodingEvidence>)> {
    if !dst.is_valid()
        || dst.width != SPIRIT_VFX_SIZE
        || dst.height != SPIRIT_VFX_SIZE
        || dst.pitch_bytes < SPIRIT_VFX_SIZE * 4
        || dst.storage_order != GpgpuRgba8StorageOrder::Bgra
        || !source.is_valid()
        || source.storage_order != GpgpuRgba8StorageOrder::Rgba
        || source.width == 0
        || source.height == 0
        || source.width > SPIRIT_VFX_SIZE
        || source.height > SPIRIT_VFX_SIZE
    {
        return None;
    }
    let started_tick = direct_rcs_now_tick();
    let dev = super::claimed_device()?;
    let background_upload = if matches!(control.background_mode, 2..=11) {
        Some(upload_spirit_vfx_background_rgba8_kernel()?)
    } else {
        None
    };
    let sprite_upload = upload_spirit_vfx_sprite_rgba8_kernel()?;
    let shared_state = execution_rcs_state_once(dev)?;
    let runtime = spirit_vfx_runtime_once()?;
    let frame = SPIRIT_VFX_NEXT_FRAME.fetch_add(1, Ordering::AcqRel);
    spirit_vfx_write_control(runtime.control, frame, dst.pitch_bytes, source, control, present_fps);

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, shared_state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(shared_state);
    let background_ok = match background_upload {
        Some(upload) => {
            direct_rcs_map_ppgtt_kernel(shared_state, upload.gpu, upload.phys, upload.mapped_bytes)
        }
        None => true,
    };
    let kernels_ok = ppgtt_ok
        && background_ok
        && direct_rcs_map_ppgtt_kernel(
            shared_state,
            sprite_upload.gpu,
            sprite_upload.phys,
            sprite_upload.mapped_bytes,
        );
    let resources_ok = kernels_ok
        && direct_rcs_map_ppgtt_kernel(
            shared_state,
            runtime.control.gpu,
            runtime.control.phys,
            runtime.control.bytes,
        )
        && direct_rcs_map_ppgtt_kernel(shared_state, source.gpu, source.phys, source.bytes);
    let dst_ok =
        resources_ok && direct_rcs_map_ppgtt_scanout(shared_state, dst.gpu, dst.phys, dst.bytes);
    let state = execution_rcs_next_job_slot(shared_state)?;
    let batch_ok = dst_ok
        && direct_rcs_encode_spirit_vfx_batch(
            state,
            background_upload,
            sprite_upload,
            runtime.control,
            source,
            dst,
        );
    if !batch_ok {
        return None;
    }
    let encoding_evidence = (SPIRIT_VFX_ENCODING_LOGS.load(Ordering::Acquire)
        < SPIRIT_VFX_ENCODING_LOG_LIMIT).then(|| {
        spirit_vfx_capture_encoding_evidence(dev, state, runtime.control, background_upload.is_some())
    }).flatten();
    let submitted_to_guc = execution_rcs_submit_batch(dev, state);
    // Every result other than Busy after tail publication is ambiguous: GuC
    // may still fetch and execute the request even though the software call
    // returned an error. The lane-level submit path quarantines that exact
    // context. Preserve a detached Spirit handle in that case so its cursor
    // producer lease cannot be cancelled/reused until marker + saved HEAD
    // prove retirement. If GuC never accepted the request, that proof never
    // arrives and the allocation intentionally remains pinned.
    let submit_ambiguous = !submitted_to_guc && execution_rcs_context_is_quarantined();
    if !submitted_to_guc && !submit_ambiguous {
        return None;
    }
    Some((SpiritVfxSubmitted {
        state,
        first_kernel: background_upload.unwrap_or(sprite_upload),
        control: runtime.control,
        source,
        dst,
        submit_ambiguous,
        frame,
        revision: control.revision,
        background_mode: control.background_mode,
        shader_mode: control.shader_mode,
        started_tick,
        encoding_evidence_logged: false,
    }, encoding_evidence))
}

fn spirit_vfx_trace_frame(frame: u32) -> bool {
    frame < SPIRIT_VFX_INITIAL_TRACE_FRAMES
        || frame
            .wrapping_add(1)
            .is_multiple_of(SPIRIT_VFX_PERIODIC_TRACE_FRAMES)
}
