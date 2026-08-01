// Display proof contract.
//
// Current evidence from the bring-up transcript:
// - `primary-boot-surface` programs pipe-a at `2560x1440`, pitch `0x2800`.
// - The surface GPU address is `0x02000000`.
// - `surf_live` matches `surf` after the primary bootstrap.
//
// This proves scanout handoff to known memory.  It does not prove the 3D
// pipeline rendered that memory; render must separately produce `ps-rt-proof
// accepted=1` before a displayed pixel can be attributed to GPU rendering.

use crate::intel::types::Rgba8;
use alloc::{string::String, vec::Vec};
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
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
pub(crate) use self::display_probes::log_display_plane_ladder_probe;
use self::display_probes::{
    log_pipe_scanout_probe, log_primary_dimensions_probe, log_primary_plane_probe,
    probe_primary_present_psr,
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
// so it cannot collide with resident-scene's fixed resident scene addresses.
const DISPLAY_DIRECT_RCS_VA_LIMIT: u64 = 0x4000_0000;
// Scanout GGTT addresses are not render-engine addresses. Give every
// compositor destination a stable private PPGTT VA for its entire lifetime.
// Reusing one alias across alternating buffers or different planes requires a
// render TLB invalidation between jobs and can otherwise send complete GuC
// work into a stale, non-live scanout allocation.
const PRIMARY_COMPOSE_RCS_GPU_ALIAS_BASE: u64 = 0x2000_0000;
const OVERLAY_COMPOSE_RCS_GPU_ALIAS_BASE: u64 = 0x2200_0000;
const COMPOSE_RCS_GPU_ALIAS_BYTES: u64 = 0x0100_0000;
const OVERLAY_COMPOSE_RCS_GPU_PLANE_STRIDE: u64 = 0x0200_0000;
// One persistent GuC RCS context now advances its logical-ring tail for every
// admitted submission. This makes the probe followed by a multi-source UI4
// worklist real successive jobs instead of republishing the first ring entry.
// Never run the synchronous UI4 compositor through the general GPGPU LRC.
// Video conversion and fonts own that queue.  UI4 is re-enabled only through
// its isolated asynchronous compositor context.
const UI4_GPGPU_MULTI_RUN_COMPOSITOR_ENABLED: bool = false;
const OVERLAY_SWAP_GPU_BASE: u64 = 0x1800_0000;
const OVERLAY_SWAP_GPU_STRIDE: u64 = 0x0100_0000;
const OVERLAY_PIPE_GPU_STRIDE: u64 = 0x0200_0000;
const OVERLAY_PLANE_GPU_STRIDE: u64 = DISPLAY_PIPELINE_COUNT as u64 * OVERLAY_PIPE_GPU_STRIDE;
const OVERLAY_UNIVERSAL_PLANE_COUNT: usize = crate::ui4::UNIVERSAL_PLANE_COUNT - 1;
const DIRECT_RCS_OVERLAY_UNIVERSAL_PLANE_COUNT: usize = 3;
const INTERACTION_OVERLAY_GPU_BASE: u64 = DISPLAY_DIRECT_RCS_VA_LIMIT;
// Slot 0 is part of the premultiplied-RGBA application stack. Its compositor
// swap aliases occupy the gap between slot 4's
// interaction surfaces and the direct-scanout alias arena.
const UI4_SLOT0_OVERLAY_GPU_BASE: u64 =
    INTERACTION_OVERLAY_GPU_BASE + DISPLAY_PIPELINE_COUNT as u64 * OVERLAY_PIPE_GPU_STRIDE;
// Published UI4 buffers keep producer-owned PPGTT addresses. Direct scanout
// imports each producer surface into a display-owned GGTT alias. Keep enough
// aliases for the deepest UI4 buffering contract so a four-buffer video bridge
// reaches a steady state with one stable mapping per render target. SURF and
// SURFLIVE still protect queued/live aliases; remapping is only a fallback for
// a genuinely new allocation (for example after resize or frame teardown).
const UI4_DIRECT_SCANOUT_ALIAS_COUNT: usize = crate::ui4::FrameBuffering::Quad.count();
const UI4_DIRECT_SCANOUT_GPU_BASE: u64 = 0x5000_0000;
// Match the trusted UI-surface maximum so a 4K RGBA frame remains eligible.
const UI4_DIRECT_SCANOUT_GPU_STRIDE: u64 = 0x0200_0000;
const UI4_DIRECT_SCANOUT_PIPE_STRIDE: u64 =
    UI4_DIRECT_SCANOUT_ALIAS_COUNT as u64 * UI4_DIRECT_SCANOUT_GPU_STRIDE;
const UI4_DIRECT_SCANOUT_PLANE_STRIDE: u64 =
    DISPLAY_PIPELINE_COUNT as u64 * UI4_DIRECT_SCANOUT_PIPE_STRIDE;
const UI4_DIRECT_SCANOUT_PLANE_COUNT: usize = crate::ui4::INTERACTION_OVERLAY_PLANE_SLOT;
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
    PRIMARY_COMPOSE_RCS_GPU_ALIAS_BASE
        + PRIMARY_SWAP_BUFFER_COUNT as u64 * COMPOSE_RCS_GPU_ALIAS_BYTES
        <= OVERLAY_COMPOSE_RCS_GPU_ALIAS_BASE
);
const _: () = assert!(
    OVERLAY_COMPOSE_RCS_GPU_ALIAS_BASE
        + DIRECT_RCS_OVERLAY_UNIVERSAL_PLANE_COUNT as u64 * OVERLAY_COMPOSE_RCS_GPU_PLANE_STRIDE
        <= 0x2800_0000
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
        <= UI4_SLOT0_OVERLAY_GPU_BASE
);
const _: () = assert!(
    UI4_SLOT0_OVERLAY_GPU_BASE + DISPLAY_PIPELINE_COUNT as u64 * OVERLAY_PIPE_GPU_STRIDE
        <= UI4_DIRECT_SCANOUT_GPU_BASE
);
const _: () = assert!(
    UI4_DIRECT_SCANOUT_GPU_BASE
        + UI4_DIRECT_SCANOUT_PLANE_COUNT as u64 * UI4_DIRECT_SCANOUT_PLANE_STRIDE
        <= (u32::MAX as u64) + 1
);
pub(super) const DISPLAY_FRAME_TARGET_CAPACITY: usize =
    DISPLAY_PIPELINE_COUNT * OVERLAY_UNIVERSAL_PLANE_COUNT * OVERLAY_SWAP_BUFFER_COUNT;
const VIDEO_NV12_HIDE_PARK_BEFORE_DISABLE: bool = true;
const VIDEO_NV12_HIDE_PARK_SIZE: u32 = 64;

#[derive(Copy, Clone, Eq, PartialEq)]
struct PlaneSurfaceGeometry {
    stride_reg: u32,
    pos_reg: u32,
    size_reg: u32,
    offset_reg: u32,
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum PlaneScalerMode {
    Detached,
    Scaled {
        scaler_id: usize,
        window_pos_reg: u32,
        window_size_reg: u32,
        hphase_reg: u32,
        vphase_reg: u32,
    },
}

#[derive(Copy, Clone, Eq, PartialEq)]
struct PlaneScalerFlip {
    pipe_slot: usize,
    plane_slot: usize,
    mode: PlaneScalerMode,
}

#[derive(Copy, Clone, Eq, PartialEq)]
struct PlaneSurfaceFlip {
    plane_base: usize,
    surface_reg: u32,
    geometry: Option<PlaneSurfaceGeometry>,
    /// Hardware plane opacity. `None` is used by the opaque primary plane;
    /// overlays always stage an explicit value with the SURF transaction.
    constant_alpha: Option<u8>,
    /// Shared pipe-scaler state for this plane. Primary updates leave it
    /// untouched; every overlay update explicitly binds or detaches scaling.
    scaler: Option<PlaneScalerFlip>,
}

#[derive(Copy, Clone)]
struct PlaneSurfaceFlipBatch {
    active: bool,
    accepting: bool,
    len: usize,
    entries: [Option<PlaneSurfaceFlip>; UI4_PLANE_SURFACE_FLIP_BATCH_CAPACITY],
    submitted_ns: u64,
    polls: u32,
}

impl PlaneSurfaceFlipBatch {
    const fn new() -> Self {
        Self {
            active: false,
            accepting: false,
            len: 0,
            entries: [None; UI4_PLANE_SURFACE_FLIP_BATCH_CAPACITY],
            submitted_ns: 0,
            polls: 0,
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum PlaneSurfaceFlipQueueResult {
    Inactive,
    Queued,
    Rejected,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui4PlaneSurfaceFlipPoll {
    Pending,
    Complete { wait_ns: u64, polls: u32 },
    Failed,
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
static OVERLAY_SURFACES_SLOT_0: [Mutex<OverlaySurfacePool>; DISPLAY_PIPELINE_COUNT] = [
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
static UI4_DIRECT_SCANOUT_SLOT_1: [Mutex<Ui4DirectScanoutPool>; DISPLAY_PIPELINE_COUNT] = [
    Mutex::new(Ui4DirectScanoutPool::new()),
    Mutex::new(Ui4DirectScanoutPool::new()),
    Mutex::new(Ui4DirectScanoutPool::new()),
    Mutex::new(Ui4DirectScanoutPool::new()),
];
static UI4_DIRECT_SCANOUT_SLOT_0: [Mutex<Ui4DirectScanoutPool>; DISPLAY_PIPELINE_COUNT] = [
    Mutex::new(Ui4DirectScanoutPool::new()),
    Mutex::new(Ui4DirectScanoutPool::new()),
    Mutex::new(Ui4DirectScanoutPool::new()),
    Mutex::new(Ui4DirectScanoutPool::new()),
];
static UI4_DIRECT_SCANOUT_SLOT_2: [Mutex<Ui4DirectScanoutPool>; DISPLAY_PIPELINE_COUNT] = [
    Mutex::new(Ui4DirectScanoutPool::new()),
    Mutex::new(Ui4DirectScanoutPool::new()),
    Mutex::new(Ui4DirectScanoutPool::new()),
    Mutex::new(Ui4DirectScanoutPool::new()),
];
static UI4_DIRECT_SCANOUT_SLOT_3: [Mutex<Ui4DirectScanoutPool>; DISPLAY_PIPELINE_COUNT] = [
    Mutex::new(Ui4DirectScanoutPool::new()),
    Mutex::new(Ui4DirectScanoutPool::new()),
    Mutex::new(Ui4DirectScanoutPool::new()),
    Mutex::new(Ui4DirectScanoutPool::new()),
];
static PRIMARY_SWAP_SURFACES: [Mutex<PrimarySwapSurfacePool>; DISPLAY_PIPELINE_COUNT] = [
    Mutex::new(PrimarySwapSurfacePool::new()),
    Mutex::new(PrimarySwapSurfacePool::new()),
    Mutex::new(PrimarySwapSurfacePool::new()),
    Mutex::new(PrimarySwapSurfacePool::new()),
];
static VIDEO_NV12_PLANE_ALPHA: AtomicU32 = AtomicU32::new(0xFF);

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

/// Borrowed view of the immutable UI4 slot-0 base currently latched by pipe A.
///
/// This is intentionally the fixed D01 test-rig contract: after the one-time
/// XRGB-to-RGBA handoff, the original primary allocation remains the opaque
/// full-output logo/background and the broker places no windows on slot 0.
pub(crate) struct Ui4StreamSlot0View<'a> {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
    pub(crate) rgba_premultiplied: &'a [u8],
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
    /// Keep the producer's PAT3/UC PPGTT policy when this surface was authored
    /// for direct scanout. Generic composition may sample it, but must not
    /// create a PAT0/WB synonym at the same GPU virtual address.
    pub(crate) gpgpu_scanout_cache: bool,
    /// Additional whole-tile opacity applied after source alpha.
    pub(crate) opacity: u8,
    /// The producer contract guarantees alpha=255 for every published pixel.
    /// This permits an opaque primary tile to replace damaged pixels directly,
    /// without first sampling the immutable base in a second GPU run.
    pub(crate) known_opaque: bool,
    pub(crate) expected_rgba: Option<Rgba8>,
}

/// A producer-published premultiplied RGBA frame eligible for display-plane
/// import. Its producer GPU address is intentionally absent: scanout receives
/// a separate display-owned GGTT alias and launches no render work.
#[derive(Copy, Clone)]
pub(crate) struct Ui4DirectRgbaFrame {
    pub(crate) phys: u64,
    pub(crate) byte_len: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
    /// Producer-side identity carried only for end-to-end handoff evidence.
    /// Display still consumes the physical allocation through its own GGTT
    /// alias and never interprets the producer's GPU virtual address.
    pub(crate) producer_frame: u64,
    pub(crate) producer_buffer_index: u8,
    pub(crate) producer_publish_serial: u64,
    pub(crate) producer_release_sequence: u64,
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

pub(crate) fn with_ui4_stream_pipe_a_slot0_surflive<R>(
    read: impl FnOnce(Ui4StreamSlot0View<'_>) -> R,
) -> Option<R> {
    const STREAM_PIPE: usize = 0;

    let dev = crate::intel::claimed_device()?;
    let pipe = PIPES[STREAM_PIPE];
    if !ui4_rgba8_plane_stack_ready(pipe) {
        return None;
    }
    let owner = primary_surface_owner(pipe).lock();
    let surface = (*owner)?;
    let plane = pipe.plane(crate::ui4::PRIMARY_PLANE_SLOT);
    let ctl = crate::intel::mmio_read(dev, plane.ctl());
    let expected_live = u32::try_from(surface.gpu).ok()?;
    let expected_stride = plane_stride_reg_value(surface.pitch_bytes)?;
    if ctl & PLANE_CTL_ENABLE == 0
        || ctl & PLANE_CTL_FORMAT_MASK_SKL != PLANE_CTL_FORMAT_XRGB_8888
        || ctl & PLANE_CTL_TILED_MASK != PLANE_CTL_TILED_LINEAR
        || ctl & PLANE_CTL_ORDER_RGBX == 0
        || crate::intel::mmio_read(dev, plane.surf_live()) != expected_live
        || crate::intel::mmio_read(dev, plane.stride()) != expected_stride
        || crate::intel::mmio_read(dev, plane.base() + UNI_PLANE_POS_OFF)
            != plane_pos_reg_value(0, 0)
        || crate::intel::mmio_read(dev, plane.base() + UNI_PLANE_SIZE_OFF)
            != plane_size_reg_value(surface.width, surface.height)
        || crate::intel::mmio_read(dev, plane.base() + UNI_PLANE_OFFSET_OFF) != 0
        || crate::intel::mmio_read(dev, plane.base() + UNI_PLANE_CUS_CTL_OFF) != 0
    {
        return None;
    }
    let visible_bytes = (surface.pitch_bytes as usize).checked_mul(surface.height as usize)?;
    if surface.virt.is_null() || visible_bytes > surface.byte_len {
        return None;
    }
    let rgba_premultiplied =
        unsafe { core::slice::from_raw_parts(surface.virt.cast_const(), visible_bytes) };
    Some(read(Ui4StreamSlot0View {
        width: surface.width,
        height: surface.height,
        pitch_bytes: surface.pitch_bytes,
        rgba_premultiplied,
    }))
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
struct Ui4DirectOverlaySurface {
    width: u32,
    height: u32,
    dest_width: u32,
    dest_height: u32,
    pitch_bytes: u32,
    byte_len: usize,
    phys: u64,
    gpu: u64,
    pipe: PipeInfo,
    plane_slot: usize,
    alias_index: usize,
    pos_x: u32,
    pos_y: u32,
    opacity: u8,
    producer_frame: u64,
    producer_buffer_index: u8,
    producer_publish_serial: u64,
    producer_release_sequence: u64,
}

#[derive(Copy, Clone)]
struct Ui4DirectScanoutMapping {
    phys: u64,
    byte_len: usize,
}

#[derive(Copy, Clone)]
struct Ui4DirectScanoutPool {
    next_alias: usize,
    mappings: [Option<Ui4DirectScanoutMapping>; UI4_DIRECT_SCANOUT_ALIAS_COUNT],
}

#[derive(Copy, Clone)]
struct OverlaySurfacePool {
    width: u32,
    height: u32,
    pipe_slot: usize,
    front_index: Option<usize>,
    surfaces: [Option<OverlaySurface>; OVERLAY_SWAP_BUFFER_COUNT],
    damage_debt: [CompositionDamageRegion; OVERLAY_SWAP_BUFFER_COUNT],
    /// False means the allocation still contains the transparent zero fill
    /// performed by `ensure_overlay_surface_for_pipe` and has never been used
    /// as a composition destination. Sparse immutable painters can use that
    /// known base directly instead of launching a fullscreen clear.
    content_initialized: [bool; OVERLAY_SWAP_BUFFER_COUNT],
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
    /// A false entry still contains the immutable primary copied at allocation;
    /// true means the surface can additionally contain an older composition.
    composited: [bool; PRIMARY_SWAP_BUFFER_COUNT],
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
            content_initialized: [false; OVERLAY_SWAP_BUFFER_COUNT],
            retiring_front: None,
        }
    }

    fn matches(self, width: u32, height: u32, pipe: PipeInfo) -> bool {
        self.width == width && self.height == height && self.pipe_slot == pipe.slot
    }
}

impl Ui4DirectScanoutPool {
    const fn new() -> Self {
        Self {
            next_alias: 0,
            mappings: [None; UI4_DIRECT_SCANOUT_ALIAS_COUNT],
        }
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
            composited: [false; PRIMARY_SWAP_BUFFER_COUNT],
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

/// Give the firmware-compatible XRGB boot phase deterministic pixels. UI4
/// discards this boot content when Slot0 becomes its transparent RGBA slice.
fn initialize_primary_boot_surface_black(surface: PrimarySurface) -> bool {
    if surface.virt.is_null()
        || surface.width == 0
        || surface.height == 0
        || surface.pitch_bytes < surface.width.saturating_mul(PRIMARY_BYTES_PER_PIXEL)
    {
        return false;
    }

    // Initialize the padding and edge-guard area as opaque black too. Only the
    // width x height viewport is scanned out, but no uninitialized allocation
    // bytes should border a display surface used by later GPU work.
    let allocation_pixels = surface.byte_len / core::mem::size_of::<u32>();
    let allocation =
        unsafe { core::slice::from_raw_parts_mut(surface.virt.cast::<u32>(), allocation_pixels) };
    for pixel in allocation {
        *pixel = 0xFF00_0000;
    }

    crate::intel::dma_flush(surface.virt, surface.byte_len);
    crate::log!(
        "intel/display: primary-boot initialization pipe={} size={}x{} slot=0 xrgb=opaque-black ui4_handoff=transparent-rgba\n",
        surface.pipe.name,
        surface.width,
        surface.height,
    );
    true
}

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

    let primary_initialized = initialize_primary_boot_surface_black(primary_surface);

    log_primary_plane_probe(dev, pipe, "before-rgba8-stack-bootstrap");
    let ok = bootstrap_ui4_rgba8_plane_stack_once(dev, primary_surface);
    log_primary_plane_probe(dev, pipe, "after-rgba8-stack-bootstrap");
    log_pipe_scanout_probe(dev, "after-primary-init");
    let surf_armed = crate::intel::mmio_read(dev, pipe.primary_plane().surf());
    let surf_live = crate::intel::mmio_read(dev, pipe.primary_plane().surf_live());
    let ctl_after = crate::intel::mmio_read(dev, pipe.primary_plane().ctl());

    crate::log!(
        "intel/display: primary-boot-surface pipe={} size={}x{} backing={}x{} pitch=0x{:X} bytes=0x{:X} guard={} gpu=0x{:X} phys=0x{:X} plane_enabled={} ctl_before=0x{:08X} ctl_after=0x{:08X} surf_before=0x{:08X} surf=0x{:08X} surf_live=0x{:08X} ok={} primary_initialized={} boot_images=none overlays=transparent-native-rgba8-slots1-4 ui=bootstrap-stack-ready\n",
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
        primary_initialized as u8,
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

fn program_pipe_bottom_color(dev: crate::intel::Dev, pipe: PipeInfo, raw: u32) -> bool {
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
    readback == raw
}

fn pipe_bottom_color_from_xrgb(color: u32) -> u32 {
    let red = ((color >> 16) & 0xFF) * 0x3FF / 0xFF;
    let green = ((color >> 8) & 0xFF) * 0x3FF / 0xFF;
    let blue = (color & 0xFF) * 0x3FF / 0xFF;
    pipe_bottom_color_u0_10(red, green, blue)
}

/// Program the real Pipe A bottom color. This register has RGB channels but no
/// alpha channel; it is visible only where enabled planes do not cover the
/// pipe output.
pub(crate) fn set_pipe_a_bottom_color_rgb8(red: u8, green: u8, blue: u8) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let pipe = PIPES[0];
    let xrgb = (u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue);
    let raw = pipe_bottom_color_from_xrgb(xrgb);
    let programmed = program_pipe_bottom_color(dev, pipe, raw);
    crate::log_info!(target: "intel/display";
        "intel/display: runtime bottom-color pipe={} rgb8={},{},{} raw=0x{:08X} programmed={} source=ui4-color-picker\n",
        pipe.name,
        red,
        green,
        blue,
        raw,
        programmed as u8,
    );
    programmed
}

pub(crate) fn active_scanout_dimensions() -> Option<(u32, u32)> {
    let target = primary_display_output_target()?.pipeline_target;
    Some((target.width, target.height))
}

/// Hardware pipe with a complete scanout route for the primary logical output.
///
/// Cursor-plane owners must not bind from a provisional compatibility rank:
/// the cursor register bank is pipe-local, so accepting a merely observed pipe
/// can permanently attach the one-worker Spirit deployment to the wrong bank.
fn complete_scanout_pipeline_target() -> Option<DisplayPipelineTarget> {
    let dev = crate::intel::claimed_device()?;
    let snapshots = display_pipeline_snapshots_for_dev(dev);
    let selection = select_compatibility_pipeline_from_snapshots(&snapshots)?;
    log_compatibility_pipeline_selection(selection);
    if selection.snapshot.activity != DisplayPipelineActivity::Scanout {
        return None;
    }
    selection.snapshot.target
}

pub(crate) fn complete_scanout_pipeline_slot() -> Option<usize> {
    Some(complete_scanout_pipeline_target()?.pipeline.slot())
}

/// Dimensions belonging to one exact complete hardware pipeline.
///
/// This keeps cursor placement on the same accepted topology snapshot as the
/// fence-to-cursor-bank selection. Callers must not reinterpret PIPESRC through
/// a second private decoder after this proof has succeeded.
pub(crate) fn complete_scanout_pipeline_dimensions(slot: usize) -> Option<(u32, u32)> {
    let target = complete_scanout_pipeline_target()?;
    (target.pipeline.slot() == slot).then_some((target.width, target.height))
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

/// Produce a read-only display-engine register snapshot suitable for a
/// persistent diagnostic dump.
pub(crate) fn diagnostic_snapshot_text() -> String {
    let mut out = String::new();
    let Some(dev) = crate::intel::claimed_device() else {
        let _ = writeln!(out, "Intel display device not claimed");
        return out;
    };

    let _ = writeln!(
        out,
        "device={:02X}:{:02X}.{} did=0x{:04X} rev=0x{:02X} mmio=0x{:016X} mmio_len=0x{:X}",
        dev.bus,
        dev.slot,
        dev.function,
        dev.device_id,
        dev.revision_id,
        dev.mmio as u64,
        dev.mmio_len
    );
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
        let _ = writeln!(
            out,
            "pipeline={} slot={} activity={} observed={} mode={}x{} selected={} reason={}",
            snapshot.pipeline.name(),
            snapshot.pipeline.slot(),
            snapshot.activity.name(),
            snapshot.observed,
            width,
            height,
            selected,
            selection_reason
        );
        let _ = writeln!(
            out,
            "  pipe_enabled={} transcoder_enabled={} primary_enabled={} primary_bound={} ddi={} link_mode={} bpc={} sync_polarity=0x{:X} port_width={}",
            snapshot.pipe_enabled,
            snapshot.transcoder_enabled,
            snapshot.primary_enabled,
            snapshot.primary_bound,
            snapshot.route.ddi.name(),
            snapshot.route.mode_name(),
            snapshot.route.bits_per_color(),
            snapshot.route.sync_polarity,
            snapshot.route.port_width
        );
        let _ = writeln!(
            out,
            "  pipe_src=0x{:08X} pipeconf=0x{:08X} transcoder=0x{:08X} primary_ctl=0x{:08X} primary_surf=0x{:08X} primary_live=0x{:08X}",
            snapshot.pipe_src,
            snapshot.pipeconf,
            snapshot.transcoder,
            snapshot.primary_ctl,
            snapshot.primary_surf,
            snapshot.primary_live
        );
    }

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
    let _ = writeln!(
        out,
        "power_wells main_bios=0x{:08X} main_driver=0x{:08X} main_kvmr=0x{:08X} main_debug=0x{:08X}",
        main_bios, main_driver, main_kvmr, main_debug
    );
    let _ = writeln!(
        out,
        "power_wells aux_bios=0x{:08X} aux_driver=0x{:08X} aux_debug=0x{:08X} ddi_bios=0x{:08X} ddi_driver=0x{:08X} ddi_debug=0x{:08X} fuse=0x{:08X}",
        aux_bios, aux_driver, aux_debug, ddi_bios, ddi_driver, ddi_debug, fuse
    );
    if let Some(gpu) = primary_surface_gpu_addr() {
        let _ = writeln!(out, "tracked_primary_surface_gpu=0x{:016X}", gpu);
    } else {
        let _ = writeln!(out, "tracked_primary_surface_gpu=unavailable");
    }
    if let Some(samples) = capture_primary_surface_samples() {
        let _ = writeln!(
            out,
            "tracked_primary_samples tl=0x{:08X} center=0x{:08X} br=0x{:08X} apex=0x{:08X} centroid=0x{:08X} left=0x{:08X} right=0x{:08X}",
            samples.tl,
            samples.center,
            samples.br,
            samples.apex,
            samples.centroid,
            samples.left,
            samples.right
        );
    } else {
        let _ = writeln!(out, "tracked_primary_samples=unavailable");
    }
    out
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
        // This is also the eligibility proof consumed by pipe-local owners
        // such as Spirit. Keep the one-shot route transition visible even
        // when the focused GFX policy suppresses ordinary info records.
        crate::log!(
            "intel/display: compatibility-pipeline selected={} rank={} reason={} route={} link_mode={} bpc={} port_width={} mode={}x{} candidates=0x{:X} scanout=0x{:X}\n",
            selection.snapshot.pipeline.name(),
            selection.rank,
            selection.reason,
            selection.snapshot.route.ddi.name(),
            selection.snapshot.route.mode_name(),
            selection.snapshot.route.bits_per_color(),
            selection.snapshot.route.port_width,
            selection
                .snapshot
                .target
                .map(|target| target.width)
                .unwrap_or(0),
            selection
                .snapshot
                .target
                .map(|target| target.height)
                .unwrap_or(0),
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

fn primary_surface_gpu_addr() -> Option<u64> {
    active_primary_surface().map(|surface| surface.gpu)
}

fn log_primary_surface_samples(label: &str) {
    let Some(surface) = active_primary_surface() else {
        return;
    };
    log_surface_samples(surface, label);
}

fn capture_primary_surface_samples() -> Option<PrimarySurfaceSampleSet> {
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

fn sample_primary_surface_pixel(x: u32, y: u32) -> Option<u32> {
    let surface = active_primary_surface()?;
    sample_surface_pixel(surface, x as usize, y as usize)
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
    let ui4_primary_batch_only = reason == "ui4-compositor-primary-async";
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
    if ui4_primary_batch_only && surf_before != surf_live_before {
        crate::log_warn!(target: "ui4";
            "ui4/primary-flip: stage rejected reason={} pipeline={} surf=0x{:08X} surf_live=0x{:08X} action=retry batch_only=1 direct_mmio_fallback=0 contract_rearm=0\n",
            reason,
            pipeline.name(),
            surf_before,
            surf_live_before,
        );
        return false;
    }
    if surf_before == surf_live_before {
        match queue_ui4_plane_surface_flip(
            pipe.primary_plane().base(),
            surface_reg,
            None,
            None,
            None,
            reason,
        ) {
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
            PlaneSurfaceFlipQueueResult::Inactive if ui4_primary_batch_only => return false,
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

#[derive(Copy, Clone)]
pub(crate) struct Ui4LiveOverlayFlip {
    surface: OverlaySurface,
    surface_reg: u32,
    change: CompositionDamageRegion,
    effective: CompositionDamageRegion,
    rect_count: usize,
    submitted_ns: u64,
    reason: &'static str,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui4LiveOverlayFlipPoll {
    Pending,
    Complete,
    Failed,
}

/// Draw sparse interaction rectangles into the next slot-local swap surface,
/// flush only their accumulated damage, publish SURF once, and return without
/// waiting for vblank/SURFLIVE.
pub(crate) fn queue_ui4_live_overlay_rects_on_slot_damage_region(
    plane_slot: usize,
    rects: &[LiveOverlayRect],
    damage: CompositionDamageRegion,
    reason: &'static str,
) -> Option<Ui4LiveOverlayFlip> {
    if plane_slot != crate::ui4::INTERACTION_OVERLAY_PLANE_SLOT {
        crate::log_error!(target: "intel/display";
            "intel/display: ui4-live-overlay rejected reason={} slot={} cause=interaction-slot4-required\n",
            reason,
            plane_slot,
        );
        return None;
    }
    let dev = crate::intel::claimed_device()?;
    let (width, height) = active_scanout_dimensions()
        .or_else(|| active_primary_surface().map(|primary| (primary.width, primary.height)))?;
    let change = clip_composition_damage_region(damage, width, height);
    if change.is_empty() || !ui4_rgba8_plane_stack_ready(active_pipe(dev)?) {
        return None;
    }
    let surface = ensure_overlay_surface_on_slot(dev, plane_slot, width, height)?;
    let effective = {
        let surface_pool = overlay_surface_pool(surface.pipe, surface.plane_slot)?;
        let pool = surface_pool.lock();
        let mut effective = pool.damage_debt[surface.buffer_index];
        effective.add_region(change);
        effective
    };

    for damaged in effective.rects() {
        fill_overlay_rect(surface, damaged.x, damaged.y, damaged.width, damaged.height, 0);
        for rect in rects {
            fill_overlay_rect_rgba_clipped(surface, *rect, *damaged);
        }
    }
    if !dma_flush_overlay_region(surface, effective) {
        return None;
    }

    let surface_reg = u32::try_from(surface.gpu).ok()?;
    let plane_base = overlay_plane_base(surface.pipe, surface.plane_slot);
    let already_live = !overlay_plane_needs_rearm(dev, surface, 0, 0, UI4_RGBA8_OVERLAY_CONTRACT);
    if !already_live
        && overlay_plane_surface_flip_guard(dev, surface, 0, 0, UI4_RGBA8_OVERLAY_CONTRACT).is_err()
    {
        return None;
    }
    if !already_live {
        let geometry =
            overlay_plane_geometry(surface.pitch_bytes, surface.width, surface.height, 0, 0)?;
        program_overlay_plane_geometry(dev, plane_base, geometry);
        crate::intel::mmio_write(dev, plane_base + UNI_PLANE_SURF_OFF, surface_reg);
    }
    Some(Ui4LiveOverlayFlip {
        surface,
        surface_reg,
        change,
        effective,
        rect_count: rects.len(),
        submitted_ns: crate::chronos::monotonic_nanos(),
        reason,
    })
}

/// Read slot-local SURFLIVE exactly once.  No executor turn spins waiting for
/// the display engine; ownership and damage debt advance only after the latch.
pub(crate) fn poll_ui4_live_overlay_flip(flip: Ui4LiveOverlayFlip) -> Ui4LiveOverlayFlipPoll {
    let Some(dev) = crate::intel::claimed_device() else {
        return Ui4LiveOverlayFlipPoll::Failed;
    };
    let plane_base = overlay_plane_base(flip.surface.pipe, flip.surface.plane_slot);
    let live = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF);
    let elapsed_ns = crate::chronos::monotonic_nanos().saturating_sub(flip.submitted_ns);
    if live != flip.surface_reg {
        if elapsed_ns < UI4_PLANE_SURFACE_FLIP_TIMEOUT_NS {
            return Ui4LiveOverlayFlipPoll::Pending;
        }
        crate::log_warn!(target: "intel/display";
            "intel/display: ui4-live-overlay-flip timeout reason={} pipe={} slot={} surf=0x{:08X} live=0x{:08X} wait_ns={}\n",
            flip.reason,
            flip.surface.pipe.name,
            flip.surface.plane_slot,
            flip.surface_reg,
            live,
            elapsed_ns,
        );
        return Ui4LiveOverlayFlipPoll::Failed;
    }

    mark_overlay_composition_surface_front(flip.surface, flip.change);
    let seq = OVERLAY_PRESENT_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if seq <= 8 || seq.is_multiple_of(120) {
        let change_bounds = flip.change.bounding_rect().unwrap_or_default();
        let effective_bounds = flip.effective.bounding_rect().unwrap_or_default();
        crate::log!(
            "intel/display: live-overlay-damage-present seq={} reason={} pipe={} slot={} buffer={} path=surf-only-async rects={} damage_rects={} damage_bounds={}x{}@{},{} effective_rects={} effective_bounds={}x{}@{},{} wait_ns={}\n",
            seq,
            flip.reason,
            flip.surface.pipe.name,
            flip.surface.plane_slot,
            flip.surface.buffer_index,
            flip.rect_count,
            flip.change.len(),
            change_bounds.width,
            change_bounds.height,
            change_bounds.x,
            change_bounds.y,
            flip.effective.len(),
            effective_bounds.width,
            effective_bounds.height,
            effective_bounds.x,
            effective_bounds.y,
            elapsed_ns,
        );
    }
    Ui4LiveOverlayFlipPoll::Complete
}

/// Compose positioned RGBA tiles into one full-scanout transparent surface and
/// commit one hardware universal plane. Each selected slot owns an independent
/// double-buffered composition surface.
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

/// Begin one bounded UI4 display transaction. Plane format, DBUF, watermarks
/// and pixel-alpha interpretation remain fixed. An overlay may stage its
/// constant opacity together with linear RGBA geometry and the SURF address.
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

pub(crate) fn cancel_ui4_plane_surface_flip_batch() {
    *UI4_PLANE_SURFACE_FLIP_BATCH.lock() = PlaneSurfaceFlipBatch::new();
}

fn queue_ui4_plane_surface_flip(
    plane_base: usize,
    surface_reg: u32,
    geometry: Option<PlaneSurfaceGeometry>,
    constant_alpha: Option<u8>,
    scaler: Option<PlaneScalerFlip>,
    reason: &str,
) -> PlaneSurfaceFlipQueueResult {
    // This transaction belongs exclusively to the application compositor.
    // Slot 4 has its own input-driven presentation loop and must never be
    // absorbed into, or rejected by, an application-plane transaction merely
    // because both callers use a `ui4-` reason prefix.
    if !matches!(
        reason,
        "ui4-compositor-primary-async"
            | "ui4-alpha-slot1-async"
            | "ui4-rgb-slot2-async"
            | "ui4-rgb-slot3-async"
            | "ui4-solara-slot2-async"
            | "ui4-overlay-async"
    ) {
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
            return if entry.surface_reg == surface_reg
                && entry.geometry == geometry
                && entry.constant_alpha == constant_alpha
                && entry.scaler == scaler
            {
                PlaneSurfaceFlipQueueResult::Queued
            } else {
                PlaneSurfaceFlipQueueResult::Rejected
            };
        }
    }
    if batch.len >= batch.entries.len() {
        return PlaneSurfaceFlipQueueResult::Rejected;
    }
    if let Some(PlaneScalerFlip {
        pipe_slot,
        plane_slot,
        mode: PlaneScalerMode::Scaled { scaler_id, .. },
    }) = scaler
        && batch.entries[..batch.len]
            .iter()
            .flatten()
            .filter_map(|entry| entry.scaler)
            .any(|queued| {
                queued.pipe_slot == pipe_slot
                    && matches!(
                        queued.mode,
                        PlaneScalerMode::Scaled {
                            scaler_id: queued_id,
                            ..
                        } if queued_id == scaler_id && queued.plane_slot != plane_slot
                    )
            })
    {
        return PlaneSurfaceFlipQueueResult::Rejected;
    }
    let index = batch.len;
    batch.entries[index] = Some(PlaneSurfaceFlip {
        plane_base,
        surface_reg,
        geometry,
        constant_alpha,
        scaler,
    });
    batch.len += 1;
    PlaneSurfaceFlipQueueResult::Queued
}

#[derive(Copy, Clone)]
struct PipeScalerRegisters {
    control: usize,
    window_pos: usize,
    window_size: usize,
    hphase: usize,
    vphase: usize,
}

fn pipe_scaler_registers(pipe_slot: usize, scaler_id: usize) -> Option<PipeScalerRegisters> {
    let control = match (pipe_slot, scaler_id) {
        (0, 0) => PIPE_SCALER_0_A_CTRL,
        (0, 1) => PIPE_SCALER_1_A_CTRL,
        (1, 0) => PIPE_SCALER_0_B_CTRL,
        (1, 1) => PIPE_SCALER_1_B_CTRL,
        (2, 0) => PIPE_SCALER_0_C_CTRL,
        _ => return None,
    };
    Some(PipeScalerRegisters {
        control,
        window_pos: control - PIPE_SCALER_WIN_POS_FROM_CTRL,
        window_size: control - PIPE_SCALER_WIN_SIZE_FROM_CTRL,
        hphase: control + PIPE_SCALER_HPHASE_FROM_CTRL,
        vphase: control + PIPE_SCALER_VPHASE_FROM_CTRL,
    })
}

const fn pipe_scaler_binding(plane_slot: usize) -> u32 {
    (((plane_slot as u32).saturating_add(1)) << 25) & PIPE_SCALER_BINDING_MASK
}

fn pipe_scaler_bound_to_plane(
    dev: crate::intel::Dev,
    pipe_slot: usize,
    scaler_id: usize,
    plane_slot: usize,
) -> bool {
    let Some(regs) = pipe_scaler_registers(pipe_slot, scaler_id) else {
        return false;
    };
    let control = crate::intel::mmio_read(dev, regs.control);
    control & (PIPE_SCALER_ENABLE | PIPE_SCALER_BINDING_MASK)
        == PIPE_SCALER_ENABLE | pipe_scaler_binding(plane_slot)
}

fn disable_pipe_scaler(dev: crate::intel::Dev, regs: PipeScalerRegisters) {
    crate::intel::mmio_write(dev, regs.control, 0);
    crate::intel::mmio_write(dev, regs.window_pos, 0);
    crate::intel::mmio_write(dev, regs.window_size, 0);
}

fn detach_pipe_scalers_from_plane(dev: crate::intel::Dev, pipe_slot: usize, plane_slot: usize) {
    for scaler_id in 0..2 {
        if pipe_scaler_bound_to_plane(dev, pipe_slot, scaler_id, plane_slot)
            && let Some(regs) = pipe_scaler_registers(pipe_slot, scaler_id)
        {
            disable_pipe_scaler(dev, regs);
        }
    }
}

fn prepare_plane_scaler_flips(
    dev: crate::intel::Dev,
    entries: &[Option<PlaneSurfaceFlip>],
) -> bool {
    for scaler in entries.iter().flatten().filter_map(|entry| entry.scaler) {
        let wanted_scaler = match scaler.mode {
            PlaneScalerMode::Detached => None,
            PlaneScalerMode::Scaled { scaler_id, .. } => {
                if pipe_scaler_registers(scaler.pipe_slot, scaler_id).is_none() {
                    return false;
                }
                Some(scaler_id)
            }
        };
        for scaler_id in 0..2 {
            let Some(regs) = pipe_scaler_registers(scaler.pipe_slot, scaler_id) else {
                continue;
            };
            let bound_to_plane =
                pipe_scaler_bound_to_plane(dev, scaler.pipe_slot, scaler_id, scaler.plane_slot);
            let target_rebound = wanted_scaler == Some(scaler_id)
                && crate::intel::mmio_read(dev, regs.control)
                    & (PIPE_SCALER_ENABLE | PIPE_SCALER_BINDING_MASK)
                    != PIPE_SCALER_ENABLE | pipe_scaler_binding(scaler.plane_slot);
            if (bound_to_plane && wanted_scaler != Some(scaler_id)) || target_rebound {
                disable_pipe_scaler(dev, regs);
            }
        }
    }
    true
}

fn program_plane_scaler_flip(dev: crate::intel::Dev, scaler: PlaneScalerFlip) -> bool {
    let PlaneScalerMode::Scaled {
        scaler_id,
        window_pos_reg,
        window_size_reg,
        hphase_reg,
        vphase_reg,
    } = scaler.mode
    else {
        return true;
    };
    let Some(regs) = pipe_scaler_registers(scaler.pipe_slot, scaler_id) else {
        return false;
    };
    crate::intel::mmio_write(
        dev,
        regs.control,
        PIPE_SCALER_ENABLE | pipe_scaler_binding(scaler.plane_slot),
    );
    crate::intel::mmio_write(dev, regs.vphase, vphase_reg);
    crate::intel::mmio_write(dev, regs.hphase, hphase_reg);
    crate::intel::mmio_write(dev, regs.window_pos, window_pos_reg);
    crate::intel::mmio_write(dev, regs.window_size, window_size_reg);
    true
}

fn plane_scaler_flip_matches(dev: crate::intel::Dev, scaler: PlaneScalerFlip) -> bool {
    match scaler.mode {
        PlaneScalerMode::Detached => !(0..2).any(|scaler_id| {
            pipe_scaler_bound_to_plane(dev, scaler.pipe_slot, scaler_id, scaler.plane_slot)
        }),
        PlaneScalerMode::Scaled {
            scaler_id,
            window_pos_reg,
            window_size_reg,
            hphase_reg,
            vphase_reg,
        } => {
            let Some(regs) = pipe_scaler_registers(scaler.pipe_slot, scaler_id) else {
                return false;
            };
            pipe_scaler_bound_to_plane(dev, scaler.pipe_slot, scaler_id, scaler.plane_slot)
                && crate::intel::mmio_read(dev, regs.window_pos) == window_pos_reg
                && crate::intel::mmio_read(dev, regs.window_size) == window_size_reg
                && crate::intel::mmio_read(dev, regs.hphase) == hphase_reg
                && crate::intel::mmio_read(dev, regs.vphase) == vphase_reg
                && !(0..2).any(|other_id| {
                    other_id != scaler_id
                        && pipe_scaler_bound_to_plane(
                            dev,
                            scaler.pipe_slot,
                            other_id,
                            scaler.plane_slot,
                        )
                })
        }
    }
}

/// Publish all staged scaler, geometry, opacity and SURF state back-to-back.
pub(crate) fn submit_ui4_plane_surface_flip_batch() -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let mut batch = UI4_PLANE_SURFACE_FLIP_BATCH.lock();
    if !batch.active || !batch.accepting {
        return false;
    }
    batch.accepting = false;
    batch.submitted_ns = crate::chronos::monotonic_nanos();
    batch.polls = 0;
    if !prepare_plane_scaler_flips(dev, &batch.entries[..batch.len]) {
        return false;
    }
    for entry in batch.entries[..batch.len].iter().flatten() {
        if entry
            .scaler
            .is_some_and(|scaler| !program_plane_scaler_flip(dev, scaler))
        {
            return false;
        }
        if let Some(geometry) = entry.geometry {
            crate::intel::mmio_write(
                dev,
                entry.plane_base + UNI_PLANE_STRIDE_OFF,
                geometry.stride_reg,
            );
            crate::intel::mmio_write(dev, entry.plane_base + UNI_PLANE_POS_OFF, geometry.pos_reg);
            crate::intel::mmio_write(dev, entry.plane_base + UNI_PLANE_SIZE_OFF, geometry.size_reg);
            crate::intel::mmio_write(
                dev,
                entry.plane_base + UNI_PLANE_OFFSET_OFF,
                geometry.offset_reg,
            );
        }
        if let Some(alpha) = entry.constant_alpha {
            program_overlay_plane_constant_alpha(dev, entry.plane_base, alpha);
        }
        crate::intel::mmio_write(dev, entry.plane_base + UNI_PLANE_SURF_OFF, entry.surface_reg);
    }
    true
}

/// Observe every staged SURFLIVE register once.  AP1 calls this from a later
/// compositor tick; it never burns a frame interval in a polling loop.
pub(crate) fn poll_ui4_plane_surface_flip_batch() -> Ui4PlaneSurfaceFlipPoll {
    let Some(dev) = crate::intel::claimed_device() else {
        *UI4_PLANE_SURFACE_FLIP_BATCH.lock() = PlaneSurfaceFlipBatch::new();
        return Ui4PlaneSurfaceFlipPoll::Failed;
    };
    let mut batch = UI4_PLANE_SURFACE_FLIP_BATCH.lock();
    if !batch.active || batch.accepting {
        return Ui4PlaneSurfaceFlipPoll::Failed;
    }
    batch.polls = batch.polls.saturating_add(1);
    let mut live = [0u32; UI4_PLANE_SURFACE_FLIP_BATCH_CAPACITY];
    let mut live_mask = 0u32;
    let mut alpha_mask = 0u32;
    let mut scaler_mask = 0u32;
    for (index, entry) in batch.entries[..batch.len].iter().flatten().enumerate() {
        live[index] = crate::intel::mmio_read(dev, entry.plane_base + UNI_PLANE_SURFLIVE_OFF);
        if live[index] == entry.surface_reg {
            live_mask |= 1u32 << index;
        }
        if entry
            .constant_alpha
            .is_none_or(|alpha| overlay_plane_constant_alpha_matches(dev, entry.plane_base, alpha))
        {
            alpha_mask |= 1u32 << index;
        }
        if entry
            .scaler
            .is_none_or(|scaler| plane_scaler_flip_matches(dev, scaler))
        {
            scaler_mask |= 1u32 << index;
        }
    }
    let want_mask = if batch.len == 0 {
        0
    } else {
        (1u32 << batch.len) - 1
    };
    let elapsed_ns = crate::chronos::monotonic_nanos().saturating_sub(batch.submitted_ns);
    let committed = live_mask == want_mask && alpha_mask == want_mask && scaler_mask == want_mask;
    let timed_out = !committed && elapsed_ns >= UI4_PLANE_SURFACE_FLIP_TIMEOUT_NS;
    if !committed && !timed_out {
        return Ui4PlaneSurfaceFlipPoll::Pending;
    }

    let seq = UI4_PLANE_SURFACE_FLIP_BATCH_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    if timed_out || seq <= 8 || seq.is_multiple_of(60) {
        crate::log!(
            "intel/display: ui4-plane-surface-flip-batch seq={} ok={} planes={} live_mask=0x{:X} alpha_mask=0x{:X} scaler_mask=0x{:X} want_mask=0x{:X} polls={} wait_ns={} commit=surface+plane-alpha+scaler-together wait=async\n",
            seq,
            committed as u8,
            batch.len,
            live_mask,
            alpha_mask,
            scaler_mask,
            want_mask,
            batch.polls,
            elapsed_ns,
        );
    }
    if timed_out {
        for (index, entry) in batch.entries[..batch.len].iter().flatten().enumerate() {
            let alpha_ok = entry.constant_alpha.is_none_or(|alpha| {
                overlay_plane_constant_alpha_matches(dev, entry.plane_base, alpha)
            });
            let scaler_ok = entry
                .scaler
                .is_none_or(|scaler| plane_scaler_flip_matches(dev, scaler));
            if live[index] != entry.surface_reg || !alpha_ok || !scaler_ok {
                crate::log_warn!(
                    target: "intel/display";
                    "intel/display: ui4-plane-surface-flip timeout plane_base=0x{:X} surf=0x{:08X} live=0x{:08X} plane_opacity={} alpha_ok={} scaler_ok={}\n",
                    entry.plane_base,
                    entry.surface_reg,
                    live[index],
                    entry.constant_alpha.map_or(u8::MAX, |alpha| alpha),
                    alpha_ok as u8,
                    scaler_ok as u8,
                );
            }
        }
    }
    let polls = batch.polls;
    *batch = PlaneSurfaceFlipBatch::new();
    if committed {
        Ui4PlaneSurfaceFlipPoll::Complete {
            wait_ns: elapsed_ns,
            polls,
        }
    } else {
        Ui4PlaneSurfaceFlipPoll::Failed
    }
}

/// Compatibility wrapper for display clients not yet moved to a task-level
/// state machine. UI4's compositor service uses submit+poll directly.
pub(crate) fn finish_ui4_plane_surface_flip_batch() -> bool {
    if !submit_ui4_plane_surface_flip_batch() {
        return false;
    }
    loop {
        match poll_ui4_plane_surface_flip_batch() {
            Ui4PlaneSurfaceFlipPoll::Pending => core::hint::spin_loop(),
            Ui4PlaneSurfaceFlipPoll::Complete { .. } => return true,
            Ui4PlaneSurfaceFlipPoll::Failed => return false,
        }
    }
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

fn overlay_plane_constant_alpha_matches(
    dev: crate::intel::Dev,
    plane_base: usize,
    alpha: u8,
) -> bool {
    crate::intel::mmio_read(dev, plane_base + UNI_PLANE_KEYMSK_OFF) == plane_keymsk_alpha(alpha)
        && crate::intel::mmio_read(dev, plane_base + UNI_PLANE_KEYMAX_OFF)
            == plane_keymax_alpha(alpha)
}

fn overlay_plane_constant_alpha_is_valid(dev: crate::intel::Dev, plane_base: usize) -> bool {
    let keymax = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_KEYMAX_OFF);
    let alpha = ((keymax & PLANE_KEYMAX_ALPHA_MASK) >> 24) as u8;
    keymax == plane_keymax_alpha(alpha)
        && crate::intel::mmio_read(dev, plane_base + UNI_PLANE_KEYMSK_OFF)
            == plane_keymsk_alpha(alpha)
}

fn program_overlay_plane_constant_alpha(dev: crate::intel::Dev, plane_base: usize, alpha: u8) {
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_KEYMSK_OFF, plane_keymsk_alpha(alpha));
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_KEYMAX_OFF, plane_keymax_alpha(alpha));
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

/// Hand Slot0 to UI4 as an enabled, initially empty premultiplied-RGBA plane.
/// Boot-phase XRGB pixels are deliberately discarded: the pipe bottom color is
/// the permanent base beneath UI4's independently populated plane slices.
pub(crate) fn activate_ui4_application_rgba_planes() -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let Some(primary) = active_primary_surface() else {
        return false;
    };
    let pipe = primary.pipe;
    if overlay_plane_dynamic_flip_guard(
        dev,
        pipe,
        crate::ui4::PRIMARY_PLANE_SLOT,
        UI4_RGBA8_OVERLAY_CONTRACT,
    )
    .is_ok()
    {
        return true;
    }
    if !ui4_rgba8_plane_stack_ready(pipe) || primary.virt.is_null() {
        return false;
    }

    // Slot0 remains enabled. Transparent pixels make it an empty slice rather
    // than an opaque imitation of a background.
    unsafe { core::ptr::write_bytes(primary.virt, 0, primary.byte_len) };
    crate::intel::dma_flush(primary.virt, primary.byte_len);

    let plane = pipe.plane(crate::ui4::PRIMARY_PLANE_SLOT);
    let ctl_before = crate::intel::mmio_read(dev, plane.ctl());
    let ctl = overlay_plane_ctl_enabled(ctl_before, UI4_RGBA8_OVERLAY_CONTRACT);
    let color_ctl_off = plane.base() + UNI_PLANE_COLOR_CTL_OFF;
    let color_ctl = plane_color_ctl_alpha(
        crate::intel::mmio_read(dev, color_ctl_off),
        UI4_RGBA8_OVERLAY_CONTRACT,
    );
    let surface = crate::intel::mmio_read(dev, plane.surf());
    crate::intel::mmio_write(dev, plane.ctl(), ctl & !PLANE_CTL_ENABLE);
    crate::intel::mmio_write(dev, color_ctl_off, color_ctl);
    crate::intel::mmio_write(dev, plane.base() + UNI_PLANE_KEYVAL_OFF, 0);
    program_overlay_plane_constant_alpha(dev, plane.base(), u8::MAX);
    crate::intel::mmio_write(dev, plane.ctl(), ctl);
    crate::intel::mmio_write(dev, plane.surf(), surface);
    let (frame_before, frame_after, frame_wait) = wait_for_pipe_next_frame(dev, pipe);
    let (live, live_iters) = wait_for_plane_live_for(dev, plane.base(), surface, 5_000_000);
    let ready = live == surface
        && overlay_plane_dynamic_flip_guard(
            dev,
            pipe,
            crate::ui4::PRIMARY_PLANE_SLOT,
            UI4_RGBA8_OVERLAY_CONTRACT,
        )
        .is_ok();
    crate::log_info!(target: "ui4";
        "ui4/application-plane-stack rgba_handoff={} pipe={} slots=0-3/premultiplied-rgba8 slot4=interaction-only boot_primary=discarded slot0_initial=transparent pipe_bottom=permanent-base cpu_blend=0 frame={}=>{} frame_wait={} surf=0x{:08X} live=0x{:08X} live_iters={}\n",
        ready as u8,
        pipe.name,
        frame_before,
        frame_after,
        frame_wait,
        surface,
        live,
        live_iters,
    );
    ready
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
/// owned by UI4 interaction. Bootstrap fixes the pixel format, blend mode and
/// DBUF allocation. A direct-present transaction may still change geometry,
/// constant plane alpha and PLANE_SURF, with SURFLIVE proving the new surface.
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
    // Publish the driver-local readiness stores before UI4's hardware-neutral
    // descriptor. Consumers use that descriptor as the final cross-subsystem
    // readiness signal, so observing it must imply the display stack is ready.
    UI4_RGBA8_PLANE_STACK_PIPE_SLOT.store(pipe.slot as u32, Ordering::Release);
    UI4_RGBA8_PLANE_STACK_STATE.store(UI4_RGBA8_PLANE_STACK_READY, Ordering::Release);
    let output = crate::ui4::OutputId::from_slot(0).expect("static UI4 D01 output");
    let application_plane_mask = (1u8 << crate::ui4::INTERACTION_OVERLAY_PLANE_SLOT) - 1;
    if let Err(reason) =
        crate::ui4::publish_ui4_output_capabilities(crate::ui4::Ui4OutputCapabilities {
            output,
            width: primary.width,
            height: primary.height,
            application_plane_mask,
        })
    {
        return fail(reason);
    }
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
        0 => Some(&OVERLAY_SURFACES_SLOT_0[pipe.slot]),
        1 => Some(&OVERLAY_SURFACES_SLOT_1[pipe.slot]),
        2 => Some(&OVERLAY_SURFACES_SLOT_2[pipe.slot]),
        3 => Some(&OVERLAY_SURFACES_SLOT_3[pipe.slot]),
        4 => Some(&OVERLAY_SURFACES_SLOT_4[pipe.slot]),
        _ => None,
    }
}

fn ui4_direct_scanout_pool(
    pipe: PipeInfo,
    plane_slot: usize,
) -> Option<&'static Mutex<Ui4DirectScanoutPool>> {
    match plane_slot {
        0 => Some(&UI4_DIRECT_SCANOUT_SLOT_0[pipe.slot]),
        1 => Some(&UI4_DIRECT_SCANOUT_SLOT_1[pipe.slot]),
        2 => Some(&UI4_DIRECT_SCANOUT_SLOT_2[pipe.slot]),
        3 => Some(&UI4_DIRECT_SCANOUT_SLOT_3[pipe.slot]),
        _ => None,
    }
}

fn ui4_direct_scanout_gpu_for_alias(
    pipe: PipeInfo,
    plane_slot: usize,
    alias_index: usize,
) -> Option<u64> {
    let plane_index = plane_slot;
    if plane_index >= UI4_DIRECT_SCANOUT_PLANE_COUNT
        || alias_index >= UI4_DIRECT_SCANOUT_ALIAS_COUNT
    {
        return None;
    }
    UI4_DIRECT_SCANOUT_GPU_BASE
        .checked_add((plane_index as u64).checked_mul(UI4_DIRECT_SCANOUT_PLANE_STRIDE)?)?
        .checked_add((pipe.slot as u64).checked_mul(UI4_DIRECT_SCANOUT_PIPE_STRIDE)?)?
        .checked_add((alias_index as u64).checked_mul(UI4_DIRECT_SCANOUT_GPU_STRIDE)?)
}

fn primary_swap_surface_pool(pipe: PipeInfo) -> &'static Mutex<PrimarySwapSurfacePool> {
    &PRIMARY_SWAP_SURFACES[pipe.slot]
}

fn overlay_surface_gpu_for_index(pipe: PipeInfo, plane_slot: usize, index: usize) -> Option<u64> {
    if plane_slot == crate::ui4::PRIMARY_PLANE_SLOT {
        if index >= OVERLAY_SWAP_BUFFER_COUNT {
            return None;
        }
        return UI4_SLOT0_OVERLAY_GPU_BASE
            .checked_add((pipe.slot as u64).checked_mul(OVERLAY_PIPE_GPU_STRIDE)?)?
            .checked_add((index as u64).checked_mul(OVERLAY_SWAP_GPU_STRIDE)?);
    }
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

fn primary_compose_rcs_gpu_for_surface(surface: PrimarySwapSurface) -> Option<u64> {
    if surface.buffer_index >= PRIMARY_SWAP_BUFFER_COUNT {
        return None;
    }
    PRIMARY_COMPOSE_RCS_GPU_ALIAS_BASE
        .checked_add((surface.buffer_index as u64).checked_mul(COMPOSE_RCS_GPU_ALIAS_BYTES)?)
}

fn overlay_compose_rcs_gpu_for_surface(surface: OverlaySurface) -> Option<u64> {
    if surface.plane_slot == crate::ui4::PRIMARY_PLANE_SLOT {
        if surface.buffer_index >= OVERLAY_SWAP_BUFFER_COUNT {
            return None;
        }
        return PRIMARY_COMPOSE_RCS_GPU_ALIAS_BASE
            .checked_add((surface.buffer_index as u64).checked_mul(COMPOSE_RCS_GPU_ALIAS_BYTES)?);
    }
    let plane_index = surface.plane_slot.checked_sub(1)?;
    if plane_index >= DIRECT_RCS_OVERLAY_UNIVERSAL_PLANE_COUNT
        || surface.buffer_index >= OVERLAY_SWAP_BUFFER_COUNT
    {
        return None;
    }
    OVERLAY_COMPOSE_RCS_GPU_ALIAS_BASE
        .checked_add((plane_index as u64).checked_mul(OVERLAY_COMPOSE_RCS_GPU_PLANE_STRIDE)?)?
        .checked_add((surface.buffer_index as u64).checked_mul(COMPOSE_RCS_GPU_ALIAS_BYTES)?)
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
        pool.content_initialized[surface.buffer_index] = true;
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

fn mark_overlay_surface_content_initialized(surface: OverlaySurface) {
    let Some(surface_pool) = overlay_surface_pool(surface.pipe, surface.plane_slot) else {
        return;
    };
    let mut pool = surface_pool.lock();
    if pool.matches(surface.width, surface.height, surface.pipe)
        && pool
            .surfaces
            .get(surface.buffer_index)
            .copied()
            .flatten()
            .is_some_and(|owned| owned.gpu == surface.gpu)
    {
        pool.content_initialized[surface.buffer_index] = true;
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
        pool.composited[surface.buffer_index] = true;
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
    pool.composited[surface.buffer_index] = true;
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

fn restore_primary_composition_base_outside_opaque_rect(
    back: PrimarySwapSurface,
    damage: CompositionDamageRegion,
    opaque: CompositionDamageRect,
) -> Option<CompositionDamageRegion> {
    let already_composited = {
        let pool = primary_swap_surface_pool(back.pipe).lock();
        if !pool.matches(back.width, back.height, back.pipe) {
            return None;
        }
        pool.composited[back.buffer_index]
    };
    if !already_composited {
        // A newly allocated swap surface is already an exact base copy.
        return Some(CompositionDamageRegion::EMPTY);
    }

    let mut restored = CompositionDamageRegion::EMPTY;
    for damaged in damage.rects().iter().copied() {
        let Some(covered) = damaged.intersection(opaque) else {
            if !restore_primary_composition_base_rect(back, damaged) {
                return None;
            }
            restored.add(damaged);
            continue;
        };
        let damaged_right = damaged.x.saturating_add(damaged.width);
        let damaged_bottom = damaged.y.saturating_add(damaged.height);
        let covered_right = covered.x.saturating_add(covered.width);
        let covered_bottom = covered.y.saturating_add(covered.height);
        let pieces = [
            CompositionDamageRect::new(
                damaged.x,
                damaged.y,
                damaged.width,
                covered.y.saturating_sub(damaged.y),
            ),
            CompositionDamageRect::new(
                damaged.x,
                covered_bottom,
                damaged.width,
                damaged_bottom.saturating_sub(covered_bottom),
            ),
            CompositionDamageRect::new(
                damaged.x,
                covered.y,
                covered.x.saturating_sub(damaged.x),
                covered.height,
            ),
            CompositionDamageRect::new(
                covered_right,
                covered.y,
                damaged_right.saturating_sub(covered_right),
                covered.height,
            ),
        ];
        for piece in pieces.into_iter().filter(|piece| piece.valid()) {
            if !restore_primary_composition_base_rect(back, piece) {
                return None;
            }
            restored.add(piece);
        }
    }
    Some(restored)
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
                content_initialized: [false; OVERLAY_SWAP_BUFFER_COUNT],
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
    let seeded_from_primary = primary_surface_for_pipe(pipe)
        .filter(|primary| {
            !primary.virt.is_null()
                && primary.width == width
                && primary.height == height
                && primary.pitch_bytes == pitch_bytes
                // The boot surface retains guard rows below the visible mode.
                // Its visible prefix is nevertheless byte-for-byte compatible
                // with the smaller scanout swap allocation.
                && primary.byte_len >= byte_len
        })
        .map(|primary| unsafe {
            core::ptr::copy_nonoverlapping(primary.virt, virt, byte_len);
        })
        .is_some();
    if !seeded_from_primary {
        fill_surface_color(virt, pitch_bytes as usize, width, height, 0);
    }
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
                composited: [false; PRIMARY_SWAP_BUFFER_COUNT],
            };
        }
        pool.surfaces[buffer_index] = Some(surface);
    }
    crate::log!(
        "intel/display: primary-swap-surface pipe={} buffer={} size={}x{} pitch=0x{:X} bytes=0x{:X} gpu=0x{:X} phys=0x{:X} seeded_from_primary={}\n",
        pipe.name,
        buffer_index,
        width,
        height,
        pitch_bytes,
        byte_len,
        gpu,
        phys,
        seeded_from_primary as u8,
    );
    Some(surface)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum GpgpuCompositionResult {
    Unavailable,
    Complete,
    SubmittedIncomplete,
    Queued(crate::intel::gpgpu::Ui4CompositorSubmission),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui4AsyncCompositionError {
    Unavailable,
    Busy,
    Failed,
}

#[derive(Copy, Clone)]
enum Ui4AsyncCompositionTarget {
    Primary {
        surface: PrimarySwapSurface,
        pipeline: DisplayPipelineId,
    },
    Overlay {
        surface: OverlaySurface,
        /// Empty application planes stay parked at zero hardware opacity.
        /// This prevents a retiring direct surface from becoming visible if
        /// plane alpha updates before the transparent SURF latch.
        constant_alpha: u8,
    },
    DirectOverlay {
        surface: Ui4DirectOverlaySurface,
    },
}

#[derive(Copy, Clone)]
enum Ui4AsyncCompositionWork {
    GucRcs(crate::intel::gpgpu::Ui4CompositorSubmission),
    GucBcs(crate::intel::GucBcs0CopySubmission),
}

/// Display-owned record for either a GPU composition or a zero-work direct
/// import whose destination is the next plane surface. UI4 retains producer
/// leases until the resulting SURFLIVE transition completes.
#[derive(Copy, Clone)]
pub(crate) struct Ui4AsyncComposition {
    work: Option<Ui4AsyncCompositionWork>,
    target: Ui4AsyncCompositionTarget,
    proof: Option<Ui4CompositionProof>,
    change: CompositionDamageRegion,
    effective: CompositionDamageRegion,
    tile_count: usize,
    queued_ns: u64,
    reason: &'static str,
}

#[derive(Copy, Clone)]
struct Ui4CompositionProof {
    sequence: u64,
    x: u32,
    y: u32,
    source_x: u32,
    source_y: u32,
    source_gpu: u64,
    source_rgba: u32,
    expected_destination: u32,
}

static UI4_COMPOSITION_PROOF_SEQUENCE: AtomicU64 = AtomicU64::new(0);
// Direct scanout is the ordinary resident-scene frame path, so lifecycle proof logs
// must not become per-frame work. Keep enough checkpoints to diagnose buffer
// rotation and release ordering without feeding thousands of formatted lines
// through the kernel logger during a performance run.
static UI4_DIRECT_QUEUE_LOG_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static UI4_DIRECT_SCANOUT_LOG_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static UI4_DIRECT_PLANE_ALPHA_LOGGED: AtomicBool = AtomicBool::new(false);
static UI4_DIRECT_PLANE_SCALER_LOGGED: AtomicBool = AtomicBool::new(false);
static UI4_EMPTY_OVERLAY_CPU_PARK_LOGGED: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone)]
struct Ui4DirectScanoutProof {
    producer_frame: u64,
    publish_serial: u64,
}

const UI4_DIRECT_SCANOUT_PROOF_EMPTY: Ui4DirectScanoutProof = Ui4DirectScanoutProof {
    producer_frame: 0,
    publish_serial: 0,
};
static UI4_DIRECT_SCANOUT_PROOFS: Mutex<[Ui4DirectScanoutProof; 5]> =
    Mutex::new([UI4_DIRECT_SCANOUT_PROOF_EMPTY; 5]);

const fn overlay_composition_constant_alpha(tile_count: usize, all_tiles_transparent: bool) -> u8 {
    if tile_count == 0 || all_tiles_transparent {
        0
    } else {
        u8::MAX
    }
}

pub(crate) fn ui4_direct_scanout_ready_for_frame(producer_frame: u64) -> Option<u64> {
    (producer_frame != 0)
        .then(|| {
            UI4_DIRECT_SCANOUT_PROOFS
                .lock()
                .iter()
                .filter(|proof| proof.producer_frame == producer_frame)
                .map(|proof| proof.publish_serial)
                .max()
        })
        .flatten()
}

const fn should_log_ui4_direct_checkpoint(sequence: u64) -> bool {
    sequence <= 8 || sequence.is_multiple_of(120)
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum Ui4AsyncCompositionPoll {
    Pending,
    Ready,
    Failed,
}

pub(crate) fn queue_ui4_primary_composition(
    tiles: &[RgbaOverlayTile<'_>],
    damage: CompositionDamageRegion,
    reason: &'static str,
) -> Result<Ui4AsyncComposition, Ui4AsyncCompositionError> {
    let dev = crate::intel::claimed_device().ok_or(Ui4AsyncCompositionError::Unavailable)?;
    let target = active_display_pipeline_target().ok_or(Ui4AsyncCompositionError::Unavailable)?;
    let pipe = target
        .pipeline
        .pipe()
        .ok_or(Ui4AsyncCompositionError::Unavailable)?;
    if target.width == 0 || target.height == 0 {
        return Err(Ui4AsyncCompositionError::Unavailable);
    }
    let change = clip_composition_damage_region(damage, target.width, target.height);
    if change.is_empty() {
        return Err(Ui4AsyncCompositionError::Unavailable);
    }
    let surface = ensure_primary_swap_surface_for_pipe(dev, pipe, target.width, target.height)
        .ok_or(Ui4AsyncCompositionError::Unavailable)?;
    let effective = {
        let pool = primary_swap_surface_pool(surface.pipe).lock();
        let mut effective = pool.damage_debt[surface.buffer_index];
        effective.add_region(change);
        effective
    };
    let proof = prepare_ui4_composition_proof(tiles, effective, target.width, target.height, true);
    match compose_premultiplied_rgba_tiles_into_primary_gpgpu(surface, tiles, effective, true) {
        GpgpuCompositionResult::Queued(gpu) => Ok(Ui4AsyncComposition {
            work: Some(Ui4AsyncCompositionWork::GucRcs(gpu)),
            target: Ui4AsyncCompositionTarget::Primary {
                surface,
                pipeline: target.pipeline,
            },
            proof,
            change,
            effective,
            tile_count: tiles.len(),
            queued_ns: crate::chronos::monotonic_nanos(),
            reason,
        }),
        GpgpuCompositionResult::SubmittedIncomplete => Err(Ui4AsyncCompositionError::Busy),
        _ => Err(Ui4AsyncCompositionError::Failed),
    }
}

pub(crate) fn queue_ui4_overlay_composition(
    plane_slot: usize,
    tiles: &[RgbaOverlayTile<'_>],
    damage: CompositionDamageRegion,
    sparse_static_painter: bool,
    reason: &'static str,
) -> Result<Ui4AsyncComposition, Ui4AsyncCompositionError> {
    let dev = crate::intel::claimed_device().ok_or(Ui4AsyncCompositionError::Unavailable)?;
    let (width, height) = active_scanout_dimensions()
        .or_else(|| active_primary_surface().map(|primary| (primary.width, primary.height)))
        .ok_or(Ui4AsyncCompositionError::Unavailable)?;
    let change = clip_composition_damage_region(damage, width, height);
    if change.is_empty() {
        return Err(Ui4AsyncCompositionError::Unavailable);
    }
    let surface = ensure_overlay_surface_on_slot(dev, plane_slot, width, height)
        .ok_or(Ui4AsyncCompositionError::Unavailable)?;
    let (effective, destination_fresh_transparent) = {
        let surface_pool = overlay_surface_pool(surface.pipe, surface.plane_slot)
            .ok_or(Ui4AsyncCompositionError::Unavailable)?;
        let pool = surface_pool.lock();
        let mut effective = pool.damage_debt[surface.buffer_index];
        effective.add_region(change);
        (effective, !pool.content_initialized[surface.buffer_index])
    };
    if tiles.is_empty() {
        // Teardown must remain available after an application has quarantined
        // direct RCS. Park a known-transparent display-owned surface using a
        // one-shot CPU clear, then latch it at hardware alpha zero. Publishing
        // full damage also forces the other swap surface to be cleared before
        // it can be reused by a later application.
        let full = CompositionDamageRegion::from_rect(CompositionDamageRect::new(
            0,
            0,
            surface.width,
            surface.height,
        ));
        if !destination_fresh_transparent {
            fill_overlay_rect(surface, 0, 0, surface.width, surface.height, 0);
            if !dma_flush_overlay_region(surface, full) {
                return Err(Ui4AsyncCompositionError::Failed);
            }
        }
        mark_overlay_surface_content_initialized(surface);
        if !UI4_EMPTY_OVERLAY_CPU_PARK_LOGGED.swap(true, Ordering::AcqRel) {
            crate::log_info!(target: "ui4";
                "ui4/empty-plane-park: backend=cpu-transparent-clear plane_slot={} surface={}x{} fresh={} hardware_opacity=0 rcs_dependency=none damage=full log=once\n",
                plane_slot,
                surface.width,
                surface.height,
                destination_fresh_transparent as u8,
            );
        }
        return Ok(Ui4AsyncComposition {
            work: None,
            target: Ui4AsyncCompositionTarget::Overlay {
                surface,
                constant_alpha: 0,
            },
            proof: None,
            change: full,
            effective: full,
            tile_count: 0,
            queued_ns: crate::chronos::monotonic_nanos(),
            reason,
        });
    }
    let proof =
        prepare_ui4_composition_proof(tiles, effective, surface.width, surface.height, false);
    let constant_alpha =
        overlay_composition_constant_alpha(tiles.len(), tiles.iter().all(|tile| tile.opacity == 0));
    let content_change = if sparse_static_painter && destination_fresh_transparent {
        let mut painted = CompositionDamageRegion::EMPTY;
        for tile in tiles {
            if let Some(rect) = clip_composition_damage(
                CompositionDamageRect::new(tile.x, tile.y, tile.width, tile.height),
                surface.width,
                surface.height,
            ) {
                painted.add(rect);
            }
        }
        painted
    } else {
        change
    };
    match compose_premultiplied_rgba_tiles_into_overlay_gpgpu(
        surface,
        tiles,
        effective,
        true,
        sparse_static_painter,
        destination_fresh_transparent,
    ) {
        GpgpuCompositionResult::Queued(gpu) => {
            // The accepted GPU request now owns this destination. A later
            // flip retry must no longer treat it as the pristine zero-filled
            // allocation even though it has not become scanout front yet.
            mark_overlay_surface_content_initialized(surface);
            Ok(Ui4AsyncComposition {
                work: Some(Ui4AsyncCompositionWork::GucRcs(gpu)),
                target: Ui4AsyncCompositionTarget::Overlay {
                    surface,
                    constant_alpha,
                },
                proof,
                // A pristine destination was already transparent outside the
                // static rectangles. Only those painted rectangles become
                // debt on the other swap buffer, not the broker's conservative
                // first-scene fullscreen invalidation.
                change: content_change,
                effective,
                tile_count: tiles.len(),
                queued_ns: crate::chronos::monotonic_nanos(),
                reason,
            })
        }
        GpgpuCompositionResult::SubmittedIncomplete => Err(Ui4AsyncCompositionError::Busy),
        _ => Err(Ui4AsyncCompositionError::Failed),
    }
}

/// First activation of the GuC BCS0 Frame painter.
///
/// This path is intentionally narrower than the CPU baseline: a pristine
/// transparent destination needs no clear, and immutable 1:1 rectangles can
/// be emitted as one ordered XY_FAST_COPY_BLT batch. Broker order therefore
/// retains the temporary "later rectangle paints over earlier rectangle"
/// rule for same-slot overlap. Once a swap buffer contains old pixels, the
/// caller keeps using the CPU path until a BCS-native clear is proven.
pub(crate) fn queue_ui4_static_overlay_composition_bcs0(
    plane_slot: usize,
    tiles: &[RgbaOverlayTile<'_>],
    damage: CompositionDamageRegion,
    reason: &'static str,
) -> Result<Ui4AsyncComposition, Ui4AsyncCompositionError> {
    let dev = crate::intel::claimed_device().ok_or(Ui4AsyncCompositionError::Unavailable)?;
    let (width, height) = active_scanout_dimensions()
        .or_else(|| active_primary_surface().map(|primary| (primary.width, primary.height)))
        .ok_or(Ui4AsyncCompositionError::Unavailable)?;
    let change = clip_composition_damage_region(damage, width, height);
    if change.is_empty() || tiles.is_empty() {
        return Err(Ui4AsyncCompositionError::Unavailable);
    }
    let surface = ensure_overlay_surface_on_slot(dev, plane_slot, width, height)
        .ok_or(Ui4AsyncCompositionError::Unavailable)?;
    let destination_fresh_transparent = {
        let surface_pool = overlay_surface_pool(surface.pipe, surface.plane_slot)
            .ok_or(Ui4AsyncCompositionError::Unavailable)?;
        let pool = surface_pool.lock();
        !pool.content_initialized[surface.buffer_index]
    };
    if !destination_fresh_transparent {
        return Err(Ui4AsyncCompositionError::Unavailable);
    }

    let mut painted = CompositionDamageRegion::EMPTY;
    let mut copies = Vec::with_capacity(tiles.len());
    for tile in tiles {
        if tile.opacity != u8::MAX
            || tile.width != tile.source_width
            || tile.height != tile.source_height
        {
            return Err(Ui4AsyncCompositionError::Unavailable);
        }
        let source = tile
            .gpgpu_surface
            .filter(|source| source.is_valid())
            .ok_or(Ui4AsyncCompositionError::Unavailable)?;
        let Some(draw) = clip_composition_damage(
            CompositionDamageRect::new(tile.x, tile.y, tile.width, tile.height),
            surface.width,
            surface.height,
        ) else {
            continue;
        };
        painted.add(draw);
        copies.push(crate::intel::GucBcs0RgbaCopy {
            source: crate::intel::GucBcs0RgbaSurface {
                phys: source.phys,
                gpu: source.gpu,
                bytes: source.bytes,
                width: source.width,
                height: source.height,
                pitch_bytes: source.pitch_bytes,
            },
            source_x: draw.x.saturating_sub(tile.x),
            source_y: draw.y.saturating_sub(tile.y),
            destination_x: draw.x,
            destination_y: draw.y,
            width: draw.width,
            height: draw.height,
        });
    }
    if painted.is_empty() || copies.is_empty() {
        return Err(Ui4AsyncCompositionError::Unavailable);
    }
    let destination_gpu = overlay_compose_rcs_gpu_for_surface(surface)
        .ok_or(Ui4AsyncCompositionError::Unavailable)?;
    let destination = crate::intel::GucBcs0RgbaSurface {
        phys: surface.phys,
        gpu: destination_gpu,
        bytes: surface.byte_len,
        width: surface.width,
        height: surface.height,
        pitch_bytes: surface.pitch_bytes,
    };
    let blit =
        crate::intel::queue_guc_bcs0_rgba_copies(destination, &copies).map_err(
            |error| match error {
                crate::intel::GucBcs0CopySubmitError::Busy => Ui4AsyncCompositionError::Busy,
                crate::intel::GucBcs0CopySubmitError::Unavailable => {
                    Ui4AsyncCompositionError::Unavailable
                }
                crate::intel::GucBcs0CopySubmitError::InvalidRequest
                | crate::intel::GucBcs0CopySubmitError::SubmitFailed => {
                    Ui4AsyncCompositionError::Failed
                }
            },
        )?;
    mark_overlay_surface_content_initialized(surface);
    Ok(Ui4AsyncComposition {
        work: Some(Ui4AsyncCompositionWork::GucBcs(blit)),
        target: Ui4AsyncCompositionTarget::Overlay {
            surface,
            constant_alpha: u8::MAX,
        },
        proof: None,
        change: painted,
        effective: painted,
        tile_count: tiles.len(),
        queued_ns: crate::chronos::monotonic_nanos(),
        reason,
    })
}

/// Correctness-first composition backend for immutable single-buffer Frames.
///
/// This deliberately contains no shader, GuC submission, source binding-table
/// switch, marker, or CPU readback. Producer pixels are painted directly into
/// the display-owned back buffer over slot-local damage, after which the
/// ordinary UI4 batched plane flip owns presentation.
pub(crate) fn queue_ui4_static_overlay_composition_cpu(
    plane_slot: usize,
    tiles: &[RgbaOverlayTile<'_>],
    damage: CompositionDamageRegion,
    reason: &'static str,
) -> Result<Ui4AsyncComposition, Ui4AsyncCompositionError> {
    let dev = crate::intel::claimed_device().ok_or(Ui4AsyncCompositionError::Unavailable)?;
    let (width, height) = active_scanout_dimensions()
        .or_else(|| active_primary_surface().map(|primary| (primary.width, primary.height)))
        .ok_or(Ui4AsyncCompositionError::Unavailable)?;
    let change = clip_composition_damage_region(damage, width, height);
    if change.is_empty() || tiles.is_empty() {
        return Err(Ui4AsyncCompositionError::Unavailable);
    }
    let surface = ensure_overlay_surface_on_slot(dev, plane_slot, width, height)
        .ok_or(Ui4AsyncCompositionError::Unavailable)?;
    let (effective, destination_fresh_transparent) = {
        let surface_pool = overlay_surface_pool(surface.pipe, surface.plane_slot)
            .ok_or(Ui4AsyncCompositionError::Unavailable)?;
        let pool = surface_pool.lock();
        let mut effective = pool.damage_debt[surface.buffer_index];
        effective.add_region(change);
        (effective, !pool.content_initialized[surface.buffer_index])
    };

    // A fresh allocation was zero-filled and flushed before mapping. Only the
    // rectangles painted now differ from that transparent base; carrying the
    // broker's conservative first-scene fullscreen damage into the other back
    // buffer would recreate the very avalanche this baseline is meant to cut.
    let mut painted = CompositionDamageRegion::EMPTY;
    for tile in tiles {
        if let Some(rect) = clip_composition_damage(
            CompositionDamageRect::new(tile.x, tile.y, tile.width, tile.height),
            surface.width,
            surface.height,
        ) {
            painted.add(rect);
        }
    }
    if painted.is_empty() {
        return Err(Ui4AsyncCompositionError::Unavailable);
    }
    let work_damage = if destination_fresh_transparent {
        painted
    } else {
        effective
    };

    for damaged in work_damage.rects() {
        if !destination_fresh_transparent {
            fill_overlay_rect(surface, damaged.x, damaged.y, damaged.width, damaged.height, 0);
        }
        for tile in tiles {
            copy_premultiplied_rgba_tile_into_overlay_clipped(surface, tile, *damaged)
                .ok_or(Ui4AsyncCompositionError::Failed)?;
        }
    }
    if !dma_flush_overlay_region(surface, work_damage) {
        return Err(Ui4AsyncCompositionError::Failed);
    }
    mark_overlay_surface_content_initialized(surface);

    Ok(Ui4AsyncComposition {
        work: None,
        target: Ui4AsyncCompositionTarget::Overlay {
            surface,
            constant_alpha: overlay_composition_constant_alpha(
                tiles.len(),
                tiles.iter().all(|tile| tile.opacity == 0),
            ),
        },
        proof: None,
        change: if destination_fresh_transparent {
            painted
        } else {
            change
        },
        effective: work_damage,
        tile_count: tiles.len(),
        queued_ns: crate::chronos::monotonic_nanos(),
        reason,
    })
}

/// Import one already-rendered UI4 frame into a display-owned GGTT alias.
/// This creates presentation state only: there is deliberately no GuC/RCS
/// submission to poll before the plane flip can be staged.
pub(crate) fn queue_ui4_direct_overlay_frame(
    plane_slot: usize,
    source: Ui4DirectRgbaFrame,
    pos_x: u32,
    pos_y: u32,
    dest_width: u32,
    dest_height: u32,
    opacity: u8,
    reason: &'static str,
) -> Result<Ui4AsyncComposition, Ui4AsyncCompositionError> {
    let dev = crate::intel::claimed_device().ok_or(Ui4AsyncCompositionError::Unavailable)?;
    let target = active_display_pipeline_target().ok_or(Ui4AsyncCompositionError::Unavailable)?;
    let pipe = target
        .pipeline
        .pipe()
        .ok_or(Ui4AsyncCompositionError::Unavailable)?;
    if plane_slot >= crate::ui4::INTERACTION_OVERLAY_PLANE_SLOT
        || source.width == 0
        || source.height == 0
        || dest_width == 0
        || dest_height == 0
        || source.phys == 0
        || !source.phys.is_multiple_of(crate::intel::WARM_ALIGN as u64)
        || source.byte_len == 0
        || source.byte_len as u64 > UI4_DIRECT_SCANOUT_GPU_STRIDE
        || plane_stride_reg_value(source.pitch_bytes).is_none()
        || source.pitch_bytes < source.width.saturating_mul(PRIMARY_BYTES_PER_PIXEL)
        || (source.pitch_bytes as usize)
            .checked_mul(source.height as usize)
            .is_none_or(|required| required > source.byte_len)
        || pos_x
            .checked_add(dest_width)
            .is_none_or(|right| right > target.width)
        || pos_y
            .checked_add(dest_height)
            .is_none_or(|bottom| bottom > target.height)
    {
        return Err(Ui4AsyncCompositionError::Unavailable);
    }
    overlay_plane_dynamic_flip_guard(dev, pipe, plane_slot, UI4_RGBA8_OVERLAY_CONTRACT)
        .map_err(|_| Ui4AsyncCompositionError::Unavailable)?;

    let plane_base = overlay_plane_base(pipe, plane_slot);
    let current_surf = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF);
    let current_live = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF);
    let pool_mutex =
        ui4_direct_scanout_pool(pipe, plane_slot).ok_or(Ui4AsyncCompositionError::Unavailable)?;
    let mut pool = pool_mutex.lock();
    let next_alias = pool.next_alias;
    let aliases_from_next = || {
        (0..UI4_DIRECT_SCANOUT_ALIAS_COUNT)
            .map(move |offset| (next_alias + offset) % UI4_DIRECT_SCANOUT_ALIAS_COUNT)
    };
    let alias_is_idle = |index| {
        ui4_direct_scanout_gpu_for_alias(pipe, plane_slot, index)
            .and_then(|gpu| u32::try_from(gpu).ok())
            .is_some_and(|gpu| gpu != current_surf && gpu != current_live)
    };
    let mapping_matches_source = |mapping: Ui4DirectScanoutMapping| {
        mapping.phys == source.phys && mapping.byte_len == source.byte_len
    };
    let alias_index = aliases_from_next()
        // Stable mappings are the normal multi-buffered path. Never choose a
        // live/queued alias even when it already names the requested bytes.
        .find(|index| {
            alias_is_idle(*index) && pool.mappings[*index].is_some_and(mapping_matches_source)
        })
        // A never-used alias is preferable to evicting another resident
        // producer surface during bring-up or after a buffering-mode change.
        .or_else(|| {
            aliases_from_next()
                .find(|index| alias_is_idle(*index) && pool.mappings[*index].is_none())
        })
        // Resize and frame replacement can introduce a fourth allocation.
        // Only then recycle an alias which hardware proves is neither queued
        // in SURF nor latched in SURFLIVE.
        .or_else(|| aliases_from_next().find(|index| alias_is_idle(*index)))
        .ok_or(Ui4AsyncCompositionError::Busy)?;
    let gpu = ui4_direct_scanout_gpu_for_alias(pipe, plane_slot, alias_index)
        .ok_or(Ui4AsyncCompositionError::Unavailable)?;
    if let Some(mapping) = pool.mappings[alias_index] {
        if mapping.phys != source.phys || mapping.byte_len != source.byte_len {
            if !crate::intel::unmap_display_scanout_ggtt(dev, mapping.byte_len, gpu) {
                return Err(Ui4AsyncCompositionError::Failed);
            }
            pool.mappings[alias_index] = None;
        }
    }
    if pool.mappings[alias_index].is_none() {
        if !crate::intel::map_display_scanout_ggtt(dev, source.phys, source.byte_len, gpu) {
            let _ = crate::intel::unmap_display_scanout_ggtt(dev, source.byte_len, gpu);
            return Err(Ui4AsyncCompositionError::Failed);
        }
        crate::intel::ggtt_invalidate(dev);
        pool.mappings[alias_index] = Some(Ui4DirectScanoutMapping {
            phys: source.phys,
            byte_len: source.byte_len,
        });
    }
    pool.next_alias = (alias_index + 1) % UI4_DIRECT_SCANOUT_ALIAS_COUNT;
    drop(pool);

    let surface = Ui4DirectOverlaySurface {
        width: source.width,
        height: source.height,
        dest_width,
        dest_height,
        pitch_bytes: source.pitch_bytes,
        byte_len: source.byte_len,
        phys: source.phys,
        gpu,
        pipe,
        plane_slot,
        alias_index,
        pos_x,
        pos_y,
        opacity,
        producer_frame: source.producer_frame,
        producer_buffer_index: source.producer_buffer_index,
        producer_publish_serial: source.producer_publish_serial,
        producer_release_sequence: source.producer_release_sequence,
    };
    let change = CompositionDamageRegion::from_rect(CompositionDamageRect::new(
        pos_x,
        pos_y,
        dest_width,
        dest_height,
    ));
    let queue_sequence = UI4_DIRECT_QUEUE_LOG_SEQUENCE
        .fetch_add(1, Ordering::Relaxed)
        .saturating_add(1);
    if opacity != u8::MAX && !UI4_DIRECT_PLANE_ALPHA_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log_info!(target: "ui4";
            "ui4/direct-present: hardware-opacity active slot={} producer_frame={} pixel_alpha=premultiplied plane_opacity={} source_buffer_mutation=none alpha_commit=surflive+key-alpha-readback\n",
            plane_slot,
            source.producer_frame,
            opacity,
        );
    }
    if should_log_ui4_direct_checkpoint(queue_sequence) {
        crate::log_trace!(target: "ui4";
            "ui4/direct-present: queued checkpoint={} reason={} slot={} alias={} producer_frame={} producer_buffer={} publish_serial={} producer_release_sequence={} source_phys=0x{:X} display_gpu=0x{:X} source={}x{} destination={}x{}@{},{} pitch=0x{:X} pixel_alpha=premultiplied plane_opacity={} guc_jobs=0\n",
            queue_sequence,
            reason,
            plane_slot,
            alias_index,
            source.producer_frame,
            source.producer_buffer_index,
            source.producer_publish_serial,
            source.producer_release_sequence,
            source.phys,
            gpu,
            source.width,
            source.height,
            dest_width,
            dest_height,
            pos_x,
            pos_y,
            source.pitch_bytes,
            opacity,
        );
    }
    Ok(Ui4AsyncComposition {
        work: None,
        target: Ui4AsyncCompositionTarget::DirectOverlay { surface },
        proof: None,
        change,
        effective: change,
        tile_count: 1,
        queued_ns: crate::chronos::monotonic_nanos(),
        reason,
    })
}

pub(crate) fn poll_ui4_composition(composition: Ui4AsyncComposition) -> Ui4AsyncCompositionPoll {
    match composition.work {
        None => Ui4AsyncCompositionPoll::Ready,
        Some(Ui4AsyncCompositionWork::GucRcs(gpu)) => {
            match crate::intel::gpgpu::poll_ui4_compositor_submission(gpu) {
                crate::intel::gpgpu::Ui4CompositorCompletion::Pending => {
                    Ui4AsyncCompositionPoll::Pending
                }
                crate::intel::gpgpu::Ui4CompositorCompletion::Complete(_) => {
                    verify_ui4_composition_proof(composition);
                    Ui4AsyncCompositionPoll::Ready
                }
                crate::intel::gpgpu::Ui4CompositorCompletion::Failed
                | crate::intel::gpgpu::Ui4CompositorCompletion::InvalidSubmission => {
                    Ui4AsyncCompositionPoll::Failed
                }
            }
        }
        Some(Ui4AsyncCompositionWork::GucBcs(blit)) => {
            match crate::intel::poll_guc_bcs0_rgba_copies(blit) {
                crate::intel::GucBcs0CopyCompletion::Pending => Ui4AsyncCompositionPoll::Pending,
                crate::intel::GucBcs0CopyCompletion::Complete => Ui4AsyncCompositionPoll::Ready,
                crate::intel::GucBcs0CopyCompletion::Failed
                | crate::intel::GucBcs0CopyCompletion::InvalidSubmission => {
                    Ui4AsyncCompositionPoll::Failed
                }
            }
        }
    }
}

fn prepare_ui4_composition_proof(
    tiles: &[RgbaOverlayTile<'_>],
    damage: CompositionDamageRegion,
    target_width: u32,
    target_height: u32,
    destination_xrgb: bool,
) -> Option<Ui4CompositionProof> {
    let sequence = UI4_COMPOSITION_PROOF_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    if sequence > 24 && !sequence.is_power_of_two() {
        return None;
    }

    // The last tile is the topmost ordered layer. Restrict the proof to one of
    // its fully opaque pixels, so the expected result is independent of the
    // destination's previous contents and of every lower layer.
    let tile = tiles.last()?;
    // A released direct-scanout source is GPU-owned PAT3/UC memory. Sampling it
    // for diagnostics on the CPU would reintroduce the cache walk that the
    // producer-release contract removed from the present path.
    if tile.gpgpu_scanout_cache {
        return None;
    }
    if tile.opacity != u8::MAX
        || tile.width == 0
        || tile.height == 0
        || tile.source_width == 0
        || tile.source_height == 0
    {
        return None;
    }
    let source = tile.gpgpu_surface?;
    let tile_rect = CompositionDamageRect::new(tile.x, tile.y, tile.width, tile.height);
    let target_rect = CompositionDamageRect::new(0, 0, target_width, target_height);
    for damaged in damage.rects().iter().rev().copied() {
        let Some(draw) = intersect_composition_damage(tile_rect, damaged)
            .and_then(|rect| intersect_composition_damage(rect, target_rect))
        else {
            continue;
        };
        let candidates = [
            (draw.x.saturating_add(draw.width / 2), draw.y.saturating_add(draw.height / 2)),
            (draw.x, draw.y),
            (draw.x.saturating_add(draw.width.saturating_sub(1)), draw.y),
            (draw.x, draw.y.saturating_add(draw.height.saturating_sub(1))),
            (
                draw.x.saturating_add(draw.width.saturating_sub(1)),
                draw.y.saturating_add(draw.height.saturating_sub(1)),
            ),
        ];
        for (x, y) in candidates {
            let source_x = tile_source_coordinate(
                u64::from(x.saturating_sub(tile.x)),
                tile.source_width,
                tile.width,
            );
            let source_y = tile_source_coordinate(
                u64::from(y.saturating_sub(tile.y)),
                tile.source_height,
                tile.height,
            );
            let source_offset = source_y
                .checked_mul(tile.pitch_bytes)?
                .checked_add(source_x.checked_mul(PRIMARY_BYTES_PER_PIXEL as usize)?)?;
            let pixel = tile
                .pixels
                .get(source_offset..source_offset.saturating_add(4))?;
            let source_ptr = pixel.as_ptr().cast_mut();
            crate::intel::dma_flush(source_ptr, 4);
            let source_rgba = unsafe { core::ptr::read_volatile(source_ptr.cast::<u32>()) };
            if source_rgba >> 24 != u8::MAX as u32 {
                continue;
            }
            let expected_destination = if destination_xrgb {
                ((source_rgba & 0xFF) << 16)
                    | (source_rgba & 0x0000_FF00)
                    | ((source_rgba >> 16) & 0xFF)
            } else {
                source_rgba
            };
            return Some(Ui4CompositionProof {
                sequence,
                x,
                y,
                source_x: source_x as u32,
                source_y: source_y as u32,
                source_gpu: source.gpu,
                source_rgba,
                expected_destination,
            });
        }
    }
    None
}

fn verify_ui4_composition_proof(composition: Ui4AsyncComposition) {
    let Some(proof) = composition.proof else {
        return;
    };
    let (target, slot, buffer, destination_gpu, virt, pitch_bytes, byte_len) =
        match composition.target {
            Ui4AsyncCompositionTarget::Primary { surface, .. } => (
                "primary",
                0usize,
                surface.buffer_index,
                surface.gpu,
                surface.virt,
                surface.pitch_bytes as usize,
                surface.byte_len,
            ),
            Ui4AsyncCompositionTarget::Overlay { surface, .. } => (
                "overlay",
                surface.plane_slot,
                surface.buffer_index,
                surface.gpu,
                surface.virt,
                surface.pitch_bytes as usize,
                surface.byte_len,
            ),
            Ui4AsyncCompositionTarget::DirectOverlay { .. } => return,
        };
    let Some(offset) = (proof.y as usize)
        .checked_mul(pitch_bytes)
        .and_then(|row| row.checked_add(proof.x as usize * PRIMARY_BYTES_PER_PIXEL as usize))
        .filter(|offset| offset.saturating_add(4) <= byte_len)
    else {
        return;
    };
    let destination_ptr = unsafe { virt.add(offset) };
    crate::intel::dma_flush(destination_ptr, 4);
    let observed_destination = unsafe { core::ptr::read_volatile(destination_ptr.cast::<u32>()) };
    crate::log_info!(target: "ui4";
        "ui4/guc-compositor-proof: seq={} reason={} target={} slot={} buffer={} xy={},{} source_xy={},{} source_gpu=0x{:X} destination_gpu=0x{:X} source_rgba=0x{:08X} expected=0x{:08X} observed=0x{:08X} match={} boundary=post-marker-before-flip\n",
        proof.sequence,
        composition.reason,
        target,
        slot,
        buffer,
        proof.x,
        proof.y,
        proof.source_x,
        proof.source_y,
        proof.source_gpu,
        destination_gpu,
        proof.source_rgba,
        proof.expected_destination,
        observed_destination,
        (observed_destination == proof.expected_destination) as u8,
    );
}

/// Stage only the stable SURF address.  Front ownership and damage history are
/// committed later, after the asynchronous SURFLIVE poll proves the latch.
pub(crate) fn stage_ui4_composition_flip(composition: Ui4AsyncComposition) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    match composition.target {
        Ui4AsyncCompositionTarget::Primary { surface, pipeline } => {
            if active_display_pipeline_target().map(|target| target.pipeline) != Some(pipeline) {
                return false;
            }
            program_primary_plane_source_for_pipeline(
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
                pipeline,
                composition.reason,
                true,
            )
        }
        Ui4AsyncCompositionTarget::Overlay {
            surface,
            constant_alpha,
        } => stage_overlay_plane_surface_flip(dev, surface, constant_alpha, composition.reason),
        Ui4AsyncCompositionTarget::DirectOverlay { surface } => {
            stage_ui4_direct_overlay_flip(dev, surface, composition.reason)
        }
    }
}

pub(crate) fn commit_ui4_composition_flip(composition: Ui4AsyncComposition) {
    match composition.target {
        Ui4AsyncCompositionTarget::Primary { surface, .. } => {
            mark_primary_composition_surface_front(surface, composition.change)
        }
        Ui4AsyncCompositionTarget::Overlay { surface, .. } => {
            mark_overlay_composition_surface_front(surface, composition.change)
        }
        Ui4AsyncCompositionTarget::DirectOverlay { .. } => {}
    }
    let elapsed_us =
        crate::chronos::monotonic_nanos().saturating_sub(composition.queued_ns) / 1_000;
    let effective_bounds = composition.effective.bounding_rect().unwrap_or_default();
    if let Ui4AsyncCompositionTarget::DirectOverlay { surface } = composition.target {
        if let Some(proof) = UI4_DIRECT_SCANOUT_PROOFS.lock().get_mut(surface.plane_slot) {
            *proof = Ui4DirectScanoutProof {
                producer_frame: surface.producer_frame,
                publish_serial: surface.producer_publish_serial,
            };
        }
        let scanout_sequence = UI4_DIRECT_SCANOUT_LOG_SEQUENCE
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        if should_log_ui4_direct_checkpoint(scanout_sequence) {
            crate::log_info!(target: "ui4";
                "ui4/direct-present: scanout-ready checkpoint={} reason={} slot={} alias={} producer_frame={} producer_buffer={} publish_serial={} producer_release_sequence={} source_phys=0x{:X} display_gpu=0x{:X} source={}x{} destination={}x{} pitch=0x{:X} pixel_alpha=premultiplied plane_opacity={} guc_jobs=0 engine_output_ready=surflive ownership=display-live elapsed_us={}\n",
                scanout_sequence,
                composition.reason,
                surface.plane_slot,
                surface.alias_index,
                surface.producer_frame,
                surface.producer_buffer_index,
                surface.producer_publish_serial,
                surface.producer_release_sequence,
                surface.phys,
                surface.gpu,
                surface.width,
                surface.height,
                surface.dest_width,
                surface.dest_height,
                surface.pitch_bytes,
                surface.opacity,
                elapsed_us,
            );
        }
    } else {
        let backend = match composition.work {
            Some(Ui4AsyncCompositionWork::GucRcs(_)) => "guc-rcs",
            Some(Ui4AsyncCompositionWork::GucBcs(_)) => "guc-bcs0-fast-copy",
            None => "cpu-sparse-copy",
        };
        crate::log_trace!(target: "ui4";
            "ui4/compositor: scanout-ready backend={} reason={} tiles={} effective_rects={} effective_bounds={}x{}@{},{} elapsed_us={}\n",
            backend,
            composition.reason,
            composition.tile_count,
            composition.effective.len(),
            effective_bounds.width,
            effective_bounds.height,
            effective_bounds.x,
            effective_bounds.y,
            elapsed_us,
        );
    }
}

pub(crate) fn ui4_direct_composition_plane_slot(composition: Ui4AsyncComposition) -> Option<usize> {
    match composition.target {
        Ui4AsyncCompositionTarget::DirectOverlay { surface } => Some(surface.plane_slot),
        _ => None,
    }
}

pub(crate) const fn ui4_composition_has_guc_work(composition: Ui4AsyncComposition) -> bool {
    composition.work.is_some()
}

pub(crate) fn ui4_composition_flip_is_live(composition: Ui4AsyncComposition) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let (plane_base, surface_reg) = match composition.target {
        Ui4AsyncCompositionTarget::Primary { surface, .. } => {
            (surface.pipe.primary_plane().base(), u32::try_from(surface.gpu).ok())
        }
        Ui4AsyncCompositionTarget::Overlay { surface, .. } => {
            (overlay_plane_base(surface.pipe, surface.plane_slot), u32::try_from(surface.gpu).ok())
        }
        Ui4AsyncCompositionTarget::DirectOverlay { surface } => {
            (overlay_plane_base(surface.pipe, surface.plane_slot), u32::try_from(surface.gpu).ok())
        }
    };
    surface_reg.is_some_and(|surface_reg| {
        crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF) == surface_reg
    })
}

fn compose_premultiplied_rgba_tiles_into_primary_gpgpu(
    surface: PrimarySwapSurface,
    tiles: &[RgbaOverlayTile<'_>],
    damage: CompositionDamageRegion,
    asynchronous: bool,
) -> GpgpuCompositionResult {
    if surface.byte_len as u64 > COMPOSE_RCS_GPU_ALIAS_BYTES
        || (!asynchronous
            && (!UI4_GPGPU_MULTI_RUN_COMPOSITOR_ENABLED
                || !crate::intel::gpgpu::sprite_quad_worklist_ready()))
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
    let Some(destination_gpu) = primary_compose_rcs_gpu_for_surface(surface) else {
        return GpgpuCompositionResult::Unavailable;
    };
    let Some(destination) = crate::intel::gpgpu::GpgpuRgba8Surface::new(
        surface.phys,
        destination_gpu,
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

    if asynchronous {
        let Some(bounds) = damage
            .bounding_rect()
            .and_then(|rect| clip_composition_damage(rect, surface.width, surface.height))
        else {
            return GpgpuCompositionResult::Complete;
        };
        let mut layers = Vec::with_capacity(tiles.len());
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
                || gpgpu_ranges_overlap(
                    destination.gpu,
                    destination.bytes,
                    source.gpu,
                    source.bytes,
                )
            {
                return GpgpuCompositionResult::Unavailable;
            }
            layers.push(crate::intel::gpgpu::GpgpuUi4ComposeLayer {
                src: source,
                src_scanout_cache: tile.gpgpu_scanout_cache,
                dst_x: tile.x.min(i32::MAX as u32) as i32,
                dst_y: tile.y.min(i32::MAX as u32) as i32,
                dst_width: tile.width,
                dst_height: tile.height,
                opacity: tile.opacity,
            });
        }
        return match crate::intel::gpgpu::queue_ui4_compositor_layers(
            Some(base),
            destination,
            &layers,
            crate::intel::gpgpu::GpgpuRect::new(
                bounds.x.min(i32::MAX as u32) as i32,
                bounds.y.min(i32::MAX as u32) as i32,
                bounds.width,
                bounds.height,
            ),
            crate::intel::gpgpu::UI4_COMPOSE_FLAG_BASE_XRGB
                | crate::intel::gpgpu::UI4_COMPOSE_FLAG_DEST_XRGB,
        ) {
            Ok(submission) => GpgpuCompositionResult::Queued(submission),
            Err(crate::intel::gpgpu::Ui4CompositorSubmitError::Busy) => {
                GpgpuCompositionResult::SubmittedIncomplete
            }
            Err(_) => GpgpuCompositionResult::Unavailable,
        };
    }

    // The normal video case is deliberately one source run. The decoded RGBA
    // contract is fully opaque, so its covered pixels do not depend on the old
    // destination. Newly allocated swap surfaces start as exact base copies;
    // after a move, only damage left outside the current video rectangle is
    // restored on the CPU. This avoids the unproven Gen12 mid-batch bindful
    // source switch which retired successfully while replaying the base source.
    if let [tile] = tiles {
        if tile.known_opaque && tile.opacity == u8::MAX {
            let Some(source) = tile.gpgpu_surface else {
                return GpgpuCompositionResult::Unavailable;
            };
            if !source.is_valid()
                || source.width != tile.source_width
                || source.height != tile.source_height
                || source.pitch_bytes as usize != tile.pitch_bytes
                || tile.width == 0
                || tile.height == 0
                || gpgpu_ranges_overlap(
                    destination.gpu,
                    destination.bytes,
                    source.gpu,
                    source.bytes,
                )
            {
                return GpgpuCompositionResult::Unavailable;
            }
            let tile_rect = CompositionDamageRect::new(tile.x, tile.y, tile.width, tile.height);
            let Some(restored) =
                restore_primary_composition_base_outside_opaque_rect(surface, damage, tile_rect)
            else {
                return GpgpuCompositionResult::Unavailable;
            };
            if !restored.is_empty() && !dma_flush_primary_swap_region(surface, restored) {
                return GpgpuCompositionResult::Unavailable;
            }
            let descriptors = damage
                .rects()
                .iter()
                .filter_map(|damaged| {
                    intersect_composition_damage(tile_rect, *damaged)
                        .and_then(|rect| {
                            clip_composition_damage(rect, surface.width, surface.height)
                        })
                        .map(|draw| {
                            composition_quad_descriptor(
                                draw,
                                tile.x,
                                tile.y,
                                tile.width,
                                tile.height,
                                u8::MAX,
                                crate::intel::gpgpu::SPRITE_QUAD_WORKLIST_FLAG_DEST_XRGB,
                            )
                        })
                })
                .collect::<Vec<_>>();
            if descriptors.is_empty()
                || descriptors.len() > crate::intel::gpgpu::sprite_quad_worklist_max_descs()
            {
                return GpgpuCompositionResult::Unavailable;
            }
            let run = crate::intel::gpgpu::GpgpuSpriteQuadWorklistRun {
                src: source,
                descs: &descriptors,
            };
            if asynchronous {
                return match crate::intel::gpgpu::queue_ui4_compositor_sprite_quad_runs(
                    destination,
                    core::slice::from_ref(&run),
                ) {
                    Ok(submission) => GpgpuCompositionResult::Queued(submission),
                    Err(crate::intel::gpgpu::Ui4CompositorSubmitError::Busy) => {
                        GpgpuCompositionResult::SubmittedIncomplete
                    }
                    Err(_) => GpgpuCompositionResult::Unavailable,
                };
            }
            let result = crate::intel::gpgpu::sprite_quad_worklist_rgba8_runs_over_result(
                destination,
                core::slice::from_ref(&run),
            );
            return match result.outcome {
                crate::intel::gpgpu::GpgpuSubmissionOutcome::Complete
                    if result.stats.descs == descriptors.len() && result.stats.submits == 1 =>
                {
                    GpgpuCompositionResult::Complete
                }
                crate::intel::gpgpu::GpgpuSubmissionOutcome::SubmittedIncomplete => {
                    GpgpuCompositionResult::SubmittedIncomplete
                }
                _ => GpgpuCompositionResult::Unavailable,
            };
        }
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
    if asynchronous {
        return match crate::intel::gpgpu::queue_ui4_compositor_sprite_quad_runs(destination, &runs)
        {
            Ok(submission) => GpgpuCompositionResult::Queued(submission),
            Err(crate::intel::gpgpu::Ui4CompositorSubmitError::Busy) => {
                GpgpuCompositionResult::SubmittedIncomplete
            }
            Err(_) => GpgpuCompositionResult::Unavailable,
        };
    }
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
    asynchronous: bool,
    sparse_static_painter: bool,
    destination_fresh_transparent: bool,
) -> GpgpuCompositionResult {
    if surface.byte_len as u64 > COMPOSE_RCS_GPU_ALIAS_BYTES
        || (!asynchronous
            && (!UI4_GPGPU_MULTI_RUN_COMPOSITOR_ENABLED
                || !crate::intel::gpgpu::sprite_quad_worklist_ready()))
    {
        return GpgpuCompositionResult::Unavailable;
    }
    let Some(destination_gpu) = overlay_compose_rcs_gpu_for_surface(surface) else {
        return GpgpuCompositionResult::Unavailable;
    };
    let Some(destination) = crate::intel::gpgpu::GpgpuRgba8Surface::new(
        surface.phys,
        destination_gpu,
        surface.byte_len,
        surface.width,
        surface.height,
        surface.pitch_bytes,
    ) else {
        return GpgpuCompositionResult::Unavailable;
    };

    if asynchronous && !sparse_static_painter {
        let Some(bounds) = damage
            .bounding_rect()
            .and_then(|rect| clip_composition_damage(rect, surface.width, surface.height))
        else {
            return GpgpuCompositionResult::Complete;
        };
        let mut layers = Vec::with_capacity(tiles.len());
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
                || gpgpu_ranges_overlap(
                    destination.gpu,
                    destination.bytes,
                    source.gpu,
                    source.bytes,
                )
            {
                return GpgpuCompositionResult::Unavailable;
            }
            layers.push(crate::intel::gpgpu::GpgpuUi4ComposeLayer {
                src: source,
                src_scanout_cache: tile.gpgpu_scanout_cache,
                dst_x: tile.x.min(i32::MAX as u32) as i32,
                dst_y: tile.y.min(i32::MAX as u32) as i32,
                dst_width: tile.width,
                dst_height: tile.height,
                opacity: tile.opacity,
            });
        }
        return match crate::intel::gpgpu::queue_ui4_compositor_layers(
            None,
            destination,
            &layers,
            crate::intel::gpgpu::GpgpuRect::new(
                bounds.x.min(i32::MAX as u32) as i32,
                bounds.y.min(i32::MAX as u32) as i32,
                bounds.width,
                bounds.height,
            ),
            0,
        ) {
            Ok(submission) => GpgpuCompositionResult::Queued(submission),
            Err(crate::intel::gpgpu::Ui4CompositorSubmitError::Busy) => {
                GpgpuCompositionResult::SubmittedIncomplete
            }
            Err(_) => GpgpuCompositionResult::Unavailable,
        };
    }

    // New overlay allocations are synchronously zero-filled and cache-flushed
    // before they are mapped. For the immutable sparse painter that is already
    // the required transparent base, so its first composition must not launch
    // an otherwise redundant 2560x1440 clear. Once a destination has carried
    // content, ordinary damage-local clears remain in the same ordered batch.
    let clear_descriptors = if sparse_static_painter && destination_fresh_transparent {
        Vec::new()
    } else {
        damage
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
            .collect::<Vec<_>>()
    };

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
    if !clear_descriptors.is_empty() {
        runs.push(crate::intel::gpgpu::GpgpuSpriteQuadWorklistRun {
            // CLEAR descriptors never sample this binding; using the
            // destination keeps the run valid without another allocation.
            src: destination,
            descs: &clear_descriptors,
        });
    }
    runs.extend(
        descriptors
            .iter()
            .filter(|(_, descriptors)| !descriptors.is_empty())
            .map(|(src, descriptors)| crate::intel::gpgpu::GpgpuSpriteQuadWorklistRun {
                src: *src,
                descs: descriptors,
            }),
    );
    if runs.is_empty() {
        return GpgpuCompositionResult::Complete;
    }
    if asynchronous {
        return match crate::intel::gpgpu::queue_ui4_compositor_sprite_quad_runs(destination, &runs)
        {
            Ok(submission) => GpgpuCompositionResult::Queued(submission),
            Err(crate::intel::gpgpu::Ui4CompositorSubmitError::Busy) => {
                GpgpuCompositionResult::SubmittedIncomplete
            }
            Err(_) => GpgpuCompositionResult::Unavailable,
        };
    }
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
    let want_stride = plane_stride_reg_value(surface.pitch_bytes).unwrap_or(0);
    let want_surf = u32::try_from(surface.gpu).unwrap_or(0);
    let ctl = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_CTL_OFF);
    let stride = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_STRIDE_OFF);
    let pos = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_POS_OFF);
    let size = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SIZE_OFF);
    let surf = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF);
    let surf_live = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF);
    let color_ctl = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_COLOR_CTL_OFF);
    let want_color_ctl = plane_color_ctl_alpha(color_ctl, alpha);

    let want_ctl = overlay_plane_ctl_enabled(ctl, alpha);
    (ctl & PLANE_CTL_ENABLE) == 0
        || (ctl & PLANE_CTL_ORDER_RGBX) != (want_ctl & PLANE_CTL_ORDER_RGBX)
        || stride != want_stride
        || pos != want_pos
        || size != want_size
        || surf != want_surf
        || surf_live != want_surf
        || color_ctl != want_color_ctl
        || !overlay_plane_constant_alpha_matches(dev, plane_base, u8::MAX)
}

fn overlay_plane_surface_flip_guard(
    dev: crate::intel::Dev,
    surface: OverlaySurface,
    _pos_x: u32,
    _pos_y: u32,
    alpha: OverlayAlphaMode,
) -> Result<(), &'static str> {
    {
        let surface_pool =
            overlay_surface_pool(surface.pipe, surface.plane_slot).ok_or("surface-plane-slot")?;
        let pool = surface_pool.lock();
        if !pool.matches(surface.width, surface.height, surface.pipe) {
            return Err("surface-pool-shape");
        }
        let owned = pool.surfaces.get(surface.buffer_index).copied().flatten();
        if owned.map(|owned| owned.gpu) != Some(surface.gpu) {
            return Err("surface-pool-ownership");
        }
    }
    plane_stride_reg_value(surface.pitch_bytes).ok_or("stride-range")?;
    overlay_plane_dynamic_flip_guard(dev, surface.pipe, surface.plane_slot, alpha)
}

fn overlay_plane_dynamic_flip_guard(
    dev: crate::intel::Dev,
    pipe: PipeInfo,
    plane_slot: usize,
    alpha: OverlayAlphaMode,
) -> Result<(), &'static str> {
    if plane_slot > OVERLAY_UNIVERSAL_PLANE_COUNT {
        return Err("surface-plane-slot");
    }
    if !ui4_rgba8_plane_stack_ready(pipe) {
        return Err("rgba8-stack-not-ready");
    }
    let plane_base = overlay_plane_base(pipe, plane_slot);
    let ctl = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_CTL_OFF);
    let color_ctl = crate::intel::mmio_read(dev, plane_base + UNI_PLANE_COLOR_CTL_OFF);
    let resources = [
        (overlay_plane_base(pipe, 0), PLANE_DBUF_SLOT_0_START, PLANE_DBUF_SLOT_0_END),
        (
            overlay_plane_base(pipe, UI_OVERLAY_PLANE_SLOT),
            PLANE_DBUF_SLOT_1_START,
            PLANE_DBUF_SLOT_1_END,
        ),
        (
            overlay_plane_base(pipe, VIDEO_NV12_PLANE_SLOT),
            PLANE_DBUF_SLOT_2_START,
            PLANE_DBUF_SLOT_2_END,
        ),
        (
            overlay_plane_base(pipe, VIDEO_NV12_Y_PLANE_SLOT),
            PLANE_DBUF_SLOT_3_START,
            PLANE_DBUF_SLOT_3_END,
        ),
        (
            overlay_plane_base(pipe, crate::ui4::INTERACTION_OVERLAY_PLANE_SLOT),
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
    if crate::intel::mmio_read(dev, plane_base + UNI_PLANE_KEYVAL_OFF) != 0
        || !overlay_plane_constant_alpha_is_valid(dev, plane_base)
    {
        return Err("plane-color-key");
    }
    if crate::intel::mmio_read(dev, plane_base + UNI_PLANE_AUX_DIST_OFF) != 0
        || crate::intel::mmio_read(dev, plane_base + UNI_PLANE_AUX_OFFSET_OFF) != 0
    {
        return Err("plane-aux-surface");
    }
    if crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURF_OFF)
        != crate::intel::mmio_read(dev, plane_base + UNI_PLANE_SURFLIVE_OFF)
    {
        return Err("previous-flip-pending");
    }
    Ok(())
}

fn overlay_plane_geometry(
    pitch_bytes: u32,
    width: u32,
    height: u32,
    pos_x: u32,
    pos_y: u32,
) -> Option<PlaneSurfaceGeometry> {
    if width == 0 || height == 0 {
        return None;
    }
    Some(PlaneSurfaceGeometry {
        stride_reg: plane_stride_reg_value(pitch_bytes)?,
        pos_reg: plane_pos_reg_value(pos_x, pos_y),
        size_reg: plane_size_reg_value(width, height),
        offset_reg: 0,
    })
}

const DIRECT_PLANE_SCALER_MIN_DIMENSION: u32 = 8;
/// Bound ordinary direct-plane enlargement to 2x. The phase path already
/// handles factors below 1.0; close transitions retain their existing
/// just-under-3x downscale ceiling.
const DIRECT_PLANE_SCALER_MIN_FACTOR: u32 = 0x8000;
const DIRECT_PLANE_SCALER_MAX_FACTOR: u32 = 0x2_FFFF;

fn direct_plane_scaler_factor(source: u32, destination: u32) -> Option<u32> {
    if source < DIRECT_PLANE_SCALER_MIN_DIMENSION || destination < DIRECT_PLANE_SCALER_MIN_DIMENSION
    {
        return None;
    }
    let numerator = u64::from(source).checked_shl(16)?;
    let factor = numerator
        .saturating_add(u64::from(destination).saturating_sub(1))
        .checked_div(u64::from(destination))?;
    u32::try_from(factor).ok().filter(|factor| {
        (DIRECT_PLANE_SCALER_MIN_FACTOR..=DIRECT_PLANE_SCALER_MAX_FACTOR).contains(factor)
    })
}

fn direct_plane_scaler_phase(factor: u32) -> u32 {
    let phase = i64::from(factor) / 2 - 0x8000;
    if phase < 0 {
        (((0x1_0000i64 + phase) >> 2) as u32) & PIPE_SCALER_PHASE_MASK
    } else {
        (((phase >> 2) as u32) & PIPE_SCALER_PHASE_MASK) | PIPE_SCALER_PHASE_TRIP
    }
}

const fn direct_plane_scaler_id(plane_slot: usize) -> Option<usize> {
    match plane_slot {
        1 | 3 => Some(0),
        2 => Some(1),
        _ => None,
    }
}

fn direct_overlay_geometry_and_scaler(
    surface: Ui4DirectOverlaySurface,
) -> Option<(PlaneSurfaceGeometry, PlaneScalerFlip)> {
    if surface.dest_width == surface.width && surface.dest_height == surface.height {
        return Some((
            overlay_plane_geometry(
                surface.pitch_bytes,
                surface.width,
                surface.height,
                surface.pos_x,
                surface.pos_y,
            )?,
            PlaneScalerFlip {
                pipe_slot: surface.pipe.slot,
                plane_slot: surface.plane_slot,
                mode: PlaneScalerMode::Detached,
            },
        ));
    }
    let scaler_id = direct_plane_scaler_id(surface.plane_slot)?;
    pipe_scaler_registers(surface.pipe.slot, scaler_id)?;
    let hfactor = direct_plane_scaler_factor(surface.width, surface.dest_width)?;
    let vfactor = direct_plane_scaler_factor(surface.height, surface.dest_height)?;
    let hphase = direct_plane_scaler_phase(hfactor);
    let vphase = direct_plane_scaler_phase(vfactor);
    let window_pos_reg = (surface.pos_x.checked_shl(16)?) | surface.pos_y;
    let window_size_reg = (surface.dest_width.checked_shl(16)?) | surface.dest_height;
    Some((
        overlay_plane_geometry(surface.pitch_bytes, surface.width, surface.height, 0, 0)?,
        PlaneScalerFlip {
            pipe_slot: surface.pipe.slot,
            plane_slot: surface.plane_slot,
            mode: PlaneScalerMode::Scaled {
                scaler_id,
                window_pos_reg,
                window_size_reg,
                hphase_reg: (hphase << 16) | hphase,
                vphase_reg: (vphase << 16) | vphase,
            },
        },
    ))
}

fn program_overlay_plane_geometry(
    dev: crate::intel::Dev,
    plane_base: usize,
    geometry: PlaneSurfaceGeometry,
) {
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_STRIDE_OFF, geometry.stride_reg);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_POS_OFF, geometry.pos_reg);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_SIZE_OFF, geometry.size_reg);
    crate::intel::mmio_write(dev, plane_base + UNI_PLANE_OFFSET_OFF, geometry.offset_reg);
}

/// Fast path for the stable RGBA plane contract. Format, pixel-alpha mode,
/// DBUF and watermarks remain untouched; opaque plane alpha and geometry latch
/// with SURF.
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
    let Some(geometry) =
        overlay_plane_geometry(surface.pitch_bytes, surface.width, surface.height, pos_x, pos_y)
    else {
        return false;
    };
    match queue_ui4_plane_surface_flip(
        plane_base,
        surface_reg,
        Some(geometry),
        Some(u8::MAX),
        Some(PlaneScalerFlip {
            pipe_slot: surface.pipe.slot,
            plane_slot: surface.plane_slot,
            mode: PlaneScalerMode::Detached,
        }),
        reason,
    ) {
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
        PlaneSurfaceFlipQueueResult::Inactive => {
            if surface.plane_slot < crate::ui4::INTERACTION_OVERLAY_PLANE_SLOT
                && crate::r::readiness::is_set(crate::r::readiness::UI4_COMPOSITOR_READY)
            {
                crate::log_error!(target: "intel/display";
                    "intel/display: overlay-surf-only rejected reason={} pipe={} slot={} cause=ui4-compositor-batch-required\n",
                    reason,
                    surface.pipe.name,
                    surface.plane_slot,
                );
                return false;
            }
        }
    }
    // Direct MMIO remains available only during pre-compositor display
    // initialization and for the independent slot-4 interaction plane.
    detach_pipe_scalers_from_plane(dev, surface.pipe.slot, surface.plane_slot);
    program_overlay_plane_geometry(dev, plane_base, geometry);
    program_overlay_plane_constant_alpha(dev, plane_base, u8::MAX);
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

fn stage_overlay_plane_surface_flip(
    dev: crate::intel::Dev,
    surface: OverlaySurface,
    constant_alpha: u8,
    reason: &str,
) -> bool {
    if overlay_plane_surface_flip_guard(dev, surface, 0, 0, UI4_RGBA8_OVERLAY_CONTRACT).is_err() {
        return false;
    }
    let plane_base = overlay_plane_base(surface.pipe, surface.plane_slot);
    let Some(surface_reg) = u32::try_from(surface.gpu).ok() else {
        return false;
    };
    let Some(geometry) =
        overlay_plane_geometry(surface.pitch_bytes, surface.width, surface.height, 0, 0)
    else {
        return false;
    };
    queue_ui4_plane_surface_flip(
        plane_base,
        surface_reg,
        Some(geometry),
        Some(constant_alpha),
        Some(PlaneScalerFlip {
            pipe_slot: surface.pipe.slot,
            plane_slot: surface.plane_slot,
            mode: PlaneScalerMode::Detached,
        }),
        reason,
    ) == PlaneSurfaceFlipQueueResult::Queued
}

fn stage_ui4_direct_overlay_flip(
    dev: crate::intel::Dev,
    surface: Ui4DirectOverlaySurface,
    reason: &str,
) -> bool {
    if active_display_pipeline_target()
        .and_then(|target| target.pipeline.pipe())
        .map(|pipe| pipe.slot)
        != Some(surface.pipe.slot)
        || overlay_plane_dynamic_flip_guard(
            dev,
            surface.pipe,
            surface.plane_slot,
            UI4_RGBA8_OVERLAY_CONTRACT,
        )
        .is_err()
    {
        return false;
    }
    let mapping_matches = ui4_direct_scanout_pool(surface.pipe, surface.plane_slot)
        .and_then(|pool| {
            pool.lock()
                .mappings
                .get(surface.alias_index)
                .copied()
                .flatten()
        })
        .is_some_and(|mapping| {
            mapping.phys == surface.phys && mapping.byte_len == surface.byte_len
        });
    if !mapping_matches {
        return false;
    }
    let Some(surface_reg) = u32::try_from(surface.gpu).ok() else {
        return false;
    };
    let Some((geometry, scaler)) = direct_overlay_geometry_and_scaler(surface) else {
        return false;
    };
    if matches!(scaler.mode, PlaneScalerMode::Scaled { .. })
        && !UI4_DIRECT_PLANE_SCALER_LOGGED.swap(true, Ordering::AcqRel)
    {
        crate::log_info!(target: "ui4";
            "ui4/direct-present: hardware-scale active slot={} producer_frame={} source={}x{} destination={}x{}@{},{} source_buffer_mutation=none scaler_commit=surflive+scaler-readback\n",
            surface.plane_slot,
            surface.producer_frame,
            surface.width,
            surface.height,
            surface.dest_width,
            surface.dest_height,
            surface.pos_x,
            surface.pos_y,
        );
    }
    queue_ui4_plane_surface_flip(
        overlay_plane_base(surface.pipe, surface.plane_slot),
        surface_reg,
        Some(geometry),
        Some(surface.opacity),
        Some(scaler),
        reason,
    ) == PlaneSurfaceFlipQueueResult::Queued
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

#[cfg(test)]
mod direct_plane_scaler_tests {
    use super::{direct_plane_scaler_factor, overlay_composition_constant_alpha};

    #[test]
    fn accepts_two_x_upscale_and_bounded_close_downscale() {
        assert_eq!(direct_plane_scaler_factor(1280, 2560), Some(0x8000));
        assert_eq!(direct_plane_scaler_factor(2560, 1280), Some(0x2_0000));
    }

    #[test]
    fn rejects_scaling_outside_the_bounded_factor_range() {
        assert_eq!(direct_plane_scaler_factor(640, 2560), None);
        assert_eq!(direct_plane_scaler_factor(2560, 800), None);
    }

    #[test]
    fn empty_overlay_composition_parks_plane_at_zero_alpha() {
        assert_eq!(overlay_composition_constant_alpha(0, true), 0);
        assert_eq!(overlay_composition_constant_alpha(1, true), 0);
        assert_eq!(overlay_composition_constant_alpha(1, false), u8::MAX);
        assert_eq!(overlay_composition_constant_alpha(4, false), u8::MAX);
    }
}
