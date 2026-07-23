const SPIRIT_VFX_INITIAL_TRACE_FRAMES: u32 = 30;
const SPIRIT_VFX_PERIODIC_TRACE_FRAMES: u32 = 60;

#[derive(Copy, Clone, Debug)]
pub(crate) struct SpiritVfxControl {
    pub(crate) revision: u64,
    pub(crate) background_mode: u32,
    pub(crate) background_opacity: f32,
    pub(crate) background_scale: f32,
    pub(crate) background_speed: f32,
    pub(crate) background_intensity: f32,
    pub(crate) background_color_a: u32,
    pub(crate) background_color_b: u32,
    pub(crate) position_x: f32,
    pub(crate) position_y: f32,
    pub(crate) sprite_scale: f32,
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
    source: GpgpuRgba8Surface,
    dst: GpgpuRgba8Surface,
    frame: u32,
    revision: u64,
    background_mode: u32,
    shader_mode: u32,
    started_tick: u64,
}

#[derive(Copy, Clone)]
struct SpiritVfxPending {
    handle: SpiritVfxSubmission,
    submitted: SpiritVfxSubmitted,
}

static SPIRIT_VFX_RUNTIME: Mutex<Option<SpiritVfxRuntime>> = Mutex::new(None);
static SPIRIT_VFX_PENDING: Mutex<Option<SpiritVfxPending>> = Mutex::new(None);
static SPIRIT_VFX_NEXT_TAG: AtomicU64 = AtomicU64::new(1);
static SPIRIT_VFX_NEXT_FRAME: AtomicU32 = AtomicU32::new(0);

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

fn spirit_vfx_write_control(
    control_buffer: SpiritVfxBuffer,
    frame: u32,
    dst_pitch_bytes: u32,
    source: GpgpuRgba8Surface,
    control: SpiritVfxControl,
    present_fps: u32,
) {
    unsafe {
        core::ptr::write_bytes(control_buffer.virt, 0, control_buffer.bytes);
        let dwords = control_buffer.virt as *mut u32;
        core::ptr::write_volatile(dwords, SPIRIT_VFX_CONTROL_MAGIC);
        core::ptr::write_volatile(dwords.add(1), SPIRIT_VFX_CONTROL_VERSION);
        core::ptr::write_volatile(dwords.add(2), frame);
        core::ptr::write_volatile(
            dwords.add(3),
            matches!(control.background_mode, 1 | 4)
                .then_some(control.background_mode)
                .unwrap_or(0),
        );
        core::ptr::write_volatile(dwords.add(4), (frame as f32 * (1.0 / 60.0)).to_bits());
        core::ptr::write_volatile(
            dwords.add(5),
            spirit_vfx_bounded(control.background_opacity, 0.0, 1.0, 0.0).to_bits(),
        );
        core::ptr::write_volatile(
            dwords.add(6),
            spirit_vfx_bounded(control.background_scale, 0.25, 3.0, 1.0).to_bits(),
        );
        core::ptr::write_volatile(
            dwords.add(7),
            spirit_vfx_bounded(control.background_speed, 0.0, 4.0, 1.0).to_bits(),
        );
        core::ptr::write_volatile(
            dwords.add(8),
            spirit_vfx_bounded(control.background_intensity, 0.1, 2.5, 1.0).to_bits(),
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
        core::ptr::write_volatile(
            dwords.add(13),
            spirit_vfx_bounded(control.sprite_scale, 0.35, 1.55, 0.9).to_bits(),
        );
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
        core::ptr::write_volatile(dwords.add(17), (control.shader_mode == 1) as u32);
        for (index, value) in control.shader_parameters.iter().copied().enumerate() {
            let fallback = [12.0, 1.15, 1.2, 0.18][index];
            core::ptr::write_volatile(
                dwords.add(18 + index),
                spirit_vfx_bounded(value, -128.0, 256.0, fallback).to_bits(),
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
        || LAB256_SPIRIT_PENDING.lock().is_some()
    {
        return None;
    }
    let submitted = submit_spirit_vfx_batch(dst, source, control, present_fps)?;
    let tag = SPIRIT_VFX_NEXT_TAG
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1)
        .max(1);
    let handle = SpiritVfxSubmission {
        tag,
        frame: submitted.frame,
    };
    EXECUTION_RCS_DETACHED_TAG.store(tag, Ordering::Release);
    *SPIRIT_VFX_PENDING.lock() = Some(SpiritVfxPending { handle, submitted });
    if spirit_vfx_trace_frame(submitted.frame) {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: spirit-vfx accepted tag={} frame={} revision={} background={} shader={} src={}x{}@0x{:X} dst=0x{:X} walkers=2 dependency=hdc-flush+invalidate owner=spirit-worker wait=detached\n",
            tag,
            submitted.frame,
            submitted.revision,
            submitted.background_mode,
            submitted.shader_mode,
            submitted.source.width,
            submitted.source.height,
            submitted.source.gpu,
            submitted.dst.gpu,
        );
    }
    Some(handle)
}

pub(crate) fn poll_spirit_vfx_submission(handle: SpiritVfxSubmission) -> SpiritVfxCompletion {
    if EXECUTION_RCS_DETACHED_TAG.load(Ordering::Acquire) != handle.tag {
        return SpiritVfxCompletion::InvalidSubmission;
    }
    let mut pending_slot = SPIRIT_VFX_PENDING.lock();
    let Some(pending) = *pending_slot else {
        return SpiritVfxCompletion::InvalidSubmission;
    };
    if pending.handle != handle {
        return SpiritVfxCompletion::InvalidSubmission;
    }
    let marker = direct_rcs_read_result_slot(pending.submitted.state, SPIRIT_VFX_POST_MARKER_SLOT);
    let elapsed_ms = direct_rcs_elapsed_ms_since(pending.submitted.started_tick);
    if marker != SPIRIT_VFX_POST_MARKER && elapsed_ms < SPIRIT_VFX_COMPLETION_TIMEOUT_MS {
        return SpiritVfxCompletion::Pending;
    }
    *pending_slot = None;
    drop(pending_slot);

    let ok = marker == SPIRIT_VFX_POST_MARKER;
    complete_execution_rcs_submission(ok);
    if !ok {
        quarantine_execution_rcs_context("spirit-vfx-marker-timeout");
    }
    EXECUTION_RCS_DETACHED_TAG.store(0, Ordering::Release);
    if spirit_vfx_trace_frame(pending.submitted.frame) || !ok {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: spirit-vfx complete frame={} ok={} marker=0x{:08X} revision={} background={} shader={} submit_ms={} producer-release=guc-post-sync\n",
            pending.submitted.frame,
            ok as u8,
            marker,
            pending.submitted.revision,
            pending.submitted.background_mode,
            pending.submitted.shader_mode,
            direct_rcs_elapsed_ms_since(pending.submitted.started_tick),
        );
    }
    if ok {
        SpiritVfxCompletion::Complete(gpgpu_rgba8_release(pending.submitted.dst))
    } else {
        SpiritVfxCompletion::Failed
    }
}

fn submit_spirit_vfx_batch(
    dst: GpgpuRgba8Surface,
    source: GpgpuRgba8Surface,
    control: SpiritVfxControl,
    present_fps: u32,
) -> Option<SpiritVfxSubmitted> {
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
    let background_upload = upload_spirit_vfx_background_rgba8_kernel()?;
    let sprite_upload = upload_spirit_vfx_sprite_rgba8_kernel()?;
    let state = execution_rcs_state_once(dev)?;
    let runtime = spirit_vfx_runtime_once()?;
    let frame = SPIRIT_VFX_NEXT_FRAME.fetch_add(1, Ordering::AcqRel);
    spirit_vfx_write_control(runtime.control, frame, dst.pitch_bytes, source, control, present_fps);

    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernels_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            background_upload.gpu,
            background_upload.phys,
            background_upload.mapped_bytes,
        )
        && direct_rcs_map_ppgtt_kernel(
            state,
            sprite_upload.gpu,
            sprite_upload.phys,
            sprite_upload.mapped_bytes,
        );
    let resources_ok = kernels_ok
        && direct_rcs_map_ppgtt_kernel(
            state,
            runtime.control.gpu,
            runtime.control.phys,
            runtime.control.bytes,
        )
        && direct_rcs_map_ppgtt_kernel(state, source.gpu, source.phys, source.bytes);
    let dst_ok = resources_ok && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok = dst_ok
        && direct_rcs_encode_spirit_vfx_batch(
            state,
            background_upload,
            sprite_upload,
            runtime.control,
            source,
            dst,
        );
    if !batch_ok || !execution_rcs_submit_batch(dev, state) {
        return None;
    }
    Some(SpiritVfxSubmitted {
        state,
        source,
        dst,
        frame,
        revision: control.revision,
        background_mode: control.background_mode,
        shader_mode: control.shader_mode,
        started_tick,
    })
}

fn spirit_vfx_trace_frame(frame: u32) -> bool {
    frame < SPIRIT_VFX_INITIAL_TRACE_FRAMES
        || frame
            .wrapping_add(1)
            .is_multiple_of(SPIRIT_VFX_PERIODIC_TRACE_FRAMES)
}
