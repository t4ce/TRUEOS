// Display proof contract.
//
// Current evidence from the bring-up transcript:
// - `primary-boot-surface` programs pipe-a at `2560x1440`, pitch `0x2800`.
// - The surface GPU address is `0x02000000`.
// - `surf_live` matches `surf`, and the boot logo path reports `ok=1`.
//
// This proves scanout handoff to known memory.  It does not prove the 3D
// pipeline rendered that memory; render must separately produce `ps-rt-proof
// accepted=1` before a displayed pixel can be attributed to GPU rendering.

use crate::intel::types::{Rgba8, UiRect, UiSurface, UiSurfaceFormat};
use alloc::{collections::VecDeque, string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

mod regs;
pub(super) use self::regs::*;

mod display_metrics;

macro_rules! intel_display_focus_log {
    ($($arg:tt)*) => {
        if crate::log_os::flags::INTEL_STAGE1_LOGS || crate::log_os::flags::INTEL_DISPLAY_NGIN_LOGS {
            crate::log!($($arg)*);
        }
    };
}

macro_rules! intel_display_verbose_log {
    ($($arg:tt)*) => {
        if crate::log_os::flags::INTEL_DISPLAY_NGIN_LOGS && !crate::log_os::flags::INTEL_STAGE1_LOGS {
            crate::log!($($arg)*);
        }
    };
}

mod display_probes;
pub(crate) use self::display_probes::{
    arm_decoded_nv12_overlay_plane_probe, decoded_nv12_overlay_plane_probe_replaces_cpu_present,
    log_display_plane_ladder_probe, ui4_copy_decoded_ytile_nv12_to_linear,
    ui4_decoded_nv12_linear_staging_set, ui4_decoded_nv12_staging_scale,
    ui4_present_linear_nv12_surface,
};
use self::display_probes::{
    log_pipe_scanout_probe, log_primary_dimensions_probe, log_primary_plane_probe,
    probe_boot_logo_decode, probe_primary_present_psr,
};

// PIPE_BOTTOM_COLOR is not A/R/G/B bytes. PRM layout is:
// bit31 gamma enable, bit30 CSC enable, bits29:20 R/V, bits19:10 G/Y, bits9:0 B/U.
// The color channels are unsigned U0.10, so white is 0x3FF in each channel.
const PIPE_BOTTOM_COLOR_RAW: u32 = pipe_bottom_color_u0_10(0x3FF, 0x3FF, 0x3FF);
const PRIMARY_FORMAT_PROBE_XRGB: u32 = 0;
const PRIMARY_FORMAT_PROBE_XBGR: u32 = 1;
const PRIMARY_FORMAT_PROBE_MODE: u32 = PRIMARY_FORMAT_PROBE_XRGB;
// Display version 13 exposes one primary plus four sprite planes. TRUEOS uses
// zero-based indices, so the hardware's top sprite is slot 4.
const UNIVERSAL_PLANE_SLOTS: usize = crate::ui4::UNIVERSAL_PLANE_COUNT;
const UI4_PLANE_SURFACE_FLIP_BATCH_CAPACITY: usize = UNIVERSAL_PLANE_SLOTS;
const UI4_PLANE_SURFACE_FLIP_TIMEOUT_NS: u64 = 25_000_000;
const PRIMARY_BYTES_PER_PIXEL: u32 = 4;
const PRIMARY_BASELINE_COLOR: u32 = 0x00FF_37FF;
const VIDEO_NV12_BLACK_PROOF_LIFT: bool = false;
const PRIMARY_BOOT_LOGO_JPEG: &[u8] = include_bytes!("../../logo.jpg");
const PRIMARY_BOOT_HORIZON_STAMP_PNG: &[u8] = include_bytes!("../../HorizonServer.png");
const PRIMARY_BOOT_LOGO_ENABLED: bool = true;
const PRIMARY_BOOT_HORIZON_STAMP_ENABLED: bool = false;
const PRIMARY_BOOT_LOGO_DECODE_MODE: PrimaryBootLogoDecodeMode =
    PrimaryBootLogoDecodeMode::ZuneJpeg;
const PRIMARY_BOOT_LOGO_WAIT_TIMEOUT_MS: u64 = 5000;
const PRIMARY_BOOT_LOGO_PRESENT_HOLD_MS: u64 = 3000;
const PRIMARY_BOOT_DISPLAY_WARMUP_ENABLED: bool = true;
const PRIMARY_GPGPU_EDGE_GUARD_PIXELS: u32 = 64;

const fn pipe_bottom_color_u0_10(red: u32, green: u32, blue: u32) -> u32 {
    ((red & 0x3FF) << 20) | ((green & 0x3FF) << 10) | (blue & 0x3FF)
}
const JPG_CENTER_CROP: bool = true;
// Universal plane role map for pipe-local planes.
const UI_OVERLAY_PLANE_SLOT: usize = crate::ui4::ALPHA_OVERLAY_PLANE_SLOT;
const VIDEO_NV12_PLANE_SLOT: usize = crate::ui4::NV12_UV_PLANE_SLOT;
const VIDEO_NV12_Y_PLANE_SLOT: usize = crate::ui4::NV12_Y_PLANE_SLOT;
// The retired linked-NV12 experiment owns zero-based slots 2/3 (hardware
// planes 3/4). Keep its ABI callable but inert while UI4 owns those slots as
// ordinary RGB windows.
const LEGACY_DIRECT_NV12_PLANE_ABI_ENABLED: bool = false;
const OVERLAY_PLANE_SLOT: usize = UI_OVERLAY_PLANE_SLOT;
const DEFAULT_OVERLAY_MARKER_ENABLED: bool = false;
const DEFAULT_OVERLAY_MARKER_SIZE: u32 = 50;
const DEFAULT_OVERLAY_MARKER_COLOR: u32 = 0x0000_0000;
const OVERLAY_MARGIN_X: u32 = 0;
const OVERLAY_MARGIN_Y: u32 = 0;
const NATIVE_PLANE_SLOT_BARS_ENABLED: bool = false;
const PRIMARY_BOOT_NATIVE_PLANE_SLOT_BARS_ENABLED: bool = false;
const NATIVE_PLANE_SLOT_BAR_MARGIN: u32 = 16;
const NATIVE_PLANE_SLOT_BAR_GAP: u32 = 8;
pub(super) const NATIVE_PLANE_SLOT_BAR_WIDTH: u32 = 64;
pub(super) const NATIVE_PLANE_SLOT_BAR_HEIGHT: u32 = 128;
const NATIVE_PLANE_SLOT_BAR_XRGB: u32 = 0x0000_0000;
const OVERLAY_COMPOSITION_PROOF_MARKER_ENABLED: bool = false;
const OVERLAY_COMPOSITION_PROOF_MARKER_SIZE: u32 = 96;
const OVERLAY_COMPOSITION_PROOF_MARKER_GAP: u32 = 16;
const OVERLAY_COMPOSITION_PROOF_MARKER_X: u32 = 48;
const OVERLAY_COMPOSITION_PROOF_MARKER_Y: u32 = 48;
const OVERLAY_SWAP_BUFFER_COUNT: usize = crate::ui4::FrameBuffering::Double.count();
pub(super) const DISPLAY_PIPELINE_COUNT: usize = PIPES.len();
pub(super) const DISPLAY_OUTPUT_COUNT: usize = crate::ui4::OUTPUT_COUNT;
const _: () = assert!(DISPLAY_OUTPUT_COUNT == DISPLAY_PIPELINE_COUNT);
// The first three display overlays retain their bootstrap addresses below the
// legacy direct-RCS 1 GiB boundary. Slot 4 is CPU-authored UI interaction
// chrome and therefore needs no direct-RCS alias; keep it above that boundary
// so it cannot collide with Draw3D's fixed resident scene addresses.
const DISPLAY_DIRECT_RCS_VA_LIMIT: u64 = 0x4000_0000;
// Scanout GGTT addresses are not render-engine addresses. The compositor maps
// the selected physical back buffer at one of these private aliases in the
// direct-RCS PPGTT, then leaves the stable scanout address untouched.
const PRIMARY_COMPOSE_RCS_GPU_ALIAS: u64 = 0x3D00_0000;
const OVERLAY_COMPOSE_RCS_GPU_ALIAS: u64 = 0x3E00_0000;
const COMPOSE_RCS_GPU_ALIAS_BYTES: u64 = 0x0100_0000;
const OVERLAY_SWAP_GPU_BASE: u64 = 0x1800_0000;
const OVERLAY_SWAP_GPU_STRIDE: u64 = 0x0100_0000;
const OVERLAY_PIPE_GPU_STRIDE: u64 = 0x0200_0000;
const OVERLAY_PLANE_GPU_STRIDE: u64 = DISPLAY_PIPELINE_COUNT as u64 * OVERLAY_PIPE_GPU_STRIDE;
const OVERLAY_UNIVERSAL_PLANE_COUNT: usize = crate::ui4::UNIVERSAL_PLANE_COUNT - 1;
const DIRECT_RCS_OVERLAY_UNIVERSAL_PLANE_COUNT: usize = 3;
const INTERACTION_OVERLAY_GPU_BASE: u64 = DISPLAY_DIRECT_RCS_VA_LIMIT;
const PRIMARY_SWAP_BUFFER_COUNT: usize = crate::ui4::FrameBuffering::Double.count();
const PRIMARY_SWAP_GPU_BASE: u64 = 0x3100_0000;
const PRIMARY_SWAP_GPU_STRIDE: u64 = 0x0100_0000;
const PRIMARY_SWAP_PIPE_GPU_STRIDE: u64 = 0x0200_0000;
const PRIMARY_SECONDARY_PIPE_GPU_BASE: u64 = 0x3900_0000;
const PRIMARY_PIPE_GPU_STRIDE: u64 = 0x0200_0000;
const PRIMARY_LEGACY_PIPE_GPU_CAPACITY: u64 = 0x0100_0000;
const _: () = assert!(OVERLAY_PIPE_GPU_STRIDE >= OVERLAY_SWAP_GPU_STRIDE * 2);
const _: () = assert!(OVERLAY_UNIVERSAL_PLANE_COUNT == 4);
const _: () = assert!(
    PRIMARY_COMPOSE_RCS_GPU_ALIAS + COMPOSE_RCS_GPU_ALIAS_BYTES <= OVERLAY_COMPOSE_RCS_GPU_ALIAS
);
const _: () = assert!(
    OVERLAY_COMPOSE_RCS_GPU_ALIAS + COMPOSE_RCS_GPU_ALIAS_BYTES <= DISPLAY_DIRECT_RCS_VA_LIMIT
);
const _: () = assert!(PRIMARY_SWAP_PIPE_GPU_STRIDE >= PRIMARY_SWAP_GPU_STRIDE * 2);
const _: () = assert!(
    OVERLAY_SWAP_GPU_BASE
        + DIRECT_RCS_OVERLAY_UNIVERSAL_PLANE_COUNT as u64 * OVERLAY_PLANE_GPU_STRIDE
        <= PRIMARY_SWAP_GPU_BASE
);
const _: () = assert!(
    PRIMARY_SWAP_GPU_BASE + DISPLAY_PIPELINE_COUNT as u64 * PRIMARY_SWAP_PIPE_GPU_STRIDE
        <= PRIMARY_SECONDARY_PIPE_GPU_BASE
);
const _: () = assert!(
    PRIMARY_SECONDARY_PIPE_GPU_BASE + (DISPLAY_PIPELINE_COUNT as u64 - 1) * PRIMARY_PIPE_GPU_STRIDE
        <= DISPLAY_DIRECT_RCS_VA_LIMIT
);
const _: () = assert!(
    INTERACTION_OVERLAY_GPU_BASE + DISPLAY_PIPELINE_COUNT as u64 * OVERLAY_PIPE_GPU_STRIDE
        <= (u32::MAX as u64) + 1
);
pub(super) const DISPLAY_FRAME_TARGET_CAPACITY: usize =
    DISPLAY_PIPELINE_COUNT * OVERLAY_UNIVERSAL_PLANE_COUNT * OVERLAY_SWAP_BUFFER_COUNT;
const VIDEO_NV12_HIDE_PARK_BEFORE_DISABLE: bool = true;
const VIDEO_NV12_HIDE_PARK_SIZE: u32 = 64;

#[derive(Copy, Clone)]
struct PlaneSurfaceFlip {
    plane_base: usize,
    surface_reg: u32,
}

#[derive(Copy, Clone)]
struct PlaneSurfaceFlipBatch {
    active: bool,
    accepting: bool,
    len: usize,
    entries: [Option<PlaneSurfaceFlip>; UI4_PLANE_SURFACE_FLIP_BATCH_CAPACITY],
}

impl PlaneSurfaceFlipBatch {
    const fn new() -> Self {
        Self {
            active: false,
            accepting: false,
            len: 0,
            entries: [None; UI4_PLANE_SURFACE_FLIP_BATCH_CAPACITY],
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum PlaneSurfaceFlipQueueResult {
    Inactive,
    Queued,
    Rejected,
}

static PRIMARY_BOOT_SURFACE_INIT: AtomicBool = AtomicBool::new(false);
const UI4_RGBA8_PLANE_STACK_UNINITIALIZED: u32 = 0;
const UI4_RGBA8_PLANE_STACK_INITIALIZING: u32 = 1;
const UI4_RGBA8_PLANE_STACK_READY: u32 = 2;
const UI4_RGBA8_PLANE_STACK_FAILED: u32 = 3;
static UI4_RGBA8_PLANE_STACK_STATE: AtomicU32 = AtomicU32::new(UI4_RGBA8_PLANE_STACK_UNINITIALIZED);
static UI4_RGBA8_PLANE_STACK_PIPE_SLOT: AtomicU32 = AtomicU32::new(u32::MAX);
static PRIMARY_PRESENT_SEQ: AtomicU32 = AtomicU32::new(0);
static PRIMARY_SOURCE_PROGRAM_SEQ: AtomicU32 = AtomicU32::new(0);
static UI4_PLANE_SURFACE_FLIP_BATCH_SEQ: AtomicU32 = AtomicU32::new(0);
static UI4_PLANE_SURFACE_FLIP_BATCH: Mutex<PlaneSurfaceFlipBatch> =
    Mutex::new(PlaneSurfaceFlipBatch::new());
static UI_SURFACE_PRIMARY_COPY_SEQ: AtomicU32 = AtomicU32::new(0);
static PRIMARY_SURFACES: [Mutex<Option<PrimarySurface>>; DISPLAY_PIPELINE_COUNT] = [
    Mutex::new(None),
    Mutex::new(None),
    Mutex::new(None),
    Mutex::new(None),
];
static PRIMARY_PLANE_SOURCE_BINDINGS: [Mutex<Option<PrimaryPlaneSourceBinding>>;
    DISPLAY_PIPELINE_COUNT] = [
    Mutex::new(None),
    Mutex::new(None),
    Mutex::new(None),
    Mutex::new(None),
];
static OVERLAY_PRESENT_SEQ: AtomicU32 = AtomicU32::new(0);
static OVERLAY_IN_PLACE_FALLBACK_SEQ: AtomicU32 = AtomicU32::new(0);
static DISPLAY_PIPELINE_SELECTION_SIGNATURE: AtomicU32 = AtomicU32::new(u32::MAX);
static OVERLAY_SURFACES_SLOT_1: [Mutex<OverlaySurfacePool>; DISPLAY_PIPELINE_COUNT] = [
    Mutex::new(OverlaySurfacePool::new()),
    Mutex::new(OverlaySurfacePool::new()),
    Mutex::new(OverlaySurfacePool::new()),
    Mutex::new(OverlaySurfacePool::new()),
];
static OVERLAY_SURFACES_SLOT_2: [Mutex<OverlaySurfacePool>; DISPLAY_PIPELINE_COUNT] = [
    Mutex::new(OverlaySurfacePool::new()),
    Mutex::new(OverlaySurfacePool::new()),
    Mutex::new(OverlaySurfacePool::new()),
    Mutex::new(OverlaySurfacePool::new()),
];
static OVERLAY_SURFACES_SLOT_3: [Mutex<OverlaySurfacePool>; DISPLAY_PIPELINE_COUNT] = [
    Mutex::new(OverlaySurfacePool::new()),
    Mutex::new(OverlaySurfacePool::new()),
    Mutex::new(OverlaySurfacePool::new()),
    Mutex::new(OverlaySurfacePool::new()),
];
static OVERLAY_SURFACES_SLOT_4: [Mutex<OverlaySurfacePool>; DISPLAY_PIPELINE_COUNT] = [
    Mutex::new(OverlaySurfacePool::new()),
    Mutex::new(OverlaySurfacePool::new()),
    Mutex::new(OverlaySurfacePool::new()),
    Mutex::new(OverlaySurfacePool::new()),
];
static PRIMARY_SWAP_SURFACES: [Mutex<PrimarySwapSurfacePool>; DISPLAY_PIPELINE_COUNT] = [
    Mutex::new(PrimarySwapSurfacePool::new()),
    Mutex::new(PrimarySwapSurfacePool::new()),
    Mutex::new(PrimarySwapSurfacePool::new()),
    Mutex::new(PrimarySwapSurfacePool::new()),
];
static VIDEO_NV12_PLANE_ALPHA: AtomicU32 = AtomicU32::new(0xFF);
static HW_LOGO_PENDING_IDS: Mutex<VecDeque<u32>> = Mutex::new(VecDeque::new());
static HW_LOGO_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();
static HW_LOGO_NEXT_STAGE: AtomicU32 = AtomicU32::new(0);
static HW_LOGO_SEQUENCE_DONE: AtomicBool = AtomicBool::new(false);
static HW_LOGO_SEQUENCE_DONE_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum PrimaryBootLogoDecodeMode {
    HwPic,
    ZuneJpeg,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct PrimarySurfaceSampleSet {
    pub(crate) tl: u32,
    pub(crate) center: u32,
    pub(crate) br: u32,
    pub(crate) apex: u32,
    pub(crate) centroid: u32,
    pub(crate) left: u32,
    pub(crate) right: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct PrimarySurfaceBgra8Snapshot {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pixels: Vec<u8>,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct LiveOverlayRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) color: Rgba8,
}

impl LiveOverlayRect {
    pub(crate) const fn new(x: u32, y: u32, width: u32, height: u32, color: Rgba8) -> Self {
        Self {
            x,
            y,
            width,
            height,
            color,
        }
    }
}

/// One positioned RGBA8 image consumed by a display composition.
pub(crate) struct RgbaOverlayTile<'a> {
    pub(crate) x: u32,
    pub(crate) y: u32,
    /// Destination extent in output coordinates.
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// Published source extent. A differing destination extent is sampled
    /// with nearest-neighbour scaling by the compositor.
    pub(crate) source_width: u32,
    pub(crate) source_height: u32,
    pub(crate) pitch_bytes: usize,
    pub(crate) pixels: &'a [u8],
    /// GPU-visible source for the UI4 compositor fast path. Legacy/readback
    /// callers may omit this and retain the CPU fallback.
    pub(crate) gpgpu_surface: Option<crate::intel::gpgpu::GpgpuRgba8Surface>,
    /// Additional whole-tile opacity applied after source alpha.
    pub(crate) opacity: u8,
    pub(crate) expected_rgba: Option<Rgba8>,
}

/// Output-space damage consumed by the display compositor.
pub(crate) type CompositionDamageRect = crate::ui4::DamageRect;
pub(crate) type CompositionDamageRegion = crate::ui4::DamageRegion;

impl PrimarySurfaceSampleSet {
    pub(crate) fn any_changed_since(self, before: Self) -> bool {
        self.tl != before.tl
            || self.center != before.center
            || self.br != before.br
            || self.apex != before.apex
            || self.centroid != before.centroid
            || self.left != before.left
            || self.right != before.right
    }

    pub(crate) fn triangle_points_changed_since(self, before: Self) -> bool {
        self.apex != before.apex
            || self.centroid != before.centroid
            || self.left != before.left
            || self.right != before.right
    }
}

#[derive(Copy, Clone)]
struct PrimarySurface {
    width: u32,
    height: u32,
    backing_width: u32,
    backing_height: u32,
    pitch_bytes: u32,
    byte_len: usize,
    phys: u64,
    virt: *mut u8,
    gpu: u64,
    pipe: PipeInfo,
}

unsafe impl Send for PrimarySurface {}
unsafe impl Sync for PrimarySurface {}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum PrimaryPlaneSourceFormat {
    Xrgb8888,
    Xbgr8888,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct PrimaryPlaneSource {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) byte_len: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
    pub(crate) format: PrimaryPlaneSourceFormat,
    pub(crate) src_x: u32,
    pub(crate) src_y: u32,
    pub(crate) dst_x: u32,
    pub(crate) dst_y: u32,
    pub(crate) dst_w: u32,
    pub(crate) dst_h: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct PrimaryPlaneSourceBinding {
    phys: u64,
    gpu: u64,
    byte_len: usize,
    width: u32,
    height: u32,
    pitch_bytes: u32,
    format: PrimaryPlaneSourceFormat,
}

#[derive(Copy, Clone)]
struct PrimaryBackingCopyRect {
    src_x: usize,
    src_y: usize,
    dst_x: usize,
    dst_y: usize,
    width: usize,
    height: usize,
    src_pitch: usize,
    dst_pitch: usize,
    row_bytes: usize,
    flush_offset: usize,
    flush_bytes: usize,
}

/// Stable identity for one of Intel's four display-pipeline slots.
///
/// A pipeline is not the same thing as a connector or a monitor. Routing can
/// change later; frame and plane ownership remains keyed by this hardware
/// slot so an inactive pipe never aliases the active pipe's resources.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub(super) struct DisplayPipelineId(u8);

impl DisplayPipelineId {
    pub(super) const fn from_slot(slot: usize) -> Option<Self> {
        if slot < DISPLAY_PIPELINE_COUNT {
            Some(Self(slot as u8))
        } else {
            None
        }
    }

    pub(super) const fn slot(self) -> usize {
        self.0 as usize
    }

    pub(super) const fn name(self) -> &'static str {
        match self.0 {
            0 => "pipe-a",
            1 => "pipe-b",
            2 => "pipe-c",
            3 => "pipe-d",
            _ => "pipe-invalid",
        }
    }

    fn from_pipe(pipe: PipeInfo) -> Option<Self> {
        Self::from_slot(pipe.slot)
    }

    fn pipe(self) -> Option<PipeInfo> {
        PIPES.get(self.slot()).copied()
    }
}

/// Stable compositor-facing identity for one of the four logical display
/// outputs. An output is not a pipe, DDI route, connector, or monitor. The
/// current baseline assigns live hardware routes to these slots; connector
/// discovery can later preserve an output while moving it between pipes.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
#[repr(transparent)]
pub(super) struct DisplayOutputId(u8);

impl DisplayOutputId {
    pub(super) const fn from_slot(slot: usize) -> Option<Self> {
        if slot < DISPLAY_OUTPUT_COUNT {
            Some(Self(slot as u8))
        } else {
            None
        }
    }

    pub(super) const fn slot(self) -> usize {
        self.0 as usize
    }

    pub(super) const fn name(self) -> &'static str {
        match self.0 {
            0 => "D01",
            1 => "D02",
            2 => "D03",
            3 => "D04",
            _ => "D-invalid",
        }
    }
}

fn primary_surface_owner(pipe: PipeInfo) -> &'static Mutex<Option<PrimarySurface>> {
    &PRIMARY_SURFACES[pipe.slot]
}

fn primary_plane_source_binding_owner(
    pipe: PipeInfo,
) -> &'static Mutex<Option<PrimaryPlaneSourceBinding>> {
    &PRIMARY_PLANE_SOURCE_BINDINGS[pipe.slot]
}

fn primary_plane_source_binding_conflict(
    pipeline: DisplayPipelineId,
    binding: PrimaryPlaneSourceBinding,
) -> Option<(DisplayPipelineId, PrimaryPlaneSourceBinding)> {
    for (slot, owner) in PRIMARY_PLANE_SOURCE_BINDINGS.iter().enumerate() {
        let other_pipeline = DisplayPipelineId::from_slot(slot)?;
        if other_pipeline == pipeline {
            continue;
        }
        let Some(other) = *owner.lock() else {
            continue;
        };
        let identical_mapping = binding.phys == other.phys
            && binding.gpu == other.gpu
            && binding.byte_len == other.byte_len;
        if !identical_mapping
            && display_gpu_ranges_overlap(binding.gpu, binding.byte_len, other.gpu, other.byte_len)
        {
            return Some((other_pipeline, other));
        }
    }
    None
}

fn display_gpu_ranges_overlap(a_gpu: u64, a_len: usize, b_gpu: u64, b_len: usize) -> bool {
    let Some(a_len) = u64::try_from(a_len).ok() else {
        return true;
    };
    let Some(b_len) = u64::try_from(b_len).ok() else {
        return true;
    };
    let Some(a_end) = a_gpu.checked_add(a_len) else {
        return true;
    };
    let Some(b_end) = b_gpu.checked_add(b_len) else {
        return true;
    };
    a_gpu < b_end && b_gpu < a_end
}

fn primary_surface_for_pipe(pipe: PipeInfo) -> Option<PrimarySurface> {
    *primary_surface_owner(pipe).lock()
}

fn primary_surface_for_pipeline(pipeline: DisplayPipelineId) -> Option<PrimarySurface> {
    primary_surface_for_pipe(pipeline.pipe()?)
}

fn active_primary_surface() -> Option<PrimarySurface> {
    if let Some(dev) = crate::intel::claimed_device() {
        if let Some(surface) = active_pipe(dev).and_then(primary_surface_for_pipe) {
            return Some(surface);
        }
    }
    PRIMARY_SURFACES.iter().find_map(|owner| *owner.lock())
}

fn primary_surface_gpu_for_pipe(pipe: PipeInfo) -> Option<u64> {
    if pipe.slot == 0 {
        return Some(crate::intel::GPU_VA_DISPLAY_PRIMARY_BASE);
    }
    PRIMARY_SECONDARY_PIPE_GPU_BASE
        .checked_add((pipe.slot as u64 - 1).checked_mul(PRIMARY_PIPE_GPU_STRIDE)?)
}

fn primary_surface_gpu_capacity(pipe: PipeInfo) -> u64 {
    if pipe.slot == 0 {
        PRIMARY_LEGACY_PIPE_GPU_CAPACITY
    } else {
        PRIMARY_PIPE_GPU_STRIDE
    }
}

/// Hardware evidence for whether a display pipeline is merely programmed or
/// is complete enough to be treated as a live scanout path.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum DisplayPipelineActivity {
    Inactive,
    Programmed,
    Scanout,
}

impl DisplayPipelineActivity {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Inactive => "inactive",
            Self::Programmed => "programmed",
            Self::Scanout => "scanout",
        }
    }
}

/// Route selected by TRANS_DDI_FUNC_CTL. This identifies the display-engine
/// route, not a connector or monitor; connector discovery remains a separate
/// policy layer.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum DisplayDdiRoute {
    None,
    DdiB,
    DdiC,
    DdiD,
    DdiETc1,
    DdiFTc2,
    DdiGTc3,
    DdiHTc4,
}

impl DisplayDdiRoute {
    const fn from_select(select: u32) -> Self {
        match select & 0x07 {
            0 => Self::None,
            1 => Self::DdiB,
            2 => Self::DdiC,
            3 => Self::DdiD,
            4 => Self::DdiETc1,
            5 => Self::DdiFTc2,
            6 => Self::DdiGTc3,
            _ => Self::DdiHTc4,
        }
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::DdiB => "ddi-b",
            Self::DdiC => "ddi-c",
            Self::DdiD => "ddi-d",
            Self::DdiETc1 => "ddi-e/tc1",
            Self::DdiFTc2 => "ddi-f/tc2",
            Self::DdiGTc3 => "ddi-g/tc3",
            Self::DdiHTc4 => "ddi-h/tc4",
        }
    }

    const fn select(self) -> u8 {
        match self {
            Self::None => 0,
            Self::DdiB => 1,
            Self::DdiC => 2,
            Self::DdiD => 3,
            Self::DdiETc1 => 4,
            Self::DdiFTc2 => 5,
            Self::DdiGTc3 => 6,
            Self::DdiHTc4 => 7,
        }
    }
}

/// Stable route facts which must remain true from frame acquisition through
/// scanout commit. This catches same-resolution reroutes that a width/height
/// comparison alone cannot see.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) struct DisplayPipelineRoute {
    pub(super) ddi: DisplayDdiRoute,
    pub(super) transcoder_mode: u8,
    pub(super) bits_per_color_select: u8,
    pub(super) sync_polarity: u8,
    pub(super) port_width: u8,
    pub(super) pipe_enabled: bool,
    pub(super) transcoder_enabled: bool,
    pub(super) primary_enabled: bool,
    pub(super) primary_bound: bool,
}

impl DisplayPipelineRoute {
    fn from_registers(
        pipeconf: u32,
        transcoder: u32,
        primary_ctl: u32,
        primary_surf: u32,
        primary_live: u32,
    ) -> Self {
        Self {
            ddi: DisplayDdiRoute::from_select((transcoder >> 27) & 0x07),
            transcoder_mode: ((transcoder >> 24) & 0x07) as u8,
            bits_per_color_select: ((transcoder >> 20) & 0x03) as u8,
            sync_polarity: ((transcoder >> 16) & 0x03) as u8,
            port_width: ((transcoder >> 1) & 0x07) as u8,
            pipe_enabled: (pipeconf & (1 << 31)) != 0,
            transcoder_enabled: (transcoder & (1 << 31)) != 0,
            primary_enabled: (primary_ctl & PLANE_CTL_ENABLE) != 0,
            primary_bound: primary_surf != 0 || primary_live != 0,
        }
    }

    const fn complete(self) -> bool {
        self.pipe_enabled
            && self.transcoder_enabled
            && !matches!(self.ddi, DisplayDdiRoute::None)
            && self.primary_enabled
            && self.primary_bound
    }

    pub(super) fn mode_name(self) -> &'static str {
        decode_trans_ddi_mode(self.transcoder_mode as u32)
    }

    pub(super) fn bits_per_color(self) -> u32 {
        decode_trans_bits_per_color(self.bits_per_color_select as u32)
    }
}

/// A frame-route lease. Equality deliberately includes the output route and
/// readiness facts, not just the framebuffer dimensions.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) struct DisplayPipelineTarget {
    pub(super) pipeline: DisplayPipelineId,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) route: DisplayPipelineRoute,
    pub(super) activity: DisplayPipelineActivity,
}

/// A logical-output lease over one concrete pipeline route. The output ID is
/// compositor policy; the nested target is the hardware fact which must stay
/// valid until the completed frame is committed.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) struct DisplayOutputTarget {
    pub(super) output: DisplayOutputId,
    pub(super) pipeline_target: DisplayPipelineTarget,
}

/// Read-only four-pipeline hardware baseline consumed by compositor policy.
/// It deliberately keeps route, mode, and activity independent: a stale
/// PIPE_SRC is a programmed mode, not proof that pixels reach a monitor.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) struct DisplayPipelineSnapshot {
    pub(super) pipeline: DisplayPipelineId,
    pub(super) activity: DisplayPipelineActivity,
    pub(super) target: Option<DisplayPipelineTarget>,
    pub(super) route: DisplayPipelineRoute,
    pub(super) pipe_enabled: bool,
    pub(super) transcoder_enabled: bool,
    pub(super) primary_enabled: bool,
    pub(super) primary_bound: bool,
    pipe_src: u32,
    pipeconf: u32,
    transcoder: u32,
    primary_ctl: u32,
    primary_surf: u32,
    primary_live: u32,
    observed: bool,
}

#[derive(Copy, Clone)]
struct CompatibilityPipelineSelection {
    snapshot: DisplayPipelineSnapshot,
    rank: u8,
    reason: &'static str,
    candidate_mask: u8,
    best_mask: u8,
    best_count: u8,
    scanout_mask: u8,
}

#[derive(Copy, Clone)]
pub(super) struct PrimarySurfaceGpgpuTarget {
    pub(super) pipeline: DisplayPipelineId,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) pitch_bytes: u32,
    pub(super) gpu: u64,
    pub(super) phys: u64,
    pub(super) virt: *mut u8,
    pub(super) byte_len: usize,
    pub(super) marker_gpu: u64,
    pub(super) marker_virt: *mut u8,
    pub(super) marker_offset: usize,
    pub(super) marker_x: u32,
    pub(super) marker_y: u32,
}

unsafe impl Send for PrimarySurfaceGpgpuTarget {}
unsafe impl Sync for PrimarySurfaceGpgpuTarget {}

#[derive(Copy, Clone)]
struct OverlaySurface {
    width: u32,
    height: u32,
    pitch_bytes: u32,
    byte_len: usize,
    phys: u64,
    virt: *mut u8,
    gpu: u64,
    pipe: PipeInfo,
    plane_slot: usize,
    buffer_index: usize,
}

unsafe impl Send for OverlaySurface {}
unsafe impl Sync for OverlaySurface {}

#[derive(Copy, Clone)]
struct OverlaySurfacePool {
    width: u32,
    height: u32,
    pipe_slot: usize,
    front_index: Option<usize>,
    surfaces: [Option<OverlaySurface>; OVERLAY_SWAP_BUFFER_COUNT],
    damage_debt: [CompositionDamageRegion; OVERLAY_SWAP_BUFFER_COUNT],
    /// Previous-size surface retained only until the replacement is proven
    /// live. It is not a render target and must be reclaimed after the latch.
    retiring_front: Option<OverlaySurface>,
}

#[derive(Copy, Clone)]
struct PrimarySwapSurface {
    width: u32,
    height: u32,
    pitch_bytes: u32,
    byte_len: usize,
    phys: u64,
    virt: *mut u8,
    gpu: u64,
    pipe: PipeInfo,
    buffer_index: usize,
}

unsafe impl Send for PrimarySwapSurface {}
unsafe impl Sync for PrimarySwapSurface {}

#[derive(Copy, Clone)]
struct PrimarySwapSurfacePool {
    width: u32,
    height: u32,
    pipe_slot: usize,
    front_index: Option<usize>,
    surfaces: [Option<PrimarySwapSurface>; PRIMARY_SWAP_BUFFER_COUNT],
    damage_debt: [CompositionDamageRegion; PRIMARY_SWAP_BUFFER_COUNT],
}

#[derive(Copy, Clone, Debug)]
pub(super) struct DecodedNv12PlaneAlphaProgram {
    alpha: u8,
    uv_keymsk_before: u32,
    uv_keymsk_after: u32,
    uv_keymax_before: u32,
    uv_keymax_after: u32,
    y_keymsk_before: u32,
    y_keymsk_after: u32,
    y_keymax_before: u32,
    y_keymax_after: u32,
}

impl OverlaySurfacePool {
    const fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            pipe_slot: usize::MAX,
            front_index: None,
            surfaces: [None; OVERLAY_SWAP_BUFFER_COUNT],
            damage_debt: [CompositionDamageRegion::EMPTY; OVERLAY_SWAP_BUFFER_COUNT],
            retiring_front: None,
        }
    }

    fn matches(self, width: u32, height: u32, pipe: PipeInfo) -> bool {
        self.width == width && self.height == height && self.pipe_slot == pipe.slot
    }
}

impl PrimarySwapSurfacePool {
    const fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            pipe_slot: usize::MAX,
            front_index: None,
            surfaces: [None; PRIMARY_SWAP_BUFFER_COUNT],
            damage_debt: [CompositionDamageRegion::EMPTY; PRIMARY_SWAP_BUFFER_COUNT],
        }
    }

    fn matches(self, width: u32, height: u32, pipe: PipeInfo) -> bool {
        self.width == width && self.height == height && self.pipe_slot == pipe.slot
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum OverlayAlphaMode {
    Opaque,
    /// GPU-authored premultiplied RGBA bytes (`AABBGGRR` as a u32).
    PremultipliedRgba,
}

const UI4_RGBA8_OVERLAY_CONTRACT: OverlayAlphaMode = OverlayAlphaMode::PremultipliedRgba;

pub(crate) fn init_primary_boot_surface(dev: crate::intel::Dev) {
    if PRIMARY_BOOT_SURFACE_INIT.swap(true, Ordering::AcqRel) {
        return;
    }

    log_pipe_scanout_probe(dev, "before-primary-init");
    log_transcoder_a_state(dev, "before-primary-init");
    log_display_pipeline_topology(dev, "before-primary-init");

    let Some(pipe) = active_pipe(dev) else {
        crate::log!("intel/display: primary-boot-surface skipped no active pipe discovered\n");
        return;
    };
    let pipe_src_raw = crate::intel::mmio_read(dev, pipe.pipe_src_off);
    let pipe_src_dims = decode_pipe_src(pipe_src_raw);
    let fb_dims = framebuffer_hint();
    let chosen = pipe_src_dims
        .map(|(width, height)| (width, height, "pipe-src"))
        .or_else(|| fb_dims.map(|(width, height)| (width, height, "fb-hint")));
    let Some((width, height, chosen_from)) = chosen else {
        crate::log!(
            "intel/display: primary-boot-surface skipped no dimensions pipe={}\n",
            pipe.name
        );
        return;
    };
    log_primary_dimensions_probe(pipe.name, pipe_src_raw, pipe_src_dims, fb_dims, chosen_from);
    program_pipe_bottom_color(dev, pipe, PIPE_BOTTOM_COLOR_RAW);

    let backing_width = width.saturating_add(PRIMARY_GPGPU_EDGE_GUARD_PIXELS);
    let backing_height = height.saturating_add(PRIMARY_GPGPU_EDGE_GUARD_PIXELS);
    let Some(pitch_bytes) = aligned_pitch_bytes(backing_width, PRIMARY_BYTES_PER_PIXEL) else {
        crate::log!("intel/display: primary-boot-surface skipped bad pitch width={}\n", width);
        return;
    };
    let Some(byte_len) = usize::try_from(u64::from(pitch_bytes) * u64::from(backing_height)).ok()
    else {
        crate::log!("intel/display: primary-boot-surface skipped surface too large\n");
        return;
    };
    let Some(primary_gpu) = primary_surface_gpu_for_pipe(pipe) else {
        crate::log_warn!(
            target: "intel/display";
            "intel/display: primary-boot-surface skipped pipeline_slot={} potential_reason=no-reserved-primary-gpu-address\n",
            pipe.slot,
        );
        return;
    };
    let gpu_capacity = primary_surface_gpu_capacity(pipe);
    if byte_len as u64 > gpu_capacity {
        crate::log_warn!(
            target: "intel/display";
            "intel/display: primary-boot-surface skipped pipe={} size={}x{} backing={}x{} bytes=0x{:X} reserved_slot_bytes=0x{:X} potential_reason=mode-exceeds-primary-pipeline-gpu-address-slot\n",
            pipe.name,
            width,
            height,
            backing_width,
            backing_height,
            byte_len,
            gpu_capacity,
        );
        return;
    }
    let Some((phys, virt)) = crate::dma::alloc(byte_len, crate::intel::WARM_ALIGN) else {
        crate::log!("intel/display: primary-boot-surface alloc failed bytes=0x{:X}\n", byte_len);
        return;
    };

    crate::intel::dma_flush(virt, byte_len);

    if !crate::intel::map_display_scanout_ggtt(dev, phys, byte_len, primary_gpu) {
        crate::log!(
            "intel/display: primary-boot-surface ggtt map failed bytes=0x{:X} gpu=0x{:X}\n",
            byte_len,
            primary_gpu
        );
        let _ = crate::intel::unmap_display_scanout_ggtt(dev, byte_len, primary_gpu);
        crate::dma::dealloc(virt, byte_len);
        return;
    }
    crate::intel::ggtt_invalidate(dev);

    let Some(_stride_reg) = plane_stride_reg_value(pitch_bytes) else {
        crate::log!(
            "intel/display: primary-boot-surface stride encode failed pitch=0x{:X}\n",
            pitch_bytes
        );
        let _ = crate::intel::unmap_display_scanout_ggtt(dev, byte_len, primary_gpu);
        crate::dma::dealloc(virt, byte_len);
        return;
    };
    let Some(_surface_reg) = u32::try_from(primary_gpu).ok() else {
        crate::log!("intel/display: primary-boot-surface gpu addr out of range\n");
        let _ = crate::intel::unmap_display_scanout_ggtt(dev, byte_len, primary_gpu);
        crate::dma::dealloc(virt, byte_len);
        return;
    };

    let ctl_before = crate::intel::mmio_read(dev, pipe.primary_plane().ctl());
    let surf_before = crate::intel::mmio_read(dev, pipe.primary_plane().surf());
    let primary_surface = PrimarySurface {
        width,
        height,
        backing_width,
        backing_height,
        pitch_bytes,
        byte_len,
        phys,
        virt,
        gpu: primary_gpu,
        pipe,
    };
    *primary_surface_owner(pipe).lock() = Some(primary_surface);
    log_primary_scanout_pte_window(dev, "after-primary-init", primary_gpu, byte_len);

    log_primary_plane_probe(dev, pipe, "before-rgba8-stack-bootstrap");
    let ok = bootstrap_ui4_rgba8_plane_stack_once(dev, primary_surface);
    log_primary_plane_probe(dev, pipe, "after-rgba8-stack-bootstrap");
    log_pipe_scanout_probe(dev, "after-primary-init");
    let surf_armed = crate::intel::mmio_read(dev, pipe.primary_plane().surf());
    let surf_live = crate::intel::mmio_read(dev, pipe.primary_plane().surf_live());
    let ctl_after = crate::intel::mmio_read(dev, pipe.primary_plane().ctl());

    let logo_ok = if PRIMARY_BOOT_LOGO_ENABLED {
        let warmup_ok = if PRIMARY_BOOT_DISPLAY_WARMUP_ENABLED {
            run_primary_display_warmup(primary_surface, false)
        } else {
            false
        };
        let logo_submitted = probe_boot_logo_decode();
        if !logo_submitted && warmup_ok {
            mark_hw_logo_sequence_done("display-warmup-no-logo");
        }
        logo_submitted || warmup_ok
    } else if PRIMARY_BOOT_DISPLAY_WARMUP_ENABLED {
        run_primary_display_warmup(primary_surface, true)
    } else {
        false
    };
    if !logo_ok {
        mark_hw_logo_sequence_done("not-started");
    }

    crate::log!(
        "intel/display: primary-boot-surface pipe={} size={}x{} backing={}x{} pitch=0x{:X} bytes=0x{:X} guard={} gpu=0x{:X} phys=0x{:X} plane_enabled={} ctl_before=0x{:08X} ctl_after=0x{:08X} surf_before=0x{:08X} surf=0x{:08X} surf_live=0x{:08X} ok={} logo={} overlays=transparent-native-rgba8-slots1-4 ui=bootstrap-stack-ready\n",
        pipe.name,
        width,
        height,
        backing_width,
        backing_height,
        pitch_bytes,
        byte_len,
        PRIMARY_GPGPU_EDGE_GUARD_PIXELS,
        primary_gpu,
        phys,
        ((ctl_after & PLANE_CTL_ENABLE) != 0) as u8,
        ctl_before,
        ctl_after,
        surf_before,
        surf_armed,
        surf_live,
        ok as u8,
        logo_ok as u8,
    );
    log_display_pipeline_topology(dev, "after-primary-init");
}

/// Probe the firmware-captured monitor metadata while the BSP still owns
/// bring-up. This is read-only: Limine already obtained the EDID through the
/// active firmware display path, so probing cannot disturb the live DDI link.
pub(super) fn log_bsp_display_metrics_probe(dev: crate::intel::Dev) {
    let snapshots = display_pipeline_snapshots_for_dev(dev);
    let active_target = select_compatibility_pipeline_from_snapshots(&snapshots)
        .and_then(|selection| selection.snapshot.target);
    display_metrics::log_bsp_display_metrics_probe(active_target);
}

fn stamp_horizon_logo_top_left_screen() -> bool {
    if !PRIMARY_BOOT_HORIZON_STAMP_ENABLED {
        return false;
    }

    let stamp = match crate::graphics::png_codec::decode_png_rgba(PRIMARY_BOOT_HORIZON_STAMP_PNG) {
        Ok(stamp) => stamp,
        Err(err) => {
            crate::log!(
                "intel/display: boot-logo horizon stamp decode failed code={} bytes=0x{:X}\n",
                err.code(),
                PRIMARY_BOOT_HORIZON_STAMP_PNG.len()
            );
            return false;
        }
    };

    let stamped = blend_rgba_primary_rect(
        stamp.rgba.as_slice(),
        stamp.width,
        stamp.height,
        stamp.width as usize * 4,
        0,
        0,
        0,
        0,
        stamp.width,
        stamp.height,
        "boot-logo-horizon-stamp-top-left-screen",
    );
    crate::log!(
        "intel/display: boot-logo horizon stamp src={}x{} dst=0,0 screen=top-left stored={}\n",
        stamp.width,
        stamp.height,
        stamped as u8
    );
    stamped
}

fn stamp_bgrt_logo_bottom_right_screen() -> bool {
    let Some((bgrt_width, bgrt_height, bgrt_pixels)) = crate::efi::acpi::bgrt::decoded_logo_rgba()
    else {
        crate::log!("intel/display: boot-logo bgrt stamp skipped reason=no-bgrt-logo\n");
        return false;
    };

    let Some(surface) = active_primary_surface() else {
        return false;
    };
    if surface.virt.is_null()
        || surface.width == 0
        || surface.height == 0
        || surface.pitch_bytes < surface.width.saturating_mul(4)
        || bgrt_width == 0
        || bgrt_height == 0
    {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    let copy_w = bgrt_width.min(dst_width);
    let copy_h = bgrt_height.min(dst_height);
    if copy_w == 0 || copy_h == 0 || bgrt_pixels.len() < bgrt_width.saturating_mul(bgrt_height) {
        return false;
    }

    let dst_x = dst_width.saturating_sub(copy_w);
    let dst_y = dst_height.saturating_sub(copy_h);
    let src_x = bgrt_width.saturating_sub(copy_w);
    let src_y = bgrt_height.saturating_sub(copy_h);

    for row in 0..copy_h {
        let src_row = src_y.saturating_add(row).saturating_mul(bgrt_width);
        let dst_row_off = dst_y
            .saturating_add(row)
            .saturating_mul(dst_pitch)
            .saturating_add(dst_x.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col in 0..copy_w {
            let rgb = bgrt_pixels[src_row.saturating_add(src_x).saturating_add(col)];
            let r = ((rgb >> 16) & 0xFF) as u8;
            let g = ((rgb >> 8) & 0xFF) as u8;
            let b = (rgb & 0xFF) as u8;
            unsafe {
                core::ptr::write_volatile(dst_row.add(col), u32::from_le_bytes([b, g, r, 0]));
            }
        }
    }

    let flush_offset = dst_y
        .saturating_mul(dst_pitch)
        .saturating_add(dst_x.saturating_mul(4));
    let flush_bytes = copy_h
        .saturating_sub(1)
        .saturating_mul(dst_pitch)
        .saturating_add(copy_w.saturating_mul(4));
    let stamped = notify_primary_surface_external_write(
        "boot-logo-bgrt-stamp-bottom-right-screen",
        flush_offset,
        flush_bytes,
    );
    crate::log!(
        "intel/display: boot-logo bgrt stamp src={}x{} dst={},{} {}x{} screen=bottom-right stored={}\n",
        bgrt_width,
        bgrt_height,
        dst_x,
        dst_y,
        copy_w,
        copy_h,
        stamped as u8
    );
    stamped
}

/// Place one solid bar per universal-plane slot. Slots fill downward first,
/// then continue in columns toward the left. If the scanout cannot fit a full
/// additional bar, return `None` rather than overlap an existing slot marker.
pub(super) fn native_plane_slot_bar_screen_position(
    slot: usize,
    scanout_width: u32,
    scanout_height: u32,
) -> Option<(u32, u32)> {
    let usable_width = scanout_width.checked_sub(NATIVE_PLANE_SLOT_BAR_MARGIN.checked_mul(2)?)?;
    let usable_height = scanout_height.checked_sub(NATIVE_PLANE_SLOT_BAR_MARGIN.checked_mul(2)?)?;
    if usable_width < NATIVE_PLANE_SLOT_BAR_WIDTH || usable_height < NATIVE_PLANE_SLOT_BAR_HEIGHT {
        return None;
    }

    let row_stride = NATIVE_PLANE_SLOT_BAR_HEIGHT.checked_add(NATIVE_PLANE_SLOT_BAR_GAP)?;
    let column_stride = NATIVE_PLANE_SLOT_BAR_WIDTH.checked_add(NATIVE_PLANE_SLOT_BAR_GAP)?;
    let rows_per_column = 1 + usable_height
        .saturating_sub(NATIVE_PLANE_SLOT_BAR_HEIGHT)
        .checked_div(row_stride)?;
    let slot = u32::try_from(slot).ok()?;
    let row = slot % rows_per_column;
    let column = slot / rows_per_column;
    let column_offset = column.checked_mul(column_stride)?;
    if column_offset.checked_add(NATIVE_PLANE_SLOT_BAR_WIDTH)? > usable_width {
        return None;
    }

    let x = scanout_width
        .checked_sub(NATIVE_PLANE_SLOT_BAR_MARGIN)?
        .checked_sub(NATIVE_PLANE_SLOT_BAR_WIDTH)?
        .checked_sub(column_offset)?;
    let y = NATIVE_PLANE_SLOT_BAR_MARGIN.checked_add(row.checked_mul(row_stride)?)?;
    Some((x, y))
}

fn stamp_primary_plane_slot_bar(surface: PrimarySurface, slot: usize) -> bool {
    if surface.virt.is_null() || surface.width == 0 || surface.height == 0 {
        return false;
    }
    let (scanout_width, scanout_height) =
        active_scanout_dimensions().unwrap_or((surface.width, surface.height));
    let Some((dst_x, dst_y)) =
        native_plane_slot_bar_screen_position(slot, scanout_width, scanout_height)
    else {
        return false;
    };
    let copy_width = NATIVE_PLANE_SLOT_BAR_WIDTH.min(surface.width.saturating_sub(dst_x));
    let copy_height = NATIVE_PLANE_SLOT_BAR_HEIGHT.min(surface.height.saturating_sub(dst_y));
    if copy_width == 0 || copy_height == 0 {
        return false;
    }
    let pitch_pixels = surface.pitch_bytes as usize / 4;
    for y in 0..copy_height {
        let row = unsafe {
            (surface.virt as *mut u32)
                .add(dst_y.saturating_add(y) as usize * pitch_pixels + dst_x as usize)
        };
        for x in 0..copy_width {
            unsafe {
                core::ptr::write_volatile(row.add(x as usize), NATIVE_PLANE_SLOT_BAR_XRGB);
            }
        }
    }
    let flush_offset = dst_y as usize * surface.pitch_bytes as usize + dst_x as usize * 4;
    let flush_bytes = copy_height.saturating_sub(1) as usize * surface.pitch_bytes as usize
        + copy_width as usize * 4;
    notify_primary_surface_external_write(
        "boot-native-plane-slot-bar-p0",
        flush_offset,
        flush_bytes,
    )
}

fn stamp_overlay_plane_slot_bar(
    dev: crate::intel::Dev,
    primary: PrimarySurface,
    slot: usize,
) -> bool {
    let Some(surface) = ensure_overlay_surface_for_pipe(
        dev,
        primary.pipe,
        slot,
        NATIVE_PLANE_SLOT_BAR_WIDTH,
        NATIVE_PLANE_SLOT_BAR_HEIGHT,
    ) else {
        return false;
    };
    let pitch_pixels = surface.pitch_bytes as usize / 4;
    for y in 0..surface.height {
        let row = unsafe { (surface.virt as *mut u32).add(y as usize * pitch_pixels) };
        for x in 0..surface.width {
            unsafe {
                core::ptr::write_volatile(row.add(x as usize), NATIVE_PLANE_SLOT_BAR_XRGB);
            }
        }
    }
    crate::intel::dma_flush(surface.virt, surface.byte_len);
    let (scanout_width, scanout_height) =
        active_scanout_dimensions().unwrap_or((primary.width, primary.height));
    let Some((pos_x, pos_y)) =
        native_plane_slot_bar_screen_position(slot, scanout_width, scanout_height)
    else {
        return false;
    };
    let reason = "boot-native-plane-slot-bar-p1";
    present_overlay_surface_with_bootstrap_contract(
        dev,
        surface,
        pos_x,
        pos_y,
        UI4_RGBA8_OVERLAY_CONTRACT,
        reason,
    )
}

/// P0 is written into the primary backing and P1 into the exact surface armed
/// on universal plane 1. No allocation, font tessellation, or render-engine
/// submission is involved in generating either identity.
fn stamp_boot_native_plane_slot_bars_top_right() -> bool {
    if !NATIVE_PLANE_SLOT_BARS_ENABLED {
        return false;
    }
    let Some(primary) = active_primary_surface() else {
        return false;
    };
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let (scanout_width, scanout_height) =
        active_scanout_dimensions().unwrap_or((primary.width, primary.height));
    let primary_stamped = stamp_primary_plane_slot_bar(primary, 0);
    let overlay_stamped = stamp_overlay_plane_slot_bar(dev, primary, UI_OVERLAY_PLANE_SLOT);
    crate::log!(
        "intel/display: native-plane-slot-bars boot generator=cpu-inline render_submits=0 pipe={} scanout={}x{} bar={}x{} p0={} p1={} universal_slots={} cursor=excluded placement=top-right-nonoverlap margin={} gap={}\n",
        primary.pipe.name,
        scanout_width,
        scanout_height,
        NATIVE_PLANE_SLOT_BAR_WIDTH,
        NATIVE_PLANE_SLOT_BAR_HEIGHT,
        primary_stamped as u8,
        overlay_stamped as u8,
        UNIVERSAL_PLANE_SLOTS,
        NATIVE_PLANE_SLOT_BAR_MARGIN,
        NATIVE_PLANE_SLOT_BAR_GAP,
    );
    primary_stamped && overlay_stamped
}

fn run_primary_display_warmup(_surface: PrimarySurface, release_render_after: bool) -> bool {
    if release_render_after {
        mark_hw_logo_sequence_done("display-warmup");
    }
    crate::log!("intel/display: primary-display-warmup skipped reason=no-initial-white-fill\n");
    true
}

pub(crate) async fn wait_hw_logo_sequence_done() {
    if !PRIMARY_BOOT_LOGO_ENABLED {
        return;
    }
    while !HW_LOGO_SEQUENCE_DONE.load(Ordering::Acquire) {
        if !HW_LOGO_SEQUENCE_DONE_WAIT
            .wait_for_event_timeout(PRIMARY_BOOT_LOGO_WAIT_TIMEOUT_MS)
            .await
        {
            mark_hw_logo_sequence_done("logo-wait-timeout");
            return;
        }
    }
}

fn mark_hw_logo_sequence_done(reason: &'static str) {
    if HW_LOGO_SEQUENCE_DONE.swap(true, Ordering::AcqRel) {
        return;
    }
    crate::log!("intel/display: hw-logo sequence done reason={}\n", reason);
    HW_LOGO_SEQUENCE_DONE_WAIT.notify_all();
}

fn submit_next_hw_logo_stage() -> bool {
    let stage_idx = HW_LOGO_NEXT_STAGE.fetch_add(1, Ordering::AcqRel) as usize;
    if stage_idx != 0 {
        return false;
    }
    submit_hw_logo_stage("logo", PRIMARY_BOOT_LOGO_JPEG)
}

fn submit_hw_logo_stage(name: &'static str, jpeg: &'static [u8]) -> bool {
    match crate::intel::hw_pic_submit_jpeg(jpeg) {
        Ok(id) => {
            HW_LOGO_PENDING_IDS.lock().push_back(id);
            HW_LOGO_WAIT.notify_all();
            let snap = crate::intel::hw_pic_snapshot();
            crate::log!(
                "intel/display: hw-logo submit ok stage={} id={} bytes=0x{:X} pending={} outputs={} service={}\n",
                name,
                id,
                jpeg.len(),
                snap.pending,
                snap.outputs,
                snap.service_started as u8
            );
            true
        }
        Err(code) => {
            let snap = crate::intel::hw_pic_snapshot();
            crate::log!(
                "intel/display: hw-logo submit failed stage={} code={} bytes=0x{:X} pending={} outputs={} service={}\n",
                name,
                code,
                jpeg.len(),
                snap.pending,
                snap.outputs,
                snap.service_started as u8
            );
            false
        }
    }
}

#[embassy_executor::task]
pub(crate) async fn hw_logo_present_task() {
    loop {
        let pending_id = HW_LOGO_PENDING_IDS.lock().pop_front();
        let Some(pending_id) = pending_id else {
            HW_LOGO_WAIT.wait_for_event().await;
            continue;
        };

        let Some(output) = crate::intel::hw_pic_wait_output_for_id(pending_id, 0).await else {
            continue;
        };

        let (visible_x, visible_y, visible_width, visible_height, target_width, target_height) =
            if output.width != 0 && output.height != 0 {
                if let Some(surface) = active_primary_surface() {
                    let source_width = output.visible_width.max(1).min(output.width);
                    let source_height = output.visible_height.max(1).min(output.height);
                    if JPG_CENTER_CROP
                        && (source_width > surface.width || source_height > surface.height)
                    {
                        let (crop_w, crop_h) = center_crop_size(
                            source_width as usize,
                            source_height as usize,
                            surface.width as usize,
                            surface.height as usize,
                        );
                        (
                            source_width.saturating_sub(crop_w as u32) / 2,
                            source_height.saturating_sub(crop_h as u32) / 2,
                            crop_w as u32,
                            crop_h as u32,
                            surface.width as usize,
                            surface.height as usize,
                        )
                    } else {
                        let (fit_w, fit_h) = aspect_fit_size(
                            source_width as usize,
                            source_height as usize,
                            surface.width as usize,
                            surface.height as usize,
                        );
                        (0, 0, source_width, source_height, fit_w, fit_h)
                    }
                } else {
                    (0, 0, 0, 0, 0, 0)
                }
            } else {
                (0, 0, 0, 0, 0, 0)
            };

        let stored = if matches!(
            output.status,
            crate::intel::hw_pic::HwPicStatus::Ready | crate::intel::hw_pic::HwPicStatus::Streamed
        ) && matches!(
            output.format,
            crate::intel::hw_pic::HwPicPixelFormat::Imc3
                | crate::intel::hw_pic::HwPicPixelFormat::Nv12
        ) && output.width != 0
            && output.height != 0
            && output.visible_width != 0
            && output.visible_height != 0
            && output.pitch_bytes != 0
            && output.byte_len != 0
            && output.virt_addr != 0
        {
            let src = unsafe {
                core::slice::from_raw_parts(output.virt_addr as *const u8, output.byte_len)
            };
            match output.format {
                crate::intel::hw_pic::HwPicPixelFormat::Imc3 => present_imc3_surface_center(
                    src,
                    output.width,
                    output.height,
                    visible_x,
                    visible_y,
                    output.visible_width.min(visible_width),
                    output.visible_height.min(visible_height),
                    output.pitch_bytes,
                ),
                crate::intel::hw_pic::HwPicPixelFormat::Nv12 => present_nv12_surface_center(
                    src,
                    output.width,
                    output.height,
                    visible_x,
                    visible_y,
                    output.visible_width.min(visible_width),
                    output.visible_height.min(visible_height),
                    output.pitch_bytes,
                ),
                _ => false,
            }
        } else {
            false
        };

        let plane_bars_stamped = stored
            && PRIMARY_BOOT_NATIVE_PLANE_SLOT_BARS_ENABLED
            && stamp_boot_native_plane_slot_bars_top_right();
        crate::log!(
            "intel/display: hw-logo output id={} status={:?} fmt={:?} decoded={}x{} visible={}x{} target={}x{} pitch=0x{:X} uv=0x{:X} bytes=0x{:X} gpu=0x{:X} phys=0x{:X} stored={} plane_bars={} err={}\n",
            output.id,
            output.status,
            output.format,
            output.width,
            output.height,
            output.visible_width,
            output.visible_height,
            target_width,
            target_height,
            output.pitch_bytes,
            output.uv_offset,
            output.byte_len,
            output.gpu_addr,
            output.phys_addr,
            stored as u8,
            plane_bars_stamped as u8,
            output.error_code,
        );

        if stored {
            Timer::after(EmbassyDuration::from_millis(PRIMARY_BOOT_LOGO_PRESENT_HOLD_MS)).await;
        }
        if !submit_next_hw_logo_stage() {
            mark_hw_logo_sequence_done("stages-drained");
        }
    }
}

fn log_transcoder_a_state(dev: crate::intel::Dev, label: &str) {
    let pipe_src = crate::intel::mmio_read(dev, PIPE_A_SRC);
    let pipeconf = crate::intel::mmio_read(dev, PIPECONF_A);
    let htotal = crate::intel::mmio_read(dev, TRANS_HTOTAL_A);
    let hsync = crate::intel::mmio_read(dev, TRANS_HSYNC_A);
    let vtotal = crate::intel::mmio_read(dev, TRANS_VTOTAL_A);
    let vsync = crate::intel::mmio_read(dev, TRANS_VSYNC_A);
    let ddi_func_ctl = crate::intel::mmio_read(dev, TRANS_DDI_FUNC_CTL_A);
    let ddi_select = (ddi_func_ctl >> 27) & 0x07;
    let mode_select = (ddi_func_ctl >> 24) & 0x07;
    let bits_per_color = (ddi_func_ctl >> 20) & 0x03;
    let sync_polarity = (ddi_func_ctl >> 16) & 0x03;
    let port_width = (ddi_func_ctl >> 1) & 0x07;
    intel_display_verbose_log!(
        "intel/display: transcoder-a label={} pipe_src=0x{:08X} pipeconf=0x{:08X} pipe_enable={} pipe_state={} ddi_func_ctl=0x{:08X} trans_enable={} ddi_select={} ddi={} mode_select={} mode={} bpc={} sync_pol=0x{:X} port_width={} htotal=0x{:08X} hsync=0x{:08X} vtotal=0x{:08X} vsync=0x{:08X}\n",
        label,
        pipe_src,
        pipeconf,
        ((pipeconf >> 31) & 1),
        ((pipeconf >> 30) & 1),
        ddi_func_ctl,
        ((ddi_func_ctl >> 31) & 1),
        ddi_select,
        decode_trans_ddi_select(ddi_select),
        mode_select,
        decode_trans_ddi_mode(mode_select),
        decode_trans_bits_per_color(bits_per_color),
        sync_polarity,
        port_width,
        htotal,
        hsync,
        vtotal,
        vsync
    );
}

fn log_display_pipeline_topology(dev: crate::intel::Dev, label: &str) {
    let snapshots = display_pipeline_snapshots_for_dev(dev);
    let selection = select_compatibility_pipeline_from_snapshots(&snapshots);
    for snapshot in snapshots {
        let (width, height) = snapshot
            .target
            .map(|target| (target.width, target.height))
            .unwrap_or((0, 0));
        let selected = selection
            .map(|selection| selection.snapshot.pipeline == snapshot.pipeline)
            .unwrap_or(false);
        let selection_reason = selection
            .filter(|selection| selection.snapshot.pipeline == snapshot.pipeline)
            .map(|selection| selection.reason)
            .unwrap_or("-");
        crate::log!(
            "intel/display: pipeline-topology label={} pipeline={} slot={} state={} mode={}x{} pipe_enable={} transcoder_enable={} primary_enable={} primary_bound={} ddi={} link_mode={} bpc={} sync_pol=0x{:X} port_width={} compat_selected={} selection_reason={} pipe_src=0x{:08X} pipeconf=0x{:08X} transcoder=0x{:08X} primary_ctl=0x{:08X} surf=0x{:08X} live=0x{:08X} connector=unresolved\n",
            label,
            snapshot.pipeline.name(),
            snapshot.pipeline.slot(),
            snapshot.activity.name(),
            width,
            height,
            snapshot.pipe_enabled as u8,
            snapshot.transcoder_enabled as u8,
            snapshot.primary_enabled as u8,
            snapshot.primary_bound as u8,
            snapshot.route.ddi.name(),
            snapshot.route.mode_name(),
            snapshot.route.bits_per_color(),
            snapshot.route.sync_polarity,
            snapshot.route.port_width,
            selected as u8,
            selection_reason,
            snapshot.pipe_src,
            snapshot.pipeconf,
            snapshot.transcoder,
            snapshot.primary_ctl,
            snapshot.primary_surf,
            snapshot.primary_live,
        );
    }
    let output_targets = display_output_targets_from_snapshots(&snapshots);
    for output_slot in 0..DISPLAY_OUTPUT_COUNT {
        let output = DisplayOutputId::from_slot(output_slot).expect("static display output slot");
        match output_targets[output_slot] {
            Some(target) => crate::log!(
                "intel/display: output-topology label={} output={} slot={} assignment=provisional-pipeline-route pipeline={} state={} mode={}x{} ddi={} link_mode={} bpc={} connector=unresolved monitor_identity=unresolved\n",
                label,
                output.name(),
                output.slot(),
                target.pipeline_target.pipeline.name(),
                target.pipeline_target.activity.name(),
                target.pipeline_target.width,
                target.pipeline_target.height,
                target.pipeline_target.route.ddi.name(),
                target.pipeline_target.route.mode_name(),
                target.pipeline_target.route.bits_per_color(),
            ),
            None => crate::log!(
                "intel/display: output-topology label={} output={} slot={} assignment=unassigned connector=unresolved monitor_identity=unresolved\n",
                label,
                output.name(),
                output.slot(),
            ),
        }
    }
    if let Some(selection) = selection {
        log_compatibility_pipeline_selection(selection);
    }
}

fn decode_trans_ddi_select(v: u32) -> &'static str {
    DisplayDdiRoute::from_select(v).name()
}

fn decode_trans_ddi_mode(v: u32) -> &'static str {
    match v {
        0 => "hdmi",
        1 => "dvi",
        2 => "dp-sst",
        3 => "dp-mst",
        4 => "fdi-or-reserved",
        _ => "unknown",
    }
}

fn decode_trans_bits_per_color(v: u32) -> u32 {
    match v {
        0 => 8,
        1 => 10,
        2 => 6,
        3 => 12,
        _ => 0,
    }
}

fn program_pipe_bottom_color(dev: crate::intel::Dev, pipe: PipeInfo, raw: u32) {
    let reg = SKL_BOTTOM_COLOR_A + pipe.slot * SKL_BOTTOM_COLOR_PIPE_STRIDE;
    crate::intel::mmio_write(dev, reg, raw);
    let readback = crate::intel::mmio_read(dev, reg);
    intel_display_verbose_log!(
        "intel/display: bottom-color pipe={} reg=0x{:05X} raw=0x{:08X} readback=0x{:08X}\n",
        pipe.name,
        reg,
        raw,
        readback
    );
}

fn pipe_bottom_color_from_xrgb(color: u32) -> u32 {
    let red = ((color >> 16) & 0xFF) * 0x3FF / 0xFF;
    let green = ((color >> 8) & 0xFF) * 0x3FF / 0xFF;
    let blue = (color & 0xFF) * 0x3FF / 0xFF;
    pipe_bottom_color_u0_10(red, green, blue)
}

pub(crate) fn active_scanout_dimensions() -> Option<(u32, u32)> {
    let target = primary_display_output_target()?.pipeline_target;
    Some((target.width, target.height))
}

/// Resolve a physical extent against the active mode and validated boot EDID.
/// Callers retain their own fallback policy for displays which omit size data.
pub(crate) fn physical_extent_pixels(width_mm: u32, height_mm: u32) -> Option<(u32, u32)> {
    let target = primary_display_output_target()?.pipeline_target;
    display_metrics::physical_extent_pixels(target, width_mm, height_mm)
}

/// Compatibility wrapper for hardware-oriented callers. New compositor and
/// scene owners should retain the logical output returned by
/// `primary_display_output_target` through their whole frame transaction.
pub(super) fn active_display_pipeline_target() -> Option<DisplayPipelineTarget> {
    Some(primary_display_output_target()?.pipeline_target)
}

/// The primary logical output used by today's single-output services. D01 is
/// stable at the compositor boundary even when compatibility policy selects a
/// different hardware pipe after a future route change.
pub(super) fn primary_display_output_target() -> Option<DisplayOutputTarget> {
    display_output_target(DisplayOutputId::from_slot(0).expect("primary display output slot"))
}

/// Returns the current route lease for one logical output slot.
pub(super) fn display_output_target(output: DisplayOutputId) -> Option<DisplayOutputTarget> {
    let dev = crate::intel::claimed_device()?;
    let snapshots = display_pipeline_snapshots_for_dev(dev);
    let selection = select_compatibility_pipeline_from_snapshots(&snapshots)?;
    log_compatibility_pipeline_selection(selection);
    display_output_targets_from_snapshots_with_selection(&snapshots, selection)[output.slot()]
}

/// Fixed four-slot compositor topology. D01 receives the best compatibility
/// route so current single-monitor behavior remains unchanged. Additional
/// complete scanout routes fill D02-D04 in stable pipeline order; incomplete
/// secondary routes are intentionally not exposed as monitors.
pub(super) fn display_output_targets() -> [Option<DisplayOutputTarget>; DISPLAY_OUTPUT_COUNT] {
    let Some(dev) = crate::intel::claimed_device() else {
        return [None; DISPLAY_OUTPUT_COUNT];
    };
    let snapshots = display_pipeline_snapshots_for_dev(dev);
    let Some(selection) = select_compatibility_pipeline_from_snapshots(&snapshots) else {
        return [None; DISPLAY_OUTPUT_COUNT];
    };
    log_compatibility_pipeline_selection(selection);
    display_output_targets_from_snapshots_with_selection(&snapshots, selection)
}

/// Returns the currently programmed target for a stable A-D pipeline slot.
/// Connector discovery remains a separate layer; callers never need to infer
/// ownership from whichever pipe happened to be discovered first.
pub(super) fn display_pipeline_target(
    pipeline: DisplayPipelineId,
) -> Option<DisplayPipelineTarget> {
    let dev = crate::intel::claimed_device()?;
    display_pipeline_target_for_pipe(dev, pipeline.pipe()?)
}

/// Four-slot topology view for compositor policy. An empty entry means no
/// usable mode is currently programmed on that hardware pipeline.
pub(super) fn display_pipeline_targets() -> [Option<DisplayPipelineTarget>; DISPLAY_PIPELINE_COUNT]
{
    let Some(dev) = crate::intel::claimed_device() else {
        return [None; DISPLAY_PIPELINE_COUNT];
    };
    display_pipeline_snapshots_for_dev(dev).map(|snapshot| snapshot.target)
}

/// Full hardware topology view. Unlike `display_pipeline_targets`, inactive
/// or partially programmed slots remain visible to routing policy.
pub(super) fn display_pipeline_snapshots()
-> [Option<DisplayPipelineSnapshot>; DISPLAY_PIPELINE_COUNT] {
    let Some(dev) = crate::intel::claimed_device() else {
        return [None; DISPLAY_PIPELINE_COUNT];
    };
    display_pipeline_snapshots_for_dev(dev).map(Some)
}

fn display_pipeline_target_for_pipe(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
) -> Option<DisplayPipelineTarget> {
    display_pipeline_snapshot_for_pipe(dev, pipe).target
}

fn display_pipeline_snapshot_for_pipe(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
) -> DisplayPipelineSnapshot {
    let pipeline = DisplayPipelineId::from_pipe(pipe).expect("static display pipe slot");
    let pipe_src = crate::intel::mmio_read(dev, pipe.pipe_src_off);
    let pipeconf =
        crate::intel::mmio_read(dev, PIPECONF_A + pipe.slot.saturating_mul(PIPE_MMIO_STRIDE));
    let transcoder = crate::intel::mmio_read(
        dev,
        TRANS_DDI_FUNC_CTL_A + pipe.slot.saturating_mul(PIPE_MMIO_STRIDE),
    );
    let primary_ctl = crate::intel::mmio_read(dev, pipe.primary_plane().ctl());
    let primary_surf = crate::intel::mmio_read(dev, pipe.primary_plane().surf());
    let primary_live = crate::intel::mmio_read(dev, pipe.primary_plane().surf_live());
    let dimensions = decode_pipe_src(pipe_src)
        .or_else(|| primary_surface_for_pipe(pipe).map(|surface| (surface.width, surface.height)));
    let route = DisplayPipelineRoute::from_registers(
        pipeconf,
        transcoder,
        primary_ctl,
        primary_surf,
        primary_live,
    );
    let observed = dimensions.is_some()
        || pipeconf != 0
        || transcoder != 0
        || primary_ctl != 0
        || primary_surf != 0
        || primary_live != 0;
    let activity = if dimensions.is_some() && route.complete() {
        DisplayPipelineActivity::Scanout
    } else if observed {
        DisplayPipelineActivity::Programmed
    } else {
        DisplayPipelineActivity::Inactive
    };
    let target = dimensions.map(|(width, height)| DisplayPipelineTarget {
        pipeline,
        width,
        height,
        route,
        activity,
    });

    DisplayPipelineSnapshot {
        pipeline,
        activity,
        target,
        route,
        pipe_enabled: route.pipe_enabled,
        transcoder_enabled: route.transcoder_enabled,
        primary_enabled: route.primary_enabled,
        primary_bound: route.primary_bound,
        pipe_src,
        pipeconf,
        transcoder,
        primary_ctl,
        primary_surf,
        primary_live,
        observed,
    }
}

fn display_pipeline_snapshots_for_dev(
    dev: crate::intel::Dev,
) -> [DisplayPipelineSnapshot; DISPLAY_PIPELINE_COUNT] {
    PIPES.map(|pipe| display_pipeline_snapshot_for_pipe(dev, pipe))
}

fn compatibility_pipeline_rank(snapshot: DisplayPipelineSnapshot) -> (u8, &'static str) {
    if snapshot.activity == DisplayPipelineActivity::Scanout {
        return (6, "complete-scanout");
    }
    if snapshot.target.is_some()
        && snapshot.pipe_enabled
        && snapshot.primary_enabled
        && snapshot.primary_bound
    {
        return (5, "pipe-primary-live-transcoder-incomplete");
    }
    if snapshot.primary_enabled && snapshot.primary_bound {
        return (4, "primary-live-mode-unresolved");
    }
    if snapshot.target.is_some() && (snapshot.pipe_enabled || snapshot.transcoder_enabled) {
        return (3, "enabled-mode-primary-incomplete");
    }
    if snapshot.target.is_some() {
        return (2, "programmed-mode-only");
    }
    if snapshot.observed {
        return (1, "register-state-only");
    }
    (0, "inactive")
}

fn select_compatibility_pipeline_from_snapshots(
    snapshots: &[DisplayPipelineSnapshot; DISPLAY_PIPELINE_COUNT],
) -> Option<CompatibilityPipelineSelection> {
    let mut selected = None;
    let mut selected_rank = 0;
    let mut selected_reason = "inactive";
    let mut candidate_mask = 0u8;
    let mut best_mask = 0u8;
    let mut best_count = 0u8;
    let mut scanout_mask = 0u8;

    for snapshot in snapshots.iter().copied() {
        let bit = 1u8 << snapshot.pipeline.slot();
        let (rank, reason) = compatibility_pipeline_rank(snapshot);
        if rank != 0 {
            candidate_mask |= bit;
        }
        if snapshot.activity == DisplayPipelineActivity::Scanout {
            scanout_mask |= bit;
        }
        if rank > selected_rank {
            selected = Some(snapshot);
            selected_rank = rank;
            selected_reason = reason;
            best_mask = bit;
            best_count = 1;
        } else if rank != 0 && rank == selected_rank {
            best_mask |= bit;
            best_count = best_count.saturating_add(1);
        }
    }

    Some(CompatibilityPipelineSelection {
        snapshot: selected?,
        rank: selected_rank,
        reason: selected_reason,
        candidate_mask,
        best_mask,
        best_count,
        scanout_mask,
    })
}

fn display_output_targets_from_snapshots(
    snapshots: &[DisplayPipelineSnapshot; DISPLAY_PIPELINE_COUNT],
) -> [Option<DisplayOutputTarget>; DISPLAY_OUTPUT_COUNT] {
    let Some(selection) = select_compatibility_pipeline_from_snapshots(snapshots) else {
        return [None; DISPLAY_OUTPUT_COUNT];
    };
    display_output_targets_from_snapshots_with_selection(snapshots, selection)
}

fn display_output_targets_from_snapshots_with_selection(
    snapshots: &[DisplayPipelineSnapshot; DISPLAY_PIPELINE_COUNT],
    selection: CompatibilityPipelineSelection,
) -> [Option<DisplayOutputTarget>; DISPLAY_OUTPUT_COUNT] {
    let mut outputs = [None; DISPLAY_OUTPUT_COUNT];
    let Some(primary_target) = selection.snapshot.target else {
        return outputs;
    };
    outputs[0] = Some(DisplayOutputTarget {
        output: DisplayOutputId::from_slot(0).expect("primary display output slot"),
        pipeline_target: primary_target,
    });

    let mut output_slot = 1usize;
    for snapshot in snapshots.iter().copied() {
        if output_slot >= DISPLAY_OUTPUT_COUNT {
            break;
        }
        if snapshot.pipeline == primary_target.pipeline
            || snapshot.activity != DisplayPipelineActivity::Scanout
        {
            continue;
        }
        let Some(pipeline_target) = snapshot.target else {
            continue;
        };
        outputs[output_slot] = Some(DisplayOutputTarget {
            output: DisplayOutputId::from_slot(output_slot).expect("static display output slot"),
            pipeline_target,
        });
        output_slot += 1;
    }
    outputs
}

fn log_compatibility_pipeline_selection(selection: CompatibilityPipelineSelection) {
    let route = selection.snapshot.route;
    let signature = (selection.snapshot.pipeline.slot() as u32)
        | ((selection.rank as u32) << 2)
        | ((selection.candidate_mask as u32) << 5)
        | ((selection.best_mask as u32) << 9)
        | ((selection.scanout_mask as u32) << 13)
        | ((route.ddi.select() as u32) << 17)
        | ((route.transcoder_mode as u32) << 20)
        | ((route.bits_per_color_select as u32) << 23)
        | ((route.sync_polarity as u32) << 25)
        | ((route.port_width as u32) << 27);
    if DISPLAY_PIPELINE_SELECTION_SIGNATURE.swap(signature, Ordering::AcqRel) == signature {
        return;
    }

    if selection.rank < 6 || selection.best_count > 1 {
        let potential_reason = if selection.best_count > 1 {
            "multiple-equally-ranked-pipelines-in-single-display-compatibility-policy"
        } else {
            "selected-pipeline-is-partially-programmed-or-route-is-incomplete"
        };
        crate::log_warn!(
            target: "intel/display";
            "intel/display: compatibility-pipeline selected={} rank={} reason={} candidates=0x{:X} best=0x{:X} scanout=0x{:X} ties={} potential_reason={} action=prefer-complete-scanout-then-lowest-stable-pipeline-slot\n",
            selection.snapshot.pipeline.name(),
            selection.rank,
            selection.reason,
            selection.candidate_mask,
            selection.best_mask,
            selection.scanout_mask,
            selection.best_count,
            potential_reason,
        );
    } else {
        crate::log_info!(
            target: "intel/display";
            "intel/display: compatibility-pipeline selected={} rank={} reason={} route={} link_mode={} bpc={} port_width={} mode={}x{} candidates=0x{:X} scanout=0x{:X}\n",
            selection.snapshot.pipeline.name(),
            selection.rank,
            selection.reason,
            selection.snapshot.route.ddi.name(),
            selection.snapshot.route.mode_name(),
            selection.snapshot.route.bits_per_color(),
            selection.snapshot.route.port_width,
            selection.snapshot.target.map(|target| target.width).unwrap_or(0),
            selection.snapshot.target.map(|target| target.height).unwrap_or(0),
            selection.candidate_mask,
            selection.scanout_mask,
        );
    }
}

fn select_compatibility_pipeline(dev: crate::intel::Dev) -> Option<CompatibilityPipelineSelection> {
    let snapshots = display_pipeline_snapshots_for_dev(dev);
    let selection = select_compatibility_pipeline_from_snapshots(&snapshots)?;
    log_compatibility_pipeline_selection(selection);
    Some(selection)
}

pub(crate) fn primary_surface_gpu_addr() -> Option<u64> {
    active_primary_surface().map(|surface| surface.gpu)
}

pub(crate) fn log_primary_surface_samples(label: &str) {
    let Some(surface) = active_primary_surface() else {
        return;
    };
    log_surface_samples(surface, label);
}

pub(crate) fn capture_primary_surface_samples() -> Option<PrimarySurfaceSampleSet> {
    let surface = active_primary_surface()?;
    capture_surface_samples(surface)
}

pub(crate) fn capture_primary_surface_bgra8() -> Option<PrimarySurfaceBgra8Snapshot> {
    let surface = active_primary_surface()?;
    let width = surface.width as usize;
    let height = surface.height as usize;
    let pitch_bytes = surface.pitch_bytes as usize;
    if width == 0 || height == 0 || pitch_bytes < width.checked_mul(4)? || surface.virt.is_null() {
        return None;
    }

    let row_bytes = width.checked_mul(4)?;
    let byte_len = pitch_bytes.checked_mul(height)?;
    let pixel_bytes = row_bytes.checked_mul(height)?;
    let mut pixels = Vec::new();
    if pixels.try_reserve_exact(pixel_bytes).is_err() {
        return None;
    }
    pixels.resize(pixel_bytes, 0);

    crate::intel::dma_flush(surface.virt, byte_len);
    for y in 0..height {
        let src_off = y.checked_mul(pitch_bytes)?;
        let dst_off = y.checked_mul(row_bytes)?;
        unsafe {
            core::ptr::copy_nonoverlapping(
                surface.virt.add(src_off),
                pixels.as_mut_ptr().add(dst_off),
                row_bytes,
            );
        }
    }

    Some(PrimarySurfaceBgra8Snapshot {
        width: surface.width,
        height: surface.height,
        pixels,
    })
}

pub(crate) fn sample_primary_surface_pixel(x: u32, y: u32) -> Option<u32> {
    let surface = active_primary_surface()?;
    sample_surface_pixel(surface, x as usize, y as usize)
}

pub(crate) fn clear_primary_surface_color(color: u32, reason: &str) -> bool {
    let Some(surface) = active_primary_surface() else {
        crate::log!(
            "intel/display: primary-clear skipped reason={} cause=no-primary-surface\n",
            reason,
        );
        return false;
    };
    if surface.virt.is_null()
        || surface.width == 0
        || surface.height == 0
        || surface.pitch_bytes == 0
    {
        crate::log!("intel/display: primary-clear skipped reason={} cause=bad-surface\n", reason,);
        return false;
    }

    let byte_len = (surface.pitch_bytes as usize).saturating_mul(surface.height as usize);
    if byte_len == 0 {
        crate::log!("intel/display: primary-clear skipped reason={} cause=empty-surface\n", reason,);
        return false;
    }

    fill_surface_color(
        surface.virt,
        surface.pitch_bytes as usize,
        surface.width,
        surface.height,
        color,
    );
    crate::intel::dma_flush(surface.virt, byte_len);
    let presented = notify_primary_surface_present(surface, reason, byte_len);
    crate::log!(
        "intel/display: primary-clear reason={} color=0x{:08X} size={}x{} pitch=0x{:X} bytes=0x{:X} presented={}\n",
        reason,
        color,
        surface.width,
        surface.height,
        surface.pitch_bytes,
        byte_len,
        presented as u8,
    );
    presented
}

pub(crate) fn present_i226_diagnostic_screen(
    snapshot: crate::net::i226::I226Snapshot,
    reason: &str,
) -> bool {
    let Some(surface) = active_primary_surface() else {
        crate::log!(
            "intel/display: i226-screen skipped reason={} cause=no-primary-surface\n",
            reason
        );
        return false;
    };
    if surface.virt.is_null()
        || surface.width == 0
        || surface.height == 0
        || surface.pitch_bytes == 0
    {
        crate::log!("intel/display: i226-screen skipped reason={} cause=bad-surface\n", reason);
        return false;
    }

    let byte_len = (surface.pitch_bytes as usize).saturating_mul(surface.height as usize);
    if byte_len == 0 {
        crate::log!("intel/display: i226-screen skipped reason={} cause=empty-surface\n", reason);
        return false;
    }

    fill_surface_color(
        surface.virt,
        surface.pitch_bytes as usize,
        surface.width,
        surface.height,
        0x00FF_FFFF,
    );

    let title_scale = if surface.width >= 1920 { 8 } else { 5 };
    let body_scale = if surface.width >= 1920 { 4 } else { 3 };
    let left = 72u32.min(surface.width.saturating_sub(1));
    let mut y = 72u32.min(surface.height.saturating_sub(1));
    let title = "NETWORK CARD";
    let title_pixels = draw_primary_text_line(surface, left, y, title_scale, title);
    y = y.saturating_add(title_scale.saturating_mul(11));
    let subtitle = "INTEL I226-V CLAIMED - PASSIVE DIAGNOSTIC MODE";
    let subtitle_pixels = draw_primary_text_line(surface, left, y, body_scale, subtitle);
    y = y.saturating_add(body_scale.saturating_mul(12));

    let mut lines: Vec<String> = Vec::new();
    lines.push(alloc::format!(
        "BDF {:02X}:{:02X}.{}  VID:PID {:04X}:{:04X}  REV {:02X}",
        snapshot.bus,
        snapshot.slot,
        snapshot.function,
        snapshot.vendor,
        snapshot.device,
        snapshot.revision
    ));
    lines.push(alloc::format!(
        "CLASS {:02X}:{:02X}.{:02X}  PCI CMD {:04X}->{:04X}  PCI STATUS {:04X}",
        snapshot.class,
        snapshot.subclass,
        snapshot.prog_if,
        snapshot.pci_command_before,
        snapshot.pci_command_after,
        snapshot.pci_status
    ));
    lines.push(alloc::format!(
        "BAR{} PHYS 0X{:X}  BAR SIZE 0X{:X}  MAP SIZE 0X{:X}",
        snapshot.bar_index,
        snapshot.bar_phys,
        snapshot.bar_size,
        snapshot.map_size
    ));
    lines.push(alloc::format!(
        "MAC {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
        snapshot.mac[0],
        snapshot.mac[1],
        snapshot.mac[2],
        snapshot.mac[3],
        snapshot.mac[4],
        snapshot.mac[5]
    ));
    lines.push(alloc::format!(
        "STATUS 0X{:08X}  LINK RAW={}  SPEED RAW={}MBIT  FULL DUPLEX RAW={}",
        snapshot.status,
        yes_no(snapshot.raw_link_up()),
        snapshot.raw_speed_mbps(),
        yes_no(snapshot.raw_full_duplex())
    ));
    lines.push(alloc::format!(
        "CTRL 0X{:08X}  EECD 0X{:08X}  ICR 0X{:08X}  IMS 0X{:08X}",
        snapshot.ctrl,
        snapshot.eecd,
        snapshot.icr,
        snapshot.ims
    ));
    lines.push(alloc::format!(
        "RCTL 0X{:08X}  TCTL 0X{:08X}  MSI-X VECTORS {}",
        snapshot.rctl,
        snapshot.tctl,
        snapshot.msix_vectors
    ));
    lines.push(alloc::format!(
        "CAP MASK 0X{:08X}  CAPS {}  PASSIVE {}",
        snapshot.cap_mask,
        snapshot.caps_text(),
        yes_no(snapshot.passive)
    ));
    lines.push(String::from("RX/TX DMA DEFERRED. NO RESET. NO RINGS. NO PACKET HANDOFF YET."));
    lines.push(String::from(
        "THIS SCREEN WAS DRAWN 10S AFTER THE BOOT LOGO USING OWNED PRIMARY SCANOUT.",
    ));

    let mut text_pixels = title_pixels.saturating_add(subtitle_pixels);
    for line in lines.iter() {
        text_pixels = text_pixels.saturating_add(draw_primary_text_line(
            surface,
            left,
            y,
            body_scale,
            line.as_str(),
        ));
        y = y.saturating_add(body_scale.saturating_mul(10));
        if y >= surface.height.saturating_sub(body_scale.saturating_mul(8)) {
            break;
        }
    }

    crate::intel::dma_flush(surface.virt, byte_len);
    let presented = notify_primary_surface_present(surface, reason, byte_len);
    crate::log!(
        "intel/display: i226-screen reason={} bdf={:02x}:{:02x}.{} size={}x{} pitch=0x{:X} bytes=0x{:X} text_pixels={} presented={}\n",
        reason,
        snapshot.bus,
        snapshot.slot,
        snapshot.function,
        surface.width,
        surface.height,
        surface.pitch_bytes,
        byte_len,
        text_pixels,
        presented as u8
    );
    presented
}

fn yes_no(v: bool) -> &'static str {
    if v { "YES" } else { "NO" }
}

fn draw_primary_text_line(
    surface: PrimarySurface,
    x: u32,
    y: u32,
    scale: u32,
    text: &str,
) -> usize {
    if scale == 0 || surface.virt.is_null() {
        return 0;
    }
    let mut pen_x = x;
    let mut pixels = 0usize;
    let advance = scale.saturating_mul(6);
    for ch in text.chars() {
        if pen_x >= surface.width {
            break;
        }
        pixels = pixels.saturating_add(draw_primary_glyph(surface, pen_x, y, scale, ch));
        pen_x = pen_x.saturating_add(advance);
    }
    pixels
}

fn draw_primary_glyph(surface: PrimarySurface, x: u32, y: u32, scale: u32, ch: char) -> usize {
    let glyph = glyph5x7(ch);
    let pitch = surface.pitch_bytes as usize;
    let mut pixels = 0usize;
    for (row_idx, row_bits) in glyph.iter().copied().enumerate() {
        for col in 0..5u32 {
            if (row_bits & (1 << (4 - col))) == 0 {
                continue;
            }
            let px0 = x.saturating_add(col.saturating_mul(scale));
            let py0 = y.saturating_add((row_idx as u32).saturating_mul(scale));
            for sy in 0..scale {
                let py = py0.saturating_add(sy);
                if py >= surface.height {
                    continue;
                }
                for sx in 0..scale {
                    let px = px0.saturating_add(sx);
                    if px >= surface.width {
                        continue;
                    }
                    let off = (py as usize).saturating_mul(pitch).saturating_add(
                        (px as usize).saturating_mul(PRIMARY_BYTES_PER_PIXEL as usize),
                    );
                    if off.saturating_add(core::mem::size_of::<u32>()) > surface.byte_len {
                        continue;
                    }
                    unsafe {
                        core::ptr::write_volatile(surface.virt.add(off) as *mut u32, 0x0000_0000);
                    }
                    pixels = pixels.saturating_add(1);
                }
            }
        }
    }
    pixels
}

fn glyph5x7(ch: char) -> [u8; 7] {
    let upper = if ch.is_ascii_lowercase() {
        ((ch as u8) - b'a' + b'A') as char
    } else {
        ch
    };
    match upper {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0F, 0x10, 0x10, 0x10, 0x10, 0x10, 0x0F],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0F, 0x10, 0x10, 0x13, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x1F],
        'J' => [0x01, 0x01, 0x01, 0x01, 0x11, 0x11, 0x0E],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1E, 0x01, 0x01, 0x0E, 0x01, 0x01, 0x1E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x10, 0x1E, 0x01, 0x01, 0x1E],
        '6' => [0x0E, 0x10, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x01, 0x0E],
        ':' => [0x00, 0x04, 0x04, 0x00, 0x04, 0x04, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '>' => [0x10, 0x08, 0x04, 0x02, 0x04, 0x08, 0x10],
        '/' => [0x01, 0x01, 0x02, 0x04, 0x08, 0x10, 0x10],
        '=' => [0x00, 0x1F, 0x00, 0x00, 0x1F, 0x00, 0x00],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        '_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F],
        ',' => [0x00, 0x00, 0x00, 0x00, 0x0C, 0x04, 0x08],
        ' ' => [0; 7],
        _ => [0x1F, 0x11, 0x01, 0x02, 0x04, 0x00, 0x04],
    }
}

fn log_surface_samples(surface: PrimarySurface, label: &str) {
    let Some(samples) = capture_surface_samples(surface) else {
        return;
    };

    intel_display_focus_log!(
        "intel/display: primary-samples label={} gpu=0x{:X} phys=0x{:X} pitch=0x{:X} tl=0x{:08X} center=0x{:08X} br=0x{:08X} apex=0x{:08X} centroid=0x{:08X} left=0x{:08X} right=0x{:08X}\n",
        label,
        surface.gpu,
        surface.phys,
        surface.pitch_bytes,
        samples.tl,
        samples.center,
        samples.br,
        samples.apex,
        samples.centroid,
        samples.left,
        samples.right
    );
}

fn capture_surface_samples(surface: PrimarySurface) -> Option<PrimarySurfaceSampleSet> {
    let width = surface.width as usize;
    let height = surface.height as usize;
    let pitch_bytes = surface.pitch_bytes as usize;
    if width == 0 || height == 0 || pitch_bytes < 4 || surface.virt.is_null() {
        return None;
    }

    let clip_to_screen = |clip_x: f32, clip_y: f32| -> (usize, usize) {
        let sx = ((clip_x + 1.0) * 0.5 * width as f32).clamp(0.0, width.saturating_sub(1) as f32)
            as usize;
        let sy = ((1.0 - (clip_y + 1.0) * 0.5) * height as f32)
            .clamp(0.0, height.saturating_sub(1) as f32) as usize;
        (sx, sy)
    };
    let (apex_x, apex_y) = clip_to_screen(0.0, 0.72);
    let (left_x, left_y) = clip_to_screen(-0.72, -0.58);
    let (right_x, right_y) = clip_to_screen(0.72, -0.58);
    let (centroid_x, centroid_y) = clip_to_screen(0.0, -0.15);

    Some(PrimarySurfaceSampleSet {
        tl: sample_surface_pixel(surface, 0, 0)?,
        center: sample_surface_pixel(surface, width / 2, height / 2)?,
        br: sample_surface_pixel(surface, width.saturating_sub(1), height.saturating_sub(1))?,
        apex: sample_surface_pixel(surface, apex_x, apex_y)?,
        centroid: sample_surface_pixel(surface, centroid_x, centroid_y)?,
        left: sample_surface_pixel(surface, left_x, left_y)?,
        right: sample_surface_pixel(surface, right_x, right_y)?,
    })
}

fn sample_surface_pixel(surface: PrimarySurface, x: usize, y: usize) -> Option<u32> {
    let width = surface.width as usize;
    let height = surface.height as usize;
    let pitch_bytes = surface.pitch_bytes as usize;
    if width == 0 || height == 0 || pitch_bytes < 4 || surface.virt.is_null() {
        return None;
    }

    let clamped_x = x.min(width.saturating_sub(1));
    let clamped_y = y.min(height.saturating_sub(1));
    let byte_offset = clamped_y
        .checked_mul(pitch_bytes)?
        .checked_add(clamped_x.checked_mul(4)?)?;
    let sample_ptr = unsafe { surface.virt.add(byte_offset) };
    crate::intel::dma_flush(sample_ptr, core::mem::size_of::<u32>());
    Some(unsafe { core::ptr::read_volatile(sample_ptr as *const u32) })
}

pub(super) fn primary_surface_gpgpu_marker_target() -> Option<PrimarySurfaceGpgpuTarget> {
    let surface = active_primary_surface()?;
    let pipeline = DisplayPipelineId::from_pipe(surface.pipe)?;
    primary_surface_gpgpu_marker_target_for_pipeline(pipeline)
}

/// Resolves the owned primary backing for one stable hardware pipeline.
///
/// The compatibility helper above preserves today's single-monitor callers;
/// compositor and diagnostic owners can use this entry point without racing a
/// later change in which pipeline is considered active.
pub(super) fn primary_surface_gpgpu_marker_target_for_pipeline(
    pipeline: DisplayPipelineId,
) -> Option<PrimarySurfaceGpgpuTarget> {
    let surface = primary_surface_for_pipeline(pipeline)?;
    if surface.virt.is_null()
        || surface.width == 0
        || surface.height == 0
        || surface.pitch_bytes < PRIMARY_BYTES_PER_PIXEL
    {
        return None;
    }

    let marker_x = core::cmp::min(32, surface.width.saturating_sub(1));
    let marker_y = core::cmp::min(32, surface.height.saturating_sub(1));
    let marker_offset = (marker_y as usize)
        .saturating_mul(surface.pitch_bytes as usize)
        .saturating_add((marker_x as usize).saturating_mul(PRIMARY_BYTES_PER_PIXEL as usize));
    let byte_len = surface.byte_len;
    if marker_offset.saturating_add(core::mem::size_of::<u32>()) > byte_len {
        return None;
    }

    Some(PrimarySurfaceGpgpuTarget {
        pipeline,
        width: surface.backing_width,
        height: surface.backing_height,
        pitch_bytes: surface.pitch_bytes,
        gpu: surface.gpu,
        phys: surface.phys,
        virt: surface.virt,
        byte_len,
        marker_gpu: surface.gpu + marker_offset as u64,
        marker_virt: unsafe { surface.virt.add(marker_offset) },
        marker_offset,
        marker_x,
        marker_y,
    })
}

pub(super) fn notify_primary_surface_external_write(
    reason: &str,
    flush_offset: usize,
    flush_bytes: usize,
) -> bool {
    let Some(surface) = active_primary_surface() else {
        return false;
    };
    let byte_len = surface.byte_len;
    if !surface.virt.is_null() && flush_offset < byte_len {
        let flush_bytes = core::cmp::min(flush_bytes, byte_len.saturating_sub(flush_offset));
        crate::intel::dma_flush(unsafe { surface.virt.add(flush_offset) }, flush_bytes);
    }
    notify_primary_surface_present(surface, reason, byte_len)
}

pub(crate) fn set_primary_plane_source(source: PrimaryPlaneSource, reason: &str) -> bool {
    let Some(pipeline) =
        active_primary_surface().and_then(|surface| DisplayPipelineId::from_pipe(surface.pipe))
    else {
        return false;
    };
    set_primary_plane_source_inner(pipeline, source, reason, false)
}

pub(crate) fn set_primary_plane_source_mapped(source: PrimaryPlaneSource, reason: &str) -> bool {
    let Some(pipeline) =
        active_primary_surface().and_then(|surface| DisplayPipelineId::from_pipe(surface.pipe))
    else {
        return false;
    };
    set_primary_plane_source_inner(pipeline, source, reason, true)
}

pub(super) fn set_primary_plane_source_for_pipeline(
    pipeline: DisplayPipelineId,
    source: PrimaryPlaneSource,
    reason: &str,
) -> bool {
    set_primary_plane_source_inner(pipeline, source, reason, false)
}

pub(super) fn set_primary_plane_source_mapped_for_pipeline(
    pipeline: DisplayPipelineId,
    source: PrimaryPlaneSource,
    reason: &str,
) -> bool {
    set_primary_plane_source_inner(pipeline, source, reason, true)
}

fn set_primary_plane_source_inner(
    pipeline: DisplayPipelineId,
    source: PrimaryPlaneSource,
    reason: &str,
    already_mapped: bool,
) -> bool {
    program_primary_plane_source_for_pipeline(source, pipeline, reason, already_mapped)
}

fn program_primary_plane_source_for_pipeline(
    source: PrimaryPlaneSource,
    pipeline: DisplayPipelineId,
    reason: &str,
    already_mapped: bool,
) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some(primary) = primary_surface_for_pipeline(pipeline) else {
        return false;
    };
    if source.phys == 0
        || source.gpu == 0
        || source.byte_len == 0
        || source.width == 0
        || source.height == 0
        || source.dst_w == 0
        || source.dst_h == 0
    {
        return false;
    }
    let Some(surface_reg) = u32::try_from(source.gpu).ok() else {
        return false;
    };
    let Some(stride_reg) = plane_stride_reg_value(source.pitch_bytes) else {
        return false;
    };

    let src_w = source.width.saturating_sub(source.src_x);
    let src_h = source.height.saturating_sub(source.src_y);
    let dst_w = source
        .dst_w
        .min(src_w)
        .min(primary.width.saturating_sub(source.dst_x));
    let dst_h = source
        .dst_h
        .min(src_h)
        .min(primary.height.saturating_sub(source.dst_y));
    if dst_w == 0 || dst_h == 0 {
        return false;
    }

    let min_pitch = source
        .width
        .saturating_mul(core::mem::size_of::<u32>() as u32);
    let min_bytes = (source.height as usize)
        .saturating_sub(1)
        .saturating_mul(source.pitch_bytes as usize)
        .saturating_add(min_pitch as usize);
    if source.pitch_bytes < min_pitch || source.byte_len < min_bytes {
        return false;
    }

    let binding = PrimaryPlaneSourceBinding {
        phys: source.phys,
        gpu: source.gpu,
        byte_len: source.byte_len,
        width: source.width,
        height: source.height,
        pitch_bytes: source.pitch_bytes,
        format: source.format,
    };
    if let Some((other_pipeline, other)) = primary_plane_source_binding_conflict(pipeline, binding)
    {
        crate::log_warn!(
            target: "intel/display";
            "intel/display: primary-plane-source rejected reason={} pipeline={} gpu=0x{:X} phys=0x{:X} bytes=0x{:X} conflict_pipeline={} conflict_gpu=0x{:X} conflict_phys=0x{:X} conflict_bytes=0x{:X} potential_reason=global-ggtt-range-alias-between-pipelines action=allocate-unique-address-range-or-share-identical-mapping\n",
            reason,
            pipeline.name(),
            binding.gpu,
            binding.phys,
            binding.byte_len,
            other_pipeline.name(),
            other.gpu,
            other.phys,
            other.byte_len,
        );
        return false;
    }
    let mut mapped_now = false;
    let binding_owner = primary_plane_source_binding_owner(primary.pipe);
    if *binding_owner.lock() != Some(binding) {
        if !already_mapped
            && !crate::intel::map_display_scanout_ggtt(
                dev,
                source.phys,
                source.byte_len,
                source.gpu,
            )
        {
            crate::log!(
                "intel/display: primary-plane-source failed reason={} cause=ggtt gpu=0x{:X} phys=0x{:X} bytes=0x{:X}\n",
                reason,
                source.gpu,
                source.phys,
                source.byte_len
            );
            return false;
        }
        if !already_mapped {
            crate::intel::ggtt_invalidate(dev);
        }
        *binding_owner.lock() = Some(binding);
        mapped_now = !already_mapped;
    }

    let pipe = primary.pipe;
    let ctl_before = crate::intel::mmio_read(dev, pipe.primary_plane().ctl());
    let ctl_enabled = primary_plane_ctl_enabled_for_format(ctl_before, source.format);
    let color_ctl_off = pipe.primary_plane().base() + UNI_PLANE_COLOR_CTL_OFF;
    let color_ctl = crate::intel::mmio_read(dev, color_ctl_off);
    let color_ctl_enabled = plane_color_ctl_alpha(color_ctl, OverlayAlphaMode::Opaque);
    let stride_before = crate::intel::mmio_read(dev, pipe.primary_plane().stride());
    let pos_off = pipe.primary_plane().base() + UNI_PLANE_POS_OFF;
    let size_off = pipe.primary_plane().base() + UNI_PLANE_SIZE_OFF;
    let offset_off = pipe.primary_plane().base() + UNI_PLANE_OFFSET_OFF;
    let pos_want = plane_pos_reg_value(source.dst_x, source.dst_y);
    let size_want = plane_size_reg_value(dst_w, dst_h);
    let offset_want = plane_pos_reg_value(source.src_x, source.src_y);
    let contract_changed = ctl_before != ctl_enabled
        || stride_before != stride_reg
        || crate::intel::mmio_read(dev, pos_off) != pos_want
        || crate::intel::mmio_read(dev, size_off) != size_want
        || crate::intel::mmio_read(dev, offset_off) != offset_want
        || color_ctl != color_ctl_enabled;
    let surf_before = crate::intel::mmio_read(dev, pipe.primary_plane().surf());
    let surf_live_before = crate::intel::mmio_read(dev, pipe.primary_plane().surf_live());
    if !ui4_rgba8_plane_stack_ready(pipe) || contract_changed {
        crate::log_error!(target: "intel/display";
            "intel/display: primary-plane-source rejected reason={} pipeline={} cause=immutable-rgba8-contract-mismatch ready={} contract_changed={} fmt={:?} src={}x{} dst={}x{} size={}x{} pitch=0x{:X}\n",
            reason,
            pipeline.name(),
            ui4_rgba8_plane_stack_ready(pipe) as u8,
            contract_changed as u8,
            source.format,
            source.src_x,
            source.src_y,
            source.dst_x,
            source.dst_y,
            dst_w,
            dst_h,
            source.pitch_bytes,
        );
        return false;
    }
    if surf_before == surf_live_before {
        match queue_ui4_plane_surface_flip(pipe.primary_plane().base(), surface_reg, reason) {
            PlaneSurfaceFlipQueueResult::Queued => {
                let program_seq = PRIMARY_SOURCE_PROGRAM_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
                if mapped_now || program_seq <= 8 || program_seq.is_multiple_of(60) {
                    intel_display_verbose_log!(
                        "intel/display: primary-plane-source seq={} reason={} pipe={} ok=1 live_ok=deferred mapped={} contract_rearm=0 fmt={:?} src={}x{} dst={}x{} size={}x{} pitch=0x{:X} surf=0x{:08X} before=0x{:08X} live=0x{:08X} path=ui4-batched-surf\n",
                        program_seq,
                        reason,
                        pipe.name,
                        mapped_now as u8,
                        source.format,
                        source.src_x,
                        source.src_y,
                        source.dst_x,
                        source.dst_y,
                        dst_w,
                        dst_h,
                        source.pitch_bytes,
                        surface_reg,
                        surf_before,
                        surf_live_before,
                    );
                }
                return true;
            }
            PlaneSurfaceFlipQueueResult::Rejected => return false,
            PlaneSurfaceFlipQueueResult::Inactive => {}
        }
    }
    crate::intel::mmio_write(dev, pipe.primary_plane().surf(), surface_reg);

    let surf_after = crate::intel::mmio_read(dev, pipe.primary_plane().surf());
    let (surf_live_after, surf_live_iter) =
        wait_for_plane_live_for(dev, pipe.primary_plane().base(), surface_reg, 25_000_000);
    let program_seq = PRIMARY_SOURCE_PROGRAM_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if mapped_now || program_seq <= 8 || program_seq.is_multiple_of(60) {
        intel_display_verbose_log!(
            "intel/display: primary-plane-source seq={} reason={} pipe={} ok={} live_ok={} mapped={} contract_rearm={} fmt={:?} src={}x{} dst={}x{} size={}x{} pitch=0x{:X} surf=0x{:08X} after=0x{:08X} live=0x{:08X}=>0x{:08X} live_iter={}\n",
            program_seq,
            reason,
            pipe.name,
            (surf_after == surface_reg) as u8,
            (surf_live_after == surface_reg) as u8,
            mapped_now as u8,
            0,
            source.format,
            source.src_x,
            source.src_y,
            source.dst_x,
            source.dst_y,
            dst_w,
            dst_h,
            source.pitch_bytes,
            surface_reg,
            surf_after,
            surf_live_before,
            surf_live_after,
            surf_live_iter
        );
    }
    surf_after == surface_reg && surf_live_after == surface_reg
}

pub(crate) fn present_ui_surface_to_primary_plane(
    surface: UiSurface,
    phys: u64,
    byte_len: usize,
    src: UiRect,
    dst: UiRect,
    reason: &str,
) -> bool {
    if src.is_empty() || dst.is_empty() {
        return false;
    }
    let format = match surface.format {
        UiSurfaceFormat::Xrgb8888 => PrimaryPlaneSourceFormat::Xrgb8888,
        UiSurfaceFormat::Xbgr8888 => PrimaryPlaneSourceFormat::Xbgr8888,
        UiSurfaceFormat::Rgba8888 => return false,
    };
    set_primary_plane_source(
        PrimaryPlaneSource {
            phys,
            gpu: surface.gpu,
            byte_len,
            width: surface.width,
            height: surface.height,
            pitch_bytes: surface.pitch,
            format,
            src_x: src.x,
            src_y: src.y,
            dst_x: dst.x,
            dst_y: dst.y,
            dst_w: dst.w,
            dst_h: dst.h,
        },
        reason,
    )
}

pub(crate) fn present_ui_surface_to_primary_backing(
    surface: UiSurface,
    virt: *const u8,
    byte_len: usize,
    src: UiRect,
    dst: UiRect,
    reason: &str,
) -> bool {
    let started_ns = crate::chronos::monotonic_nanos();
    let Some(primary) = active_primary_surface() else {
        return false;
    };
    if !matches!(
        surface.format,
        UiSurfaceFormat::Rgba8888 | UiSurfaceFormat::Xrgb8888 | UiSurfaceFormat::Xbgr8888
    ) {
        return false;
    }
    if virt.is_null()
        || primary.virt.is_null()
        || byte_len == 0
        || surface.width == 0
        || surface.height == 0
        || src.is_empty()
        || dst.is_empty()
        || surface.pitch < surface.width.saturating_mul(4)
        || primary.pitch_bytes < primary.width.saturating_mul(PRIMARY_BYTES_PER_PIXEL)
    {
        return false;
    }

    let Some(rect) = primary_backing_copy_rect(surface, primary, src, dst, byte_len) else {
        return false;
    };

    for row in 0..rect.height {
        let src_off = rect
            .src_y
            .saturating_add(row)
            .saturating_mul(rect.src_pitch)
            .saturating_add(rect.src_x.saturating_mul(4));
        let dst_off = rect
            .dst_y
            .saturating_add(row)
            .saturating_mul(rect.dst_pitch)
            .saturating_add(rect.dst_x.saturating_mul(PRIMARY_BYTES_PER_PIXEL as usize));
        if src_off.saturating_add(rect.row_bytes) > byte_len
            || dst_off.saturating_add(rect.row_bytes) > primary.byte_len
        {
            return false;
        }
        match surface.format {
            UiSurfaceFormat::Xrgb8888 => unsafe {
                core::ptr::copy_nonoverlapping(
                    virt.add(src_off),
                    primary.virt.add(dst_off),
                    rect.row_bytes,
                );
            },
            UiSurfaceFormat::Xbgr8888 => {
                let src_row =
                    unsafe { core::slice::from_raw_parts(virt.add(src_off), rect.row_bytes) };
                let dst_row = unsafe { primary.virt.add(dst_off) as *mut u32 };
                for col in 0..rect.width {
                    let off = col.saturating_mul(4);
                    let r = src_row[off];
                    let g = src_row[off + 1];
                    let b = src_row[off + 2];
                    unsafe {
                        core::ptr::write_volatile(
                            dst_row.add(col),
                            u32::from_le_bytes([b, g, r, 0]),
                        );
                    }
                }
            }
            UiSurfaceFormat::Rgba8888 => {
                let src_row =
                    unsafe { core::slice::from_raw_parts(virt.add(src_off), rect.row_bytes) };
                let dst_row = unsafe { primary.virt.add(dst_off) as *mut u32 };
                for col in 0..rect.width {
                    let off = col.saturating_mul(4);
                    let r = src_row[off];
                    let g = src_row[off + 1];
                    let b = src_row[off + 2];
                    unsafe {
                        core::ptr::write_volatile(
                            dst_row.add(col),
                            u32::from_le_bytes([b, g, r, 0]),
                        );
                    }
                }
            }
        }
    }

    let presented =
        notify_primary_surface_external_write(reason, rect.flush_offset, rect.flush_bytes);
    let seq = UI_SURFACE_PRIMARY_COPY_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if seq <= 8 || seq.is_multiple_of(60) {
        let copied_bytes = rect.row_bytes.saturating_mul(rect.height);
        let copy_present_us = crate::chronos::monotonic_nanos().saturating_sub(started_ns) / 1_000;
        crate::log!(
            "intel/display: ui-surface-primary-copy seq={} reason={} contract=cpu-convert-copy-to-primary zero_copy=0 fmt={:?} src={},{} {}x{} dst={},{} copied={}x{} copied_bytes=0x{:X} copy_present_us={} frame_budget_us=16667 over_budget={} presented={}\n",
            seq,
            reason,
            surface.format,
            src.x,
            src.y,
            src.w,
            src.h,
            dst.x,
            dst.y,
            rect.width,
            rect.height,
            copied_bytes,
            copy_present_us,
            (copy_present_us > 16_667) as u8,
            presented as u8
        );
    }
    presented
}

fn primary_backing_copy_rect(
    surface: UiSurface,
    primary: PrimarySurface,
    src: UiRect,
    dst: UiRect,
    byte_len: usize,
) -> Option<PrimaryBackingCopyRect> {
    let src_pitch = surface.pitch as usize;
    let dst_pitch = primary.pitch_bytes as usize;
    let src_x = src.x as usize;
    let src_y = src.y as usize;
    let dst_x = dst.x as usize;
    let dst_y = dst.y as usize;
    let src_w = surface.width.saturating_sub(src.x).min(src.w).min(dst.w) as usize;
    let src_h = surface.height.saturating_sub(src.y).min(src.h).min(dst.h) as usize;
    let dst_w = primary.width.saturating_sub(dst.x) as usize;
    let dst_h = primary.height.saturating_sub(dst.y) as usize;
    let width = src_w.min(dst_w);
    let height = src_h.min(dst_h);
    if width == 0 || height == 0 {
        return None;
    }

    let row_bytes = width.checked_mul(PRIMARY_BYTES_PER_PIXEL as usize)?;
    let src_last = src_y
        .checked_add(height.saturating_sub(1))?
        .checked_mul(src_pitch)?
        .checked_add(src_x.checked_mul(4)?)?
        .checked_add(row_bytes)?;
    let dst_last = dst_y
        .checked_add(height.saturating_sub(1))?
        .checked_mul(dst_pitch)?
        .checked_add(dst_x.checked_mul(PRIMARY_BYTES_PER_PIXEL as usize)?)?
        .checked_add(row_bytes)?;
    if src_last > byte_len || dst_last > primary.byte_len {
        return None;
    }

    let flush_offset = dst_y
        .checked_mul(dst_pitch)?
        .checked_add(dst_x.checked_mul(PRIMARY_BYTES_PER_PIXEL as usize)?)?;
    let flush_bytes = height
        .saturating_sub(1)
        .checked_mul(dst_pitch)?
        .checked_add(row_bytes)?;

    Some(PrimaryBackingCopyRect {
        src_x,
        src_y,
        dst_x,
        dst_y,
        width,
        height,
        src_pitch,
        dst_pitch,
        row_bytes,
        flush_offset,
        flush_bytes,
    })
}

pub(crate) fn present_rgba_primary(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    reason: &str,
) -> bool {
    let Some(surface) = active_primary_surface() else {
        return false;
    };
    if surface.virt.is_null()
        || src_width == 0
        || src_height == 0
        || src_pitch_bytes < src_width as usize * 4
    {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    let copy_w = (src_width as usize).min(dst_width);
    let copy_h = (src_height as usize).min(dst_height);
    if copy_w == 0 || copy_h == 0 || dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    for row_idx in 0..copy_h {
        let src_row_off = row_idx.saturating_mul(src_pitch_bytes);
        let Some(src_row) = src.get(src_row_off..src_row_off + copy_w.saturating_mul(4)) else {
            return false;
        };
        let dst_row_off = row_idx.saturating_mul(dst_pitch);
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_off = col_idx.saturating_mul(4);
            let r = src_row[src_off];
            let g = src_row[src_off + 1];
            let b = src_row[src_off + 2];
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
        crate::intel::dma_flush(unsafe { surface.virt.add(dst_row_off) }, copy_w.saturating_mul(4));
    }

    let byte_len = dst_pitch.saturating_mul(dst_height);
    notify_primary_surface_present(surface, reason, byte_len)
}

pub(crate) fn present_rgba_primary_center_unscaled(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    reason: &str,
) -> bool {
    present_rgba_primary_center_unscaled_bg(src, src_width, src_height, src_pitch_bytes, 0, reason)
}

pub(crate) fn present_rgba_primary_center_unscaled_bg(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    bg_xrgb: u32,
    reason: &str,
) -> bool {
    let Some(surface) = active_primary_surface() else {
        return false;
    };
    if surface.virt.is_null()
        || src_width == 0
        || src_height == 0
        || src_pitch_bytes < src_width as usize * 4
    {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    let src_width = src_width as usize;
    let src_height = src_height as usize;
    if dst_width == 0 || dst_height == 0 || dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    let copy_w = src_width.min(dst_width);
    let copy_h = src_height.min(dst_height);
    if copy_w == 0 || copy_h == 0 {
        return false;
    }

    let src_x = src_width.saturating_sub(copy_w) / 2;
    let src_y = src_height.saturating_sub(copy_h) / 2;
    let dst_x = dst_width.saturating_sub(copy_w) / 2;
    let dst_y = dst_height.saturating_sub(copy_h) / 2;
    let byte_len = dst_pitch.saturating_mul(dst_height);

    fill_surface_color(surface.virt, dst_pitch, surface.width, surface.height, bg_xrgb);

    for row_idx in 0..copy_h {
        let src_row_off = src_y
            .saturating_add(row_idx)
            .saturating_mul(src_pitch_bytes)
            .saturating_add(src_x.saturating_mul(4));
        let Some(src_row) = src.get(src_row_off..src_row_off.saturating_add(copy_w * 4)) else {
            return false;
        };
        let dst_row_off = dst_y
            .saturating_add(row_idx)
            .saturating_mul(dst_pitch)
            .saturating_add(dst_x.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_off = col_idx.saturating_mul(4);
            let r = src_row[src_off];
            let g = src_row[src_off + 1];
            let b = src_row[src_off + 2];
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
    }

    crate::intel::dma_flush(surface.virt, byte_len);
    notify_primary_surface_present(surface, reason, byte_len)
}

pub(crate) fn present_rgba_primary_center_plane_bg(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    bg_xrgb: u32,
    reason: &str,
) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some(surface) = active_primary_surface() else {
        return false;
    };
    if surface.virt.is_null()
        || src_width == 0
        || src_height == 0
        || src_pitch_bytes < src_width as usize * 4
    {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    let src_width = src_width as usize;
    let src_height = src_height as usize;
    if dst_width == 0 || dst_height == 0 || dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    let copy_w = src_width.min(dst_width);
    let copy_h = src_height.min(dst_height);
    if copy_w == 0 || copy_h == 0 {
        return false;
    }

    let src_x = src_width.saturating_sub(copy_w) / 2;
    let src_y = src_height.saturating_sub(copy_h) / 2;
    let dst_x = dst_width.saturating_sub(copy_w) / 2;
    let dst_y = dst_height.saturating_sub(copy_h) / 2;

    program_pipe_bottom_color(dev, surface.pipe, pipe_bottom_color_from_xrgb(bg_xrgb));

    for row_idx in 0..copy_h {
        let src_row_off = src_y
            .saturating_add(row_idx)
            .saturating_mul(src_pitch_bytes)
            .saturating_add(src_x.saturating_mul(4));
        let Some(src_row) = src.get(src_row_off..src_row_off.saturating_add(copy_w * 4)) else {
            return false;
        };
        let dst_row_off = row_idx.saturating_mul(dst_pitch);
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_off = col_idx.saturating_mul(4);
            let r = src_row[src_off];
            let g = src_row[src_off + 1];
            let b = src_row[src_off + 2];
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
        crate::intel::dma_flush(unsafe { surface.virt.add(dst_row_off) }, copy_w.saturating_mul(4));
    }

    let Some(pipeline) = DisplayPipelineId::from_pipe(surface.pipe) else {
        return false;
    };
    set_primary_plane_source_inner(
        pipeline,
        PrimaryPlaneSource {
            phys: surface.phys,
            gpu: surface.gpu,
            byte_len: surface.byte_len,
            width: surface.width,
            height: surface.height,
            pitch_bytes: surface.pitch_bytes,
            format: match PRIMARY_FORMAT_PROBE_MODE {
                PRIMARY_FORMAT_PROBE_XBGR => PrimaryPlaneSourceFormat::Xbgr8888,
                _ => PrimaryPlaneSourceFormat::Xrgb8888,
            },
            src_x: 0,
            src_y: 0,
            dst_x: dst_x as u32,
            dst_y: dst_y as u32,
            dst_w: copy_w as u32,
            dst_h: copy_h as u32,
        },
        reason,
        true,
    )
}

fn present_rgba_primary_center(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    reason: &str,
) -> bool {
    let Some(surface) = active_primary_surface() else {
        return false;
    };
    if surface.virt.is_null()
        || src_width == 0
        || src_height == 0
        || src_pitch_bytes < src_width as usize * 4
    {
        return false;
    }

    let src_width = src_width as usize;
    let src_height = src_height as usize;
    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    if dst_width == 0 || dst_height == 0 || dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    let (copy_w, copy_h) = aspect_fit_size(src_width, src_height, dst_width, dst_height);
    if copy_w == 0 || copy_h == 0 {
        return false;
    }
    let dst_x = dst_width.saturating_sub(copy_w) / 2;
    let dst_y = dst_height.saturating_sub(copy_h) / 2;

    for row_idx in 0..copy_h {
        let src_y = row_idx
            .saturating_mul(src_height)
            .checked_div(copy_h.max(1))
            .unwrap_or(0)
            .min(src_height.saturating_sub(1));
        let src_row_off = src_y.saturating_mul(src_pitch_bytes);
        let Some(src_row) = src.get(src_row_off..src_row_off + src_width.saturating_mul(4)) else {
            return false;
        };
        let dst_row_off = (dst_y + row_idx)
            .saturating_mul(dst_pitch)
            .saturating_add(dst_x.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_x = col_idx
                .saturating_mul(src_width)
                .checked_div(copy_w.max(1))
                .unwrap_or(0)
                .min(src_width.saturating_sub(1));
            let src_off = src_x.saturating_mul(4);
            let r = src_row[src_off];
            let g = src_row[src_off + 1];
            let b = src_row[src_off + 2];
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
    }

    let byte_len = dst_pitch.saturating_mul(dst_height);
    crate::intel::dma_flush(surface.virt, byte_len);
    notify_primary_surface_present(surface, reason, byte_len)
}

pub(crate) fn blend_rgba_primary_rect(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    src_x: u32,
    src_y: u32,
    dst_x: i32,
    dst_y: i32,
    width: u32,
    height: u32,
    reason: &str,
) -> bool {
    let Some(surface) = active_primary_surface() else {
        return false;
    };
    if surface.virt.is_null()
        || src_width == 0
        || src_height == 0
        || width == 0
        || height == 0
        || src_pitch_bytes < src_width as usize * 4
    {
        return false;
    }

    let mut sx = src_x as i64;
    let mut sy = src_y as i64;
    let mut dx = dst_x as i64;
    let mut dy = dst_y as i64;
    let mut w = width as i64;
    let mut h = height as i64;

    if dx < 0 {
        sx -= dx;
        w += dx;
        dx = 0;
    }
    if dy < 0 {
        sy -= dy;
        h += dy;
        dy = 0;
    }

    let src_max_w = src_width as i64 - sx;
    let src_max_h = src_height as i64 - sy;
    let dst_max_w = surface.width as i64 - dx;
    let dst_max_h = surface.height as i64 - dy;
    w = w.min(src_max_w).min(dst_max_w);
    h = h.min(src_max_h).min(dst_max_h);
    if sx < 0 || sy < 0 || w <= 0 || h <= 0 {
        return false;
    }

    let dst_pitch = surface.pitch_bytes as usize;
    let copy_w = w as usize;
    let copy_h = h as usize;
    let sx = sx as usize;
    let sy = sy as usize;
    let dx = dx as usize;
    let dy = dy as usize;

    for row in 0..copy_h {
        let src_off = sy
            .saturating_add(row)
            .saturating_mul(src_pitch_bytes)
            .saturating_add(sx.saturating_mul(4));
        let Some(src_row) = src.get(src_off..src_off.saturating_add(copy_w.saturating_mul(4)))
        else {
            return false;
        };
        let dst_row_off = dy
            .saturating_add(row)
            .saturating_mul(dst_pitch)
            .saturating_add(dx.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col in 0..copy_w {
            let src_px = col.saturating_mul(4);
            let sa = src_row[src_px + 3] as u32;
            if sa == 0 {
                continue;
            }
            let sr = src_row[src_px] as u32;
            let sg = src_row[src_px + 1] as u32;
            let sb = src_row[src_px + 2] as u32;
            let pixel = if sa == 0xFF {
                u32::from_le_bytes([sb as u8, sg as u8, sr as u8, 0])
            } else {
                let dst = unsafe { core::ptr::read_volatile(dst_row.add(col)) };
                let db = dst & 0xFF;
                let dg = (dst >> 8) & 0xFF;
                let dr = (dst >> 16) & 0xFF;
                let inv = 255 - sa;
                let out_r = (sr * sa + dr * inv + 127) / 255;
                let out_g = (sg * sa + dg * inv + 127) / 255;
                let out_b = (sb * sa + db * inv + 127) / 255;
                u32::from_le_bytes([out_b as u8, out_g as u8, out_r as u8, 0])
            };
            unsafe {
                core::ptr::write_volatile(dst_row.add(col), pixel);
            }
        }
        crate::intel::dma_flush(unsafe { surface.virt.add(dst_row_off) }, copy_w.saturating_mul(4));
    }

    let flush_offset = dy
        .saturating_mul(dst_pitch)
        .saturating_add(dx.saturating_mul(4));
    let flush_bytes = copy_h
        .saturating_sub(1)
        .saturating_mul(dst_pitch)
        .saturating_add(copy_w.saturating_mul(4));
    notify_primary_surface_external_write(reason, flush_offset, flush_bytes)
}

pub(crate) fn blend_rgba_primary_rect_scaled(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    src_x: u32,
    src_y: u32,
    src_w: u32,
    src_h: u32,
    dst_x: i32,
    dst_y: i32,
    dst_w: u32,
    dst_h: u32,
    reason: &str,
) -> bool {
    let Some(surface) = active_primary_surface() else {
        return false;
    };
    if surface.virt.is_null()
        || src_width == 0
        || src_height == 0
        || src_w == 0
        || src_h == 0
        || dst_w == 0
        || dst_h == 0
        || src_pitch_bytes < src_width as usize * 4
        || src_x.saturating_add(src_w) > src_width
        || src_y.saturating_add(src_h) > src_height
    {
        return false;
    }

    let mut dx = dst_x as i64;
    let mut dy = dst_y as i64;
    let mut clip_x0 = 0i64;
    let mut clip_y0 = 0i64;
    let mut copy_w = dst_w as i64;
    let mut copy_h = dst_h as i64;

    if dx < 0 {
        clip_x0 = -dx;
        copy_w += dx;
        dx = 0;
    }
    if dy < 0 {
        clip_y0 = -dy;
        copy_h += dy;
        dy = 0;
    }

    copy_w = copy_w.min(surface.width as i64 - dx);
    copy_h = copy_h.min(surface.height as i64 - dy);
    if copy_w <= 0 || copy_h <= 0 {
        return false;
    }

    let dst_pitch = surface.pitch_bytes as usize;
    let dx = dx as usize;
    let dy = dy as usize;
    let copy_w = copy_w as usize;
    let copy_h = copy_h as usize;
    let clip_x0 = clip_x0 as usize;
    let clip_y0 = clip_y0 as usize;
    let src_x = src_x as usize;
    let src_y = src_y as usize;
    let src_w = src_w as usize;
    let src_h = src_h as usize;
    let dst_w = dst_w as usize;
    let dst_h = dst_h as usize;

    for row in 0..copy_h {
        let mapped_y = src_y.saturating_add(
            (clip_y0.saturating_add(row))
                .saturating_mul(src_h)
                .checked_div(dst_h)
                .unwrap_or(0)
                .min(src_h.saturating_sub(1)),
        );
        let dst_row_off = dy
            .saturating_add(row)
            .saturating_mul(dst_pitch)
            .saturating_add(dx.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col in 0..copy_w {
            let mapped_x = src_x.saturating_add(
                (clip_x0.saturating_add(col))
                    .saturating_mul(src_w)
                    .checked_div(dst_w)
                    .unwrap_or(0)
                    .min(src_w.saturating_sub(1)),
            );
            let src_off = mapped_y
                .saturating_mul(src_pitch_bytes)
                .saturating_add(mapped_x.saturating_mul(4));
            let Some(src_px) = src.get(src_off..src_off.saturating_add(4)) else {
                return false;
            };
            let sa = src_px[3] as u32;
            if sa == 0 {
                continue;
            }
            let sr = src_px[0] as u32;
            let sg = src_px[1] as u32;
            let sb = src_px[2] as u32;
            let pixel = if sa == 0xFF {
                u32::from_le_bytes([sb as u8, sg as u8, sr as u8, 0])
            } else {
                let dst = unsafe { core::ptr::read_volatile(dst_row.add(col)) };
                let db = dst & 0xFF;
                let dg = (dst >> 8) & 0xFF;
                let dr = (dst >> 16) & 0xFF;
                let inv = 255 - sa;
                let out_r = (sr * sa + dr * inv + 127) / 255;
                let out_g = (sg * sa + dg * inv + 127) / 255;
                let out_b = (sb * sa + db * inv + 127) / 255;
                u32::from_le_bytes([out_b as u8, out_g as u8, out_r as u8, 0])
            };
            unsafe {
                core::ptr::write_volatile(dst_row.add(col), pixel);
            }
        }
        crate::intel::dma_flush(unsafe { surface.virt.add(dst_row_off) }, copy_w.saturating_mul(4));
    }

    let flush_offset = dy
        .saturating_mul(dst_pitch)
        .saturating_add(dx.saturating_mul(4));
    let flush_bytes = copy_h
        .saturating_sub(1)
        .saturating_mul(dst_pitch)
        .saturating_add(copy_w.saturating_mul(4));
    notify_primary_surface_external_write(reason, flush_offset, flush_bytes)
}

pub(crate) fn present_rgba_primary_rot180(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    reason: &str,
) -> bool {
    let Some(surface) = active_primary_surface() else {
        return false;
    };
    if surface.virt.is_null()
        || src_width == 0
        || src_height == 0
        || src_pitch_bytes < src_width as usize * 4
    {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    let copy_w = (src_width as usize).min(dst_width);
    let copy_h = (src_height as usize).min(dst_height);
    if copy_w == 0 || copy_h == 0 || dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    for row_idx in 0..copy_h {
        let src_y = copy_h.saturating_sub(1).saturating_sub(row_idx);
        let src_row_off = src_y.saturating_mul(src_pitch_bytes);
        let Some(src_row) = src.get(src_row_off..src_row_off + copy_w.saturating_mul(4)) else {
            return false;
        };
        let dst_row_off = row_idx.saturating_mul(dst_pitch);
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_x = copy_w.saturating_sub(1).saturating_sub(col_idx);
            let src_off = src_x.saturating_mul(4);
            let r = src_row[src_off];
            let g = src_row[src_off + 1];
            let b = src_row[src_off + 2];
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
        crate::intel::dma_flush(unsafe { surface.virt.add(dst_row_off) }, copy_w.saturating_mul(4));
    }

    let byte_len = dst_pitch.saturating_mul(dst_height);
    notify_primary_surface_present(surface, reason, byte_len)
}

pub(crate) fn present_rgba_primary_flip_y(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    reason: &str,
) -> bool {
    let Some(surface) = active_primary_surface() else {
        return false;
    };
    if surface.virt.is_null()
        || src_width == 0
        || src_height == 0
        || src_pitch_bytes < src_width as usize * 4
    {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    let copy_w = (src_width as usize).min(dst_width);
    let copy_h = (src_height as usize).min(dst_height);
    if copy_w == 0 || copy_h == 0 || dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    for row_idx in 0..copy_h {
        let src_y = copy_h.saturating_sub(1).saturating_sub(row_idx);
        let src_row_off = src_y.saturating_mul(src_pitch_bytes);
        let Some(src_row) = src.get(src_row_off..src_row_off + copy_w.saturating_mul(4)) else {
            return false;
        };
        let dst_row_off = row_idx.saturating_mul(dst_pitch);
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_off = col_idx.saturating_mul(4);
            let r = src_row[src_off];
            let g = src_row[src_off + 1];
            let b = src_row[src_off + 2];
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
        crate::intel::dma_flush(unsafe { surface.virt.add(dst_row_off) }, copy_w.saturating_mul(4));
    }

    let byte_len = dst_pitch.saturating_mul(dst_height);
    notify_primary_surface_present(surface, reason, byte_len)
}

pub(crate) fn present_rgba_primary_top_right(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
) -> bool {
    let Some(surface) = active_primary_surface() else {
        return false;
    };
    if surface.virt.is_null()
        || src_width == 0
        || src_height == 0
        || src_pitch_bytes < src_width as usize * 4
    {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    let copy_w = (src_width as usize).min(dst_width);
    let copy_h = (src_height as usize).min(dst_height);
    if copy_w == 0 || copy_h == 0 || dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    let dst_x = dst_width.saturating_sub(copy_w);
    let dst_y = 0usize;
    for row_idx in 0..copy_h {
        let src_row_off = row_idx.saturating_mul(src_pitch_bytes);
        let Some(src_row) = src.get(src_row_off..src_row_off + copy_w.saturating_mul(4)) else {
            return false;
        };
        let dst_row_off = (dst_y + row_idx)
            .saturating_mul(dst_pitch)
            .saturating_add(dst_x.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_off = col_idx.saturating_mul(4);
            let r = src_row[src_off];
            let g = src_row[src_off + 1];
            let b = src_row[src_off + 2];
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
        crate::intel::dma_flush(unsafe { surface.virt.add(dst_row_off) }, copy_w.saturating_mul(4));
    }

    let byte_len = dst_pitch.saturating_mul(dst_height);
    notify_primary_surface_present(surface, "rgba-primary-top-right", byte_len)
}

pub(crate) fn present_rgba_overlay_top_right(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
) -> bool {
    present_rgba_overlay(src, src_width, src_height, src_pitch_bytes, None, false, "camera-overlay")
}

pub(crate) fn present_rgba_overlay_at(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    x: u32,
    y: u32,
    preserve_alpha: bool,
    reason: &str,
) -> bool {
    present_rgba_overlay(
        src,
        src_width,
        src_height,
        src_pitch_bytes,
        Some((x, y)),
        preserve_alpha,
        reason,
    )
}

pub(crate) fn present_live_overlay_rects(rects: &[LiveOverlayRect], reason: &str) -> bool {
    present_live_overlay_rects_preserving(rects, None, reason)
}

/// Update only the cursor/menu damage on the double-buffered alpha plane.
/// Per-buffer damage debt keeps a buffer coherent even when it was last used
/// two or more cursor updates ago.
pub(crate) fn present_live_overlay_rects_damage(
    rects: &[LiveOverlayRect],
    damage: CompositionDamageRect,
    reason: &str,
) -> bool {
    present_live_overlay_rects_on_slot_damage(OVERLAY_PLANE_SLOT, rects, damage, reason)
}

/// Damage-aware sparse rectangle compositor for a selected universal plane.
/// UI4 uses slot 4 for input chrome, leaving the default slot-1 helper intact
/// for bootstrap probes and older kernel callers.
pub(crate) fn present_live_overlay_rects_on_slot_damage(
    plane_slot: usize,
    rects: &[LiveOverlayRect],
    damage: CompositionDamageRect,
    reason: &str,
) -> bool {
    present_live_overlay_rects_on_slot_damage_region(
        plane_slot,
        rects,
        CompositionDamageRegion::from_rect(damage),
        reason,
    )
}

/// Region-preserving form used by UI4's independent interaction plane. A set
/// of distant software cursors must not turn into one screen-spanning flush.
pub(crate) fn present_live_overlay_rects_on_slot_damage_region(
    plane_slot: usize,
    rects: &[LiveOverlayRect],
    damage: CompositionDamageRegion,
    reason: &str,
) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let (width, height) = active_scanout_dimensions()
        .or_else(|| active_primary_surface().map(|primary| (primary.width, primary.height)))
        .unwrap_or((0, 0));
    let change = clip_composition_damage_region(damage, width, height);
    if change.is_empty() {
        return true;
    }
    let Some(surface) = ensure_overlay_surface_on_slot(dev, plane_slot, width, height) else {
        return false;
    };
    let effective = {
        let Some(surface_pool) = overlay_surface_pool(surface.pipe, surface.plane_slot) else {
            return false;
        };
        let pool = surface_pool.lock();
        let mut effective = pool.damage_debt[surface.buffer_index];
        effective.add_region(change);
        effective
    };

    for damage in effective.rects() {
        fill_overlay_rect(surface, damage.x, damage.y, damage.width, damage.height, 0);
        for rect in rects {
            fill_overlay_rect_rgba_clipped(surface, *rect, *damage);
        }
    }
    if !dma_flush_overlay_region(surface, effective) {
        return false;
    }

    let needs_flip = overlay_plane_needs_rearm(dev, surface, 0, 0, UI4_RGBA8_OVERLAY_CONTRACT);
    let (presented, path) = (
        present_overlay_surface_with_bootstrap_contract(
            dev,
            surface,
            0,
            0,
            UI4_RGBA8_OVERLAY_CONTRACT,
            reason,
        ),
        if needs_flip {
            "surf-only"
        } else {
            "already-live"
        },
    );
    if !presented {
        return false;
    }

    let Some(surface_pool) = overlay_surface_pool(surface.pipe, surface.plane_slot) else {
        return false;
    };
    let mut pool = surface_pool.lock();
    if pool.matches(surface.width, surface.height, surface.pipe) {
        for index in 0..OVERLAY_SWAP_BUFFER_COUNT {
            if index == surface.buffer_index {
                pool.damage_debt[index] = CompositionDamageRegion::EMPTY;
            } else {
                pool.damage_debt[index].add_region(change);
            }
        }
    }
    drop(pool);
    let seq = OVERLAY_PRESENT_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if seq <= 8 || seq.is_multiple_of(120) {
        let change_bounds = change.bounding_rect().unwrap_or_default();
        let effective_bounds = effective.bounding_rect().unwrap_or_default();
        crate::log!(
            "intel/display: live-overlay-damage-present seq={} reason={} pipe={} slot={} buffer={} path={} rects={} damage_rects={} damage_bounds={}x{}@{},{} effective_rects={} effective_bounds={}x{}@{},{}\n",
            seq,
            reason,
            surface.pipe.name,
            surface.plane_slot,
            surface.buffer_index,
            path,
            rects.len(),
            change.len(),
            change_bounds.width,
            change_bounds.height,
            change_bounds.x,
            change_bounds.y,
            effective.len(),
            effective_bounds.width,
            effective_bounds.height,
            effective_bounds.x,
            effective_bounds.y,
        );
    }
    true
}

/// Compose positioned RGBA tiles into one full-scanout transparent surface and
/// commit one hardware universal plane. Each selected slot owns an independent
/// double-buffered composition surface.
pub(crate) fn present_rgba_overlay_tiles(tiles: &[RgbaOverlayTile<'_>], reason: &str) -> bool {
    present_rgba_overlay_tiles_on_slot_with_background(OVERLAY_PLANE_SLOT, tiles, None, reason)
}

pub(crate) fn present_rgba_overlay_tiles_on_slot(
    plane_slot: usize,
    tiles: &[RgbaOverlayTile<'_>],
    reason: &str,
) -> bool {
    present_rgba_overlay_tiles_on_slot_with_background(plane_slot, tiles, None, reason)
}

/// Compose UI4's native premultiplied RGBA frames into one alpha plane while
/// touching only pixels whose window content or placement changed.
///
/// Each swap buffer carries damage debt because it can be one presentation
/// behind the current front. This is the overlay counterpart of the primary
/// UI4 compositor path and avoids clearing/flushing the full scanout for a
/// small animated window.
pub(crate) fn present_premultiplied_rgba_overlay_tiles_on_slot_damage(
    plane_slot: usize,
    tiles: &[RgbaOverlayTile<'_>],
    damage: CompositionDamageRegion,
    reason: &str,
) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some((width, height)) = active_scanout_dimensions()
        .or_else(|| active_primary_surface().map(|primary| (primary.width, primary.height)))
    else {
        return false;
    };
    let change = clip_composition_damage_region(damage, width, height);
    if change.is_empty() {
        return true;
    }
    let Some(surface) = ensure_overlay_surface_on_slot(dev, plane_slot, width, height) else {
        return false;
    };
    let effective = {
        let Some(surface_pool) = overlay_surface_pool(surface.pipe, surface.plane_slot) else {
            return false;
        };
        let pool = surface_pool.lock();
        let mut effective = pool.damage_debt[surface.buffer_index];
        effective.add_region(change);
        effective
    };

    let composition_started_ns = crate::chronos::monotonic_nanos();
    let gpu_composed =
        compose_premultiplied_rgba_tiles_into_overlay_gpgpu(surface, tiles, effective);
    let compositor = match gpu_composed {
        GpgpuCompositionResult::Complete => "guc-simd16-sprite-quad",
        GpgpuCompositionResult::Unavailable => {
            for damage in effective.rects() {
                fill_overlay_rect(surface, damage.x, damage.y, damage.width, damage.height, 0);
                for tile in tiles {
                    if copy_premultiplied_rgba_tile_into_overlay_clipped(surface, tile, *damage)
                        .is_none()
                    {
                        return false;
                    }
                }
            }
            if !dma_flush_overlay_region(surface, effective) {
                return false;
            }
            "cpu-fallback"
        }
        GpgpuCompositionResult::SubmittedIncomplete => {
            // The old front remains valid. Never race the CPU or scanout
            // against a destination that may still be owned by the GPU.
            return false;
        }
    };
    let composition_us =
        crate::chronos::monotonic_nanos().saturating_sub(composition_started_ns) / 1_000;

    let needs_flip = overlay_plane_needs_rearm(dev, surface, 0, 0, UI4_RGBA8_OVERLAY_CONTRACT);
    let (presented, path) = (
        present_overlay_surface_with_bootstrap_contract(
            dev,
            surface,
            0,
            0,
            UI4_RGBA8_OVERLAY_CONTRACT,
            reason,
        ),
        if needs_flip {
            "surf-only"
        } else {
            "already-live"
        },
    );
    if !presented {
        return false;
    }
    mark_overlay_composition_surface_front(surface, change);

    let seq = OVERLAY_PRESENT_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if seq <= 8 || seq.is_multiple_of(60) {
        let change_bounds = change.bounding_rect().unwrap_or_default();
        let effective_bounds = effective.bounding_rect().unwrap_or_default();
        crate::log!(
            "intel/display: rgba-tile-overlay-damage-present seq={} reason={} pipe={} slot={} buffer={} path={} compositor={} composition_us={} tiles={} damage_rects={} damage_bounds={}x{}@{},{} effective_rects={} effective_bounds={}x{}@{},{} scanout={}x{} pitch=0x{:X}\n",
            seq,
            reason,
            surface.pipe.name,
            surface.plane_slot,
            surface.buffer_index,
            path,
            compositor,
            composition_us,
            tiles.len(),
            change.len(),
            change_bounds.width,
            change_bounds.height,
            change_bounds.x,
            change_bounds.y,
            effective.len(),
            effective_bounds.width,
            effective_bounds.height,
            effective_bounds.x,
            effective_bounds.y,
            surface.width,
            surface.height,
            surface.pitch_bytes,
        );
    }
    true
}

pub(crate) fn present_rgba_overlay_tiles_with_background(
    tiles: &[RgbaOverlayTile<'_>],
    background: Option<Rgba8>,
    reason: &str,
) -> bool {
    present_rgba_overlay_tiles_on_slot_with_background(
        OVERLAY_PLANE_SLOT,
        tiles,
        background,
        reason,
    )
}

fn present_rgba_overlay_tiles_on_slot_with_background(
    plane_slot: usize,
    tiles: &[RgbaOverlayTile<'_>],
    background: Option<Rgba8>,
    reason: &str,
) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some((width, height)) = active_scanout_dimensions()
        .or_else(|| active_primary_surface().map(|primary| (primary.width, primary.height)))
    else {
        return false;
    };
    if width == 0 || height == 0 {
        return false;
    }
    let Some(surface) = ensure_overlay_surface_on_slot(dev, plane_slot, width, height) else {
        return false;
    };
    let background_pixel = background
        .map(|color| overlay_scanout_pixel_rgba_premul(color.r, color.g, color.b, color.a))
        .unwrap_or(0);
    fill_surface_color(
        surface.virt,
        surface.pitch_bytes as usize,
        surface.width,
        surface.height,
        background_pixel,
    );
    let mut contract_pixels = 0u64;
    let mut source_mismatches = 0u64;
    let mut storage_mismatches = 0u64;
    for tile in tiles {
        let Some((pixels, source_errors, storage_errors)) =
            copy_rgba_tile_into_overlay(surface, tile)
        else {
            return false;
        };
        contract_pixels = contract_pixels.saturating_add(pixels);
        source_mismatches = source_mismatches.saturating_add(source_errors);
        storage_mismatches = storage_mismatches.saturating_add(storage_errors);
    }
    crate::intel::dma_flush(surface.virt, surface.byte_len);

    let needs_flip = overlay_plane_needs_rearm(dev, surface, 0, 0, UI4_RGBA8_OVERLAY_CONTRACT);
    let (presented, path) = (
        present_overlay_surface_with_bootstrap_contract(
            dev,
            surface,
            0,
            0,
            UI4_RGBA8_OVERLAY_CONTRACT,
            reason,
        ),
        if needs_flip {
            "surf-only"
        } else {
            "already-live"
        },
    );
    if !presented {
        return false;
    }

    let seq = OVERLAY_PRESENT_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if seq <= 8 || seq.is_multiple_of(60) {
        crate::log!(
            "intel/display: rgba-tile-overlay-present seq={} reason={} pipe={} slot={} buffer={} path={} tiles={} scanout={}x{} pitch=0x{:X} background={:?}\n",
            seq,
            reason,
            surface.pipe.name,
            surface.plane_slot,
            surface.buffer_index,
            path,
            tiles.len(),
            surface.width,
            surface.height,
            surface.pitch_bytes,
            background,
        );
    }
    if tiles.iter().any(|tile| tile.expected_rgba.is_some()) {
        crate::log!(
            "intel/display: rgba-color-contract-proof tiles={} written_pixels={} source_mismatches={} storage_mismatches={} exact={} source_format=straight-rgba8 plane_storage=premultiplied-rgba8 alpha_contract=sw-premul\n",
            tiles.len(),
            contract_pixels,
            source_mismatches,
            storage_mismatches,
            (contract_pixels != 0 && source_mismatches == 0 && storage_mismatches == 0) as u8,
        );
    }
    true
}

pub(crate) fn present_live_overlay_rects_preserving(
    rects: &[LiveOverlayRect],
    preserve: Option<LiveOverlayRect>,
    reason: &str,
) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let (width, height) = active_scanout_dimensions()
        .or_else(|| active_primary_surface().map(|primary| (primary.width, primary.height)))
        .unwrap_or((0, 0));
    if width == 0 || height == 0 {
        return false;
    }
    let Some(surface) = ensure_overlay_surface(dev, width, height) else {
        return false;
    };

    if let Some(rect) = preserve {
        let _ = copy_overlay_front_into_back(surface);
        clear_overlay_except_rect(surface, rect);
    } else {
        fill_surface_color(
            surface.virt,
            surface.pitch_bytes as usize,
            surface.width,
            surface.height,
            0,
        );
    }
    for rect in rects {
        fill_overlay_rect_rgba(surface, *rect);
    }

    let byte_len = surface.byte_len;
    crate::intel::dma_flush(surface.virt, byte_len);

    if !present_overlay_surface_with_bootstrap_contract(
        dev,
        surface,
        0,
        0,
        UI4_RGBA8_OVERLAY_CONTRACT,
        reason,
    ) {
        return false;
    }

    let seq = OVERLAY_PRESENT_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if seq <= 8 || seq.is_multiple_of(60) {
        let plane_base = overlay_plane_base(surface.pipe, surface.plane_slot);
        crate::log!(
            "intel/display: live-overlay-present seq={} reason={} pipe={} slot={} rects={} size={}x{} pitch=0x{:X} surf=0x{:08X} surf_live=0x{:08X}\n",
            seq,
            reason,
            surface.pipe.name,
            surface.plane_slot,
            rects.len(),
            surface.width,
            surface.height,
            surface.pitch_bytes,
            crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF),
            crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF),
        );
    }

    true
}

pub(crate) fn present_rgba8_surface_to_primary_swap_xrgb(
    src: crate::intel::gpgpu::GpgpuRgba8Surface,
    src_rect: crate::intel::gpgpu::GpgpuRect,
    dst_xy: crate::intel::gpgpu::GpgpuPoint,
    reason: &str,
) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some(target) = active_display_pipeline_target() else {
        return false;
    };
    let Some(pipe) = target.pipeline.pipe() else {
        return false;
    };
    if target.width == 0
        || target.height == 0
        || src_rect.is_empty()
        || dst_xy.x < 0
        || dst_xy.y < 0
    {
        return false;
    }

    let Some(surface) =
        ensure_primary_swap_surface_for_pipe(dev, pipe, target.width, target.height)
    else {
        return false;
    };
    let dst_x = dst_xy.x as u32;
    let dst_y = dst_xy.y as u32;
    if dst_x >= surface.width || dst_y >= surface.height {
        return false;
    }
    let copy_w = src_rect.width.min(surface.width.saturating_sub(dst_x));
    let copy_h = src_rect.height.min(surface.height.saturating_sub(dst_y));
    if copy_w == 0 || copy_h == 0 {
        return false;
    }
    let covers_surface =
        dst_x == 0 && dst_y == 0 && copy_w >= surface.width && copy_h >= surface.height;
    if !covers_surface {
        let _ = copy_primary_swap_front_into_back(surface);
    } else {
        fill_surface_color(
            surface.virt,
            surface.pitch_bytes as usize,
            surface.width,
            surface.height,
            0,
        );
    }

    let Some(dst) = crate::intel::gpgpu::GpgpuRgba8Surface::new(
        surface.phys,
        surface.gpu,
        surface.byte_len,
        surface.width,
        surface.height,
        surface.pitch_bytes,
    ) else {
        return false;
    };
    let clipped_src_rect =
        crate::intel::gpgpu::GpgpuRect::new(src_rect.x, src_rect.y, copy_w, copy_h);
    let clipped_dst_xy = crate::intel::gpgpu::GpgpuPoint::new(dst_x as i32, dst_y as i32);
    let stats = crate::intel::gpgpu::present_rgba8_to_primary_xrgb_rect_stats(
        src,
        clipped_src_rect,
        dst,
        clipped_dst_xy,
        false,
    );
    if stats.spans == 0 || stats.submits == 0 {
        return false;
    }

    crate::intel::dma_flush(surface.virt, surface.byte_len);
    let live_target = display_pipeline_target_for_pipe(dev, pipe);
    if live_target != Some(target) {
        let live_activity = live_target
            .map(|live| live.activity.name())
            .unwrap_or("unavailable");
        let live_ddi = live_target
            .map(|live| live.route.ddi.name())
            .unwrap_or("unavailable");
        let live_mode = live_target
            .map(|live| live.route.mode_name())
            .unwrap_or("unavailable");
        crate::log_warn!(
            target: "intel/display";
            "intel/display: primary-swap commit rejected reason={} pipeline={} requested={}x{} requested_state={} requested_ddi={} requested_link={} live={}x{} live_state={} live_ddi={} live_link={} potential_reason=display-routing-or-mode-changed-during-custom-copy action=retain-current-scanout\n",
            reason,
            target.pipeline.name(),
            target.width,
            target.height,
            target.activity.name(),
            target.route.ddi.name(),
            target.route.mode_name(),
            live_target.map(|live| live.width).unwrap_or(0),
            live_target.map(|live| live.height).unwrap_or(0),
            live_activity,
            live_ddi,
            live_mode,
        );
        return false;
    }
    if !program_primary_plane_source_for_pipeline(
        PrimaryPlaneSource {
            phys: surface.phys,
            gpu: surface.gpu,
            byte_len: surface.byte_len,
            width: surface.width,
            height: surface.height,
            pitch_bytes: surface.pitch_bytes,
            format: PrimaryPlaneSourceFormat::Xrgb8888,
            src_x: 0,
            src_y: 0,
            dst_x: 0,
            dst_y: 0,
            dst_w: surface.width,
            dst_h: surface.height,
        },
        target.pipeline,
        reason,
        true,
    ) {
        return false;
    }
    mark_primary_swap_surface_front(surface);

    let seq = PRIMARY_PRESENT_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if seq <= 8 || seq.is_multiple_of(60) {
        crate::log!(
            "intel/display: primary-swap-gpgpu-present seq={} reason={} pipeline={} state={} ddi={} link_mode={} bpc={} pipe={} buffer={} copy_path=custom-gpgpu-rgba8-to-primary-xrgb zero_copy=0 rect={}x{}@{},{} size={}x{} pitch=0x{:X} gpu=0x{:X} spans={} submits={} submit_ms={} total_ms={}\n",
            seq,
            reason,
            DisplayPipelineId::from_pipe(surface.pipe)
                .map(DisplayPipelineId::name)
                .unwrap_or("pipe-invalid"),
            target.activity.name(),
            target.route.ddi.name(),
            target.route.mode_name(),
            target.route.bits_per_color(),
            surface.pipe.name,
            surface.buffer_index,
            copy_w,
            copy_h,
            dst_x,
            dst_y,
            surface.width,
            surface.height,
            surface.pitch_bytes,
            surface.gpu,
            stats.spans,
            stats.submits,
            stats.submit_ms,
            stats.total_ms,
        );
    }

    true
}

/// Compose premultiplied RGBA client frames into one opaque, double-buffered
/// primary scanout. This is the UI4 baseline compositor path: the display
/// reads one plane and the firmware-proven primary DBUF allocation remains
/// untouched.
pub(crate) fn present_premultiplied_rgba_primary_tiles(
    tiles: &[RgbaOverlayTile<'_>],
    reason: &str,
) -> bool {
    let (width, height) = active_scanout_dimensions().unwrap_or((0, 0));
    present_premultiplied_rgba_primary_tiles_damage(
        tiles,
        CompositionDamageRegion::from_rect(CompositionDamageRect::new(0, 0, width, height)),
        reason,
    )
}

pub(crate) fn present_premultiplied_rgba_primary_tiles_damage(
    tiles: &[RgbaOverlayTile<'_>],
    damage: CompositionDamageRegion,
    reason: &str,
) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some(target) = active_display_pipeline_target() else {
        return false;
    };
    let Some(pipe) = target.pipeline.pipe() else {
        return false;
    };
    if target.width == 0 || target.height == 0 {
        return false;
    }
    let damage = clip_composition_damage_region(damage, target.width, target.height);
    if damage.is_empty() {
        return true;
    }
    let Some(surface) =
        ensure_primary_swap_surface_for_pipe(dev, pipe, target.width, target.height)
    else {
        return false;
    };
    let effective = {
        let pool = primary_swap_surface_pool(surface.pipe).lock();
        let mut effective = pool.damage_debt[surface.buffer_index];
        effective.add_region(damage);
        effective
    };
    let composition_started_ns = crate::chronos::monotonic_nanos();
    let compositor =
        match compose_premultiplied_rgba_tiles_into_primary_gpgpu(surface, tiles, effective) {
            GpgpuCompositionResult::Complete => "guc-simd16-sprite-quad",
            GpgpuCompositionResult::Unavailable => {
                // Reconstruct damaged pixels from the original opaque primary,
                // then apply the current scene exactly once. Reusing the previous
                // composite as an alpha source would accumulate translucent
                // content every presentation.
                for damage in effective.rects() {
                    if !restore_primary_composition_base_rect(surface, *damage) {
                        return false;
                    }
                    for tile in tiles {
                        if !blend_premultiplied_rgba_tile_into_primary_clipped(
                            surface, tile, *damage,
                        ) {
                            return false;
                        }
                    }
                }
                if !dma_flush_primary_swap_region(surface, effective) {
                    return false;
                }
                "cpu-fallback"
            }
            GpgpuCompositionResult::SubmittedIncomplete => {
                // This back buffer may still be GPU-owned. Keep scanning the old
                // front instead of racing it with a CPU replay or plane flip.
                return false;
            }
        };
    let composition_us =
        crate::chronos::monotonic_nanos().saturating_sub(composition_started_ns) / 1_000;

    if display_pipeline_target_for_pipe(dev, pipe) != Some(target) {
        return false;
    }
    if !program_primary_plane_source_for_pipeline(
        PrimaryPlaneSource {
            phys: surface.phys,
            gpu: surface.gpu,
            byte_len: surface.byte_len,
            width: surface.width,
            height: surface.height,
            pitch_bytes: surface.pitch_bytes,
            format: PrimaryPlaneSourceFormat::Xrgb8888,
            src_x: 0,
            src_y: 0,
            dst_x: 0,
            dst_y: 0,
            dst_w: surface.width,
            dst_h: surface.height,
        },
        target.pipeline,
        reason,
        true,
    ) {
        return false;
    }
    mark_primary_composition_surface_front(surface, damage);

    let seq = PRIMARY_PRESENT_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if seq <= 8 || seq.is_multiple_of(60) {
        let primary_base = pipe.primary_plane().base();
        let damage_bounds = damage.bounding_rect().unwrap_or_default();
        let effective_bounds = effective.bounding_rect().unwrap_or_default();
        crate::log!(
            "intel/display: primary-tile-compositor-present seq={} reason={} pipeline={} pipe={} buffer={} compositor={} composition_us={} tiles={} damage_rects={} damage_bounds={}x{}@{},{} effective_rects={} effective_bounds={}x{}@{},{} size={}x{} pitch=0x{:X} gpu=0x{:X} buf_cfg=0x{:08X} surf=0x{:08X} surf_live=0x{:08X}\n",
            seq,
            reason,
            target.pipeline.name(),
            pipe.name,
            surface.buffer_index,
            compositor,
            composition_us,
            tiles.len(),
            damage.len(),
            damage_bounds.width,
            damage_bounds.height,
            damage_bounds.x,
            damage_bounds.y,
            effective.len(),
            effective_bounds.width,
            effective_bounds.height,
            effective_bounds.x,
            effective_bounds.y,
            surface.width,
            surface.height,
            surface.pitch_bytes,
            surface.gpu,
            crate::intel::mmio_read(dev, primary_base + UNI_PLANE_BUF_CFG_OFF),
            crate::intel::mmio_read(dev, primary_base + UNI_PLANE_SURF_OFF),
            crate::intel::mmio_read(dev, primary_base + UNI_PLANE_SURFLIVE_OFF),
        );
    }
    true
}

fn clear_overlay_except_rect(surface: OverlaySurface, rect: LiveOverlayRect) {
    let x0 = rect.x.min(surface.width);
    let y0 = rect.y.min(surface.height);
    let x1 = x0.saturating_add(rect.width).min(surface.width);
    let y1 = y0.saturating_add(rect.height).min(surface.height);
    if x0 >= x1 || y0 >= y1 {
        fill_surface_color(
            surface.virt,
            surface.pitch_bytes as usize,
            surface.width,
            surface.height,
            0,
        );
        return;
    }

    fill_overlay_rect(surface, 0, 0, surface.width, y0, 0);
    fill_overlay_rect(surface, 0, y1, surface.width, surface.height.saturating_sub(y1), 0);
    fill_overlay_rect(surface, 0, y0, x0, y1.saturating_sub(y0), 0);
    fill_overlay_rect(surface, x1, y0, surface.width.saturating_sub(x1), y1.saturating_sub(y0), 0);
}

fn present_rgba_overlay(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    position: Option<(u32, u32)>,
    preserve_alpha: bool,
    reason: &str,
) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    if src_width == 0 || src_height == 0 || src_pitch_bytes < src_width as usize * 4 {
        return false;
    }

    let Some((scanout_width, scanout_height)) = active_scanout_dimensions() else {
        return false;
    };
    let Some(surface) = ensure_overlay_surface(dev, scanout_width, scanout_height) else {
        return false;
    };
    let pos_x = position
        .map(|(x, _)| x.min(scanout_width.saturating_sub(src_width)))
        .unwrap_or_else(|| {
            scanout_width
                .saturating_sub(src_width)
                .saturating_sub(OVERLAY_MARGIN_X)
        });
    let pos_y = position
        .map(|(_, y)| y.min(scanout_height.saturating_sub(src_height)))
        .unwrap_or_else(|| OVERLAY_MARGIN_Y.min(scanout_height.saturating_sub(src_height)));

    fill_surface_color(
        surface.virt,
        surface.pitch_bytes as usize,
        surface.width,
        surface.height,
        0,
    );
    if !copy_rgba_into_overlay(
        surface,
        src,
        src_width,
        src_height,
        src_pitch_bytes,
        pos_x,
        pos_y,
        preserve_alpha,
    ) {
        return false;
    }
    if reason == "gfx-full-scene-alpha-overlay" {
        stamp_overlay_composition_proof_marker(surface, UI4_RGBA8_OVERLAY_CONTRACT, reason);
    }

    let byte_len = surface.byte_len;
    crate::intel::dma_flush(surface.virt, byte_len);

    if !present_overlay_surface_with_bootstrap_contract(
        dev,
        surface,
        0,
        0,
        UI4_RGBA8_OVERLAY_CONTRACT,
        reason,
    ) {
        return false;
    }

    let seq = OVERLAY_PRESENT_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if seq <= 8 || seq.is_multiple_of(60) {
        log_primary_surface_samples("under-overlay-present");
        log_pipe_live_scanout_state("overlay-present");
        log_display_power_well_snapshot("overlay-present");
        let plane_base = overlay_plane_base(surface.pipe, surface.plane_slot);
        crate::log!(
            "intel/display: overlay-present seq={} reason={} pipe={} slot={} source_alpha={} scanout=premultiplied-rgba8 pos={}x{} size={}x{} pitch=0x{:X} gpu=0x{:X} phys=0x{:X} surf=0x{:08X} surf_live=0x{:08X}\n",
            seq,
            reason,
            surface.pipe.name,
            surface.plane_slot,
            if preserve_alpha { "preserve" } else { "opaque" },
            pos_x,
            pos_y,
            surface.width,
            surface.height,
            surface.pitch_bytes,
            surface.gpu,
            surface.phys,
            crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF),
            crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF)
        );
    }

    true
}

#[inline]
fn clamp_u8_i32(value: i32) -> u8 {
    value.clamp(0, 255) as u8
}

fn aspect_fit_size(
    src_width: usize,
    src_height: usize,
    dst_width: usize,
    dst_height: usize,
) -> (usize, usize) {
    if src_width == 0 || src_height == 0 || dst_width == 0 || dst_height == 0 {
        return (0, 0);
    }
    if src_width <= dst_width && src_height <= dst_height {
        return (src_width, src_height);
    }
    if dst_width.saturating_mul(src_height) <= dst_height.saturating_mul(src_width) {
        let copy_w = dst_width.max(1);
        let copy_h = src_height
            .saturating_mul(copy_w)
            .checked_div(src_width)
            .unwrap_or(1)
            .max(1)
            .min(dst_height);
        (copy_w, copy_h)
    } else {
        let copy_h = dst_height.max(1);
        let copy_w = src_width
            .saturating_mul(copy_h)
            .checked_div(src_height)
            .unwrap_or(1)
            .max(1)
            .min(dst_width);
        (copy_w, copy_h)
    }
}

fn center_crop_size(
    src_width: usize,
    src_height: usize,
    dst_width: usize,
    dst_height: usize,
) -> (usize, usize) {
    if src_width == 0 || src_height == 0 || dst_width == 0 || dst_height == 0 {
        return (0, 0);
    }

    (src_width.min(dst_width), src_height.min(dst_height))
}

#[inline(always)]
fn media_ytile_8bpp_offset(byte_x: usize, row_y: usize, tiles_per_row: usize) -> usize {
    const YTILE_W: usize = 128;
    const YTILE_H: usize = 32;

    let tile_col = byte_x / YTILE_W;
    let tile_row = row_y / YTILE_H;
    let in_x = byte_x % YTILE_W;
    let in_y = row_y % YTILE_H;
    let oword_col = in_x / 16;
    let byte_in_oword = in_x % 16;
    let within_tile = oword_col * 512 + in_y * 16 + byte_in_oword;
    (tile_row * tiles_per_row + tile_col) * 4096 + within_tile
}

#[inline(always)]
fn nv12_pixel_to_bgra(y: i32, u: i32, v: i32) -> u32 {
    let c = (y - 16).max(0);
    let u = u - 128;
    let v = v - 128;
    let r = clamp_u8_i32((298 * c + 409 * v + 128) >> 8);
    let g = clamp_u8_i32((298 * c - 100 * u - 208 * v + 128) >> 8);
    let b = clamp_u8_i32((298 * c + 516 * u + 128) >> 8);
    u32::from_le_bytes([b, g, r, 0])
}

fn present_ytile_nv12_surface_center_1to1(
    surface: PrimarySurface,
    src: &[u8],
    visible_x: usize,
    visible_y: usize,
    visible_width: usize,
    visible_height: usize,
    tiles_per_row: usize,
    chroma_y_offset: usize,
    dst_x: usize,
    dst_y: usize,
) {
    let dst_pitch = surface.pitch_bytes as usize;
    for row_idx in 0..visible_height {
        let src_y = visible_y + row_idx;
        let uv_row = chroma_y_offset + src_y / 2;
        let dst_row_off = (dst_y + row_idx)
            .saturating_mul(dst_pitch)
            .saturating_add(dst_x.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..visible_width {
            let src_x = visible_x + col_idx;
            let y_off = media_ytile_8bpp_offset(src_x, src_y, tiles_per_row);
            let uv_x = (src_x / 2).saturating_mul(2);
            let u_off = media_ytile_8bpp_offset(uv_x, uv_row, tiles_per_row);
            let v_off = media_ytile_8bpp_offset(uv_x + 1, uv_row, tiles_per_row);
            let pixel = nv12_pixel_to_bgra(
                unsafe { i32::from(*src.get_unchecked(y_off)) },
                unsafe { i32::from(*src.get_unchecked(u_off)) },
                unsafe { i32::from(*src.get_unchecked(v_off)) },
            );
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
    }
}

fn dma_flush_primary_rect(
    surface: PrimarySurface,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
) -> usize {
    let dst_pitch = surface.pitch_bytes as usize;
    let row_bytes = width.saturating_mul(4);
    if row_bytes == 0 || height == 0 {
        return 0;
    }
    for row_idx in 0..height {
        let row_off = y
            .saturating_add(row_idx)
            .saturating_mul(dst_pitch)
            .saturating_add(x.saturating_mul(4));
        unsafe {
            crate::intel::dma_flush(surface.virt.add(row_off), row_bytes);
        }
    }
    row_bytes.saturating_mul(height)
}

pub(crate) fn present_imc3_surface_center(
    src: &[u8],
    coded_width: u32,
    coded_height: u32,
    visible_x: u32,
    visible_y: u32,
    visible_width: u32,
    visible_height: u32,
    src_pitch_bytes: usize,
) -> bool {
    let Some(surface) = active_primary_surface() else {
        return false;
    };
    if surface.virt.is_null() || coded_width == 0 || coded_height == 0 {
        return false;
    }

    let coded_width = coded_width as usize;
    let coded_height = coded_height as usize;
    let visible_x = visible_x as usize;
    let visible_y = visible_y as usize;
    let visible_width = visible_width as usize;
    let visible_height = visible_height as usize;
    if src_pitch_bytes < coded_width || visible_width == 0 || visible_height == 0 {
        return false;
    }
    if visible_x.saturating_add(visible_width) > coded_width
        || visible_y.saturating_add(visible_height) > coded_height
    {
        return false;
    }

    const YTILE_W: usize = 128;
    const YTILE_H: usize = 32;
    let tiles_per_row = src_pitch_bytes / YTILE_W;
    if tiles_per_row == 0 {
        return false;
    }
    let chroma_y_offset = (coded_height + YTILE_H - 1) & !(YTILE_H - 1);
    let chroma_plane_rows = coded_height.div_ceil(2);
    let chroma_plane_stride_rows = (chroma_plane_rows + YTILE_H - 1) & !(YTILE_H - 1);
    let cr_y_offset = chroma_y_offset + chroma_plane_stride_rows;
    let total_height = cr_y_offset + chroma_plane_rows;
    let total_tile_rows = (total_height + YTILE_H - 1) / YTILE_H;
    let needed = total_tile_rows * tiles_per_row * 4096;
    if src.len() < needed {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    if dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    let (copy_w, copy_h) = aspect_fit_size(visible_width, visible_height, dst_width, dst_height);
    if copy_w == 0 || copy_h == 0 {
        return false;
    }
    let dst_x = dst_width.saturating_sub(copy_w) / 2;
    let dst_y = dst_height.saturating_sub(copy_h) / 2;

    #[inline(always)]
    fn ytile_offset(byte_x: usize, row_y: usize, tiles_per_row: usize) -> usize {
        let tile_col = byte_x / YTILE_W;
        let tile_row = row_y / YTILE_H;
        let in_x = byte_x % YTILE_W;
        let in_y = row_y % YTILE_H;
        let oword_col = in_x / 16;
        let byte_in_oword = in_x % 16;
        let within_tile = oword_col * 512 + in_y * 16 + byte_in_oword;
        (tile_row * tiles_per_row + tile_col) * 4096 + within_tile
    }

    for row_idx in 0..copy_h {
        let src_y = visible_y.saturating_add(
            row_idx
                .saturating_mul(visible_height)
                .checked_div(copy_h.max(1))
                .unwrap_or(0)
                .min(visible_height.saturating_sub(1)),
        );
        let dst_row_off = (dst_y + row_idx)
            .saturating_mul(dst_pitch)
            .saturating_add(dst_x.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        let cb_row = chroma_y_offset + src_y / 2;
        let cr_row = cr_y_offset + src_y / 2;
        for col_idx in 0..copy_w {
            let src_x = visible_x.saturating_add(
                col_idx
                    .saturating_mul(visible_width)
                    .checked_div(copy_w.max(1))
                    .unwrap_or(0)
                    .min(visible_width.saturating_sub(1)),
            );
            let y_off = ytile_offset(src_x, src_y, tiles_per_row);
            let chroma_x = src_x / 2;
            let cb_off = ytile_offset(chroma_x, cb_row, tiles_per_row);
            let cr_off = ytile_offset(chroma_x, cr_row, tiles_per_row);
            let y = unsafe { i32::from(*src.get_unchecked(y_off)) };
            let c = (y - 16).max(0);
            let u = unsafe { i32::from(*src.get_unchecked(cb_off)) } - 128;
            let v = unsafe { i32::from(*src.get_unchecked(cr_off)) } - 128;
            let r = clamp_u8_i32((298 * c + 409 * v + 128) >> 8);
            let g = clamp_u8_i32((298 * c - 100 * u - 208 * v + 128) >> 8);
            let b = clamp_u8_i32((298 * c + 516 * u + 128) >> 8);
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
    }

    let byte_len = dst_pitch.saturating_mul(dst_height);
    crate::intel::dma_flush(surface.virt, byte_len);
    notify_primary_surface_present(surface, "hw-logo-imc3-center", byte_len);
    true
}

pub(crate) fn present_ytile_nv12_surface_center(
    src: &[u8],
    coded_width: u32,
    coded_height: u32,
    visible_x: u32,
    visible_y: u32,
    visible_width: u32,
    visible_height: u32,
    src_pitch_bytes: usize,
    src_uv_offset: usize,
) -> bool {
    let Some(surface) = active_primary_surface() else {
        return false;
    };
    if surface.virt.is_null() || coded_width == 0 || coded_height == 0 {
        return false;
    }

    const YTILE_W: usize = 128;
    const YTILE_H: usize = 32;

    let coded_width = coded_width as usize;
    let coded_height = coded_height as usize;
    let visible_x = visible_x as usize;
    let visible_y = visible_y as usize;
    let visible_width = visible_width as usize;
    let visible_height = visible_height as usize;
    if src_pitch_bytes < coded_width
        || !src_pitch_bytes.is_multiple_of(YTILE_W)
        || visible_width == 0
        || visible_height == 0
    {
        return false;
    }
    if visible_x.saturating_add(visible_width) > coded_width
        || visible_y.saturating_add(visible_height) > coded_height
    {
        return false;
    }

    let tiles_per_row = src_pitch_bytes / YTILE_W;
    if tiles_per_row == 0 {
        return false;
    }
    if src_uv_offset < src_pitch_bytes.saturating_mul(coded_height)
        || src_uv_offset % src_pitch_bytes != 0
    {
        return false;
    }
    let chroma_y_offset = src_uv_offset / src_pitch_bytes;
    let total_height = chroma_y_offset.saturating_add(coded_height.div_ceil(2));
    let needed = total_height
        .div_ceil(YTILE_H)
        .saturating_mul(tiles_per_row)
        .saturating_mul(4096);
    if src.len() < needed {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    if dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    let (copy_w, copy_h) = aspect_fit_size(visible_width, visible_height, dst_width, dst_height);
    if copy_w == 0 || copy_h == 0 {
        return false;
    }
    let dst_x = dst_width.saturating_sub(copy_w) / 2;
    let dst_y = dst_height.saturating_sub(copy_h) / 2;

    if copy_w == visible_width && copy_h == visible_height {
        present_ytile_nv12_surface_center_1to1(
            surface,
            src,
            visible_x,
            visible_y,
            visible_width,
            visible_height,
            tiles_per_row,
            chroma_y_offset,
            dst_x,
            dst_y,
        );
        let byte_len = dma_flush_primary_rect(surface, dst_x, dst_y, copy_w, copy_h);
        notify_primary_surface_present(surface, "ytile-nv12-center-1to1", byte_len);
        return true;
    }

    for row_idx in 0..copy_h {
        let src_y = visible_y.saturating_add(
            row_idx
                .saturating_mul(visible_height)
                .checked_div(copy_h.max(1))
                .unwrap_or(0)
                .min(visible_height.saturating_sub(1)),
        );
        let dst_row_off = (dst_y + row_idx)
            .saturating_mul(dst_pitch)
            .saturating_add(dst_x.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_x = visible_x.saturating_add(
                col_idx
                    .saturating_mul(visible_width)
                    .checked_div(copy_w.max(1))
                    .unwrap_or(0)
                    .min(visible_width.saturating_sub(1)),
            );
            let y_off = media_ytile_8bpp_offset(src_x, src_y, tiles_per_row);
            let uv_x = (src_x / 2).saturating_mul(2);
            let uv_row = chroma_y_offset.saturating_add(src_y / 2);
            let u_off = media_ytile_8bpp_offset(uv_x, uv_row, tiles_per_row);
            let v_off = media_ytile_8bpp_offset(uv_x + 1, uv_row, tiles_per_row);
            let pixel = nv12_pixel_to_bgra(
                unsafe { i32::from(*src.get_unchecked(y_off)) },
                unsafe { i32::from(*src.get_unchecked(u_off)) },
                unsafe { i32::from(*src.get_unchecked(v_off)) },
            );
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
    }

    let byte_len = dst_pitch.saturating_mul(dst_height);
    crate::intel::dma_flush(surface.virt, byte_len);
    notify_primary_surface_present(surface, "ytile-nv12-center", byte_len);
    true
}

pub(crate) fn present_nv12_surface_center(
    src: &[u8],
    coded_width: u32,
    coded_height: u32,
    visible_x: u32,
    visible_y: u32,
    visible_width: u32,
    visible_height: u32,
    src_pitch_bytes: usize,
) -> bool {
    let Some(surface) = active_primary_surface() else {
        return false;
    };
    if surface.virt.is_null() || coded_width == 0 || coded_height == 0 {
        return false;
    }

    let coded_width = coded_width as usize;
    let coded_height = coded_height as usize;
    let visible_x = visible_x as usize;
    let visible_y = visible_y as usize;
    let visible_width = visible_width as usize;
    let visible_height = visible_height as usize;
    if src_pitch_bytes < coded_width || visible_width == 0 || visible_height == 0 {
        return false;
    }
    if visible_x.saturating_add(visible_width) > coded_width
        || visible_y.saturating_add(visible_height) > coded_height
    {
        return false;
    }

    if !src_pitch_bytes.is_multiple_of(super::xelp_media2_ngin::MEDIA_TILE64_W) {
        return false;
    }
    let tiles_per_row = src_pitch_bytes / super::xelp_media2_ngin::MEDIA_TILE64_W;
    if tiles_per_row == 0 {
        return false;
    }
    let Some((chroma_y_offset, needed)) =
        super::xelp_media2_ngin::media_tile64_nv12_surface_layout(coded_height, src_pitch_bytes)
    else {
        return false;
    };
    if src.len() < needed {
        return false;
    }

    let dst_width = surface.width as usize;
    let dst_height = surface.height as usize;
    let dst_pitch = surface.pitch_bytes as usize;
    if dst_pitch < dst_width.saturating_mul(4) {
        return false;
    }

    let (copy_w, copy_h) = aspect_fit_size(visible_width, visible_height, dst_width, dst_height);
    if copy_w == 0 || copy_h == 0 {
        return false;
    }
    let dst_x = dst_width.saturating_sub(copy_w) / 2;
    let dst_y = dst_height.saturating_sub(copy_h) / 2;

    for row_idx in 0..copy_h {
        let src_y = visible_y.saturating_add(
            row_idx
                .saturating_mul(visible_height)
                .checked_div(copy_h.max(1))
                .unwrap_or(0)
                .min(visible_height.saturating_sub(1)),
        );
        let dst_row_off = (dst_y + row_idx)
            .saturating_mul(dst_pitch)
            .saturating_add(dst_x.saturating_mul(4));
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col_idx in 0..copy_w {
            let src_x = visible_x.saturating_add(
                col_idx
                    .saturating_mul(visible_width)
                    .checked_div(copy_w.max(1))
                    .unwrap_or(0)
                    .min(visible_width.saturating_sub(1)),
            );
            let y_off =
                super::xelp_media2_ngin::media_tile64_8bpp_offset(src_x, src_y, tiles_per_row);
            let uv_x = (src_x / 2).saturating_mul(2);
            let uv_row = chroma_y_offset.saturating_add(src_y / 2);
            let u_off =
                super::xelp_media2_ngin::media_tile64_8bpp_offset(uv_x, uv_row, tiles_per_row);
            let v_off =
                super::xelp_media2_ngin::media_tile64_8bpp_offset(uv_x + 1, uv_row, tiles_per_row);
            let y = unsafe { i32::from(*src.get_unchecked(y_off)) };
            let c = (y - 16).max(0);
            let u = unsafe { i32::from(*src.get_unchecked(u_off)) } - 128;
            let v = unsafe { i32::from(*src.get_unchecked(v_off)) } - 128;
            let (r, g, b) =
                if VIDEO_NV12_BLACK_PROOF_LIFT && y <= 24 && u.abs() <= 4 && v.abs() <= 4 {
                    let checker = ((row_idx >> 5) ^ (col_idx >> 5)) & 1;
                    if checker == 0 {
                        (0x30, 0x58, 0xD0)
                    } else {
                        (0x70, 0x20, 0xA0)
                    }
                } else {
                    (
                        clamp_u8_i32((298 * c + 409 * v + 128) >> 8),
                        clamp_u8_i32((298 * c - 100 * u - 208 * v + 128) >> 8),
                        clamp_u8_i32((298 * c + 516 * u + 128) >> 8),
                    )
                };
            let pixel = u32::from_le_bytes([b, g, r, 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
    }

    let byte_len = dst_pitch.saturating_mul(dst_height);
    crate::intel::dma_flush(surface.virt, byte_len);
    notify_primary_surface_present(surface, "nv12-center", byte_len);
    true
}

fn notify_primary_surface_present(surface: PrimarySurface, reason: &str, byte_len: usize) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some(surface_reg) = u32::try_from(surface.gpu).ok() else {
        return false;
    };

    let seq = PRIMARY_PRESENT_SEQ.fetch_add(1, Ordering::AcqRel) + 1;

    let pipeconf_off = PIPECONF_A + surface.pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let trans_ddi_func_ctl_off =
        TRANS_DDI_FUNC_CTL_A + surface.pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let trans_psr_ctl_off = TRANS_PSR_CTL_A + surface.pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let trans_psr_status_off =
        TRANS_PSR_STATUS_A + surface.pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let trans_psr2_ctl_off = TRANS_PSR2_CTL_A + surface.pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let trans_psr2_status_off =
        TRANS_PSR2_STATUS_A + surface.pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    probe_primary_present_psr(dev, surface, reason, seq);

    let plane = surface.pipe.primary_plane();
    let surf_before = crate::intel::mmio_read(dev, plane.surf());
    let surf_live_before = crate::intel::mmio_read(dev, plane.surf_live());
    if !ui4_rgba8_plane_stack_ready(surface.pipe)
        || !primary_plane_contract_regs_match(dev, surface)
    {
        crate::log_error!(target: "intel/display";
            "intel/display: primary-present rejected seq={} reason={} pipe={} cause=immutable-rgba8-contract-mismatch surf=0x{:08X} live=0x{:08X}\n",
            seq,
            reason,
            surface.pipe.name,
            surf_before,
            surf_live_before,
        );
        return false;
    }

    // The Intel bootstrap already installed the complete plane contract. If
    // this exact surface is live, CPU cache visibility is the only required
    // operation; otherwise publish only PLANE_SURF and preserve the contract.
    if surf_before == surface_reg && surf_live_before == surface_reg {
        if should_log_primary_present(seq) {
            intel_display_verbose_log!(
                "intel/display: primary-flip seq={} reason={} pipe={} surf=0x{:08X} fast-skip contract=bootstrap-immutable\n",
                seq,
                reason,
                surface.pipe.name,
                surface_reg,
            );
        }
        return true;
    }

    crate::intel::mmio_write(dev, plane.surf(), surface_reg);
    let (surf_live_after, iter) =
        wait_for_plane_live_for(dev, plane.base(), surface_reg, UI4_PLANE_SURFACE_FLIP_TIMEOUT_NS);
    if should_log_primary_present(seq) {
        intel_display_verbose_log!(
            "intel/display: primary-present seq={} reason={} pipe={} bytes=0x{:X} pipeconf=0x{:08X} ddi_func_ctl=0x{:08X} psr_ctl=0x{:08X} psr_status=0x{:08X} psr2_ctl=0x{:08X} psr2_status=0x{:08X} surf=0x{:08X}=>0x{:08X} live=0x{:08X}=>0x{:08X} iter={} contract=bootstrap-immutable mutation=surf-only\n",
            seq,
            reason,
            surface.pipe.name,
            byte_len,
            crate::intel::mmio_read(dev, pipeconf_off),
            crate::intel::mmio_read(dev, trans_ddi_func_ctl_off),
            crate::intel::mmio_read(dev, trans_psr_ctl_off),
            crate::intel::mmio_read(dev, trans_psr_status_off),
            crate::intel::mmio_read(dev, trans_psr2_ctl_off),
            crate::intel::mmio_read(dev, trans_psr2_status_off),
            surf_before,
            crate::intel::mmio_read(dev, plane.surf()),
            surf_live_before,
            surf_live_after,
            iter,
        );
    }
    surf_live_after == surface_reg
}

fn primary_plane_contract_regs_match(dev: crate::intel::Dev, surface: PrimarySurface) -> bool {
    let Some(stride_reg) = plane_stride_reg_value(surface.pitch_bytes) else {
        return false;
    };
    let ctl = crate::intel::mmio_read(dev, surface.pipe.primary_plane().ctl());
    let ctl_expected = primary_plane_ctl_enabled(ctl);
    let ctl_match_mask = PLANE_CTL_ENABLE
        | PLANE_CTL_ARB_SLOTS_MASK
        | PLANE_CTL_FORMAT_MASK_SKL
        | PLANE_CTL_KEY_ENABLE_MASK
        | PLANE_CTL_TILED_MASK
        | PLANE_CTL_ORDER_RGBX;
    crate::intel::mmio_read(dev, surface.pipe.primary_plane().stride()) == stride_reg
        && crate::intel::mmio_read(dev, surface.pipe.primary_plane().base() + UNI_PLANE_POS_OFF)
            == plane_pos_reg_value(0, 0)
        && crate::intel::mmio_read(dev, surface.pipe.primary_plane().base() + UNI_PLANE_SIZE_OFF)
            == plane_size_reg_value(surface.width, surface.height)
        && crate::intel::mmio_read(dev, surface.pipe.primary_plane().base() + UNI_PLANE_OFFSET_OFF)
            == plane_pos_reg_value(0, 0)
        && (ctl & ctl_match_mask) == (ctl_expected & ctl_match_mask)
}

#[inline]
fn should_log_primary_present(seq: u32) -> bool {
    if crate::log_os::flags::INTEL_STAGE1_LOGS {
        return false;
    }
    seq <= 8 || seq.is_multiple_of(60)
}

fn wait_for_pipe_next_frame(dev: crate::intel::Dev, pipe: PipeInfo) -> (u32, u32, usize) {
    let frame_off = PIPE_FRMCOUNT_A + pipe.slot.saturating_mul(PIPE_MMIO_STRIDE);
    let before = crate::intel::mmio_read(dev, frame_off);
    let mut after = before;
    let mut iter = 0usize;
    while iter < 200_000 && after == before {
        core::hint::spin_loop();
        after = crate::intel::mmio_read(dev, frame_off);
        iter += 1;
    }
    (before, after, iter)
}

fn wait_for_plane_live(
    dev: crate::intel::Dev,
    plane_base: usize,
    want_live: u32,
    max_iters: usize,
) -> (u32, usize) {
    let mut live = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF);
    let mut iter = 0usize;
    while iter < max_iters && live != want_live {
        core::hint::spin_loop();
        live = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF);
        iter += 1;
    }
    (live, iter)
}

fn wait_for_plane_live_for(
    dev: crate::intel::Dev,
    plane_base: usize,
    want_live: u32,
    timeout_ns: u64,
) -> (u32, usize) {
    let started_ns = crate::chronos::monotonic_nanos();
    let mut live = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF);
    let mut iter = 0usize;
    while iter < 5_000_000 && live != want_live {
        if iter.is_multiple_of(256)
            && crate::chronos::monotonic_nanos().saturating_sub(started_ns) >= timeout_ns
        {
            break;
        }
        core::hint::spin_loop();
        live = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF);
        iter += 1;
    }
    (live, iter)
}

/// Begin one bounded UI4 display transaction. Stable plane contracts may
/// stage only their double-buffered SURF address while this transaction is
/// active; format, geometry, DBUF and watermark changes remain synchronous.
pub(crate) fn begin_ui4_plane_surface_flip_batch() -> bool {
    if crate::intel::claimed_device().is_none() {
        return false;
    }
    let mut batch = UI4_PLANE_SURFACE_FLIP_BATCH.lock();
    if batch.active {
        return false;
    }
    *batch = PlaneSurfaceFlipBatch {
        active: true,
        accepting: true,
        ..PlaneSurfaceFlipBatch::new()
    };
    true
}

fn queue_ui4_plane_surface_flip(
    plane_base: usize,
    surface_reg: u32,
    reason: &str,
) -> PlaneSurfaceFlipQueueResult {
    // Do not absorb a concurrent non-UI4 display client merely because UI4
    // currently has a transaction open on another CPU.
    if !reason.starts_with("ui4-") {
        return PlaneSurfaceFlipQueueResult::Inactive;
    }
    let mut batch = UI4_PLANE_SURFACE_FLIP_BATCH.lock();
    if !batch.active {
        return PlaneSurfaceFlipQueueResult::Inactive;
    }
    if !batch.accepting {
        return PlaneSurfaceFlipQueueResult::Rejected;
    }
    for entry in batch.entries[..batch.len].iter().flatten() {
        if entry.plane_base == plane_base {
            return if entry.surface_reg == surface_reg {
                PlaneSurfaceFlipQueueResult::Queued
            } else {
                PlaneSurfaceFlipQueueResult::Rejected
            };
        }
    }
    if batch.len >= batch.entries.len() {
        return PlaneSurfaceFlipQueueResult::Rejected;
    }
    let index = batch.len;
    batch.entries[index] = Some(PlaneSurfaceFlip {
        plane_base,
        surface_reg,
    });
    batch.len += 1;
    PlaneSurfaceFlipQueueResult::Queued
}

/// Publish every staged SURF address back-to-back, then wait once for the
/// complete set of SURFLIVE registers. This avoids serially consuming one
/// scanout-latch wait per changed UI4 plane.
pub(crate) fn finish_ui4_plane_surface_flip_batch() -> bool {
    let queued = {
        let mut batch = UI4_PLANE_SURFACE_FLIP_BATCH.lock();
        if !batch.active || !batch.accepting {
            return false;
        }
        batch.accepting = false;
        let queued = *batch;
        queued
    };
    if queued.len == 0 {
        *UI4_PLANE_SURFACE_FLIP_BATCH.lock() = PlaneSurfaceFlipBatch::new();
        return true;
    }
    let Some(dev) = crate::intel::claimed_device() else {
        *UI4_PLANE_SURFACE_FLIP_BATCH.lock() = PlaneSurfaceFlipBatch::new();
        return false;
    };

    for entry in queued.entries[..queued.len].iter().flatten() {
        crate::intel::mmio_write(dev, entry.plane_base + UNI_PLANE_SURF_OFF, entry.surface_reg);
    }

    let started_ns = crate::chronos::monotonic_nanos();
    let mut live = [0u32; UI4_PLANE_SURFACE_FLIP_BATCH_CAPACITY];
    let mut live_mask: u32;
    let mut iterations = 0usize;
    loop {
        live_mask = 0;
        for (index, entry) in queued.entries[..queued.len].iter().flatten().enumerate() {
            live[index] = crate::intel::mmio_read(dev, entry.plane_base + UNI_PLANE_SURFLIVE_OFF);
            if live[index] == entry.surface_reg {
                live_mask |= 1u32 << index;
            }
        }
        let want_mask = (1u32 << queued.len) - 1;
        if live_mask == want_mask {
            break;
        }
        if iterations.is_multiple_of(256)
            && crate::chronos::monotonic_nanos().saturating_sub(started_ns)
                >= UI4_PLANE_SURFACE_FLIP_TIMEOUT_NS
        {
            break;
        }
        core::hint::spin_loop();
        iterations += 1;
    }

    let want_mask = (1u32 << queued.len) - 1;
    let committed = live_mask == want_mask;
    let elapsed_ns = crate::chronos::monotonic_nanos().saturating_sub(started_ns);
    let seq = UI4_PLANE_SURFACE_FLIP_BATCH_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if !committed || seq <= 8 || seq.is_multiple_of(60) {
        crate::log!(
            "intel/display: ui4-plane-surface-flip-batch seq={} ok={} planes={} live_mask=0x{:X} want_mask=0x{:X} wait_iters={} wait_ns={} commit=surf-addresses-together wait=shared\n",
            seq,
            committed as u8,
            queued.len,
            live_mask,
            want_mask,
            iterations,
            elapsed_ns,
        );
    }
    if !committed {
        for (index, entry) in queued.entries[..queued.len].iter().flatten().enumerate() {
            if live[index] != entry.surface_reg {
                crate::log_warn!(
                    target: "intel/display";
                    "intel/display: ui4-plane-surface-flip timeout plane_base=0x{:X} surf=0x{:08X} live=0x{:08X}\n",
                    entry.plane_base,
                    entry.surface_reg,
                    live[index],
                );
            }
        }
    }
    *UI4_PLANE_SURFACE_FLIP_BATCH.lock() = PlaneSurfaceFlipBatch::new();
    committed
}

fn primary_plane_ctl_enabled(ctl_before: u32) -> u32 {
    let format = match PRIMARY_FORMAT_PROBE_MODE {
        PRIMARY_FORMAT_PROBE_XBGR => PrimaryPlaneSourceFormat::Xbgr8888,
        _ => PrimaryPlaneSourceFormat::Xrgb8888,
    };
    primary_plane_ctl_enabled_for_format(ctl_before, format)
}

fn primary_plane_ctl_enabled_for_format(ctl_before: u32, format: PrimaryPlaneSourceFormat) -> u32 {
    let order_bits = match format {
        PrimaryPlaneSourceFormat::Xrgb8888 => 0,
        PrimaryPlaneSourceFormat::Xbgr8888 => PLANE_CTL_ORDER_RGBX,
    };
    (ctl_before
        & !(PLANE_CTL_ENABLE
            | PLANE_CTL_ARB_SLOTS_MASK
            | PLANE_CTL_FORMAT_MASK_SKL
            | PLANE_CTL_KEY_ENABLE_MASK
            | PLANE_CTL_TILED_MASK
            | PLANE_CTL_ORDER_RGBX
            | PLANE_CTL_YUV420_Y_PLANE))
        | PLANE_CTL_ENABLE
        | PLANE_CTL_ARB_SLOTS_4BPP
        | PLANE_CTL_FORMAT_XRGB_8888
        | PLANE_CTL_TILED_LINEAR
        | order_bits
}

fn overlay_plane_ctl_enabled(ctl_before: u32, alpha: OverlayAlphaMode) -> u32 {
    let ctl = primary_plane_ctl_enabled(ctl_before);
    match alpha {
        // The GPGPU kernels store bytes R,G,B,A. Gen12's RGBX order bit
        // selects that byte order for the XRGB8888 plane format.
        OverlayAlphaMode::PremultipliedRgba => ctl | PLANE_CTL_ORDER_RGBX,
        OverlayAlphaMode::Opaque => ctl & !PLANE_CTL_ORDER_RGBX,
    }
}

fn plane_color_ctl_alpha(color_ctl: u32, alpha: OverlayAlphaMode) -> u32 {
    let alpha_bits = match alpha {
        OverlayAlphaMode::Opaque => PLANE_COLOR_ALPHA_DISABLE,
        OverlayAlphaMode::PremultipliedRgba => PLANE_COLOR_ALPHA_SW_PREMULT,
    };
    (color_ctl
        & !(PLANE_COLOR_ALPHA_MASK
            | PLANE_COLOR_YUV_RANGE_CORRECTION_DISABLE
            | PLANE_COLOR_PIPE_CSC_ENABLE
            | PLANE_COLOR_PLANE_CSC_ENABLE
            | PLANE_COLOR_INPUT_CSC_ENABLE
            | PLANE_COLOR_CSC_MODE_MASK))
        | PLANE_COLOR_PLANE_GAMMA_DISABLE
        | PLANE_COLOR_CSC_MODE_BYPASS
        | alpha_bits
}

fn plane_keymax_alpha(alpha: u8) -> u32 {
    (u32::from(alpha) << 24) & PLANE_KEYMAX_ALPHA_MASK
}

fn plane_keymsk_alpha(alpha: u8) -> u32 {
    if alpha < 0xFF {
        PLANE_KEYMSK_ALPHA_ENABLE
    } else {
        0
    }
}

pub(super) fn decoded_nv12_overlay_plane_alpha() -> u8 {
    VIDEO_NV12_PLANE_ALPHA.load(Ordering::Acquire) as u8
}

pub(super) fn program_decoded_nv12_overlay_plane_alpha(
    dev: crate::intel::Dev,
    uv_base: usize,
    y_base: usize,
) -> DecodedNv12PlaneAlphaProgram {
    let alpha = decoded_nv12_overlay_plane_alpha();
    let keymsk = plane_keymsk_alpha(alpha);
    let keymax = plane_keymax_alpha(alpha);
    let uv_keymsk_before = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_KEYMSK_OFF);
    let uv_keymax_before = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_KEYMAX_OFF);
    let y_keymsk_before = crate::intel::mmio_read(dev, y_base + UNI_PLANE_KEYMSK_OFF);
    let y_keymax_before = crate::intel::mmio_read(dev, y_base + UNI_PLANE_KEYMAX_OFF);

    crate::intel::mmio_write(dev, uv_base + UNI_PLANE_KEYMSK_OFF, keymsk);
    crate::intel::mmio_write(dev, uv_base + UNI_PLANE_KEYMAX_OFF, keymax);
    crate::intel::mmio_write(dev, y_base + UNI_PLANE_KEYMSK_OFF, keymsk);
    crate::intel::mmio_write(dev, y_base + UNI_PLANE_KEYMAX_OFF, keymax);

    DecodedNv12PlaneAlphaProgram {
        alpha,
        uv_keymsk_before,
        uv_keymsk_after: crate::intel::mmio_read(dev, uv_base + UNI_PLANE_KEYMSK_OFF),
        uv_keymax_before,
        uv_keymax_after: crate::intel::mmio_read(dev, uv_base + UNI_PLANE_KEYMAX_OFF),
        y_keymsk_before,
        y_keymsk_after: crate::intel::mmio_read(dev, y_base + UNI_PLANE_KEYMSK_OFF),
        y_keymax_before,
        y_keymax_after: crate::intel::mmio_read(dev, y_base + UNI_PLANE_KEYMAX_OFF),
    }
}

pub(crate) fn set_decoded_nv12_overlay_plane_alpha(alpha: u8, reason: &str) -> bool {
    if !LEGACY_DIRECT_NV12_PLANE_ABI_ENABLED {
        return false;
    }
    let before = VIDEO_NV12_PLANE_ALPHA.swap(u32::from(alpha), Ordering::AcqRel) as u8;
    let mut applied = None;
    if let Some(dev) = crate::intel::claimed_device()
        && let Some(pipe) = active_pipe(dev)
    {
        if ui4_rgba8_plane_stack_ready(pipe) {
            return false;
        }
        let uv_base = overlay_plane_base(pipe, VIDEO_NV12_PLANE_SLOT);
        let y_base = overlay_plane_base(pipe, VIDEO_NV12_Y_PLANE_SLOT);
        let uv_ctl = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_CTL_OFF);
        let y_ctl = crate::intel::mmio_read(dev, y_base + UNI_PLANE_CTL_OFF);
        if (uv_ctl & PLANE_CTL_ENABLE) != 0 && (y_ctl & PLANE_CTL_ENABLE) != 0 {
            let proof = program_decoded_nv12_overlay_plane_alpha(dev, uv_base, y_base);
            let uv_surf = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_SURF_OFF);
            let y_surf = crate::intel::mmio_read(dev, y_base + UNI_PLANE_SURF_OFF);
            crate::intel::mmio_write(dev, uv_base + UNI_PLANE_SURF_OFF, uv_surf);
            crate::intel::mmio_write(dev, y_base + UNI_PLANE_SURF_OFF, y_surf);
            let (frame_before, frame_after, frame_wait) = wait_for_pipe_next_frame(dev, pipe);
            applied = Some((pipe, proof, frame_before, frame_after, frame_wait));
        }
    }

    if let Some((pipe, proof, frame_before, frame_after, frame_wait)) = applied {
        crate::log!(
            "intel/display: nv12-linked-plane-alpha-set reason={} pipe={} stored={}=>{} applied=1 uv_slot={} y_slot={} uv_keymsk=0x{:08X}->0x{:08X} uv_keymax=0x{:08X}->0x{:08X} y_keymsk=0x{:08X}->0x{:08X} y_keymax=0x{:08X}->0x{:08X} frame={}=>{} frame_wait={}\n",
            reason,
            pipe.name,
            before,
            alpha,
            VIDEO_NV12_PLANE_SLOT,
            VIDEO_NV12_Y_PLANE_SLOT,
            proof.uv_keymsk_before,
            proof.uv_keymsk_after,
            proof.uv_keymax_before,
            proof.uv_keymax_after,
            proof.y_keymsk_before,
            proof.y_keymsk_after,
            proof.y_keymax_before,
            proof.y_keymax_after,
            frame_before,
            frame_after,
            frame_wait
        );
    } else {
        crate::log!(
            "intel/display: nv12-linked-plane-alpha-set reason={} stored={}=>{} applied=0 note=will-apply-on-next-nv12-arm\n",
            reason,
            before,
            alpha
        );
    }
    true
}

fn plane_buf_cfg_value(start: u16, end_inclusive: u16) -> u32 {
    ((u32::from(end_inclusive) & 0x1FFF) << 16) | (u32::from(start) & 0x1FFF)
}

const fn plane_buf_cfg_start(raw: u32) -> u16 {
    (raw & 0x1FFF) as u16
}

const fn plane_buf_cfg_end(raw: u32) -> u16 {
    ((raw >> 16) & 0x1FFF) as u16
}

fn program_plane_watermark_boot_safe(dev: crate::intel::Dev, plane_base: usize, enable: bool) {
    crate::intel::mmio_write(
        dev,
        plane_base + UNI_PLANE_WM_0_OFF,
        if enable { PLANE_WM_LEVEL0_BOOT_SAFE } else { 0 },
    );

    let mut level = 1usize;
    while level < UNI_PLANE_WM_LEVELS {
        crate::intel::mmio_write(dev, plane_base + UNI_PLANE_WM_0_OFF + level * 4, 0);
        level += 1;
    }

    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_WM_TRANS_OFF, 0);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_WM_SAGV_OFF, 0);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_WM_SAGV_TRANS_OFF, 0);
}

fn plane_watermarks_are_boot_safe(dev: crate::intel::Dev, plane_base: usize) -> bool {
    if crate::intel::mmio_read(dev, plane_base + UNI_PLANE_WM_0_OFF) != PLANE_WM_LEVEL0_BOOT_SAFE {
        return false;
    }
    let mut level = 1usize;
    while level < UNI_PLANE_WM_LEVELS {
        if crate::intel::mmio_read(dev, plane_base + UNI_PLANE_WM_0_OFF + level * 4) != 0 {
            return false;
        }
        level += 1;
    }
    crate::intel::mmio_read(dev, plane_base + UNI_PLANE_WM_TRANS_OFF) == 0
        && crate::intel::mmio_read(dev, plane_base + UNI_PLANE_WM_SAGV_OFF) == 0
        && crate::intel::mmio_read(dev, plane_base + UNI_PLANE_WM_SAGV_TRANS_OFF) == 0
}

fn program_plane_buf_cfg(
    dev: crate::intel::Dev,
    plane_base: usize,
    start: u16,
    end_inclusive: u16,
) -> (u32, u32) {
    let before = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_BUF_CFG_OFF);
    let after = plane_buf_cfg_value(start, end_inclusive);
    if before != after {
        crate::intel::mmio_write(dev, plane_base + UNI_PLANE_BUF_CFG_OFF, after);
    }
    (before, crate::intel::mmio_read(dev, plane_base + UNI_PLANE_BUF_CFG_OFF))
}

fn ui4_rgba8_plane_stack_ready(pipe: PipeInfo) -> bool {
    UI4_RGBA8_PLANE_STACK_STATE.load(Ordering::Acquire) == UI4_RGBA8_PLANE_STACK_READY
        && UI4_RGBA8_PLANE_STACK_PIPE_SLOT.load(Ordering::Acquire) == pipe.slot as u32
}

pub(crate) fn ui4_rgba8_plane_stack_is_ready() -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    active_pipe(dev).is_some_and(ui4_rgba8_plane_stack_ready)
}

fn wait_for_plane_stack_live(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    wanted: [u32; UNIVERSAL_PLANE_SLOTS],
    max_iters: usize,
) -> (u32, usize) {
    let want_mask = (1u32 << UNIVERSAL_PLANE_SLOTS) - 1;
    let mut live_mask = 0u32;
    let mut iterations = 0usize;
    while iterations < max_iters {
        live_mask = 0;
        for (slot, wanted) in wanted.iter().copied().enumerate() {
            if crate::intel::mmio_read(dev, pipe.plane(slot).surf_live()) == wanted {
                live_mask |= 1u32 << slot;
            }
        }
        if live_mask == want_mask {
            break;
        }
        core::hint::spin_loop();
        iterations += 1;
    }
    (live_mask, iterations)
}

fn program_rgba8_plane_static_contract(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    slot: usize,
    width: u32,
    height: u32,
    pitch_bytes: u32,
) -> Option<u32> {
    let plane = pipe.plane(slot);
    let stride = plane_stride_reg_value(pitch_bytes)?;
    let ctl_before = crate::intel::mmio_read(dev, plane.ctl());
    let alpha = if slot == crate::ui4::PRIMARY_PLANE_SLOT {
        OverlayAlphaMode::Opaque
    } else {
        // UI4 producers and the fixed overlay scanout buffers share native
        // premultiplied RGBA8; no per-frame channel swizzle is required.
        UI4_RGBA8_OVERLAY_CONTRACT
    };
    let ctl_enabled = if slot == crate::ui4::PRIMARY_PLANE_SLOT {
        primary_plane_ctl_enabled(ctl_before)
    } else {
        overlay_plane_ctl_enabled(ctl_before, alpha)
    };
    let color_ctl_off = plane.base() + UNI_PLANE_COLOR_CTL_OFF;
    let color_ctl = plane_color_ctl_alpha(crate::intel::mmio_read(dev, color_ctl_off), alpha);

    crate::intel::mmio_write(dev, plane.ctl(), ctl_enabled & !PLANE_CTL_ENABLE);
    crate::intel::mmio_write(dev, plane.stride(), stride);
    crate::intel::mmio_write(dev, plane.base() + UNI_PLANE_POS_OFF, plane_pos_reg_value(0, 0));
    crate::intel::mmio_write(
        dev,
        plane.base() + UNI_PLANE_SIZE_OFF,
        plane_size_reg_value(width, height),
    );
    crate::intel::mmio_write(dev, plane.base() + UNI_PLANE_OFFSET_OFF, 0);
    crate::intel::mmio_write(dev, plane.base() + UNI_PLANE_CUS_CTL_OFF, 0);
    crate::intel::mmio_write(dev, plane.base() + UNI_PLANE_KEYVAL_OFF, 0);
    crate::intel::mmio_write(dev, plane.base() + UNI_PLANE_KEYMSK_OFF, 0);
    crate::intel::mmio_write(dev, plane.base() + UNI_PLANE_KEYMAX_OFF, 0xFF00_0000);
    crate::intel::mmio_write(dev, plane.base() + UNI_PLANE_AUX_DIST_OFF, 0);
    crate::intel::mmio_write(dev, plane.base() + UNI_PLANE_AUX_OFFSET_OFF, 0);
    crate::intel::mmio_write(dev, color_ctl_off, color_ctl);
    Some(ctl_enabled)
}

/// Establish the complete active UI4 pipe contract (normally Pipe A) once.
///
/// Slots 0-3 share one full-output 4-BPP composition lifecycle: slot 0 is the
/// opaque primary and slots 1-3 are native premultiplied RGBA8 overlays. Slot
/// 4 receives the same immutable overlay contract but remains independently
/// owned by UI4 interaction. After bootstrap, only PLANE_SURF may change.
fn bootstrap_ui4_rgba8_plane_stack_once(dev: crate::intel::Dev, primary: PrimarySurface) -> bool {
    if UI4_RGBA8_PLANE_STACK_STATE
        .compare_exchange(
            UI4_RGBA8_PLANE_STACK_UNINITIALIZED,
            UI4_RGBA8_PLANE_STACK_INITIALIZING,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_err()
    {
        return ui4_rgba8_plane_stack_ready(primary.pipe);
    }

    let fail = |reason: &'static str| {
        UI4_RGBA8_PLANE_STACK_STATE.store(UI4_RGBA8_PLANE_STACK_FAILED, Ordering::Release);
        crate::log_error!(target: "intel/display";
            "intel/display: ui4-rgba8-plane-stack bootstrap failed pipe={} reason={} retry=forbidden\n",
            primary.pipe.name,
            reason,
        );
        false
    };
    let pipe = primary.pipe;
    let mut overlay_surfaces = [None; UNIVERSAL_PLANE_SLOTS - 1];
    for slot in 1..UNIVERSAL_PLANE_SLOTS {
        let Some(surface) =
            ensure_overlay_surface_for_pipe(dev, pipe, slot, primary.width, primary.height)
        else {
            return fail("transparent-front-allocation");
        };
        overlay_surfaces[slot - 1] = Some(surface);
    }

    let primary_reg = match u32::try_from(primary.gpu) {
        Ok(reg) => reg,
        Err(_) => return fail("primary-address-range"),
    };
    let mut surface_regs = [0u32; UNIVERSAL_PLANE_SLOTS];
    surface_regs[0] = primary_reg;
    for (index, surface) in overlay_surfaces.iter().flatten().enumerate() {
        let Ok(surface_reg) = u32::try_from(surface.gpu) else {
            return fail("overlay-address-range");
        };
        surface_regs[index + 1] = surface_reg;
    }

    // PLANE_SURF is the double-buffered commit trigger. Stage every disabled
    // CTL and static register without issuing an interim SURF=0 commit, so the
    // firmware front remains visible until the complete UI4 stack replaces it
    // at one shared frame boundary.
    let full_mask = (1u32 << UNIVERSAL_PLANE_SLOTS) - 1;
    let dbuf_ranges = [
        (PLANE_DBUF_SLOT_0_START, PLANE_DBUF_SLOT_0_END),
        (PLANE_DBUF_SLOT_1_START, PLANE_DBUF_SLOT_1_END),
        (PLANE_DBUF_SLOT_2_START, PLANE_DBUF_SLOT_2_END),
        (PLANE_DBUF_SLOT_3_START, PLANE_DBUF_SLOT_3_END),
        (PLANE_DBUF_SLOT_4_START, PLANE_DBUF_SLOT_4_END),
    ];
    let mut controls = [0u32; UNIVERSAL_PLANE_SLOTS];
    for slot in 0..UNIVERSAL_PLANE_SLOTS {
        let pitch = if slot == 0 {
            primary.pitch_bytes
        } else {
            overlay_surfaces[slot - 1]
                .expect("overlay bootstrap surface")
                .pitch_bytes
        };
        let Some(ctl) = program_rgba8_plane_static_contract(
            dev,
            pipe,
            slot,
            primary.width,
            primary.height,
            pitch,
        ) else {
            return fail("static-contract");
        };
        controls[slot] = ctl;
        let plane_base = pipe.plane(slot).base();
        program_plane_watermark_boot_safe(dev, plane_base, true);
        let (start, end) = dbuf_ranges[slot];
        let (_, programmed) = program_plane_buf_cfg(dev, plane_base, start, end);
        if programmed != plane_buf_cfg_value(start, end) {
            return fail("dbuf-readback");
        }
    }

    // Publish every final CTL/SURF pair back-to-back and consume one common
    // latch. These are the only bootstrap SURF writes for slots 0-4.
    for slot in 0..UNIVERSAL_PLANE_SLOTS {
        let plane = pipe.plane(slot);
        crate::intel::mmio_write(dev, plane.ctl(), controls[slot]);
        crate::intel::mmio_write(dev, plane.surf(), surface_regs[slot]);
    }
    let (arm_frame_before, arm_frame_after, arm_frame_wait) = wait_for_pipe_next_frame(dev, pipe);
    let (live_mask, live_iters) = wait_for_plane_stack_live(dev, pipe, surface_regs, 5_000_000);
    if live_mask != full_mask {
        return fail("plane-arm-timeout");
    }

    for surface in overlay_surfaces.iter().flatten().copied() {
        mark_overlay_surface_front(surface);
    }
    UI4_RGBA8_PLANE_STACK_PIPE_SLOT.store(pipe.slot as u32, Ordering::Release);
    UI4_RGBA8_PLANE_STACK_STATE.store(UI4_RGBA8_PLANE_STACK_READY, Ordering::Release);
    crate::log!(
        "intel/display: ui4-rgba8-plane-stack bootstrap ok=1 pipe={} slots=0-3-composition+4-interaction size={}x{} slot0=xrgb8-opaque slots1-4=premultiplied-rgba8-linear contracts=once bootstrap_surf_writes=one-per-slot runtime=surf-flips-only dbuf_blocks={}/{}/{}/{}/{} commit_frame={}=>{} commit_wait={} live_iters={} live_mask=0x{:X}\n",
        pipe.name,
        primary.width,
        primary.height,
        PLANE_DBUF_BALANCED_BLOCKS,
        PLANE_DBUF_BALANCED_BLOCKS,
        PLANE_DBUF_BALANCED_BLOCKS,
        PLANE_DBUF_BALANCED_BLOCKS,
        PLANE_DBUF_TOP_BLOCKS,
        arm_frame_before,
        arm_frame_after,
        arm_frame_wait,
        live_iters,
        live_mask,
    );
    true
}

fn park_decoded_nv12_overlay_plane_before_disable(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    uv_base: usize,
    y_base: usize,
    reason: &str,
) -> bool {
    let uv_ctl = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_CTL_OFF);
    let y_ctl = crate::intel::mmio_read(dev, y_base + UNI_PLANE_CTL_OFF);
    if (uv_ctl & PLANE_CTL_ENABLE) == 0 || (y_ctl & PLANE_CTL_ENABLE) == 0 {
        return false;
    }

    let uv_pos_before = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_POS_OFF);
    let uv_size_before = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_SIZE_OFF);
    let y_pos_before = crate::intel::mmio_read(dev, y_base + UNI_PLANE_POS_OFF);
    let y_size_before = crate::intel::mmio_read(dev, y_base + UNI_PLANE_SIZE_OFF);
    let uv_surf_before = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_SURF_OFF);
    let y_surf_before = crate::intel::mmio_read(dev, y_base + UNI_PLANE_SURF_OFF);
    let (scanout_w, scanout_h) = active_scanout_dimensions()
        .unwrap_or((VIDEO_NV12_HIDE_PARK_SIZE, VIDEO_NV12_HIDE_PARK_SIZE));
    let mut park_w = VIDEO_NV12_HIDE_PARK_SIZE.min(scanout_w);
    let mut park_h = VIDEO_NV12_HIDE_PARK_SIZE.min(scanout_h);
    if park_w > 1 {
        park_w &= !1;
    }
    if park_h > 1 {
        park_h &= !1;
    }
    if park_w == 0 || park_h == 0 {
        return false;
    }
    let hide_pos =
        plane_pos_reg_value(scanout_w.saturating_sub(park_w), scanout_h.saturating_sub(park_h));
    let hide_size = plane_size_reg_value(park_w, park_h);

    crate::intel::mmio_write(dev, uv_base + UNI_PLANE_POS_OFF, hide_pos);
    crate::intel::mmio_write(dev, y_base + UNI_PLANE_POS_OFF, hide_pos);
    crate::intel::mmio_write(dev, uv_base + UNI_PLANE_SIZE_OFF, hide_size);
    crate::intel::mmio_write(dev, y_base + UNI_PLANE_SIZE_OFF, hide_size);
    crate::intel::mmio_write(dev, uv_base + UNI_PLANE_SURF_OFF, uv_surf_before);
    crate::intel::mmio_write(dev, y_base + UNI_PLANE_SURF_OFF, y_surf_before);

    let (frame_before, frame_after, frame_wait) = wait_for_pipe_next_frame(dev, pipe);
    let uv_pos_after = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_POS_OFF);
    let y_pos_after = crate::intel::mmio_read(dev, y_base + UNI_PLANE_POS_OFF);
    let uv_size_after = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_SIZE_OFF);
    let y_size_after = crate::intel::mmio_read(dev, y_base + UNI_PLANE_SIZE_OFF);
    let ok = uv_pos_after == hide_pos
        && y_pos_after == hide_pos
        && uv_size_after == hide_size
        && y_size_after == hide_size;

    crate::log!(
        "intel/display: nv12-linked-plane-park-before-hide reason={} pipe={} ok={} uv_slot={} y_slot={} scanout={}x{} park={}x{} pos=0x{:08X}/0x{:08X}->0x{:08X} y_pos=0x{:08X}->0x{:08X} size=0x{:08X}/0x{:08X}->0x{:08X} y_size=0x{:08X}->0x{:08X} frame={}=>{} frame_wait={}\n",
        reason,
        pipe.name,
        ok as u8,
        VIDEO_NV12_PLANE_SLOT,
        VIDEO_NV12_Y_PLANE_SLOT,
        scanout_w,
        scanout_h,
        park_w,
        park_h,
        uv_pos_before,
        hide_pos,
        uv_pos_after,
        y_pos_before,
        y_pos_after,
        uv_size_before,
        hide_size,
        uv_size_after,
        y_size_before,
        y_size_after,
        frame_before,
        frame_after,
        frame_wait
    );

    ok
}

fn mute_decoded_nv12_overlay_plane_before_hide(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    uv_base: usize,
    y_base: usize,
    reason: &str,
) -> bool {
    let uv_ctl = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_CTL_OFF);
    let y_ctl = crate::intel::mmio_read(dev, y_base + UNI_PLANE_CTL_OFF);
    if (uv_ctl & PLANE_CTL_ENABLE) == 0 || (y_ctl & PLANE_CTL_ENABLE) == 0 {
        return false;
    }

    let uv_keymsk_before = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_KEYMSK_OFF);
    let uv_keymax_before = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_KEYMAX_OFF);
    let y_keymsk_before = crate::intel::mmio_read(dev, y_base + UNI_PLANE_KEYMSK_OFF);
    let y_keymax_before = crate::intel::mmio_read(dev, y_base + UNI_PLANE_KEYMAX_OFF);
    let uv_surf = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_SURF_OFF);
    let y_surf = crate::intel::mmio_read(dev, y_base + UNI_PLANE_SURF_OFF);

    let keymsk = plane_keymsk_alpha(0);
    let keymax = plane_keymax_alpha(0);
    crate::intel::mmio_write(dev, uv_base + UNI_PLANE_KEYMSK_OFF, keymsk);
    crate::intel::mmio_write(dev, uv_base + UNI_PLANE_KEYMAX_OFF, keymax);
    crate::intel::mmio_write(dev, y_base + UNI_PLANE_KEYMSK_OFF, keymsk);
    crate::intel::mmio_write(dev, y_base + UNI_PLANE_KEYMAX_OFF, keymax);
    crate::intel::mmio_write(dev, uv_base + UNI_PLANE_SURF_OFF, uv_surf);
    crate::intel::mmio_write(dev, y_base + UNI_PLANE_SURF_OFF, y_surf);

    let (frame_before, frame_after, frame_wait) = wait_for_pipe_next_frame(dev, pipe);
    let uv_keymsk_after = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_KEYMSK_OFF);
    let uv_keymax_after = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_KEYMAX_OFF);
    let y_keymsk_after = crate::intel::mmio_read(dev, y_base + UNI_PLANE_KEYMSK_OFF);
    let y_keymax_after = crate::intel::mmio_read(dev, y_base + UNI_PLANE_KEYMAX_OFF);
    let ok = uv_keymsk_after == keymsk
        && uv_keymax_after == keymax
        && y_keymsk_after == keymsk
        && y_keymax_after == keymax;

    crate::log!(
        "intel/display: nv12-linked-plane-mute-before-hide reason={} pipe={} ok={} uv_slot={} y_slot={} uv_keymsk=0x{:08X}->0x{:08X} uv_keymax=0x{:08X}->0x{:08X} y_keymsk=0x{:08X}->0x{:08X} y_keymax=0x{:08X}->0x{:08X} frame={}=>{} frame_wait={}\n",
        reason,
        pipe.name,
        ok as u8,
        VIDEO_NV12_PLANE_SLOT,
        VIDEO_NV12_Y_PLANE_SLOT,
        uv_keymsk_before,
        uv_keymsk_after,
        uv_keymax_before,
        uv_keymax_after,
        y_keymsk_before,
        y_keymsk_after,
        y_keymax_before,
        y_keymax_after,
        frame_before,
        frame_after,
        frame_wait
    );

    ok
}

pub(crate) fn hide_decoded_nv12_overlay_plane(reason: &str) -> bool {
    if !LEGACY_DIRECT_NV12_PLANE_ABI_ENABLED {
        return false;
    }
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some(pipe) = active_pipe(dev) else {
        return false;
    };
    if ui4_rgba8_plane_stack_ready(pipe) {
        crate::log_warn!(target: "intel/display";
            "intel/display: legacy nv12 hide rejected reason={} pipe={} cause=immutable-rgba8-stack\n",
            reason,
            pipe.name,
        );
        return false;
    }

    let uv_base = overlay_plane_base(pipe, VIDEO_NV12_PLANE_SLOT);
    let y_base = overlay_plane_base(pipe, VIDEO_NV12_Y_PLANE_SLOT);
    let muted_ok = mute_decoded_nv12_overlay_plane_before_hide(dev, pipe, uv_base, y_base, reason);
    let parked_ok = VIDEO_NV12_HIDE_PARK_BEFORE_DISABLE
        && park_decoded_nv12_overlay_plane_before_disable(dev, pipe, uv_base, y_base, reason);
    let uv_ctl_before = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_CTL_OFF);
    let uv_surf_before = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_SURF_OFF);
    let uv_live_before = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_SURFLIVE_OFF);
    let y_ctl_before = crate::intel::mmio_read(dev, y_base + UNI_PLANE_CTL_OFF);
    let y_surf_before = crate::intel::mmio_read(dev, y_base + UNI_PLANE_SURF_OFF);
    let y_live_before = crate::intel::mmio_read(dev, y_base + UNI_PLANE_SURFLIVE_OFF);

    let uv_ctl_after_want = uv_ctl_before & !PLANE_CTL_ENABLE;
    let y_ctl_after_want = y_ctl_before & !PLANE_CTL_ENABLE;
    crate::intel::mmio_write(dev, uv_base + UNI_PLANE_CTL_OFF, uv_ctl_after_want);
    crate::intel::mmio_write(dev, y_base + UNI_PLANE_CTL_OFF, y_ctl_after_want);
    crate::intel::mmio_write(dev, uv_base + UNI_PLANE_SURF_OFF, 0);
    crate::intel::mmio_write(dev, y_base + UNI_PLANE_SURF_OFF, 0);
    let (frame_before, frame_after, frame_wait) = wait_for_pipe_next_frame(dev, pipe);
    let (uv_live_after, uv_live_iters) = wait_for_plane_live(dev, uv_base, 0, 20_000);
    let (y_live_after, y_live_iters) = wait_for_plane_live(dev, y_base, 0, 20_000);

    crate::intel::mmio_write(dev, uv_base + UNI_PLANE_CUS_CTL_OFF, 0);
    crate::intel::mmio_write(dev, y_base + UNI_PLANE_CUS_CTL_OFF, 0);

    let uv_ctl_after = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_CTL_OFF);
    let uv_surf_after = crate::intel::mmio_read(dev, uv_base + UNI_PLANE_SURF_OFF);
    let y_ctl_after = crate::intel::mmio_read(dev, y_base + UNI_PLANE_CTL_OFF);
    let y_surf_after = crate::intel::mmio_read(dev, y_base + UNI_PLANE_SURF_OFF);
    let ok = (uv_ctl_after & PLANE_CTL_ENABLE) == 0
        && (y_ctl_after & PLANE_CTL_ENABLE) == 0
        && uv_surf_after == 0
        && y_surf_after == 0;

    crate::log!(
        "intel/display: nv12-linked-plane-hide reason={} pipe={} ok={} muted={} parked={} uv_slot={} y_slot={} uv_ctl=0x{:08X}->0x{:08X} uv_surf=0x{:08X}->0x{:08X} uv_live=0x{:08X}->0x{:08X} y_ctl=0x{:08X}->0x{:08X} y_surf=0x{:08X}->0x{:08X} y_live=0x{:08X}->0x{:08X} frame={}=>{} frame_wait={} uv_live_iters={} y_live_iters={}\n",
        reason,
        pipe.name,
        ok as u8,
        muted_ok as u8,
        parked_ok as u8,
        VIDEO_NV12_PLANE_SLOT,
        VIDEO_NV12_Y_PLANE_SLOT,
        uv_ctl_before,
        uv_ctl_after,
        uv_surf_before,
        uv_surf_after,
        uv_live_before,
        uv_live_after,
        y_ctl_before,
        y_ctl_after,
        y_surf_before,
        y_surf_after,
        y_live_before,
        y_live_after,
        frame_before,
        frame_after,
        frame_wait,
        uv_live_iters,
        y_live_iters
    );

    ok
}

pub(crate) fn kick_primary_surface_scanout(label: &str) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some(surface) = active_primary_surface() else {
        return false;
    };

    let pos_off = surface.pipe.primary_plane().base() + UNI_PLANE_POS_OFF;
    let size_off = surface.pipe.primary_plane().base() + UNI_PLANE_SIZE_OFF;
    let pos_before = crate::intel::mmio_read(dev, pos_off);
    let size_before = crate::intel::mmio_read(dev, size_off);
    let stride_before = crate::intel::mmio_read(dev, surface.pipe.primary_plane().stride());
    let surf_before = crate::intel::mmio_read(dev, surface.pipe.primary_plane().surf());
    let live_before = crate::intel::mmio_read(dev, surface.pipe.primary_plane().surf_live());
    let Some(surface_reg) = u32::try_from(surface.gpu).ok() else {
        return false;
    };

    let presented = notify_primary_surface_present(surface, label, surface.byte_len);
    let live_after = crate::intel::mmio_read(dev, surface.pipe.primary_plane().surf_live());
    let pos_after = crate::intel::mmio_read(dev, pos_off);
    let size_after = crate::intel::mmio_read(dev, size_off);
    let stride_after = crate::intel::mmio_read(dev, surface.pipe.primary_plane().stride());
    let surf_after = crate::intel::mmio_read(dev, surface.pipe.primary_plane().surf());

    intel_display_verbose_log!(
        "intel/display: primary-scanout-kick label={} pipe={} stride_before=0x{:08X} stride_after=0x{:08X} size_before=0x{:08X} size_after=0x{:08X} pos_before=0x{:08X} pos_after=0x{:08X} surf_before=0x{:08X} surf_after=0x{:08X} live_before=0x{:08X} live_after=0x{:08X} contract=bootstrap-immutable mutation=surf-only\n",
        label,
        surface.pipe.name,
        stride_before,
        stride_after,
        size_before,
        size_after,
        pos_before,
        pos_after,
        surf_before,
        surf_after,
        live_before,
        live_after,
    );

    presented && live_after == surface_reg
}

pub(crate) fn log_pipe_live_scanout_state(label: &str) {
    let Some(dev) = crate::intel::claimed_device() else {
        return;
    };
    let Some(surface) = active_primary_surface() else {
        return;
    };
    let pipe = surface.pipe;
    let pipe_src_raw = crate::intel::mmio_read(dev, pipe.pipe_src_off);
    let (pipe_w, pipe_h) = decode_pipe_src(pipe_src_raw).unwrap_or((0, 0));
    crate::log!(
        "intel/display: live-scanout label={} pipe={} pipe_src=0x{:08X} dims={}x{} primary_surf_gpu=0x{:08X}\n",
        label,
        pipe.name,
        pipe_src_raw,
        pipe_w,
        pipe_h,
        crate::intel::mmio_read(dev, pipe.primary_plane().surf())
    );

    let mut slot = 0usize;
    while slot < UNIVERSAL_PLANE_SLOTS {
        let plane_base = pipe.plane(slot).base();
        let ctl = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_CTL_OFF);
        let stride = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_STRIDE_OFF);
        let pos = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_POS_OFF);
        let size = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SIZE_OFF);
        let keyval = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_KEYVAL_OFF);
        let keymsk = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_KEYMSK_OFF);
        let surf = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF);
        let keymax = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_KEYMAX_OFF);
        let offset = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_OFFSET_OFF);
        let surf_live = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF);
        let aux_dist = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_AUX_DIST_OFF);
        let aux_offset = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_AUX_OFFSET_OFF);
        let cus_ctl = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_CUS_CTL_OFF);
        let color_ctl = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_COLOR_CTL_OFF);
        let wm0 = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_WM_0_OFF);
        let wm_sagv = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_WM_SAGV_OFF);
        let wm_sagv_trans = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_WM_SAGV_TRANS_OFF);
        let wm_trans = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_WM_TRANS_OFF);
        let buf_cfg = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_BUF_CFG_OFF);
        crate::log!(
            "intel/display: live-plane label={} pipe={} slot={} enabled={} format={} tiled={} rot={} rgbx={} stride=0x{:08X} pos={}x{} size={}x{} surf=0x{:08X} surf_live=0x{:08X} cus_ctl=0x{:08X} cus_en={} cus_y={} cus_h={} cus_v={} color_ctl=0x{:08X} color_alpha={} buf_cfg=0x{:08X}\n",
            label,
            pipe.name,
            slot,
            ((ctl & PLANE_CTL_ENABLE) != 0) as u8,
            decode_plane_format(ctl),
            decode_plane_tiling(ctl),
            decode_plane_rotation(ctl),
            ((ctl & PLANE_CTL_ORDER_RGBX) != 0) as u8,
            stride,
            decode_xy_x(pos),
            decode_xy_y(pos),
            decode_xy_x(size).saturating_add(1),
            decode_xy_y(size).saturating_add(1),
            surf,
            surf_live,
            cus_ctl,
            ((cus_ctl & PLANE_CUS_ENABLE) != 0) as u8,
            ((cus_ctl & PLANE_CUS_Y_PLANE) != 0) as u8,
            decode_plane_cus_phase(cus_ctl, PLANE_CUS_HPHASE_MASK, PLANE_CUS_HPHASE_SIGN_NEGATIVE),
            decode_plane_cus_phase(cus_ctl, PLANE_CUS_VPHASE_MASK, PLANE_CUS_VPHASE_SIGN_NEGATIVE),
            color_ctl,
            decode_plane_color_alpha(color_ctl),
            buf_cfg
        );
        crate::log!(
            "intel/display: plane-raw label={} pipe={} slot={} base=0x{:05X} ctl=0x{:08X} stride=0x{:08X} pos=0x{:08X} size=0x{:08X} key=0x{:08X}/0x{:08X}/0x{:08X} offset=0x{:08X} surf=0x{:08X} live=0x{:08X} aux=0x{:08X}/0x{:08X} cus=0x{:08X} color=0x{:08X} wm0=0x{:08X} wm_sagv=0x{:08X} wm_sagv_trans=0x{:08X} wm_trans=0x{:08X} buf=0x{:08X}\n",
            label,
            pipe.name,
            slot,
            plane_base,
            ctl,
            stride,
            pos,
            size,
            keyval,
            keymsk,
            keymax,
            offset,
            surf,
            surf_live,
            aux_dist,
            aux_offset,
            cus_ctl,
            color_ctl,
            wm0,
            wm_sagv,
            wm_sagv_trans,
            wm_trans,
            buf_cfg
        );
        slot += 1;
    }
}

fn log_display_power_well_snapshot(label: &str) {
    let Some(dev) = crate::intel::claimed_device() else {
        return;
    };

    let main_bios = crate::intel::mmio_read(dev, HSW_PWR_WELL_CTL1);
    let main_driver = crate::intel::mmio_read(dev, HSW_PWR_WELL_CTL2);
    let main_kvmr = crate::intel::mmio_read(dev, HSW_PWR_WELL_CTL3);
    let main_debug = crate::intel::mmio_read(dev, HSW_PWR_WELL_CTL4);
    let aux_bios = crate::intel::mmio_read(dev, ICL_PWR_WELL_CTL_AUX1);
    let aux_driver = crate::intel::mmio_read(dev, ICL_PWR_WELL_CTL_AUX2);
    let aux_debug = crate::intel::mmio_read(dev, ICL_PWR_WELL_CTL_AUX4);
    let ddi_bios = crate::intel::mmio_read(dev, ICL_PWR_WELL_CTL_DDI1);
    let ddi_driver = crate::intel::mmio_read(dev, ICL_PWR_WELL_CTL_DDI2);
    let ddi_debug = crate::intel::mmio_read(dev, ICL_PWR_WELL_CTL_DDI4);
    let fuse = crate::intel::mmio_read(dev, SKL_FUSE_STATUS);

    crate::log!(
        "intel/display: power-wells label={} main_ctl=[bios=0x{:08X},driver=0x{:08X},kvmr=0x{:08X},debug=0x{:08X}] main_state_pg1..5=[{},{},{},{},{}] main_req_pg1..5=[{},{},{},{},{}] aux_ctl=[bios=0x{:08X},driver=0x{:08X},debug=0x{:08X}] ddi_ctl=[bios=0x{:08X},driver=0x{:08X},debug=0x{:08X}] fuse=0x{:08X} fuse_pg1..5=[{},{},{},{},{}]\n",
        label,
        main_bios,
        main_driver,
        main_kvmr,
        main_debug,
        power_well_state_bit(main_driver, 0),
        power_well_state_bit(main_driver, 1),
        power_well_state_bit(main_driver, 2),
        power_well_state_bit(main_driver, 3),
        power_well_state_bit(main_driver, 4),
        power_well_request_bit(main_driver, 0),
        power_well_request_bit(main_driver, 1),
        power_well_request_bit(main_driver, 2),
        power_well_request_bit(main_driver, 3),
        power_well_request_bit(main_driver, 4),
        aux_bios,
        aux_driver,
        aux_debug,
        ddi_bios,
        ddi_driver,
        ddi_debug,
        fuse,
        fuse_pg_distribution_done(fuse, 1),
        fuse_pg_distribution_done(fuse, 2),
        fuse_pg_distribution_done(fuse, 3),
        fuse_pg_distribution_done(fuse, 4),
        fuse_pg_distribution_done(fuse, 5)
    );
}

#[inline]
fn power_well_state_bit(raw: u32, index: u32) -> u8 {
    ((raw >> index.saturating_mul(2)) & 0x1) as u8
}

#[inline]
fn power_well_request_bit(raw: u32, index: u32) -> u8 {
    ((raw >> index.saturating_mul(2).saturating_add(1)) & 0x1) as u8
}

#[inline]
fn fuse_pg_distribution_done(raw: u32, pg: u32) -> u8 {
    if pg > 27 {
        return 0;
    }
    ((raw >> (27 - pg)) & 0x1) as u8
}

fn log_primary_scanout_pte_window(
    dev: crate::intel::Dev,
    label: &str,
    base_gpu: u64,
    byte_len: usize,
) {
    let page_count = byte_len.div_ceil(crate::intel::WARM_ALIGN);
    let mut entries = [0u64; 4];
    let count = page_count.min(entries.len());
    let mut idx = 0usize;
    while idx < count {
        let gpu = base_gpu + (idx as u64) * crate::intel::WARM_ALIGN as u64;
        entries[idx] = crate::intel::read_ggtt_pte(dev, gpu).unwrap_or(0);
        idx += 1;
    }
    intel_display_verbose_log!(
        "intel/display: primary-ggtt label={} gpu=0x{:X} bytes=0x{:X} pages={} pte0=0x{:016X} pte1=0x{:016X} pte2=0x{:016X} pte3=0x{:016X}\n",
        label,
        base_gpu,
        byte_len,
        page_count,
        entries[0],
        entries[1],
        entries[2],
        entries[3]
    );
}

fn overlay_plane_base(pipe: PipeInfo, plane_slot: usize) -> usize {
    pipe.plane(plane_slot).base()
}

fn overlay_surface_pool(
    pipe: PipeInfo,
    plane_slot: usize,
) -> Option<&'static Mutex<OverlaySurfacePool>> {
    match plane_slot {
        1 => Some(&OVERLAY_SURFACES_SLOT_1[pipe.slot]),
        2 => Some(&OVERLAY_SURFACES_SLOT_2[pipe.slot]),
        3 => Some(&OVERLAY_SURFACES_SLOT_3[pipe.slot]),
        4 => Some(&OVERLAY_SURFACES_SLOT_4[pipe.slot]),
        _ => None,
    }
}

fn primary_swap_surface_pool(pipe: PipeInfo) -> &'static Mutex<PrimarySwapSurfacePool> {
    &PRIMARY_SWAP_SURFACES[pipe.slot]
}

fn overlay_surface_gpu_for_index(pipe: PipeInfo, plane_slot: usize, index: usize) -> Option<u64> {
    let plane_index = plane_slot.checked_sub(1)?;
    if plane_index >= OVERLAY_UNIVERSAL_PLANE_COUNT || index >= OVERLAY_SWAP_BUFFER_COUNT {
        return None;
    }
    if plane_slot == crate::ui4::INTERACTION_OVERLAY_PLANE_SLOT {
        return INTERACTION_OVERLAY_GPU_BASE
            .checked_add((pipe.slot as u64).checked_mul(OVERLAY_PIPE_GPU_STRIDE)?)?
            .checked_add((index as u64).checked_mul(OVERLAY_SWAP_GPU_STRIDE)?);
    }
    if plane_index >= DIRECT_RCS_OVERLAY_UNIVERSAL_PLANE_COUNT {
        return None;
    }
    OVERLAY_SWAP_GPU_BASE
        .checked_add((plane_index as u64).checked_mul(OVERLAY_PLANE_GPU_STRIDE)?)?
        .checked_add((pipe.slot as u64).checked_mul(OVERLAY_PIPE_GPU_STRIDE)?)?
        .checked_add((index as u64).checked_mul(OVERLAY_SWAP_GPU_STRIDE)?)
}

fn primary_swap_surface_gpu_for_index(pipe: PipeInfo, index: usize) -> Option<u64> {
    if index >= PRIMARY_SWAP_BUFFER_COUNT {
        return None;
    }
    PRIMARY_SWAP_GPU_BASE
        .checked_add((pipe.slot as u64).checked_mul(PRIMARY_SWAP_PIPE_GPU_STRIDE)?)?
        .checked_add((index as u64).checked_mul(PRIMARY_SWAP_GPU_STRIDE)?)
}

fn overlay_back_buffer_index(pool: OverlaySurfacePool) -> usize {
    pool.front_index
        .map(|front| (front + 1) % OVERLAY_SWAP_BUFFER_COUNT)
        .or_else(|| pool.surfaces.iter().position(Option::is_some))
        .unwrap_or(0)
}

fn primary_swap_back_buffer_index(pool: PrimarySwapSurfacePool) -> usize {
    pool.front_index
        .map(|front| (front + 1) % PRIMARY_SWAP_BUFFER_COUNT)
        .unwrap_or(0)
}

fn mark_overlay_surface_front(surface: OverlaySurface) {
    let Some(surface_pool) = overlay_surface_pool(surface.pipe, surface.plane_slot) else {
        return;
    };
    let retiring = {
        let mut pool = surface_pool.lock();
        if !pool.matches(surface.width, surface.height, surface.pipe) {
            return;
        }
        pool.front_index = Some(surface.buffer_index);
        pool.retiring_front.take()
    };
    if let Some(retiring) = retiring {
        let Some(dev) = crate::intel::claimed_device() else {
            surface_pool.lock().retiring_front = Some(retiring);
            return;
        };
        if !release_detached_overlay_surface(dev, retiring, "replacement-proven-live") {
            surface_pool.lock().retiring_front = Some(retiring);
        }
    }
}

fn mark_overlay_composition_surface_front(
    surface: OverlaySurface,
    change: CompositionDamageRegion,
) {
    // The plane commit helpers establish front ownership and retire any
    // previous-size surface. Keep that lifecycle centralized there, then add
    // the per-buffer composition history used by partial updates.
    mark_overlay_surface_front(surface);
    let Some(surface_pool) = overlay_surface_pool(surface.pipe, surface.plane_slot) else {
        return;
    };
    let mut pool = surface_pool.lock();
    if !pool.matches(surface.width, surface.height, surface.pipe) {
        return;
    }
    for index in 0..OVERLAY_SWAP_BUFFER_COUNT {
        if index == surface.buffer_index {
            pool.damage_debt[index] = CompositionDamageRegion::EMPTY;
        } else {
            pool.damage_debt[index].add_region(change);
        }
    }
}

fn release_detached_overlay_surface(
    dev: crate::intel::Dev,
    surface: OverlaySurface,
    reason: &str,
) -> bool {
    let render_unmapped =
        crate::intel::render::unmap_render_ppgtt_range(surface.gpu, surface.byte_len);
    let scanout_unmapped =
        crate::intel::unmap_display_scanout_ggtt(dev, surface.byte_len, surface.gpu);
    if !scanout_unmapped {
        crate::log_warn!(
            target: "intel/display";
            "intel/display: overlay-surface retire deferred reason={} pipeline={} buffer={} gpu=0x{:X} bytes=0x{:X} render_unmapped={} potential_reason=scanout-ggtt-unmap-failed\n",
            reason,
            DisplayPipelineId::from_pipe(surface.pipe)
                .map(DisplayPipelineId::name)
                .unwrap_or("pipe-invalid"),
            surface.buffer_index,
            surface.gpu,
            surface.byte_len,
            render_unmapped as u8,
        );
        return false;
    }
    crate::dma::dealloc(surface.virt, surface.byte_len);
    crate::log!(
        "intel/display: overlay-surface retired reason={} pipeline={} buffer={} gpu=0x{:X} bytes=0x{:X} render_unmapped={}\n",
        reason,
        DisplayPipelineId::from_pipe(surface.pipe)
            .map(DisplayPipelineId::name)
            .unwrap_or("pipe-invalid"),
        surface.buffer_index,
        surface.gpu,
        surface.byte_len,
        render_unmapped as u8,
    );
    true
}

fn mark_primary_swap_surface_front(surface: PrimarySwapSurface) {
    let mut pool = primary_swap_surface_pool(surface.pipe).lock();
    if pool.matches(surface.width, surface.height, surface.pipe) {
        pool.front_index = Some(surface.buffer_index);
        let full = CompositionDamageRegion::from_rect(CompositionDamageRect::new(
            0,
            0,
            surface.width,
            surface.height,
        ));
        for index in 0..PRIMARY_SWAP_BUFFER_COUNT {
            pool.damage_debt[index] = if index != surface.buffer_index {
                full
            } else {
                CompositionDamageRegion::EMPTY
            };
        }
    }
}

fn mark_primary_composition_surface_front(
    surface: PrimarySwapSurface,
    change: CompositionDamageRegion,
) {
    let mut pool = primary_swap_surface_pool(surface.pipe).lock();
    if !pool.matches(surface.width, surface.height, surface.pipe) {
        return;
    }
    pool.front_index = Some(surface.buffer_index);
    for index in 0..PRIMARY_SWAP_BUFFER_COUNT {
        if index == surface.buffer_index {
            pool.damage_debt[index] = CompositionDamageRegion::EMPTY;
        } else {
            pool.damage_debt[index].add_region(change);
        }
    }
}

fn copy_overlay_front_into_back(back: OverlaySurface) -> bool {
    let front = {
        let Some(surface_pool) = overlay_surface_pool(back.pipe, back.plane_slot) else {
            return false;
        };
        let pool = surface_pool.lock();
        if !pool.matches(back.width, back.height, back.pipe) {
            return false;
        }
        let Some(front_index) = pool.front_index else {
            return false;
        };
        if front_index == back.buffer_index {
            return false;
        }
        let Some(front) = pool.surfaces[front_index] else {
            return false;
        };
        front
    };
    if front.virt.is_null()
        || back.virt.is_null()
        || front.byte_len != back.byte_len
        || front.pitch_bytes != back.pitch_bytes
    {
        return false;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(front.virt, back.virt, back.byte_len);
    }
    true
}

fn copy_primary_swap_front_into_back(back: PrimarySwapSurface) -> bool {
    let front = {
        let pool = primary_swap_surface_pool(back.pipe).lock();
        if !pool.matches(back.width, back.height, back.pipe) {
            return false;
        }
        let Some(front_index) = pool.front_index else {
            return false;
        };
        if front_index == back.buffer_index {
            return false;
        }
        let Some(front) = pool.surfaces[front_index] else {
            return false;
        };
        front
    };
    if front.virt.is_null()
        || back.virt.is_null()
        || front.byte_len != back.byte_len
        || front.pitch_bytes != back.pitch_bytes
    {
        return false;
    }
    unsafe {
        core::ptr::copy_nonoverlapping(front.virt, back.virt, back.byte_len);
    }
    true
}

fn restore_primary_composition_base_rect(
    back: PrimarySwapSurface,
    damage: CompositionDamageRect,
) -> bool {
    let Some(primary) = primary_surface_for_pipe(back.pipe) else {
        return false;
    };
    if primary.virt.is_null()
        || back.virt.is_null()
        || primary.width != back.width
        || primary.height != back.height
        || primary.pitch_bytes < primary.width.saturating_mul(4)
        || back.pitch_bytes < back.width.saturating_mul(4)
    {
        return false;
    }
    let row_bytes = damage.width as usize * 4;
    for row in 0..damage.height as usize {
        let src_offset = (damage.y as usize + row)
            .saturating_mul(primary.pitch_bytes as usize)
            .saturating_add(damage.x as usize * 4);
        let dst_offset = (damage.y as usize + row)
            .saturating_mul(back.pitch_bytes as usize)
            .saturating_add(damage.x as usize * 4);
        unsafe {
            core::ptr::copy_nonoverlapping(
                primary.virt.add(src_offset),
                back.virt.add(dst_offset),
                row_bytes,
            );
        }
    }
    true
}

fn dma_flush_primary_swap_region(
    surface: PrimarySwapSurface,
    damage: CompositionDamageRegion,
) -> bool {
    dma_flush_surface_region(surface.virt, surface.byte_len, surface.pitch_bytes as usize, damage)
}

fn blend_premultiplied_rgba_tile_into_primary_clipped(
    surface: PrimarySwapSurface,
    tile: &RgbaOverlayTile<'_>,
    damage: CompositionDamageRect,
) -> bool {
    if surface.virt.is_null()
        || tile.width == 0
        || tile.height == 0
        || tile.source_width == 0
        || tile.source_height == 0
        || tile.pitch_bytes < tile.source_width as usize * 4
    {
        return false;
    }
    let Some(required) = tile
        .pitch_bytes
        .checked_mul(tile.source_height as usize - 1)
        .and_then(|bytes| bytes.checked_add(tile.source_width as usize * 4))
    else {
        return false;
    };
    if tile.pixels.len() < required {
        return false;
    }

    let tile_rect = CompositionDamageRect::new(tile.x, tile.y, tile.width, tile.height);
    let Some(draw) = intersect_composition_damage(tile_rect, damage)
        .and_then(|rect| clip_composition_damage(rect, surface.width, surface.height))
    else {
        return true;
    };
    let copy_w = draw.width as usize;
    let copy_h = draw.height as usize;
    let destination_x = draw.x.saturating_sub(tile.x);
    let destination_y = draw.y.saturating_sub(tile.y);

    for row in 0..copy_h {
        let source_y = tile_source_coordinate(
            u64::from(destination_y).saturating_add(row as u64),
            tile.source_height,
            tile.height,
        );
        let src_row = &tile.pixels[source_y * tile.pitch_bytes..];
        let dst_row = unsafe {
            surface
                .virt
                .add(
                    (draw.y as usize + row)
                        .saturating_mul(surface.pitch_bytes as usize)
                        .saturating_add(draw.x as usize * 4),
                )
                .cast::<u32>()
        };
        for col in 0..copy_w {
            let source_x = tile_source_coordinate(
                u64::from(destination_x).saturating_add(col as u64),
                tile.source_width,
                tile.width,
            );
            let offset = source_x * 4;
            let r = apply_tile_opacity(src_row[offset], tile.opacity);
            let g = apply_tile_opacity(src_row[offset + 1], tile.opacity);
            let b = apply_tile_opacity(src_row[offset + 2], tile.opacity);
            let a = apply_tile_opacity(src_row[offset + 3], tile.opacity);
            let dst = unsafe { core::ptr::read_volatile(dst_row.add(col)) }.to_le_bytes();
            let inverse_alpha = u16::from(u8::MAX - a);
            let blend = |src: u8, under: u8| -> u8 {
                let under = (u16::from(under) * inverse_alpha + 127) / 255;
                u16::from(src).saturating_add(under).min(255) as u8
            };
            let pixel =
                u32::from_le_bytes([blend(b, dst[0]), blend(g, dst[1]), blend(r, dst[2]), 0]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col), pixel);
            }
        }
    }
    true
}

fn live_rect_covers_surface(rect: LiveOverlayRect, surface: OverlaySurface) -> bool {
    rect.x == 0 && rect.y == 0 && rect.width >= surface.width && rect.height >= surface.height
}

fn init_default_overlay_marker(dev: crate::intel::Dev, primary: PrimarySurface) -> bool {
    if !DEFAULT_OVERLAY_MARKER_ENABLED {
        return false;
    }

    let Some(surface) =
        ensure_overlay_surface(dev, DEFAULT_OVERLAY_MARKER_SIZE, DEFAULT_OVERLAY_MARKER_SIZE)
    else {
        crate::log!(
            "intel/display: default-overlay-marker skipped pipe={} cause=no-surface\n",
            primary.pipe.name
        );
        return false;
    };
    fill_surface_color(
        surface.virt,
        surface.pitch_bytes as usize,
        surface.width,
        surface.height,
        DEFAULT_OVERLAY_MARKER_COLOR,
    );
    crate::intel::dma_flush(
        surface.virt,
        (surface.pitch_bytes as usize).saturating_mul(surface.height as usize),
    );

    let (scanout_w, scanout_h) =
        active_scanout_dimensions().unwrap_or((primary.width, primary.height));
    let pos_x = scanout_w.saturating_sub(surface.width) / 2;
    let pos_y = scanout_h.saturating_sub(surface.height) / 2;
    let reason = "default-overlay-marker";
    if !present_overlay_surface_with_bootstrap_contract(
        dev,
        surface,
        pos_x,
        pos_y,
        UI4_RGBA8_OVERLAY_CONTRACT,
        reason,
    ) {
        return false;
    }

    crate::log!(
        "intel/display: default-overlay-marker pipe={} slot={} pos={}x{} size={}x{} color=0x{:08X}\n",
        surface.pipe.name,
        surface.plane_slot,
        pos_x,
        pos_y,
        surface.width,
        surface.height,
        DEFAULT_OVERLAY_MARKER_COLOR
    );
    true
}

fn ensure_overlay_surface(
    dev: crate::intel::Dev,
    width: u32,
    height: u32,
) -> Option<OverlaySurface> {
    ensure_overlay_surface_on_slot(dev, OVERLAY_PLANE_SLOT, width, height)
}

fn ensure_overlay_surface_on_slot(
    dev: crate::intel::Dev,
    plane_slot: usize,
    width: u32,
    height: u32,
) -> Option<OverlaySurface> {
    let pipe = active_pipe(dev)?;
    ensure_overlay_surface_for_pipe(dev, pipe, plane_slot, width, height)
}

fn ensure_overlay_surface_for_pipe(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    plane_slot: usize,
    width: u32,
    height: u32,
) -> Option<OverlaySurface> {
    let surface_pool = overlay_surface_pool(pipe, plane_slot)?;
    let (buffer_index, resize_guard, stale_back) = {
        let mut pool = surface_pool.lock();
        if pool.matches(width, height, pipe) {
            let index = overlay_back_buffer_index(*pool);
            if let Some(surface) = pool.surfaces[index] {
                return Some(surface);
            }
            if pool
                .retiring_front
                .is_some_and(|retiring| retiring.buffer_index == index)
            {
                crate::log_warn!(
                    target: "intel/display";
                    "intel/display: overlay acquire deferred pipeline={} buffer={} potential_reason=previous-size-surface-retirement-pending\n",
                    DisplayPipelineId::from_pipe(pipe)?.name(),
                    index,
                );
                return None;
            }
            (index, None, None)
        } else {
            // Buffer GPU addresses are stable across surface-size changes.
            // Preserve the slot which the plane is currently scanning and
            // allocate the resized surface in the other slot. Reusing slot 0
            // unconditionally here can expose an incomplete first frame when
            // a small boot marker grows into a full-screen composition.
            let guarded_front = pool.front_index.and_then(|front_index| {
                pool.surfaces[front_index]
                    .map(|surface| (pool.width, pool.height, pool.pipe_slot, front_index, surface))
            });
            let index = guarded_front
                .map(|(_, _, _, front_index, _)| (front_index + 1) % OVERLAY_SWAP_BUFFER_COUNT)
                .unwrap_or(0);
            let stale_back = pool.surfaces[index].take();
            (index, guarded_front, stale_back)
        }
    };
    if let Some(stale_back) = stale_back {
        if !release_detached_overlay_surface(dev, stale_back, "resize-stale-back") {
            let mut pool = surface_pool.lock();
            if pool.surfaces[buffer_index].is_none() {
                pool.surfaces[buffer_index] = Some(stale_back);
            }
            return None;
        }
    }
    let gpu = overlay_surface_gpu_for_index(pipe, plane_slot, buffer_index)?;

    let pitch_bytes = aligned_pitch_bytes(width, PRIMARY_BYTES_PER_PIXEL)?;
    let byte_len = usize::try_from(u64::from(pitch_bytes) * u64::from(height)).ok()?;
    if byte_len as u64 > OVERLAY_SWAP_GPU_STRIDE {
        crate::log_warn!(
            target: "intel/display";
            "intel/display: overlay-surface rejected pipeline={} size={}x{} pitch=0x{:X} bytes=0x{:X} reserved_slot_bytes=0x{:X} potential_reason=mode-exceeds-per-pipeline-gpu-address-slot\n",
            DisplayPipelineId::from_pipe(pipe)?.name(),
            width,
            height,
            pitch_bytes,
            byte_len,
            OVERLAY_SWAP_GPU_STRIDE,
        );
        return None;
    }
    let (phys, virt) = match crate::dma::alloc(byte_len, crate::intel::WARM_ALIGN) {
        Some(allocation) => allocation,
        None => {
            let in_place_front = {
                let pool = surface_pool.lock();
                pool.matches(width, height, pipe)
                    .then(|| {
                        pool.front_index
                            .and_then(|front_index| pool.surfaces[front_index])
                    })
                    .flatten()
            };
            let Some(front) = in_place_front else {
                crate::log_warn!(
                    target: "intel/display";
                    "intel/display: overlay-surface allocation failed pipeline={} slot={} buffer={} size={}x{} bytes=0x{:X} fallback=unavailable\n",
                    DisplayPipelineId::from_pipe(pipe)?.name(),
                    plane_slot,
                    buffer_index,
                    width,
                    height,
                    byte_len,
                );
                return None;
            };
            let seq = OVERLAY_IN_PLACE_FALLBACK_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
            if seq <= 3 || seq.is_multiple_of(600) {
                crate::log_warn!(
                    target: "intel/display";
                    "intel/display: overlay double-buffer allocation unavailable seq={} pipeline={} slot={} requested_buffer={} live_buffer={} size={}x{} bytes=0x{:X} action=update-live-front-in-place retry=next-frame tearing_possible=1\n",
                    seq,
                    DisplayPipelineId::from_pipe(pipe)?.name(),
                    plane_slot,
                    buffer_index,
                    front.buffer_index,
                    width,
                    height,
                    byte_len,
                );
            }
            return Some(front);
        }
    };
    fill_surface_color(virt, pitch_bytes as usize, width, height, 0);
    crate::intel::dma_flush(virt, byte_len);

    if !crate::intel::map_display_scanout_ggtt(dev, phys, byte_len, gpu) {
        crate::log!(
            "intel/display: overlay-surface ggtt map failed pipe={} slot={} buffer={} size={}x{} bytes=0x{:X} gpu=0x{:X}\n",
            pipe.name,
            plane_slot,
            buffer_index,
            width,
            height,
            byte_len,
            gpu
        );
        let _ = crate::intel::unmap_display_scanout_ggtt(dev, byte_len, gpu);
        crate::dma::dealloc(virt, byte_len);
        return None;
    }
    crate::intel::ggtt_invalidate(dev);

    let surface = OverlaySurface {
        width,
        height,
        pitch_bytes,
        byte_len,
        phys,
        virt,
        gpu,
        pipe,
        plane_slot,
        buffer_index,
    };
    {
        let mut pool = surface_pool.lock();
        if !pool.matches(width, height, pipe) {
            *pool = OverlaySurfacePool {
                width,
                height,
                pipe_slot: pipe.slot,
                // The old surface metadata no longer matches, but its GPU
                // slot remains live until this resized back buffer commits.
                front_index: None,
                surfaces: [None; OVERLAY_SWAP_BUFFER_COUNT],
                damage_debt: [CompositionDamageRegion::EMPTY; OVERLAY_SWAP_BUFFER_COUNT],
                retiring_front: resize_guard.map(|(_, _, _, _, surface)| surface),
            };
        }
        pool.surfaces[buffer_index] = Some(surface);
    }
    if let Some((old_width, old_height, old_pipe_slot, front_index, _)) = resize_guard {
        crate::log_warn!(
            target: "intel/display";
            "intel/display: overlay resize guarded old={}x{} pipe_slot={} live_buffer={} new={}x{} staging_buffer={} potential_reason=avoid-overwriting-live-scanout-before-first-complete-frame\n",
            old_width,
            old_height,
            old_pipe_slot,
            front_index,
            width,
            height,
            buffer_index
        );
    }
    crate::log!(
        "intel/display: overlay-surface pipe={} slot={} buffer={} size={}x{} pitch=0x{:X} bytes=0x{:X} gpu=0x{:X} phys=0x{:X}\n",
        pipe.name,
        plane_slot,
        buffer_index,
        width,
        height,
        pitch_bytes,
        byte_len,
        gpu,
        phys
    );
    Some(surface)
}

fn ensure_primary_swap_surface_for_pipe(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    width: u32,
    height: u32,
) -> Option<PrimarySwapSurface> {
    let buffer_index = {
        let pool = primary_swap_surface_pool(pipe).lock();
        if pool.matches(width, height, pipe) {
            let index = primary_swap_back_buffer_index(*pool);
            if let Some(surface) = pool.surfaces[index] {
                return Some(surface);
            }
            index
        } else {
            0
        }
    };
    let gpu = primary_swap_surface_gpu_for_index(pipe, buffer_index)?;

    // Preserve the already-live primary stride when the mode matches. That
    // makes normal compositor presentation a SURF-only flip and avoids a
    // disable/re-arm transition between the guarded boot surface and UI4.
    let pitch_bytes = primary_surface_for_pipe(pipe)
        .filter(|primary| primary.width == width && primary.height == height)
        .map(|primary| primary.pitch_bytes)
        .unwrap_or(aligned_pitch_bytes(width, PRIMARY_BYTES_PER_PIXEL)?);
    let byte_len = usize::try_from(u64::from(pitch_bytes) * u64::from(height)).ok()?;
    if byte_len as u64 > PRIMARY_SWAP_GPU_STRIDE {
        crate::log_warn!(
            target: "intel/display";
            "intel/display: primary-swap-surface rejected pipeline={} size={}x{} pitch=0x{:X} bytes=0x{:X} reserved_slot_bytes=0x{:X} potential_reason=mode-exceeds-per-pipeline-gpu-address-slot\n",
            DisplayPipelineId::from_pipe(pipe)?.name(),
            width,
            height,
            pitch_bytes,
            byte_len,
            PRIMARY_SWAP_GPU_STRIDE,
        );
        return None;
    }
    let (phys, virt) = crate::dma::alloc(byte_len, crate::intel::WARM_ALIGN)?;
    fill_surface_color(virt, pitch_bytes as usize, width, height, 0);
    crate::intel::dma_flush(virt, byte_len);

    if !crate::intel::map_display_scanout_ggtt(dev, phys, byte_len, gpu) {
        crate::log!(
            "intel/display: primary-swap-surface ggtt map failed pipe={} buffer={} size={}x{} bytes=0x{:X} gpu=0x{:X}\n",
            pipe.name,
            buffer_index,
            width,
            height,
            byte_len,
            gpu
        );
        let _ = crate::intel::unmap_display_scanout_ggtt(dev, byte_len, gpu);
        crate::dma::dealloc(virt, byte_len);
        return None;
    }
    crate::intel::ggtt_invalidate(dev);

    let surface = PrimarySwapSurface {
        width,
        height,
        pitch_bytes,
        byte_len,
        phys,
        virt,
        gpu,
        pipe,
        buffer_index,
    };
    {
        let mut pool = primary_swap_surface_pool(pipe).lock();
        if !pool.matches(width, height, pipe) {
            let full =
                CompositionDamageRegion::from_rect(CompositionDamageRect::new(0, 0, width, height));
            *pool = PrimarySwapSurfacePool {
                width,
                height,
                pipe_slot: pipe.slot,
                front_index: None,
                surfaces: [None; PRIMARY_SWAP_BUFFER_COUNT],
                damage_debt: [full; PRIMARY_SWAP_BUFFER_COUNT],
            };
        }
        pool.surfaces[buffer_index] = Some(surface);
    }
    crate::log!(
        "intel/display: primary-swap-surface pipe={} buffer={} size={}x{} pitch=0x{:X} bytes=0x{:X} gpu=0x{:X} phys=0x{:X}\n",
        pipe.name,
        buffer_index,
        width,
        height,
        pitch_bytes,
        byte_len,
        gpu,
        phys
    );
    Some(surface)
}

fn copy_rgba_into_overlay(
    surface: OverlaySurface,
    src: &[u8],
    src_width: u32,
    src_height: u32,
    src_pitch_bytes: usize,
    dst_x: u32,
    dst_y: u32,
    preserve_alpha: bool,
) -> bool {
    let dst_pitch = surface.pitch_bytes as usize;
    if dst_x >= surface.width || dst_y >= surface.height {
        return false;
    }
    if src_pitch_bytes < src_width as usize * 4 || dst_pitch < src_width as usize * 4 {
        return false;
    }

    let copy_width = src_width.min(surface.width.saturating_sub(dst_x));
    let copy_height = src_height.min(surface.height.saturating_sub(dst_y));
    if copy_width == 0 || copy_height == 0 {
        return false;
    }

    for row_idx in 0..(copy_height as usize) {
        let src_row_off = row_idx.saturating_mul(src_pitch_bytes);
        let Some(src_row) = src.get(src_row_off..src_row_off + copy_width as usize * 4) else {
            return false;
        };
        let dst_row = unsafe {
            surface
                .virt
                .add(
                    (dst_y as usize + row_idx)
                        .saturating_mul(dst_pitch)
                        .saturating_add(dst_x as usize * 4),
                )
                .cast::<u32>()
        };
        for col_idx in 0..(copy_width as usize) {
            let src_off = col_idx.saturating_mul(4);
            let r = src_row[src_off];
            let g = src_row[src_off + 1];
            let b = src_row[src_off + 2];
            let a = if preserve_alpha {
                src_row[src_off + 3]
            } else {
                u8::MAX
            };
            let pixel = u32::from_le_bytes([premul_u8(r, a), premul_u8(g, a), premul_u8(b, a), a]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col_idx), pixel);
            }
        }
    }

    true
}

fn copy_rgba_tile_into_overlay(
    surface: OverlaySurface,
    tile: &RgbaOverlayTile<'_>,
) -> Option<(u64, u64, u64)> {
    if tile.width == 0
        || tile.height == 0
        || tile.source_width == 0
        || tile.source_height == 0
        || tile.pitch_bytes < tile.source_width as usize * 4
        || tile.pixels.len() < tile.pitch_bytes.saturating_mul(tile.source_height as usize)
    {
        return None;
    }
    let copy_w = tile.width.min(surface.width.saturating_sub(tile.x));
    let copy_h = tile.height.min(surface.height.saturating_sub(tile.y));
    if copy_w == 0 || copy_h == 0 {
        return None;
    }
    let dst_pitch = surface.pitch_bytes as usize;
    let mut contract_pixels = 0u64;
    let mut source_mismatches = 0u64;
    let mut storage_mismatches = 0u64;
    for row in 0..copy_h as usize {
        let source_y = tile_source_coordinate(row as u64, tile.source_height, tile.height);
        let src_row_off = source_y.saturating_mul(tile.pitch_bytes);
        let dst_row_off = (tile.y as usize + row)
            .saturating_mul(dst_pitch)
            .saturating_add(tile.x as usize * 4);
        let dst_row = unsafe { surface.virt.add(dst_row_off) as *mut u32 };
        for col in 0..copy_w as usize {
            let source_x = tile_source_coordinate(col as u64, tile.source_width, tile.width);
            let src_off = src_row_off.saturating_add(source_x.saturating_mul(4));
            let Some(pixel) = tile.pixels.get(src_off..src_off.saturating_add(4)) else {
                return None;
            };
            let (r, g, b, a) = (pixel[0], pixel[1], pixel[2], pixel[3]);
            if let Some(expected) = tile.expected_rgba
                && a != 0
            {
                contract_pixels = contract_pixels.saturating_add(1);
                if [r, g, b, a] != [expected.r, expected.g, expected.b, expected.a] {
                    source_mismatches = source_mismatches.saturating_add(1);
                }
            }
            let alpha = apply_tile_opacity(a, tile.opacity);
            let premultiplied = u32::from_le_bytes([
                premul_u8(r, alpha),
                premul_u8(g, alpha),
                premul_u8(b, alpha),
                alpha,
            ]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col), premultiplied);
                if core::ptr::read_volatile(dst_row.add(col)) != premultiplied {
                    storage_mismatches = storage_mismatches.saturating_add(1);
                }
            }
        }
    }
    Some((contract_pixels, source_mismatches, storage_mismatches))
}

/// Compose UI4 frame surfaces with the existing GuC-submitted SIMD16
/// sprite-quad worklist. The display-owned back buffer is both the render
/// destination and the next universal-plane surface, so no CPU pixel copy or
/// readback sits between producer completion and scanout.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum GpgpuCompositionResult {
    Unavailable,
    Complete,
    SubmittedIncomplete,
}

fn compose_premultiplied_rgba_tiles_into_primary_gpgpu(
    surface: PrimarySwapSurface,
    tiles: &[RgbaOverlayTile<'_>],
    damage: CompositionDamageRegion,
) -> GpgpuCompositionResult {
    if surface.byte_len as u64 > COMPOSE_RCS_GPU_ALIAS_BYTES
        || !crate::intel::gpgpu::sprite_quad_worklist_ready()
    {
        return GpgpuCompositionResult::Unavailable;
    }
    let Some(primary) = primary_surface_for_pipe(surface.pipe) else {
        return GpgpuCompositionResult::Unavailable;
    };
    if primary.width != surface.width
        || primary.height != surface.height
        || primary.pitch_bytes < primary.width.saturating_mul(4)
    {
        return GpgpuCompositionResult::Unavailable;
    }
    let Some(destination) = crate::intel::gpgpu::GpgpuRgba8Surface::new(
        surface.phys,
        PRIMARY_COMPOSE_RCS_GPU_ALIAS,
        surface.byte_len,
        surface.width,
        surface.height,
        surface.pitch_bytes,
    ) else {
        return GpgpuCompositionResult::Unavailable;
    };
    let Some(base) = crate::intel::gpgpu::GpgpuRgba8Surface::new(
        primary.phys,
        primary.gpu,
        primary.byte_len,
        primary.width,
        primary.height,
        primary.pitch_bytes,
    ) else {
        return GpgpuCompositionResult::Unavailable;
    };
    if gpgpu_ranges_overlap(destination.gpu, destination.bytes, base.gpu, base.bytes) {
        return GpgpuCompositionResult::Unavailable;
    }

    // The immutable boot/base primary is copied only over damage. Every later
    // run source-overs one producer surface onto that restored XRGB base. All
    // work stays in one ordered submission and the display keeps its separate
    // stable GGTT address for the same physical destination allocation.
    let base_descriptors = damage
        .rects()
        .iter()
        .copied()
        .map(|rect| {
            composition_quad_descriptor(
                rect,
                0,
                0,
                surface.width,
                surface.height,
                u8::MAX,
                crate::intel::gpgpu::SPRITE_QUAD_WORKLIST_FLAG_SOURCE_XRGB
                    | crate::intel::gpgpu::SPRITE_QUAD_WORKLIST_FLAG_DEST_XRGB,
            )
        })
        .collect::<Vec<_>>();
    if base_descriptors.is_empty() {
        return GpgpuCompositionResult::Complete;
    }

    let mut descriptors = Vec::with_capacity(tiles.len());
    for tile in tiles {
        let Some(source) = tile.gpgpu_surface else {
            return GpgpuCompositionResult::Unavailable;
        };
        if !source.is_valid()
            || source.width != tile.source_width
            || source.height != tile.source_height
            || source.pitch_bytes as usize != tile.pitch_bytes
            || tile.width == 0
            || tile.height == 0
            || gpgpu_ranges_overlap(destination.gpu, destination.bytes, source.gpu, source.bytes)
        {
            return GpgpuCompositionResult::Unavailable;
        }
        let tile_rect = CompositionDamageRect::new(tile.x, tile.y, tile.width, tile.height);
        let mut tile_descriptors = Vec::new();
        for damaged in damage.rects() {
            let Some(draw) = intersect_composition_damage(tile_rect, *damaged)
                .and_then(|rect| clip_composition_damage(rect, surface.width, surface.height))
            else {
                continue;
            };
            tile_descriptors.push(composition_quad_descriptor(
                draw,
                tile.x,
                tile.y,
                tile.width,
                tile.height,
                tile.opacity,
                crate::intel::gpgpu::SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER
                    | crate::intel::gpgpu::SPRITE_QUAD_WORKLIST_FLAG_PREMUL_SRC
                    | crate::intel::gpgpu::SPRITE_QUAD_WORKLIST_FLAG_DEST_XRGB,
            ));
        }
        descriptors.push((source, tile_descriptors));
    }

    let expected_descriptors = base_descriptors.len().saturating_add(
        descriptors
            .iter()
            .map(|(_, descriptors)| descriptors.len())
            .sum::<usize>(),
    );
    if expected_descriptors > crate::intel::gpgpu::sprite_quad_worklist_max_descs() {
        return GpgpuCompositionResult::Unavailable;
    }
    let mut runs = Vec::with_capacity(descriptors.len().saturating_add(1));
    runs.push(crate::intel::gpgpu::GpgpuSpriteQuadWorklistRun {
        src: base,
        descs: &base_descriptors,
    });
    runs.extend(
        descriptors
            .iter()
            .filter(|(_, descriptors)| !descriptors.is_empty())
            .map(|(src, descriptors)| crate::intel::gpgpu::GpgpuSpriteQuadWorklistRun {
                src: *src,
                descs: descriptors,
            }),
    );
    let result =
        crate::intel::gpgpu::sprite_quad_worklist_rgba8_runs_over_result(destination, &runs);
    match result.outcome {
        crate::intel::gpgpu::GpgpuSubmissionOutcome::Complete
            if result.stats.descs == expected_descriptors && result.stats.submits == 1 =>
        {
            GpgpuCompositionResult::Complete
        }
        crate::intel::gpgpu::GpgpuSubmissionOutcome::SubmittedIncomplete => {
            GpgpuCompositionResult::SubmittedIncomplete
        }
        _ => GpgpuCompositionResult::Unavailable,
    }
}

fn compose_premultiplied_rgba_tiles_into_overlay_gpgpu(
    surface: OverlaySurface,
    tiles: &[RgbaOverlayTile<'_>],
    damage: CompositionDamageRegion,
) -> GpgpuCompositionResult {
    if surface.byte_len as u64 > COMPOSE_RCS_GPU_ALIAS_BYTES
        || !crate::intel::gpgpu::sprite_quad_worklist_ready()
    {
        return GpgpuCompositionResult::Unavailable;
    }
    let Some(destination) = crate::intel::gpgpu::GpgpuRgba8Surface::new(
        surface.phys,
        OVERLAY_COMPOSE_RCS_GPU_ALIAS,
        surface.byte_len,
        surface.width,
        surface.height,
        surface.pitch_bytes,
    ) else {
        return GpgpuCompositionResult::Unavailable;
    };

    // Clear and composition live in one ordered batch. This avoids a second
    // submit/poll and makes the submission boundary unambiguous for fallback.
    let clear_descriptors = damage
        .rects()
        .iter()
        .copied()
        .map(|rect| {
            composition_quad_descriptor(
                rect,
                0,
                0,
                surface.width,
                surface.height,
                u8::MAX,
                crate::intel::gpgpu::SPRITE_QUAD_WORKLIST_FLAG_CLEAR,
            )
        })
        .collect::<Vec<_>>();
    if clear_descriptors.is_empty() {
        return GpgpuCompositionResult::Complete;
    }

    let mut descriptors = Vec::with_capacity(tiles.len());
    for tile in tiles {
        let Some(source) = tile.gpgpu_surface else {
            return GpgpuCompositionResult::Unavailable;
        };
        if !source.is_valid()
            || source.width != tile.source_width
            || source.height != tile.source_height
            || source.pitch_bytes as usize != tile.pitch_bytes
            || tile.width == 0
            || tile.height == 0
            || gpgpu_ranges_overlap(destination.gpu, destination.bytes, source.gpu, source.bytes)
        {
            return GpgpuCompositionResult::Unavailable;
        }

        let tile_rect = CompositionDamageRect::new(tile.x, tile.y, tile.width, tile.height);
        let mut tile_descriptors = Vec::new();
        for damaged in damage.rects() {
            let Some(draw) = intersect_composition_damage(tile_rect, *damaged)
                .and_then(|rect| clip_composition_damage(rect, surface.width, surface.height))
            else {
                continue;
            };
            tile_descriptors.push(composition_quad_descriptor(
                draw,
                tile.x,
                tile.y,
                tile.width,
                tile.height,
                tile.opacity,
                crate::intel::gpgpu::SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER
                    | crate::intel::gpgpu::SPRITE_QUAD_WORKLIST_FLAG_PREMUL_SRC,
            ));
        }
        descriptors.push((source, tile_descriptors));
    }

    let expected_descriptors = clear_descriptors.len().saturating_add(
        descriptors
            .iter()
            .map(|(_, descriptors)| descriptors.len())
            .sum::<usize>(),
    );
    if expected_descriptors > crate::intel::gpgpu::sprite_quad_worklist_max_descs() {
        return GpgpuCompositionResult::Unavailable;
    }
    let mut runs = Vec::with_capacity(descriptors.len().saturating_add(1));
    runs.push(crate::intel::gpgpu::GpgpuSpriteQuadWorklistRun {
        // CLEAR descriptors never sample this binding; using the destination
        // keeps the run fully valid without another allocation or VA.
        src: destination,
        descs: &clear_descriptors,
    });
    runs.extend(
        descriptors
            .iter()
            .filter(|(_, descriptors)| !descriptors.is_empty())
            .map(|(src, descriptors)| crate::intel::gpgpu::GpgpuSpriteQuadWorklistRun {
                src: *src,
                descs: descriptors,
            }),
    );
    let result =
        crate::intel::gpgpu::sprite_quad_worklist_rgba8_runs_over_result(destination, &runs);
    match result.outcome {
        crate::intel::gpgpu::GpgpuSubmissionOutcome::Complete
            if result.stats.descs == expected_descriptors && result.stats.submits == 1 =>
        {
            GpgpuCompositionResult::Complete
        }
        crate::intel::gpgpu::GpgpuSubmissionOutcome::SubmittedIncomplete => {
            GpgpuCompositionResult::SubmittedIncomplete
        }
        _ => GpgpuCompositionResult::Unavailable,
    }
}

fn composition_quad_descriptor(
    draw: CompositionDamageRect,
    source_origin_x: u32,
    source_origin_y: u32,
    destination_extent_width: u32,
    destination_extent_height: u32,
    opacity: u8,
    flags: u32,
) -> crate::intel::gpgpu::GpgpuSpriteQuadWorklistDesc {
    let left = draw.x as f32;
    let top = draw.y as f32;
    let right = draw.x.saturating_add(draw.width) as f32;
    let bottom = draw.y.saturating_add(draw.height) as f32;
    // The kernel evaluates UV at destination pixel centers. Offset by half a
    // destination pixel so nearest sampling exactly matches the CPU fallback:
    // floor(destination_coordinate * source_extent / destination_extent).
    let source_x = draw.x.saturating_sub(source_origin_x) as f32;
    let source_y = draw.y.saturating_sub(source_origin_y) as f32;
    let u0 = (source_x - 0.5) / destination_extent_width as f32;
    let v0 = (source_y - 0.5) / destination_extent_height as f32;
    let u1 = (source_x + draw.width as f32 - 0.5) / destination_extent_width as f32;
    let v1 = (source_y + draw.height as f32 - 0.5) / destination_extent_height as f32;
    crate::intel::gpgpu::GpgpuSpriteQuadWorklistDesc {
        c0_x: left,
        c0_y: top,
        c0_u: u0,
        c0_v: v0,
        c1_x: right,
        c1_y: top,
        c1_u: u1,
        c1_v: v0,
        c2_x: right,
        c2_y: bottom,
        c2_u: u1,
        c2_v: v1,
        c3_x: left,
        c3_y: bottom,
        c3_u: u0,
        c3_v: v1,
        color_rgba: u32::from_le_bytes([opacity, opacity, opacity, opacity]),
        flags,
    }
}

fn gpgpu_ranges_overlap(
    left_gpu: u64,
    left_bytes: usize,
    right_gpu: u64,
    right_bytes: usize,
) -> bool {
    let left_end = left_gpu.saturating_add(left_bytes as u64);
    let right_end = right_gpu.saturating_add(right_bytes as u64);
    left_gpu < right_end && right_gpu < left_end
}

fn copy_premultiplied_rgba_tile_into_overlay_clipped(
    surface: OverlaySurface,
    tile: &RgbaOverlayTile<'_>,
    clip: CompositionDamageRect,
) -> Option<(u64, u64, u64)> {
    if surface.virt.is_null()
        || tile.width == 0
        || tile.height == 0
        || tile.source_width == 0
        || tile.source_height == 0
        || tile.pitch_bytes < tile.source_width as usize * 4
    {
        return None;
    }
    let required = tile
        .pitch_bytes
        .checked_mul(tile.source_height as usize - 1)?
        .checked_add(tile.source_width as usize * 4)?;
    if tile.pixels.len() < required {
        return None;
    }
    let tile_rect = CompositionDamageRect::new(tile.x, tile.y, tile.width, tile.height);
    let Some(draw) = intersect_composition_damage(tile_rect, clip)
        .and_then(|rect| clip_composition_damage(rect, surface.width, surface.height))
    else {
        return Some((0, 0, 0));
    };
    let destination_x = draw.x.saturating_sub(tile.x);
    let destination_y = draw.y.saturating_sub(tile.y);
    let dst_pitch = surface.pitch_bytes as usize;
    let mut contract_pixels = 0u64;
    let mut source_mismatches = 0u64;
    let mut storage_mismatches = 0u64;
    for row in 0..draw.height as usize {
        let source_y = tile_source_coordinate(
            u64::from(destination_y).saturating_add(row as u64),
            tile.source_height,
            tile.height,
        );
        let src_row_off = source_y.saturating_mul(tile.pitch_bytes);
        let dst_row_off = (draw.y as usize + row)
            .saturating_mul(dst_pitch)
            .saturating_add(draw.x as usize * 4);
        let dst_row = unsafe { surface.virt.add(dst_row_off).cast::<u32>() };
        for col in 0..draw.width as usize {
            let source_x = tile_source_coordinate(
                u64::from(destination_x).saturating_add(col as u64),
                tile.source_width,
                tile.width,
            );
            let src_off = src_row_off.saturating_add(source_x.saturating_mul(4));
            let pixel = tile.pixels.get(src_off..src_off.saturating_add(4))?;
            let (r, g, b, a) = (pixel[0], pixel[1], pixel[2], pixel[3]);
            if let Some(expected) = tile.expected_rgba
                && a != 0
            {
                contract_pixels = contract_pixels.saturating_add(1);
                if [r, g, b, a] != [expected.r, expected.g, expected.b, expected.a] {
                    source_mismatches = source_mismatches.saturating_add(1);
                }
            }
            // UI4 producers and plane storage are both premultiplied RGBA.
            // Apply only the independent window opacity.
            let source_r = apply_tile_opacity(r, tile.opacity);
            let source_g = apply_tile_opacity(g, tile.opacity);
            let source_b = apply_tile_opacity(b, tile.opacity);
            let source_a = apply_tile_opacity(a, tile.opacity);
            let destination = unsafe { core::ptr::read_volatile(dst_row.add(col)) }.to_le_bytes();
            let inverse_alpha = u16::from(u8::MAX - source_a);
            let blend = |source: u8, under: u8| -> u8 {
                let under = (u16::from(under) * inverse_alpha + 127) / 255;
                u16::from(source).saturating_add(under).min(255) as u8
            };
            let rgba = u32::from_le_bytes([
                blend(source_r, destination[0]),
                blend(source_g, destination[1]),
                blend(source_b, destination[2]),
                blend(source_a, destination[3]),
            ]);
            unsafe {
                core::ptr::write_volatile(dst_row.add(col), rgba);
                if core::ptr::read_volatile(dst_row.add(col)) != rgba {
                    storage_mismatches = storage_mismatches.saturating_add(1);
                }
            }
        }
    }
    Some((contract_pixels, source_mismatches, storage_mismatches))
}

#[inline]
fn tile_source_coordinate(destination: u64, source_extent: u32, destination_extent: u32) -> usize {
    if source_extent == destination_extent {
        return destination.min(u64::from(source_extent.saturating_sub(1))) as usize;
    }
    (destination
        .saturating_mul(u64::from(source_extent))
        .checked_div(u64::from(destination_extent))
        .unwrap_or(0))
    .min(u64::from(source_extent.saturating_sub(1))) as usize
}

#[inline]
fn apply_tile_opacity(channel: u8, opacity: u8) -> u8 {
    if opacity == u8::MAX {
        channel
    } else {
        premul_u8(channel, opacity)
    }
}

#[inline]
fn premul_u8(color: u8, alpha: u8) -> u8 {
    (((color as u16) * (alpha as u16) + 127) / 255) as u8
}

fn stamp_overlay_composition_proof_marker(
    surface: OverlaySurface,
    alpha: OverlayAlphaMode,
    reason: &str,
) -> bool {
    if !OVERLAY_COMPOSITION_PROOF_MARKER_ENABLED || alpha != UI4_RGBA8_OVERLAY_CONTRACT {
        return false;
    }

    let size = OVERLAY_COMPOSITION_PROOF_MARKER_SIZE;
    let gap = OVERLAY_COMPOSITION_PROOF_MARKER_GAP;
    let x0 = OVERLAY_COMPOSITION_PROOF_MARKER_X;
    let y0 = OVERLAY_COMPOSITION_PROOF_MARKER_Y;
    let x1 = x0.saturating_add(size).saturating_add(gap);
    let x2 = x1.saturating_add(size).saturating_add(gap);
    if x2.saturating_add(size) > surface.width || y0.saturating_add(size) > surface.height {
        crate::log!(
            "intel/display: overlay-proof skipped reason={} cause=surface-too-small size={}x{} marker={}x{}@{},{}\n",
            reason,
            surface.width,
            surface.height,
            size.saturating_mul(3).saturating_add(gap.saturating_mul(2)),
            size,
            x0,
            y0
        );
        return false;
    }

    let transparent = overlay_scanout_pixel_rgba_premul(0xFF, 0x00, 0xFF, 0x00);
    let half_red = overlay_scanout_pixel_rgba_premul(0xFF, 0x00, 0x00, 0x80);
    let opaque_green = overlay_scanout_pixel_rgba_premul(0x00, 0xFF, 0x00, 0xFF);
    fill_overlay_rect(surface, x0, y0, size, size, transparent);
    fill_overlay_rect(surface, x1, y0, size, size, half_red);
    fill_overlay_rect(surface, x2, y0, size, size, opaque_green);

    let cy = y0.saturating_add(size / 2);
    let transparent_cx = x0.saturating_add(size / 2);
    let half_red_cx = x1.saturating_add(size / 2);
    let opaque_green_cx = x2.saturating_add(size / 2);
    let overlay_transparent = sample_overlay_surface_pixel(surface, transparent_cx, cy);
    let overlay_half_red = sample_overlay_surface_pixel(surface, half_red_cx, cy);
    let overlay_opaque_green = sample_overlay_surface_pixel(surface, opaque_green_cx, cy);
    let primary_transparent = sample_primary_surface_pixel(transparent_cx, cy).unwrap_or_default();
    let primary_half_red = sample_primary_surface_pixel(half_red_cx, cy).unwrap_or_default();
    let primary_opaque_green =
        sample_primary_surface_pixel(opaque_green_cx, cy).unwrap_or_default();

    crate::log!(
        "intel/display: overlay-proof reason={} pipe={} slot={} badge={}x{}@{},{} cells=transparent,half-red,opaque-green overlay=[0x{:08X},0x{:08X},0x{:08X}] primary_under=[0x{:08X},0x{:08X},0x{:08X}] expectation=alpha-ok:underlay/red-blend/green alpha-ignored:black/dark-red/green\n",
        reason,
        surface.pipe.name,
        surface.plane_slot,
        size.saturating_mul(3).saturating_add(gap.saturating_mul(2)),
        size,
        x0,
        y0,
        overlay_transparent,
        overlay_half_red,
        overlay_opaque_green,
        primary_transparent,
        primary_half_red,
        primary_opaque_green
    );
    true
}

#[inline]
fn overlay_scanout_pixel_rgba_premul(r: u8, g: u8, b: u8, a: u8) -> u32 {
    u32::from_le_bytes([premul_u8(r, a), premul_u8(g, a), premul_u8(b, a), a])
}

fn fill_overlay_rect(surface: OverlaySurface, x: u32, y: u32, width: u32, height: u32, pixel: u32) {
    if surface.virt.is_null() || surface.pitch_bytes < surface.width.saturating_mul(4) {
        return;
    }
    let x0 = x.min(surface.width);
    let y0 = y.min(surface.height);
    let x1 = x0.saturating_add(width).min(surface.width);
    let y1 = y0.saturating_add(height).min(surface.height);
    let pitch_pixels = (surface.pitch_bytes as usize) / 4;
    for row_idx in y0 as usize..y1 as usize {
        let row = unsafe { (surface.virt as *mut u32).add(row_idx.saturating_mul(pitch_pixels)) };
        for col_idx in x0 as usize..x1 as usize {
            unsafe {
                core::ptr::write_volatile(row.add(col_idx), pixel);
            }
        }
    }
}

fn fill_overlay_rect_rgba(surface: OverlaySurface, rect: LiveOverlayRect) {
    if rect.width == 0 || rect.height == 0 || rect.color.a == 0 {
        return;
    }
    fill_overlay_rect(
        surface,
        rect.x,
        rect.y,
        rect.width,
        rect.height,
        overlay_scanout_pixel_rgba_premul(rect.color.r, rect.color.g, rect.color.b, rect.color.a),
    );
}

fn fill_overlay_rect_rgba_clipped(
    surface: OverlaySurface,
    rect: LiveOverlayRect,
    clip: CompositionDamageRect,
) {
    if rect.width == 0 || rect.height == 0 || rect.color.a == 0 {
        return;
    }
    let rect_damage = CompositionDamageRect::new(rect.x, rect.y, rect.width, rect.height);
    let Some(draw) = intersect_composition_damage(rect_damage, clip) else {
        return;
    };
    fill_overlay_rect(
        surface,
        draw.x,
        draw.y,
        draw.width,
        draw.height,
        overlay_scanout_pixel_rgba_premul(rect.color.r, rect.color.g, rect.color.b, rect.color.a),
    );
}

fn dma_flush_overlay_region(surface: OverlaySurface, damage: CompositionDamageRegion) -> bool {
    dma_flush_surface_region(surface.virt, surface.byte_len, surface.pitch_bytes as usize, damage)
}

fn dma_flush_surface_region(
    virt: *mut u8,
    byte_len: usize,
    pitch_bytes: usize,
    damage: CompositionDamageRegion,
) -> bool {
    const DAMAGE_FLUSH_SPAN_CAPACITY: usize = 16;

    let rects = damage.rects();
    if rects.len() > DAMAGE_FLUSH_SPAN_CAPACITY {
        return false;
    }
    let mut spans = [crate::intel::DmaFlushRows::EMPTY; DAMAGE_FLUSH_SPAN_CAPACITY];
    for (index, rect) in rects.iter().copied().enumerate() {
        let Some(span) = dma_flush_surface_rect_span(virt, byte_len, pitch_bytes, rect) else {
            return false;
        };
        spans[index] = span;
    }
    crate::intel::dma_flush_strided_row_spans(&spans[..rects.len()])
}

fn dma_flush_surface_rect_span(
    virt: *mut u8,
    byte_len: usize,
    pitch_bytes: usize,
    rect: CompositionDamageRect,
) -> Option<crate::intel::DmaFlushRows> {
    if rect.width == 0 || rect.height == 0 {
        return Some(crate::intel::DmaFlushRows::EMPTY);
    }
    if virt.is_null() {
        return None;
    }
    let bytes_per_pixel = PRIMARY_BYTES_PER_PIXEL as usize;
    let x_bytes = (rect.x as usize).checked_mul(bytes_per_pixel)?;
    let row_bytes = (rect.width as usize).checked_mul(bytes_per_pixel)?;
    let start = (rect.y as usize)
        .checked_mul(pitch_bytes)
        .and_then(|offset| offset.checked_add(x_bytes))?;
    let rows = rect.height as usize;
    let end = pitch_bytes
        .checked_mul(rows.saturating_sub(1))
        .and_then(|span| start.checked_add(span))
        .and_then(|last_row| last_row.checked_add(row_bytes))?;
    if end > byte_len {
        return None;
    }
    Some(crate::intel::DmaFlushRows::new(unsafe { virt.add(start) }, row_bytes, pitch_bytes, rows))
}

fn clip_composition_damage(
    rect: CompositionDamageRect,
    width: u32,
    height: u32,
) -> Option<CompositionDamageRect> {
    intersect_composition_damage(rect, CompositionDamageRect::new(0, 0, width, height))
}

fn clip_composition_damage_region(
    region: CompositionDamageRegion,
    width: u32,
    height: u32,
) -> CompositionDamageRegion {
    region.clipped(CompositionDamageRect::new(0, 0, width, height))
}

fn intersect_composition_damage(
    a: CompositionDamageRect,
    b: CompositionDamageRect,
) -> Option<CompositionDamageRect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = a.x.saturating_add(a.width).min(b.x.saturating_add(b.width));
    let bottom =
        a.y.saturating_add(a.height)
            .min(b.y.saturating_add(b.height));
    (right > x && bottom > y).then(|| CompositionDamageRect::new(x, y, right - x, bottom - y))
}

fn sample_overlay_surface_pixel(surface: OverlaySurface, x: u32, y: u32) -> u32 {
    if surface.virt.is_null()
        || x >= surface.width
        || y >= surface.height
        || surface.pitch_bytes < surface.width.saturating_mul(4)
    {
        return 0;
    }
    let pitch_pixels = (surface.pitch_bytes as usize) / 4;
    unsafe {
        core::ptr::read_volatile(
            (surface.virt as *const u32).add(
                (y as usize)
                    .saturating_mul(pitch_pixels)
                    .saturating_add(x as usize),
            ),
        )
    }
}

fn overlay_plane_needs_rearm(
    dev: crate::intel::Dev,
    surface: OverlaySurface,
    pos_x: u32,
    pos_y: u32,
    alpha: OverlayAlphaMode,
) -> bool {
    let plane_base = overlay_plane_base(surface.pipe, surface.plane_slot);
    let want_pos = plane_pos_reg_value(pos_x, pos_y);
    let want_size = plane_size_reg_value(surface.width, surface.height);
    let want_surf = u32::try_from(surface.gpu).unwrap_or(0);
    let ctl = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_CTL_OFF);
    let pos = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_POS_OFF);
    let size = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SIZE_OFF);
    let surf = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF);
    let surf_live = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF);
    let color_ctl = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_COLOR_CTL_OFF);
    let want_color_ctl = plane_color_ctl_alpha(color_ctl, alpha);

    let want_ctl = overlay_plane_ctl_enabled(ctl, alpha);
    (ctl & PLANE_CTL_ENABLE) == 0
        || (ctl & PLANE_CTL_ORDER_RGBX) != (want_ctl & PLANE_CTL_ORDER_RGBX)
        || pos != want_pos
        || size != want_size
        || surf != want_surf
        || surf_live != want_surf
        || color_ctl != want_color_ctl
}

fn overlay_plane_surface_flip_guard(
    dev: crate::intel::Dev,
    surface: OverlaySurface,
    pos_x: u32,
    pos_y: u32,
    alpha: OverlayAlphaMode,
) -> Result<(), &'static str> {
    let front_reg = {
        let Some(surface_pool) = overlay_surface_pool(surface.pipe, surface.plane_slot) else {
            return Err("surface-plane-slot");
        };
        let pool = surface_pool.lock();
        if !pool.matches(surface.width, surface.height, surface.pipe) {
            return Err("surface-pool-shape");
        }
        let front = pool
            .front_index
            .and_then(|front_index| pool.surfaces[front_index])
            .or(pool.retiring_front)
            .ok_or("no-complete-front")?;
        u32::try_from(front.gpu).map_err(|_| "front-address-range")?
    };
    let Some(stride_reg) = plane_stride_reg_value(surface.pitch_bytes) else {
        return Err("stride-range");
    };
    let plane_base = overlay_plane_base(surface.pipe, surface.plane_slot);
    let ctl = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_CTL_OFF);
    let color_ctl = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_COLOR_CTL_OFF);
    let resources = [
        (overlay_plane_base(surface.pipe, 0), PLANE_DBUF_SLOT_0_START, PLANE_DBUF_SLOT_0_END),
        (
            overlay_plane_base(surface.pipe, UI_OVERLAY_PLANE_SLOT),
            PLANE_DBUF_SLOT_1_START,
            PLANE_DBUF_SLOT_1_END,
        ),
        (
            overlay_plane_base(surface.pipe, VIDEO_NV12_PLANE_SLOT),
            PLANE_DBUF_SLOT_2_START,
            PLANE_DBUF_SLOT_2_END,
        ),
        (
            overlay_plane_base(surface.pipe, VIDEO_NV12_Y_PLANE_SLOT),
            PLANE_DBUF_SLOT_3_START,
            PLANE_DBUF_SLOT_3_END,
        ),
        (
            overlay_plane_base(surface.pipe, crate::ui4::INTERACTION_OVERLAY_PLANE_SLOT),
            PLANE_DBUF_SLOT_4_START,
            PLANE_DBUF_SLOT_4_END,
        ),
    ];
    if !resources.iter().all(|(base, start, end)| {
        plane_watermarks_are_boot_safe(dev, *base)
            && crate::intel::mmio_read(dev, *base + UNI_PLANE_BUF_CFG_OFF)
                == plane_buf_cfg_value(*start, *end)
    }) {
        return Err("dbuf-or-watermark-state");
    }
    if ctl != overlay_plane_ctl_enabled(ctl, alpha) {
        return Err("plane-control");
    }
    if color_ctl != plane_color_ctl_alpha(color_ctl, alpha) {
        return Err("plane-color-alpha");
    }
    if crate::intel::mmio_read(dev, plane_base + UNI_PLANE_STRIDE_OFF) != stride_reg {
        return Err("plane-stride");
    }
    if crate::intel::mmio_read(dev, plane_base + UNI_PLANE_POS_OFF)
        != plane_pos_reg_value(pos_x, pos_y)
    {
        return Err("plane-position");
    }
    if crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SIZE_OFF)
        != plane_size_reg_value(surface.width, surface.height)
    {
        return Err("plane-size");
    }
    if crate::intel::mmio_read(dev, plane_base + UNI_PLANE_OFFSET_OFF) != 0 {
        return Err("plane-offset");
    }
    if crate::intel::mmio_read(dev, plane_base + UNI_PLANE_KEYVAL_OFF) != 0
        || crate::intel::mmio_read(dev, plane_base + UNI_PLANE_KEYMSK_OFF) != 0
        || crate::intel::mmio_read(dev, plane_base + UNI_PLANE_KEYMAX_OFF) != 0xFF00_0000
    {
        return Err("plane-color-key");
    }
    if crate::intel::mmio_read(dev, plane_base + UNI_PLANE_AUX_DIST_OFF) != 0
        || crate::intel::mmio_read(dev, plane_base + UNI_PLANE_AUX_OFFSET_OFF) != 0
    {
        return Err("plane-aux-surface");
    }
    if crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF) != front_reg
        || crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF) != front_reg
    {
        return Err("front-ownership");
    }
    Ok(())
}

/// Fast path for a stable plane contract. Only the surface address changes;
/// DBUF, watermarks, format, alpha, stride and geometry remain untouched.
fn flip_overlay_plane_surface(
    dev: crate::intel::Dev,
    surface: OverlaySurface,
    pos_x: u32,
    pos_y: u32,
    alpha: OverlayAlphaMode,
    reason: &str,
) -> bool {
    if let Err(cause) = overlay_plane_surface_flip_guard(dev, surface, pos_x, pos_y, alpha) {
        intel_display_verbose_log!(
            "intel/display: overlay-surf-only rejected reason={} pipe={} buffer={} cause={}\n",
            reason,
            surface.pipe.name,
            surface.buffer_index,
            cause,
        );
        return false;
    }
    let plane_base = overlay_plane_base(surface.pipe, surface.plane_slot);
    let Some(surface_reg) = u32::try_from(surface.gpu).ok() else {
        return false;
    };
    match queue_ui4_plane_surface_flip(plane_base, surface_reg, reason) {
        PlaneSurfaceFlipQueueResult::Queued => {
            mark_overlay_surface_front(surface);
            let seq = OVERLAY_PRESENT_SEQ.load(Ordering::Relaxed).wrapping_add(1);
            if seq <= 8 || seq.is_multiple_of(120) {
                crate::log!(
                    "intel/display: overlay-surf-only seq={} reason={} pipe={} slot={} buffer={} surf=0x{:08X} live=deferred contract=preserved path=ui4-batched-surf\n",
                    seq,
                    reason,
                    surface.pipe.name,
                    surface.plane_slot,
                    surface.buffer_index,
                    surface_reg,
                );
            }
            return true;
        }
        PlaneSurfaceFlipQueueResult::Rejected => return false,
        PlaneSurfaceFlipQueueResult::Inactive => {}
    }
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_SURF_OFF, surface_reg);
    let (live, live_iters) = wait_for_plane_live_for(dev, plane_base, surface_reg, 25_000_000);
    if live != surface_reg {
        return false;
    }
    mark_overlay_surface_front(surface);
    let seq = OVERLAY_PRESENT_SEQ.load(Ordering::Relaxed).wrapping_add(1);
    if seq <= 8 || seq.is_multiple_of(120) {
        crate::log!(
            "intel/display: overlay-surf-only seq={} reason={} pipe={} slot={} buffer={} surf=0x{:08X} live=0x{:08X} live_iters={} contract=preserved\n",
            seq,
            reason,
            surface.pipe.name,
            surface.plane_slot,
            surface.buffer_index,
            surface_reg,
            live,
            live_iters,
        );
    }
    true
}

fn present_overlay_surface_with_bootstrap_contract(
    dev: crate::intel::Dev,
    surface: OverlaySurface,
    pos_x: u32,
    pos_y: u32,
    alpha: OverlayAlphaMode,
    reason: &str,
) -> bool {
    if !ui4_rgba8_plane_stack_ready(surface.pipe) {
        crate::log_error!(target: "intel/display";
            "intel/display: overlay present rejected reason={} pipe={} slot={} cause=rgba8-stack-not-ready\n",
            reason,
            surface.pipe.name,
            surface.plane_slot,
        );
        return false;
    }
    if !overlay_plane_needs_rearm(dev, surface, pos_x, pos_y, alpha) {
        mark_overlay_surface_front(surface);
        return true;
    }
    if flip_overlay_plane_surface(dev, surface, pos_x, pos_y, alpha, reason) {
        return true;
    }
    crate::log_error!(target: "intel/display";
        "intel/display: overlay present rejected reason={} pipe={} slot={} cause=immutable-rgba8-contract-mismatch action=no-runtime-rearm\n",
        reason,
        surface.pipe.name,
        surface.plane_slot,
    );
    false
}

pub(super) fn active_pipe(dev: crate::intel::Dev) -> Option<PipeInfo> {
    select_compatibility_pipeline(dev)?.snapshot.pipeline.pipe()
}

fn decode_plane_format(ctl: u32) -> &'static str {
    match ctl & PLANE_CTL_FORMAT_MASK_SKL {
        0x0000_0000 => "YUV422",
        0x0100_0000 => "NV12",
        0x0200_0000 => "XRGB2101010",
        0x0300_0000 => "P010",
        0x0400_0000 => "XRGB8888/ARGB8888",
        0x0500_0000 => "P012",
        0x0600_0000 => "XRGB16161616F",
        0x0700_0000 => "P016",
        0x0800_0000 => "XYUV",
        0x0C00_0000 => "INDEXED",
        0x0E00_0000 => "RGB565",
        _ => "unknown",
    }
}

fn decode_plane_color_alpha(color_ctl: u32) -> &'static str {
    match color_ctl & PLANE_COLOR_ALPHA_MASK {
        PLANE_COLOR_ALPHA_DISABLE => "disable",
        PLANE_COLOR_ALPHA_SW_PREMULT => "sw-premul",
        PLANE_COLOR_ALPHA_HW_PREMULT => "hw-premul",
        _ => "unknown",
    }
}

fn decode_plane_cus_phase(cus_ctl: u32, mask: u32, sign_bit: u32) -> i32 {
    let shift = mask.trailing_zeros();
    let magnitude = ((cus_ctl & mask) >> shift) as i32;
    if (cus_ctl & sign_bit) != 0 {
        -magnitude
    } else {
        magnitude
    }
}

fn decode_plane_tiling(ctl: u32) -> &'static str {
    match ctl & PLANE_CTL_TILED_MASK {
        0x0000 => "linear",
        0x0400 => "x",
        0x1000 => "y",
        0x1400 => "yf/4",
        _ => "unknown",
    }
}

fn decode_plane_rotation(ctl: u32) -> &'static str {
    match ctl & PLANE_CTL_ROTATE_MASK {
        0 => "0",
        1 => "90",
        2 => "180",
        3 => "270",
        _ => "unknown",
    }
}

#[inline]
fn decode_xy_x(v: u32) -> u32 {
    v & 0xFFFF
}

#[inline]
fn decode_xy_y(v: u32) -> u32 {
    (v >> 16) & 0xFFFF
}
