//! Shell-controlled GPGPU live previews backed exclusively by UI4 frames.
//!
//! This is a trusted kernel app beside the permanent UI4 compositor. It
//! owns frame/window lifetime and compute cadence. The compute trio is admitted
//! through one broker session onto dedicated universal-plane slots; standalone
//! C++/IGC demos reuse the same exact-surface publication lifecycle on slot 1.
//! Display pipe programming remains exclusively compositor-owned.

use alloc::{string::String, sync::Arc, vec::Vec};

use spin::Mutex;
use trueos_time::{Duration, Instant, Timer};

use super::{
    DamageRect, FrameCadence, FrameContent, FrameHandle, FramePlanError, FramePoolError, FrameSpec,
    FrameWriteLease, OutputId, PremultipliedRgba8, ScanoutFormat, Ui4CursorSource, Ui4InputEvent,
    WindowCreate, WindowId, WindowOwner, WindowPlacement, WindowPlane, WindowSessionCloseRequest,
    WindowSessionId, acquire_frame_buffer, begin_window_session, cancel_frame_buffer, create_frame,
    create_window, destroy_frame, finish_window_session, finish_window_session_with_request,
    gpgpu_rgba_surface, publish_frame_buffer, publish_gpgpu_frame_buffer,
    publish_gpu_font_frame_buffer, publish_window_frame, publish_window_frames,
    replace_window_frame, reselect_window_for_cursor, set_windows_visible, take_owner_input_events,
    window_placement, writable_rgba_view,
};

const PREVIEW_OWNER: WindowOwner = WindowOwner::GPGPU_PREVIEW;
const PREVIEW_WIDTH: u32 = super::DEFAULT_FRAME_WIDTH;
const PREVIEW_HEIGHT: u32 = super::DEFAULT_FRAME_HEIGHT;
const LAB256_PREVIEW_SIZE: u32 = 256;
const PREVIEW_MARGIN: u32 = 64;
const PREVIEW_GRID_GAP: u32 = 16;
const PREVIEW_Z: i32 = 30;
const IDLE_POLL_MS: u64 = 20;
const COMMAND_POLL_MAX_MS: u64 = 10;
const RESIZE_RETRY_MS: u64 = 250;
const PRIMARY_BUTTON_MASK: u32 = 1 << 0;
const STATIC30_FRAME_COUNT: usize = 30;
const STATIC30_PLANE_COUNT: usize = 3;
const STATIC30_COLUMNS: u32 = 6;
const STATIC30_ROWS: u32 = 5;
const STATIC30_MAX_WIDTH: u32 = 320;
const STATIC30_MAX_HEIGHT: u32 = 180;
const CPP_FONT_RUSH_MAX_PLANES: usize = 4;
const CPP_FONT_RUSH2_PRODUCER_COUNT: usize = 32;
const CPP_FONT_RUSH2_ROW_HEIGHT: u32 = 48;
const CPP_FONT_RUSH2_LADDER: [usize; 7] = [1, 2, 4, 8, 16, 24, 32];

const fn cpp_font_rush2_tier(producer: usize) -> u16 {
    (producer % 4 + 1) as u16
}

const _: () = {
    assert!(cpp_font_rush2_tier(0) == 1);
    assert!(cpp_font_rush2_tier(3) == 4);
    assert!(cpp_font_rush2_tier(31) == 4);
};
const CPP_FONT_RUSH_CADENCE_MS: u64 = 250;
const CPP_FONT_RUSH_STAGE_MS: u64 = 3_000;
const CPP_FONT_RUSH_TITLE_LETTER_MS: u64 = 150;
const CPP_FONT_RUSH_TITLE_HOLD_MS: u64 = 1_000;
const CPP_FONT_RUSH_BLANK_MIN_MS: u64 = 2_000;
const CPP_FONT_RUSH_SECTION_CADENCE_MS: u64 = 100;
const CPP_FONT_RUSH_SECTION_DURATION_MS: u64 = 3_000;
const CPP_FONT_RUSH_STORM_CADENCE_MS: u64 = 250;
const CPP_FONT_RUSH_STORM_COLUMNS: u8 = 8;
const CPP_FONT_RUSH_STORM_ROWS: u8 = 4;
const CPP_FONT_RUSH_STORM_GLYPHS_PER_PRODUCER: usize = 2;
const CPP_FONT_RUSH_RAW_STORM_GLYPHS: usize =
    crate::r::font_plan_service::FONT_PLAN_WORKER_COUNT * CPP_FONT_RUSH_STORM_GLYPHS_PER_PRODUCER;
const CPP_FONT_RUSH_GLYPH_ID_LOG_LIMIT: usize = 16;
const CPP_FONT_RUSH_VIEWPORT_SCALE: u32 = 4;
const CPP_FONT_RUSH_GLYPHS: [usize; CPP_FONT_RUSH_MAX_PLANES] = [1, 2, 4, 16];
const CPP_FONT_RUSH_TITLE_LETTERS: [char; 6] = ['T', 'R', 'U', 'E', 'O', 'S'];
const CPP_FONT_RUSH_TITLE_WORD: [char; 6] = ['T', 'r', 'u', 'e', 'O', 'S'];
const CPP_FONT_RUSH_TITLE_LETTER_MAX_FONT_PIXELS: f32 = 208.0;
const CPP_FONT_RUSH_TITLE_WORD_MAX_FONT_PIXELS: f32 = 116.0;
const CPP_FONT_RUSH_TITLE_WORD_WIDTH_FRACTION: f32 = 0.75;
// Lucida's visual bounds are deliberately packed more tightly than its text
// advances.  The one-time source remains comfortably below Font's analytical
// admission limit; its finished RGBA pixels are enlarged at presentation.
const CPP_FONT_RUSH_TITLE_WORD_X_FRACTIONS: [f32; 6] =
    [0.281_72, 0.368_89, 0.439_55, 0.522_91, 0.628_20, 0.734_14];
const CPP_FONT_RUSH_TITLE_WORD_PRESENTATION_SCALE: f32 = 1.8;
const CPP_FONT_RUSH_SECTION_PRESENTATION_SCALE: f32 = 3.0;
const CPP_GALLERY_PRESETS: [GpgpuPreviewPreset; 10] = [
    GpgpuPreviewPreset::CppGallery,
    GpgpuPreviewPreset::CppCloudHighWisps,
    GpgpuPreviewPreset::CppAurora,
    GpgpuPreviewPreset::CppJulia,
    GpgpuPreviewPreset::CppSdf,
    GpgpuPreviewPreset::CppVoronoi,
    GpgpuPreviewPreset::CppRetroSun,
    GpgpuPreviewPreset::CppAudio,
    GpgpuPreviewPreset::CppParticle,
    GpgpuPreviewPreset::Static30,
];

pub(crate) const GPGPU_PREVIEW_DEFAULT_DURATION_MS: u64 = 5_000;
pub(crate) const GPGPU_PREVIEW_DEFAULT_CADENCE_MS: u64 = 33;
pub(crate) const GPGPU_PREVIEW_DEFAULT_PUBLISH_EVERY: u32 = 1;
pub(crate) const GPGPU_PREVIEW_MAX_CADENCE_MS: u64 = 60_000;
pub(crate) const GPGPU_PREVIEW_MAX_PUBLISH_EVERY: u32 = 1_024;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuPreviewPreset {
    All,
    Static,
    Static30,
    Mandelbrot,
    Chart,
    Plasma,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    Lab256,
    CppGallery,
    CppCloudHighWisps,
    CppAurora,
    CppJulia,
    CppSdf,
    CppVoronoi,
    CppRetroSun,
    CppAudio,
    CppParticle,
    CppFont,
    CppFontRush,
    CppFontRush2,
}

impl GpgpuPreviewPreset {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::All => "compute-trio",
            Self::Static => "static",
            Self::Static30 => "static30",
            Self::Mandelbrot => "mandelbrot",
            Self::Chart => "chart",
            Self::Plasma => "plasma",
            Self::Lab256 => "lab256",
            Self::CppGallery => "cpp-gallery",
            Self::CppCloudHighWisps => "cpp-cloud-high-wisps",
            Self::CppAurora => "cpp-aurora",
            Self::CppJulia => "cpp-julia",
            Self::CppSdf => "cpp-sdf",
            Self::CppVoronoi => "cpp-voronoi",
            Self::CppRetroSun => "cpp-retro-sun",
            Self::CppAudio => "cpp-audio",
            Self::CppParticle => "cpp-particle",
            Self::CppFont => "cpp-font",
            Self::CppFontRush => "cpp-font-rush",
            Self::CppFontRush2 => "cpp-font-rush2",
        }
    }

    pub(crate) const fn is_cpp(self) -> bool {
        matches!(
            self,
            Self::CppGallery
                | Self::CppCloudHighWisps
                | Self::CppAurora
                | Self::CppJulia
                | Self::CppSdf
                | Self::CppVoronoi
                | Self::CppRetroSun
                | Self::CppAudio
                | Self::CppParticle
                | Self::CppFont
                | Self::CppFontRush
                | Self::CppFontRush2
        )
    }

    pub(crate) const fn is_resizable_cpp(self) -> bool {
        self.is_cpp() && !matches!(self, Self::CppFont | Self::CppFontRush | Self::CppFontRush2)
    }

    pub(crate) const fn buffering_label(self) -> &'static str {
        match self {
            Self::All => "double-per-frame",
            Self::Static30 | Self::CppFont => "single",
            Self::CppFontRush => "double-per-plane",
            Self::CppFontRush2 => "double-per-producer-row",
            _ => "double",
        }
    }

    pub(crate) const fn plane_layout_label(self) -> &'static str {
        match self {
            Self::All => "slots1+2+3-direct",
            Self::Static => "slot1-direct",
            Self::Static30 => "slots1+2+3/10-each",
            Self::Mandelbrot => "slot1-direct",
            Self::Chart => "slot2-direct",
            Self::Plasma => "slot3-direct",
            Self::Lab256 => "slot1-alpha-256x256",
            Self::CppGallery
            | Self::CppCloudHighWisps
            | Self::CppAurora
            | Self::CppJulia
            | Self::CppSdf
            | Self::CppVoronoi
            | Self::CppRetroSun
            | Self::CppAudio
            | Self::CppParticle
            | Self::CppFont => "slot1-direct",
            Self::CppFontRush => "slots0+1+2+3-direct-capability-bounded",
            Self::CppFontRush2 => "slots0+1+2+3-ui4-frame-stack-32-rows",
        }
    }
}

const fn preview_extent(preset: GpgpuPreviewPreset) -> (u32, u32) {
    match preset {
        GpgpuPreviewPreset::Lab256 => (LAB256_PREVIEW_SIZE, LAB256_PREVIEW_SIZE),
        GpgpuPreviewPreset::CppParticle => (
            crate::intel::gpgpu::PARTICLE_CRAFT_FRAME_WIDTH,
            crate::intel::gpgpu::PARTICLE_CRAFT_FRAME_HEIGHT,
        ),
        _ => (PREVIEW_WIDTH, PREVIEW_HEIGHT),
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuPreviewConfig {
    pub(crate) preset: GpgpuPreviewPreset,
    pub(crate) duration_ms: u64,
    pub(crate) cadence_ms: u64,
    pub(crate) publish_every: u32,
}

impl GpgpuPreviewConfig {
    pub(crate) const DEFAULT: Self = Self {
        preset: GpgpuPreviewPreset::All,
        duration_ms: GPGPU_PREVIEW_DEFAULT_DURATION_MS,
        cadence_ms: GPGPU_PREVIEW_DEFAULT_CADENCE_MS,
        publish_every: GPGPU_PREVIEW_DEFAULT_PUBLISH_EVERY,
    };

    fn validate(self) -> Result<Self, &'static str> {
        if self.cadence_ms == 0 || self.cadence_ms > GPGPU_PREVIEW_MAX_CADENCE_MS {
            return Err("cadence-ms-out-of-range-1-to-60000");
        }
        if self.publish_every == 0 || self.publish_every > GPGPU_PREVIEW_MAX_PUBLISH_EVERY {
            return Err("publish-every-out-of-range-1-to-1024");
        }
        Ok(self)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuPreviewPhase {
    Offline,
    Idle,
    Starting,
    Running,
    Faulted,
}

impl GpgpuPreviewPhase {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Faulted => "faulted",
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpgpuPreviewMetrics {
    pub(crate) attempted: u64,
    pub(crate) submitted: u64,
    pub(crate) completed: u64,
    pub(crate) published: u64,
    pub(crate) scanout_live: u64,
    pub(crate) scanout_superseded: u64,
    pub(crate) dropped_busy: u64,
    pub(crate) dropped_frame_busy: u64,
    pub(crate) dropped_queue_full: u64,
    pub(crate) dropped_in_flight: u64,
    pub(crate) dropped_cadence: u64,
    pub(crate) failed: u64,
    pub(crate) late: u64,
    pub(crate) elapsed_ms: u64,
    pub(crate) last_iterations: u32,
    pub(crate) last_marker: u32,
    pub(crate) last_submit_ms: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuPreviewStatus {
    pub(crate) online: bool,
    pub(crate) desired_running: bool,
    pub(crate) phase: GpgpuPreviewPhase,
    pub(crate) request_serial: u64,
    pub(crate) applied_serial: u64,
    pub(crate) config: GpgpuPreviewConfig,
    pub(crate) frame: Option<FrameHandle>,
    pub(crate) window: Option<WindowId>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) metrics: GpgpuPreviewMetrics,
    pub(crate) members: [GpgpuPreviewMemberStatus; 4],
    pub(crate) last_error: &'static str,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuPreviewMemberStatus {
    pub(crate) preset: GpgpuPreviewPreset,
    pub(crate) frame: Option<FrameHandle>,
    pub(crate) window: Option<WindowId>,
    pub(crate) plane_slot: u8,
    pub(crate) active: bool,
    pub(crate) metrics: GpgpuPreviewMetrics,
}

impl GpgpuPreviewMemberStatus {
    const fn inactive(preset: GpgpuPreviewPreset, plane_slot: u8) -> Self {
        Self {
            preset,
            frame: None,
            window: None,
            plane_slot,
            active: false,
            metrics: GpgpuPreviewMetrics {
                attempted: 0,
                submitted: 0,
                completed: 0,
                published: 0,
                scanout_live: 0,
                scanout_superseded: 0,
                dropped_busy: 0,
                dropped_frame_busy: 0,
                dropped_queue_full: 0,
                dropped_in_flight: 0,
                dropped_cadence: 0,
                failed: 0,
                late: 0,
                elapsed_ms: 0,
                last_iterations: 0,
                last_marker: 0,
                last_submit_ms: 0,
            },
        }
    }
}

const INACTIVE_PREVIEW_MEMBERS: [GpgpuPreviewMemberStatus; 4] = [
    GpgpuPreviewMemberStatus::inactive(GpgpuPreviewPreset::Mandelbrot, 1),
    GpgpuPreviewMemberStatus::inactive(GpgpuPreviewPreset::Chart, 2),
    GpgpuPreviewMemberStatus::inactive(GpgpuPreviewPreset::Plasma, 3),
    GpgpuPreviewMemberStatus::inactive(GpgpuPreviewPreset::CppFontRush, 0),
];

const COMPUTE_PREVIEW_PRESETS: [GpgpuPreviewPreset; 3] = [
    GpgpuPreviewPreset::Mandelbrot,
    GpgpuPreviewPreset::Chart,
    GpgpuPreviewPreset::Plasma,
];

impl GpgpuPreviewStatus {
    const fn initial() -> Self {
        Self {
            online: false,
            desired_running: false,
            phase: GpgpuPreviewPhase::Offline,
            request_serial: 0,
            applied_serial: 0,
            config: GpgpuPreviewConfig::DEFAULT,
            frame: None,
            window: None,
            width: 0,
            height: 0,
            metrics: GpgpuPreviewMetrics {
                attempted: 0,
                submitted: 0,
                completed: 0,
                published: 0,
                scanout_live: 0,
                scanout_superseded: 0,
                dropped_busy: 0,
                dropped_frame_busy: 0,
                dropped_queue_full: 0,
                dropped_in_flight: 0,
                dropped_cadence: 0,
                failed: 0,
                late: 0,
                elapsed_ms: 0,
                last_iterations: 0,
                last_marker: 0,
                last_submit_ms: 0,
            },
            members: INACTIVE_PREVIEW_MEMBERS,
            last_error: "none",
        }
    }
}

#[derive(Copy, Clone)]
struct DesiredPreview {
    serial: u64,
    running: bool,
    config: GpgpuPreviewConfig,
    policy: PreviewRunPolicy,
}

#[derive(Copy, Clone)]
struct PreviewRunPolicy {
    frame_limit: u64,
    target_hz: u64,
    interactive_cpp_gallery: bool,
    refocus_source: Option<Ui4CursorSource>,
}

impl PreviewRunPolicy {
    const SHELL: Self = Self {
        frame_limit: 0,
        target_hz: 0,
        interactive_cpp_gallery: false,
        refocus_source: None,
    };

    const CPP_GALLERY: Self = Self {
        frame_limit: 0,
        target_hz: 0,
        interactive_cpp_gallery: true,
        refocus_source: None,
    };
}

struct PreviewControl {
    desired: DesiredPreview,
    status: GpgpuPreviewStatus,
}

impl PreviewControl {
    const fn new() -> Self {
        Self {
            desired: DesiredPreview {
                serial: 0,
                running: false,
                config: GpgpuPreviewConfig::DEFAULT,
                policy: PreviewRunPolicy::SHELL,
            },
            status: GpgpuPreviewStatus::initial(),
        }
    }
}

static PREVIEW_CONTROL: Mutex<PreviewControl> = Mutex::new(PreviewControl::new());
static CPP_FONT_REQUEST: Mutex<Option<(u64, crate::r::font_kernel_service::FontStampRequest)>> =
    Mutex::new(None);
static CPP_FONT_RUSH2_RETIRED: Mutex<Vec<CppFontRush2RetiredFrame>> = Mutex::new(Vec::new());

struct CppFontRush2RetiredFrame {
    frame: FrameHandle,
    state: CppFontRush2ProducerState,
}

struct CloudBrushState {
    points: [u32; crate::intel::gpgpu::CPP_CLOUD_BRUSH_POINT_CAPACITY],
    count: usize,
    next: usize,
    dragging: Option<Ui4CursorSource>,
    last: Option<(i32, i32)>,
}

impl CloudBrushState {
    const fn new() -> Self {
        Self {
            points: [0; crate::intel::gpgpu::CPP_CLOUD_BRUSH_POINT_CAPACITY],
            count: 0,
            next: 0,
            dragging: None,
            last: None,
        }
    }

    fn push(&mut self, local_x: i32, local_y: i32, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let x = local_x.clamp(0, width.saturating_sub(1) as i32) as u32;
        let y = local_y.clamp(0, height.saturating_sub(1) as i32) as u32;
        let packed_x = x.saturating_mul(u16::MAX as u32) / width.saturating_sub(1).max(1);
        let packed_y = y.saturating_mul(u16::MAX as u32) / height.saturating_sub(1).max(1);
        self.points[self.next] = packed_x | (packed_y << 16);
        self.next = (self.next + 1) % self.points.len();
        self.count = self.count.saturating_add(1).min(self.points.len());
    }

    fn drag_to(&mut self, local_x: i32, local_y: i32, width: u32, height: u32) {
        let Some((from_x, from_y)) = self.last else {
            self.push(local_x, local_y, width, height);
            self.last = Some((local_x, local_y));
            return;
        };
        let dx = local_x.saturating_sub(from_x);
        let dy = local_y.saturating_sub(from_y);
        let distance = dx.unsigned_abs().max(dy.unsigned_abs());
        let spacing = width.min(height).saturating_div(24).max(1);
        let steps = distance
            .div_ceil(spacing)
            .max(1)
            .min(self.points.len() as u32);
        for step in 1..=steps {
            let x = i64::from(from_x) + i64::from(dx) * i64::from(step) / i64::from(steps);
            let y = i64::from(from_y) + i64::from(dy) * i64::from(step) / i64::from(steps);
            self.push(x as i32, y as i32, width, height);
        }
        self.last = Some((local_x, local_y));
    }
}

struct ActivePreview {
    request_serial: u64,
    config: GpgpuPreviewConfig,
    policy: PreviewRunPolicy,
    cadence_phase: u64,
    session: WindowSessionId,
    frame: FrameHandle,
    window: WindowId,
    width: u32,
    height: u32,
    resize_retry_width: u32,
    resize_retry_height: u32,
    resize_retry_at: Instant,
    started: Instant,
    next_render: Instant,
    static_needs_publish: bool,
    extra_surfaces: Vec<StaticPreviewSurface>,
    particle_craft: Option<crate::intel::gpgpu::GpgpuOwnedParticleCraftState>,
    cloud_brush: CloudBrushState,
    font_stamp: Option<crate::r::font_kernel_service::FontStampRequest>,
    font_rush: Option<CppFontRushPlaneState>,
    font_rush2: Option<CppFontRush2ProducerState>,
    metrics: GpgpuPreviewMetrics,
}

struct CppFontRush2ProducerState {
    producer: crate::r::font_kernel_service::FontGpuProducer,
    producer_index: u8,
    font_pixels: f32,
    rng: crate::tyche::SoftRng,
    pending: Option<CppFontRush2PendingRow>,
    published: [Option<crate::r::font_kernel_service::FontProducedRow>; 2],
}

struct CppFontRush2PendingRow {
    lease: FrameWriteLease,
    pending: crate::r::font_kernel_service::PendingFontProducerRow,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct CppFontRushTopology {
    width: u32,
    height: u32,
    plane_mask: u8,
    plane_slots: [u8; CPP_FONT_RUSH_MAX_PLANES],
    plane_count: u8,
}

struct CppFontRushPlaneState {
    rank: u8,
    plane_slot: u8,
    topology: CppFontRushTopology,
    stage: CppFontRushLayerStage,
    first_scanout_at: Option<Instant>,
    blank_started_at: Option<Instant>,
    rng: crate::tyche::SoftRng,
    planning: Option<CppFontRushPendingPlan>,
    ready_plan: Option<CppFontRushReadyPlan>,
    pending: Option<CppFontRushPendingFrame>,
    scanout_pending: Option<CppFontRushPendingScanout>,
    showcase_sources: CppFontRushShowcaseSources,
}

#[derive(Default)]
struct CppFontRushShowcaseSources {
    title_pending: Option<crate::r::font_kernel_service::PendingFontStamp>,
    title: Option<Arc<crate::r::font_kernel_service::FontStampedBuffer>>,
    section_pending: Option<crate::r::font_kernel_service::PendingFontStamp>,
    section: Option<Arc<crate::r::font_kernel_service::FontStampedBuffer>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CppFontRushLayerStage {
    Base,
    /// Every base cell is a 2x2 independently rolled subgrid.
    Expanded,
    /// Show exactly one fullscreen centered letter from `T`, `R`, `U`, `E`, `O`, `S`.
    TitleLetter(u8),
    /// Hold a tightly packed, GPU-enlarged, case-exact `TrueOS` for one second.
    TitleHold,
    /// Publish one transparent frame before the section sequence starts.
    BlankPrime,
    /// Keep the proven blank scanout live for the minimum two-second pause.
    BlankHold,
    /// Three 3x section signs with a foreground color step every 100 ms.
    SectionPulse,
    /// Clear both double-buffer members to transparent and stamp the same
    /// final section-sign base before no-clear accumulation begins.
    StormPrime {
        mirror: u8,
    },
    /// Mirror each raw, deterministic 32-producer wave into both frame buffers.
    ProducerStorm {
        wave: u64,
        mirror: u8,
    },
    /// A retired hardware layer which no longer creates font work.
    Dormant,
}

impl CppFontRushLayerStage {
    const fn label(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Expanded => "expanded-2x2",
            Self::TitleLetter(_) => "title-letter-150ms",
            Self::TitleHold => "title-hold-scaled-1s",
            Self::BlankPrime => "pre-section-blank-prime",
            Self::BlankHold => "pre-section-blank-hold-2s",
            Self::SectionPulse => "section-pulse-3x-100ms",
            Self::StormPrime { .. } => "producer-storm-prime",
            Self::ProducerStorm { .. } => "raw-producer-storm-32x2",
            Self::Dormant => "dormant",
        }
    }

    const fn cadence_ms(self) -> u64 {
        match self {
            Self::TitleLetter(_) => CPP_FONT_RUSH_TITLE_LETTER_MS,
            Self::SectionPulse | Self::StormPrime { .. } => CPP_FONT_RUSH_SECTION_CADENCE_MS,
            Self::ProducerStorm { .. } => CPP_FONT_RUSH_STORM_CADENCE_MS,
            Self::Base
            | Self::Expanded
            | Self::TitleHold
            | Self::BlankPrime
            | Self::BlankHold
            | Self::Dormant => CPP_FONT_RUSH_CADENCE_MS,
        }
    }

    const fn clear_color(self) -> Option<PremultipliedRgba8> {
        match self {
            Self::BlankPrime | Self::BlankHold | Self::ProducerStorm { .. } | Self::Dormant => None,
            _ => Some(PremultipliedRgba8::TRANSPARENT),
        }
    }

    const fn produces_frames(self) -> bool {
        !matches!(self, Self::BlankHold | Self::Dormant)
    }

    const fn repeats_while_live(self) -> bool {
        matches!(self, Self::Base | Self::Expanded | Self::SectionPulse)
    }

    const fn uses_showcase_sprite(self) -> bool {
        matches!(self, Self::TitleHold | Self::SectionPulse | Self::StormPrime { .. })
    }
}

struct CppFontRushPendingPlan {
    completion: crate::r::font_plan_service::PendingPreparedGlyphPlan,
    sequence: u64,
    scheduled_at: Instant,
    enqueued_at: Instant,
    requested_glyphs: usize,
    columns: u8,
    rows: u8,
    stage: CppFontRushLayerStage,
    font: crate::intel::gpu_font::GpuFontFace,
}

struct CppFontRushReadyPlan {
    plan: crate::r::font_plan_service::PreparedGlyphPlan,
    stats: crate::r::font_plan_service::FontPlanBuildStats,
    sequence: u64,
    scheduled_at: Instant,
    enqueued_at: Instant,
    ready_at: Instant,
    requested_glyphs: usize,
    columns: u8,
    rows: u8,
    stage: CppFontRushLayerStage,
    font: crate::intel::gpu_font::GpuFontFace,
    submit_attempts: u32,
}

struct CppFontRushPendingFrame {
    lease: FrameWriteLease,
    completion: crate::r::font_kernel_service::PendingFontFrameStamp,
    ticket: crate::r::font_kernel_service::FontKernelTicket,
    sequence: u64,
    scheduled_at: Instant,
    submit_started_at: Instant,
    accepted_at: Instant,
    requested_glyphs: usize,
    columns: u8,
    rows: u8,
    stage: CppFontRushLayerStage,
    font: crate::intel::gpu_font::GpuFontFace,
    glyph_fingerprint: u64,
    glyph_ids_sample: String,
    fifo_queued_ahead: usize,
    plan_batch_id: u64,
    plan_enqueue_delay_ms: u64,
    plan_queue_wait_ms: u64,
    plan_build_ms: u64,
    plan_total_ms: u64,
    plan_candidate_attempts: u64,
    plan_rejected_candidates: u64,
    plan_worker_slices: u64,
    plan_cooperative_yields: u64,
    plan_parallelism: usize,
    prepared_ops_bytes: usize,
    prepared_reserved_ops_bytes: usize,
    prepared_work: u64,
}

#[derive(Copy, Clone, Debug)]
struct CppFontRushPendingScanout {
    ticket: crate::r::font_kernel_service::FontKernelTicket,
    sequence: u64,
    producer_buffer: u8,
    frame_publish_serial: u64,
    window_publish_serial: u64,
    release_sequence: u64,
    published_at: Instant,
    requested_glyphs: usize,
    columns: u8,
    rows: u8,
    stage: CppFontRushLayerStage,
    font: crate::intel::gpu_font::GpuFontFace,
    glyph_fingerprint: u64,
}

#[derive(Copy, Clone)]
struct StaticPreviewSurface {
    frame: FrameHandle,
    window: WindowId,
    scheme: u8,
}

pub(crate) fn request_cpp_gallery_start() -> Result<u64, &'static str> {
    request_gpgpu_preview_start_with_policy(
        GpgpuPreviewConfig {
            preset: GpgpuPreviewPreset::CppGallery,
            duration_ms: 0,
            cadence_ms: GPGPU_PREVIEW_DEFAULT_CADENCE_MS,
            publish_every: GPGPU_PREVIEW_DEFAULT_PUBLISH_EVERY,
        },
        PreviewRunPolicy::CPP_GALLERY,
    )
}

pub(crate) fn request_cpp_font_preview_start(
    request: crate::r::font_kernel_service::FontStampRequest,
) -> Result<u64, &'static str> {
    let first = request.layers.first().ok_or("font-stamp-layer-count")?;
    if request.fit != crate::r::font_kernel_service::FontStampFit::Canvas {
        return Err("font-preview-requires-canvas");
    }
    let width = first.scene.raster_width;
    let height = first.scene.raster_height;
    let config = GpgpuPreviewConfig {
        preset: GpgpuPreviewPreset::CppFont,
        duration_ms: 0,
        cadence_ms: GPGPU_PREVIEW_DEFAULT_CADENCE_MS,
        publish_every: 1,
    };
    let mut control = PREVIEW_CONTROL.lock();
    let serial = next_serial(control.desired.serial);
    *CPP_FONT_REQUEST.lock() = Some((serial, request));
    control.desired = DesiredPreview {
        serial,
        running: true,
        config,
        policy: PreviewRunPolicy::SHELL,
    };
    control.status.desired_running = true;
    control.status.phase = GpgpuPreviewPhase::Starting;
    control.status.request_serial = serial;
    control.status.config = config;
    control.status.width = width;
    control.status.height = height;
    control.status.last_error = "none";
    Ok(serial)
}

pub(crate) fn request_cpp_font_rush_start() -> Result<u64, &'static str> {
    if !crate::r::font_kernel_service::status().online {
        return Err("font-service-offline");
    }
    if crate::r::font_plan_service::status().online_workers
        != crate::r::font_plan_service::FONT_PLAN_WORKER_COUNT
    {
        return Err("font-plan-service-offline");
    }
    // Font Rush is intentionally pinned to face 1 (Lucida Sans Unicode). The
    // finite showcase and terminal producer storm therefore have one warm,
    // stable recipe namespace and no asynchronous face-cycle state.
    if !crate::intel::gpu_font::font_face_is_available(crate::intel::gpu_font::GpuFontFace::Default)
    {
        return Err("font-not-registered");
    }
    ensure_cpp_font_rush_ui4_idle("request")?;
    let (_, topology) = cpp_font_rush_topology()?;
    log_cpp_font_rush_capabilities("request", 0, topology);
    let config = GpgpuPreviewConfig {
        preset: GpgpuPreviewPreset::CppFontRush,
        duration_ms: 0,
        cadence_ms: CPP_FONT_RUSH_CADENCE_MS,
        publish_every: 1,
    }
    .validate()?;
    // The authoritative busy check and request commit share one control lock.
    // A concurrent preview request can win before this point, but it can no
    // longer be silently overwritten between separate check/commit sections.
    let mut control = PREVIEW_CONTROL.lock();
    if control.desired.running {
        return Err("gpgpu-preview-busy");
    }
    CPP_FONT_REQUEST.lock().take();
    let serial = next_serial(control.desired.serial);
    control.desired = DesiredPreview {
        serial,
        running: true,
        config,
        policy: PreviewRunPolicy::SHELL,
    };
    control.status.desired_running = true;
    control.status.phase = GpgpuPreviewPhase::Starting;
    control.status.request_serial = serial;
    control.status.config = config;
    control.status.last_error = "none";
    Ok(serial)
}

pub(crate) fn request_cpp_font_rush_stop() -> Result<u64, &'static str> {
    let mut control = PREVIEW_CONTROL.lock();
    if !control.desired.running || control.desired.config.preset != GpgpuPreviewPreset::CppFontRush
    {
        return Err("font-rush-not-running");
    }
    let serial = next_serial(control.desired.serial);
    control.desired.serial = serial;
    control.desired.running = false;
    control.status.desired_running = false;
    control.status.request_serial = serial;
    CPP_FONT_REQUEST.lock().take();
    Ok(serial)
}

pub(crate) fn request_cpp_font_rush2_start() -> Result<u64, &'static str> {
    if !crate::r::font_kernel_service::status().online {
        return Err("font-service-offline");
    }
    ensure_cpp_font_rush_ui4_idle("request-rush2")?;
    let config = GpgpuPreviewConfig {
        preset: GpgpuPreviewPreset::CppFontRush2,
        duration_ms: 0,
        cadence_ms: 33,
        publish_every: 1,
    };
    let mut control = PREVIEW_CONTROL.lock();
    if control.desired.running {
        return Err("gpgpu-preview-busy");
    }
    let serial = next_serial(control.desired.serial);
    control.desired = DesiredPreview {
        serial,
        running: true,
        config,
        policy: PreviewRunPolicy::SHELL,
    };
    control.status.desired_running = true;
    control.status.phase = GpgpuPreviewPhase::Starting;
    control.status.request_serial = serial;
    control.status.config = config;
    control.status.last_error = "none";
    Ok(serial)
}

pub(crate) fn request_cpp_font_rush2_stop() -> Result<u64, &'static str> {
    let mut control = PREVIEW_CONTROL.lock();
    if !control.desired.running || control.desired.config.preset != GpgpuPreviewPreset::CppFontRush2
    {
        return Err("font-rush2-not-running");
    }
    let serial = next_serial(control.desired.serial);
    control.desired.serial = serial;
    control.desired.running = false;
    control.status.desired_running = false;
    control.status.request_serial = serial;
    Ok(serial)
}

fn ensure_cpp_font_rush_ui4_idle(stage: &'static str) -> Result<(), &'static str> {
    let usage = super::ui4_live_resource_usage();
    if usage.is_display_idle() {
        if usage.active_frames != 0 {
            crate::log_info!(
                target: "ui4";
                "ui4 cpp-font-rush admission accepted stage={} display_idle=1 detached_active_frames={} active_sessions=0 live_windows=0 detached_policy=not-display-live\n",
                stage,
                usage.active_frames,
            );
        }
        return Ok(());
    }
    crate::log_warn!(
        target: "ui4";
        "ui4 cpp-font-rush admission rejected stage={} reason=ui4-not-idle active_frames={} active_sessions={} live_windows={}\n",
        stage,
        usage.active_frames,
        usage.active_sessions,
        usage.live_windows,
    );
    Err("ui4-not-idle")
}

fn cpp_font_rush_plane_slots(application_plane_mask: u8) -> ([u8; CPP_FONT_RUSH_MAX_PLANES], u8) {
    let mut slots = [0u8; CPP_FONT_RUSH_MAX_PLANES];
    let mut count = 0u8;
    let mut slot = 0usize;
    while slot < super::INTERACTION_OVERLAY_PLANE_SLOT
        && usize::from(count) < CPP_FONT_RUSH_MAX_PLANES
    {
        if application_plane_mask & (1u8 << slot) != 0 {
            slots[usize::from(count)] = slot as u8;
            count += 1;
        }
        slot += 1;
    }
    (slots, count)
}

#[cfg(test)]
const fn cpp_font_rush_target_plane_count(elapsed_ms: u64, available_planes: u8) -> usize {
    let timed_planes = 1usize.saturating_add((elapsed_ms / CPP_FONT_RUSH_STAGE_MS) as usize);
    let available = if available_planes as usize > CPP_FONT_RUSH_MAX_PLANES {
        CPP_FONT_RUSH_MAX_PLANES
    } else {
        available_planes as usize
    };
    if timed_planes < available {
        timed_planes
    } else {
        available
    }
}

fn cpp_font_rush_topology() -> Result<(OutputId, CppFontRushTopology), &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let capabilities =
        super::ui4_output_capabilities(output).ok_or("ui4-output-capabilities-unavailable")?;
    let (plane_slots, plane_count) = cpp_font_rush_plane_slots(capabilities.application_plane_mask);
    if usize::from(plane_count) != CPP_FONT_RUSH_MAX_PLANES {
        return Err("font-rush-four-layer-topology-unavailable");
    }
    if capabilities.width == 0
        || capabilities.height == 0
        || capabilities.width > crate::r::font_kernel_service::FONT_STAMP_MAX_EXTENT
        || capabilities.height > crate::r::font_kernel_service::FONT_STAMP_MAX_EXTENT
        || u64::from(capabilities.width) * u64::from(capabilities.height)
            > crate::r::font_kernel_service::FONT_STAMP_MAX_PIXELS
    {
        return Err("font-rush-output-extent-unsupported");
    }
    Ok((
        output,
        CppFontRushTopology {
            width: capabilities.width,
            height: capabilities.height,
            plane_mask: capabilities.application_plane_mask,
            plane_slots,
            plane_count,
        },
    ))
}

fn log_cpp_font_rush_capabilities(
    stage: &'static str,
    active_planes: usize,
    topology: CppFontRushTopology,
) {
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font-rush capabilities stage={} extent={}x{} application_plane_mask=0x{:02X} reported_planes={} enumerated_slots={:?} usable_planes={} active_planes={} max_planes={} policy=one-consumer-per-enumerated-application-plane\n",
        stage,
        topology.width,
        topology.height,
        topology.plane_mask,
        topology.plane_mask.count_ones(),
        &topology.plane_slots[..usize::from(topology.plane_count)],
        topology.plane_count,
        active_planes,
        CPP_FONT_RUSH_MAX_PLANES,
    );
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn request_gpgpu_lab256_startup(
    frame_limit: u64,
    target_hz: u64,
) -> Result<u64, &'static str> {
    if frame_limit == 0 {
        return Err("frame-limit-must-be-nonzero");
    }
    if target_hz == 0 || target_hz > trueos_time::TICK_HZ {
        return Err("target-hz-out-of-range");
    }
    request_gpgpu_preview_start_with_policy(
        GpgpuPreviewConfig {
            preset: GpgpuPreviewPreset::Lab256,
            duration_ms: 0,
            cadence_ms: 1_000u64.div_ceil(target_hz),
            publish_every: 1,
        },
        PreviewRunPolicy {
            frame_limit,
            target_hz,
            interactive_cpp_gallery: false,
            refocus_source: None,
        },
    )
}

fn request_gpgpu_preview_start_with_policy(
    config: GpgpuPreviewConfig,
    policy: PreviewRunPolicy,
) -> Result<u64, &'static str> {
    let config = config.validate()?;
    let mut control = PREVIEW_CONTROL.lock();
    CPP_FONT_REQUEST.lock().take();
    let serial = next_serial(control.desired.serial);
    control.desired = DesiredPreview {
        serial,
        running: true,
        config,
        policy,
    };
    control.status.desired_running = true;
    control.status.phase = GpgpuPreviewPhase::Starting;
    control.status.request_serial = serial;
    control.status.config = config;
    control.status.last_error = "none";
    Ok(serial)
}

pub(crate) fn request_gpgpu_preview_stop() -> u64 {
    let mut control = PREVIEW_CONTROL.lock();
    let serial = next_serial(control.desired.serial);
    control.desired.serial = serial;
    control.desired.running = false;
    control.status.desired_running = false;
    control.status.request_serial = serial;
    CPP_FONT_REQUEST.lock().take();
    serial
}

pub(crate) fn gpgpu_preview_status() -> GpgpuPreviewStatus {
    PREVIEW_CONTROL.lock().status
}

#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn gpgpu_preview_consumer_service_task(worker_slot: u32) {
    crate::log_info!(
        target: "ui4";
        "ui4 gpgpu-preview-consumer carrier online owner={:?} placement=worker-ap2+ assigned_slot={} current_slot={} display_api=none activation=Shell2/on-demand buffering=double compute_release=completion-marker-before-publish interaction=movable/maximize-resize-cpp\n",
        PREVIEW_OWNER,
        worker_slot,
        crate::percpu::current_slot(),
    );
    {
        let mut control = PREVIEW_CONTROL.lock();
        control.status.online = true;
        control.status.phase = GpgpuPreviewPhase::Idle;
    }

    let mut applied_serial = 0u64;
    let mut active = Vec::<ActivePreview>::new();
    let mut retired_frames = Vec::new();

    loop {
        retire_cpp_font_rush2_frames();
        retire_frames(&mut retired_frames);
        drain_preview_input(&mut active, &mut retired_frames);
        reconcile_preview_extents(&mut active, &mut retired_frames);

        let mut desired = PREVIEW_CONTROL.lock().desired;
        if desired.serial != applied_serial {
            if !active.is_empty() {
                drain_cpp_font_rush_pending(&mut active).await;
                stop_active_previews(
                    core::mem::take(&mut active),
                    &mut retired_frames,
                    "command-replaced",
                );
            }
            // A drain can take long enough for another shell command to
            // replace the one which initiated it. Apply the newest request
            // after all old producer leases are safe, never a stale snapshot.
            desired = PREVIEW_CONTROL.lock().desired;
            applied_serial = desired.serial;
            if desired.running {
                crate::aud::audio_visualizer::set_enabled(
                    desired.config.preset == GpgpuPreviewPreset::CppAudio,
                );
                mark_starting(desired);
                match initialize_previews(desired) {
                    Ok(previews) => {
                        crate::log_info!(
                            target: "ui4";
                            "ui4 gpgpu-preview start request={} preset={} producer={} owner={:?} frames={} windows={} extent={}x{} cadence_ms={} target_hz={} frame_limit={} publish_every={} duration_ms={} buffering={} release={} plane_layout={} slot_policy=fixed-per-window/no-round-robin broker_session={} plane_mutation=none\n",
                            desired.serial,
                            desired.config.preset.label(),
                            preview_producer_label(desired.config.preset),
                            PREVIEW_OWNER,
                            previews.len(),
                            previews.iter().map(preview_surface_count).sum::<usize>(),
                            previews.first().map_or(0, |preview| preview.width),
                            previews.first().map_or(0, |preview| preview.height),
                            desired.config.cadence_ms,
                            desired.policy.target_hz,
                            desired.policy.frame_limit,
                            desired.config.publish_every,
                            desired.config.duration_ms,
                            desired.config.preset.buffering_label(),
                            preview_release_label(desired.config.preset),
                            desired.config.preset.plane_layout_label(),
                            previews.first().map_or(0, |preview| preview.session.raw()),
                        );
                        for preview in &previews {
                            crate::log_info!(
                                target: "ui4";
                                "ui4 gpgpu-preview broker-member request={} preset={} frame={} window={} slot={} extent={}x{} buffering=double consumer={} display_release=surflive\n",
                                preview.request_serial,
                                preview.config.preset.label(),
                                preview.frame.raw(),
                                preview.window.raw(),
                                active_preview_plane_slot(preview),
                                preview.width,
                                preview.height,
                                preview_consumer_label(preview.config.preset),
                            );
                        }
                        publish_active_status(&previews, GpgpuPreviewPhase::Running, "none");
                        active = previews;
                    }
                    Err(reason) => {
                        crate::aud::audio_visualizer::set_enabled(false);
                        mark_faulted(desired, reason);
                        crate::log_warn!(
                            target: "ui4";
                            "ui4 gpgpu-preview start rejected request={} reason={}\n",
                            desired.serial,
                            reason,
                        );
                    }
                }
            } else {
                crate::aud::audio_visualizer::set_enabled(false);
                mark_idle(applied_serial, "stopped");
            }
        }

        let now = Instant::now();
        let mut duration_expired = false;
        let mut render_fault = None;
        let font_rush_active = active
            .first()
            .is_some_and(|preview| preview.config.preset == GpgpuPreviewPreset::CppFontRush);
        let font_rush2_active = active
            .first()
            .is_some_and(|preview| preview.config.preset == GpgpuPreviewPreset::CppFontRush2);
        if font_rush_active {
            if let Err(reason) = poll_cpp_font_rush_consumers(&mut active) {
                render_fault = Some(reason);
            }
            let now = Instant::now();
            if render_fault.is_none()
                && let Err(reason) = grow_cpp_font_rush(&mut active, now)
            {
                render_fault = Some(reason);
            }
            for preview in &mut active {
                preview.metrics.elapsed_ms = Instant::now()
                    .saturating_duration_since(preview.started)
                    .as_millis();
                duration_expired |= preview.config.duration_ms != 0
                    && preview.metrics.elapsed_ms >= preview.config.duration_ms;
            }
            if render_fault.is_none()
                && !duration_expired
                && let Err(reason) = queue_due_cpp_font_rush_consumers(&mut active, Instant::now())
            {
                render_fault = Some(reason);
            }
        } else if font_rush2_active {
            if let Err(reason) = grow_cpp_font_rush2(&mut active, now) {
                render_fault = Some(reason);
            }
            for preview in &mut active {
                preview.metrics.elapsed_ms =
                    now.saturating_duration_since(preview.started).as_millis();
                if render_fault.is_none() && preview.static_needs_publish {
                    if let Err(reason) = render_cpp_font_rush2_frame(preview) {
                        render_fault = Some(reason);
                    }
                }
            }
        } else {
            for preview in &mut active {
                if render_fault.is_some() {
                    break;
                }
                preview.metrics.elapsed_ms =
                    now.saturating_duration_since(preview.started).as_millis();
                duration_expired |= preview.config.duration_ms != 0
                    && preview.metrics.elapsed_ms >= preview.config.duration_ms;
                if !duration_expired && preview_needs_render(preview) && now >= preview.next_render
                {
                    match render_preview_frame(preview).await {
                        Ok(()) => {
                            schedule_next_render(preview);
                        }
                        Err(reason) => render_fault = Some(reason),
                    }
                }
            }
        }
        restore_interactive_cpp_focus(&mut active);
        if !active.is_empty() && render_fault.is_none() && !duration_expired {
            publish_active_status(&active, GpgpuPreviewPhase::Running, "none");
        }

        if let Some(reason) = render_fault {
            if !active.is_empty() {
                drain_cpp_font_rush_pending(&mut active).await;
                let serial = active[0].request_serial;
                let metrics = aggregate_preview_metrics(&active);
                let members = preview_member_statuses(&active);
                stop_active_previews(
                    core::mem::take(&mut active),
                    &mut retired_frames,
                    "render-fault",
                );
                mark_runtime_fault(serial, metrics, members, reason);
                crate::log_warn!(
                    target: "ui4";
                    "ui4 gpgpu-preview faulted request={} reason={} attempted={} submitted={} completed={} published={} failed={}\n",
                    serial,
                    reason,
                    metrics.attempted,
                    metrics.submitted,
                    metrics.completed,
                    metrics.published,
                    metrics.failed,
                );
            }
        }

        if duration_expired {
            if !active.is_empty() {
                drain_cpp_font_rush_pending(&mut active).await;
                let serial = active[0].request_serial;
                let metrics = aggregate_preview_metrics(&active);
                let members = preview_member_statuses(&active);
                stop_active_previews(
                    core::mem::take(&mut active),
                    &mut retired_frames,
                    "duration-complete",
                );
                mark_duration_complete(serial, metrics, members);
            }
        }

        let wait_ms = next_preview_group_poll_ms(&active);
        Timer::after(Duration::from_millis(wait_ms)).await;
    }
}

fn initialize_previews(desired: DesiredPreview) -> Result<Vec<ActivePreview>, &'static str> {
    if desired.config.preset == GpgpuPreviewPreset::All {
        initialize_compute_preview_set(desired)
    } else if matches!(
        desired.config.preset,
        GpgpuPreviewPreset::CppFontRush | GpgpuPreviewPreset::CppFontRush2
    ) {
        initialize_cpp_font_rush_set(desired)
    } else {
        Ok(alloc::vec![initialize_preview(desired)?])
    }
}

fn initialize_compute_preview_set(
    desired: DesiredPreview,
) -> Result<Vec<ActivePreview>, &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let session =
        begin_window_session(PREVIEW_OWNER).map_err(|_| "compute-trio-session-create-failed")?;
    let (output_width, output_height) =
        crate::intel::active_scanout_dimensions().unwrap_or((2560, 1440));
    let columns = ((output_width.saturating_sub(PREVIEW_GRID_GAP))
        / PREVIEW_WIDTH.saturating_add(PREVIEW_GRID_GAP))
    .clamp(1, COMPUTE_PREVIEW_PRESETS.len() as u32);
    let mut previews = Vec::with_capacity(COMPUTE_PREVIEW_PRESETS.len());

    for (index, preset) in COMPUTE_PREVIEW_PRESETS.iter().copied().enumerate() {
        let frame = match create_preview_frame(output, PREVIEW_WIDTH, PREVIEW_HEIGHT) {
            Ok(frame) => frame,
            Err(_) => {
                abandon_compute_preview_initialization(session, &previews);
                return Err("compute-trio-frame-create-failed");
            }
        };
        let column = index as u32 % columns;
        let row = index as u32 / columns;
        let x = PREVIEW_GRID_GAP
            .saturating_add(column.saturating_mul(PREVIEW_WIDTH.saturating_add(PREVIEW_GRID_GAP)));
        let y = PREVIEW_GRID_GAP
            .saturating_add(row.saturating_mul(PREVIEW_HEIGHT.saturating_add(PREVIEW_GRID_GAP)));
        if x.saturating_add(PREVIEW_WIDTH) > output_width
            || y.saturating_add(PREVIEW_HEIGHT) > output_height
        {
            let _ = destroy_frame(frame);
            abandon_compute_preview_initialization(session, &previews);
            return Err("compute-trio-output-too-small");
        }
        let plane_slot = index + 1;
        let window = match create_window(WindowCreate {
            owner: PREVIEW_OWNER,
            session,
            frame,
            output,
            plane: WindowPlane::Universal(plane_slot as u8),
            placement: WindowPlacement {
                x: x as i32,
                y: y as i32,
                width: PREVIEW_WIDTH,
                height: PREVIEW_HEIGHT,
                z: PREVIEW_Z.saturating_add(index as i32),
                opacity: u8::MAX,
                visible: true,
            },
            interaction: super::WindowInteraction::MOVABLE_FRAME,
        }) {
            Ok(window) => window,
            Err(_) => {
                let _ = destroy_frame(frame);
                abandon_compute_preview_initialization(session, &previews);
                return Err("compute-trio-window-create-failed");
            }
        };
        let mut config = desired.config;
        config.preset = preset;
        let now = Instant::now();
        previews.push(ActivePreview {
            request_serial: desired.serial,
            config,
            policy: desired.policy,
            cadence_phase: 0,
            session,
            frame,
            window,
            width: PREVIEW_WIDTH,
            height: PREVIEW_HEIGHT,
            resize_retry_width: 0,
            resize_retry_height: 0,
            resize_retry_at: now,
            started: now,
            next_render: now,
            static_needs_publish: true,
            extra_surfaces: Vec::new(),
            particle_craft: None,
            cloud_brush: CloudBrushState::new(),
            font_stamp: None,
            font_rush: None,
            font_rush2: None,
            metrics: GpgpuPreviewMetrics::default(),
        });
    }
    Ok(previews)
}

fn abandon_compute_preview_initialization(session: WindowSessionId, previews: &[ActivePreview]) {
    let _ = finish_window_session(PREVIEW_OWNER, session);
    for preview in previews {
        let _ = destroy_frame(preview.frame);
    }
}

fn initialize_cpp_font_rush_set(
    desired: DesiredPreview,
) -> Result<Vec<ActivePreview>, &'static str> {
    if desired.config.preset == GpgpuPreviewPreset::CppFontRush2 {
        return initialize_cpp_font_rush2_set(desired);
    }
    // Recheck at the service boundary so a stale shell request cannot start a
    // hardware-plane probe over an already live UI4 scene. The probe itself
    // owns no global UI4 admission token; its enumerated planes are the only
    // display resources it requests.
    ensure_cpp_font_rush_ui4_idle("initialize")?;
    let (output, topology) = cpp_font_rush_topology()?;
    log_cpp_font_rush_capabilities("start", 1, topology);
    let session =
        begin_window_session(PREVIEW_OWNER).map_err(|_| "font-rush-session-create-failed")?;
    let started = Instant::now();
    let preview =
        match create_cpp_font_rush_preview(desired, output, session, topology, 0, started, started)
        {
            Ok(preview) => preview,
            Err(reason) => {
                let _ = finish_window_session(PREVIEW_OWNER, session);
                return Err(reason);
            }
        };
    Ok(alloc::vec![preview])
}

fn initialize_cpp_font_rush2_set(
    desired: DesiredPreview,
) -> Result<Vec<ActivePreview>, &'static str> {
    ensure_cpp_font_rush_ui4_idle("initialize-rush2")?;
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let (width, _height) =
        crate::intel::active_scanout_dimensions().unwrap_or((PREVIEW_WIDTH, PREVIEW_HEIGHT));
    let session =
        begin_window_session(PREVIEW_OWNER).map_err(|_| "font-rush2-session-create-failed")?;
    let mut previews = Vec::with_capacity(CPP_FONT_RUSH2_PRODUCER_COUNT);
    let now = Instant::now();
    for producer in 0..CPP_FONT_RUSH2_PRODUCER_COUNT {
        let frame = match create_frame(FrameSpec {
            output,
            content: FrameContent::FontScene2d,
            cadence: FrameCadence::Dirty,
            buffering: super::FrameBuffering::Double,
            format: ScanoutFormat::Rgba8888Premultiplied,
            width,
            height: CPP_FONT_RUSH2_ROW_HEIGHT,
            base_color: None,
        }) {
            Ok(frame) => frame,
            Err(error) => {
                let usage = super::ui4_live_resource_usage();
                let pmm = crate::phys::pmm_stats();
                crate::log_warn!(target: "ui4";
                    "ui4 cpp-font-rush2 frame creation rejected request={} producer={} extent={}x{} buffering=double error={:?} active_frames={} active_sessions={} live_windows={} pmm_free_bytes={} pmm_largest_free_bytes={} pmm_free_regions={}\n",
                    desired.serial,
                    producer,
                    width,
                    CPP_FONT_RUSH2_ROW_HEIGHT,
                    error,
                    usage.active_frames,
                    usage.active_sessions,
                    usage.live_windows,
                    pmm.map_or(0, |stats| stats.free_bytes),
                    pmm.map_or(0, |stats| stats.largest_free_region),
                    pmm.map_or(0, |stats| stats.free_regions),
                );
                abandon_compute_preview_initialization(session, &previews);
                return Err("font-rush2-frame-create-failed");
            }
        };
        let slot = (producer % 4) as u8;
        let row = (producer / 4) as u32;
        // Size is a registration property.  The four tiers deliberately stay
        // fixed for the lifetime of each lease while glyph and color payloads
        // change continuously.
        let font_pixels = 22.0 + (producer % 4) as f32 * 4.0;
        let registration = crate::r::font_producer_service::FontProducerRegistration {
            face: crate::intel::gpu_font::GpuFontFace::Default.id() as u16,
            tier: cpp_font_rush2_tier(producer),
            font_pixels_milli: (font_pixels * 1_000.0) as u32,
            row_width_px: width,
            row_height_px: CPP_FONT_RUSH2_ROW_HEIGHT,
            format: crate::r::font_producer_service::FontProducerFormat::Rgba8Premultiplied,
            max_chars: 1,
            row_ring_depth: 2,
        };
        let producer_lease = match crate::r::font_kernel_service::register_ui4_gpu_font_producer(
            registration,
        ) {
            Ok(producer) => producer,
            Err(error) => {
                crate::log_warn!(target: "ui4";
                    "ui4 cpp-font-rush2 producer registration rejected request={} producer={} face={} tier={} font_pixels_milli={} extent={}x{} rows={} error={:?}\n",
                    desired.serial,
                    producer,
                    registration.face,
                    registration.tier,
                    registration.font_pixels_milli,
                    registration.row_width_px,
                    registration.row_height_px,
                    registration.row_ring_depth,
                    error,
                );
                let _ = destroy_frame(frame);
                abandon_compute_preview_initialization(session, &previews);
                return Err("font-rush2-producer-register-failed");
            }
        };
        let window = match create_window(WindowCreate {
            owner: PREVIEW_OWNER,
            session,
            frame,
            output,
            plane: if slot == 0 {
                WindowPlane::Primary
            } else {
                WindowPlane::Universal(slot)
            },
            placement: WindowPlacement {
                x: 0,
                y: row.saturating_mul(CPP_FONT_RUSH2_ROW_HEIGHT) as i32,
                width,
                height: CPP_FONT_RUSH2_ROW_HEIGHT,
                z: PREVIEW_Z + row as i32,
                opacity: u8::MAX,
                visible: producer == 0,
            },
            interaction: super::WindowInteraction {
                movable: false,
                maximizable: false,
                receives_input: false,
                hit_testable: false,
                resize_on_maximize: false,
            },
        }) {
            Ok(window) => window,
            Err(_) => {
                let _ = destroy_frame(frame);
                drop(producer_lease);
                abandon_compute_preview_initialization(session, &previews);
                return Err("font-rush2-window-create-failed");
            }
        };
        let rush2 = CppFontRush2ProducerState {
            producer: producer_lease,
            producer_index: producer as u8,
            font_pixels,
            rng: crate::tyche::SoftRng::from_seed(
                desired.serial ^ (producer as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15),
            ),
            pending: None,
            published: [None, None],
        };
        let mut config = desired.config;
        config.preset = GpgpuPreviewPreset::CppFontRush2;
        previews.push(ActivePreview {
            request_serial: desired.serial,
            config,
            policy: desired.policy,
            cadence_phase: 0,
            session,
            frame,
            window,
            width,
            height: CPP_FONT_RUSH2_ROW_HEIGHT,
            resize_retry_width: 0,
            resize_retry_height: 0,
            resize_retry_at: now,
            started: now,
            next_render: now,
            static_needs_publish: producer == 0,
            extra_surfaces: Vec::new(),
            particle_craft: None,
            cloud_brush: CloudBrushState::new(),
            font_stamp: None,
            font_rush: None,
            font_rush2: Some(rush2),
            metrics: GpgpuPreviewMetrics::default(),
        });
    }
    Ok(previews)
}

fn grow_cpp_font_rush2(previews: &mut [ActivePreview], now: Instant) -> Result<(), &'static str> {
    let elapsed = now
        .saturating_duration_since(previews.first().map_or(now, |p| p.started))
        .as_millis();
    let rung = (elapsed / CPP_FONT_RUSH_STAGE_MS) as usize;
    let active = CPP_FONT_RUSH2_LADDER[rung.min(CPP_FONT_RUSH2_LADDER.len() - 1)];
    for preview in previews.iter_mut().take(active) {
        preview.static_needs_publish = true;
    }
    let windows: Vec<WindowId> = previews.iter().take(active).map(|p| p.window).collect();
    set_windows_visible(PREVIEW_OWNER, windows.as_slice(), true)
        .map(|_| ())
        .map_err(|_| "font-rush2-visibility-failed")
}

fn create_cpp_font_rush_preview(
    desired: DesiredPreview,
    output: OutputId,
    session: WindowSessionId,
    topology: CppFontRushTopology,
    rank: u8,
    started: Instant,
    activated: Instant,
) -> Result<ActivePreview, &'static str> {
    if rank >= topology.plane_count {
        return Err("font-rush-plane-unsupported");
    }
    let plane_slot = topology.plane_slots[usize::from(rank)];
    let frame = create_cpp_font_rush_frame(output, topology.width, topology.height)
        .map_err(preview_frame_create_error_label)?;
    let plane = if usize::from(plane_slot) == super::PRIMARY_PLANE_SLOT {
        WindowPlane::Primary
    } else {
        WindowPlane::Universal(plane_slot)
    };
    let window = match create_window(WindowCreate {
        owner: PREVIEW_OWNER,
        session,
        frame,
        output,
        plane,
        placement: WindowPlacement {
            x: 0,
            y: 0,
            width: topology.width,
            height: topology.height,
            z: PREVIEW_Z.saturating_add(i32::from(rank)),
            opacity: u8::MAX,
            visible: true,
        },
        interaction: super::WindowInteraction {
            movable: false,
            maximizable: false,
            receives_input: false,
            hit_testable: true,
            resize_on_maximize: false,
        },
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = destroy_frame(frame);
            return Err("font-rush-window-create-failed");
        }
    };
    Ok(ActivePreview {
        request_serial: desired.serial,
        config: desired.config,
        policy: desired.policy,
        cadence_phase: 0,
        session,
        frame,
        window,
        width: topology.width,
        height: topology.height,
        resize_retry_width: 0,
        resize_retry_height: 0,
        resize_retry_at: started,
        started,
        // Each newly added plane is an independent consumer whose cadence
        // begins when that consumer joins, not at Slot0's older start time.
        next_render: activated,
        static_needs_publish: true,
        extra_surfaces: Vec::new(),
        particle_craft: None,
        cloud_brush: CloudBrushState::new(),
        font_stamp: None,
        font_rush: Some(CppFontRushPlaneState {
            rank,
            plane_slot,
            topology,
            stage: CppFontRushLayerStage::Base,
            first_scanout_at: None,
            blank_started_at: None,
            rng: crate::tyche::soft_rng(),
            planning: None,
            ready_plan: None,
            pending: None,
            scanout_pending: None,
            showcase_sources: CppFontRushShowcaseSources::default(),
        }),
        font_rush2: None,
        metrics: GpgpuPreviewMetrics::default(),
    })
}

fn grow_cpp_font_rush(previews: &mut Vec<ActivePreview>, now: Instant) -> Result<(), &'static str> {
    let Some(first) = previews.first() else {
        return Ok(());
    };
    if first.config.preset != GpgpuPreviewPreset::CppFontRush {
        return Ok(());
    }
    let topology = first
        .font_rush
        .as_ref()
        .ok_or("font-rush-plane-state-missing")?
        .topology;
    if previews.iter().any(|preview| preview.font_rush.is_none()) {
        return Err("font-rush-plane-state-missing");
    }
    let controller_stage = first
        .font_rush
        .as_ref()
        .ok_or("font-rush-plane-state-missing")?
        .stage;
    if !matches!(controller_stage, CppFontRushLayerStage::Base | CppFontRushLayerStage::Expanded) {
        return advance_cpp_font_rush_showcase(previews, now);
    }
    let active_planes = previews.len();
    if active_planes == 0 {
        return Ok(());
    }
    if active_planes >= usize::from(topology.plane_count) {
        return expand_next_cpp_font_rush_grid(previews, now, topology);
    }
    let Some(stage_started) = previews[active_planes - 1]
        .font_rush
        .as_ref()
        .and_then(|state| state.first_scanout_at)
    else {
        return Ok(());
    };
    if now.saturating_duration_since(stage_started).as_millis() < CPP_FONT_RUSH_STAGE_MS {
        return Ok(());
    }

    // Re-read the hardware-neutral descriptor at every 3-second boundary.
    // Only one layer advances per boundary, so a delayed controller turn can
    // never collapse several stages into one catch-up burst.
    let (output, current_topology) = cpp_font_rush_topology()?;
    if current_topology != topology {
        return Err("font-rush-output-topology-changed");
    }

    let desired = DesiredPreview {
        serial: first.request_serial,
        running: true,
        config: first.config,
        policy: first.policy,
    };
    let session = first.session;
    let started = first.started;
    let rank = active_planes as u8;
    log_cpp_font_rush_capabilities("expand", active_planes, topology);
    let preview =
        create_cpp_font_rush_preview(desired, output, session, topology, rank, started, now)?;
    previews.push(preview);
    let preview = &previews[active_planes];
    let state = preview
        .font_rush
        .as_ref()
        .ok_or("font-rush-plane-state-missing")?;
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font-rush layer activated request={} elapsed_ms={} rank={} slot={} frame={} window={} active_layers={} usable_layers={} application_plane_mask=0x{:02X} glyphs={} grid={}x{} cadence_ms={} resource={}\n",
        desired.serial,
        now.saturating_duration_since(started).as_millis(),
        rank,
        state.plane_slot,
        preview.frame.raw(),
        preview.window.raw(),
        active_planes + 1,
        topology.plane_count,
        topology.plane_mask,
        cpp_font_rush_glyph_count(rank, false),
        cpp_font_rush_grid(rank, false).0,
        cpp_font_rush_grid(rank, false).1,
        desired.config.cadence_ms,
        "created",
    );
    Ok(())
}

fn expand_next_cpp_font_rush_grid(
    previews: &mut [ActivePreview],
    now: Instant,
    topology: CppFontRushTopology,
) -> Result<(), &'static str> {
    let plane_count = usize::from(topology.plane_count);
    let expanded_planes = previews
        .iter()
        .take(plane_count)
        .take_while(|preview| {
            preview
                .font_rush
                .as_ref()
                .is_some_and(|state| state.stage == CppFontRushLayerStage::Expanded)
        })
        .count();
    if expanded_planes >= plane_count {
        let Some(stage_started) = previews
            .get(plane_count.saturating_sub(1))
            .and_then(|preview| preview.font_rush.as_ref())
            .and_then(|state| state.first_scanout_at)
        else {
            return Ok(());
        };
        if now.saturating_duration_since(stage_started).as_millis() < CPP_FONT_RUSH_STAGE_MS {
            return Ok(());
        }
        return begin_cpp_font_rush_showcase(previews, now, topology);
    }

    // The first nested stage follows the final base layer; every later one
    // follows the preceding nested layer.  SURFLIVE, rather than submission
    // time, is the sole 3-second stage clock.
    let predecessor = if expanded_planes == 0 {
        plane_count.saturating_sub(1)
    } else {
        expanded_planes - 1
    };
    let Some(stage_started) = previews
        .get(predecessor)
        .and_then(|preview| preview.font_rush.as_ref())
        .and_then(|state| state.first_scanout_at)
    else {
        return Ok(());
    };
    if now.saturating_duration_since(stage_started).as_millis() < CPP_FONT_RUSH_STAGE_MS {
        return Ok(());
    }

    let (_, current_topology) = cpp_font_rush_topology()?;
    if current_topology != topology {
        return Err("font-rush-output-topology-changed");
    }
    let preview = previews
        .get_mut(expanded_planes)
        .ok_or("font-rush-expanded-layer-missing")?;
    let rank = expanded_planes as u8;
    let had_scanout_pending = {
        let state = preview
            .font_rush
            .as_mut()
            .ok_or("font-rush-plane-state-missing")?;
        if state.rank != rank || state.stage != CppFontRushLayerStage::Base {
            return Err("font-rush-expanded-layer-state");
        }
        // Never let an older base-grid completion prove the expanded stage.
        // Wait for any producer already using the layer's RNG/state, then
        // discard only its outstanding presentation proof.
        if state.planning.is_some() || state.ready_plan.is_some() || state.pending.is_some() {
            return Ok(());
        }
        let had_scanout_pending = state.scanout_pending.take().is_some();
        state.stage = CppFontRushLayerStage::Expanded;
        state.first_scanout_at = None;
        had_scanout_pending
    };
    if had_scanout_pending {
        preview.metrics.scanout_superseded = preview.metrics.scanout_superseded.saturating_add(1);
    }
    preview.cadence_phase = 0;
    preview.next_render = now;
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font-rush nested-grid activated request={} elapsed_ms={} rank={} slot={} active_layers={} expanded_layers={} base_glyphs={} glyphs={} base_grid={}x{} grid={}x{} cadence_ms={} stage_ms={} action=expand-each-base-cell-to-2x2-soft-rng-grid\n",
        preview.request_serial,
        now.saturating_duration_since(preview.started).as_millis(),
        rank,
        cpp_font_rush_plane_slot(preview)?,
        plane_count,
        expanded_planes + 1,
        cpp_font_rush_glyph_count(rank, false),
        cpp_font_rush_glyph_count(rank, true),
        cpp_font_rush_grid(rank, false).0,
        cpp_font_rush_grid(rank, false).1,
        cpp_font_rush_grid(rank, true).0,
        cpp_font_rush_grid(rank, true).1,
        preview.config.cadence_ms,
        CPP_FONT_RUSH_STAGE_MS,
    );
    Ok(())
}

fn cpp_font_rush_state_quiescent(state: &CppFontRushPlaneState) -> bool {
    state.planning.is_none()
        && state.ready_plan.is_none()
        && state.pending.is_none()
        && state.scanout_pending.is_none()
}

fn cpp_font_rush_existing_sequence_complete(previews: &[ActivePreview], now: Instant) -> bool {
    let Some(first) = previews.first() else {
        return false;
    };
    let Some(topology) = first.font_rush.as_ref().map(|state| state.topology) else {
        return false;
    };
    let plane_count = usize::from(topology.plane_count);
    if previews.len() != plane_count
        || previews.iter().any(|preview| {
            preview
                .font_rush
                .as_ref()
                .is_none_or(|state| state.stage != CppFontRushLayerStage::Expanded)
        })
    {
        return false;
    }
    previews
        .get(plane_count.saturating_sub(1))
        .and_then(|preview| preview.font_rush.as_ref())
        .and_then(|state| state.first_scanout_at)
        .is_some_and(|started| {
            now.saturating_duration_since(started).as_millis() >= CPP_FONT_RUSH_STAGE_MS
        })
}

fn cpp_font_rush_section_pulse_complete(previews: &[ActivePreview], now: Instant) -> bool {
    previews
        .first()
        .and_then(|preview| preview.font_rush.as_ref())
        .filter(|state| state.stage == CppFontRushLayerStage::SectionPulse)
        .and_then(|state| state.first_scanout_at)
        .is_some_and(|started| {
            now.saturating_duration_since(started).as_millis() >= CPP_FONT_RUSH_SECTION_DURATION_MS
        })
}

fn set_cpp_font_rush_stage(
    preview: &mut ActivePreview,
    stage: CppFontRushLayerStage,
    now: Instant,
) -> Result<(), &'static str> {
    let state = preview
        .font_rush
        .as_mut()
        .ok_or("font-rush-plane-state-missing")?;
    if !cpp_font_rush_state_quiescent(state) {
        return Err("font-rush-stage-not-quiescent");
    }
    state.stage = stage;
    state.first_scanout_at = None;
    preview.cadence_phase = 0;
    preview.next_render = now;
    Ok(())
}

fn begin_cpp_font_rush_showcase(
    previews: &mut [ActivePreview],
    now: Instant,
    topology: CppFontRushTopology,
) -> Result<(), &'static str> {
    if previews.len() != usize::from(topology.plane_count)
        || previews.iter().any(|preview| {
            preview.font_rush.as_ref().is_none_or(|state| {
                !cpp_font_rush_state_quiescent(state)
                    || state.showcase_sources.title_pending.is_some()
                    || state.showcase_sources.section_pending.is_some()
            })
        })
    {
        // The one-time source jobs share Font's FIFO/GPU lane.  Let charge-up
        // retire completely before starting the cadence-critical 150ms title
        // cards, so no old preparation can sit in front of `T` through `S`.
        return Ok(());
    }

    let hidden_windows = previews
        .iter()
        .skip(1)
        .map(|preview| preview.window)
        .collect::<Vec<_>>();
    set_windows_visible(PREVIEW_OWNER, hidden_windows.as_slice(), false)
        .map_err(|_| "font-rush-showcase-hide-layers")?;

    for preview in previews.iter_mut().skip(1) {
        set_cpp_font_rush_stage(preview, CppFontRushLayerStage::Dormant, now)?;
    }
    let request_serial = previews
        .first()
        .ok_or("font-rush-set-empty")?
        .request_serial;
    let started = previews[0].started;
    set_cpp_font_rush_stage(&mut previews[0], CppFontRushLayerStage::TitleLetter(0), now)?;
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font-rush showcase started request={} elapsed_ms={} hidden_layers={} dormant_consumers={} active_planes=1 font={} font_id={} first_stage=title-letter letters=T/R/U/E/O/S letter_ms={} final_word=TrueOS final_word_ms={} action=hide-hardware-layers-1to3+retain-primary\n",
        request_serial,
        now.saturating_duration_since(started).as_millis(),
        hidden_windows.len(),
        hidden_windows.len(),
        crate::intel::gpu_font::GpuFontFace::Default.registry_name(),
        crate::intel::gpu_font::GpuFontFace::Default.id(),
        CPP_FONT_RUSH_TITLE_LETTER_MS,
        CPP_FONT_RUSH_TITLE_HOLD_MS,
    );
    Ok(())
}

fn cpp_font_rush_showcase_next_stage(
    stage: CppFontRushLayerStage,
) -> Result<CppFontRushLayerStage, &'static str> {
    Ok(match stage {
        CppFontRushLayerStage::TitleLetter(index)
            if usize::from(index) + 1 < CPP_FONT_RUSH_TITLE_LETTERS.len() =>
        {
            CppFontRushLayerStage::TitleLetter(index + 1)
        }
        CppFontRushLayerStage::TitleLetter(_) => CppFontRushLayerStage::TitleHold,
        CppFontRushLayerStage::TitleHold => CppFontRushLayerStage::BlankPrime,
        CppFontRushLayerStage::BlankPrime => CppFontRushLayerStage::BlankHold,
        CppFontRushLayerStage::BlankHold => CppFontRushLayerStage::SectionPulse,
        CppFontRushLayerStage::SectionPulse => CppFontRushLayerStage::StormPrime { mirror: 0 },
        CppFontRushLayerStage::StormPrime { mirror: 0 } => {
            CppFontRushLayerStage::StormPrime { mirror: 1 }
        }
        CppFontRushLayerStage::StormPrime { .. } => {
            CppFontRushLayerStage::ProducerStorm { wave: 0, mirror: 0 }
        }
        CppFontRushLayerStage::ProducerStorm { wave, mirror: 0 } => {
            CppFontRushLayerStage::ProducerStorm { wave, mirror: 1 }
        }
        CppFontRushLayerStage::ProducerStorm { wave, .. } => CppFontRushLayerStage::ProducerStorm {
            wave: wave.saturating_add(1),
            mirror: 0,
        },
        CppFontRushLayerStage::Base
        | CppFontRushLayerStage::Expanded
        | CppFontRushLayerStage::Dormant => return Err("font-rush-showcase-stage"),
    })
}

fn advance_cpp_font_rush_showcase(
    previews: &mut [ActivePreview],
    now: Instant,
) -> Result<(), &'static str> {
    let first = previews.first().ok_or("font-rush-set-empty")?;
    let state = first
        .font_rush
        .as_ref()
        .ok_or("font-rush-plane-state-missing")?;
    if !cpp_font_rush_state_quiescent(state) {
        return Ok(());
    }
    let stage = state.stage;
    let blank_started_at = state.blank_started_at;
    let stage_started = match stage {
        CppFontRushLayerStage::BlankHold => {
            let Some(started) = blank_started_at else {
                return Err("font-rush-blank-start-missing");
            };
            started
        }
        _ => {
            let Some(started) = state.first_scanout_at else {
                return Ok(());
            };
            started
        }
    };
    let wait_ms = match stage {
        CppFontRushLayerStage::TitleLetter(_) => CPP_FONT_RUSH_TITLE_LETTER_MS,
        CppFontRushLayerStage::TitleHold => CPP_FONT_RUSH_TITLE_HOLD_MS,
        CppFontRushLayerStage::BlankPrime => 0,
        CppFontRushLayerStage::BlankHold => CPP_FONT_RUSH_BLANK_MIN_MS,
        CppFontRushLayerStage::SectionPulse => CPP_FONT_RUSH_SECTION_DURATION_MS,
        CppFontRushLayerStage::StormPrime { .. } => CPP_FONT_RUSH_SECTION_CADENCE_MS,
        CppFontRushLayerStage::ProducerStorm { .. } => CPP_FONT_RUSH_STORM_CADENCE_MS,
        CppFontRushLayerStage::Dormant => return Ok(()),
        CppFontRushLayerStage::Base | CppFontRushLayerStage::Expanded => {
            return Err("font-rush-showcase-stage");
        }
    };
    if now.saturating_duration_since(stage_started).as_millis() < wait_ms {
        return Ok(());
    }

    let next = cpp_font_rush_showcase_next_stage(stage)?;
    let request_serial = first.request_serial;
    let run_started = first.started;
    let elapsed_ms = now.saturating_duration_since(run_started).as_millis();
    if stage == CppFontRushLayerStage::BlankPrime {
        previews[0]
            .font_rush
            .as_mut()
            .ok_or("font-rush-plane-state-missing")?
            .blank_started_at = Some(stage_started);
    }
    set_cpp_font_rush_stage(&mut previews[0], next, now)?;
    let released_showcase_source = {
        let sources = &mut previews[0]
            .font_rush
            .as_mut()
            .ok_or("font-rush-plane-state-missing")?
            .showcase_sources;
        match stage {
            CppFontRushLayerStage::TitleHold => sources
                .title
                .take()
                .map(|source| ("TrueOS", source.surface().width, source.surface().height)),
            CppFontRushLayerStage::StormPrime { mirror: 1 } => sources
                .section
                .take()
                .map(|source| ("section-sign", source.surface().width, source.surface().height)),
            _ => None,
        }
    };
    if let Some((source, width, height)) = released_showcase_source {
        crate::log_info!(
            target: "ui4";
            "ui4 cpp-font-rush showcase source released request={} source={} extent={}x{} previous={} next={} policy=drop-immediately-after-last-proven-presentation\n",
            request_serial,
            source,
            width,
            height,
            stage.label(),
            next.label(),
        );
    }
    if next == CppFontRushLayerStage::BlankHold {
        let blank_started_at = previews[0]
            .font_rush
            .as_ref()
            .and_then(|state| state.blank_started_at)
            .ok_or("font-rush-blank-start-missing")?;
        let deadline = blank_started_at + Duration::from_millis(CPP_FONT_RUSH_BLANK_MIN_MS);
        previews[0].next_render = if deadline > now { deadline } else { now };
    }
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font-rush showcase advanced request={} elapsed_ms={} previous={} next={} cadence_ms={} clear={} backdrop=pipe-a-bottom-color font={} font_id={}\n",
        request_serial,
        elapsed_ms,
        stage.label(),
        next.label(),
        next.cadence_ms(),
        if next.clear_color().is_some() {
            "transparent"
        } else {
            "none"
        },
        crate::intel::gpu_font::GpuFontFace::Default.registry_name(),
        crate::intel::gpu_font::GpuFontFace::Default.id(),
    );
    Ok(())
}

fn initialize_preview(desired: DesiredPreview) -> Result<ActivePreview, &'static str> {
    if desired.config.preset == GpgpuPreviewPreset::CppFont {
        return initialize_cpp_font_preview(desired);
    }
    if desired.config.preset == GpgpuPreviewPreset::Static30 {
        return initialize_static30_preview(desired);
    }
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let (width, height) = preview_extent(desired.config.preset);
    let frame =
        create_preview_frame(output, width, height).map_err(preview_frame_create_error_label)?;
    let session = match begin_window_session(PREVIEW_OWNER) {
        Ok(session) => session,
        Err(_) => {
            let _ = destroy_frame(frame);
            return Err("window-session-create-failed");
        }
    };
    let (scanout_width, _) = crate::intel::active_scanout_dimensions().unwrap_or((width, height));
    let x = scanout_width.saturating_sub(width.saturating_add(PREVIEW_MARGIN)) as i32;
    let window = match create_window(WindowCreate {
        owner: PREVIEW_OWNER,
        session,
        frame,
        output,
        // A sole opaque static or released compute window on its assigned
        // slot uses UI4's direct GGTT-import + hardware-plane-flip path.
        // SURFLIVE remains the consumer-side ownership boundary.
        plane: preview_plane(desired.config.preset),
        placement: WindowPlacement {
            x,
            y: PREVIEW_MARGIN as i32,
            width,
            height,
            z: PREVIEW_Z,
            opacity: u8::MAX,
            visible: true,
        },
        // Every C++ demo consumes UI4's maximize/restore extent notification
        // and replaces its double-buffer frame. Other probes retain their
        // proven fixed-size placement behavior.
        interaction: if desired.config.preset.is_resizable_cpp() {
            super::WindowInteraction::APPLICATION
        } else {
            super::WindowInteraction::MOVABLE_FRAME
        },
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = finish_window_session(PREVIEW_OWNER, session);
            let _ = destroy_frame(frame);
            return Err("window-create-failed");
        }
    };
    let now = Instant::now();
    Ok(ActivePreview {
        request_serial: desired.serial,
        config: desired.config,
        policy: desired.policy,
        cadence_phase: 0,
        session,
        frame,
        window,
        width,
        height,
        resize_retry_width: 0,
        resize_retry_height: 0,
        resize_retry_at: now,
        started: now,
        next_render: now,
        static_needs_publish: true,
        extra_surfaces: Vec::new(),
        particle_craft: None,
        cloud_brush: CloudBrushState::new(),
        font_stamp: None,
        font_rush: None,
        font_rush2: None,
        metrics: GpgpuPreviewMetrics::default(),
    })
}

fn initialize_cpp_font_preview(desired: DesiredPreview) -> Result<ActivePreview, &'static str> {
    let request = {
        let mut queued = CPP_FONT_REQUEST.lock();
        match queued.take() {
            Some((serial, request)) if serial == desired.serial => request,
            Some(stale) => {
                *queued = Some(stale);
                return Err("font-preview-request-serial-mismatch");
            }
            None => return Err("font-preview-request-missing"),
        }
    };
    let scene = &request.layers[0].scene;
    let (width, height) = (scene.raster_width, scene.raster_height);
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let frame = create_frame(FrameSpec {
        output,
        content: FrameContent::FontScene2d,
        cadence: FrameCadence::Immutable,
        buffering: super::FrameBuffering::Single,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::from_straight_rgba(10, 14, 24, u8::MAX)),
    })
    .map_err(preview_frame_create_error_label)?;
    let session = match begin_window_session(PREVIEW_OWNER) {
        Ok(session) => session,
        Err(_) => {
            let _ = destroy_frame(frame);
            return Err("font-preview-window-session-create-failed");
        }
    };
    let (scanout_width, _) = crate::intel::active_scanout_dimensions().unwrap_or((width, height));
    let x = scanout_width.saturating_sub(width.saturating_add(PREVIEW_MARGIN)) as i32;
    let window = match create_window(WindowCreate {
        owner: PREVIEW_OWNER,
        session,
        frame,
        output,
        plane: preview_plane(GpgpuPreviewPreset::CppFont),
        placement: WindowPlacement {
            x,
            y: PREVIEW_MARGIN as i32,
            width,
            height,
            z: PREVIEW_Z,
            opacity: u8::MAX,
            visible: true,
        },
        interaction: super::WindowInteraction::MOVABLE_FRAME,
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = finish_window_session(PREVIEW_OWNER, session);
            let _ = destroy_frame(frame);
            return Err("font-preview-window-create-failed");
        }
    };
    let now = Instant::now();
    Ok(ActivePreview {
        request_serial: desired.serial,
        config: desired.config,
        policy: desired.policy,
        cadence_phase: 0,
        session,
        frame,
        window,
        width,
        height,
        resize_retry_width: 0,
        resize_retry_height: 0,
        resize_retry_at: now,
        started: now,
        next_render: now,
        static_needs_publish: true,
        extra_surfaces: Vec::new(),
        particle_craft: None,
        cloud_brush: CloudBrushState::new(),
        font_stamp: Some(request),
        font_rush: None,
        font_rush2: None,
        metrics: GpgpuPreviewMetrics::default(),
    })
}

fn initialize_static30_preview(desired: DesiredPreview) -> Result<ActivePreview, &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let (output_width, output_height) = crate::intel::active_scanout_dimensions()
        .unwrap_or((PREVIEW_WIDTH.saturating_mul(2), PREVIEW_HEIGHT.saturating_mul(2)));
    let cell_width = (output_width / STATIC30_COLUMNS).max(1);
    let cell_height = (output_height / STATIC30_ROWS).max(1);
    let mut frames = Vec::with_capacity(STATIC30_FRAME_COUNT);
    let mut surfaces = Vec::with_capacity(STATIC30_FRAME_COUNT);
    let session =
        begin_window_session(PREVIEW_OWNER).map_err(|_| "static30-session-create-failed")?;

    for index in 0..STATIC30_FRAME_COUNT {
        let index_u32 = index as u32;
        let plane_slot = index % STATIC30_PLANE_COUNT + 1;
        let plane_offset = plane_slot as u32 * 4;
        let inset = 8u32.saturating_add(plane_offset);
        let width = cell_width
            .saturating_sub(inset.saturating_mul(2))
            .min(STATIC30_MAX_WIDTH)
            .max(1);
        let height = cell_height
            .saturating_sub(inset.saturating_mul(2))
            .min(STATIC30_MAX_HEIGHT)
            .max(1);
        let x = (index_u32 % STATIC30_COLUMNS)
            .saturating_mul(cell_width)
            .saturating_add(inset);
        let y = (index_u32 / STATIC30_COLUMNS)
            .saturating_mul(cell_height)
            .saturating_add(inset);
        let frame = match create_static30_frame(output, width, height, index as u8) {
            Ok(frame) => frame,
            Err(_) => {
                abandon_static30_initialization(session, &frames);
                return Err("static30-frame-create-failed");
            }
        };
        frames.push(frame);
        let window = match create_window(WindowCreate {
            owner: PREVIEW_OWNER,
            session,
            frame,
            output,
            plane: WindowPlane::Universal(plane_slot as u8),
            placement: WindowPlacement {
                x: x as i32,
                y: y as i32,
                width,
                height,
                z: PREVIEW_Z.saturating_add(index as i32),
                opacity: u8::MAX,
                visible: true,
            },
            interaction: if desired.policy.interactive_cpp_gallery {
                super::WindowInteraction::APPLICATION_FIXED_FRAME
            } else {
                super::WindowInteraction::MOVABLE_FRAME
            },
        }) {
            Ok(window) => window,
            Err(error) => {
                crate::log_warn!(
                    target: "ui4";
                    "ui4 gpgpu-preview static30 window admission failed index={} plane_slot={} session={} active_frames={} error={:?}\n",
                    index,
                    plane_slot,
                    session.raw(),
                    frames.len(),
                    error,
                );
                abandon_static30_initialization(session, &frames);
                return Err("static30-window-create-failed");
            }
        };
        surfaces.push(StaticPreviewSurface {
            frame,
            window,
            scheme: index as u8,
        });
    }

    let first = surfaces[0];
    let now = Instant::now();
    Ok(ActivePreview {
        request_serial: desired.serial,
        config: desired.config,
        policy: desired.policy,
        cadence_phase: 0,
        session,
        frame: first.frame,
        window: first.window,
        width: cell_width.saturating_sub(24).min(STATIC30_MAX_WIDTH).max(1),
        height: cell_height
            .saturating_sub(24)
            .min(STATIC30_MAX_HEIGHT)
            .max(1),
        resize_retry_width: 0,
        resize_retry_height: 0,
        resize_retry_at: now,
        started: now,
        next_render: now,
        static_needs_publish: true,
        extra_surfaces: surfaces.iter().copied().skip(1).collect(),
        particle_craft: None,
        cloud_brush: CloudBrushState::new(),
        font_stamp: None,
        font_rush: None,
        font_rush2: None,
        metrics: GpgpuPreviewMetrics::default(),
    })
}

fn abandon_static30_initialization(session: WindowSessionId, frames: &[FrameHandle]) {
    let _ = finish_window_session(PREVIEW_OWNER, session);
    for frame in frames.iter().copied() {
        let _ = destroy_frame(frame);
    }
}

const fn preview_frame_create_error_label(error: FramePoolError) -> &'static str {
    match error {
        FramePoolError::InvalidPlan(FramePlanError::EmptyExtent) => {
            "frame-create-invalid-plan-empty-extent"
        }
        FramePoolError::InvalidPlan(FramePlanError::BaseColorRequiresPremultipliedRgba) => {
            "frame-create-invalid-plan-base-color-format"
        }
        FramePoolError::InvalidPlan(FramePlanError::VideoRequiresPremultipliedRgba) => {
            "frame-create-invalid-plan-video-format"
        }
        FramePoolError::InvalidPlan(FramePlanError::VideoRequiresStreamingCadence) => {
            "frame-create-invalid-plan-video-cadence"
        }
        FramePoolError::InvalidPlan(FramePlanError::VideoRequiresQuadBuffering) => {
            "frame-create-invalid-plan-video-buffering"
        }
        FramePoolError::InvalidPlan(FramePlanError::VideoExceedsPixelSoftCap) => {
            "frame-create-invalid-plan-video-extent"
        }
        FramePoolError::InvalidPlan(FramePlanError::RenderSceneRequiresPremultipliedRgba) => {
            "frame-create-invalid-plan-render-scene-format"
        }
        FramePoolError::InvalidPlan(FramePlanError::RenderSceneRequiresStreamingCadence) => {
            "frame-create-invalid-plan-render-scene-cadence"
        }
        FramePoolError::InvalidPlan(FramePlanError::RenderSceneRequiresTripleBuffering) => {
            "frame-create-invalid-plan-render-scene-buffering"
        }
        FramePoolError::InvalidHandle => "frame-create-invalid-handle",
        FramePoolError::UnsupportedFormat => "frame-create-unsupported-format",
        FramePoolError::OutOfMemory => "frame-create-out-of-memory",
        FramePoolError::Busy => "frame-create-busy",
        FramePoolError::ImmutablePublished => "frame-create-immutable-published",
        FramePoolError::NotPublished => "frame-create-not-published",
        FramePoolError::InvalidLease => "frame-create-invalid-lease",
        FramePoolError::ProducerReleaseRequired => "frame-create-producer-release-required",
    }
}

fn create_preview_frame(
    output: OutputId,
    width: u32,
    height: u32,
) -> Result<FrameHandle, FramePoolError> {
    create_frame(FrameSpec {
        output,
        content: FrameContent::Image,
        // Each compute dispatch waits for its completion marker before this
        // frame is published. Two buffers are therefore sufficient: the
        // published front and one producer back buffer, with Busy providing
        // backpressure while the compositor still owns the retired front.
        cadence: FrameCadence::Dirty,
        buffering: super::FrameBuffering::Double,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::from_straight_rgba(0, 0, 0, u8::MAX)),
    })
}

fn create_static30_frame(
    output: OutputId,
    width: u32,
    height: u32,
    scheme: u8,
) -> Result<FrameHandle, FramePoolError> {
    let seed = u16::from(scheme);
    create_frame(FrameSpec {
        output,
        content: FrameContent::FontScene2d,
        // Every Lorem Ipsum canvas is stamped and published exactly once. A
        // single buffer keeps the full lease/release contract honest while
        // avoiding redundant storage across the 30-window probe.
        cadence: FrameCadence::Immutable,
        buffering: super::FrameBuffering::Single,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::from_straight_rgba(
            (10 + seed * 7 % 28) as u8,
            (14 + seed * 11 % 32) as u8,
            (28 + seed * 13 % 44) as u8,
            u8::MAX,
        )),
    })
}

fn create_cpp_font_rush_frame(
    output: OutputId,
    width: u32,
    height: u32,
) -> Result<FrameHandle, FramePoolError> {
    create_frame(FrameSpec {
        output,
        content: FrameContent::FontScene2d,
        cadence: FrameCadence::Dirty,
        buffering: super::FrameBuffering::Double,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        // UiSurface creation already zeroes both ring members. Do not paint a
        // second authored backdrop: transparent RGBA exposes Pipe A's
        // persistent ColorPicker-selected bottom color from the outset.
        base_color: None,
    })
}

async fn render_preview_frame(preview: &mut ActivePreview) -> Result<(), &'static str> {
    if preview.config.preset == GpgpuPreviewPreset::CppFont {
        return render_cpp_font_frame(preview).await;
    }
    if preview.config.preset == GpgpuPreviewPreset::CppFontRush {
        return Err("font-rush-independent-controller-required");
    }
    if preview.config.preset == GpgpuPreviewPreset::Static30 {
        return render_static30_frames(preview).await;
    }
    preview.metrics.attempted = preview.metrics.attempted.saturating_add(1);
    let publish_this_frame =
        (preview.metrics.attempted - 1) % u64::from(preview.config.publish_every) == 0;
    let lease = match acquire_frame_buffer(preview.frame) {
        Ok(lease) => lease,
        Err(FramePoolError::Busy) => {
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
            return Ok(());
        }
        Err(_) => {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("frame-acquire-failed");
        }
    };
    let mut gpu_release = None;
    if preview.config.preset == GpgpuPreviewPreset::Static {
        if let Err(reason) = fill_static_preview_frame(lease, 0) {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err(reason);
        }
    } else {
        let surface = match gpgpu_rgba_surface(lease) {
            Ok(surface) => surface,
            Err(_) => {
                let _ = cancel_frame_buffer(lease);
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("gpgpu-surface-unavailable");
            }
        };

        let result = dispatch_preview_kernel(preview, surface);
        gpu_release = result.release;
        preview.metrics.last_iterations = result.iterations;
        preview.metrics.last_marker = result.marker;
        if result.submitted {
            preview.metrics.submitted = preview.metrics.submitted.saturating_add(1);
        }
        preview.metrics.last_submit_ms = result.submit_ms;
        if !result.ok {
            if result.submitted {
                // A submitted producer owns this exact back buffer until its
                // retirement marker is observed. On a genuine timeout we
                // quarantine the write lease by leaving it acquired; making
                // the surface reusable would permit late GPU writes into a
                // future frame.
                crate::log_error!(
                    target: "ui4";
                    "ui4 gpgpu-preview producer quarantine request={} preset={} frame={} buffer={} observed=0x{:08X} reason=submit-accepted-marker-timeout action=retain-write-lease-no-reuse\n",
                    preview.request_serial,
                    preview.config.preset.label(),
                    lease.frame.raw(),
                    lease.buffer_index,
                    result.marker,
                );
            } else {
                let _ = cancel_frame_buffer(lease);
            }
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err(result.error);
        }
    }
    preview.metrics.completed = preview.metrics.completed.saturating_add(1);

    if !publish_this_frame {
        let _ = cancel_frame_buffer(lease);
        return Ok(());
    }
    let publish_result = match preview.config.preset {
        GpgpuPreviewPreset::All
        | GpgpuPreviewPreset::Mandelbrot
        | GpgpuPreviewPreset::Chart
        | GpgpuPreviewPreset::Plasma
        | GpgpuPreviewPreset::Lab256
        | GpgpuPreviewPreset::CppGallery
        | GpgpuPreviewPreset::CppCloudHighWisps
        | GpgpuPreviewPreset::CppAurora
        | GpgpuPreviewPreset::CppJulia
        | GpgpuPreviewPreset::CppSdf
        | GpgpuPreviewPreset::CppVoronoi
        | GpgpuPreviewPreset::CppRetroSun
        | GpgpuPreviewPreset::CppAudio
        | GpgpuPreviewPreset::CppParticle => match gpu_release {
            Some(release) => publish_gpgpu_frame_buffer(lease, release),
            None => Err(FramePoolError::ProducerReleaseRequired),
        },
        GpgpuPreviewPreset::Static
        | GpgpuPreviewPreset::Static30
        | GpgpuPreviewPreset::CppFont
        | GpgpuPreviewPreset::CppFontRush
        | GpgpuPreviewPreset::CppFontRush2 => publish_frame_buffer(lease),
    };
    let published = match publish_result {
        Ok(published) => published,
        Err(_) => {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("frame-publish-failed");
        }
    };
    if publish_window_frame(PREVIEW_OWNER, preview.window, DamageRect::FULL).is_err() {
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        return Err("window-publish-failed");
    }
    preview.metrics.published = preview.metrics.published.saturating_add(1);
    if preview.policy.frame_limit != 0 && preview.metrics.published == preview.policy.frame_limit {
        crate::log_info!(
            target: "ui4";
            "ui4 gpgpu-preview frame-limit reached request={} preset={} published={} action=hold-last-frame producer=unchanged display_release=surflive\n",
            preview.request_serial,
            preview.config.preset.label(),
            preview.metrics.published,
        );
    }
    if !matches!(preview.config.preset, GpgpuPreviewPreset::Static | GpgpuPreviewPreset::Static30)
        && should_log_preview_checkpoint(preview.metrics.published)
    {
        crate::log_info!(
            target: "ui4";
            "ui4 gpgpu-preview frame-ready request={} preset={} frame={} buffer={} publish_serial={} marker=0x{:08X} producer_release={} consumer={} compositor_backpressure=read-lease buffering=double published={} dropped_busy={}\n",
            preview.request_serial,
            preview.config.preset.label(),
            published.frame.raw(),
            published.buffer_index,
            published.publish_serial,
            preview.metrics.last_marker,
            preview_release_label(preview.config.preset),
            preview_consumer_label(preview.config.preset),
            preview.metrics.published,
            preview.metrics.dropped_busy,
        );
    }
    if preview.config.preset == GpgpuPreviewPreset::Static {
        preview.static_needs_publish = false;
        crate::log_info!(
            target: "ui4";
            "ui4 gpgpu-preview static frame published request={} frame={} window={} submitted=0 marker=0 producer=cpu release=clflush-mfence\n",
            preview.request_serial,
            preview.frame.raw(),
            preview.window.raw(),
        );
    }
    Ok(())
}

async fn render_cpp_font_frame(preview: &mut ActivePreview) -> Result<(), &'static str> {
    preview.metrics.attempted = preview.metrics.attempted.saturating_add(1);
    let lease = match acquire_frame_buffer(preview.frame) {
        Ok(lease) => lease,
        Err(FramePoolError::Busy) => {
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
            return Ok(());
        }
        Err(_) => {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-preview-frame-acquire-failed");
        }
    };
    let destination = match gpgpu_rgba_surface(lease) {
        Ok(destination) => destination,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-preview-surface-unavailable");
        }
    };
    let Some(request) = preview.font_stamp.take() else {
        let _ = cancel_frame_buffer(lease);
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        return Err("font-preview-request-consumed");
    };
    let pending =
        match crate::r::font_kernel_service::submit_frame_stamp(request.clone(), destination) {
            Ok(pending) => pending,
            Err(crate::r::font_kernel_service::FontKernelError::QueueFull) => {
                preview.font_stamp = Some(request);
                let _ = cancel_frame_buffer(lease);
                preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
                return Ok(());
            }
            Err(_) => {
                let _ = cancel_frame_buffer(lease);
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("font-preview-submit-failed");
            }
        };
    let stamped = match pending.wait().await {
        Ok(stamped) => stamped,
        Err(crate::r::font_kernel_service::FontKernelError::SubmittedIncomplete(_)) => {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-preview-submit-incomplete");
        }
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-preview-stamp-failed");
        }
    };
    preview.metrics.submitted = preview.metrics.submitted.saturating_add(1);
    preview.metrics.completed = preview.metrics.completed.saturating_add(1);
    if publish_gpu_font_frame_buffer(lease, stamped.release()).is_err() {
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        return Err("font-preview-frame-publish-failed");
    }
    if publish_window_frame(PREVIEW_OWNER, preview.window, DamageRect::FULL).is_err() {
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        return Err("font-preview-window-publish-failed");
    }
    preview.metrics.published = preview.metrics.published.saturating_add(1);
    preview.metrics.last_iterations = stamped.glyphs() as u32;
    preview.metrics.last_marker = stamped.release().sequence() as u32;
    preview.static_needs_publish = false;
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font preview ready request={} frame={} window={} extent={}x{} glyphs={} submits={} walkers={} release={} context=kernel-gpgpu-font path=skrifa->gpu-vm-r8->cpp-igc->guc-font-rcs->ui4-font-scene\n",
        preview.request_serial,
        preview.frame.raw(),
        preview.window.raw(),
        preview.width,
        preview.height,
        stamped.glyphs(),
        stamped.submits(),
        stamped.active_walkers(),
        stamped.release().sequence(),
    );
    Ok(())
}

fn render_cpp_font_rush2_frame(preview: &mut ActivePreview) -> Result<(), &'static str> {
    use crate::r::font_kernel_service::{FontGpuProducerError, FontKernelError};
    use crate::r::font_producer_service::FontProducerError;

    preview.metrics.attempted = preview.metrics.attempted.saturating_add(1);

    let completion = preview
        .font_rush2
        .as_mut()
        .ok_or("font-rush2-producer-state-missing")?
        .pending
        .as_mut()
        .and_then(|pending| pending.pending.try_take());
    if let Some(completion) = completion {
        let pending = preview
            .font_rush2
            .as_mut()
            .and_then(|state| state.pending.take())
            .ok_or("font-rush2-pending-state-missing")?;
        let mut produced = match completion {
            Ok(produced) => produced,
            Err(FontKernelError::SubmittedIncomplete(_)) => {
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                // Keep the exact write lease acquired: the service has
                // quarantined an ambiguous GPU write to this allocation.
                return Err("font-rush2-submit-incomplete");
            }
            Err(_) => {
                let _ = cancel_frame_buffer(pending.lease);
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("font-rush2-stamp-failed");
            }
        };
        let index = usize::from(pending.lease.buffer_index);
        let exact_surface =
            gpgpu_rgba_surface(pending.lease).map_err(|_| "font-rush2-surface-lost")?;
        if produced.token().row_index() as usize != index
            || !produced
                .stamp()
                .release()
                .matches(exact_surface.phys, exact_surface.bytes)
        {
            let _ = cancel_frame_buffer(pending.lease);
            return Err("font-rush2-row-buffer-mismatch");
        }
        let published = publish_gpu_font_frame_buffer(pending.lease, produced.stamp().release())
            .map_err(|_| "font-rush2-frame-publish-failed")?;
        publish_window_frame(PREVIEW_OWNER, preview.window, DamageRect::FULL)
            .map_err(|_| "font-rush2-window-publish-failed")?;
        produced
            .mark_surflive()
            .map_err(|_| "font-rush2-surflive-state-failed")?;
        let state = preview
            .font_rush2
            .as_mut()
            .ok_or("font-rush2-producer-state-missing")?;
        if state.published[index].replace(produced).is_some() {
            return Err("font-rush2-buffer-capability-overwrite");
        }
        preview.metrics.completed = preview.metrics.completed.saturating_add(1);
        preview.metrics.published = preview.metrics.published.saturating_add(1);
        preview.metrics.last_marker = published.publish_serial as u32;
    }

    if preview
        .font_rush2
        .as_ref()
        .is_some_and(|state| state.pending.is_some())
    {
        return Ok(());
    }
    let lease = match acquire_frame_buffer(preview.frame) {
        Ok(lease) => lease,
        Err(FramePoolError::Busy) => {
            preview.metrics.dropped_busy += 1;
            return Ok(());
        }
        Err(_) => {
            preview.metrics.failed += 1;
            return Err("font-rush2-frame-acquire-failed");
        }
    };
    let state = preview
        .font_rush2
        .as_mut()
        .ok_or("font-rush2-producer-state-missing")?;
    let index = usize::from(lease.buffer_index);
    if let Some(displayed) = state.published[index].take() {
        if displayed.acknowledge_display_release().is_err() {
            let _ = cancel_frame_buffer(lease);
            return Err("font-rush2-display-ack-failed");
        }
    }
    let destination = match gpgpu_rgba_surface(lease) {
        Ok(surface) => surface,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed += 1;
            return Err("font-rush2-row-surface-failed");
        }
    };
    let request = cpp_font_rush2_request(state, preview.width, preview.height);
    let result = state.producer.submit_ui4_row(
        request,
        destination,
        lease.buffer_index,
        u32::from_le_bytes(PremultipliedRgba8::TRANSPARENT.to_native_bytes()),
    );
    let pending = match result {
        Ok(pending) => pending,
        Err(FontGpuProducerError::Kernel(FontKernelError::QueueFull))
        | Err(FontGpuProducerError::Control(FontProducerError::NoCredits)) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.dropped_queue_full =
                preview.metrics.dropped_queue_full.saturating_add(1);
            return Ok(());
        }
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-rush2-submit-failed");
        }
    };
    state.pending = Some(CppFontRush2PendingRow { lease, pending });
    preview.metrics.submitted = preview.metrics.submitted.saturating_add(1);
    Ok(())
}

fn cpp_font_rush2_request(
    state: &mut CppFontRush2ProducerState,
    width: u32,
    height: u32,
) -> crate::r::font_kernel_service::FontStampRequest {
    use crate::r::font_kernel_service::{
        FontStampFit, FontStampLayer, FontStampRequest, RetainSceneRequest,
        RetainedFontPositioning, RetainedFontRun,
    };

    let roll = state.rng.next_u64();
    let scalar = char::from_u32(33 + (roll % 94) as u32).unwrap_or('?');
    let mut text = String::new();
    text.push(scalar);
    let alpha = 80u8.saturating_add((roll >> 32) as u8 % 176);
    FontStampRequest {
        fit: FontStampFit::Canvas,
        layers: alloc::vec![FontStampLayer {
            scene: RetainSceneRequest {
                runs: alloc::vec![RetainedFontRun {
                    text,
                    position: [
                        8.0 + (state.producer_index % 4) as f32 * 2.0,
                        height as f32 * 0.72,
                    ],
                    font_pixels: state.font_pixels,
                    slant: 0.0,
                }],
                font: crate::intel::gpu_font::GpuFontFace::Default,
                viewport_width: width,
                viewport_height: height,
                raster_width: width,
                raster_height: height,
                positioning: RetainedFontPositioning::SceneOrigin,
            },
            foreground: crate::intel::gpu_font::GpuFontRgba::new(
                48u8.saturating_add((roll >> 8) as u8),
                48u8.saturating_add((roll >> 16) as u8),
                48u8.saturating_add((roll >> 24) as u8),
                alpha,
            ),
        }],
    }
}

fn poll_cpp_font_rush_consumers(previews: &mut [ActivePreview]) -> Result<(), &'static str> {
    for preview in previews {
        poll_cpp_font_rush_scanout(preview)?;
        poll_cpp_font_rush_frame(preview)?;
        poll_cpp_font_rush_plan(preview)?;
        poll_cpp_font_rush_showcase_source(preview, CppFontRushShowcaseSource::Title)?;
        poll_cpp_font_rush_showcase_source(preview, CppFontRushShowcaseSource::Section)?;
        maintain_cpp_font_rush_showcase_sources(preview)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CppFontRushShowcaseSource {
    Title,
    Section,
}

impl CppFontRushShowcaseSource {
    const fn label(self) -> &'static str {
        match self {
            Self::Title => "TrueOS",
            Self::Section => "section-sign",
        }
    }

    const fn glyphs(self) -> usize {
        match self {
            Self::Title => CPP_FONT_RUSH_TITLE_WORD.len(),
            Self::Section => 1,
        }
    }
}

fn cpp_font_rush_showcase_source_request(
    kind: CppFontRushShowcaseSource,
    width: u32,
    height: u32,
) -> crate::r::font_kernel_service::FontStampRequest {
    use crate::r::font_kernel_service::{
        FontStampFit, FontStampLayer, FontStampRequest, RetainSceneRequest,
        RetainedFontPositioning, RetainedFontRun,
    };

    let viewport_width = width.div_ceil(CPP_FONT_RUSH_VIEWPORT_SCALE).max(1);
    let viewport_height = height.div_ceil(CPP_FONT_RUSH_VIEWPORT_SCALE).max(1);
    let viewport_width_f = viewport_width as f32;
    let viewport_height_f = viewport_height as f32;
    let runs = match kind {
        CppFontRushShowcaseSource::Title => {
            let step = viewport_width_f * CPP_FONT_RUSH_TITLE_WORD_WIDTH_FRACTION
                / CPP_FONT_RUSH_TITLE_WORD.len() as f32;
            let font_pixels = (step * 1.50)
                .min(viewport_height_f * 0.90)
                .clamp(8.0, CPP_FONT_RUSH_TITLE_WORD_MAX_FONT_PIXELS);
            let runs = CPP_FONT_RUSH_TITLE_WORD
                .iter()
                .copied()
                .zip(CPP_FONT_RUSH_TITLE_WORD_X_FRACTIONS)
                .map(|(scalar, fraction)| {
                    let mut text = String::new();
                    text.push(scalar);
                    RetainedFontRun {
                        text,
                        position: [viewport_width_f * fraction, viewport_height_f * 0.5],
                        font_pixels,
                        slant: 0.0,
                    }
                })
                .collect::<Vec<_>>();
            runs
        }
        CppFontRushShowcaseSource::Section => {
            let font_pixels = (viewport_width_f * 0.16)
                .min(viewport_height_f * 0.48)
                .clamp(12.0, 196.0);
            alloc::vec![RetainedFontRun {
                text: String::from("§"),
                position: [viewport_width_f * 0.5, viewport_height_f * 0.52],
                font_pixels,
                slant: 0.0,
            }]
        }
    };
    FontStampRequest {
        layers: alloc::vec![FontStampLayer {
            scene: RetainSceneRequest {
                runs,
                font: crate::intel::gpu_font::GpuFontFace::Default,
                viewport_width,
                viewport_height,
                raster_width: width,
                raster_height: height,
                positioning: RetainedFontPositioning::VisualBoundsCenter,
            },
            // A white premultiplied source lets the sprite worklist apply the
            // stage color without regenerating coverage.
            foreground: crate::intel::gpu_font::GpuFontRgba::new(
                u8::MAX,
                u8::MAX,
                u8::MAX,
                u8::MAX,
            ),
        }],
        fit: FontStampFit::Tight,
    }
}

fn queue_cpp_font_rush_showcase_source(
    preview: &mut ActivePreview,
    kind: CppFontRushShowcaseSource,
) -> Result<(), &'static str> {
    use crate::r::font_kernel_service::FontKernelError;

    let already_owned = {
        let sources = &preview
            .font_rush
            .as_ref()
            .ok_or("font-rush-plane-state-missing")?
            .showcase_sources;
        match kind {
            CppFontRushShowcaseSource::Title => {
                sources.title.is_some() || sources.title_pending.is_some()
            }
            CppFontRushShowcaseSource::Section => {
                sources.section.is_some() || sources.section_pending.is_some()
            }
        }
    };
    if already_owned {
        return Ok(());
    }
    let request = cpp_font_rush_showcase_source_request(kind, preview.width, preview.height);
    let completion = match crate::r::font_kernel_service::submit_stamp(request) {
        Ok(completion) => completion,
        Err(FontKernelError::QueueFull) => return Ok(()),
        Err(error) => {
            crate::log_warn!(
                target: "ui4";
                "ui4 cpp-font-rush showcase source rejected request={} source={} reason={:?} action=stop-before-use\n",
                preview.request_serial,
                kind.label(),
                error,
            );
            return Err("font-rush-showcase-source-submit");
        }
    };
    let ticket = completion.ticket();
    let sources = &mut preview
        .font_rush
        .as_mut()
        .ok_or("font-rush-plane-state-missing")?
        .showcase_sources;
    match kind {
        CppFontRushShowcaseSource::Title => sources.title_pending = Some(completion),
        CppFontRushShowcaseSource::Section => sources.section_pending = Some(completion),
    }
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font-rush showcase source enqueued request={} source={} ticket={} glyphs={} fit=tight foreground=white lifecycle=run-owned-until-last-presentation completion=cooperative-signal-try-take\n",
        preview.request_serial,
        kind.label(),
        ticket.raw(),
        kind.glyphs(),
    );
    Ok(())
}

fn poll_cpp_font_rush_showcase_source(
    preview: &mut ActivePreview,
    kind: CppFontRushShowcaseSource,
) -> Result<(), &'static str> {
    let completion = {
        let sources = &mut preview
            .font_rush
            .as_mut()
            .ok_or("font-rush-plane-state-missing")?
            .showcase_sources;
        match kind {
            CppFontRushShowcaseSource::Title => sources
                .title_pending
                .as_mut()
                .and_then(|pending| pending.try_take()),
            CppFontRushShowcaseSource::Section => sources
                .section_pending
                .as_mut()
                .and_then(|pending| pending.try_take()),
        }
    };
    let Some(completion) = completion else {
        return Ok(());
    };
    let pending = {
        let sources = &mut preview
            .font_rush
            .as_mut()
            .ok_or("font-rush-plane-state-missing")?
            .showcase_sources;
        match kind {
            CppFontRushShowcaseSource::Title => sources.title_pending.take(),
            CppFontRushShowcaseSource::Section => sources.section_pending.take(),
        }
    }
    .ok_or("font-rush-showcase-source-state")?;
    let ticket = pending.ticket();
    let stamped = match completion {
        Ok(stamped) => stamped,
        Err(error) => {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            crate::log_warn!(
                target: "ui4";
                "ui4 cpp-font-rush showcase source failed request={} source={} ticket={} reason={:?} action=stop-before-sprite-use\n",
                preview.request_serial,
                kind.label(),
                ticket.raw(),
                error,
            );
            return Err("font-rush-showcase-source-failed");
        }
    };
    let surface = stamped.surface();
    if stamped.ticket() != ticket
        || stamped.glyphs() != kind.glyphs()
        || !surface.is_valid()
        || surface.width == 0
        || surface.height == 0
    {
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        return Err("font-rush-showcase-source-contract");
    }
    let origin = stamped.origin_px();
    let source = Arc::new(stamped);
    let sources = &mut preview
        .font_rush
        .as_mut()
        .ok_or("font-rush-plane-state-missing")?
        .showcase_sources;
    match kind {
        CppFontRushShowcaseSource::Title => sources.title = Some(source),
        CppFontRushShowcaseSource::Section => sources.section = Some(source),
    }
    let stage = preview
        .font_rush
        .as_ref()
        .ok_or("font-rush-plane-state-missing")?
        .stage;
    if stage.uses_showcase_sprite() {
        preview.cadence_phase = 0;
        preview.next_render = Instant::now();
    }
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font-rush showcase source ready request={} source={} ticket={} glyphs={} extent={}x{} origin={},{} submits={} walkers={} storage=private-font-rgba8 reuse=sprite-only rgba_cpu_readback=0\n",
        preview.request_serial,
        kind.label(),
        ticket.raw(),
        kind.glyphs(),
        surface.width,
        surface.height,
        origin[0],
        origin[1],
        match kind {
            CppFontRushShowcaseSource::Title => preview
                .font_rush
                .as_ref()
                .and_then(|state| state.showcase_sources.title.as_ref())
                .map_or(0, |source| source.submits()),
            CppFontRushShowcaseSource::Section => preview
                .font_rush
                .as_ref()
                .and_then(|state| state.showcase_sources.section.as_ref())
                .map_or(0, |source| source.submits()),
        },
        match kind {
            CppFontRushShowcaseSource::Title => preview
                .font_rush
                .as_ref()
                .and_then(|state| state.showcase_sources.title.as_ref())
                .map_or(0, |source| source.active_walkers()),
            CppFontRushShowcaseSource::Section => preview
                .font_rush
                .as_ref()
                .and_then(|state| state.showcase_sources.section.as_ref())
                .map_or(0, |source| source.active_walkers()),
        },
    );
    Ok(())
}

fn maintain_cpp_font_rush_showcase_sources(
    preview: &mut ActivePreview,
) -> Result<(), &'static str> {
    let (rank, stage) = preview
        .font_rush
        .as_ref()
        .map(|state| (state.rank, state.stage))
        .ok_or("font-rush-plane-state-missing")?;
    // The primary expanded experiment is the showcase charge-up.  Its later
    // sibling stages leave several seconds to materialize both tiny private
    // sources without inserting a cold build between the 150ms title cards.
    let precursor_charge = rank == 0 && stage == CppFontRushLayerStage::Expanded;
    // Never inject this relatively large source build between the six 150ms
    // title-letter frames. If the early charge could not finish, TitleHold
    // waits and performs the fallback only after every letter was presented.
    let needs_title = stage == CppFontRushLayerStage::TitleHold || precursor_charge;
    let needs_section = match stage {
        CppFontRushLayerStage::TitleHold
        | CppFontRushLayerStage::BlankPrime
        | CppFontRushLayerStage::BlankHold
        | CppFontRushLayerStage::SectionPulse
        | CppFontRushLayerStage::StormPrime { .. } => true,
        _ => precursor_charge,
    };
    if needs_title {
        queue_cpp_font_rush_showcase_source(preview, CppFontRushShowcaseSource::Title)?;
    }
    if needs_section {
        queue_cpp_font_rush_showcase_source(preview, CppFontRushShowcaseSource::Section)?;
    }
    Ok(())
}

fn poll_cpp_font_rush_plan(preview: &mut ActivePreview) -> Result<(), &'static str> {
    let completion = {
        let state = preview
            .font_rush
            .as_mut()
            .ok_or("font-rush-plane-state-missing")?;
        state
            .planning
            .as_mut()
            .and_then(|pending| pending.completion.try_take())
    };

    if let Some(completion) = completion {
        let pending = preview
            .font_rush
            .as_mut()
            .and_then(|state| state.planning.take())
            .ok_or("font-rush-plan-state-missing")?;
        let ready_at = Instant::now();
        let output = match completion {
            Ok(output) => output,
            Err(error) => {
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                crate::log_warn!(
                    target: "ui4";
                    "ui4 cpp-font-rush plan failed request={} rank={} slot={} sequence={} batch={} layout={} font={} reason={:?} elapsed_ms={} action=stop-consumer-without-frame-lease\n",
                    preview.request_serial,
                    cpp_font_rush_rank(preview)?,
                    cpp_font_rush_plane_slot(preview)?,
                    pending.sequence,
                    pending.completion.batch_id(),
                    pending.stage.label(),
                    pending.font.registry_name(),
                    error,
                    ready_at.saturating_duration_since(pending.enqueued_at).as_millis(),
                );
                return Err("font-rush-plan-build-failed");
            }
        };
        let (plan, stats) = output.into_parts();
        let expected_extent = (preview.width, preview.height);
        if plan.glyph_count() != pending.requested_glyphs
            || plan.font() != pending.font
            || (plan.raster_width(), plan.raster_height()) != expected_extent
        {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-rush-plan-contract-mismatch");
        }
        let diagnostics = plan.diagnostics();
        crate::log_info!(
            target: "ui4";
            "ui4 cpp-font-rush plan-ready request={} rank={} slot={} sequence={} batch={} layout={} font={} font_id={} glyphs={} grid={}x{} glyph_hash=0x{:016X} glyph_ids_sample={} queue_wait_ms={} build_ms={} total_ms={} attempts={} rejected={} worker_slices={} yields={} parallelism={} participants={} participant_mask=0x{:08X} ops_bytes={} reserved_ops_bytes={} estimated_work={} ownership=sealed-move-only frame_lease=none gpu_queue=none\n",
            preview.request_serial,
            cpp_font_rush_rank(preview)?,
            cpp_font_rush_plane_slot(preview)?,
            pending.sequence,
            stats.batch_id(),
            pending.stage.label(),
            pending.font.registry_name(),
            pending.font.id(),
            plan.glyph_count(),
            pending.columns,
            pending.rows,
            diagnostics.glyph_fingerprint(),
            diagnostics.glyph_ids_sample(),
            stats.queue_wait_ms(),
            stats.build_ms(),
            ready_at.saturating_duration_since(pending.enqueued_at).as_millis(),
            stats.candidate_attempts(),
            stats.rejected_candidates(),
            stats.worker_slices(),
            stats.cooperative_yields(),
            stats.parallelism(),
            stats.participants(),
            stats.participant_mask(),
            plan.ops_bytes(),
            stats.reserved_ops_bytes(),
            plan.estimated_work(),
        );
        let state = preview
            .font_rush
            .as_mut()
            .ok_or("font-rush-plane-state-missing")?;
        state.ready_plan = Some(CppFontRushReadyPlan {
            plan,
            stats,
            sequence: pending.sequence,
            scheduled_at: pending.scheduled_at,
            enqueued_at: pending.enqueued_at,
            ready_at,
            requested_glyphs: pending.requested_glyphs,
            columns: pending.columns,
            rows: pending.rows,
            stage: pending.stage,
            font: pending.font,
            submit_attempts: 0,
        });
    }

    try_submit_cpp_font_rush_ready_plan(preview)
}

fn try_submit_cpp_font_rush_ready_plan(preview: &mut ActivePreview) -> Result<(), &'static str> {
    use crate::r::font_kernel_service::FontKernelError;

    let has_ready_plan = preview
        .font_rush
        .as_ref()
        .ok_or("font-rush-plane-state-missing")?
        .ready_plan
        .is_some();
    if !has_ready_plan {
        return Ok(());
    }
    if preview
        .font_rush
        .as_ref()
        .is_some_and(|state| state.pending.is_some())
    {
        return Err("font-rush-plan-and-frame-in-flight");
    }

    let rank = cpp_font_rush_rank(preview)?;
    let plane_slot = cpp_font_rush_plane_slot(preview)?;
    let lease = match acquire_frame_buffer(preview.frame) {
        Ok(lease) => lease,
        Err(FramePoolError::Busy) => return Ok(()),
        Err(_) => {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-rush-frame-acquire-failed");
        }
    };
    let destination = match gpgpu_rgba_surface(lease) {
        Ok(destination) => destination,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-rush-surface-unavailable");
        }
    };
    let ready = preview
        .font_rush
        .as_mut()
        .and_then(|state| state.ready_plan.take())
        .ok_or("font-rush-ready-plan-missing")?;
    let CppFontRushReadyPlan {
        plan,
        stats,
        sequence,
        scheduled_at,
        enqueued_at,
        ready_at,
        requested_glyphs,
        columns,
        rows,
        stage,
        font,
        submit_attempts,
    } = ready;
    let glyph_fingerprint = plan.diagnostics().glyph_fingerprint();
    let glyph_ids_sample = String::from(plan.diagnostics().glyph_ids_sample());
    let prepared_ops_bytes = plan.ops_bytes();
    let prepared_work = plan.estimated_work();
    let submit_started_at = Instant::now();
    let submitted = if let Some(clear_color) = stage.clear_color() {
        let clear_rgba = u32::from_le_bytes(clear_color.to_native_bytes());
        crate::r::font_kernel_service::submit_prepared_frame_stamp_with_clear(
            plan,
            destination,
            clear_rgba,
        )
    } else {
        crate::r::font_kernel_service::submit_prepared_frame_stamp(plan, destination)
    };
    let completion = match submitted {
        Ok(pending) => pending,
        Err(rejection) => {
            let (error, plan) = rejection.into_parts();
            let _ = cancel_frame_buffer(lease);
            if error == FontKernelError::QueueFull {
                let next_attempt = submit_attempts.saturating_add(1);
                preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
                preview.metrics.dropped_queue_full =
                    preview.metrics.dropped_queue_full.saturating_add(1);
                if next_attempt == 1 || next_attempt.is_power_of_two() {
                    crate::log_warn!(
                        target: "ui4";
                        "ui4 cpp-font-rush prepared handoff backpressure request={} rank={} slot={} sequence={} batch={} attempt={} reason=font-queue-full service_queued={} action=return-exact-plan+retry-without-rebuild\n",
                        preview.request_serial,
                        rank,
                        plane_slot,
                        sequence,
                        stats.batch_id(),
                        next_attempt,
                        crate::r::font_kernel_service::status().queued,
                    );
                }
                preview
                    .font_rush
                    .as_mut()
                    .ok_or("font-rush-plane-state-missing")?
                    .ready_plan = Some(CppFontRushReadyPlan {
                    plan,
                    stats,
                    sequence,
                    scheduled_at,
                    enqueued_at,
                    ready_at,
                    requested_glyphs,
                    columns,
                    rows,
                    stage,
                    font,
                    submit_attempts: next_attempt,
                });
                return Ok(());
            }
            drop(plan);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-rush-submit-failed");
        }
    };
    let accepted_at = Instant::now();
    let ticket = completion.ticket();
    let fifo_queued_ahead = completion.queued_ahead();
    preview.metrics.submitted = preview.metrics.submitted.saturating_add(1);
    preview
        .font_rush
        .as_mut()
        .ok_or("font-rush-plane-state-missing")?
        .pending = Some(CppFontRushPendingFrame {
        lease,
        completion,
        ticket,
        sequence,
        scheduled_at,
        submit_started_at,
        accepted_at,
        requested_glyphs,
        columns,
        rows,
        stage,
        font,
        glyph_fingerprint,
        glyph_ids_sample: glyph_ids_sample.clone(),
        fifo_queued_ahead,
        plan_batch_id: stats.batch_id(),
        plan_enqueue_delay_ms: enqueued_at
            .saturating_duration_since(scheduled_at)
            .as_millis(),
        plan_queue_wait_ms: stats.queue_wait_ms(),
        plan_build_ms: stats.build_ms(),
        plan_total_ms: ready_at.saturating_duration_since(enqueued_at).as_millis(),
        plan_candidate_attempts: stats.candidate_attempts(),
        plan_rejected_candidates: stats.rejected_candidates(),
        plan_worker_slices: stats.worker_slices(),
        plan_cooperative_yields: stats.cooperative_yields(),
        plan_parallelism: stats.parallelism(),
        prepared_ops_bytes,
        prepared_reserved_ops_bytes: stats.reserved_ops_bytes(),
        prepared_work,
    });
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font-rush consumer enqueued request={} consumer={} rank={} slot={} sequence={} batch={} ticket={} frame={} producer_buffer={} layout={} font={} font_id={} glyph_hash=0x{:016X} glyph_ids_sample={} glyph_ids_sample_limit={} glyphs={} grid={}x{} scheduled_ms={} plan_enqueue_delay_ms={} plan_queue_wait_ms={} plan_build_ms={} plan_total_ms={} plan_attempts={} plan_rejected={} plan_worker_slices={} plan_yields={} plan_parallelism={} submit_call_us={} prepared_ops_bytes={} prepared_reserved_ops_bytes={} prepared_segment_evaluations={} prepared_storage=transient-move-once prepared_replay=0 fifo_queued_ahead={} consumer_in_flight=1 consumer_pending_limit=1 service_model=plan-pool-32->font-fifo-32+one-global-gpu-in-flight\n",
        preview.request_serial,
        rank,
        rank,
        plane_slot,
        sequence,
        stats.batch_id(),
        ticket.raw(),
        lease.frame.raw(),
        lease.buffer_index,
        stage.label(),
        font.registry_name(),
        font.id(),
        glyph_fingerprint,
        glyph_ids_sample,
        CPP_FONT_RUSH_GLYPH_ID_LOG_LIMIT,
        requested_glyphs,
        columns,
        rows,
        scheduled_at.saturating_duration_since(preview.started).as_millis(),
        enqueued_at.saturating_duration_since(scheduled_at).as_millis(),
        stats.queue_wait_ms(),
        stats.build_ms(),
        ready_at.saturating_duration_since(enqueued_at).as_millis(),
        stats.candidate_attempts(),
        stats.rejected_candidates(),
        stats.worker_slices(),
        stats.cooperative_yields(),
        stats.parallelism(),
        accepted_at.saturating_duration_since(submit_started_at).as_micros(),
        prepared_ops_bytes,
        stats.reserved_ops_bytes(),
        prepared_work,
        fifo_queued_ahead,
    );
    Ok(())
}

fn poll_cpp_font_rush_scanout(preview: &mut ActivePreview) -> Result<(), &'static str> {
    let state = preview
        .font_rush
        .as_mut()
        .ok_or("font-rush-plane-state-missing")?;
    let Some(pending) = state.scanout_pending else {
        return Ok(());
    };
    let ready_serial = crate::intel::ui4_direct_scanout_ready_for_frame(preview.frame.raw());
    if !ready_serial.is_some_and(|serial| serial >= pending.frame_publish_serial) {
        return Ok(());
    }
    let live_at = Instant::now();
    state.scanout_pending = None;
    if pending.stage != state.stage {
        return Err("font-rush-scanout-layout-mismatch");
    }
    if state.first_scanout_at.is_none() {
        state.first_scanout_at = Some(live_at);
    }
    preview.metrics.scanout_live = preview.metrics.scanout_live.saturating_add(1);
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font-rush scanout-live request={} rank={} slot={} sequence={} ticket={} frame={} producer_buffer={} frame_publish_serial={} window_publish_serial={} release={} layout={} glyphs={} grid={}x{} font={} font_id={} glyph_hash=0x{:016X} publish_to_surflive_us={} proof_serial={} path={} compositor_jobs=0\n",
        preview.request_serial,
        state.rank,
        state.plane_slot,
        pending.sequence,
        pending.ticket.raw(),
        preview.frame.raw(),
        pending.producer_buffer,
        pending.frame_publish_serial,
        pending.window_publish_serial,
        pending.release_sequence,
        pending.stage.label(),
        pending.requested_glyphs,
        pending.columns,
        pending.rows,
        pending.font.registry_name(),
        pending.font.id(),
        pending.glyph_fingerprint,
        live_at.saturating_duration_since(pending.published_at).as_micros(),
        ready_serial.unwrap_or(0),
        match pending.stage {
            CppFontRushLayerStage::BlankPrime => {
                "font-clear-only->ui4-rgba8->display-plane-direct"
            }
            CppFontRushLayerStage::ProducerStorm { .. } => {
                "plan-pool-32->font-r8-coverage-and-mask-batch->display-plane-direct"
            }
            stage if stage.uses_showcase_sprite() => {
                "tight-font-rgba8->scaled-font-sprite-worklist->display-plane-direct"
            }
            _ => "ui4-font-scene->display-plane-direct",
        },
    );
    Ok(())
}

fn poll_cpp_font_rush_frame(preview: &mut ActivePreview) -> Result<(), &'static str> {
    use crate::r::font_kernel_service::FontKernelError;

    let completion = {
        let state = preview
            .font_rush
            .as_mut()
            .ok_or("font-rush-plane-state-missing")?;
        state
            .pending
            .as_mut()
            .and_then(|pending| pending.completion.try_take())
    };
    let Some(completion) = completion else {
        return Ok(());
    };
    let pending = preview
        .font_rush
        .as_mut()
        .and_then(|state| state.pending.take())
        .ok_or("font-rush-pending-state-missing")?;
    let completed_at = Instant::now();
    let stamped = match completion {
        Ok(stamped) => stamped,
        Err(FontKernelError::SubmittedIncomplete(reason)) => {
            // The worker may still reference this exact allocation. Removing
            // the state intentionally leaves its frame write lease acquired,
            // quarantining the destination until reboot.
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            crate::log_error!(
                target: "ui4";
                "ui4 cpp-font-rush consumer quarantined request={} rank={} sequence={} ticket={} frame={} buffer={} reason={} action=retain-frame-write-lease\n",
                preview.request_serial,
                cpp_font_rush_rank(preview)?,
                pending.sequence,
                pending.ticket.raw(),
                pending.lease.frame.raw(),
                pending.lease.buffer_index,
                reason,
            );
            return Err("font-rush-submit-incomplete");
        }
        Err(error) => {
            let _ = cancel_frame_buffer(pending.lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            crate::log_warn!(
                target: "ui4";
                "ui4 cpp-font-rush consumer failed request={} rank={} sequence={} ticket={} stage=font-service reason={:?} action=cancel-complete-write-lease\n",
                preview.request_serial,
                cpp_font_rush_rank(preview)?,
                pending.sequence,
                pending.ticket.raw(),
                error,
            );
            return Err("font-rush-stamp-failed");
        }
    };
    if stamped.ticket() != pending.ticket {
        let _ = cancel_frame_buffer(pending.lease);
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        return Err("font-rush-ticket-mismatch");
    }
    preview.metrics.completed = preview.metrics.completed.saturating_add(1);
    let release = stamped.release();
    let published = match publish_gpu_font_frame_buffer(pending.lease, release) {
        Ok(published) => published,
        Err(_) => {
            let _ = cancel_frame_buffer(pending.lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-rush-frame-publish-failed");
        }
    };
    let window_publish_serial =
        match publish_window_frame(PREVIEW_OWNER, preview.window, DamageRect::FULL) {
            Ok(serial) => serial,
            Err(_) => {
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("font-rush-window-publish-failed");
            }
        };
    let published_at = Instant::now();
    preview.metrics.published = preview.metrics.published.saturating_add(1);
    preview.metrics.last_iterations = stamped.glyphs() as u32;
    preview.metrics.last_marker = release.sequence() as u32;
    preview.metrics.last_submit_ms = completed_at
        .saturating_duration_since(pending.submit_started_at)
        .as_millis();

    let (rank, plane_slot, topology, replaced_scanout) = {
        let state = preview
            .font_rush
            .as_mut()
            .ok_or("font-rush-plane-state-missing")?;
        let replaced = state.scanout_pending.replace(CppFontRushPendingScanout {
            ticket: pending.ticket,
            sequence: pending.sequence,
            producer_buffer: published.buffer_index,
            frame_publish_serial: published.publish_serial,
            window_publish_serial,
            release_sequence: release.sequence(),
            published_at,
            requested_glyphs: pending.requested_glyphs,
            columns: pending.columns,
            rows: pending.rows,
            stage: pending.stage,
            font: pending.font,
            glyph_fingerprint: pending.glyph_fingerprint,
        });
        (state.rank, state.plane_slot, state.topology, replaced)
    };
    if let Some(replaced) = replaced_scanout {
        preview.metrics.scanout_superseded = preview.metrics.scanout_superseded.saturating_add(1);
        crate::log_warn!(
            target: "ui4";
            "ui4 cpp-font-rush scanout proof superseded request={} rank={} old_sequence={} old_frame_publish_serial={} new_sequence={} new_frame_publish_serial={} scanout_superseded={} action=count-present-drop-candidate\n",
            preview.request_serial,
            rank,
            replaced.sequence,
            replaced.frame_publish_serial,
            pending.sequence,
            published.publish_serial,
            preview.metrics.scanout_superseded,
        );
    }
    let service = crate::r::font_kernel_service::status();
    if pending.stage == CppFontRushLayerStage::BlankPrime {
        crate::log_info!(
            target: "ui4";
            "ui4 cpp-font-rush blank frame-ready request={} sequence={} ticket={} frame={} producer_buffer={} frame_publish_serial={} window_publish_serial={} release={} clear_submits={} release_submits={} pre_service_ms={} clear_ms={} service_ms={} fifo_queued_ahead={} completion_to_publish_us={} clear=transparent once=1 blank_min_ms={} timer=start-on-surflive planning=0 skrifa=0 tessellation=0 coverage=0 shading=0\n",
            preview.request_serial,
            pending.sequence,
            pending.ticket.raw(),
            preview.frame.raw(),
            published.buffer_index,
            published.publish_serial,
            window_publish_serial,
            release.sequence(),
            stamped.clear_submits(),
            stamped.submits(),
            stamped.pre_service_ms(),
            stamped.clear_ms(),
            stamped.total_service_ms(),
            pending.fifo_queued_ahead,
            published_at.saturating_duration_since(completed_at).as_micros(),
            CPP_FONT_RUSH_BLANK_MIN_MS,
        );
        return Ok(());
    }
    if let CppFontRushLayerStage::ProducerStorm { wave, mirror } = pending.stage {
        crate::log_info!(
            target: "ui4";
            "ui4 cpp-font-rush raw producer frame-ready request={} wave={} mirror={} sequence={} ticket={} frame={} producer_buffer={} frame_publish_serial={} window_publish_serial={} extent={}x{} glyphs={} submits={} walkers={} release={} pre_service_ms={} service_ms={} fifo_queued_ahead={} service_queue_at_complete={} plan_batch={} plan_parallelism={} plan_build_ms={} plan_worker_slices={} plan_yields={} cadence_ms={} raw_producers={} anchors_per_producer={} pixel_op=ordered-premultiplied-source-over no_clear=1 rgba_cpu_readback=0 compositor_jobs=0 path=plan-pool-32->font-r8-coverage-and-mask-batch->guc-font-rcs->ui4-rgba8->display-plane-direct\n",
            preview.request_serial,
            wave,
            mirror,
            pending.sequence,
            pending.ticket.raw(),
            preview.frame.raw(),
            published.buffer_index,
            published.publish_serial,
            window_publish_serial,
            preview.width,
            preview.height,
            stamped.glyphs(),
            stamped.submits(),
            stamped.active_walkers(),
            release.sequence(),
            stamped.pre_service_ms(),
            stamped.total_service_ms(),
            pending.fifo_queued_ahead,
            service.queued,
            pending.plan_batch_id,
            pending.plan_parallelism,
            pending.plan_build_ms,
            pending.plan_worker_slices,
            pending.plan_cooperative_yields,
            pending.stage.cadence_ms(),
            crate::r::font_plan_service::FONT_PLAN_WORKER_COUNT,
            CPP_FONT_RUSH_STORM_GLYPHS_PER_PRODUCER,
        );
        return Ok(());
    }
    if pending.stage.uses_showcase_sprite() {
        crate::log_info!(
            target: "ui4";
            "ui4 cpp-font-rush showcase sprite frame-ready request={} sequence={} ticket={} frame={} producer_buffer={} frame_publish_serial={} window_publish_serial={} layout={} glyphs={} clear_submits={} sprite_submits={} walkers={} release={} pre_service_ms={} clear_ms={} service_ms={} fifo_queued_ahead={} cadence_ms={} planning=0 skrifa=0 tessellation=0 coverage=0 rgba_cpu_readback=0 compositor_jobs=0 path=tight-font-rgba8->guc-font-rcs-scaled-tinted-sprite->ui4-rgba8->display-plane-direct\n",
            preview.request_serial,
            pending.sequence,
            pending.ticket.raw(),
            preview.frame.raw(),
            published.buffer_index,
            published.publish_serial,
            window_publish_serial,
            pending.stage.label(),
            stamped.glyphs(),
            stamped.clear_submits(),
            stamped.submits(),
            stamped.active_walkers(),
            release.sequence(),
            stamped.pre_service_ms(),
            stamped.clear_ms(),
            stamped.total_service_ms(),
            pending.fifo_queued_ahead,
            pending.stage.cadence_ms(),
        );
        return Ok(());
    }
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font-rush frame-ready request={} consumer={} rank={} slot={} sequence={} publication={} ticket={} elapsed_ms={} scheduled_ms={} plan_batch={} plan_enqueue_delay_ms={} plan_queue_wait_ms={} plan_build_ms={} plan_total_ms={} plan_candidate_attempts={} plan_rejected_candidates={} plan_worker_slices={} plan_yields={} plan_parallelism={} deadline_to_submit_ms={} submit_call_us={} prepared_ops_bytes={} prepared_reserved_ops_bytes={} prepared_segment_evaluations={} gpu_wait_ms={} pre_service_ms={} clear_ms={} prepare_coverage_ms={} coverage_build_ms={} coverage_audit_ms={} instance_release_ms={} service_ms={} fifo_queued_ahead={} service_queue_at_complete={} completion_to_publish_us={} frame={} producer_buffer={} frame_publish_serial={} window_publish_serial={} layout={} glyph_hash=0x{:016X} glyph_ids_sample={} glyph_ids_sample_limit={} extent={}x{} cadence_ms={} requested_glyphs={} rendered_glyphs={} grid={}x{} font={} font_id={} application_plane_mask=0x{:02X} usable_planes={} clear_submits={} coverage_submits={} instance_release_submits={} known_gpu_submits={} walkers={} release={} consumer_in_flight=0 context=kernel-gpgpu-font path=registered-skrifa->ecore-plan-pool[request-local-recipes]->sealed-plan->font-service-fifo[retained-union-coverage->optional-transparent-gpu-clear->cpp-igc->guc-font-rcs]->ui4-font-scene->display-plane-direct backdrop=pipe-a-bottom-color pixel_alpha=premultiplied prepared_replay=exact-plan prepared_storage=request-local gpu_storage=none compositor_jobs=0 rgba_cpu_readback=0 cpu_frame_copy=0\n",
        preview.request_serial,
        rank,
        rank,
        plane_slot,
        pending.sequence,
        preview.metrics.published,
        pending.ticket.raw(),
        completed_at.saturating_duration_since(preview.started).as_millis(),
        pending.scheduled_at.saturating_duration_since(preview.started).as_millis(),
        pending.plan_batch_id,
        pending.plan_enqueue_delay_ms,
        pending.plan_queue_wait_ms,
        pending.plan_build_ms,
        pending.plan_total_ms,
        pending.plan_candidate_attempts,
        pending.plan_rejected_candidates,
        pending.plan_worker_slices,
        pending.plan_cooperative_yields,
        pending.plan_parallelism,
        pending
            .submit_started_at
            .saturating_duration_since(pending.scheduled_at)
            .as_millis(),
        pending
            .accepted_at
            .saturating_duration_since(pending.submit_started_at)
            .as_micros(),
        pending.prepared_ops_bytes,
        pending.prepared_reserved_ops_bytes,
        pending.prepared_work,
        completed_at
            .saturating_duration_since(pending.submit_started_at)
            .as_millis(),
        stamped.pre_service_ms(),
        stamped.clear_ms(),
        stamped.prepare_coverage_ms(),
        stamped.coverage_build_ms(),
        stamped.coverage_audit_ms(),
        stamped.instance_release_ms(),
        stamped.total_service_ms(),
        pending.fifo_queued_ahead,
        service.queued,
        published_at
            .saturating_duration_since(completed_at)
            .as_micros(),
        preview.frame.raw(),
        published.buffer_index,
        published.publish_serial,
        window_publish_serial,
        pending.stage.label(),
        pending.glyph_fingerprint,
        pending.glyph_ids_sample,
        CPP_FONT_RUSH_GLYPH_ID_LOG_LIMIT,
        preview.width,
        preview.height,
        pending.stage.cadence_ms(),
        pending.requested_glyphs,
        stamped.glyphs(),
        pending.columns,
        pending.rows,
        pending.font.registry_name(),
        pending.font.id(),
        topology.plane_mask,
        topology.plane_count,
        stamped.clear_submits(),
        stamped.coverage_submits(),
        stamped.submits(),
        stamped
            .clear_submits()
            .saturating_add(stamped.coverage_submits())
            .saturating_add(stamped.submits()),
        stamped.active_walkers(),
        release.sequence(),
    );
    Ok(())
}

fn queue_due_cpp_font_rush_consumers(
    previews: &mut [ActivePreview],
    now: Instant,
) -> Result<(), &'static str> {
    // Once the last legacy expanded layer has had its full three seconds,
    // stop admission immediately and let the four existing pipelines drain.
    // The showcase transition can then hide layers 1-3 without racing an old
    // stage token into SURFLIVE.
    if cpp_font_rush_existing_sequence_complete(previews, now)
        || cpp_font_rush_section_pulse_complete(previews, now)
    {
        return Ok(());
    }
    let font = crate::intel::gpu_font::GpuFontFace::Default;
    for preview in previews {
        if !preview_needs_render(preview) {
            continue;
        }
        let state = preview
            .font_rush
            .as_ref()
            .ok_or("font-rush-plane-state-missing")?;
        let stage = state.stage;
        // Preserve one publication -> one SURFLIVE proof.  Replacing the
        // sole scanout token before the display observes it can otherwise
        // starve a repeating stage's first_scanout_at clock indefinitely.
        if !stage.produces_frames()
            || state.scanout_pending.is_some()
            || (!stage.repeats_while_live() && state.first_scanout_at.is_some())
        {
            continue;
        }
        if stage.uses_showcase_sprite() && !cpp_font_rush_showcase_source_ready(state, stage) {
            // Completion is observed cooperatively through Signal::try_take.
            // Defer this overdue frame instead of turning source readiness
            // into a 1ms UI polling loop; the completion path pulls the
            // deadline back to `now` as soon as the source is available.
            preview.next_render = now + Duration::from_millis(COMMAND_POLL_MAX_MS);
            continue;
        }
        let rank = cpp_font_rush_rank(preview)?;
        let plane_slot = cpp_font_rush_plane_slot(preview)?;
        let Some((due_ticks, scheduled_at)) = take_cpp_font_rush_due_ticks(preview, now) else {
            continue;
        };
        preview.metrics.attempted = preview.metrics.attempted.saturating_add(due_ticks);
        let superseded = due_ticks.saturating_sub(1);
        if superseded != 0 {
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(superseded);
            preview.metrics.dropped_cadence =
                preview.metrics.dropped_cadence.saturating_add(superseded);
            preview.metrics.late = preview.metrics.late.saturating_add(superseded);
            crate::log_warn!(
                target: "ui4";
                "ui4 cpp-font-rush consumer backpressure request={} rank={} slot={} sequence={} due_ticks={} drop=cadence-superseded dropped_intervals={} service_queued={} cadence_ms={} policy=keep-latest-tick+preserve-phase\n",
                preview.request_serial,
                rank,
                plane_slot,
                preview.metrics.attempted,
                due_ticks,
                superseded,
                crate::r::font_kernel_service::status().queued,
                stage.cadence_ms(),
            );
        }
        let sequence = preview.metrics.attempted;
        let state = preview
            .font_rush
            .as_ref()
            .ok_or("font-rush-plane-state-missing")?;
        if let Some((producer_stage, active_sequence, active_since)) = state
            .planning
            .as_ref()
            .map(|plan| ("planning", plan.sequence, plan.enqueued_at))
            .or_else(|| {
                state
                    .ready_plan
                    .as_ref()
                    .map(|plan| ("plan-ready", plan.sequence, plan.ready_at))
            })
        {
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
            preview.metrics.dropped_in_flight = preview.metrics.dropped_in_flight.saturating_add(1);
            preview.metrics.late = preview.metrics.late.saturating_add(1);
            crate::log_warn!(
                target: "ui4";
                "ui4 cpp-font-rush consumer backpressure request={} rank={} slot={} sequence={} due_ticks={} drop=in-flight producer_stage={} active_sequence={} in_flight_ms={} plan_batches={} service_queued={} cadence_ms={} next_deadline_ms={}\n",
                preview.request_serial,
                rank,
                plane_slot,
                sequence,
                due_ticks,
                producer_stage,
                active_sequence,
                now.saturating_duration_since(active_since).as_millis(),
                crate::r::font_plan_service::status().active_batches,
                crate::r::font_kernel_service::status().queued,
                stage.cadence_ms(),
                preview.next_render.saturating_duration_since(preview.started).as_millis(),
            );
            continue;
        }
        let pending = state.pending.as_ref();
        if let Some(pending) = pending {
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
            preview.metrics.dropped_in_flight = preview.metrics.dropped_in_flight.saturating_add(1);
            preview.metrics.late = preview.metrics.late.saturating_add(1);
            crate::log_warn!(
                target: "ui4";
                "ui4 cpp-font-rush consumer backpressure request={} rank={} slot={} sequence={} due_ticks={} drop=in-flight active_ticket={} active_sequence={} in_flight_ms={} service_queued={} cadence_ms={} next_deadline_ms={}\n",
                preview.request_serial,
                rank,
                plane_slot,
                sequence,
                due_ticks,
                pending.ticket.raw(),
                pending.sequence,
                now.saturating_duration_since(pending.submit_started_at).as_millis(),
                crate::r::font_kernel_service::status().queued,
                stage.cadence_ms(),
                preview.next_render.saturating_duration_since(preview.started).as_millis(),
            );
            continue;
        }
        if stage == CppFontRushLayerStage::BlankPrime {
            queue_cpp_font_rush_blank(preview, sequence, scheduled_at)?;
            continue;
        }
        if stage.uses_showcase_sprite() {
            queue_cpp_font_rush_showcase_sprite(preview, sequence, scheduled_at)?;
            continue;
        }
        queue_cpp_font_rush_plan(preview, sequence, scheduled_at, font)?;
    }
    Ok(())
}

fn cpp_font_rush_showcase_source_ready(
    state: &CppFontRushPlaneState,
    stage: CppFontRushLayerStage,
) -> bool {
    match stage {
        CppFontRushLayerStage::TitleHold => state.showcase_sources.title.is_some(),
        CppFontRushLayerStage::SectionPulse | CppFontRushLayerStage::StormPrime { .. } => {
            state.showcase_sources.section.is_some()
        }
        _ => true,
    }
}

struct CppFontRushShowcaseSpriteLayout {
    descriptors: Vec<crate::intel::gpgpu::GpgpuSpriteQuadWorklistDesc>,
    scale: f32,
    glyphs: usize,
    columns: u8,
    color_rgba: u32,
}

fn cpp_font_rush_section_foreground(sequence: u64) -> crate::intel::gpu_font::GpuFontRgba {
    match sequence % 6 {
        0 => crate::intel::gpu_font::GpuFontRgba::new(255, 67, 173, u8::MAX),
        1 => crate::intel::gpu_font::GpuFontRgba::new(255, 180, 49, u8::MAX),
        2 => crate::intel::gpu_font::GpuFontRgba::new(93, 245, 255, u8::MAX),
        3 => crate::intel::gpu_font::GpuFontRgba::new(146, 102, 255, u8::MAX),
        4 => crate::intel::gpu_font::GpuFontRgba::new(78, 255, 142, u8::MAX),
        _ => crate::intel::gpu_font::GpuFontRgba::new(255, 245, 116, u8::MAX),
    }
}

fn cpp_font_rush_sprite_descriptor(
    source_width: u32,
    source_height: u32,
    center: [f32; 2],
    scale: f32,
    color_rgba: u32,
) -> crate::intel::gpgpu::GpgpuSpriteQuadWorklistDesc {
    let source_width_f = source_width as f32;
    let source_height_f = source_height as f32;
    let half_width = source_width_f * scale * 0.5;
    let half_height = source_height_f * scale * 0.5;
    let left = center[0] - half_width;
    let top = center[1] - half_height;
    let right = center[0] + half_width;
    let bottom = center[1] + half_height;
    let u0 = -0.5 / source_width_f;
    let v0 = -0.5 / source_height_f;
    let u1 = (source_width_f - 0.5) / source_width_f;
    let v1 = (source_height_f - 0.5) / source_height_f;
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
        color_rgba,
        flags: crate::intel::gpgpu::SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER
            | crate::intel::gpgpu::SPRITE_QUAD_WORKLIST_FLAG_PREMUL_SRC,
    }
}

fn cpp_font_rush_showcase_descriptors_are_contained(
    descriptors: &[crate::intel::gpgpu::GpgpuSpriteQuadWorklistDesc],
    destination_width: u32,
    destination_height: u32,
) -> bool {
    let max_x = destination_width as f32;
    let max_y = destination_height as f32;
    !descriptors.is_empty()
        && descriptors.iter().all(|descriptor| {
            [
                descriptor.c0_x,
                descriptor.c1_x,
                descriptor.c2_x,
                descriptor.c3_x,
            ]
            .into_iter()
            .all(|x| x.is_finite() && x >= 0.0 && x <= max_x)
                && [
                    descriptor.c0_y,
                    descriptor.c1_y,
                    descriptor.c2_y,
                    descriptor.c3_y,
                ]
                .into_iter()
                .all(|y| y.is_finite() && y >= 0.0 && y <= max_y)
        })
}

fn cpp_font_rush_showcase_sprite_layout(
    stage: CppFontRushLayerStage,
    sequence: u64,
    source_width: u32,
    source_height: u32,
    destination_width: u32,
    destination_height: u32,
) -> Result<CppFontRushShowcaseSpriteLayout, &'static str> {
    if source_width == 0 || source_height == 0 || destination_width == 0 || destination_height == 0
    {
        return Err("font-rush-showcase-sprite-empty-extent");
    }
    let source_width_f = source_width as f32;
    let source_height_f = source_height as f32;
    let destination_width_f = destination_width as f32;
    let destination_height_f = destination_height as f32;
    let (scale, centers, foreground, glyphs, columns) = match stage {
        CppFontRushLayerStage::TitleHold => {
            let scale = CPP_FONT_RUSH_TITLE_WORD_PRESENTATION_SCALE
                .min(destination_width_f * 0.97 / source_width_f)
                .min(destination_height_f * 0.88 / source_height_f);
            (
                scale,
                alloc::vec![[destination_width_f * 0.5, destination_height_f * 0.5]],
                crate::intel::gpu_font::GpuFontRgba::new(91, 232, 255, u8::MAX),
                CPP_FONT_RUSH_TITLE_WORD.len(),
                CPP_FONT_RUSH_TITLE_WORD.len() as u8,
            )
        }
        CppFontRushLayerStage::SectionPulse | CppFontRushLayerStage::StormPrime { .. } => {
            // The proven 102.4px Lucida source is 176x370 physical pixels at
            // 2560x1440.  A literal 3x presentation leaves visible gaps and
            // remains inside scanout, so do not silently shrink this request.
            let scale = CPP_FONT_RUSH_SECTION_PRESENTATION_SCALE;
            let foreground = if matches!(stage, CppFontRushLayerStage::StormPrime { .. }) {
                crate::intel::gpu_font::GpuFontRgba::new(255, 67, 173, u8::MAX)
            } else {
                cpp_font_rush_section_foreground(sequence)
            };
            (
                scale,
                [0.25f32, 0.5, 0.75]
                    .into_iter()
                    .map(|fraction| [destination_width_f * fraction, destination_height_f * 0.52])
                    .collect::<Vec<_>>(),
                foreground,
                3,
                3,
            )
        }
        _ => return Err("font-rush-showcase-sprite-stage"),
    };
    if !scale.is_finite() || scale <= 0.0 {
        return Err("font-rush-showcase-sprite-scale");
    }
    let color_rgba = u32::from_le_bytes([foreground.r, foreground.g, foreground.b, foreground.a]);
    let descriptors = centers
        .into_iter()
        .map(|center| {
            cpp_font_rush_sprite_descriptor(source_width, source_height, center, scale, color_rgba)
        })
        .collect::<Vec<_>>();
    if !cpp_font_rush_showcase_descriptors_are_contained(
        descriptors.as_slice(),
        destination_width,
        destination_height,
    ) {
        // Exact 3x is intentional for the section signs. Refuse an output or
        // changed-font geometry that cannot honor it instead of clipping or
        // silently shrinking the requested presentation.
        return Err("font-rush-showcase-sprite-out-of-bounds");
    }
    Ok(CppFontRushShowcaseSpriteLayout {
        descriptors,
        scale,
        glyphs,
        columns,
        color_rgba,
    })
}

fn queue_cpp_font_rush_showcase_sprite(
    preview: &mut ActivePreview,
    sequence: u64,
    scheduled_at: Instant,
) -> Result<(), &'static str> {
    use crate::r::font_kernel_service::FontKernelError;

    let (stage, source) = {
        let state = preview
            .font_rush
            .as_ref()
            .ok_or("font-rush-plane-state-missing")?;
        let source = match state.stage {
            CppFontRushLayerStage::TitleHold => state.showcase_sources.title.as_ref(),
            CppFontRushLayerStage::SectionPulse | CppFontRushLayerStage::StormPrime { .. } => {
                state.showcase_sources.section.as_ref()
            }
            _ => return Err("font-rush-showcase-sprite-stage"),
        }
        .ok_or("font-rush-showcase-source-missing")?;
        (state.stage, Arc::clone(source))
    };
    let source_surface = source.surface();
    let layout = cpp_font_rush_showcase_sprite_layout(
        stage,
        sequence,
        source_surface.width,
        source_surface.height,
        preview.width,
        preview.height,
    )?;
    let lease = match acquire_frame_buffer(preview.frame) {
        Ok(lease) => lease,
        Err(FramePoolError::Busy) => {
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
            preview.metrics.dropped_frame_busy =
                preview.metrics.dropped_frame_busy.saturating_add(1);
            return Ok(());
        }
        Err(_) => return Err("font-rush-showcase-frame-acquire"),
    };
    let destination = match gpgpu_rgba_surface(lease) {
        Ok(destination) => destination,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            return Err("font-rush-showcase-surface");
        }
    };
    let submit_started_at = Instant::now();
    let completion = match crate::r::font_kernel_service::submit_font_rush_showcase_sprite_frame(
        source,
        layout.descriptors,
        layout.glyphs,
        destination,
        u32::from_le_bytes(PremultipliedRgba8::TRANSPARENT.to_native_bytes()),
    ) {
        Ok(completion) => completion,
        Err(FontKernelError::QueueFull) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
            preview.metrics.dropped_queue_full =
                preview.metrics.dropped_queue_full.saturating_add(1);
            return Ok(());
        }
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            return Err("font-rush-showcase-submit");
        }
    };
    let accepted_at = Instant::now();
    let ticket = completion.ticket();
    let fifo_queued_ahead = completion.queued_ahead();
    preview.metrics.submitted = preview.metrics.submitted.saturating_add(1);
    preview
        .font_rush
        .as_mut()
        .ok_or("font-rush-plane-state-missing")?
        .pending = Some(CppFontRushPendingFrame {
        lease,
        completion,
        ticket,
        sequence,
        scheduled_at,
        submit_started_at,
        accepted_at,
        requested_glyphs: layout.glyphs,
        columns: layout.columns,
        rows: 1,
        stage,
        font: crate::intel::gpu_font::GpuFontFace::Default,
        glyph_fingerprint: match stage {
            CppFontRushLayerStage::TitleHold => 0x5472_7565_4F53,
            _ => 0x00A7_00A7_00A7 ^ sequence,
        },
        glyph_ids_sample: match stage {
            CppFontRushLayerStage::TitleHold => String::from("T/r/u/e/O/S"),
            _ => String::from("section-sign-x3"),
        },
        fifo_queued_ahead,
        plan_batch_id: 0,
        plan_enqueue_delay_ms: 0,
        plan_queue_wait_ms: 0,
        plan_build_ms: 0,
        plan_total_ms: 0,
        plan_candidate_attempts: 0,
        plan_rejected_candidates: 0,
        plan_worker_slices: 0,
        plan_cooperative_yields: 0,
        plan_parallelism: 0,
        prepared_ops_bytes: 0,
        prepared_reserved_ops_bytes: 0,
        prepared_work: 0,
    });
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font-rush showcase sprite enqueued request={} sequence={} ticket={} frame={} buffer={} layout={} glyphs={} source={}x{} destination={}x{} scale_milli={} tint=0x{:08X} cadence_ms={} fifo_queued_ahead={} submit_call_us={} clear=transparent source_lifecycle=run-owned planning=0 skrifa=0 tessellation=0 coverage=0 rgba_cpu_readback=0 path=tight-font-rgba8->guc-font-rcs-sprite-worklist->ui4-rgba8\n",
        preview.request_serial,
        sequence,
        ticket.raw(),
        lease.frame.raw(),
        lease.buffer_index,
        stage.label(),
        layout.glyphs,
        source_surface.width,
        source_surface.height,
        preview.width,
        preview.height,
        (layout.scale * 1_000.0) as u32,
        layout.color_rgba,
        stage.cadence_ms(),
        fifo_queued_ahead,
        accepted_at.saturating_duration_since(submit_started_at).as_micros(),
    );
    Ok(())
}

fn queue_cpp_font_rush_blank(
    preview: &mut ActivePreview,
    sequence: u64,
    scheduled_at: Instant,
) -> Result<(), &'static str> {
    use crate::r::font_kernel_service::FontKernelError;

    let stage = preview
        .font_rush
        .as_ref()
        .ok_or("font-rush-plane-state-missing")?
        .stage;
    if stage != CppFontRushLayerStage::BlankPrime {
        return Err("font-rush-blank-stage");
    }
    let lease = match acquire_frame_buffer(preview.frame) {
        Ok(lease) => lease,
        Err(FramePoolError::Busy) => {
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
            preview.metrics.dropped_frame_busy =
                preview.metrics.dropped_frame_busy.saturating_add(1);
            return Ok(());
        }
        Err(_) => return Err("font-rush-blank-frame-acquire"),
    };
    let destination = match gpgpu_rgba_surface(lease) {
        Ok(destination) => destination,
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            return Err("font-rush-blank-surface");
        }
    };
    let submit_started_at = Instant::now();
    let completion = match crate::r::font_kernel_service::submit_font_rush_frame_clear(
        destination,
        u32::from_le_bytes(PremultipliedRgba8::TRANSPARENT.to_native_bytes()),
    ) {
        Ok(completion) => completion,
        Err(FontKernelError::QueueFull) => {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
            preview.metrics.dropped_queue_full =
                preview.metrics.dropped_queue_full.saturating_add(1);
            return Ok(());
        }
        Err(_) => {
            let _ = cancel_frame_buffer(lease);
            return Err("font-rush-blank-submit");
        }
    };
    let accepted_at = Instant::now();
    let ticket = completion.ticket();
    let fifo_queued_ahead = completion.queued_ahead();
    preview.metrics.submitted = preview.metrics.submitted.saturating_add(1);
    preview
        .font_rush
        .as_mut()
        .ok_or("font-rush-plane-state-missing")?
        .pending = Some(CppFontRushPendingFrame {
        lease,
        completion,
        ticket,
        sequence,
        scheduled_at,
        submit_started_at,
        accepted_at,
        requested_glyphs: 0,
        columns: 0,
        rows: 0,
        stage,
        font: crate::intel::gpu_font::GpuFontFace::Default,
        glyph_fingerprint: 0,
        glyph_ids_sample: String::from("blank"),
        fifo_queued_ahead,
        plan_batch_id: 0,
        plan_enqueue_delay_ms: 0,
        plan_queue_wait_ms: 0,
        plan_build_ms: 0,
        plan_total_ms: 0,
        plan_candidate_attempts: 0,
        plan_rejected_candidates: 0,
        plan_worker_slices: 0,
        plan_cooperative_yields: 0,
        plan_parallelism: 0,
        prepared_ops_bytes: 0,
        prepared_reserved_ops_bytes: 0,
        prepared_work: 0,
    });
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font-rush blank enqueued request={} sequence={} ticket={} frame={} buffer={} extent={}x{} fifo_queued_ahead={} submit_call_us={} clear=transparent once=1 planning=0 skrifa=0 tessellation=0 coverage=0 shading=0\n",
        preview.request_serial,
        sequence,
        ticket.raw(),
        lease.frame.raw(),
        lease.buffer_index,
        preview.width,
        preview.height,
        fifo_queued_ahead,
        accepted_at.saturating_duration_since(submit_started_at).as_micros(),
    );
    Ok(())
}

fn queue_cpp_font_rush_plan(
    preview: &mut ActivePreview,
    sequence: u64,
    scheduled_at: Instant,
    font: crate::intel::gpu_font::GpuFontFace,
) -> Result<(), &'static str> {
    use crate::r::font_plan_service::FontPlanError;

    let rank = cpp_font_rush_rank(preview)?;
    let plane_slot = cpp_font_rush_plane_slot(preview)?;
    let stage = preview
        .font_rush
        .as_ref()
        .ok_or("font-rush-plane-state-missing")?
        .stage;
    let batch_borrow = match crate::r::font_plan_service::borrow_plan_batch() {
        Ok(batch) => batch,
        Err(FontPlanError::PoolFull) => {
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
            crate::log_warn!(
                target: "ui4";
                "ui4 cpp-font-rush plan backpressure request={} rank={} slot={} sequence={} drop=plan-pool-full active_batches={} active_cells={} queued_cells={} cadence_ms={} action=drop-cadence-sample-without-frame-lease\n",
                preview.request_serial,
                rank,
                plane_slot,
                sequence,
                crate::r::font_plan_service::status().active_batches,
                crate::r::font_plan_service::status().active_cells,
                crate::r::font_plan_service::status().queued_cells,
                stage.cadence_ms(),
            );
            return Ok(());
        }
        Err(_) => {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("font-plan-service-offline");
        }
    };
    let (request, requested_glyphs, (columns, rows)) = {
        let state = preview
            .font_rush
            .as_mut()
            .ok_or("font-rush-plane-state-missing")?;
        let (request, requested_glyphs, grid) = cpp_font_rush_plan_request(
            preview.width,
            preview.height,
            rank,
            stage,
            sequence,
            font,
            &mut state.rng,
        )?;
        (request, requested_glyphs, grid)
    };
    let requested_parallelism = request.parallelism();
    let completion = match batch_borrow.submit(request) {
        Ok(completion) => completion,
        Err(FontPlanError::PoolFull) => {
            preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
            crate::log_warn!(
                target: "ui4";
                "ui4 cpp-font-rush plan backpressure request={} rank={} slot={} sequence={} drop=plan-cell-cap glyphs={} active_batches={} active_cells={} cadence_ms={} action=drop-cadence-sample-without-frame-lease\n",
                preview.request_serial,
                rank,
                plane_slot,
                sequence,
                requested_glyphs,
                crate::r::font_plan_service::status().active_batches,
                crate::r::font_plan_service::status().active_cells,
                stage.cadence_ms(),
            );
            return Ok(());
        }
        Err(error) => {
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            crate::log_warn!(
                target: "ui4";
                "ui4 cpp-font-rush plan rejected request={} rank={} slot={} sequence={} glyphs={} reason={:?} action=stop-before-frame-lease\n",
                preview.request_serial,
                rank,
                plane_slot,
                sequence,
                requested_glyphs,
                error,
            );
            return Err("font-rush-plan-submit-failed");
        }
    };
    let batch_id = completion.batch_id();
    let enqueued_at = Instant::now();
    let state = preview
        .font_rush
        .as_mut()
        .ok_or("font-rush-plane-state-missing")?;
    state.planning = Some(CppFontRushPendingPlan {
        completion,
        sequence,
        scheduled_at,
        enqueued_at,
        requested_glyphs,
        columns,
        rows,
        stage,
        font,
    });
    crate::log_info!(
        target: "ui4";
        "ui4 cpp-font-rush plan-enqueued request={} consumer={} rank={} slot={} sequence={} batch={} layout={} font={} font_id={} glyphs={} grid={}x{} scheduled_ms={} enqueue_delay_ms={} requested_parallelism={} frame_lease=none gpu_queue=none producer_in_flight=1 producer_pending_limit=1 service_model=signal-only-plan-pool-32 active_batch_cap={} active_cell_cap={}\n",
        preview.request_serial,
        rank,
        rank,
        plane_slot,
        sequence,
        batch_id,
        stage.label(),
        font.registry_name(),
        font.id(),
        requested_glyphs,
        columns,
        rows,
        scheduled_at.saturating_duration_since(preview.started).as_millis(),
        enqueued_at.saturating_duration_since(scheduled_at).as_millis(),
        requested_parallelism,
        crate::r::font_plan_service::FONT_PLAN_MAX_ACTIVE_BATCHES,
        crate::r::font_plan_service::FONT_PLAN_MAX_ACTIVE_CELLS,
    );
    Ok(())
}

fn take_cpp_font_rush_due_ticks(
    preview: &mut ActivePreview,
    now: Instant,
) -> Option<(u64, Instant)> {
    if now < preview.next_render {
        return None;
    }
    let cadence_ms = preview
        .font_rush
        .as_ref()
        .map_or(CPP_FONT_RUSH_CADENCE_MS, |state| state.stage.cadence_ms());
    let period_ticks = Duration::from_millis(cadence_ms).as_ticks().max(1);
    let next_tick = preview.next_render.as_ticks();
    let due_ticks = cpp_font_rush_due_tick_count(now.as_ticks(), next_tick, period_ticks);
    let latest_offset = period_ticks.saturating_mul(due_ticks.saturating_sub(1));
    let advance = period_ticks.saturating_mul(due_ticks);
    let scheduled_at = Instant::from_ticks(next_tick.saturating_add(latest_offset));
    preview.next_render = Instant::from_ticks(next_tick.saturating_add(advance));
    Some((due_ticks, scheduled_at))
}

const fn cpp_font_rush_due_tick_count(now_tick: u64, next_tick: u64, period_ticks: u64) -> u64 {
    if now_tick < next_tick || period_ticks == 0 {
        return 0;
    }
    now_tick
        .saturating_sub(next_tick)
        .saturating_div(period_ticks)
        .saturating_add(1)
}

fn cpp_font_rush_rank(preview: &ActivePreview) -> Result<u8, &'static str> {
    preview
        .font_rush
        .as_ref()
        .map(|state| state.rank)
        .ok_or("font-rush-plane-state-missing")
}

fn cpp_font_rush_plane_slot(preview: &ActivePreview) -> Result<u8, &'static str> {
    preview
        .font_rush
        .as_ref()
        .map(|state| state.plane_slot)
        .ok_or("font-rush-plane-state-missing")
}

async fn drain_cpp_font_rush_pending(previews: &mut [ActivePreview]) {
    use crate::r::font_kernel_service::FontKernelError;

    for preview in previews {
        let rush2_pending = preview
            .font_rush2
            .as_mut()
            .and_then(|state| state.pending.take());
        if let Some(pending) = rush2_pending {
            let buffer_index = usize::from(pending.lease.buffer_index);
            let ticket = pending.pending.ticket();
            match pending.pending.wait().await {
                Ok(produced) => {
                    preview.metrics.completed = preview.metrics.completed.saturating_add(1);
                    let _ = cancel_frame_buffer(pending.lease);
                    if let Some(state) = preview.font_rush2.as_mut() {
                        if state.published[buffer_index].replace(produced).is_some() {
                            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                        }
                    }
                    crate::log_info!(target: "ui4";
                        "ui4 cpp-font-rush2 producer drained request={} ticket={} frame={} buffer={} action=cancel-unpublished-write+defer-row-ack-until-frame-destroy\n",
                        preview.request_serial,
                        ticket.raw(),
                        pending.lease.frame.raw(),
                        pending.lease.buffer_index,
                    );
                }
                Err(FontKernelError::SubmittedIncomplete(reason)) => {
                    preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                    crate::log_error!(target: "ui4";
                        "ui4 cpp-font-rush2 producer drain quarantined request={} ticket={} frame={} buffer={} reason={} action=retain-frame-write-lease\n",
                        preview.request_serial,
                        ticket.raw(),
                        pending.lease.frame.raw(),
                        pending.lease.buffer_index,
                        reason,
                    );
                }
                Err(error) => {
                    let _ = cancel_frame_buffer(pending.lease);
                    preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                    crate::log_warn!(target: "ui4";
                        "ui4 cpp-font-rush2 producer drain failed request={} ticket={} error={:?}\n",
                        preview.request_serial,
                        ticket.raw(),
                        error,
                    );
                }
            }
        }
        let (planning, ready_plan, pending, title_source_pending, section_source_pending) = preview
            .font_rush
            .as_mut()
            .map(|state| {
                (
                    state.planning.take(),
                    state.ready_plan.take(),
                    state.pending.take(),
                    state.showcase_sources.title_pending.take(),
                    state.showcase_sources.section_pending.take(),
                )
            })
            .unwrap_or((None, None, None, None, None));
        if let Some(planning) = planning {
            crate::log_info!(
                target: "ui4";
                "ui4 cpp-font-rush plan detached request={} rank={} sequence={} layout={} action=drop-completion-handle+cooperative-stale-result-discard\n",
                preview.request_serial,
                cpp_font_rush_rank(preview).unwrap_or(u8::MAX),
                planning.sequence,
                planning.stage.label(),
            );
            drop(planning);
        }
        if let Some(ready) = ready_plan {
            crate::log_info!(
                target: "ui4";
                "ui4 cpp-font-rush plan dropped request={} rank={} sequence={} layout={} glyphs={} prepared_ops_bytes={} action=release-unsubmitted-plan\n",
                preview.request_serial,
                cpp_font_rush_rank(preview).unwrap_or(u8::MAX),
                ready.sequence,
                ready.stage.label(),
                ready.plan.glyph_count(),
                ready.plan.ops_bytes(),
            );
            drop(ready);
        }
        for (source_label, source_pending) in [
            ("TrueOS", title_source_pending),
            ("section-sign", section_source_pending),
        ] {
            let Some(source_pending) = source_pending else {
                continue;
            };
            let ticket = source_pending.ticket();
            crate::log_info!(
                target: "ui4";
                "ui4 cpp-font-rush showcase source drain request={} source={} ticket={} action=await-private-rgba8-retirement-before-teardown\n",
                preview.request_serial,
                source_label,
                ticket.raw(),
            );
            match source_pending.wait().await {
                Ok(source) => {
                    let surface = source.surface();
                    crate::log_info!(
                        target: "ui4";
                        "ui4 cpp-font-rush showcase source drained request={} source={} ticket={} extent={}x{} result=complete action=release-unused-private-rgba8\n",
                        preview.request_serial,
                        source_label,
                        ticket.raw(),
                        surface.width,
                        surface.height,
                    );
                }
                Err(FontKernelError::SubmittedIncomplete(reason)) => {
                    preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                    crate::log_error!(
                        target: "ui4";
                        "ui4 cpp-font-rush showcase source drain quarantined request={} source={} ticket={} reason={} action=service-retains-ambiguous-storage\n",
                        preview.request_serial,
                        source_label,
                        ticket.raw(),
                        reason,
                    );
                }
                Err(error) => {
                    preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                    crate::log_warn!(
                        target: "ui4";
                        "ui4 cpp-font-rush showcase source drained request={} source={} ticket={} result={:?} action=no-live-source-storage\n",
                        preview.request_serial,
                        source_label,
                        ticket.raw(),
                        error,
                    );
                }
            }
        }
        let Some(pending) = pending else {
            continue;
        };
        crate::log_info!(
            target: "ui4";
            "ui4 cpp-font-rush consumer drain request={} rank={} sequence={} ticket={} frame={} buffer={} action=await-exact-destination-retirement-before-teardown\n",
            preview.request_serial,
            cpp_font_rush_rank(preview).unwrap_or(u8::MAX),
            pending.sequence,
            pending.ticket.raw(),
            pending.lease.frame.raw(),
            pending.lease.buffer_index,
        );
        match pending.completion.wait().await {
            Ok(stamped) => {
                preview.metrics.completed = preview.metrics.completed.saturating_add(1);
                let _ = cancel_frame_buffer(pending.lease);
                crate::log_info!(
                    target: "ui4";
                    "ui4 cpp-font-rush consumer drained request={} rank={} sequence={} ticket={} release={} result=complete action=cancel-unpublished-write-lease\n",
                    preview.request_serial,
                    cpp_font_rush_rank(preview).unwrap_or(u8::MAX),
                    pending.sequence,
                    pending.ticket.raw(),
                    stamped.release().sequence(),
                );
            }
            Err(FontKernelError::SubmittedIncomplete(reason)) => {
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                crate::log_error!(
                    target: "ui4";
                    "ui4 cpp-font-rush consumer drain quarantined request={} rank={} sequence={} ticket={} reason={} action=retain-frame-write-lease\n",
                    preview.request_serial,
                    cpp_font_rush_rank(preview).unwrap_or(u8::MAX),
                    pending.sequence,
                    pending.ticket.raw(),
                    reason,
                );
            }
            Err(error) => {
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                let _ = cancel_frame_buffer(pending.lease);
                crate::log_warn!(
                    target: "ui4";
                    "ui4 cpp-font-rush consumer drained request={} rank={} sequence={} ticket={} result={:?} action=cancel-retired-write-lease\n",
                    preview.request_serial,
                    cpp_font_rush_rank(preview).unwrap_or(u8::MAX),
                    pending.sequence,
                    pending.ticket.raw(),
                    error,
                );
            }
        }
    }
}

const fn cpp_font_rush_glyph_count(rank: u8, expanded_grid: bool) -> usize {
    let base = CPP_FONT_RUSH_GLYPHS[if rank < CPP_FONT_RUSH_MAX_PLANES as u8 {
        rank as usize
    } else {
        CPP_FONT_RUSH_MAX_PLANES - 1
    }];
    if expanded_grid { base * 4 } else { base }
}

const fn cpp_font_rush_grid(rank: u8, expanded_grid: bool) -> (u8, u8) {
    let base = match rank {
        0 => (1, 1),
        1 => (2, 1),
        2 => (2, 2),
        _ => (4, 4),
    };
    if expanded_grid {
        (base.0 * 2, base.1 * 2)
    } else {
        base
    }
}

fn cpp_font_rush_raw_storm_mix(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

fn cpp_font_rush_raw_storm_offset(seed: u64, extent: u32) -> u32 {
    ((u128::from(seed) * u128::from(extent)) >> 64) as u32
}

fn cpp_font_rush_plan_request(
    width: u32,
    height: u32,
    rank: u8,
    stage: CppFontRushLayerStage,
    _sequence: u64,
    font: crate::intel::gpu_font::GpuFontFace,
    rng: &mut crate::tyche::SoftRng,
) -> Result<(crate::r::font_plan_service::FontPlanBatchRequest, usize, (u8, u8)), &'static str> {
    use crate::r::font_plan_service::FontPlanCellRequest;

    if let CppFontRushLayerStage::ProducerStorm { wave, .. } = stage {
        let viewport_width = width.div_ceil(CPP_FONT_RUSH_VIEWPORT_SCALE).max(1);
        let viewport_height = height.div_ceil(CPP_FONT_RUSH_VIEWPORT_SCALE).max(1);
        let glyph_work_limit = crate::intel::gpu_font::gpu_font_analytical_work_limit()
            / CPP_FONT_RUSH_RAW_STORM_GLYPHS as u64;
        let foreground = match wave % 4 {
            0 => crate::intel::gpu_font::GpuFontRgba::new(255, 205, 64, 210),
            1 => crate::intel::gpu_font::GpuFontRgba::new(50, 225, 255, 210),
            2 => crate::intel::gpu_font::GpuFontRgba::new(255, 78, 214, 210),
            _ => crate::intel::gpu_font::GpuFontRgba::new(116, 255, 125, 210),
        };
        let mut cells = Vec::with_capacity(CPP_FONT_RUSH_RAW_STORM_GLYPHS);
        for worker in 0..crate::r::font_plan_service::FONT_PLAN_WORKER_COUNT {
            let column = worker % usize::from(CPP_FONT_RUSH_STORM_COLUMNS);
            let row = worker / usize::from(CPP_FONT_RUSH_STORM_COLUMNS);
            let region_left = (u64::from(viewport_width) * column as u64
                / u64::from(CPP_FONT_RUSH_STORM_COLUMNS)) as u32;
            let region_right = (u64::from(viewport_width) * (column + 1) as u64
                / u64::from(CPP_FONT_RUSH_STORM_COLUMNS)) as u32;
            let region_top = (u64::from(viewport_height) * row as u64
                / u64::from(CPP_FONT_RUSH_STORM_ROWS)) as u32;
            let region_bottom = (u64::from(viewport_height) * (row + 1) as u64
                / u64::from(CPP_FONT_RUSH_STORM_ROWS)) as u32;
            let region_width = region_right.saturating_sub(region_left).max(1);
            let region_height = region_bottom.saturating_sub(region_top).max(1);
            let font_pixels = (region_width.min(region_height) as f32 * 0.30).clamp(4.0, 64.0);
            for anchor in 0..CPP_FONT_RUSH_STORM_GLYPHS_PER_PRODUCER {
                let seed = cpp_font_rush_raw_storm_mix(
                    wave.wrapping_mul(0x9E37_79B9_7F4A_7C15)
                        ^ ((worker as u64) << 17)
                        ^ (anchor as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93),
                );
                let base_x = cpp_font_rush_raw_storm_offset(seed, region_width);
                let base_y = cpp_font_rush_raw_storm_offset(seed.rotate_left(29), region_height);
                let anchor_x = if anchor == 0 {
                    base_x
                } else {
                    (base_x + region_width.div_ceil(2)) % region_width
                };
                let anchor_y = if anchor == 0 {
                    base_y
                } else {
                    (base_y + region_height.div_ceil(2)) % region_height
                };
                let scalar =
                    char::from_u32(0x21 + (seed % 94) as u32).ok_or("font-rush-raw-scalar")?;
                cells.push(
                    FontPlanCellRequest::fixed(
                        [
                            region_left.saturating_add(anchor_x) as f32,
                            region_top.saturating_add(anchor_y) as f32,
                        ],
                        font_pixels,
                        0.0,
                        glyph_work_limit.max(1),
                        scalar,
                    )
                    .with_worker_affinity(worker as u8),
                );
            }
        }
        return Ok((
            crate::r::font_plan_service::FontPlanBatchRequest::new(
                "ui4-cpp-font-rush-raw-producer-storm",
                font,
                foreground,
                viewport_width,
                viewport_height,
                width,
                height,
                cells,
                crate::r::font_plan_service::FONT_PLAN_WORKER_COUNT,
            ),
            CPP_FONT_RUSH_RAW_STORM_GLYPHS,
            (
                CPP_FONT_RUSH_STORM_COLUMNS
                    .saturating_mul(CPP_FONT_RUSH_STORM_GLYPHS_PER_PRODUCER as u8),
                CPP_FONT_RUSH_STORM_ROWS,
            ),
        ));
    }
    if stage.uses_showcase_sprite() {
        return Err("font-rush-showcase-is-resident-rgba8-sprite-only");
    }
    // Keep the UI side deliberately cheap: it describes independent cells
    // or exact scalars. Warmed outline copying, raster transforms, analytical
    // admission, and rolled character selection belong to the E-core pool.
    let viewport_width = width.div_ceil(CPP_FONT_RUSH_VIEWPORT_SCALE).max(1);
    let viewport_height = height.div_ceil(CPP_FONT_RUSH_VIEWPORT_SCALE).max(1);
    let viewport_width_f = viewport_width as f32;
    let viewport_height_f = viewport_height as f32;
    let work_limit = crate::intel::gpu_font::gpu_font_analytical_work_limit();

    let foreground = match stage {
        CppFontRushLayerStage::Base | CppFontRushLayerStage::Expanded => match rank {
            0 => crate::intel::gpu_font::GpuFontRgba::new(245, 248, 255, u8::MAX),
            1 => crate::intel::gpu_font::GpuFontRgba::new(50, 225, 255, 224),
            2 => crate::intel::gpu_font::GpuFontRgba::new(255, 78, 214, 216),
            _ => crate::intel::gpu_font::GpuFontRgba::new(255, 205, 64, 208),
        },
        CppFontRushLayerStage::TitleLetter(_) => {
            crate::intel::gpu_font::GpuFontRgba::new(238, 247, 255, u8::MAX)
        }
        CppFontRushLayerStage::TitleHold
        | CppFontRushLayerStage::SectionPulse
        | CppFontRushLayerStage::StormPrime { .. } => {
            return Err("font-rush-showcase-is-resident-rgba8-sprite-only");
        }
        CppFontRushLayerStage::BlankPrime
        | CppFontRushLayerStage::BlankHold
        | CppFontRushLayerStage::ProducerStorm { .. } => {
            return Err("font-rush-non-plan-stage-routing");
        }
        CppFontRushLayerStage::Dormant => return Err("font-rush-dormant-plan"),
    };

    let (cells, glyph_count, columns, rows, parallelism) = match stage {
        CppFontRushLayerStage::Base | CppFontRushLayerStage::Expanded => {
            let expanded = stage == CppFontRushLayerStage::Expanded;
            let (columns, rows) = cpp_font_rush_grid(rank, expanded);
            let glyph_count = cpp_font_rush_glyph_count(rank, expanded);
            let cell_width = viewport_width_f / f32::from(columns);
            let cell_height = viewport_height_f / f32::from(rows);
            let font_pixels = (cell_width * 0.70)
                .min(cell_height * 0.70)
                .clamp(4.0, 256.0);
            let glyph_work_limit = work_limit / glyph_count.max(1) as u64;
            let mut cells = Vec::with_capacity(glyph_count);
            for index in 0..glyph_count {
                let column = (index % usize::from(columns)) as f32;
                let row = (index / usize::from(columns)) as f32;
                cells.push(FontPlanCellRequest::new(
                    [(column + 0.5) * cell_width, (row + 0.5) * cell_height],
                    font_pixels,
                    0.0,
                    glyph_work_limit,
                    rng.next_u64(),
                ));
            }
            (cells, glyph_count, columns, rows, 1)
        }
        CppFontRushLayerStage::TitleLetter(index) => {
            let scalar = *CPP_FONT_RUSH_TITLE_LETTERS
                .get(usize::from(index))
                .ok_or("font-rush-title-letter-index")?;
            let font_pixels = (viewport_width_f * 0.70)
                .min(viewport_height_f * 0.70)
                .clamp(4.0, CPP_FONT_RUSH_TITLE_LETTER_MAX_FONT_PIXELS);
            let cells = alloc::vec![FontPlanCellRequest::fixed(
                [viewport_width_f * 0.5, viewport_height_f * 0.5],
                font_pixels,
                0.0,
                work_limit,
                scalar,
            )];
            (cells, 1, 1, 1, 1)
        }
        CppFontRushLayerStage::TitleHold
        | CppFontRushLayerStage::SectionPulse
        | CppFontRushLayerStage::StormPrime { .. } => {
            return Err("font-rush-showcase-is-resident-rgba8-sprite-only");
        }
        CppFontRushLayerStage::BlankPrime
        | CppFontRushLayerStage::BlankHold
        | CppFontRushLayerStage::ProducerStorm { .. } => {
            return Err("font-rush-non-plan-stage-routing");
        }
        CppFontRushLayerStage::Dormant => return Err("font-rush-dormant-plan"),
    };

    Ok((
        crate::r::font_plan_service::FontPlanBatchRequest::new(
            "ui4-cpp-font-rush",
            font,
            foreground,
            viewport_width,
            viewport_height,
            width,
            height,
            cells,
            parallelism,
        ),
        glyph_count,
        (columns, rows),
    ))
}

async fn render_static30_frames(preview: &mut ActivePreview) -> Result<(), &'static str> {
    let mut surfaces = Vec::with_capacity(STATIC30_FRAME_COUNT);
    surfaces.push(StaticPreviewSurface {
        frame: preview.frame,
        window: preview.window,
        scheme: 0,
    });
    surfaces.extend(preview.extra_surfaces.iter().copied());

    let mut glyphs = 0usize;
    let mut kernel_submits = 0usize;
    let mut active_walkers = 0usize;
    let mut last_release = 0u64;
    for surface in &surfaces {
        preview.metrics.attempted = preview.metrics.attempted.saturating_add(1);
        let lease = match acquire_frame_buffer(surface.frame) {
            Ok(lease) => lease,
            Err(FramePoolError::Busy) => {
                preview.metrics.dropped_busy = preview.metrics.dropped_busy.saturating_add(1);
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("static30-frame-busy");
            }
            Err(_) => {
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("static30-frame-acquire-failed");
            }
        };
        let destination = match gpgpu_rgba_surface(lease) {
            Ok(destination) => destination,
            Err(_) => {
                let _ = cancel_frame_buffer(lease);
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("static30-font-surface-unavailable");
            }
        };
        let request =
            static30_font_stamp_request(destination.width, destination.height, surface.scheme);
        let pending = match crate::r::font_kernel_service::submit_frame_stamp(request, destination)
        {
            Ok(pending) => pending,
            Err(_) => {
                let _ = cancel_frame_buffer(lease);
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("static30-font-submit-failed");
            }
        };
        let stamped = match pending.wait().await {
            Ok(stamped) => stamped,
            Err(crate::r::font_kernel_service::FontKernelError::SubmittedIncomplete(_)) => {
                // The accepted producer may still target this exact surface.
                // Preserve the write lease so neither UI4 nor a future
                // producer can recycle it underneath a late GPU write.
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("static30-font-submit-incomplete");
            }
            Err(_) => {
                let _ = cancel_frame_buffer(lease);
                preview.metrics.failed = preview.metrics.failed.saturating_add(1);
                return Err("static30-font-stamp-failed");
            }
        };
        preview.metrics.submitted = preview.metrics.submitted.saturating_add(1);
        preview.metrics.completed = preview.metrics.completed.saturating_add(1);
        glyphs = glyphs.saturating_add(stamped.glyphs());
        kernel_submits = kernel_submits.saturating_add(stamped.submits());
        active_walkers = active_walkers.saturating_add(stamped.active_walkers());
        last_release = stamped.release().sequence();
        if publish_gpu_font_frame_buffer(lease, stamped.release()).is_err() {
            let _ = cancel_frame_buffer(lease);
            preview.metrics.failed = preview.metrics.failed.saturating_add(1);
            return Err("static30-font-frame-publish-failed");
        }
    }

    let publications = surfaces
        .iter()
        .map(|surface| (surface.window, DamageRect::FULL))
        .collect::<Vec<_>>();
    if publish_window_frames(PREVIEW_OWNER, &publications).is_err() {
        preview.metrics.failed = preview.metrics.failed.saturating_add(1);
        return Err("static30-window-publish-failed");
    }
    preview.metrics.published = preview
        .metrics
        .published
        .saturating_add(publications.len() as u64);

    preview.static_needs_publish = false;
    crate::log_info!(
        target: "ui4";
        "ui4 gpgpu-preview static30 published request={} frames={} windows={} slots=1+2+3 per_slot=10 glyphs={} font_jobs={} kernel_submits={} active_walkers={} release_sequence={} producer=font-kernel-service path=skrifa->gpu-vm-r8->cpp-font-instance->ui4-frame cadence=immutable/single publish_passes=1 cpu_readback=0 cpu_frame_copy=0\n",
        preview.request_serial,
        preview.metrics.published,
        preview_surface_count(preview),
        glyphs,
        preview.metrics.submitted,
        kernel_submits,
        active_walkers,
        last_release,
    );
    Ok(())
}

fn static30_font_stamp_request(
    width: u32,
    height: u32,
    scheme: u8,
) -> crate::r::font_kernel_service::FontStampRequest {
    use crate::r::font_kernel_service::{
        FontStampFit, FontStampLayer, FontStampRequest, RetainSceneRequest,
        RetainedFontPositioning, RetainedFontRun,
    };

    let horizontal_padding = 10.0f32;
    let vertical_padding = 8.0f32;
    let width_fit = (width.saturating_sub(20) as f32 / 9.0).max(8.0);
    let height_fit = (height.saturating_sub(16) as f32 / 5.4).max(8.0);
    let font_pixels = width_fit.min(height_fit).clamp(8.0, 26.0);
    let baseline = vertical_padding + font_pixels;
    let line_advance = font_pixels * 1.22;
    let scene = |runs| RetainSceneRequest {
        runs,
        font: crate::intel::gpu_font::GpuFontFace::Default,
        viewport_width: width,
        viewport_height: height,
        raster_width: width,
        raster_height: height,
        positioning: RetainedFontPositioning::SceneOrigin,
    };
    let run = |text: &'static str, row: usize| RetainedFontRun {
        text: String::from(text),
        position: [horizontal_padding, baseline + row as f32 * line_advance],
        font_pixels,
        slant: if scheme.is_multiple_of(3) { 0.08 } else { 0.0 },
    };
    let seed = u16::from(scheme);
    FontStampRequest {
        layers: alloc::vec![
            FontStampLayer {
                scene: scene(alloc::vec![run("Lorem ipsum", 0)]),
                foreground: crate::intel::gpu_font::GpuFontRgba::new(
                    (112 + seed * 37 % 143) as u8,
                    (144 + seed * 53 % 111) as u8,
                    (176 + seed * 29 % 79) as u8,
                    u8::MAX,
                ),
            },
            FontStampLayer {
                scene: scene(alloc::vec![
                    run("dolor sit amet,", 1),
                    run("consectetur", 2),
                    run("adipiscing elit.", 3),
                ]),
                foreground: crate::intel::gpu_font::GpuFontRgba::new(238, 244, 255, u8::MAX,),
            },
        ],
        fit: FontStampFit::Canvas,
    }
}

/// Paint one unmistakable CPU-authored frame. This path deliberately does not
/// request a GPGPU surface, upload a kernel, submit through GuC, or poll a GPU
/// completion marker. The cache release is the only producer-side operation
/// between the CPU writes and ordinary UI4 publication.
fn fill_static_preview_frame(lease: FrameWriteLease, scheme: u8) -> Result<(), &'static str> {
    let view = writable_rgba_view(lease).map_err(|_| "static-rgba-view-unavailable")?;
    let row_bytes = (view.width as usize)
        .checked_mul(4)
        .ok_or("static-rgba-layout-invalid")?;
    let pitch = view.pitch as usize;
    let required = pitch
        .checked_mul(view.height as usize)
        .ok_or("static-rgba-layout-invalid")?;
    if pitch < row_bytes || required > view.byte_len {
        return Err("static-rgba-layout-invalid");
    }

    let seed = u16::from(scheme);
    let navy = PremultipliedRgba8::from_straight_rgba(
        (8 + seed * 17 % 40) as u8,
        (16 + seed * 29 % 48) as u8,
        (40 + seed * 13 % 64) as u8,
        u8::MAX,
    )
    .to_native_bytes();
    let blue = PremultipliedRgba8::from_straight_rgba(
        (32 + seed * 53 % 192) as u8,
        (32 + seed * 97 % 192) as u8,
        (32 + seed * 151 % 192) as u8,
        u8::MAX,
    )
    .to_native_bytes();
    let cyan = PremultipliedRgba8::from_straight_rgba(
        (48 + seed * 71 % 176) as u8,
        (48 + seed * 113 % 176) as u8,
        (48 + seed * 37 % 176) as u8,
        u8::MAX,
    )
    .to_native_bytes();
    let magenta = PremultipliedRgba8::from_straight_rgba(
        (48 + seed * 131 % 176) as u8,
        (48 + seed * 43 % 176) as u8,
        (48 + seed * 83 % 176) as u8,
        u8::MAX,
    )
    .to_native_bytes();
    let white = PremultipliedRgba8::from_straight_rgba(240, 248, 255, u8::MAX).to_native_bytes();
    let width = view.width as usize;
    let height = view.height as usize;
    let half_width = width / 2;
    let half_height = height / 2;

    // SAFETY: the write lease exclusively owns this allocation, and the
    // checked pitch/height product is contained by `byte_len`.
    let bytes = unsafe { core::slice::from_raw_parts_mut(view.virt, view.byte_len) };
    for y in 0..height {
        let row = &mut bytes[y * pitch..y * pitch + row_bytes];
        for x in 0..width {
            let border = x < 8 || y < 8 || x + 8 >= width || y + 8 >= height;
            let grid = x % 64 < 2 || y % 64 < 2;
            let pixel = if border {
                white
            } else if grid {
                cyan
            } else if x < half_width && y < half_height {
                blue
            } else if x >= half_width && y >= half_height {
                magenta
            } else {
                navy
            };
            row[x * 4..x * 4 + 4].copy_from_slice(&pixel);
        }
    }
    crate::intel::dma_flush(view.virt, view.byte_len);
    Ok(())
}

#[derive(Copy, Clone)]
struct PreviewDispatchResult {
    ok: bool,
    submitted: bool,
    iterations: u32,
    marker: u32,
    submit_ms: u64,
    release: Option<crate::intel::gpgpu::GpgpuRgba8ReleaseFence>,
    error: &'static str,
}

fn dispatch_preview_kernel(
    preview: &mut ActivePreview,
    surface: crate::intel::gpgpu::GpgpuRgba8Surface,
) -> PreviewDispatchResult {
    match preview.config.preset {
        GpgpuPreviewPreset::All => PreviewDispatchResult {
            ok: false,
            submitted: false,
            iterations: 0,
            marker: 0,
            submit_ms: 0,
            release: None,
            error: "compute-trio-entered-single-dispatch",
        },
        GpgpuPreviewPreset::Static
        | GpgpuPreviewPreset::Static30
        | GpgpuPreviewPreset::CppFont
        | GpgpuPreviewPreset::CppFontRush
        | GpgpuPreviewPreset::CppFontRush2 => PreviewDispatchResult {
            ok: false,
            submitted: false,
            iterations: 0,
            marker: 0,
            submit_ms: 0,
            release: None,
            error: "static-preset-entered-gpu-dispatch",
        },
        GpgpuPreviewPreset::Mandelbrot => {
            let iterations = 32 + ((preview.metrics.attempted - 1) % 97) as u32;
            match crate::intel::gpgpu::mandel64_worklist_surface_full(surface, iterations) {
                Some(result) => PreviewDispatchResult {
                    ok: result.ok,
                    submitted: result.submitted,
                    iterations,
                    marker: result.marker,
                    submit_ms: result.submit_ms,
                    release: result.release,
                    error: "mandelbrot-dispatch-failed",
                },
                None => PreviewDispatchResult {
                    ok: false,
                    submitted: false,
                    iterations,
                    marker: 0,
                    submit_ms: 0,
                    release: None,
                    error: "mandelbrot-dispatch-unavailable",
                },
            }
        }
        GpgpuPreviewPreset::Chart => {
            let seconds = preview.metrics.elapsed_ms as f32 / 1_000.0;
            let flags = crate::intel::gpgpu::CHART_SINE_FLAG_GRID
                | crate::intel::gpgpu::CHART_SINE_FLAG_AXES
                | crate::intel::gpgpu::CHART_SINE_FLAG_GLOW
                | crate::intel::gpgpu::CHART_SINE_FLAG_BORDER;
            let result = crate::intel::gpgpu::chart_sine_rgba8_surface_full(
                surface,
                seconds * core::f32::consts::FRAC_PI_2,
                flags,
            );
            PreviewDispatchResult {
                ok: result.ok,
                submitted: result.submitted,
                iterations: 0,
                marker: result.marker,
                submit_ms: result.submit_ms,
                release: result.release,
                error: "chart-dispatch-failed",
            }
        }
        GpgpuPreviewPreset::Plasma => {
            let seconds = preview.metrics.elapsed_ms as f32 / 1_000.0;
            let flags = crate::intel::gpgpu::PIXEL_PLASMA_FLAG_VIGNETTE
                | crate::intel::gpgpu::PIXEL_PLASMA_FLAG_RINGS
                | crate::intel::gpgpu::PIXEL_PLASMA_FLAG_SCANLINE
                | crate::intel::gpgpu::PIXEL_PLASMA_FLAG_FIELD_PALETTE;
            let result =
                crate::intel::gpgpu::pixel_plasma_rgba8_surface_full(surface, seconds, flags);
            PreviewDispatchResult {
                ok: result.ok,
                submitted: result.submitted,
                iterations: 0,
                marker: result.marker,
                submit_ms: result.submit_ms,
                release: result.release,
                error: "plasma-dispatch-failed",
            }
        }
        GpgpuPreviewPreset::Lab256 => {
            let result = crate::intel::gpgpu::lab256_preview_frame(surface);
            PreviewDispatchResult {
                ok: result.ok,
                submitted: result.submitted,
                iterations: 3,
                marker: result.marker,
                submit_ms: result.submit_ms,
                release: result.release,
                error: "lab256-dispatch-failed",
            }
        }
        GpgpuPreviewPreset::CppGallery
        | GpgpuPreviewPreset::CppCloudHighWisps
        | GpgpuPreviewPreset::CppAurora
        | GpgpuPreviewPreset::CppJulia
        | GpgpuPreviewPreset::CppSdf
        | GpgpuPreviewPreset::CppVoronoi
        | GpgpuPreviewPreset::CppRetroSun => {
            let seconds = preview.metrics.elapsed_ms as f32 / 1_000.0;
            let mode = match preview.config.preset {
                GpgpuPreviewPreset::CppGallery => crate::intel::gpgpu::CPP_DEMO_MODE_GALLERY,
                GpgpuPreviewPreset::CppCloudHighWisps => {
                    crate::intel::gpgpu::CPP_DEMO_MODE_CLOUD_HIGH_WISPS
                }
                GpgpuPreviewPreset::CppAurora => crate::intel::gpgpu::CPP_DEMO_MODE_AURORA,
                GpgpuPreviewPreset::CppJulia => crate::intel::gpgpu::CPP_DEMO_MODE_JULIA,
                GpgpuPreviewPreset::CppSdf => crate::intel::gpgpu::CPP_DEMO_MODE_SDF,
                GpgpuPreviewPreset::CppVoronoi => crate::intel::gpgpu::CPP_DEMO_MODE_VORONOI,
                GpgpuPreviewPreset::CppRetroSun => crate::intel::gpgpu::CPP_DEMO_MODE_RETRO_SUN,
                _ => crate::intel::gpgpu::CPP_DEMO_MODE_GALLERY,
            };
            let seed = (preview.request_serial as u32)
                .rotate_left(13)
                .wrapping_add(0xC0DE_C901);
            let result = crate::intel::gpgpu::cpp_demo_rgba8_surface_full(
                surface,
                seconds,
                mode,
                seed,
                &preview.cloud_brush.points[..preview.cloud_brush.count],
            );
            PreviewDispatchResult {
                ok: result.ok,
                submitted: result.submitted,
                iterations: mode,
                marker: result.marker,
                submit_ms: result.submit_ms,
                release: result.release,
                error: "cpp-demo-dispatch-failed",
            }
        }
        GpgpuPreviewPreset::CppAudio => {
            let seconds = preview.metrics.elapsed_ms as f32 / 1_000.0;
            let snapshot = crate::aud::audio_visualizer::snapshot();
            let result = crate::intel::gpgpu::cpp_audio_visualizer_rgba8_surface_full(
                surface,
                seconds,
                preview.metrics.attempted as u32,
                &snapshot,
            );
            PreviewDispatchResult {
                ok: result.ok,
                submitted: result.submitted,
                iterations: crate::aud::audio_visualizer::AUDIO_VISUALIZER_FFT_FRAMES as u32,
                marker: result.marker,
                submit_ms: result.submit_ms,
                release: result.release,
                error: "cpp-audio-visualizer-dispatch-failed",
            }
        }
        GpgpuPreviewPreset::CppParticle => {
            let seconds = preview.metrics.elapsed_ms as f32 / 1_000.0;
            let attempted = preview.metrics.attempted;
            let dt = (preview.config.cadence_ms.min(50) as f32 / 1_000.0).max(0.001);
            let seed = (preview.request_serial as u32)
                .rotate_left(11)
                .wrapping_add(0xC0FF_EE51);
            let mut params =
                crate::intel::gpgpu::ParticleCraftParamsV1::arc_forge(seconds, dt, seed);
            if attempted == 1 {
                params.flags |= crate::intel::gpgpu::PARTICLE_CRAFT_FLAG_RESET;
            }
            let craft = match preview.particle_craft.as_mut() {
                Some(craft) => craft,
                None => {
                    preview.particle_craft =
                        crate::intel::gpgpu::GpgpuOwnedParticleCraftState::allocate();
                    let Some(craft) = preview.particle_craft.as_mut() else {
                        return PreviewDispatchResult {
                            ok: false,
                            submitted: false,
                            iterations: 0,
                            marker: 0,
                            submit_ms: 0,
                            release: None,
                            error: "particle-craft-state-allocation-failed",
                        };
                    };
                    craft
                }
            };
            let result = crate::intel::gpgpu::particle_craft_rgba8_frame(craft, surface, params);
            PreviewDispatchResult {
                ok: result.ok,
                submitted: result.submitted,
                iterations: params.active_count,
                marker: result.marker,
                submit_ms: result.submit_ms,
                release: result.release,
                error: "particle-craft-dispatch-failed",
            }
        }
    }
}

const fn preview_release_label(preset: GpgpuPreviewPreset) -> &'static str {
    match preset {
        GpgpuPreviewPreset::All
        | GpgpuPreviewPreset::Mandelbrot
        | GpgpuPreviewPreset::Chart
        | GpgpuPreviewPreset::Plasma
        | GpgpuPreviewPreset::CppGallery
        | GpgpuPreviewPreset::CppCloudHighWisps
        | GpgpuPreviewPreset::CppAurora
        | GpgpuPreviewPreset::CppJulia
        | GpgpuPreviewPreset::CppSdf
        | GpgpuPreviewPreset::CppVoronoi
        | GpgpuPreviewPreset::CppRetroSun
        | GpgpuPreviewPreset::CppAudio
        | GpgpuPreviewPreset::CppParticle => "pipe-control+post-marker-exact-surface",
        GpgpuPreviewPreset::Lab256 => "three-pass+pipe-control+post-marker-exact-surface",
        GpgpuPreviewPreset::Static => "clflush-mfence-before-publish",
        GpgpuPreviewPreset::Static30 => "font-instance+pipe-control+post-marker-exact-surface",
        GpgpuPreviewPreset::CppFont
        | GpgpuPreviewPreset::CppFontRush
        | GpgpuPreviewPreset::CppFontRush2 => {
            "font-instance+pipe-control+post-marker-exact-surface"
        }
    }
}

const fn preview_producer_label(preset: GpgpuPreviewPreset) -> &'static str {
    match preset {
        GpgpuPreviewPreset::All => "guc-compute-trio",
        GpgpuPreviewPreset::Lab256 => "guc-lab256-three-pass",
        GpgpuPreviewPreset::Static => "cpu-static",
        GpgpuPreviewPreset::Static30 => "font-kernel-service-cpp",
        GpgpuPreviewPreset::Mandelbrot | GpgpuPreviewPreset::Chart | GpgpuPreviewPreset::Plasma => {
            "guc-compute-single"
        }
        GpgpuPreviewPreset::CppGallery
        | GpgpuPreviewPreset::CppCloudHighWisps
        | GpgpuPreviewPreset::CppAurora
        | GpgpuPreviewPreset::CppJulia
        | GpgpuPreviewPreset::CppSdf
        | GpgpuPreviewPreset::CppVoronoi
        | GpgpuPreviewPreset::CppRetroSun
        | GpgpuPreviewPreset::CppAudio => "guc-cpp-single",
        GpgpuPreviewPreset::CppParticle => "guc-cpp-stateful-three-pass",
        GpgpuPreviewPreset::CppFont
        | GpgpuPreviewPreset::CppFontRush
        | GpgpuPreviewPreset::CppFontRush2 => "font-kernel-service-cpp",
    }
}

const fn preview_plane(preset: GpgpuPreviewPreset) -> WindowPlane {
    match preset {
        GpgpuPreviewPreset::All | GpgpuPreviewPreset::Static | GpgpuPreviewPreset::Static30 => {
            WindowPlane::Universal(super::ALPHA_OVERLAY_PLANE_SLOT as u8)
        }
        GpgpuPreviewPreset::Mandelbrot
        | GpgpuPreviewPreset::Chart
        | GpgpuPreviewPreset::Plasma
        | GpgpuPreviewPreset::CppGallery
        | GpgpuPreviewPreset::CppCloudHighWisps
        | GpgpuPreviewPreset::CppAurora
        | GpgpuPreviewPreset::CppJulia
        | GpgpuPreviewPreset::CppSdf
        | GpgpuPreviewPreset::CppVoronoi
        | GpgpuPreviewPreset::CppRetroSun
        | GpgpuPreviewPreset::CppAudio
        | GpgpuPreviewPreset::CppParticle
        | GpgpuPreviewPreset::CppFont => WindowPlane::Universal(preview_plane_slot(preset) as u8),
        GpgpuPreviewPreset::CppFontRush | GpgpuPreviewPreset::CppFontRush2 => WindowPlane::Primary,
        GpgpuPreviewPreset::Lab256 => WindowPlane::Universal(super::ALPHA_OVERLAY_PLANE_SLOT as u8),
    }
}

const fn preview_consumer_label(preset: GpgpuPreviewPreset) -> &'static str {
    match preset {
        GpgpuPreviewPreset::All => "ui4-direct-slots1+2+3",
        GpgpuPreviewPreset::Mandelbrot => "ui4-direct-slot1",
        GpgpuPreviewPreset::Chart => "ui4-direct-slot2",
        GpgpuPreviewPreset::Plasma => "ui4-direct-slot3",
        GpgpuPreviewPreset::Lab256 => "ui4-alpha-slot1-256x256",
        GpgpuPreviewPreset::CppGallery
        | GpgpuPreviewPreset::CppCloudHighWisps
        | GpgpuPreviewPreset::CppAurora
        | GpgpuPreviewPreset::CppJulia
        | GpgpuPreviewPreset::CppSdf
        | GpgpuPreviewPreset::CppVoronoi
        | GpgpuPreviewPreset::CppRetroSun
        | GpgpuPreviewPreset::CppAudio
        | GpgpuPreviewPreset::CppParticle => "ui4-cpp-resizable-slot1",
        GpgpuPreviewPreset::CppFont => "ui4-font-scene-slot1",
        GpgpuPreviewPreset::CppFontRush => "ui4-direct-font-scene-slots0+1+2+3",
        GpgpuPreviewPreset::CppFontRush2 => "ui4-font-scene-slots0+1+2+3-32-row-stack",
        GpgpuPreviewPreset::Static => "ui4-overlay",
        GpgpuPreviewPreset::Static30 => "ui4-font-scene-slots1+2+3",
    }
}

fn preview_surface_count(preview: &ActivePreview) -> usize {
    1usize.saturating_add(preview.extra_surfaces.len())
}

const fn should_log_preview_checkpoint(sequence: u64) -> bool {
    sequence <= 8 || sequence.is_multiple_of(120)
}

fn preview_needs_render(preview: &ActivePreview) -> bool {
    if preview.policy.frame_limit != 0 && preview.metrics.published >= preview.policy.frame_limit {
        return false;
    }
    !matches!(
        preview.config.preset,
        GpgpuPreviewPreset::Static | GpgpuPreviewPreset::Static30 | GpgpuPreviewPreset::CppFont
    ) || preview.static_needs_publish
}

fn schedule_next_render(preview: &mut ActivePreview) {
    let period = if preview.policy.target_hz == 0 {
        Duration::from_millis(preview.config.cadence_ms)
    } else {
        let hz = preview.policy.target_hz;
        let mut ticks = trueos_time::TICK_HZ / hz;
        preview.cadence_phase += trueos_time::TICK_HZ % hz;
        if preview.cadence_phase >= hz {
            preview.cadence_phase -= hz;
            ticks += 1;
        }
        Duration::from_ticks(ticks.max(1))
    };
    let scheduled = preview.next_render + period;
    let now = Instant::now();
    if now > scheduled {
        preview.metrics.late = preview.metrics.late.saturating_add(1);
        preview.next_render = now + period;
    } else {
        preview.next_render = scheduled;
    }
}

fn next_poll_ms(preview: &ActivePreview) -> u64 {
    if !preview_needs_render(preview) {
        return COMMAND_POLL_MAX_MS;
    }
    if preview.font_rush.as_ref().is_some_and(|state| {
        state.planning.is_some()
            || state.ready_plan.is_some()
            || state.pending.is_some()
            || state.scanout_pending.is_some()
    }) {
        return 1;
    }
    if preview.font_rush.as_ref().is_some_and(|state| {
        state.showcase_sources.title_pending.is_some()
            || state.showcase_sources.section_pending.is_some()
    }) {
        return COMMAND_POLL_MAX_MS;
    }
    if preview.font_rush.as_ref().is_some_and(|state| {
        state.stage.uses_showcase_sprite()
            && !cpp_font_rush_showcase_source_ready(state, state.stage)
    }) {
        return COMMAND_POLL_MAX_MS;
    }
    let now = Instant::now();
    if preview.next_render <= now {
        return 1;
    }
    preview
        .next_render
        .saturating_duration_since(now)
        .as_millis()
        .clamp(1, COMMAND_POLL_MAX_MS)
}

fn drain_preview_input(active: &mut [ActivePreview], retired_frames: &mut Vec<FrameHandle>) {
    for event in take_owner_input_events(PREVIEW_OWNER) {
        match event {
            Ui4InputEvent::Resize(event) => {
                let Some(preview) = active
                    .iter_mut()
                    .find(|preview| event.window == preview.window)
                else {
                    continue;
                };
                if event.width == 0 || event.height == 0 {
                    continue;
                }
                try_resize_preview(
                    preview,
                    event.width,
                    event.height,
                    retired_frames,
                    "input-event",
                );
            }
            Ui4InputEvent::Pointer(event) => {
                let Some(preview) = active.iter_mut().find(|preview| {
                    event.window == preview.window
                        && preview.config.preset == GpgpuPreviewPreset::CppCloudHighWisps
                }) else {
                    continue;
                };
                if event.buttons_pressed & PRIMARY_BUTTON_MASK != 0 {
                    preview.cloud_brush.dragging = Some(event.source);
                    preview.cloud_brush.last = None;
                }
                if preview.cloud_brush.dragging == Some(event.source)
                    && event.buttons_down & PRIMARY_BUTTON_MASK != 0
                {
                    preview.cloud_brush.drag_to(
                        event.local_x,
                        event.local_y,
                        preview.width,
                        preview.height,
                    );
                }
                if preview.cloud_brush.dragging == Some(event.source)
                    && event.buttons_released & PRIMARY_BUTTON_MASK != 0
                {
                    preview.cloud_brush.dragging = None;
                    preview.cloud_brush.last = None;
                }
            }
            Ui4InputEvent::Keyboard(event) => {
                let Some(preview) = active.iter().find(|preview| {
                    event.window == preview.window
                        || preview
                            .extra_surfaces
                            .iter()
                            .any(|surface| event.window == surface.window)
                }) else {
                    continue;
                };
                if !preview.policy.interactive_cpp_gallery
                    || event.event.kind != crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
                {
                    continue;
                }
                match event.event.key_code {
                    crate::r::keyboard::KEYBOARD_KEY_ARROW_LEFT => {
                        queue_interactive_cpp_cycle(-1, event.source);
                    }
                    crate::r::keyboard::KEYBOARD_KEY_ARROW_RIGHT => {
                        queue_interactive_cpp_cycle(1, event.source);
                    }
                    crate::r::keyboard::KEYBOARD_KEY_ESCAPE => {
                        queue_interactive_cpp_stop();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn cycled_cpp_gallery_preset(preset: GpgpuPreviewPreset, direction: i8) -> GpgpuPreviewPreset {
    let index = CPP_GALLERY_PRESETS
        .iter()
        .position(|candidate| *candidate == preset)
        .unwrap_or(0);
    let next = if direction < 0 {
        index
            .checked_sub(1)
            .unwrap_or(CPP_GALLERY_PRESETS.len() - 1)
    } else {
        (index + 1) % CPP_GALLERY_PRESETS.len()
    };
    CPP_GALLERY_PRESETS[next]
}

fn queue_interactive_cpp_cycle(direction: i8, source: Ui4CursorSource) {
    let (serial, previous, next) = {
        let mut control = PREVIEW_CONTROL.lock();
        if !control.desired.running || !control.desired.policy.interactive_cpp_gallery {
            return;
        }
        let previous = control.desired.config.preset;
        let next = cycled_cpp_gallery_preset(previous, direction);
        let serial = next_serial(control.desired.serial);
        control.desired.serial = serial;
        control.desired.config.preset = next;
        control.desired.policy.refocus_source = Some(source);
        control.status.desired_running = true;
        control.status.phase = GpgpuPreviewPhase::Starting;
        control.status.request_serial = serial;
        control.status.config = control.desired.config;
        control.status.last_error = "none";
        (serial, previous, next)
    };
    crate::log_info!(target: "ui4";
        "ui4 cpp-gallery cycle request={} previous={} next={} direction={} input=keyboard-arrow refocus={}:{}:{}\n",
        serial,
        previous.label(),
        next.label(),
        if direction < 0 { "left" } else { "right" },
        source.controller_id,
        source.slot_id,
        source.ep_target,
    );
}

fn queue_interactive_cpp_stop() {
    let serial = {
        let mut control = PREVIEW_CONTROL.lock();
        if !control.desired.running || !control.desired.policy.interactive_cpp_gallery {
            return;
        }
        let serial = next_serial(control.desired.serial);
        control.desired.serial = serial;
        control.desired.running = false;
        control.desired.policy.refocus_source = None;
        control.status.desired_running = false;
        control.status.request_serial = serial;
        CPP_FONT_REQUEST.lock().take();
        serial
    };
    crate::log_info!(target: "ui4";
        "ui4 cpp-gallery stop request={} input=keyboard-escape\n",
        serial,
    );
}

fn restore_interactive_cpp_focus(active: &mut [ActivePreview]) {
    let Some(source) = active
        .first()
        .filter(|preview| preview.policy.interactive_cpp_gallery)
        .and_then(|preview| preview.policy.refocus_source)
    else {
        return;
    };
    let window = active[0].window;
    if reselect_window_for_cursor(source, PREVIEW_OWNER, window).is_ok() {
        for preview in active {
            preview.policy.refocus_source = None;
        }
    }
}

/// Resize notifications are intentionally only a wakeup hint. The broker's
/// geometry is authoritative, so a dropped event or a transient allocation
/// failure cannot leave a maximized C++ preview permanently backed by its
/// original 640x400 frame or repeatedly request an already-live scaled backing.
fn reconcile_preview_extents(active: &mut [ActivePreview], retired_frames: &mut Vec<FrameHandle>) {
    for preview in active {
        if !preview.config.preset.is_resizable_cpp() {
            continue;
        }
        let Ok(placement) = window_placement(PREVIEW_OWNER, preview.window) else {
            continue;
        };
        let (backing_width, backing_height) =
            preview_backing_extent(preview.config.preset, placement.width, placement.height);
        if placement.width == 0
            || placement.height == 0
            || (backing_width == preview.width && backing_height == preview.height)
        {
            continue;
        }
        try_resize_preview(
            preview,
            placement.width,
            placement.height,
            retired_frames,
            "broker-reconcile",
        );
    }
}

fn try_resize_preview(
    preview: &mut ActivePreview,
    width: u32,
    height: u32,
    retired_frames: &mut Vec<FrameHandle>,
    source: &'static str,
) {
    let (backing_width, backing_height) =
        preview_backing_extent(preview.config.preset, width, height);
    if backing_width == preview.width && backing_height == preview.height {
        return;
    }
    let now = Instant::now();
    if width == preview.resize_retry_width
        && height == preview.resize_retry_height
        && now < preview.resize_retry_at
    {
        return;
    }
    match resize_preview(preview, width, height, retired_frames) {
        Ok(()) => {
            preview.resize_retry_width = 0;
            preview.resize_retry_height = 0;
            preview.resize_retry_at = now;
            set_active_error(preview.request_serial, "none");
        }
        Err(reason) => {
            preview.resize_retry_width = width;
            preview.resize_retry_height = height;
            preview.resize_retry_at = now + Duration::from_millis(RESIZE_RETRY_MS);
            set_active_error(preview.request_serial, reason);
            crate::log_warn!(
                target: "ui4";
                "ui4 gpgpu-preview resize rejected request={} window={} logical_extent={}x{} source={} retry_ms={} reason={}\n",
                preview.request_serial,
                preview.window.raw(),
                width,
                height,
                source,
                RESIZE_RETRY_MS,
                reason,
            );
        }
    }
}

fn resize_preview(
    preview: &mut ActivePreview,
    logical_width: u32,
    logical_height: u32,
    retired_frames: &mut Vec<FrameHandle>,
) -> Result<(), &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-d01-unavailable")?;
    let (backing_width, backing_height) =
        preview_backing_extent(preview.config.preset, logical_width, logical_height);
    let replacement = create_preview_frame(output, backing_width, backing_height)
        .map_err(preview_frame_create_error_label)?;
    if replace_window_frame(PREVIEW_OWNER, preview.window, replacement).is_err() {
        let _ = destroy_frame(replacement);
        return Err("resize-window-replace-failed");
    }
    let previous = preview.frame;
    preview.frame = replacement;
    preview.width = backing_width;
    preview.height = backing_height;
    preview.next_render = Instant::now();
    preview.static_needs_publish = true;
    retired_frames.push(previous);
    crate::log_info!(
        target: "ui4";
        "ui4 gpgpu-preview resize applied request={} window={} frame={} logical_extent={}x{} backing_extent={}x{} presentation={} plane_mutation=scaler-only\n",
        preview.request_serial,
        preview.window.raw(),
        replacement.raw(),
        logical_width,
        logical_height,
        backing_width,
        backing_height,
        if (backing_width, backing_height) == (logical_width, logical_height) {
            "1:1"
        } else {
            "direct-plane-2x"
        },
    );
    Ok(())
}

fn preview_backing_extent(
    preset: GpgpuPreviewPreset,
    logical_width: u32,
    logical_height: u32,
) -> (u32, u32) {
    if preset == GpgpuPreviewPreset::CppParticle {
        crate::intel::gpgpu::particle_craft_backing_extent(logical_width, logical_height)
    } else {
        (logical_width, logical_height)
    }
}

fn stop_active_previews(
    previews: Vec<ActivePreview>,
    retired_frames: &mut Vec<FrameHandle>,
    reason: &'static str,
) {
    let Some(first) = previews.first() else {
        return;
    };
    if previews
        .iter()
        .any(|preview| preview.config.preset == GpgpuPreviewPreset::CppAudio)
    {
        crate::aud::audio_visualizer::set_enabled(false);
    }
    // One source per hardware slot can close through pipe-scaler geometry and
    // plane constant alpha beside fresh GGTT aliases for the same allocation.
    // No composition target is created; UI4 owns retirement through the final
    // SURFLIVE-backed plane replacement. Three planes use two scaler waves.
    let direct_close = previews_support_direct_close(&previews);
    let frame_lifecycle_transferred = if direct_close {
        finish_window_session_with_request(
            PREVIEW_OWNER,
            first.session,
            WindowSessionCloseRequest::default().direct_plane_animate_and_retire_frames(),
        )
        .is_ok()
    } else {
        false
    };
    if !frame_lifecycle_transferred {
        let _ = finish_window_session(PREVIEW_OWNER, first.session);
    }
    let metrics = aggregate_preview_metrics(&previews);
    crate::log_info!(
        target: "ui4";
        "ui4 gpgpu-preview stopped request={} preset={} frames={} windows={} attempted={} submitted={} completed={} published={} scanout_live={} scanout_superseded={} dropped_busy={} dropped_frame_busy={} dropped_queue_full={} dropped_in_flight={} dropped_cadence={} failed={} late={} elapsed_ms={} reason={} teardown={} frame_retire=after-surflive-display-lease-drain source_buffer_mutation=none\n",
        first.request_serial,
        first.config.preset.label(),
        previews.len(),
        previews.iter().map(preview_surface_count).sum::<usize>(),
        metrics.attempted,
        metrics.submitted,
        metrics.completed,
        metrics.published,
        metrics.scanout_live,
        metrics.scanout_superseded,
        metrics.dropped_busy,
        metrics.dropped_frame_busy,
        metrics.dropped_queue_full,
        metrics.dropped_in_flight,
        metrics.dropped_cadence,
        metrics.failed,
        metrics.late,
        metrics.elapsed_ms,
        reason,
        if frame_lifecycle_transferred {
            "direct-plane-scaler+alpha"
        } else {
            "broker-detach-no-animation"
        },
    );
    for mut preview in previews {
        crate::log_info!(
            target: "ui4";
            "ui4 gpgpu-preview stopped-member request={} preset={} frame={} window={} slot={} attempted={} submitted={} completed={} published={} scanout_live={} scanout_superseded={} dropped_busy={} dropped_frame_busy={} dropped_queue_full={} dropped_in_flight={} dropped_cadence={} failed={} late={} elapsed_ms={} reason={}\n",
            preview.request_serial,
            preview.config.preset.label(),
            preview.frame.raw(),
            preview.window.raw(),
            active_preview_plane_slot(&preview),
            preview.metrics.attempted,
            preview.metrics.submitted,
            preview.metrics.completed,
            preview.metrics.published,
            preview.metrics.scanout_live,
            preview.metrics.scanout_superseded,
            preview.metrics.dropped_busy,
            preview.metrics.dropped_frame_busy,
            preview.metrics.dropped_queue_full,
            preview.metrics.dropped_in_flight,
            preview.metrics.dropped_cadence,
            preview.metrics.failed,
            preview.metrics.late,
            preview.metrics.elapsed_ms,
            reason,
        );
        if let Some(state) = preview.font_rush2.take() {
            CPP_FONT_RUSH2_RETIRED
                .lock()
                .push(CppFontRush2RetiredFrame {
                    frame: preview.frame,
                    state,
                });
        } else if !frame_lifecycle_transferred {
            if !retired_frames.contains(&preview.frame) {
                retired_frames.push(preview.frame);
            }
            for surface in preview.extra_surfaces {
                if !retired_frames.contains(&surface.frame) {
                    retired_frames.push(surface.frame);
                }
            }
        }
    }
}

fn retire_cpp_font_rush2_frames() {
    let mut retired = CPP_FONT_RUSH2_RETIRED.lock();
    let mut index = 0;
    while index < retired.len() {
        match destroy_frame(retired[index].frame) {
            Ok(()) | Err(FramePoolError::InvalidHandle) => {
                let mut entry = retired.swap_remove(index);
                let mut ack_failed = false;
                for produced in &mut entry.state.published {
                    if let Some(produced) = produced.take()
                        && produced.acknowledge_ui4_frame_retirement().is_err()
                    {
                        ack_failed = true;
                    }
                }
                crate::log_info!(target: "ui4";
                    "ui4 cpp-font-rush2 frame retired frame={} producer={} exact_row_acks={} action=destroy-ui4-ring+release-producer-generation\n",
                    entry.frame.raw(),
                    entry.state.producer_index,
                    if ack_failed { "failed-quarantined" } else { "complete" },
                );
                drop(entry);
            }
            Err(FramePoolError::Busy) => index += 1,
            Err(error) => {
                let entry = retired.swap_remove(index);
                crate::log_warn!(target: "ui4";
                    "ui4 cpp-font-rush2 frame retirement abandoned frame={} producer={} error={:?} action=quarantine-generation\n",
                    entry.frame.raw(),
                    entry.state.producer_index,
                    error,
                );
                drop(entry);
            }
        }
    }
}

fn previews_support_direct_close(previews: &[ActivePreview]) -> bool {
    let mut slots = 0u8;
    for preview in previews {
        if !preview.extra_surfaces.is_empty() {
            return false;
        }
        let slot = active_preview_plane_slot(preview);
        if !(1..=3).contains(&slot) {
            return false;
        }
        let bit = 1u8 << slot;
        if slots & bit != 0 {
            return false;
        }
        slots |= bit;
    }
    !previews.is_empty()
}

fn retire_frames(frames: &mut Vec<FrameHandle>) {
    let mut index = 0;
    while index < frames.len() {
        match destroy_frame(frames[index]) {
            Ok(()) | Err(FramePoolError::InvalidHandle) => {
                frames.swap_remove(index);
            }
            Err(FramePoolError::Busy) => index += 1,
            Err(error) => {
                let frame = frames.swap_remove(index);
                crate::log_warn!(
                    target: "ui4";
                    "ui4 gpgpu-preview frame retire abandoned frame={} error={:?}\n",
                    frame.raw(),
                    error,
                );
            }
        }
    }
}

fn mark_starting(desired: DesiredPreview) {
    let mut control = PREVIEW_CONTROL.lock();
    control.status.phase = GpgpuPreviewPhase::Starting;
    control.status.applied_serial = desired.serial;
    control.status.config = desired.config;
    control.status.frame = None;
    control.status.window = None;
    control.status.width = 0;
    control.status.height = 0;
    control.status.metrics = GpgpuPreviewMetrics::default();
    control.status.members = INACTIVE_PREVIEW_MEMBERS;
    control.status.last_error = "none";
}

fn mark_faulted(desired: DesiredPreview, reason: &'static str) {
    let mut control = PREVIEW_CONTROL.lock();
    control.status.phase = GpgpuPreviewPhase::Faulted;
    control.status.applied_serial = desired.serial;
    control.status.frame = None;
    control.status.window = None;
    control.status.width = 0;
    control.status.height = 0;
    control.status.members = INACTIVE_PREVIEW_MEMBERS;
    control.status.last_error = reason;
}

fn mark_idle(serial: u64, reason: &'static str) {
    let mut control = PREVIEW_CONTROL.lock();
    control.status.phase = GpgpuPreviewPhase::Idle;
    control.status.applied_serial = serial;
    control.status.frame = None;
    control.status.window = None;
    control.status.width = 0;
    control.status.height = 0;
    clear_preview_member_handles(&mut control.status.members);
    control.status.last_error = reason;
}

fn mark_duration_complete(
    serial: u64,
    metrics: GpgpuPreviewMetrics,
    mut members: [GpgpuPreviewMemberStatus; 4],
) {
    clear_preview_member_handles(&mut members);
    let mut control = PREVIEW_CONTROL.lock();
    if control.desired.serial == serial {
        control.desired.running = false;
        control.status.desired_running = false;
    }
    control.status.phase = GpgpuPreviewPhase::Idle;
    control.status.applied_serial = serial;
    control.status.frame = None;
    control.status.window = None;
    control.status.width = 0;
    control.status.height = 0;
    control.status.metrics = metrics;
    control.status.members = members;
    control.status.last_error = "duration-complete";
}

fn mark_runtime_fault(
    serial: u64,
    metrics: GpgpuPreviewMetrics,
    mut members: [GpgpuPreviewMemberStatus; 4],
    reason: &'static str,
) {
    clear_preview_member_handles(&mut members);
    let mut control = PREVIEW_CONTROL.lock();
    if control.desired.serial == serial {
        control.desired.running = false;
        control.status.desired_running = false;
    }
    control.status.phase = GpgpuPreviewPhase::Faulted;
    control.status.applied_serial = serial;
    control.status.frame = None;
    control.status.window = None;
    control.status.width = 0;
    control.status.height = 0;
    control.status.metrics = metrics;
    control.status.members = members;
    control.status.last_error = reason;
}

fn publish_active_status(
    previews: &[ActivePreview],
    phase: GpgpuPreviewPhase,
    last_error: &'static str,
) {
    let Some(first) = previews.first() else {
        return;
    };
    let mut control = PREVIEW_CONTROL.lock();
    if control.status.applied_serial != first.request_serial {
        return;
    }
    control.status.phase = phase;
    control.status.config = first.config;
    control.status.frame = Some(first.frame);
    control.status.window = Some(first.window);
    control.status.width = first.width;
    control.status.height = first.height;
    control.status.metrics = aggregate_preview_metrics(previews);
    control.status.members = preview_member_statuses(previews);
    if control.status.last_error == "none" || last_error != "none" {
        control.status.last_error = last_error;
    }
}

fn aggregate_preview_metrics(previews: &[ActivePreview]) -> GpgpuPreviewMetrics {
    let mut aggregate = GpgpuPreviewMetrics::default();
    for preview in previews {
        aggregate.attempted = aggregate
            .attempted
            .saturating_add(preview.metrics.attempted);
        aggregate.submitted = aggregate
            .submitted
            .saturating_add(preview.metrics.submitted);
        aggregate.completed = aggregate
            .completed
            .saturating_add(preview.metrics.completed);
        aggregate.published = aggregate
            .published
            .saturating_add(preview.metrics.published);
        aggregate.scanout_live = aggregate
            .scanout_live
            .saturating_add(preview.metrics.scanout_live);
        aggregate.scanout_superseded = aggregate
            .scanout_superseded
            .saturating_add(preview.metrics.scanout_superseded);
        aggregate.dropped_busy = aggregate
            .dropped_busy
            .saturating_add(preview.metrics.dropped_busy);
        aggregate.dropped_frame_busy = aggregate
            .dropped_frame_busy
            .saturating_add(preview.metrics.dropped_frame_busy);
        aggregate.dropped_queue_full = aggregate
            .dropped_queue_full
            .saturating_add(preview.metrics.dropped_queue_full);
        aggregate.dropped_in_flight = aggregate
            .dropped_in_flight
            .saturating_add(preview.metrics.dropped_in_flight);
        aggregate.dropped_cadence = aggregate
            .dropped_cadence
            .saturating_add(preview.metrics.dropped_cadence);
        aggregate.failed = aggregate.failed.saturating_add(preview.metrics.failed);
        aggregate.late = aggregate.late.saturating_add(preview.metrics.late);
        aggregate.elapsed_ms = aggregate.elapsed_ms.max(preview.metrics.elapsed_ms);
        aggregate.last_iterations = aggregate
            .last_iterations
            .max(preview.metrics.last_iterations);
        aggregate.last_marker = aggregate.last_marker.max(preview.metrics.last_marker);
        aggregate.last_submit_ms = aggregate.last_submit_ms.max(preview.metrics.last_submit_ms);
    }
    aggregate
}

fn preview_member_statuses(previews: &[ActivePreview]) -> [GpgpuPreviewMemberStatus; 4] {
    let mut members = INACTIVE_PREVIEW_MEMBERS;
    for preview in previews {
        let index = if preview.config.preset == GpgpuPreviewPreset::CppFontRush {
            match preview.font_rush.as_ref() {
                Some(state) if usize::from(state.rank) < members.len() => usize::from(state.rank),
                _ => continue,
            }
        } else {
            let Some(index) = compute_preview_index(preview.config.preset) else {
                continue;
            };
            index
        };
        members[index] = GpgpuPreviewMemberStatus {
            preset: preview.config.preset,
            frame: Some(preview.frame),
            window: Some(preview.window),
            plane_slot: active_preview_plane_slot(preview) as u8,
            active: true,
            metrics: preview.metrics,
        };
    }
    members
}

fn clear_preview_member_handles(members: &mut [GpgpuPreviewMemberStatus; 4]) {
    for member in members {
        member.frame = None;
        member.window = None;
        member.active = false;
    }
}

const fn compute_preview_index(preset: GpgpuPreviewPreset) -> Option<usize> {
    match preset {
        GpgpuPreviewPreset::Mandelbrot => Some(0),
        GpgpuPreviewPreset::Chart => Some(1),
        GpgpuPreviewPreset::Plasma => Some(2),
        GpgpuPreviewPreset::All
        | GpgpuPreviewPreset::Static
        | GpgpuPreviewPreset::Static30
        | GpgpuPreviewPreset::Lab256
        | GpgpuPreviewPreset::CppGallery
        | GpgpuPreviewPreset::CppCloudHighWisps
        | GpgpuPreviewPreset::CppAurora
        | GpgpuPreviewPreset::CppJulia
        | GpgpuPreviewPreset::CppSdf
        | GpgpuPreviewPreset::CppVoronoi
        | GpgpuPreviewPreset::CppRetroSun
        | GpgpuPreviewPreset::CppAudio
        | GpgpuPreviewPreset::CppParticle
        | GpgpuPreviewPreset::CppFont
        | GpgpuPreviewPreset::CppFontRush
        | GpgpuPreviewPreset::CppFontRush2 => None,
    }
}

const fn preview_plane_slot(preset: GpgpuPreviewPreset) -> usize {
    if matches!(preset, GpgpuPreviewPreset::CppFontRush | GpgpuPreviewPreset::CppFontRush2) {
        return 0;
    }
    if matches!(
        preset,
        GpgpuPreviewPreset::CppGallery
            | GpgpuPreviewPreset::CppAurora
            | GpgpuPreviewPreset::CppJulia
            | GpgpuPreviewPreset::CppSdf
            | GpgpuPreviewPreset::CppVoronoi
            | GpgpuPreviewPreset::CppRetroSun
            | GpgpuPreviewPreset::CppAudio
            | GpgpuPreviewPreset::CppParticle
    ) {
        return 1;
    }
    match compute_preview_index(preset) {
        Some(index) => index + 1,
        None => super::ALPHA_OVERLAY_PLANE_SLOT,
    }
}

fn active_preview_plane_slot(preview: &ActivePreview) -> usize {
    super::window_broker::window_snapshot(PREVIEW_OWNER, preview.window)
        .map(|window| window.plane.slot())
        .unwrap_or_else(|| {
            preview.font_rush.as_ref().map_or_else(
                || preview_plane_slot(preview.config.preset),
                |state| state.plane_slot as usize,
            )
        })
}

fn next_preview_group_poll_ms(previews: &[ActivePreview]) -> u64 {
    previews
        .iter()
        .map(next_poll_ms)
        .min()
        .unwrap_or(IDLE_POLL_MS)
}

fn set_active_error(serial: u64, reason: &'static str) {
    let mut control = PREVIEW_CONTROL.lock();
    if control.status.applied_serial == serial {
        control.status.last_error = reason;
    }
}

const fn next_serial(serial: u64) -> u64 {
    let next = serial.wrapping_add(1);
    if next == 0 { 1 } else { next }
}

#[cfg(test)]
mod tests {
    use super::{
        CPP_FONT_RUSH_BLANK_MIN_MS, CPP_FONT_RUSH_CADENCE_MS, CPP_FONT_RUSH_SECTION_CADENCE_MS,
        CPP_FONT_RUSH_SECTION_PRESENTATION_SCALE, CPP_FONT_RUSH_TITLE_LETTER_MAX_FONT_PIXELS,
        CPP_FONT_RUSH_TITLE_LETTERS, CPP_FONT_RUSH_TITLE_WORD,
        CPP_FONT_RUSH_TITLE_WORD_MAX_FONT_PIXELS, CPP_FONT_RUSH_TITLE_WORD_PRESENTATION_SCALE,
        CPP_FONT_RUSH_TITLE_WORD_X_FRACTIONS, CPP_GALLERY_PRESETS, CppFontRushLayerStage,
        CppFontRushShowcaseSource, FramePlanError, FramePoolError, GPGPU_PREVIEW_MAX_CADENCE_MS,
        GpgpuPreviewConfig, GpgpuPreviewPreset, LAB256_PREVIEW_SIZE, PREVIEW_HEIGHT, PREVIEW_WIDTH,
        cpp_font_rush_due_tick_count, cpp_font_rush_glyph_count, cpp_font_rush_grid,
        cpp_font_rush_plan_request, cpp_font_rush_plane_slots, cpp_font_rush_showcase_next_stage,
        cpp_font_rush_showcase_source_request, cpp_font_rush_showcase_sprite_layout,
        cpp_font_rush_target_plane_count, cycled_cpp_gallery_preset, preview_extent,
        preview_frame_create_error_label, preview_plane_slot, static30_font_stamp_request,
    };

    #[test]
    fn lab256_preview_keeps_artifact_extent() {
        assert_eq!(
            preview_extent(GpgpuPreviewPreset::Lab256),
            (LAB256_PREVIEW_SIZE, LAB256_PREVIEW_SIZE)
        );
    }

    #[test]
    fn static30_builds_bounded_lorem_canvas_layers() {
        let request = static30_font_stamp_request(320, 180, 7);
        assert_eq!(request.fit, crate::r::font_kernel_service::FontStampFit::Canvas);
        assert_eq!(request.layers.len(), 2);
        assert!(request.layers.iter().all(|layer| {
            layer.scene.raster_width == 320
                && layer.scene.raster_height == 180
                && layer.scene.viewport_width == 320
                && layer.scene.viewport_height == 180
        }));
        assert_eq!(
            request
                .layers
                .iter()
                .flat_map(|layer| layer.scene.runs.iter())
                .map(|run| run.text.chars().count())
                .sum::<usize>(),
            53
        );
    }

    #[test]
    fn cpp_font_rush_maps_each_plane_to_its_requested_grid() {
        assert_eq!(cpp_font_rush_glyph_count(0, false), 1);
        assert_eq!(cpp_font_rush_grid(0, false), (1, 1));
        assert_eq!(cpp_font_rush_glyph_count(1, false), 2);
        assert_eq!(cpp_font_rush_grid(1, false), (2, 1));
        assert_eq!(cpp_font_rush_glyph_count(2, false), 4);
        assert_eq!(cpp_font_rush_grid(2, false), (2, 2));
        assert_eq!(cpp_font_rush_glyph_count(3, false), 16);
        assert_eq!(cpp_font_rush_grid(3, false), (4, 4));

        assert_eq!(cpp_font_rush_glyph_count(0, true), 4);
        assert_eq!(cpp_font_rush_grid(0, true), (2, 2));
        assert_eq!(cpp_font_rush_glyph_count(1, true), 8);
        assert_eq!(cpp_font_rush_grid(1, true), (4, 2));
        assert_eq!(cpp_font_rush_glyph_count(2, true), 16);
        assert_eq!(cpp_font_rush_grid(2, true), (4, 4));
        assert_eq!(cpp_font_rush_glyph_count(3, true), 64);
        assert_eq!(cpp_font_rush_grid(3, true), (8, 8));
    }

    #[test]
    fn cpp_font_rush_enumerates_sparse_application_plane_masks() {
        assert_eq!(cpp_font_rush_plane_slots(0b0000), ([0, 0, 0, 0], 0));
        assert_eq!(cpp_font_rush_plane_slots(0b0001), ([0, 0, 0, 0], 1));
        assert_eq!(cpp_font_rush_plane_slots(0b0011), ([0, 1, 0, 0], 2));
        assert_eq!(cpp_font_rush_plane_slots(0b0111), ([0, 1, 2, 0], 3));
        assert_eq!(cpp_font_rush_plane_slots(0b1111), ([0, 1, 2, 3], 4));
        assert_eq!(cpp_font_rush_plane_slots(0b1101), ([0, 2, 3, 0], 3));
        assert_eq!(cpp_font_rush_plane_slots(0b1110), ([1, 2, 3, 0], 3));
        assert_eq!(cpp_font_rush_plane_slots(u8::MAX), ([0, 1, 2, 3], 4));
    }

    #[test]
    fn cpp_font_rush_adds_one_capability_bounded_plane_every_three_seconds() {
        assert_eq!(cpp_font_rush_target_plane_count(0, 4), 1);
        assert_eq!(cpp_font_rush_target_plane_count(2_999, 4), 1);
        assert_eq!(cpp_font_rush_target_plane_count(3_000, 4), 2);
        assert_eq!(cpp_font_rush_target_plane_count(5_999, 4), 2);
        assert_eq!(cpp_font_rush_target_plane_count(6_000, 4), 3);
        assert_eq!(cpp_font_rush_target_plane_count(9_000, 4), 4);
        assert_eq!(cpp_font_rush_target_plane_count(u64::MAX, 2), 2);
        assert_eq!(cpp_font_rush_target_plane_count(0, 0), 0);
    }

    #[test]
    fn cpp_font_rush_uses_the_fastest_cadence_from_start() {
        assert_eq!(CPP_FONT_RUSH_CADENCE_MS, 250);
    }

    #[test]
    fn cpp_font_rush_is_pinned_to_lucida_face_one() {
        use crate::intel::gpu_font::GpuFontFace;

        assert_eq!(GpuFontFace::Default.id(), 1);
        assert_eq!(GpuFontFace::Default.registry_name(), "font");
        assert_eq!(CppFontRushLayerStage::SectionPulse.cadence_ms(), 100);
        assert_eq!(CPP_FONT_RUSH_SECTION_CADENCE_MS, 100);
    }

    #[test]
    fn cpp_font_rush_blanks_before_section_pulse() {
        assert_eq!(CPP_FONT_RUSH_BLANK_MIN_MS, 2_000);
        assert_eq!(
            cpp_font_rush_showcase_next_stage(CppFontRushLayerStage::TitleHold),
            Ok(CppFontRushLayerStage::BlankPrime),
        );
        let stage = cpp_font_rush_showcase_next_stage(CppFontRushLayerStage::BlankPrime).unwrap();
        assert_eq!(stage, CppFontRushLayerStage::BlankHold);
        assert_eq!(
            cpp_font_rush_showcase_next_stage(stage),
            Ok(CppFontRushLayerStage::SectionPulse),
        );
        assert_eq!(
            cpp_font_rush_showcase_next_stage(CppFontRushLayerStage::SectionPulse),
            Ok(CppFontRushLayerStage::StormPrime { mirror: 0 }),
        );
    }

    #[test]
    fn cpp_font_rush_clears_to_transparent_over_pipe_bottom_color() {
        let cleared = [
            CppFontRushLayerStage::Base,
            CppFontRushLayerStage::Expanded,
            CppFontRushLayerStage::TitleLetter(0),
            CppFontRushLayerStage::TitleHold,
            CppFontRushLayerStage::SectionPulse,
            CppFontRushLayerStage::StormPrime { mirror: 0 },
        ];
        for stage in cleared {
            assert_eq!(
                stage.clear_color().map(|color| color.to_native_bytes()),
                Some([0, 0, 0, 0]),
            );
        }
        assert!(
            CppFontRushLayerStage::ProducerStorm { wave: 0, mirror: 0 }
                .clear_color()
                .is_none()
        );
        assert!(CppFontRushLayerStage::BlankPrime.clear_color().is_none());
        assert!(CppFontRushLayerStage::BlankHold.clear_color().is_none());
        assert!(CppFontRushLayerStage::Dormant.clear_color().is_none());
    }

    #[test]
    fn cpp_font_rush_due_ticks_remain_phase_anchored() {
        assert_eq!(cpp_font_rush_due_tick_count(249, 250, 250), 0);
        assert_eq!(cpp_font_rush_due_tick_count(250, 250, 250), 1);
        assert_eq!(cpp_font_rush_due_tick_count(499, 250, 250), 1);
        assert_eq!(cpp_font_rush_due_tick_count(500, 250, 250), 2);
        assert_eq!(cpp_font_rush_due_tick_count(1_150, 250, 250), 4);
        assert_eq!(cpp_font_rush_due_tick_count(1_150, 750, 250), 2);
        assert_eq!(cpp_font_rush_due_tick_count(250, 250, 0), 0);
    }

    #[test]
    fn cpp_font_rush_ui_only_describes_seeded_cells() {
        for expanded_grid in [false, true] {
            for rank in 0..4u8 {
                let mut rng = crate::tyche::SoftRng::from_seed(
                    0xC0DE_0000 + (u64::from(u8::from(expanded_grid)) << 4) + u64::from(rank),
                );
                let (request, glyphs, grid) = cpp_font_rush_plan_request(
                    2_560,
                    1_440,
                    rank,
                    if expanded_grid {
                        CppFontRushLayerStage::Expanded
                    } else {
                        CppFontRushLayerStage::Base
                    },
                    1,
                    crate::intel::gpu_font::GpuFontFace::Default,
                    &mut rng,
                )
                .unwrap();
                assert_eq!(glyphs, cpp_font_rush_glyph_count(rank, expanded_grid));
                assert_eq!(grid, cpp_font_rush_grid(rank, expanded_grid));
                assert_eq!(glyphs, usize::from(grid.0) * usize::from(grid.1));
                assert_eq!(request.font(), crate::intel::gpu_font::GpuFontFace::Default,);
                assert_eq!(request.viewport_extent(), (640, 360));
                assert_eq!(request.raster_extent(), (2_560, 1_440));
                assert_eq!(request.cells().len(), glyphs);
                assert_eq!(request.parallelism(), 1);
                let per_glyph_work_limit =
                    crate::intel::gpu_font::gpu_font_analytical_work_limit() / glyphs as u64;
                let cell_width = 640.0 / f32::from(grid.0);
                let cell_height = 360.0 / f32::from(grid.1);
                for (index, cell) in request.cells().iter().copied().enumerate() {
                    assert_eq!(
                        cell.position(),
                        [
                            ((index % usize::from(grid.0)) as f32 + 0.5) * cell_width,
                            ((index / usize::from(grid.0)) as f32 + 0.5) * cell_height,
                        ],
                    );
                    assert!(cell.font_pixels() > 0.0);
                    assert_eq!(cell.max_work(), per_glyph_work_limit);
                    assert_ne!(cell.rng_seed(), 0);
                    assert_eq!(cell.fixed_scalar(), None);
                    assert_eq!(cell.worker_affinity(), None);
                }
            }
        }
        assert!(
            cpp_font_rush_glyph_count(3, true)
                <= crate::r::font_plan_service::FONT_PLAN_MAX_CELLS_PER_BATCH
        );
    }

    #[test]
    fn cpp_font_rush_raw_storm_uses_all_32_producers_and_mirrors_one_wave() {
        let mut rng = crate::tyche::SoftRng::from_seed(7);
        let raw = |mirror| {
            cpp_font_rush_plan_request(
                2_560,
                1_440,
                0,
                CppFontRushLayerStage::ProducerStorm { wave: 19, mirror },
                1,
                crate::intel::gpu_font::GpuFontFace::Default,
                &mut rng,
            )
            .unwrap()
        };
        let (first, glyphs, grid) = raw(0);
        let (second, mirrored_glyphs, mirrored_grid) = raw(1);

        assert_eq!(glyphs, CPP_FONT_RUSH_RAW_STORM_GLYPHS);
        assert_eq!(grid, (16, 4));
        assert_eq!(mirrored_glyphs, glyphs);
        assert_eq!(mirrored_grid, grid);
        assert_eq!(first.parallelism(), crate::r::font_plan_service::FONT_PLAN_WORKER_COUNT);
        assert_eq!(first.cells().len(), glyphs);
        for worker in 0..crate::r::font_plan_service::FONT_PLAN_WORKER_COUNT {
            let cells = &first.cells()[worker * 2..worker * 2 + 2];
            assert_eq!(cells[0].worker_affinity(), Some(worker as u8));
            assert_eq!(cells[1].worker_affinity(), Some(worker as u8));
            assert_ne!(cells[0].position(), cells[1].position());
            assert_eq!(cells[0].position(), second.cells()[worker * 2].position());
            assert_eq!(cells[1].position(), second.cells()[worker * 2 + 1].position());
            assert_eq!(cells[0].fixed_scalar(), second.cells()[worker * 2].fixed_scalar());
            assert_eq!(cells[1].fixed_scalar(), second.cells()[worker * 2 + 1].fixed_scalar());
        }
    }

    #[test]
    fn cpp_font_rush_title_letters_stay_on_the_prepared_pool_path() {
        let mut rng = crate::tyche::SoftRng::from_seed(7);
        let work_limit = crate::intel::gpu_font::gpu_font_analytical_work_limit();
        for (index, expected) in CPP_FONT_RUSH_TITLE_LETTERS.iter().copied().enumerate() {
            let (request, glyphs, grid) = cpp_font_rush_plan_request(
                2_560,
                1_440,
                0,
                CppFontRushLayerStage::TitleLetter(index as u8),
                1,
                crate::intel::gpu_font::GpuFontFace::Default,
                &mut rng,
            )
            .unwrap();
            assert_eq!(glyphs, 1);
            assert_eq!(grid, (1, 1));
            assert_eq!(request.parallelism(), 1);
            assert_eq!(request.cells().len(), 1);
            let cell = request.cells()[0];
            assert_eq!(cell.fixed_scalar(), Some(expected));
            assert_eq!(cell.position(), [320.0, 180.0]);
            assert_eq!(cell.font_pixels(), CPP_FONT_RUSH_TITLE_LETTER_MAX_FONT_PIXELS);
            assert_eq!(cell.max_work(), work_limit);
        }

        assert!(matches!(
            cpp_font_rush_plan_request(
                2_560,
                1_440,
                0,
                CppFontRushLayerStage::TitleLetter(CPP_FONT_RUSH_TITLE_LETTERS.len() as u8),
                1,
                crate::intel::gpu_font::GpuFontFace::Default,
                &mut rng,
            ),
            Err("font-rush-title-letter-index")
        ));

        for stage in [
            CppFontRushLayerStage::TitleHold,
            CppFontRushLayerStage::SectionPulse,
            CppFontRushLayerStage::StormPrime { mirror: 0 },
        ] {
            assert!(matches!(
                cpp_font_rush_plan_request(
                    2_560,
                    1_440,
                    0,
                    stage,
                    2,
                    crate::intel::gpu_font::GpuFontFace::Default,
                    &mut rng,
                ),
                Err("font-rush-showcase-is-resident-rgba8-sprite-only")
            ));
        }
    }

    #[test]
    fn cpp_font_rush_showcase_builds_each_exact_white_source_once() {
        use crate::r::font_kernel_service::{FontStampFit, RetainedFontPositioning};

        let title =
            cpp_font_rush_showcase_source_request(CppFontRushShowcaseSource::Title, 2_560, 1_440);
        assert_eq!(title.fit, FontStampFit::Tight);
        assert_eq!(title.layers.len(), 1);
        assert_eq!(title.layers[0].scene.positioning, RetainedFontPositioning::VisualBoundsCenter);
        assert_eq!(title.layers[0].scene.viewport_width, 640);
        assert_eq!(title.layers[0].scene.viewport_height, 360);
        assert_eq!(title.layers[0].scene.runs.len(), CPP_FONT_RUSH_TITLE_WORD.len());
        assert_eq!(
            title.layers[0]
                .scene
                .runs
                .iter()
                .flat_map(|run| run.text.chars())
                .collect::<alloc::vec::Vec<_>>(),
            CPP_FONT_RUSH_TITLE_WORD,
        );
        for (run, fraction) in title.layers[0]
            .scene
            .runs
            .iter()
            .zip(CPP_FONT_RUSH_TITLE_WORD_X_FRACTIONS)
        {
            assert!((run.position[0] - 640.0 * fraction).abs() < 0.01);
            assert_eq!(run.position[1], 180.0);
            assert_eq!(run.font_pixels, CPP_FONT_RUSH_TITLE_WORD_MAX_FONT_PIXELS);
        }

        let section =
            cpp_font_rush_showcase_source_request(CppFontRushShowcaseSource::Section, 2_560, 1_440);
        assert_eq!(section.fit, FontStampFit::Tight);
        assert_eq!(section.layers.len(), 1);
        assert_eq!(section.layers[0].scene.runs.len(), 1);
        assert_eq!(section.layers[0].scene.runs[0].text, "§");
        assert_eq!(section.layers[0].scene.runs[0].position, [320.0, 187.2]);
        assert_eq!(section.layers[0].scene.runs[0].font_pixels, 102.4);
    }

    #[test]
    fn cpp_font_rush_showcase_scales_resident_sprite_without_clipping() {
        // Representative Lucida tight bounds at the proven source sizes.
        let title = cpp_font_rush_showcase_sprite_layout(
            CppFontRushLayerStage::TitleHold,
            1,
            1_400,
            352,
            2_560,
            1_440,
        )
        .unwrap();
        assert_eq!(title.glyphs, 6);
        assert_eq!(title.descriptors.len(), 1);
        assert!(title.scale > 1.7);
        assert!(title.scale <= CPP_FONT_RUSH_TITLE_WORD_PRESENTATION_SCALE);
        assert!(title.scale * title.scale > 3.0);
        let word = title.descriptors[0];
        assert!(word.c0_x >= 0.0 && word.c1_x <= 2_560.0);
        assert!(word.c0_y >= 0.0 && word.c3_y <= 1_440.0);

        let sections = cpp_font_rush_showcase_sprite_layout(
            CppFontRushLayerStage::SectionPulse,
            2,
            176,
            370,
            2_560,
            1_440,
        )
        .unwrap();
        assert_eq!(sections.glyphs, 3);
        assert_eq!(sections.descriptors.len(), 3);
        assert_eq!(sections.scale, CPP_FONT_RUSH_SECTION_PRESENTATION_SCALE);
        for descriptor in sections.descriptors {
            assert!(descriptor.c0_x >= 0.0 && descriptor.c1_x <= 2_560.0);
            assert!(descriptor.c0_y >= 0.0 && descriptor.c3_y <= 1_440.0);
        }

        assert!(matches!(
            cpp_font_rush_showcase_sprite_layout(
                CppFontRushLayerStage::SectionPulse,
                3,
                500,
                500,
                800,
                600,
            ),
            Err("font-rush-showcase-sprite-out-of-bounds")
        ));
    }

    #[test]
    fn cpp_font_rush_uses_primary_as_its_first_plane() {
        let mode = GpgpuPreviewPreset::CppFontRush;
        assert_eq!(preview_plane_slot(mode), 0);
        assert_eq!(mode.buffering_label(), "double-per-plane");
        assert_eq!(mode.plane_layout_label(), "slots0+1+2+3-direct-capability-bounded");
    }

    #[test]
    fn cpp_modes_share_the_resizable_slot1_application_surface() {
        let modes = [
            GpgpuPreviewPreset::CppGallery,
            GpgpuPreviewPreset::CppCloudHighWisps,
            GpgpuPreviewPreset::CppAurora,
            GpgpuPreviewPreset::CppJulia,
            GpgpuPreviewPreset::CppSdf,
            GpgpuPreviewPreset::CppVoronoi,
            GpgpuPreviewPreset::CppRetroSun,
            GpgpuPreviewPreset::CppAudio,
        ];
        for mode in modes {
            assert_eq!(preview_extent(mode), (PREVIEW_WIDTH, PREVIEW_HEIGHT));
            assert_eq!(preview_plane_slot(mode), 1);
            assert_eq!(mode.buffering_label(), "double");
            assert_eq!(mode.plane_layout_label(), "slot1-direct");
        }
    }

    #[test]
    fn interactive_cpp_gallery_cycles_both_directions_and_wraps() {
        assert_eq!(
            cycled_cpp_gallery_preset(GpgpuPreviewPreset::CppGallery, 1),
            GpgpuPreviewPreset::CppCloudHighWisps,
        );
        assert_eq!(
            cycled_cpp_gallery_preset(GpgpuPreviewPreset::CppGallery, -1),
            GpgpuPreviewPreset::Static30,
        );
        assert_eq!(
            cycled_cpp_gallery_preset(GpgpuPreviewPreset::Static30, 1),
            GpgpuPreviewPreset::CppGallery,
        );
        assert_eq!(CPP_GALLERY_PRESETS.len(), 10);
    }

    #[test]
    fn cloud_brush_uses_frame_local_normalized_points_and_bounds_its_ring() {
        let mut brush = CloudBrushState::new();
        brush.push(0, 0, 101, 51);
        brush.push(100, 50, 101, 51);
        assert_eq!(brush.points[0], 0);
        assert_eq!(brush.points[1], u32::MAX);

        brush.last = Some((0, 25));
        brush.drag_to(100, 25, 101, 51);
        assert!(brush.count > 2);
        assert!(brush.count <= crate::intel::gpgpu::CPP_CLOUD_BRUSH_POINT_CAPACITY);

        for index in 0..64 {
            brush.push(index, index, 101, 51);
        }
        assert_eq!(brush.count, crate::intel::gpgpu::CPP_CLOUD_BRUSH_POINT_CAPACITY);
    }

    #[test]
    fn particle_craft_starts_native_and_uses_the_resizable_cpp_plane() {
        let mode = GpgpuPreviewPreset::CppParticle;
        assert_eq!(
            preview_extent(mode),
            (
                crate::intel::gpgpu::PARTICLE_CRAFT_FRAME_WIDTH,
                crate::intel::gpgpu::PARTICLE_CRAFT_FRAME_HEIGHT,
            )
        );
        assert_eq!(preview_plane_slot(mode), 1);
        assert_eq!(mode.buffering_label(), "double");
        assert_eq!(mode.plane_layout_label(), "slot1-direct");
        assert_eq!(crate::intel::gpgpu::particle_craft_sample_extent(640, 400), (320, 200));
        let maximized_backing = super::preview_backing_extent(mode, 2560, 1440);
        assert_eq!(maximized_backing, (1280, 720));
        assert_eq!(
            crate::intel::gpgpu::particle_craft_sample_extent(
                maximized_backing.0,
                maximized_backing.1,
            ),
            maximized_backing,
        );
    }

    #[test]
    fn preview_config_accepts_continuous_duration() {
        assert!(
            GpgpuPreviewConfig {
                preset: GpgpuPreviewPreset::Plasma,
                duration_ms: 0,
                cadence_ms: 16,
                publish_every: 2,
            }
            .validate()
            .is_ok()
        );
    }

    #[test]
    fn preview_config_rejects_invalid_scheduler_values() {
        assert!(
            GpgpuPreviewConfig {
                preset: GpgpuPreviewPreset::Mandelbrot,
                duration_ms: 1,
                cadence_ms: 0,
                publish_every: 1,
            }
            .validate()
            .is_err()
        );
        assert!(
            GpgpuPreviewConfig {
                preset: GpgpuPreviewPreset::Chart,
                duration_ms: 1,
                cadence_ms: GPGPU_PREVIEW_MAX_CADENCE_MS + 1,
                publish_every: 1,
            }
            .validate()
            .is_err()
        );
        assert!(
            GpgpuPreviewConfig {
                preset: GpgpuPreviewPreset::Plasma,
                duration_ms: 1,
                cadence_ms: 1,
                publish_every: 0,
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn preview_frame_create_errors_keep_stable_detail() {
        assert_eq!(
            preview_frame_create_error_label(FramePoolError::OutOfMemory),
            "frame-create-out-of-memory"
        );
        assert_eq!(
            preview_frame_create_error_label(FramePoolError::UnsupportedFormat),
            "frame-create-unsupported-format"
        );
        assert_eq!(
            preview_frame_create_error_label(FramePoolError::InvalidPlan(
                FramePlanError::EmptyExtent
            )),
            "frame-create-invalid-plan-empty-extent"
        );
    }
}
