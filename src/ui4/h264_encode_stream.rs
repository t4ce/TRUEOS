//! Resident UI4 scanout-to-Intel-H.264-to-UDP service.
//!
//! After a boot-time hardware proof, each subscriber receives a fresh,
//! deadline-paced ten-second session. Logical D01 scanout is composed in
//! memory, converted to NV12, encoded by Gen12 VDEnc/MFX, and handed directly
//! to the bounded TME1 transport. No filesystem or software-codec stage
//! participates.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use embassy_time::{Duration, Timer};
use spin::Mutex;

const PROBE_START_DELAY_MS: u64 = 15_000;
const VCS0_PROBE_RETRY_MS: u64 = 50;
const TEST_RIG_SCANOUT_WIDTH: u32 = crate::intel::media::avc_encode_probe::FRAME_WIDTH as u32;
const TEST_RIG_SCANOUT_HEIGHT: u32 = crate::intel::media::avc_encode_probe::FRAME_HEIGHT as u32;
const SOURCE_RGBA_BYTES: usize =
    TEST_RIG_SCANOUT_WIDTH as usize * TEST_RIG_SCANOUT_HEIGHT as usize * 4;
const ENCODE_WIDTH: usize = TEST_RIG_SCANOUT_WIDTH as usize;
const ENCODE_HEIGHT: usize = TEST_RIG_SCANOUT_HEIGHT as usize;
const ACTIVE_HEIGHT: usize = ENCODE_HEIGHT;
const ACTIVE_TOP: usize = (ENCODE_HEIGHT - ACTIVE_HEIGHT) / 2;
const ENCODE_LUMA_BYTES: usize = ENCODE_WIDTH * ENCODE_HEIGHT;
const ENCODE_NV12_BYTES: usize = ENCODE_LUMA_BYTES * 3 / 2;
const CADENCE_TOLERANCE_PERCENT: u64 = 5;
const PREPARE_IDLE_POLL_MS: u64 = 1;
const PREPARE_SLOT_COUNT: usize = 2;
// A foreground decode stream owns a session-level VCS0 reservation. Keep the
// boot proof retryable until that stream drains instead of permanently parking
// the encoder after an arbitrary startup window.
const VCS0_PROBE_WAIT_LOG_INTERVAL: usize = 600;

static STATE: AtomicU8 = AtomicU8::new(H264EncodeStreamState::Waiting as u8);
static ENCODE_US: AtomicU64 = AtomicU64::new(0);
static SOURCE_BYTES: AtomicUsize = AtomicUsize::new(0);
static ENCODED_BYTES: AtomicUsize = AtomicUsize::new(0);
static PREPARE_WORKER_SLOT: AtomicU32 = AtomicU32::new(u32::MAX);
static PREPARE_PIPELINE: Mutex<PreparePipeline> = Mutex::new(PreparePipeline::new());

const _: () = {
    assert!(ENCODE_WIDTH % 16 == 0);
    assert!(ENCODE_HEIGHT % 16 == 0);
    assert!(ENCODE_WIDTH == crate::intel::media::avc_encode_probe::FRAME_WIDTH);
    assert!(ENCODE_HEIGHT == crate::intel::media::avc_encode_probe::FRAME_HEIGHT);
    assert!(ACTIVE_TOP == 0);
    assert!((ENCODE_HEIGHT - ACTIVE_HEIGHT - ACTIVE_TOP) == 0);
};

#[derive(Default)]
struct LiveEncodeStats {
    frames: usize,
    idr_frames: usize,
    p_frames: usize,
    source_width: u32,
    source_height: u32,
    first_source_fnv1a32: u32,
    last_source_fnv1a32: u32,
    source_changes: usize,
    capture_us: u64,
    capture_max_us: u64,
    convert_wall_us: u64,
    convert_wall_max_us: u64,
    convert_gpu_us: u64,
    convert_gpu_max_us: u64,
    encode_us: u64,
    encode_max_us: u64,
    coded_bytes: usize,
    coded_max_bytes: usize,
    slot0_scanout_frames: usize,
    slot0_scanout_pixels: usize,
    spirit_overlay_frames: usize,
    spirit_overlay_pixels: usize,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum PrepareSlotState {
    Empty,
    Filling,
    Ready,
    Consuming,
}

struct PrepareSlot {
    state: PrepareSlotState,
    generation: u64,
    session_id: u32,
    sequence: u32,
    source_width: u32,
    source_height: u32,
    source_fnv1a32: u32,
    capture_us: u64,
    convert_wall_us: u64,
    convert_gpu_us: u64,
    slot0_scanout_pixels: usize,
    spirit_overlay_pixels: usize,
    valid: bool,
    source_rgba: Option<EncodeDmaBuffer>,
    nv12: Option<EncodeDmaBuffer>,
}

impl PrepareSlot {
    const fn new() -> Self {
        Self {
            state: PrepareSlotState::Empty,
            generation: 0,
            session_id: 0,
            sequence: 0,
            source_width: 0,
            source_height: 0,
            source_fnv1a32: 0,
            capture_us: 0,
            convert_wall_us: 0,
            convert_gpu_us: 0,
            slot0_scanout_pixels: 0,
            spirit_overlay_pixels: 0,
            valid: false,
            source_rgba: None,
            nv12: None,
        }
    }

    fn reset_metadata(&mut self) {
        self.source_width = 0;
        self.source_height = 0;
        self.source_fnv1a32 = 0;
        self.capture_us = 0;
        self.convert_wall_us = 0;
        self.convert_gpu_us = 0;
        self.slot0_scanout_pixels = 0;
        self.spirit_overlay_pixels = 0;
        self.valid = false;
    }
}

struct PreparePipeline {
    active: bool,
    generation: u64,
    session_id: u32,
    access_unit_count: usize,
    next_sequence: usize,
    slots: [PrepareSlot; PREPARE_SLOT_COUNT],
}

impl PreparePipeline {
    const fn new() -> Self {
        Self {
            active: false,
            generation: 0,
            session_id: 0,
            access_unit_count: 0,
            next_sequence: 0,
            slots: [PrepareSlot::new(), PrepareSlot::new()],
        }
    }
}

struct PrepareJob {
    slot_index: usize,
    generation: u64,
    session_id: u32,
    sequence: u32,
    source_rgba: Option<EncodeDmaBuffer>,
    nv12: Option<EncodeDmaBuffer>,
}

struct PreparedScanout {
    slot_index: usize,
    generation: u64,
    session_id: u32,
    sequence: u32,
    source_width: u32,
    source_height: u32,
    source_fnv1a32: u32,
    capture_us: u64,
    convert_wall_us: u64,
    convert_gpu_us: u64,
    slot0_scanout_pixels: usize,
    spirit_overlay_pixels: usize,
    valid: bool,
    source_rgba: Option<EncodeDmaBuffer>,
    nv12: Option<EncodeDmaBuffer>,
}

struct EncodeDmaBuffer {
    phys: u64,
    virt: *mut u8,
    bytes: usize,
}

unsafe impl Send for EncodeDmaBuffer {}
unsafe impl Sync for EncodeDmaBuffer {}

impl EncodeDmaBuffer {
    fn allocate(bytes: usize) -> Option<Self> {
        let (phys, virt) = crate::dma::alloc(bytes, crate::intel::WARM_ALIGN)?;
        unsafe {
            core::ptr::write_bytes(virt, 0, bytes);
        }
        crate::intel::dma_flush(virt, bytes);
        Some(Self { phys, virt, bytes })
    }

    fn as_slice(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.virt.cast_const(), self.bytes) }
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.virt, self.bytes) }
    }
}

impl Drop for EncodeDmaBuffer {
    fn drop(&mut self) {
        crate::dma::dealloc(self.virt, self.bytes);
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum H264EncodeStreamState {
    Waiting = 0,
    Encoding = 1,
    Streaming = 2,
    Verified = 3,
    Failed = 4,
}

impl H264EncodeStreamState {
    fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::Encoding,
            2 => Self::Streaming,
            3 => Self::Verified,
            4 => Self::Failed,
            _ => Self::Waiting,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct H264EncodeStreamSnapshot {
    pub(crate) state: H264EncodeStreamState,
    pub(crate) encode_us: u64,
    pub(crate) source_bytes: usize,
    pub(crate) encoded_bytes: usize,
}

pub(crate) fn h264_encode_stream_snapshot() -> H264EncodeStreamSnapshot {
    H264EncodeStreamSnapshot {
        state: H264EncodeStreamState::from_raw(STATE.load(Ordering::Acquire)),
        encode_us: ENCODE_US.load(Ordering::Acquire),
        source_bytes: SOURCE_BYTES.load(Ordering::Acquire),
        encoded_bytes: ENCODED_BYTES.load(Ordering::Acquire),
    }
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn ui4_h264_encode_prepare_task(assigned_slot: u32) {
    let worker = crate::cpu::CpuProfile::current();
    let worker_slot = worker.map(|profile| profile.slot()).unwrap_or(u32::MAX);
    let worker_kind = worker
        .map(|profile| profile.core_kind_name())
        .unwrap_or("unknown");
    PREPARE_WORKER_SLOT.store(worker_slot, Ordering::Release);
    crate::log_info!(target: "intel/media-encode";
        "intel/media-encode: preparation service online carrier=lastap assigned_slot={} worker_slot={} worker_kind={} pipeline=rgba-to-nv12 engine=guc-rcs kernel=cpp-linear-rgba8-to-nv12 cpu_pixel_math=0 cpu_composition=opaque-zcopy-frontmost-wins-no-alpha dma_buffers=persistent producer buffering=double slots={} encode_size={}x{} mapping=native-identity active_size={}x{} padding=top:0,bottom:0 slot0_base=pipe-a-plane0-surflive-primary-rgba-premultiplied slot0_windows=hardcoded-none spirit_overlay=pipe-a-cur-surflive-bgra-binary-mask synchronization=completion-marker-before-vdbox\n",
        assigned_slot,
        worker_slot,
        worker_kind,
        PREPARE_SLOT_COUNT,
        ENCODE_WIDTH,
        ENCODE_HEIGHT,
        ENCODE_WIDTH,
        ACTIVE_HEIGHT,
    );
    if worker_slot != assigned_slot {
        crate::log_error!(target: "intel/media-encode";
            "intel/media-encode: preparation service rejected assigned_slot={} actual_slot={} reason=executor-residency-mismatch action=park\n",
            assigned_slot,
            worker_slot,
        );
        park().await;
    }

    loop {
        let Some(job) = take_prepare_job() else {
            Timer::after(Duration::from_millis(PREPARE_IDLE_POLL_MS)).await;
            continue;
        };
        prepare_scanout(job).await;
    }
}

fn take_prepare_job() -> Option<PrepareJob> {
    let mut pipeline = PREPARE_PIPELINE.lock();
    if !pipeline.active || pipeline.next_sequence >= pipeline.access_unit_count {
        return None;
    }
    let slot_index = pipeline
        .slots
        .iter()
        .position(|slot| slot.state == PrepareSlotState::Empty)?;
    let generation = pipeline.generation;
    let session_id = pipeline.session_id;
    let sequence = pipeline.next_sequence as u32;
    pipeline.next_sequence = pipeline.next_sequence.saturating_add(1);
    let slot = &mut pipeline.slots[slot_index];
    slot.state = PrepareSlotState::Filling;
    slot.generation = generation;
    slot.session_id = session_id;
    slot.sequence = sequence;
    slot.reset_metadata();
    Some(PrepareJob {
        slot_index,
        generation,
        session_id,
        sequence,
        source_rgba: slot.source_rgba.take(),
        nv12: slot.nv12.take(),
    })
}

async fn prepare_scanout(mut job: PrepareJob) {
    let mut source_width = 0;
    let mut source_height = 0;
    let mut source_fnv1a32 = 0;
    let mut capture_us = 0;
    let mut convert_wall_us = 0;
    let mut convert_gpu_us = 0;
    let mut slot0_scanout_pixels = 0;
    let mut spirit_overlay_pixels = 0;
    if job.source_rgba.is_none() {
        job.source_rgba = EncodeDmaBuffer::allocate(SOURCE_RGBA_BYTES);
    }
    if job.nv12.is_none() {
        job.nv12 = EncodeDmaBuffer::allocate(ENCODE_NV12_BYTES);
    }

    let mut valid = false;
    if job.source_rgba.is_none() || job.nv12.is_none() {
        crate::log_error!(target: "intel/media-encode";
            "intel/media-encode: live frame rejected session={} sequence={} stage=guc-rcs-dma-allocation rgba_bytes={} nv12_bytes={} software_fallback=0\n",
            job.session_id,
            job.sequence,
            SOURCE_RGBA_BYTES,
            ENCODE_NV12_BYTES,
        );
    } else {
        let composition_started_ns = crate::chronos::monotonic_nanos();
        let composition_result = {
            let source = job
                .source_rgba
                .as_mut()
                .expect("checked source DMA allocation");
            super::screenshot::compose_realtime_encode_rgba_into(source.as_mut_slice())
        };
        capture_us =
            crate::chronos::monotonic_nanos().saturating_sub(composition_started_ns) / 1_000;
        match composition_result {
            Ok(composition) => {
                source_width = composition.width;
                source_height = composition.height;
                slot0_scanout_pixels = composition.slot0_scanout_pixels;
                spirit_overlay_pixels = composition.spirit_overlay_pixels;
                let source = job
                    .source_rgba
                    .as_ref()
                    .expect("checked source DMA allocation");
                let destination = job.nv12.as_ref().expect("checked NV12 DMA allocation");
                source_fnv1a32 = fnv1a32_sampled(source.as_slice());
                crate::intel::dma_flush(source.virt, source.bytes);
                let source_surface = crate::intel::gpgpu::GpgpuRgba8Surface::new(
                    source.phys,
                    crate::intel::gpgpu::UI4_COMPOSITOR_NV12_SOURCE_GPU_BASE,
                    source.bytes,
                    composition.width,
                    composition.height,
                    composition.width.saturating_mul(4),
                );
                let destination_surface = crate::intel::gpgpu::GpgpuNv12LinearSurface::new(
                    destination.phys,
                    crate::intel::gpgpu::UI4_STREAM_NV12_DESTINATION_GPU,
                    destination.bytes,
                    ENCODE_WIDTH as u32,
                    ENCODE_HEIGHT as u32,
                    ENCODE_WIDTH as u32,
                );
                if let (Some(source_surface), Some(destination_surface)) =
                    (source_surface, destination_surface)
                {
                    let conversion_started_ns = crate::chronos::monotonic_nanos();
                    let submission = loop {
                        match crate::intel::gpgpu::queue_ui4_stream_rgba8_to_nv12_linear(
                            source_surface,
                            destination_surface,
                            ACTIVE_TOP as u32,
                            ACTIVE_HEIGHT as u32,
                        ) {
                            Ok(submission) => break Some(submission),
                            Err(crate::intel::gpgpu::Ui4CompositorSubmitError::Busy) => {
                                Timer::after(Duration::from_millis(1)).await;
                            }
                            Err(error) => {
                                crate::log_error!(target: "intel/media-encode";
                                    "intel/media-encode: live frame rejected session={} sequence={} stage=guc-rcs-rgba-to-nv12-submit error={:?} software_fallback=0\n",
                                    job.session_id,
                                    job.sequence,
                                    error,
                                );
                                break None;
                            }
                        }
                    };
                    if let Some(submission) = submission {
                        loop {
                            match crate::intel::gpgpu::poll_ui4_compositor_submission(submission) {
                                crate::intel::gpgpu::Ui4CompositorCompletion::Pending => {
                                    Timer::after(Duration::from_millis(1)).await;
                                }
                                crate::intel::gpgpu::Ui4CompositorCompletion::Complete(stats) => {
                                    // RCS is the producer and VDBOX is the consumer. The
                                    // terminal RCS marker orders its writes, and VDBOX maps
                                    // this same DMA allocation directly. Avoid invalidating
                                    // the complete NV12 surface on the CPU merely for change
                                    // telemetry; the bounded sample above uses the CPU-owned
                                    // RGBA source before its release flush.
                                    convert_gpu_us = stats.probe.gpu_walker_us;
                                    convert_wall_us = crate::chronos::monotonic_nanos()
                                        .saturating_sub(conversion_started_ns)
                                        / 1_000;
                                    crate::log_trace!(target: "intel/media-encode";
                                        "intel/media-encode: rgba-to-nv12 retired session={} sequence={} engine=guc-rcs kernel=cpp-linear-rgba8-to-nv12 submit_ms={} convert_wall_us={} gpu_walker_us={} cpu_pixel_math=0 cpu_validation_bytes_max=4096 source_bytes={} destination_bytes={}\n",
                                        job.session_id,
                                        job.sequence,
                                        stats.submit_ms,
                                        convert_wall_us,
                                        stats.probe.gpu_walker_us,
                                        source.bytes,
                                        destination.bytes,
                                    );
                                    valid = true;
                                    break;
                                }
                                crate::intel::gpgpu::Ui4CompositorCompletion::Failed
                                | crate::intel::gpgpu::Ui4CompositorCompletion::InvalidSubmission =>
                                {
                                    crate::log_error!(target: "intel/media-encode";
                                        "intel/media-encode: live frame rejected session={} sequence={} stage=guc-rcs-rgba-to-nv12-completion action=quarantine-dma-buffers software_fallback=0\n",
                                        job.session_id,
                                        job.sequence,
                                    );
                                    if let Some(source) = job.source_rgba.take() {
                                        core::mem::forget(source);
                                    }
                                    if let Some(destination) = job.nv12.take() {
                                        core::mem::forget(destination);
                                    }
                                    break;
                                }
                            }
                        }
                    }
                } else {
                    crate::log_error!(target: "intel/media-encode";
                        "intel/media-encode: live frame rejected session={} sequence={} stage=guc-rcs-surface-contract source={}x{} destination={}x{} software_fallback=0\n",
                        job.session_id,
                        job.sequence,
                        composition.width,
                        composition.height,
                        ENCODE_WIDTH,
                        ENCODE_HEIGHT,
                    );
                }
            }
            Err(error) => {
                crate::log_error!(target: "intel/media-encode";
                    "intel/media-encode: live frame rejected session={} sequence={} stage=ui4-realtime-encode-composition error={:?} filesystem_writes=0 software_fallback=0\n",
                    job.session_id,
                    job.sequence,
                    error,
                );
            }
        }
    }

    let mut pipeline = PREPARE_PIPELINE.lock();
    let session_current = pipeline.active
        && pipeline.generation == job.generation
        && pipeline.session_id == job.session_id;
    let slot = &mut pipeline.slots[job.slot_index];
    if session_current
        && slot.state == PrepareSlotState::Filling
        && slot.generation == job.generation
        && slot.sequence == job.sequence
    {
        slot.source_width = source_width;
        slot.source_height = source_height;
        slot.source_fnv1a32 = source_fnv1a32;
        slot.capture_us = capture_us;
        slot.convert_wall_us = convert_wall_us;
        slot.convert_gpu_us = convert_gpu_us;
        slot.slot0_scanout_pixels = slot0_scanout_pixels;
        slot.spirit_overlay_pixels = spirit_overlay_pixels;
        slot.valid = valid;
        slot.source_rgba = job.source_rgba;
        slot.nv12 = job.nv12;
        slot.state = PrepareSlotState::Ready;
    }
}

fn begin_preparation_session(session_id: u32, access_unit_count: usize) {
    let mut pipeline = PREPARE_PIPELINE.lock();
    pipeline.generation = pipeline.generation.wrapping_add(1);
    pipeline.active = true;
    pipeline.session_id = session_id;
    pipeline.access_unit_count = access_unit_count;
    pipeline.next_sequence = 0;
    let generation = pipeline.generation;
    for slot in &mut pipeline.slots {
        slot.state = PrepareSlotState::Empty;
        slot.generation = generation;
        slot.session_id = session_id;
        slot.sequence = 0;
        slot.reset_metadata();
    }
}

fn end_preparation_session(session_id: u32) {
    let mut pipeline = PREPARE_PIPELINE.lock();
    if !pipeline.active || pipeline.session_id != session_id {
        return;
    }
    pipeline.active = false;
    pipeline.generation = pipeline.generation.wrapping_add(1);
    pipeline.access_unit_count = 0;
    pipeline.next_sequence = 0;
    let generation = pipeline.generation;
    for slot in &mut pipeline.slots {
        slot.state = PrepareSlotState::Empty;
        slot.generation = generation;
        slot.reset_metadata();
    }
}

fn prepared_scanout_ready(session_id: u32, sequence: u32) -> bool {
    PREPARE_PIPELINE.lock().slots.iter().any(|slot| {
        slot.state == PrepareSlotState::Ready
            && slot.session_id == session_id
            && slot.sequence == sequence
    })
}

fn take_prepared_scanout(session_id: u32, sequence: u32) -> Option<PreparedScanout> {
    let mut pipeline = PREPARE_PIPELINE.lock();
    let slot_index = pipeline.slots.iter().position(|slot| {
        slot.state == PrepareSlotState::Ready
            && slot.session_id == session_id
            && slot.sequence == sequence
    })?;
    let slot = &mut pipeline.slots[slot_index];
    slot.state = PrepareSlotState::Consuming;
    Some(PreparedScanout {
        slot_index,
        generation: slot.generation,
        session_id: slot.session_id,
        sequence: slot.sequence,
        source_width: slot.source_width,
        source_height: slot.source_height,
        source_fnv1a32: slot.source_fnv1a32,
        capture_us: slot.capture_us,
        convert_wall_us: slot.convert_wall_us,
        convert_gpu_us: slot.convert_gpu_us,
        slot0_scanout_pixels: slot.slot0_scanout_pixels,
        spirit_overlay_pixels: slot.spirit_overlay_pixels,
        valid: slot.valid,
        source_rgba: slot.source_rgba.take(),
        nv12: slot.nv12.take(),
    })
}

fn release_prepared_scanout(prepared: &mut PreparedScanout) {
    let mut pipeline = PREPARE_PIPELINE.lock();
    let session_current = pipeline.active
        && pipeline.generation == prepared.generation
        && pipeline.session_id == prepared.session_id;
    let slot = &mut pipeline.slots[prepared.slot_index];
    if session_current
        && slot.state == PrepareSlotState::Consuming
        && slot.generation == prepared.generation
        && slot.sequence == prepared.sequence
    {
        slot.source_rgba = prepared.source_rgba.take();
        slot.nv12 = prepared.nv12.take();
        slot.state = PrepareSlotState::Empty;
        slot.reset_metadata();
    }
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn ui4_h264_encode_stream_task() {
    let worker = crate::cpu::CpuProfile::current();
    let worker_slot = worker.map(|profile| profile.slot()).unwrap_or(u32::MAX);
    let worker_kind = worker
        .map(|profile| profile.core_kind_name())
        .unwrap_or("unknown");
    crate::log_info!(target: "intel/media-encode";
        "intel/media-encode: service online carrier=lastap worker_slot={} worker_kind={} exclusive_carrier=1 feature=trueos_h264_encode_stream boot_proof=procedural-nv12-hardware-only live_source=ui4-logical-scanout-d01 encode_size={}x{} target_fps={} backend=gen12-vdenc-mfx completion_wait=cooperative-fence-yield output=udp-only live_high_water_cap={} pipeline=prepare-producer+encode-producer+udp-egress-consumer preparation=cooperative-lastap-double-buffer slots={} encoder_arena_cpu_maintenance=one-time-output-init+per-frame-command-result-only filesystem_writes=0 software_fallback=0 embedded_probe_asset_bytes=0 udp_protocol=tme1 udp_port={} start_delay_ms={}\n",
        worker_slot,
        worker_kind,
        ENCODE_WIDTH,
        ENCODE_HEIGHT,
        crate::allcaps::media_encode::REALTIME_HZ,
        super::h264_encode_udp::encoded_access_unit_queue_cap(),
        PREPARE_SLOT_COUNT,
        crate::allports::services::MEDIA_ENCODE_UDP_PORT,
        PROBE_START_DELAY_MS,
    );

    Timer::after(Duration::from_millis(PROBE_START_DELAY_MS)).await;

    let mut vcs0_probe = crate::intel::run_media_guc_vcs0_probe_once();
    let mut vcs0_probe_attempts = 1usize;
    while vcs0_probe.state == crate::intel::media::guc_probe::GucVcs0ProbeState::Deferred {
        if vcs0_probe_attempts % VCS0_PROBE_WAIT_LOG_INTERVAL == 0 {
            crate::log_info!(target: "intel/media-encode";
                "intel/media-encode: guc-vcs0-probe waiting state=deferred failure={} attempts={} retry_ms={} action=retry software_fallback=0\n",
                vcs0_probe.failure.name(),
                vcs0_probe_attempts,
                VCS0_PROBE_RETRY_MS,
            );
        }
        Timer::after(Duration::from_millis(VCS0_PROBE_RETRY_MS)).await;
        vcs0_probe = crate::intel::run_media_guc_vcs0_probe_once();
        vcs0_probe_attempts += 1;
    }
    if vcs0_probe.state == crate::intel::media::guc_probe::GucVcs0ProbeState::Passed {
        crate::log_info!(target: "intel/media-encode";
            "intel/media-encode: guc-vcs0-probe accepted=1 engine=vcs0 class=1 instance_mask=0x1 submission_owner=guc serial={} forcewake={} backing={} batch={} context={} registered={} submitted={} retired={} context_destroyed={} hwlrca=0x{:08X}:0x{:08X} markers=0x{:08X}/0x{:08X}/0x{:08X}/0x{:08X} poll_iters={} elapsed_us={} attempts={} codec_packets=0 encode_claim=0\n",
            vcs0_probe.serial,
            vcs0_probe.forcewake as u8,
            vcs0_probe.backing_ready as u8,
            vcs0_probe.batch_ready as u8,
            vcs0_probe.context_ready as u8,
            vcs0_probe.registered as u8,
            vcs0_probe.submitted as u8,
            vcs0_probe.retired as u8,
            vcs0_probe.context_destroyed as u8,
            vcs0_probe.hwlrca_hi,
            vcs0_probe.hwlrca_lo,
            vcs0_probe.kickoff,
            vcs0_probe.presubmit,
            vcs0_probe.postsubmit,
            vcs0_probe.complete,
            vcs0_probe.poll_iters,
            vcs0_probe.elapsed_us,
            vcs0_probe_attempts,
        );
    } else {
        crate::log_error!(target: "intel/media-encode";
            "intel/media-encode: guc-vcs0-probe accepted=0 state={:?} failure={} forcewake={} backing={} batch={} context={} registered={} submitted={} retired={} context_destroyed={} serial={} markers=0x{:08X}/0x{:08X}/0x{:08X}/0x{:08X} poll_iters={} elapsed_us={} attempts={} software_fallback=0 action=park encode_claim=0\n",
            vcs0_probe.state,
            vcs0_probe.failure.name(),
            vcs0_probe.forcewake as u8,
            vcs0_probe.backing_ready as u8,
            vcs0_probe.batch_ready as u8,
            vcs0_probe.context_ready as u8,
            vcs0_probe.registered as u8,
            vcs0_probe.submitted as u8,
            vcs0_probe.retired as u8,
            vcs0_probe.context_destroyed as u8,
            vcs0_probe.serial,
            vcs0_probe.kickoff,
            vcs0_probe.presubmit,
            vcs0_probe.postsubmit,
            vcs0_probe.complete,
            vcs0_probe.poll_iters,
            vcs0_probe.elapsed_us,
            vcs0_probe_attempts,
        );
    }

    if vcs0_probe.state == crate::intel::media::guc_probe::GucVcs0ProbeState::Passed {
        let mut avc_probe = crate::intel::run_media_avc_encode_probe_once().await;
        let mut avc_probe_attempts = 1usize;
        while avc_probe.state
            == crate::intel::media::avc_encode_probe::AvcEncodeProbeState::Deferred
        {
            if avc_probe_attempts % VCS0_PROBE_WAIT_LOG_INTERVAL == 0 {
                crate::log_info!(target: "intel/media-encode";
                    "intel/media-encode: avc-idr-probe waiting state=deferred failure={} attempts={} retry_ms={} action=retry software_fallback=0\n",
                    avc_probe.failure.name(),
                    avc_probe_attempts,
                    VCS0_PROBE_RETRY_MS,
                );
            }
            Timer::after(Duration::from_millis(VCS0_PROBE_RETRY_MS)).await;
            avc_probe = crate::intel::run_media_avc_encode_probe_once().await;
            avc_probe_attempts += 1;
        }
        if avc_probe.state == crate::intel::media::avc_encode_probe::AvcEncodeProbeState::Passed {
            crate::log_info!(target: "intel/media-encode";
                "intel/media-encode: avc-idr-probe accepted=1 engine=vcs0 codec_mode=avc-encode submission_owner=guc batch_level=second-level-return terminal_fence=primary-batch-return-marker source=procedural-nv12 source_layout=nv12-linear visible={}x{} pitch={} source_bytes={} source_fnv1a32=0x{:08X} embedded_probe_asset_bytes=0 backing={} surface_uploaded={} batch={} batch_bytes={} primary_batch_bytes={} ring_bytes={} codec_packets={} bitstream_buffer_bound={} registered={} submitted={} retired={} context_destroyed={} serial={} hwlrca=0x{:08X}:0x{:08X} markers=0x{:08X}/0x{:08X}/0x{:08X}/0x{:08X} poll_iters={} elapsed_us={} attempts={} coded_output_validated={} frame_bytes_no_excluded_headers={} excluded_header_bytes={} coded_bytes={} coded_fnv1a32=0x{:08X} nal_flags=0b{:04b} mfx_error=0x{:08X} image_status=0x{:08X} slice_bytes={} slices={} bitstream_head={:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X} hardware_encode=1\n",
                ENCODE_WIDTH,
                ENCODE_HEIGHT,
                ENCODE_WIDTH,
                avc_probe.source_nv12_bytes,
                avc_probe.source_nv12_fnv1a32,
                avc_probe.backing_ready as u8,
                avc_probe.surface_uploaded as u8,
                avc_probe.batch_ready as u8,
                avc_probe.batch_bytes,
                avc_probe.primary_batch_bytes,
                avc_probe.ring_bytes,
                avc_probe.codec_packets,
                avc_probe.bitstream_buffer_bound as u8,
                avc_probe.registered as u8,
                avc_probe.submitted as u8,
                avc_probe.retired as u8,
                avc_probe.context_destroyed as u8,
                avc_probe.serial,
                avc_probe.hwlrca_hi,
                avc_probe.hwlrca_lo,
                avc_probe.kickoff,
                avc_probe.codec_begin,
                avc_probe.codec_end,
                avc_probe.complete,
                avc_probe.poll_iters,
                avc_probe.elapsed_us,
                avc_probe_attempts,
                avc_probe.coded_output_validated as u8,
                avc_probe.mfc_bitstream_bytecount_frame,
                avc_probe.excluded_header_bytes,
                avc_probe.coded_bytes,
                avc_probe.coded_fnv1a32,
                avc_probe.coded_nal_flags,
                avc_probe.mfx_error,
                avc_probe.mfc_image_status_control,
                avc_probe.mfc_bitstream_bytecount_slice,
                avc_probe.mfc_avc_num_slices,
                avc_probe.bitstream_head[0],
                avc_probe.bitstream_head[1],
                avc_probe.bitstream_head[2],
                avc_probe.bitstream_head[3],
                avc_probe.bitstream_head[4],
                avc_probe.bitstream_head[5],
                avc_probe.bitstream_head[6],
                avc_probe.bitstream_head[7],
            );
        } else {
            crate::log_error!(target: "intel/media-encode";
                "intel/media-encode: avc-idr-probe accepted=0 state={:?} failure={} codec_mode=avc-encode submission_owner=guc batch_level=second-level-return terminal_fence=primary-batch-return-marker forcewake={} backing={} surface_uploaded={} batch={} batch_bytes={} primary_batch_bytes={} ring_bytes={} codec_packets={} bitstream_buffer_bound={} registered={} submitted={} retired={} context_destroyed={} serial={} markers=0x{:08X}/0x{:08X}/0x{:08X}/0x{:08X} poll_iters={} elapsed_us={} attempts={} coded_output_validated={} frame_bytes_no_excluded_headers={} excluded_header_bytes={} coded_bytes={} coded_fnv1a32=0x{:08X} nal_flags=0b{:04b} mfx_error=0x{:08X} image_status=0x{:08X} slice_bytes={} slices={} bitstream_head={:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X} bitstream_tail={:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X} hardware_encode=0 software_fallback=0 action=park\n",
                avc_probe.state,
                avc_probe.failure.name(),
                avc_probe.forcewake as u8,
                avc_probe.backing_ready as u8,
                avc_probe.surface_uploaded as u8,
                avc_probe.batch_ready as u8,
                avc_probe.batch_bytes,
                avc_probe.primary_batch_bytes,
                avc_probe.ring_bytes,
                avc_probe.codec_packets,
                avc_probe.bitstream_buffer_bound as u8,
                avc_probe.registered as u8,
                avc_probe.submitted as u8,
                avc_probe.retired as u8,
                avc_probe.context_destroyed as u8,
                avc_probe.serial,
                avc_probe.kickoff,
                avc_probe.codec_begin,
                avc_probe.codec_end,
                avc_probe.complete,
                avc_probe.poll_iters,
                avc_probe.elapsed_us,
                avc_probe_attempts,
                avc_probe.coded_output_validated as u8,
                avc_probe.mfc_bitstream_bytecount_frame,
                avc_probe.excluded_header_bytes,
                avc_probe.coded_bytes,
                avc_probe.coded_fnv1a32,
                avc_probe.coded_nal_flags,
                avc_probe.mfx_error,
                avc_probe.mfc_image_status_control,
                avc_probe.mfc_bitstream_bytecount_slice,
                avc_probe.mfc_avc_num_slices,
                avc_probe.bitstream_head[0],
                avc_probe.bitstream_head[1],
                avc_probe.bitstream_head[2],
                avc_probe.bitstream_head[3],
                avc_probe.bitstream_head[4],
                avc_probe.bitstream_head[5],
                avc_probe.bitstream_head[6],
                avc_probe.bitstream_head[7],
                avc_probe.bitstream_head[8],
                avc_probe.bitstream_head[9],
                avc_probe.bitstream_head[10],
                avc_probe.bitstream_head[11],
                avc_probe.bitstream_head[12],
                avc_probe.bitstream_head[13],
                avc_probe.bitstream_head[14],
                avc_probe.bitstream_head[15],
                avc_probe.bitstream_head[16],
                avc_probe.bitstream_head[17],
                avc_probe.bitstream_head[18],
                avc_probe.bitstream_head[19],
            );
            let diag = avc_probe.timeout_diagnostics;
            if diag.valid {
                crate::log_error!(target: "intel/media-encode";
                    "intel/media-encode: avc-idr-timeout engine=vcs0 ring=start:0x{:08X},ctl:0x{:08X},head:0x{:08X},tail:0x{:08X} acthd=0x{:08X}:0x{:08X} acthd_region={} acthd_off=0x{:X} acthd_dword=0x{:08X} bbaddr=0x{:08X}:0x{:08X} dma_fadd=0x{:08X}:0x{:08X} bbstate=0x{:08X} instruction=instdone:0x{:08X},instps:0x{:08X},ipeir:0x{:08X},ipehr:0x{:08X} esr=0x{:08X} psmi_ctl=0x{:08X} nopid=0x{:08X} fault8=0x{:08X} fault12=0x{:08X} fault8_tlb=0x{:08X}/0x{:08X} fault12_tlb=0x{:08X}/0x{:08X}\n",
                    diag.ring_start,
                    diag.ring_ctl,
                    diag.ring_head,
                    diag.ring_tail,
                    diag.ring_acthd_hi,
                    diag.ring_acthd_lo,
                    diag.acthd_region,
                    diag.acthd_offset_bytes,
                    diag.acthd_dword,
                    diag.bbaddr_hi,
                    diag.bbaddr_lo,
                    diag.dma_fadd_hi,
                    diag.dma_fadd_lo,
                    diag.bbstate,
                    diag.instdone,
                    diag.instps,
                    diag.ipeir,
                    diag.ipehr,
                    diag.esr,
                    diag.psmi_ctl,
                    diag.nopid,
                    diag.fault_gen8,
                    diag.fault_gen12,
                    diag.fault_tlb_data0_gen8,
                    diag.fault_tlb_data1_gen8,
                    diag.fault_tlb_data0_gen12,
                    diag.fault_tlb_data1_gen12,
                );
                crate::log_error!(target: "intel/media-encode";
                    "intel/media-encode: avc-idr-timeout codec=mfx_error:0x{:08X},frame_crc:0x{:08X},mb_count:0x{:08X} mfc=frame_bytes:0x{:08X},frame_se_bits:0x{:08X},slice_bytes:0x{:08X},image_mask:0x{:08X},image_control:0x{:08X},qp_count:0x{:08X},slices:0x{:08X} bitstream_head={:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X}/{:08X} mfx_stats_head={:08X}/{:08X}/{:08X}/{:08X} vdenc_stats_head={:08X}/{:08X}/{:08X}/{:08X} slice_size_head={:08X}/{:08X}/{:08X}/{:08X}\n",
                    diag.mfx_error,
                    diag.mfx_frame_crc,
                    diag.mfx_mb_count,
                    diag.mfc_bitstream_bytecount_frame,
                    diag.mfc_bitstream_se_bitcount_frame,
                    diag.mfc_bitstream_bytecount_slice,
                    diag.mfc_image_status_mask,
                    diag.mfc_image_status_control,
                    diag.mfc_qp_status_count,
                    diag.mfc_avc_num_slices,
                    diag.bitstream_head[0],
                    diag.bitstream_head[1],
                    diag.bitstream_head[2],
                    diag.bitstream_head[3],
                    diag.bitstream_head[4],
                    diag.bitstream_head[5],
                    diag.bitstream_head[6],
                    diag.bitstream_head[7],
                    diag.mfx_stats_head[0],
                    diag.mfx_stats_head[1],
                    diag.mfx_stats_head[2],
                    diag.mfx_stats_head[3],
                    diag.vdenc_stats_head[0],
                    diag.vdenc_stats_head[1],
                    diag.vdenc_stats_head[2],
                    diag.vdenc_stats_head[3],
                    diag.slice_size_head[0],
                    diag.slice_size_head[1],
                    diag.slice_size_head[2],
                    diag.slice_size_head[3],
                );
            }
        }
    } else {
        crate::log_error!(target: "intel/media-encode";
            "intel/media-encode: avc-idr-probe accepted=0 state=deferred failure=guc-vcs0-transport-probe-unavailable surface_uploaded=0 codec_packets=0 submitted=0 retired=0 coded_output_validated=0 hardware_encode=0 software_fallback=0 action=park\n",
        );
    }

    let readiness = crate::intel::media_encode_readiness();
    crate::log_info!(target: "intel/media-encode";
        "intel/media-encode: hardware-readiness ready={} device_claimed={} vdbox_discovered={} guc_transport_ready={} guc_media_context_wired={} guc_media_transport_probe_passed={} avc_encode_commands_wired={} avc_encode_probe_passed={} coded_bitstream_output_wired={} decode_transport=guc-fuse-selected encode_transport=guc-vcs0 direct_execlist_live_avc=0 filesystem_writes=0 software_fallback=0\n",
        readiness.ready as u8,
        readiness.device_claimed as u8,
        readiness.vdbox_discovered as u8,
        readiness.guc_transport_ready as u8,
        readiness.guc_media_context_wired as u8,
        readiness.guc_media_transport_probe_passed as u8,
        readiness.avc_encode_commands_wired as u8,
        readiness.avc_encode_probe_passed as u8,
        readiness.coded_bitstream_output_wired as u8,
    );

    if !readiness.ready {
        STATE.store(H264EncodeStreamState::Failed as u8, Ordering::Release);
        crate::log_error!(target: "intel/media-encode";
            "intel/media-encode: stream rejected stage=hardware-readiness reason=required-gate-failed filesystem_writes=0 software_fallback=0\n",
        );
        park().await;
    }

    let avc_probe = crate::intel::media::avc_encode_probe::snapshot();
    STATE.store(H264EncodeStreamState::Encoding as u8, Ordering::Release);
    let Some(probe_annex_b) = crate::intel::media::avc_encode_probe::take_coded_access_unit()
    else {
        STATE.store(H264EncodeStreamState::Failed as u8, Ordering::Release);
        crate::log_error!(target: "intel/media-encode";
            "intel/media-encode: stream rejected stage=coded-au-handoff reason=validated-access-unit-unavailable filesystem_writes=0 software_fallback=0\n",
        );
        park().await;
    };
    ENCODE_US.store(avc_probe.elapsed_us, Ordering::Release);
    SOURCE_BYTES.store(avc_probe.source_nv12_bytes, Ordering::Release);
    ENCODED_BYTES.store(probe_annex_b.len(), Ordering::Release);
    drop(probe_annex_b);

    let timestamp = crate::chronos::best_effort_unix_time_seconds();
    let mut stream_session_id = timestamp
        .map(|timestamp| timestamp as u32)
        .unwrap_or_else(|| (crate::time::uptime_seconds() as u32) ^ (vcs0_probe.serial as u32));
    loop {
        STATE.store(H264EncodeStreamState::Streaming as u8, Ordering::Release);
        let mut stats = LiveEncodeStats::default();
        let access_unit_count = crate::allcaps::media_encode::VALIDATION_SESSION_ACCESS_UNITS;
        let udp_report = super::h264_encode_udp::stream_generated_annex_b(
            stream_session_id,
            access_unit_count,
            crate::allcaps::media_encode::REALTIME_HZ,
            || begin_preparation_session(stream_session_id, access_unit_count),
            |sequence| prepared_scanout_ready(stream_session_id, sequence),
            async |sequence| encode_prepared_scanout(stream_session_id, sequence, &mut stats).await,
        )
        .await;
        end_preparation_session(stream_session_id);
        let expected_units = crate::allcaps::media_encode::VALIDATION_SESSION_ACCESS_UNITS;
        let interval_millifps = if udp_report.sent_access_units > 1 && udp_report.elapsed_us != 0 {
            (udp_report.sent_access_units.saturating_sub(1) as u64).saturating_mul(1_000_000_000)
                / udp_report.elapsed_us
        } else {
            0
        };
        let target_millifps = crate::allcaps::media_encode::REALTIME_HZ as u64 * 1_000;
        let cadence_tolerance = target_millifps.saturating_mul(CADENCE_TOLERANCE_PERCENT) / 100;
        // Delivery success and cadence quality are deliberately separate.
        // Recovered adapter backpressure or a missed real-time deadline does
        // not invalidate an access unit that reached the subscribed viewer.
        let delivered = udp_report.queued_access_units == expected_units
            && udp_report.sent_access_units == expected_units
            && udp_report.dropped_access_units == 0
            && udp_report.adapter_send_errors == 0
            && stats.frames == expected_units;
        let cadence_target_met = udp_report.late_access_units == 0
            && interval_millifps >= target_millifps.saturating_sub(cadence_tolerance)
            && interval_millifps <= target_millifps.saturating_add(cadence_tolerance);
        let retry_free =
            udp_report.submit_retries == 0 && udp_report.adapter_backpressure_events == 0;
        STATE.store(
            if delivered {
                H264EncodeStreamState::Verified
            } else {
                H264EncodeStreamState::Failed
            } as u8,
            Ordering::Release,
        );
        crate::log_info!(target: "intel/media-encode";
            "intel/media-encode: udp-live complete accepted={} delivery_complete={} cadence_target_met={} retry_free={} source=ui4-logical-scanout-d01 source_size={}x{} encode_size={}x{} mapping=native-identity conversion=guc-rcs-cpp-linear-rgba8-to-nv12 cpu_pixel_math=0 cpu_composition=opaque-zcopy-frontmost-wins-no-alpha dma_buffers=persistent vdbox_source=gpu-dma-direct cpu_nv12_copy_bytes=0 active_size={}x{} padding=top:0,bottom:0 slot0_base=pipe-a-plane0-surflive-primary-rgba-premultiplied slot0_windows=hardcoded-none slot0_scanout_frames={} slot0_scanout_pixels={} spirit_overlay=pipe-a-cur-surflive-bgra-binary-mask spirit_overlay_frames={} spirit_overlay_pixels={} synchronization=rcs-completion-before-vdbox format=nv12 target_fps={} measured_millifps={} backend=gen12-vdenc-mfx engine=vcs0 submission_owner=guc direct_execlist_submit=0 hardware_encode=1 gop=idr+p gop_pictures={} idr_units={} p_units={} protocol=tme1 version=1 egress_path=smoltcp-borrowed-direct-nic-dma-fill session={} queued_units={} sent_units={} sent_datagrams={} sent_payload_bytes={} dropped_units={} dropped_bytes={} high_water_units={} high_water_bytes={} producer_queue_wait_events={} producer_queue_wait_us={} submit_retries={} adapter_backpressure_events={} adapter_send_errors={} network_waits={} subscriber_wait_polls={} late_units={} max_late_us={} elapsed_us={} source_first_sampled_fnv1a32=0x{:08X} source_last_sampled_fnv1a32=0x{:08X} source_changes={} capture_avg_us={} capture_max_us={} convert_wall_avg_us={} convert_wall_max_us={} convert_gpu_avg_us={} convert_gpu_max_us={} encode_avg_us={} encode_max_us={} coded_avg_bytes={} coded_max_bytes={} peer={}.{}.{}.{}:{} bounded_seconds={} pipeline=prepare+encode+independent-egress buffering=double+bounded-au-queue prepare_slots={} prepare_worker_slot={} encode_worker_slot={} encode_worker_kind={} egress_worker_slot={} filesystem_writes=0 software_fallback=0 surflive_payload=0\n",
            delivered as u8,
            delivered as u8,
            cadence_target_met as u8,
            retry_free as u8,
            stats.source_width,
            stats.source_height,
            ENCODE_WIDTH,
            ENCODE_HEIGHT,
            ENCODE_WIDTH,
            ACTIVE_HEIGHT,
            stats.slot0_scanout_frames,
            stats.slot0_scanout_pixels,
            stats.spirit_overlay_frames,
            stats.spirit_overlay_pixels,
            crate::allcaps::media_encode::REALTIME_HZ,
            interval_millifps,
            crate::intel::media::avc_encode_probe::GOP_PICTURES,
            stats.idr_frames,
            stats.p_frames,
            udp_report.session_id,
            udp_report.queued_access_units,
            udp_report.sent_access_units,
            udp_report.sent_datagrams,
            udp_report.sent_payload_bytes,
            udp_report.dropped_access_units,
            udp_report.dropped_bytes,
            udp_report.high_water_access_units,
            udp_report.high_water_bytes,
            udp_report.producer_queue_wait_events,
            udp_report.producer_queue_wait_us,
            udp_report.submit_retries,
            udp_report.adapter_backpressure_events,
            udp_report.adapter_send_errors,
            udp_report.network_waits,
            udp_report.subscriber_wait_polls,
            udp_report.late_access_units,
            udp_report.max_late_us,
            udp_report.elapsed_us,
            stats.first_source_fnv1a32,
            stats.last_source_fnv1a32,
            stats.source_changes,
            average_u64(stats.capture_us, stats.frames),
            stats.capture_max_us,
            average_u64(stats.convert_wall_us, stats.frames),
            stats.convert_wall_max_us,
            average_u64(stats.convert_gpu_us, stats.frames),
            stats.convert_gpu_max_us,
            average_u64(stats.encode_us, stats.frames),
            stats.encode_max_us,
            average_usize(stats.coded_bytes, stats.frames),
            stats.coded_max_bytes,
            udp_report.peer_addr[0],
            udp_report.peer_addr[1],
            udp_report.peer_addr[2],
            udp_report.peer_addr[3],
            udp_report.peer_port,
            crate::allcaps::media_encode::VALIDATION_SESSION_SECONDS,
            PREPARE_SLOT_COUNT,
            PREPARE_WORKER_SLOT.load(Ordering::Acquire),
            worker_slot,
            worker_kind,
            super::h264_encode_udp::egress_worker_slot(),
        );

        if !delivered {
            let snapshot = h264_encode_stream_snapshot();
            crate::log_error!(target: "intel/media-encode";
                "intel/media-encode: live session rejected state={:?} encode_us={} source_bytes={} encoded_bytes={} queued_units={} sent_units={} dropped_units={} adapter_send_errors={} reason=delivery-incomplete action=return-to-subscriber-wait filesystem_writes=0 software_fallback=0\n",
                snapshot.state,
                snapshot.encode_us,
                snapshot.source_bytes,
                snapshot.encoded_bytes,
                udp_report.queued_access_units,
                udp_report.sent_access_units,
                udp_report.dropped_access_units,
                udp_report.adapter_send_errors,
            );
            Timer::after(Duration::from_millis(250)).await;
        }
        stream_session_id = stream_session_id.wrapping_add(1);
    }
}

async fn encode_prepared_scanout(
    session_id: u32,
    sequence: u32,
    stats: &mut LiveEncodeStats,
) -> Option<Vec<u8>> {
    let mut prepared = take_prepared_scanout(session_id, sequence)?;
    if !prepared.valid {
        release_prepared_scanout(&mut prepared);
        return None;
    }

    STATE.store(H264EncodeStreamState::Encoding as u8, Ordering::Release);
    let Some(nv12) = prepared.nv12.as_ref() else {
        release_prepared_scanout(&mut prepared);
        STATE.store(H264EncodeStreamState::Streaming as u8, Ordering::Release);
        return None;
    };
    let Some(surface) =
        crate::intel::media::avc_encode_probe::AvcNv12DmaSurface::new(nv12.phys, nv12.bytes)
    else {
        release_prepared_scanout(&mut prepared);
        STATE.store(H264EncodeStreamState::Streaming as u8, Ordering::Release);
        return None;
    };
    let encode = crate::intel::media::avc_encode_probe::run_nv12_dma_frame(surface, sequence).await;
    STATE.store(H264EncodeStreamState::Streaming as u8, Ordering::Release);
    release_prepared_scanout(&mut prepared);
    if encode.state != crate::intel::media::avc_encode_probe::AvcEncodeProbeState::Passed
        || !encode.coded_output_validated
    {
        crate::log_error!(target: "intel/media-encode";
            "intel/media-encode: live frame rejected session={} sequence={} stage=hardware-encode idr={} frame_num={} state={:?} failure={} elapsed_us={} coded_bytes={} mfx_error=0x{:08X} software_fallback=0\n",
            session_id,
            sequence,
            encode.idr_picture as u8,
            encode.frame_num,
            encode.state,
            encode.failure.name(),
            encode.elapsed_us,
            encode.coded_bytes,
            encode.mfx_error,
        );
        if encode.failure
            == crate::intel::media::avc_encode_probe::AvcEncodeProbeFailure::CompletionTimeout
        {
            let diag = encode.timeout_diagnostics;
            crate::log_error!(target: "intel/media-encode";
                "intel/media-encode: live timeout session={} sequence={} markers=0x{:08X}/0x{:08X}/0x{:08X}/0x{:08X} acthd=0x{:08X}:0x{:08X} region={} offset=0x{:X} dword=0x{:08X} ipehr=0x{:08X} instdone=0x{:08X} mfx_error=0x{:08X} mb_count=0x{:08X} frame_bytes=0x{:08X} slice_bytes=0x{:08X} image_control=0x{:08X} slices=0x{:08X} bitstream_head={:08X}/{:08X}/{:08X}/{:08X} mfx_stats={:08X}/{:08X}/{:08X}/{:08X} vdenc_stats={:08X}/{:08X}/{:08X}/{:08X} surface_samples=current_recon:0x{:08X},reference_recon:0x{:08X},current_ds:0x{:08X},reference_ds:0x{:08X}\n",
                session_id,
                sequence,
                encode.kickoff,
                encode.codec_begin,
                encode.codec_end,
                encode.complete,
                diag.ring_acthd_hi,
                diag.ring_acthd_lo,
                diag.acthd_region,
                diag.acthd_offset_bytes,
                diag.acthd_dword,
                diag.ipehr,
                diag.instdone,
                diag.mfx_error,
                diag.mfx_mb_count,
                diag.mfc_bitstream_bytecount_frame,
                diag.mfc_bitstream_bytecount_slice,
                diag.mfc_image_status_control,
                diag.mfc_avc_num_slices,
                diag.bitstream_head[0],
                diag.bitstream_head[1],
                diag.bitstream_head[2],
                diag.bitstream_head[3],
                diag.mfx_stats_head[0],
                diag.mfx_stats_head[1],
                diag.mfx_stats_head[2],
                diag.mfx_stats_head[3],
                diag.vdenc_stats_head[0],
                diag.vdenc_stats_head[1],
                diag.vdenc_stats_head[2],
                diag.vdenc_stats_head[3],
                diag.current_recon_sample,
                diag.reference_recon_sample,
                diag.current_ds_sample,
                diag.reference_ds_sample,
            );
        }
        return None;
    }
    let Some(annex_b) = crate::intel::media::avc_encode_probe::take_coded_access_unit() else {
        crate::log_error!(target: "intel/media-encode";
            "intel/media-encode: live frame rejected sequence={} stage=coded-au-handoff reason=validated-access-unit-unavailable software_fallback=0\n",
            sequence,
        );
        return None;
    };

    if stats.frames == 0 {
        stats.first_source_fnv1a32 = prepared.source_fnv1a32;
    } else if stats.last_source_fnv1a32 != prepared.source_fnv1a32 {
        stats.source_changes = stats.source_changes.saturating_add(1);
    }
    stats.frames = stats.frames.saturating_add(1);
    if encode.idr_picture {
        stats.idr_frames = stats.idr_frames.saturating_add(1);
    } else {
        stats.p_frames = stats.p_frames.saturating_add(1);
    }
    stats.source_width = prepared.source_width;
    stats.source_height = prepared.source_height;
    stats.last_source_fnv1a32 = prepared.source_fnv1a32;
    stats.capture_us = stats.capture_us.saturating_add(prepared.capture_us);
    stats.capture_max_us = stats.capture_max_us.max(prepared.capture_us);
    stats.convert_wall_us = stats
        .convert_wall_us
        .saturating_add(prepared.convert_wall_us);
    stats.convert_wall_max_us = stats.convert_wall_max_us.max(prepared.convert_wall_us);
    stats.convert_gpu_us = stats.convert_gpu_us.saturating_add(prepared.convert_gpu_us);
    stats.convert_gpu_max_us = stats.convert_gpu_max_us.max(prepared.convert_gpu_us);
    if prepared.slot0_scanout_pixels != 0 {
        stats.slot0_scanout_frames = stats.slot0_scanout_frames.saturating_add(1);
        stats.slot0_scanout_pixels = stats
            .slot0_scanout_pixels
            .saturating_add(prepared.slot0_scanout_pixels);
    }
    if prepared.spirit_overlay_pixels != 0 {
        stats.spirit_overlay_frames = stats.spirit_overlay_frames.saturating_add(1);
        stats.spirit_overlay_pixels = stats
            .spirit_overlay_pixels
            .saturating_add(prepared.spirit_overlay_pixels);
    }
    stats.encode_us = stats.encode_us.saturating_add(encode.elapsed_us);
    stats.encode_max_us = stats.encode_max_us.max(encode.elapsed_us);
    stats.coded_bytes = stats.coded_bytes.saturating_add(annex_b.len());
    stats.coded_max_bytes = stats.coded_max_bytes.max(annex_b.len());
    ENCODE_US.store(encode.elapsed_us, Ordering::Release);
    SOURCE_BYTES.store(ENCODE_NV12_BYTES, Ordering::Release);
    ENCODED_BYTES.store(annex_b.len(), Ordering::Release);
    Some(annex_b)
}

fn fnv1a32_sampled(bytes: &[u8]) -> u32 {
    const MAX_SAMPLES: usize = 4096;

    let mut hash = 0x811c_9dc5u32;
    let samples = bytes.len().min(MAX_SAMPLES);
    if samples == 0 {
        return hash;
    }
    for sample in 0..samples {
        let offset =
            sample.saturating_mul(bytes.len().saturating_sub(1)) / samples.saturating_sub(1).max(1);
        hash ^= u32::from(bytes[offset]);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash ^= bytes.len() as u32;
    hash = hash.wrapping_mul(0x0100_0193);
    hash
}

fn average_u64(total: u64, count: usize) -> u64 {
    total / count.max(1) as u64
}

fn average_usize(total: usize, count: usize) -> usize {
    total / count.max(1)
}

async fn park() -> ! {
    loop {
        Timer::after(Duration::from_secs(3_600)).await;
    }
}
