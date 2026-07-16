//! Production UI3 logical-frame compositor.
//!
//! This module is deliberately separate from the experimental `ui3_frame`
//! CABI.  A producer owns only a double-buffered logical BGRA8-premultiplied
//! frame.  UI3 is the sole owner of the full-screen display back buffer and
//! the only layer allowed to commit it to scanout.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::intel::gpgpu::{
    AlphaBlendWorklistRgba8Desc, COMPOSITE_WORKLIST_FLAG_PREMUL_SRC,
    COMPOSITE_WORKLIST_FLAG_SRC_OVER, COMPOSITE_WORKLIST_FLAG_TINT_ALPHA,
    COMPOSITE_WORKLIST_NEUTRAL_COLOR_RGBA, FillRectWorklistRgba8Desc, GpgpuPoint, GpgpuRect,
    GpgpuRgba8Surface, GpgpuSpriteQuadWorklistDesc,
};

pub(crate) const UI3_OUTPUT_COUNT: usize = 4;
const UI3_FRAME_COUNT: usize = 2;
const UI3_FRAME_BUFFER_COUNT: usize = 2;
const UI3_FRAME_GPU_BASE: u64 = 0x2900_0000;
const UI3_FRAME_GPU_STRIDE: u64 = 0x0100_0000;
const UI3_SCALE_SCRATCH_GPU: u64 = 0x2D00_0000;
const UI3_COMPOSITOR_GPU_LIMIT: u64 = 0x2E00_0000;
const UI3_BLACK_BOX_FRAME_SIZE: u32 = 512;
const UI3_BLACK_BOX_SIZE: u32 = 160;
const UI3_WORKLIST_TILE_AXIS: u32 = 16;
const UI3_BLACK_BOX_OFFSET: u32 = (UI3_BLACK_BOX_FRAME_SIZE.saturating_sub(UI3_BLACK_BOX_SIZE)) / 2;

const _: () = assert!(
    UI3_FRAME_GPU_BASE + (UI3_FRAME_COUNT * UI3_FRAME_BUFFER_COUNT) as u64 * UI3_FRAME_GPU_STRIDE
        <= UI3_SCALE_SCRATCH_GPU
);
const _: () = assert!(UI3_SCALE_SCRATCH_GPU + UI3_FRAME_GPU_STRIDE <= UI3_COMPOSITOR_GPU_LIMIT);
const _: () = assert!(UI3_COMPOSITOR_GPU_LIMIT <= 0x3000_0000);

static STATE: Mutex<CompositorState> = Mutex::new(CompositorState::new());
static COMPOSE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static COMPOSITION_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static LAST_COMPOSE_LAYER_COUNT: AtomicU64 = AtomicU64::new(u64::MAX);

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(u8)]
pub(crate) enum Ui3FrameId {
    Draw3dScene = 0,
    CompositorProof = 1,
}

impl Ui3FrameId {
    const fn index(self) -> usize {
        self as usize
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Draw3dScene => "draw3d-scene",
            Self::CompositorProof => "ui3-proof-512",
        }
    }

    const fn gpu_slot_base(self) -> usize {
        self.index() * UI3_FRAME_BUFFER_COUNT
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui3FrameFormat {
    /// Bytes are B, G, R, A and RGB is already multiplied by A.
    Bgra8Premultiplied,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub(crate) struct Ui3OutputId(u8);

impl Ui3OutputId {
    pub(crate) const PRIMARY: Self = Self(0);

    pub(crate) const fn from_slot(slot: usize) -> Option<Self> {
        if slot < UI3_OUTPUT_COUNT {
            Some(Self(slot as u8))
        } else {
            None
        }
    }

    pub(crate) const fn slot(self) -> usize {
        self.0 as usize
    }

    pub(crate) const fn name(self) -> &'static str {
        match self.0 {
            0 => "D01",
            1 => "D02",
            2 => "D03",
            3 => "D04",
            _ => "D-invalid",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui3FramePlacement {
    pub(crate) output: Ui3OutputId,
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) opacity: u8,
    pub(crate) z: i16,
    pub(crate) visible: bool,
}

impl Ui3FramePlacement {
    const fn inactive(output: Ui3OutputId, z: i16) -> Self {
        Self {
            output,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            opacity: 255,
            z,
            visible: false,
        }
    }
}

#[derive(Copy, Clone)]
struct SurfaceStorage {
    surface: GpgpuRgba8Surface,
    virt: *mut u8,
}

unsafe impl Send for SurfaceStorage {}
unsafe impl Sync for SurfaceStorage {}

struct FrameState {
    id: Ui3FrameId,
    buffers: [Option<SurfaceStorage>; UI3_FRAME_BUFFER_COUNT],
    front: Option<usize>,
    acquired: Option<(usize, u64)>,
    next_generation: u64,
    placement: Ui3FramePlacement,
}

impl FrameState {
    const fn new(id: Ui3FrameId, placement: Ui3FramePlacement) -> Self {
        Self {
            id,
            buffers: [None; UI3_FRAME_BUFFER_COUNT],
            front: None,
            acquired: None,
            next_generation: 0,
            placement,
        }
    }
}

struct CompositorState {
    frames: [FrameState; UI3_FRAME_COUNT],
    proof_initializing: bool,
    scale_scratch: Option<SurfaceStorage>,
}

impl CompositorState {
    const fn new() -> Self {
        Self {
            frames: [
                FrameState::new(
                    Ui3FrameId::Draw3dScene,
                    Ui3FramePlacement::inactive(Ui3OutputId::PRIMARY, 0),
                ),
                FrameState::new(
                    Ui3FrameId::CompositorProof,
                    Ui3FramePlacement {
                        output: Ui3OutputId::PRIMARY,
                        x: 64,
                        y: 64,
                        width: UI3_BLACK_BOX_FRAME_SIZE,
                        height: UI3_BLACK_BOX_FRAME_SIZE,
                        opacity: 255,
                        z: 100,
                        visible: true,
                    },
                ),
            ],
            proof_initializing: false,
            scale_scratch: None,
        }
    }
}

/// A non-copyable producer lease for one logical frame back buffer.
pub(crate) struct Ui3FrameWriteLease {
    frame: Ui3FrameId,
    buffer: usize,
    generation: u64,
    surface: GpgpuRgba8Surface,
    active: bool,
}

impl Ui3FrameWriteLease {
    pub(crate) const fn surface(&self) -> GpgpuRgba8Surface {
        self.surface
    }

    pub(crate) const fn format(&self) -> Ui3FrameFormat {
        Ui3FrameFormat::Bgra8Premultiplied
    }
}

impl Drop for Ui3FrameWriteLease {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut state = STATE.lock();
        let frame = &mut state.frames[self.frame.index()];
        if frame.acquired == Some((self.buffer, self.generation)) {
            frame.acquired = None;
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct Ui3CompositionResult {
    pub(crate) presented: bool,
    pub(crate) output: Ui3OutputId,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) layers: usize,
    pub(crate) clear_us: u64,
    pub(crate) draw3d_us: u64,
    pub(crate) proof_us: u64,
    pub(crate) commit_us: u64,
    pub(crate) total_us: u64,
}

#[derive(Copy, Clone)]
struct LayerSnapshot {
    id: Ui3FrameId,
    surface: GpgpuRgba8Surface,
    placement: Ui3FramePlacement,
}

#[derive(Copy, Clone, Debug, Default)]
struct LayerCompositionTiming {
    scratch_clear_us: u64,
    scale_us: u64,
    blend_us: u64,
}

#[derive(Copy, Clone, Debug)]
struct ProofSourceReadback {
    exact: bool,
    transparent: u32,
    box_first: u32,
    box_center: u32,
    box_last: u32,
    far_transparent: u32,
}

impl LayerCompositionTiming {
    const fn total_us(self) -> u64 {
        self.scratch_clear_us
            .saturating_add(self.scale_us)
            .saturating_add(self.blend_us)
    }
}

struct ComposeGuard;

impl ComposeGuard {
    fn acquire() -> Result<Self, &'static str> {
        COMPOSE_IN_FLIGHT
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| "ui3-compose-in-flight")
    }
}

impl Drop for ComposeGuard {
    fn drop(&mut self) {
        COMPOSE_IN_FLIGHT.store(false, Ordering::Release);
    }
}

/// Bring the production UI3 compositor online independently of every frame
/// producer.  D01 initially contains only the 512x512 proof layer; Draw3D can
/// publish its first logical frame later without owning compositor lifetime.
pub(crate) fn bootstrap_primary_output() -> Result<Ui3CompositionResult, &'static str> {
    ensure_compositor_proof()?;
    let result = compose_output(Ui3OutputId::PRIMARY)?;
    if !result.presented {
        return Err("ui3-bootstrap-present");
    }
    Ok(result)
}

/// Independent UI3 service bootstrap.  The task is gated by the Intel
/// presentation readiness bit and runs on AP1 with the other UI work.  A
/// transient route or GPU-busy failure is retried without involving TCP or a
/// scene command.
#[embassy_executor::task]
pub(crate) async fn ui3_compositor_task() {
    const RETRY_MS: u64 = 250;
    let mut attempt = 0u64;
    loop {
        attempt = attempt.saturating_add(1);
        match bootstrap_primary_output() {
            Ok(result) => {
                crate::log_info!(
                    target: "ui3";
                    "ui3-compositor: bootstrap complete=1 attempt={} output={} size={}x{} layers={} presented=1 producer_dependency=none tcp_dependency=none milestone=proof-only\n",
                    attempt,
                    result.output.name(),
                    result.width,
                    result.height,
                    result.layers,
                );
                return;
            }
            Err(error) => {
                if attempt == 1 || attempt.is_multiple_of(20) {
                    crate::log_warn!(
                        target: "ui3";
                        "ui3-compositor: bootstrap complete=0 attempt={} potential_reason={} action=retry retry_ms={} producer_dependency=none tcp_dependency=none\n",
                        attempt,
                        error,
                        RETRY_MS,
                    );
                }
            }
        }
        Timer::after(EmbassyDuration::from_millis(RETRY_MS)).await;
    }
}

/// Acquire the current D01-sized Draw3D logical back buffer.
pub(crate) fn acquire_draw3d_scene_frame() -> Result<Ui3FrameWriteLease, &'static str> {
    let output = crate::intel::ui3_compositor_output_target(Ui3OutputId::PRIMARY.slot())
        .ok_or("ui3-primary-output-unavailable")?;
    let mut state = STATE.lock();
    let frame = &mut state.frames[Ui3FrameId::Draw3dScene.index()];
    ensure_frame_buffers(frame, output.width, output.height)?;
    if frame.acquired.is_some() {
        return Err("ui3-scene-frame-already-acquired");
    }
    if frame.placement.width == 0 || frame.placement.height == 0 {
        frame.placement.x = 0;
        frame.placement.y = 0;
        frame.placement.width = output.width;
        frame.placement.height = output.height;
        frame.placement.output = Ui3OutputId::PRIMARY;
        frame.placement.opacity = 255;
        frame.placement.z = 0;
    }
    let buffer = match frame.front {
        Some(front) => (front + 1) % UI3_FRAME_BUFFER_COUNT,
        None => 0,
    };
    let surface = frame.buffers[buffer]
        .ok_or("ui3-scene-frame-buffer-unavailable")?
        .surface;
    frame.next_generation = frame.next_generation.wrapping_add(1).max(1);
    let generation = frame.next_generation;
    frame.acquired = Some((buffer, generation));
    Ok(Ui3FrameWriteLease {
        frame: Ui3FrameId::Draw3dScene,
        buffer,
        generation,
        surface,
        active: true,
    })
}

/// Publish a GPU-retired Draw3D source frame, then ask UI3 to build and
/// present one complete output frame.  Publishing and presentation are
/// separate facts: a failed output composition retains both the new logical
/// front and the display's previous complete front for a later retry.
pub(crate) fn commit_draw3d_scene_frame(
    mut lease: Ui3FrameWriteLease,
) -> Result<Ui3CompositionResult, &'static str> {
    if lease.frame != Ui3FrameId::Draw3dScene {
        return Err("ui3-scene-frame-lease-kind");
    }
    let output = {
        let mut state = STATE.lock();
        let frame = &mut state.frames[lease.frame.index()];
        if frame.acquired != Some((lease.buffer, lease.generation)) {
            return Err("ui3-scene-frame-stale-lease");
        }
        frame.front = Some(lease.buffer);
        frame.acquired = None;
        frame.placement.visible = true;
        frame.placement.output
    };
    lease.active = false;
    ensure_compositor_proof()?;
    compose_output(output)
}

pub(crate) fn discard_draw3d_scene_frame(mut lease: Ui3FrameWriteLease) -> bool {
    let discarded = {
        let mut state = STATE.lock();
        let frame = &mut state.frames[lease.frame.index()];
        if frame.acquired == Some((lease.buffer, lease.generation)) {
            frame.acquired = None;
            true
        } else {
            false
        }
    };
    lease.active = false;
    discarded
}

/// Permanent Draw3D reset removes only the scene layer.  UI3 and its proof
/// frame remain alive because display ownership belongs to the compositor,
/// not to the scene service.
pub(crate) fn reset_draw3d_scene_frame() -> bool {
    let output = {
        let mut state = STATE.lock();
        let frame = &mut state.frames[Ui3FrameId::Draw3dScene.index()];
        frame.front = None;
        frame.acquired = None;
        frame.next_generation = frame.next_generation.wrapping_add(1);
        frame.placement.visible = false;
        frame.placement.output
    };
    match ensure_compositor_proof().and_then(|_| compose_output(output)) {
        Ok(result) => result.presented,
        Err(error) => {
            crate::log_warn!(
                target: "ui3";
                "ui3-compositor: scene reset retained previous output potential_reason={} action=retry-on-next-frame\n",
                error,
            );
            false
        }
    }
}

/// Reconfigure any production UI3 layer.  Moving between outputs recomposes
/// the old output first and the new output second; each output independently
/// retains its last complete front if the route changes mid-transaction.
#[allow(dead_code)]
pub(crate) fn configure_frame(
    id: Ui3FrameId,
    placement: Ui3FramePlacement,
) -> Result<bool, &'static str> {
    if placement.width == 0 || placement.height == 0 {
        return Err("ui3-frame-placement-empty");
    }
    if placement.output.slot() >= UI3_OUTPUT_COUNT {
        return Err("ui3-frame-output-range");
    }
    let (old_output, has_front) = {
        let mut state = STATE.lock();
        let frame = &mut state.frames[id.index()];
        let old_output = frame.placement.output;
        frame.placement = placement;
        (old_output, frame.front.is_some())
    };
    if !has_front {
        return Ok(true);
    }
    ensure_compositor_proof()?;
    let old_ok = compose_output(old_output)
        .map(|result| result.presented)
        .unwrap_or(false);
    if old_output == placement.output {
        return Ok(old_ok);
    }
    Ok(old_ok && compose_output(placement.output)?.presented)
}

fn ensure_compositor_proof() -> Result<(), &'static str> {
    let (surface, virt) = {
        let mut state = STATE.lock();
        if state.frames[Ui3FrameId::CompositorProof.index()]
            .front
            .is_some()
        {
            return Ok(());
        }
        if state.proof_initializing {
            return Err("ui3-proof-initializing");
        }
        let frame = &mut state.frames[Ui3FrameId::CompositorProof.index()];
        ensure_frame_buffers(frame, UI3_BLACK_BOX_FRAME_SIZE, UI3_BLACK_BOX_FRAME_SIZE)?;
        let storage = frame.buffers[0].ok_or("ui3-proof-buffer-unavailable")?;
        state.proof_initializing = true;
        (storage.surface, storage.virt)
    };

    // Two ordered GPU submissions are intentional.  Overlapping fill
    // descriptors in one worklist can race between workgroups.
    let proof_started_ns = crate::chronos::monotonic_nanos();
    let clear_ok = parallel_fill_rect(
        surface,
        GpgpuRect::new(0, 0, surface.width, surface.height),
        0x0000_0000,
    );
    let clear_complete_ns = crate::chronos::monotonic_nanos();
    let box_ok = clear_ok
        && parallel_fill_rect(
            surface,
            GpgpuRect::new(
                UI3_BLACK_BOX_OFFSET as i32,
                UI3_BLACK_BOX_OFFSET as i32,
                UI3_BLACK_BOX_SIZE,
                UI3_BLACK_BOX_SIZE,
            ),
            0xFF00_0000,
        );
    let box_complete_ns = crate::chronos::monotonic_nanos();
    let source_proof = box_ok.then(|| verify_proof_source(surface, virt));

    let mut state = STATE.lock();
    state.proof_initializing = false;
    if !box_ok {
        return Err("ui3-proof-gpu-fill");
    }
    let source_proof = source_proof.ok_or("ui3-proof-source-readback")?;
    crate::log_info!(
        target: "ui3";
        "ui3-compositor: proof source exact={} transparent=0x{:08X} box_first=0x{:08X} box_center=0x{:08X} box_last=0x{:08X} far_transparent=0x{:08X} expected_transparent=0x00000000 expected_box=0xFF000000 clear_us={} box_us={} verify_us={} cpu_readback=diagnostic-only cpu_pixel_path=none\n",
        source_proof.exact as u8,
        source_proof.transparent,
        source_proof.box_first,
        source_proof.box_center,
        source_proof.box_last,
        source_proof.far_transparent,
        elapsed_us(proof_started_ns, clear_complete_ns),
        elapsed_us(clear_complete_ns, box_complete_ns),
        elapsed_us(box_complete_ns, crate::chronos::monotonic_nanos()),
    );
    if !source_proof.exact {
        return Err("ui3-proof-source-mismatch");
    }
    let frame = &mut state.frames[Ui3FrameId::CompositorProof.index()];
    frame.front = Some(0);
    frame.placement.visible = true;
    crate::log_info!(
        target: "ui3";
        "ui3-compositor: frame online id={} format=bgra8-premultiplied content={}x{} output={} dst={}x{}+{}+{} z={} content_proof=opaque-black-box-{}x{} producer=gpu-only\n",
        frame.id.name(),
        surface.width,
        surface.height,
        frame.placement.output.name(),
        frame.placement.width,
        frame.placement.height,
        frame.placement.x,
        frame.placement.y,
        frame.placement.z,
        UI3_BLACK_BOX_SIZE,
        UI3_BLACK_BOX_SIZE,
    );
    Ok(())
}

fn verify_proof_source(surface: GpgpuRgba8Surface, virt: *mut u8) -> ProofSourceReadback {
    const INVALID: u32 = 0xDEAD_BEEF;
    let sample = |x: u32, y: u32| -> u32 {
        let Some(row) = (y as usize).checked_mul(surface.pitch_bytes as usize) else {
            return INVALID;
        };
        let Some(column) = (x as usize).checked_mul(core::mem::size_of::<u32>()) else {
            return INVALID;
        };
        let Some(offset) = row.checked_add(column) else {
            return INVALID;
        };
        if x >= surface.width
            || y >= surface.height
            || offset.saturating_add(core::mem::size_of::<u32>()) > surface.bytes
        {
            return INVALID;
        }
        let pixel = unsafe { virt.add(offset) };
        // The GPU submission has retired.  Invalidate this diagnostic cache
        // line before CPU readback; presentation itself never consumes CPU
        // pixels or depends on this result.
        crate::intel::dma_flush(pixel, core::mem::size_of::<u32>());
        unsafe { core::ptr::read_volatile(pixel.cast::<u32>()) }
    };
    let box_last = UI3_BLACK_BOX_OFFSET + UI3_BLACK_BOX_SIZE - 1;
    let transparent = sample(0, 0);
    let box_first = sample(UI3_BLACK_BOX_OFFSET, UI3_BLACK_BOX_OFFSET);
    let box_center = sample(
        UI3_BLACK_BOX_OFFSET + UI3_BLACK_BOX_SIZE / 2,
        UI3_BLACK_BOX_OFFSET + UI3_BLACK_BOX_SIZE / 2,
    );
    let box_last = sample(box_last, box_last);
    let far_transparent = sample(surface.width.saturating_sub(1), surface.height.saturating_sub(1));
    ProofSourceReadback {
        exact: transparent == 0x0000_0000
            && box_first == 0xFF00_0000
            && box_center == 0xFF00_0000
            && box_last == 0xFF00_0000
            && far_transparent == 0x0000_0000,
        transparent,
        box_first,
        box_center,
        box_last,
        far_transparent,
    }
}

const fn elapsed_us(start_ns: u64, end_ns: u64) -> u64 {
    end_ns.saturating_sub(start_ns) / 1_000
}

fn compose_output(output: Ui3OutputId) -> Result<Ui3CompositionResult, &'static str> {
    let compose_started_ns = crate::chronos::monotonic_nanos();
    let _guard = ComposeGuard::acquire()?;
    let target = crate::intel::ui3_compositor_output_target(output.slot())
        .ok_or("ui3-output-route-unavailable")?;
    let mut layers = snapshot_layers(output);
    layers.sort_by_key(|layer| (layer.placement.z, layer.id));

    let output_frame =
        crate::intel::ui3_compositor_acquire_output(target).ok_or("ui3-output-acquire")?;
    let acquire_complete_ns = crate::chronos::monotonic_nanos();
    let dst = output_frame.surface;
    if dst.width != target.width || dst.height != target.height {
        let _ = crate::intel::ui3_compositor_discard_output(output_frame);
        return Err("ui3-output-shape-changed");
    }

    let clear_ok =
        parallel_fill_rect(dst, GpgpuRect::new(0, 0, dst.width, dst.height), 0x0000_0000);
    let clear_complete_ns = crate::chronos::monotonic_nanos();
    if !clear_ok {
        let _ = crate::intel::ui3_compositor_discard_output(output_frame);
        return Err("ui3-output-gpu-clear");
    }

    let mut draw3d_us = 0u64;
    let mut proof_us = 0u64;
    let mut scratch_clear_us = 0u64;
    let mut scale_us = 0u64;
    let mut blend_us = 0u64;
    for layer in &layers {
        let composed = match compose_layer(*layer, dst) {
            Ok(composed) => composed,
            Err(error) => {
                let _ = crate::intel::ui3_compositor_discard_output(output_frame);
                return Err(error);
            }
        };
        scratch_clear_us = scratch_clear_us.saturating_add(composed.scratch_clear_us);
        scale_us = scale_us.saturating_add(composed.scale_us);
        blend_us = blend_us.saturating_add(composed.blend_us);
        match layer.id {
            Ui3FrameId::Draw3dScene => {
                draw3d_us = draw3d_us.saturating_add(composed.total_us());
            }
            Ui3FrameId::CompositorProof => {
                proof_us = proof_us.saturating_add(composed.total_us());
            }
        }
    }

    let sequence = COMPOSITION_SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1;
    let layers_complete_ns = crate::chronos::monotonic_nanos();
    let presented = crate::intel::ui3_compositor_commit_output(output_frame, "ui3-compositor");
    let commit_complete_ns = crate::chronos::monotonic_nanos();
    let acquire_us = elapsed_us(compose_started_ns, acquire_complete_ns);
    let clear_us = elapsed_us(acquire_complete_ns, clear_complete_ns);
    let layers_us = elapsed_us(clear_complete_ns, layers_complete_ns);
    let commit_us = elapsed_us(layers_complete_ns, commit_complete_ns);
    let total_us = elapsed_us(compose_started_ns, commit_complete_ns);
    let layer_count_changed =
        LAST_COMPOSE_LAYER_COUNT.swap(layers.len() as u64, Ordering::AcqRel) != layers.len() as u64;
    if sequence <= 8 || sequence.is_multiple_of(60) || layer_count_changed || !presented {
        crate::log_info!(
            target: "ui3";
            "ui3-compositor: output frame seq={} output={} backend_output={} pipeline={} size={}x{} layers={} clear=gpu-worklist composition=gpu-premul present={} acquire_us={} clear_us={} layers_us={} draw3d_us={} proof_us={} scratch_clear_us={} scale_us={} blend_us={} commit_us={} total_us={} budget_us=16667 over_budget={} cpu_pixel_path=none fallback=none\n",
            sequence,
            output.name(),
            target.name,
            target.pipeline_name,
            target.width,
            target.height,
            layers.len(),
            presented as u8,
            acquire_us,
            clear_us,
            layers_us,
            draw3d_us,
            proof_us,
            scratch_clear_us,
            scale_us,
            blend_us,
            commit_us,
            total_us,
            (total_us > 16_667) as u8,
        );
    }
    Ok(Ui3CompositionResult {
        presented,
        output,
        width: target.width,
        height: target.height,
        layers: layers.len(),
        clear_us,
        draw3d_us,
        proof_us,
        commit_us,
        total_us,
    })
}

fn snapshot_layers(output: Ui3OutputId) -> Vec<LayerSnapshot> {
    let state = STATE.lock();
    let mut layers = Vec::with_capacity(UI3_FRAME_COUNT);
    for frame in &state.frames {
        let Some(front) = frame.front else {
            continue;
        };
        if !frame.placement.visible
            || frame.placement.opacity == 0
            || frame.placement.output != output
        {
            continue;
        }
        let Some(storage) = frame.buffers[front] else {
            continue;
        };
        layers.push(LayerSnapshot {
            id: frame.id,
            surface: storage.surface,
            placement: frame.placement,
        });
    }
    layers
}

fn compose_layer(
    layer: LayerSnapshot,
    dst: GpgpuRgba8Surface,
) -> Result<LayerCompositionTiming, &'static str> {
    if layer.placement.width == layer.surface.width
        && layer.placement.height == layer.surface.height
    {
        let blend_started_ns = crate::chronos::monotonic_nanos();
        if !blend_premultiplied_layer(layer.surface, layer.placement, dst)? {
            return Err("ui3-output-gpu-blend");
        }
        return Ok(LayerCompositionTiming {
            blend_us: elapsed_us(blend_started_ns, crate::chronos::monotonic_nanos()).max(1),
            ..LayerCompositionTiming::default()
        });
    }

    // The shipped SpriteQuad artifact is a straight-alpha compositor.  Use it
    // only as a COPY scaler into a transparent intermediate; this preserves
    // premultiplied bytes exactly.  The proven premultiplied worklist then
    // applies frame opacity and source-over to the real output.
    let scratch = ensure_scale_scratch(layer.placement.width, layer.placement.height)?;
    let scratch_clear_started_ns = crate::chronos::monotonic_nanos();
    if !parallel_fill_rect(
        scratch,
        GpgpuRect::new(0, 0, scratch.width, scratch.height),
        0x0000_0000,
    ) {
        return Err("ui3-output-gpu-scale-clear");
    }
    let scratch_clear_complete_ns = crate::chronos::monotonic_nanos();
    let width = scratch.width as f32;
    let height = scratch.height as f32;
    let desc = GpgpuSpriteQuadWorklistDesc {
        c0_x: 0.0,
        c0_y: 0.0,
        c0_u: 0.0,
        c0_v: 0.0,
        c1_x: width,
        c1_y: 0.0,
        c1_u: 1.0,
        c1_v: 0.0,
        c2_x: width,
        c2_y: height,
        c2_u: 1.0,
        c2_v: 1.0,
        c3_x: 0.0,
        c3_y: height,
        c3_u: 0.0,
        c3_v: 1.0,
        color_rgba: COMPOSITE_WORKLIST_NEUTRAL_COLOR_RGBA,
        flags: 0,
    };
    let scale_started_ns = crate::chronos::monotonic_nanos();
    let scale = crate::intel::gpgpu::sprite_quad_worklist_rgba8_over_stats(
        layer.surface,
        scratch,
        core::slice::from_ref(&desc),
    );
    let scale_complete_ns = crate::chronos::monotonic_nanos();
    if scale.descs != 1 || scale.submits != 1 {
        return Err("ui3-output-gpu-scale");
    }
    let scaled_placement = Ui3FramePlacement {
        width: scratch.width,
        height: scratch.height,
        ..layer.placement
    };
    let blend_started_ns = crate::chronos::monotonic_nanos();
    if !blend_premultiplied_layer(scratch, scaled_placement, dst)? {
        return Err("ui3-output-gpu-scale-blend");
    }
    Ok(LayerCompositionTiming {
        scratch_clear_us: elapsed_us(scratch_clear_started_ns, scratch_clear_complete_ns).max(1),
        scale_us: elapsed_us(scale_started_ns, scale_complete_ns).max(1),
        blend_us: elapsed_us(blend_started_ns, crate::chronos::monotonic_nanos()).max(1),
    })
}

fn blend_premultiplied_layer(
    src: GpgpuRgba8Surface,
    placement: Ui3FramePlacement,
    dst: GpgpuRgba8Surface,
) -> Result<bool, &'static str> {
    let Some((src_rect, dst_xy)) = clipped_unscaled_rect(src, placement, dst) else {
        return Ok(true);
    };
    let flags = COMPOSITE_WORKLIST_FLAG_SRC_OVER
        | COMPOSITE_WORKLIST_FLAG_PREMUL_SRC
        | COMPOSITE_WORKLIST_FLAG_TINT_ALPHA;
    let color = (u32::from(placement.opacity) << 24) | 0x00FF_FFFF;
    Ok(parallel_blend_rect(src, src_rect, dst, dst_xy, flags, color))
}

/// Split a rectangle into at most 16x16 non-overlapping descriptors.  The
/// shipped worklist kernel assigns descriptors across lanes/walkers; a single
/// full-screen descriptor would otherwise serialize millions of pixels on
/// one lane.
fn parallel_fill_rect(dst: GpgpuRgba8Surface, rect: GpgpuRect, color: u32) -> bool {
    let right = i64::from(rect.x) + i64::from(rect.width);
    let bottom = i64::from(rect.y) + i64::from(rect.height);
    if rect.x < 0
        || rect.y < 0
        || rect.x > i16::MAX as i32
        || rect.y > i16::MAX as i32
        || rect.width == 0
        || rect.height == 0
        || right > i64::from(dst.width)
        || bottom > i64::from(dst.height)
    {
        return false;
    }
    let mut descs = Vec::with_capacity(256);
    for_each_tile(rect.width, rect.height, |x, y, width, height| {
        let Some(dst_x) = rect.x.checked_add(x as i32) else {
            return;
        };
        let Some(dst_y) = rect.y.checked_add(y as i32) else {
            return;
        };
        if let Some(desc) = fill_desc(dst_x, dst_y, width, height, color) {
            descs.push(desc);
        }
    });
    if descs.is_empty() {
        return false;
    }
    let stats = crate::intel::gpgpu::fill_rect_worklist_rgba8_stats(dst, &descs);
    stats.descs == descs.len() && stats.submits == 1
}

fn parallel_blend_rect(
    src: GpgpuRgba8Surface,
    src_rect: GpgpuRect,
    dst: GpgpuRgba8Surface,
    dst_xy: GpgpuPoint,
    flags: u32,
    color: u32,
) -> bool {
    let src_right = i64::from(src_rect.x) + i64::from(src_rect.width);
    let src_bottom = i64::from(src_rect.y) + i64::from(src_rect.height);
    let dst_right = i64::from(dst_xy.x) + i64::from(src_rect.width);
    let dst_bottom = i64::from(dst_xy.y) + i64::from(src_rect.height);
    if src_rect.x < 0
        || src_rect.y < 0
        || src_rect.width == 0
        || src_rect.height == 0
        || dst_xy.x < 0
        || dst_xy.y < 0
        || src_right > i64::from(src.width)
        || src_bottom > i64::from(src.height)
        || dst_right > i64::from(dst.width)
        || dst_bottom > i64::from(dst.height)
    {
        return false;
    }
    let mut descs = Vec::with_capacity(256);
    for_each_tile(src_rect.width, src_rect.height, |x, y, width, height| {
        let Some(src_x) = src_rect.x.checked_add(x as i32) else {
            return;
        };
        let Some(src_y) = src_rect.y.checked_add(y as i32) else {
            return;
        };
        let Some(out_x) = dst_xy.x.checked_add(x as i32) else {
            return;
        };
        let Some(out_y) = dst_xy.y.checked_add(y as i32) else {
            return;
        };
        if src_x < 0
            || src_y < 0
            || src_x > u16::MAX as i32
            || src_y > u16::MAX as i32
            || out_x < i16::MIN as i32
            || out_x > i16::MAX as i32
            || out_y < i16::MIN as i32
            || out_y > i16::MAX as i32
            || width > u16::MAX as u32
            || height > u16::MAX as u32
        {
            return;
        }
        descs.push(AlphaBlendWorklistRgba8Desc {
            src_xy: pack_u16_pair(src_x as u16, src_y as u16),
            dst_xy: pack_i16_pair(out_x as i16, out_y as i16),
            size: pack_u16_pair(width as u16, height as u16),
            flags,
            color_rgba: color,
        });
    });
    if descs.is_empty() {
        return false;
    }
    let stats = crate::intel::gpgpu::alpha_blend_worklist_rgba8_over_stats(src, dst, &descs);
    stats.descs == descs.len() && stats.submits == 1
}

fn for_each_tile(width: u32, height: u32, mut f: impl FnMut(u32, u32, u32, u32)) {
    if width == 0 || height == 0 || width > u16::MAX as u32 || height > u16::MAX as u32 {
        return;
    }
    let cols = width.min(UI3_WORKLIST_TILE_AXIS);
    let rows = height.min(UI3_WORKLIST_TILE_AXIS);
    for row in 0..rows {
        let y0 = height.saturating_mul(row) / rows;
        let y1 = height.saturating_mul(row + 1) / rows;
        for col in 0..cols {
            let x0 = width.saturating_mul(col) / cols;
            let x1 = width.saturating_mul(col + 1) / cols;
            if x1 > x0 && y1 > y0 {
                f(x0, y0, x1 - x0, y1 - y0);
            }
        }
    }
}

fn fill_desc(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    color: u32,
) -> Option<FillRectWorklistRgba8Desc> {
    Some(FillRectWorklistRgba8Desc {
        dst_xy: pack_i16_pair(i16::try_from(x).ok()?, i16::try_from(y).ok()?),
        size: pack_u16_pair(u16::try_from(width).ok()?, u16::try_from(height).ok()?),
        color_rgba: color,
    })
}

const fn pack_u16_pair(x: u16, y: u16) -> u32 {
    (x as u32) | ((y as u32) << 16)
}

const fn pack_i16_pair(x: i16, y: i16) -> u32 {
    pack_u16_pair(x as u16, y as u16)
}

fn clipped_unscaled_rect(
    src: GpgpuRgba8Surface,
    placement: Ui3FramePlacement,
    dst: GpgpuRgba8Surface,
) -> Option<(GpgpuRect, GpgpuPoint)> {
    let left = i64::from(placement.x).max(0);
    let top = i64::from(placement.y).max(0);
    let right = (i64::from(placement.x) + i64::from(src.width)).min(i64::from(dst.width));
    let bottom = (i64::from(placement.y) + i64::from(src.height)).min(i64::from(dst.height));
    if right <= left || bottom <= top {
        return None;
    }
    let src_x = left.saturating_sub(i64::from(placement.x));
    let src_y = top.saturating_sub(i64::from(placement.y));
    let width = u32::try_from(right - left).ok()?;
    let height = u32::try_from(bottom - top).ok()?;
    Some((
        GpgpuRect::new(i32::try_from(src_x).ok()?, i32::try_from(src_y).ok()?, width, height),
        GpgpuPoint::new(i32::try_from(left).ok()?, i32::try_from(top).ok()?),
    ))
}

fn ensure_frame_buffers(
    frame: &mut FrameState,
    width: u32,
    height: u32,
) -> Result<(), &'static str> {
    let shape_changed = frame
        .buffers
        .iter()
        .flatten()
        .any(|storage| storage.surface.width != width || storage.surface.height != height);
    if shape_changed {
        if frame.acquired.is_some() {
            return Err("ui3-logical-frame-mode-resize-in-flight");
        }
        // Logical sources are never scanned out. All producer/compositor
        // submissions retire synchronously, so replacing both buffers is safe
        // here; the producer render PPGTT and direct-RCS PPGTT overwrite their
        // fixed-VA mappings before either new backing is consumed.
        for storage in frame.buffers.iter_mut().filter_map(Option::take) {
            crate::dma::dealloc(storage.virt, storage.surface.bytes);
        }
        frame.front = None;
    }
    for buffer in 0..UI3_FRAME_BUFFER_COUNT {
        if frame.buffers[buffer].is_some() {
            continue;
        }
        let slot = frame.id.gpu_slot_base() + buffer;
        let gpu = UI3_FRAME_GPU_BASE
            .checked_add((slot as u64).saturating_mul(UI3_FRAME_GPU_STRIDE))
            .ok_or("ui3-logical-frame-gpu-overflow")?;
        frame.buffers[buffer] = Some(allocate_surface(width, height, gpu)?);
    }
    Ok(())
}

fn ensure_scale_scratch(width: u32, height: u32) -> Result<GpgpuRgba8Surface, &'static str> {
    let mut state = STATE.lock();
    if let Some(storage) = state.scale_scratch {
        if storage.surface.width == width && storage.surface.height == height {
            return Ok(storage.surface);
        }
        // Every compositor kernel submission is synchronously retired before
        // return, so the old scratch backing is no longer in flight.  The next
        // direct-RCS submission overwrites this fixed VA's PTE.
        crate::dma::dealloc(storage.virt, storage.surface.bytes);
        state.scale_scratch = None;
    }
    let storage = allocate_surface(width, height, UI3_SCALE_SCRATCH_GPU)?;
    let surface = storage.surface;
    state.scale_scratch = Some(storage);
    Ok(surface)
}

fn allocate_surface(width: u32, height: u32, gpu: u64) -> Result<SurfaceStorage, &'static str> {
    if width == 0 || height == 0 || (gpu & 0xFFF) != 0 {
        return Err("ui3-logical-frame-shape");
    }
    let row_bytes = (width as usize)
        .checked_mul(core::mem::size_of::<u32>())
        .ok_or("ui3-logical-frame-size")?;
    let pitch = crate::intel::align_up(row_bytes, 64).ok_or("ui3-logical-frame-pitch")?;
    let raw_bytes = pitch
        .checked_mul(height as usize)
        .ok_or("ui3-logical-frame-size")?;
    let bytes = crate::intel::align_up(raw_bytes, crate::intel::WARM_ALIGN)
        .ok_or("ui3-logical-frame-size")?;
    if bytes as u64 > UI3_FRAME_GPU_STRIDE
        || gpu.saturating_add(bytes as u64) > UI3_COMPOSITOR_GPU_LIMIT
    {
        return Err("ui3-logical-frame-capacity");
    }
    let (phys, virt) =
        crate::dma::alloc(bytes, crate::intel::WARM_ALIGN).ok_or("ui3-logical-frame-alloc")?;
    let Some(surface) = GpgpuRgba8Surface::new(
        phys,
        gpu,
        bytes,
        width,
        height,
        u32::try_from(pitch).map_err(|_| "ui3-logical-frame-pitch")?,
    ) else {
        crate::dma::dealloc(virt, bytes);
        return Err("ui3-logical-frame-surface");
    };
    crate::log_info!(
        target: "ui3";
        "ui3-compositor: source buffer allocated gpu=0x{:X} phys=0x{:X} size={}x{} pitch=0x{:X} bytes=0x{:X} initialization=gpu-only\n",
        gpu,
        phys,
        width,
        height,
        pitch,
        bytes,
    );
    Ok(SurfaceStorage { surface, virt })
}
