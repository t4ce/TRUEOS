//! Kernel-owned cache for reusable GPU font geometry.
//!
//! The graphics font registry owns resident bytes and size-independent Skrifa
//! outlines. This service owns the reusable default mesh, one-shot arbitrary
//! text jobs, and explicitly tagged persistent jobs. A persistent-job lease
//! transfers its prepared geometry from CPU-build authority to a dedicated
//! render-PPGTT allocation; later draws borrow that allocation without another
//! geometry upload. Native size, row grouping, and eventual color remain draw
//! properties.

use alloc::{string::String, sync::Arc, vec::Vec};

use embassy_time::Instant;
use spin::Mutex;

use crate::graphics::font::{FontTesselMesh, FontTesselSummary};
pub(crate) use crate::graphics::primitives::Rgba8 as GpuFontRgba;

pub(crate) const MAX_DYNAMIC_TEXT_CHARS: usize = 256;
const MIN_FONT_STAMP_SIZE_PERCENT: u32 = 1;
const MAX_FONT_STAMP_SIZE_PERCENT: u32 = 100;
const NATIVE_FONT_STAMP_PADDING_PIXELS: u32 = 2;
const UI4_DOCUMENT_MIN_FONT_PIXELS: f32 = 12.0;
const UI4_DOCUMENT_MAX_FONT_PIXELS: f32 = 128.0;
const UI4_DOCUMENT_LINE_HEIGHT_SCALE: f32 = 1.25;
const UI4_DOCUMENT_PADDING_PIXELS: u32 = 24;
const DEFAULT_FONT_FILL_TOLERANCE: f32 = 0.1;
const MIN_NATIVE_FONT_FILL_TOLERANCE: f32 = 0.005;
const NATIVE_FONT_CURVE_ERROR_PIXELS: f32 = 0.2;
const SMALL_FONT_HINT_MIN_RASTER_PX: f32 = 8.0;
const SMALL_FONT_HINT_MAX_RASTER_PX: f32 = 32.0;
const ANALYTICAL_COVERAGE_MIN_RASTER_PX: f32 = 4.0;
const ANALYTICAL_COVERAGE_MAX_RASTER_PX: f32 = 2_048.0;
// The analytical kernel visits every pixel in the outline envelope and walks
// every outline operation (with fixed curve subdivision) for that pixel. Keep
// one synchronous shell/UI producer comfortably below its 500 ms retirement
// deadline. Larger display-sized lettering is cheaper and safer as resident
// triangles; this check happens before any GPGPU submission owns resources.
const ANALYTICAL_COVERAGE_MAX_SEGMENT_EVALUATIONS: u64 = 64_000_000;

/// Default font color. It follows the same draw-time RGBA specialization as
/// every other color; there is no separate baked-blue presentation path.
pub(crate) const GPU_FONT_DEFAULT_RGBA: GpuFontRgba = GpuFontRgba::new(0, 64, 255, 255);
pub(crate) const GPU_FONT_COLOR_KEYFRAME_CAPACITY: usize = 8;

/// Select which components a color transition is allowed to change.
/// Components outside the mask retain their value from `from`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GpuFontColorChannels(u8);

impl GpuFontColorChannels {
    const RED_BIT: u8 = 1 << 0;
    const GREEN_BIT: u8 = 1 << 1;
    const BLUE_BIT: u8 = 1 << 2;
    const ALPHA_BIT: u8 = 1 << 3;

    pub(crate) const RED: Self = Self(Self::RED_BIT);
    pub(crate) const GREEN: Self = Self(Self::GREEN_BIT);
    pub(crate) const BLUE: Self = Self(Self::BLUE_BIT);
    pub(crate) const ALPHA: Self = Self(Self::ALPHA_BIT);
    pub(crate) const RGB: Self = Self(Self::RED_BIT | Self::GREEN_BIT | Self::BLUE_BIT);
    pub(crate) const RGBA: Self = Self(Self::RGB.0 | Self::ALPHA_BIT);

    pub(crate) const fn from_bits(bits: u8) -> Option<Self> {
        if bits != 0 && bits & !Self::RGBA.0 == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }

    pub(crate) const fn bits(self) -> u8 {
        self.0
    }

    pub(crate) const fn name(self) -> &'static str {
        match self.0 {
            Self::RED_BIT => "r",
            Self::GREEN_BIT => "g",
            Self::BLUE_BIT => "b",
            Self::ALPHA_BIT => "a",
            value if value == Self::RGB.0 => "rgb",
            value if value == Self::RGBA.0 => "rgba",
            _ => "custom",
        }
    }

    const fn contains(self, bit: u8) -> bool {
        self.0 & bit != 0
    }
}

/// Time-to-progress function, equivalent to the small useful subset of CSS
/// timing functions needed for font color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GpuFontColorTiming {
    Linear,
    EaseInOutSine,
}

impl GpuFontColorTiming {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::EaseInOutSine => "sine",
        }
    }
}

/// `Alternate` is an infinite forward/reverse loop, matching the common CSS
/// `animation-iteration-count: infinite; animation-direction: alternate` case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GpuFontColorIteration {
    Once,
    Loop,
    Alternate,
}

impl GpuFontColorIteration {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Once => "once",
            Self::Loop => "loop",
            Self::Alternate => "alternate",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GpuFontColorTransition {
    pub(crate) from: GpuFontRgba,
    pub(crate) to: GpuFontRgba,
    pub(crate) channels: GpuFontColorChannels,
    pub(crate) duration_ms: u32,
    pub(crate) timing: GpuFontColorTiming,
    pub(crate) iteration: GpuFontColorIteration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GpuFontColorKeyframe {
    pub(crate) offset_permille: u16,
    pub(crate) rgba: GpuFontRgba,
}

impl GpuFontColorKeyframe {
    pub(crate) const EMPTY: Self = Self {
        offset_permille: 0,
        rgba: GpuFontRgba::new(0, 0, 0, 0),
    };
}

/// Fixed-storage CSS-like color keyframes. Validation belongs to the producer
/// boundary; the sampler can therefore stay allocation-free in the frame path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GpuFontColorKeyframes {
    pub(crate) frames: [GpuFontColorKeyframe; GPU_FONT_COLOR_KEYFRAME_CAPACITY],
    pub(crate) frame_count: u8,
    pub(crate) channels: GpuFontColorChannels,
    pub(crate) duration_ms: u32,
    pub(crate) timing: GpuFontColorTiming,
    pub(crate) iteration: GpuFontColorIteration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GpuFontColorProgram {
    Static(GpuFontRgba),
    Transition(GpuFontColorTransition),
    Keyframes(GpuFontColorKeyframes),
}

impl GpuFontColorProgram {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Static(_) => "static",
            Self::Transition(_) => "transition",
            Self::Keyframes(_) => "keyframes",
        }
    }

    pub(crate) fn sample(self, elapsed_ms: u64) -> GpuFontRgba {
        match self {
            Self::Static(rgba) => rgba,
            Self::Transition(transition) => transition.sample(elapsed_ms),
            Self::Keyframes(keyframes) => keyframes.sample(elapsed_ms),
        }
    }
}

/// Static CSS-like presentation properties retained alongside one analytical
/// coverage layer. Integer transport units make the producer ABI deterministic
/// while the C++ kernel consumes normalized floating-point values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GpuFontInstanceStyle {
    pub(crate) rotation_centidegrees: i16,
    pub(crate) scale_permille: u16,
    pub(crate) opacity_permille: u16,
    pub(crate) background: GpuFontRgba,
}

impl GpuFontInstanceStyle {
    pub(crate) const IDENTITY: Self = Self {
        rotation_centidegrees: 0,
        scale_permille: 1_000,
        opacity_permille: 1_000,
        background: GpuFontRgba::new(0, 0, 0, 0),
    };
}

/// One predefined trigonometric oscillator. Rotation, scale, opacity, and
/// translation are evaluated from the same phase entirely on the GPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GpuFontInstanceMotion {
    pub(crate) period_ms: u32,
    pub(crate) phase_permille: u16,
    pub(crate) rotation_amplitude_centidegrees: i16,
    pub(crate) scale_amplitude_permille: i16,
    pub(crate) opacity_amplitude_permille: i16,
    pub(crate) translation_x_tenths_px: i16,
    pub(crate) translation_y_tenths_px: i16,
}

impl GpuFontInstanceMotion {
    pub(crate) const NONE: Self = Self {
        period_ms: 0,
        phase_permille: 0,
        rotation_amplitude_centidegrees: 0,
        scale_amplitude_permille: 0,
        opacity_amplitude_permille: 0,
        translation_x_tenths_px: 0,
        translation_y_tenths_px: 0,
    };

    pub(crate) const fn is_active(self) -> bool {
        self.period_ms != 0
            && (self.rotation_amplitude_centidegrees != 0
                || self.scale_amplitude_permille != 0
                || self.opacity_amplitude_permille != 0
                || self.translation_x_tenths_px != 0
                || self.translation_y_tenths_px != 0)
    }
}

/// Complete producer-facing program for one Gridpaper text-color selector.
/// Coverage is deliberately absent: Skrifa owns that immutable input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GpuFontInstanceProgram {
    pub(crate) color: Option<GpuFontColorProgram>,
    pub(crate) style: GpuFontInstanceStyle,
    pub(crate) motion: GpuFontInstanceMotion,
}

impl GpuFontInstanceProgram {
    pub(crate) const fn color_only(color: GpuFontColorProgram) -> Self {
        Self {
            color: Some(color),
            style: GpuFontInstanceStyle::IDENTITY,
            motion: GpuFontInstanceMotion::NONE,
        }
    }

    pub(crate) fn sample_color(self, fallback: GpuFontRgba, elapsed_ms: u64) -> GpuFontRgba {
        self.color
            .map_or(fallback, |program| program.sample(elapsed_ms))
    }

    pub(crate) const fn needs_continuous_frames(self) -> bool {
        self.motion.is_active()
            || matches!(
                self.color,
                Some(GpuFontColorProgram::Transition(_) | GpuFontColorProgram::Keyframes(_))
            )
    }
}

impl GpuFontColorTransition {
    fn sample(self, elapsed_ms: u64) -> GpuFontRgba {
        let linear_progress =
            animation_linear_progress(self.duration_ms, self.iteration, elapsed_ms);
        let progress = animation_timed_progress(self.timing, linear_progress);
        GpuFontRgba::new(
            transition_component(
                self.from.r,
                self.to.r,
                progress,
                self.channels.contains(GpuFontColorChannels::RED_BIT),
            ),
            transition_component(
                self.from.g,
                self.to.g,
                progress,
                self.channels.contains(GpuFontColorChannels::GREEN_BIT),
            ),
            transition_component(
                self.from.b,
                self.to.b,
                progress,
                self.channels.contains(GpuFontColorChannels::BLUE_BIT),
            ),
            transition_component(
                self.from.a,
                self.to.a,
                progress,
                self.channels.contains(GpuFontColorChannels::ALPHA_BIT),
            ),
        )
    }
}

impl GpuFontColorKeyframes {
    fn sample(self, elapsed_ms: u64) -> GpuFontRgba {
        let count = usize::from(self.frame_count).clamp(2, GPU_FONT_COLOR_KEYFRAME_CAPACITY);
        let progress = animation_linear_progress(self.duration_ms, self.iteration, elapsed_ms);
        let progress_permille = progress * 1_000.0;
        let mut upper = 1usize;
        while upper + 1 < count && progress_permille > f32::from(self.frames[upper].offset_permille)
        {
            upper += 1;
        }
        let lower = upper - 1;
        let from = self.frames[lower];
        let to = self.frames[upper];
        let span = f32::from(to.offset_permille.saturating_sub(from.offset_permille)).max(1.0);
        let local = ((progress_permille - f32::from(from.offset_permille)) / span).clamp(0.0, 1.0);
        let local = animation_timed_progress(self.timing, local);
        let base = self.frames[0].rgba;
        GpuFontRgba::new(
            if self.channels.contains(GpuFontColorChannels::RED_BIT) {
                transition_component(from.rgba.r, to.rgba.r, local, true)
            } else {
                base.r
            },
            if self.channels.contains(GpuFontColorChannels::GREEN_BIT) {
                transition_component(from.rgba.g, to.rgba.g, local, true)
            } else {
                base.g
            },
            if self.channels.contains(GpuFontColorChannels::BLUE_BIT) {
                transition_component(from.rgba.b, to.rgba.b, local, true)
            } else {
                base.b
            },
            if self.channels.contains(GpuFontColorChannels::ALPHA_BIT) {
                transition_component(from.rgba.a, to.rgba.a, local, true)
            } else {
                base.a
            },
        )
    }
}

fn animation_linear_progress(
    duration_ms: u32,
    iteration: GpuFontColorIteration,
    elapsed_ms: u64,
) -> f32 {
    let duration = u64::from(duration_ms.max(1));
    match iteration {
        GpuFontColorIteration::Once => elapsed_ms.min(duration) as f32 / duration as f32,
        GpuFontColorIteration::Loop => (elapsed_ms % duration) as f32 / duration as f32,
        GpuFontColorIteration::Alternate => {
            let cycle = elapsed_ms / duration;
            let within = (elapsed_ms % duration) as f32 / duration as f32;
            if cycle.is_multiple_of(2) {
                within
            } else {
                1.0 - within
            }
        }
    }
}

fn animation_timed_progress(timing: GpuFontColorTiming, linear_progress: f32) -> f32 {
    match timing {
        GpuFontColorTiming::Linear => linear_progress,
        GpuFontColorTiming::EaseInOutSine => {
            0.5 - 0.5 * libm::cosf(core::f32::consts::PI * linear_progress)
        }
    }
}

fn transition_component(from: u8, to: u8, progress: f32, selected: bool) -> u8 {
    if !selected {
        return from;
    }
    let value = from as f32 + (to as f32 - from as f32) * progress.clamp(0.0, 1.0);
    libm::roundf(value).clamp(0.0, 255.0) as u8
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GpuFontTextLayout {
    SingleLine,
    Rows,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum GpuFontFace {
    Default = 1,
    NotoSansSc = 2,
    Inconsolata = 3,
}

impl GpuFontFace {
    pub(crate) const fn from_id(id: u32) -> Option<Self> {
        match id {
            1 => Some(Self::Default),
            2 => Some(Self::NotoSansSc),
            3 => Some(Self::Inconsolata),
            _ => None,
        }
    }

    pub(crate) const fn id(self) -> u8 {
        self as u8
    }

    pub(crate) const fn registry_name(self) -> &'static str {
        match self {
            Self::Default => "font",
            Self::NotoSansSc => "noto-sans-sc",
            Self::Inconsolata => "inconsolata",
        }
    }
}

/// Resolve the requested face at the kernel font-service boundary.
/// Embedded fonts warm lazily here if the boot task has not reached them yet.
pub(crate) fn ensure_font_face_available(font: GpuFontFace) -> Result<(), &'static str> {
    match crate::graphics::font::ensure_font_available(font.registry_name()) {
        Ok(true) => Ok(()),
        Ok(false) => Err("font-not-registered"),
        Err(_) => Err("font-warm-failed"),
    }
}

pub(crate) fn font_face_supports_text(font: GpuFontFace, text: &str) -> bool {
    crate::graphics::font::font_supports_text(font.registry_name(), text)
}

impl GpuFontTextLayout {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::SingleLine => "single-line",
            Self::Rows => "rows",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum GpuFontTextRequest<'a> {
    SingleLine(&'a str),
    Rows(&'a [&'a str]),
}

/// One positioned text group in a font job.
///
/// Positions and `font_pixels` share one +X-right/+Y-down coordinate space.
/// Callers choose whether the complete job bounds are stamp-fitted or mapped
/// directly into an explicit scene viewport at submission time.
#[derive(Clone, Copy)]
pub(crate) struct GpuFontJobEntry<'a> {
    pub(crate) text: GpuFontTextRequest<'a>,
    pub(crate) position: [f32; 2],
    /// Requested glyph em size in the same pixel coordinate space as position.
    pub(crate) font_pixels: f32,
    /// Horizontal shear applied once while building the resident triangles.
    /// Positive values lean the top of a glyph toward +X.
    pub(crate) slant: f32,
}

#[derive(Clone, Copy)]
enum GpuFontJobPositioning {
    Origin,
    VisualBoundsCenter,
}

#[derive(Clone, Copy)]
struct GpuFontRasterQuality {
    pixels_per_unit_x: f32,
    pixels_per_unit_y: f32,
}

/// Persistent analytical coverage for one centered font layer.
///
/// The mask is generated from the warmed Skrifa outline stream once, in final
/// physical-pixel coordinates. Color and scene pan remain draw-time inputs, so
/// animation does not regenerate or upload font geometry.
pub(crate) struct GpuFontCoverageMask {
    storage: crate::intel::gpgpu::GpgpuOwnedMask8Surface,
    origin_px: [i32; 2],
}

impl GpuFontCoverageMask {
    pub(crate) const fn surface(&self) -> crate::intel::gpgpu::GpgpuMask8Surface {
        self.storage.surface()
    }

    pub(crate) const fn origin_px(&self) -> [i32; 2] {
        self.origin_px
    }

    pub(crate) fn full_rect(&self) -> crate::intel::gpgpu::GpgpuRect {
        let surface = self.surface();
        crate::intel::gpgpu::GpgpuRect::new(0, 0, surface.width, surface.height)
    }
}

struct PreparedGpuFontCoverageEntry {
    ops: Vec<[u32; 8]>,
    rect: (i32, i32, i32, i32),
    optical_bias_px: f32,
}

const ANALYTICAL_COVERAGE_CURVE_SUBDIVISIONS: u32 = 8;

pub(crate) struct GpuFontJob<'a> {
    pub(crate) entries: &'a [GpuFontJobEntry<'a>],
    pub(crate) font: GpuFontFace,
    pub(crate) native_scale: u32,
}

/// Stable audit identity for one kernel-owned resident font job.
///
/// Tags are static deliberately: resident allocations must have a named kernel
/// owner and purpose rather than inheriting arbitrary input text as identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GpuFontResidencyTag {
    owner: &'static str,
    name: &'static str,
}

impl GpuFontResidencyTag {
    pub(crate) const fn new(owner: &'static str, name: &'static str) -> Self {
        Self { owner, name }
    }

    pub(crate) const fn owner(self) -> &'static str {
        self.owner
    }

    pub(crate) const fn name(self) -> &'static str {
        self.name
    }
}

/// Non-copyable authority lease for a persistent GPU font job.
///
/// The service registry owns the actual DMA pages. This lease can only borrow
/// them for synchronous submission, and dropping it requests an unmap-then-free
/// release. An uncertain GPU retirement quarantines the registry entry instead
/// of freeing memory that hardware could still reference.
pub(crate) struct PersistentGpuFontJob {
    id: u64,
    generation: u64,
    tag: GpuFontResidencyTag,
    released: bool,
}

impl PersistentGpuFontJob {
    pub(crate) const fn id(&self) -> u64 {
        self.id
    }

    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) const fn tag(&self) -> GpuFontResidencyTag {
        self.tag
    }

    pub(crate) fn submit(&self) -> Result<crate::intel::render::RenderJokerResult, &'static str> {
        self.submit_rgba(GPU_FONT_DEFAULT_RGBA)
    }

    /// Draw the resident geometry with a per-submission RGBA value.
    pub(crate) fn submit_rgba(
        &self,
        rgba: GpuFontRgba,
    ) -> Result<crate::intel::render::RenderJokerResult, &'static str> {
        submit_persistent_font_job_rgba(self, rgba)
    }

    fn submit_rgba_readback(
        &self,
        rgba: GpuFontRgba,
    ) -> Result<
        (
            crate::intel::render::RenderJokerResult,
            Option<crate::intel::render::FontRenderTargetReadback>,
        ),
        &'static str,
    > {
        let mut readback = None;
        let render = submit_persistent_font_job_inner(self, None, rgba, Some(&mut readback))?;
        Ok((render, readback))
    }

    /// Reuse the same resident geometry at another supported native size.
    pub(crate) fn submit_at_scale(
        &self,
        native_scale: u32,
    ) -> Result<crate::intel::render::RenderJokerResult, &'static str> {
        self.submit_at_scale_rgba(native_scale, GPU_FONT_DEFAULT_RGBA)
    }

    /// Reuse the same resident geometry with a draw-time size and color.
    pub(crate) fn submit_at_scale_rgba(
        &self,
        native_scale: u32,
        rgba: GpuFontRgba,
    ) -> Result<crate::intel::render::RenderJokerResult, &'static str> {
        submit_persistent_font_job_at_scale_rgba(self, native_scale, rgba)
    }

    pub(crate) fn release(mut self) -> Result<(), &'static str> {
        if self.released {
            return Err("resident-lease-released");
        }
        self.released = true;
        release_persistent_font_job(self.id, self.generation, self.tag)
    }
}

impl Drop for PersistentGpuFontJob {
    fn drop(&mut self) {
        if !self.released {
            self.released = true;
            let _ = release_persistent_font_job(self.id, self.generation, self.tag);
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GpuFontWarmResult {
    pub(crate) cache_hit: bool,
    pub(crate) generation: u64,
    pub(crate) font_name: &'static str,
    pub(crate) font_file: &'static str,
    pub(crate) text: String,
    pub(crate) base_px: f32,
    pub(crate) vertices: usize,
    pub(crate) indices: usize,
    pub(crate) geometry_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GpuFontCacheStatus {
    pub(crate) ready: bool,
    pub(crate) generation: u64,
    pub(crate) warm_requests: u64,
    pub(crate) cache_hits: u64,
    pub(crate) cache_misses: u64,
    pub(crate) build_failures: u64,
    pub(crate) invalidations: u64,
    pub(crate) geometry_bytes: usize,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GpuFontResidentStatus {
    pub(crate) active_jobs: usize,
    pub(crate) resident_bytes: usize,
    pub(crate) quarantined_jobs: usize,
    pub(crate) uploads: u64,
    pub(crate) submit_attempts: u64,
    pub(crate) retired_submits: u64,
    pub(crate) releases: u64,
    pub(crate) release_failures: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GpuFontResidentAuditEntry {
    pub(crate) id: u64,
    pub(crate) generation: u64,
    pub(crate) tag: GpuFontResidencyTag,
    pub(crate) font: GpuFontFace,
    pub(crate) gpu_base: u64,
    pub(crate) resident_bytes: usize,
    pub(crate) entries: usize,
    pub(crate) text_chars: usize,
    pub(crate) rows: usize,
    pub(crate) glyphs: usize,
    pub(crate) submits: u64,
    pub(crate) in_flight: bool,
    pub(crate) quarantined: bool,
}

/// Borrowed, uncolored fill geometry suitable for an indexed GPU draw.
///
/// The coordinates use the cached base size only as a tessellation-quality
/// reference. Consumers should transform them at draw time rather than create
/// a cache entry for every requested font size.
pub(crate) struct GpuFontGeometry<'a> {
    pub(crate) summary: &'a FontTesselSummary,
    pub(crate) vertices: &'a [[f32; 2]],
    pub(crate) indices: &'a [u32],
    pub(crate) bounds: (f32, f32, f32, f32),
}

pub(crate) struct GpuFontTextRender {
    pub(crate) summary: FontTesselSummary,
    pub(crate) render: crate::intel::render::RenderJokerResult,
    pub(crate) layout: GpuFontTextLayout,
    pub(crate) text_chars: usize,
    pub(crate) rows: usize,
}

struct GpuFontTextStamp {
    pub(crate) summary: FontTesselSummary,
    pub(crate) render: crate::intel::render::RenderJokerResult,
    pub(crate) layout: GpuFontTextLayout,
    pub(crate) text_chars: usize,
    pub(crate) rows: usize,
    pub(crate) stamped: bool,
    pub(crate) dst_x: i32,
    pub(crate) dst_y: i32,
    pub(crate) scanout_width: u32,
    pub(crate) scanout_height: u32,
    pub(crate) size_percent: u32,
    pub(crate) source_width: u32,
    pub(crate) source_height: u32,
    pub(crate) render_target_width: u32,
    pub(crate) render_target_height: u32,
    pub(crate) tessellation_tolerance: f32,
    pub(crate) stamp_width: u32,
    pub(crate) stamp_height: u32,
}

pub(crate) struct GpuFontJobRender {
    pub(crate) summaries: Vec<FontTesselSummary>,
    pub(crate) render: crate::intel::render::RenderJokerResult,
    pub(crate) entries: usize,
    pub(crate) text_chars: usize,
    pub(crate) rows: usize,
    pub(crate) glyphs: usize,
    pub(crate) vertices: usize,
    pub(crate) indices: usize,
}

struct BuiltGpuFontJob {
    summaries: Vec<FontTesselSummary>,
    vertices: Vec<[f32; 2]>,
    indices: Vec<u32>,
    bounds: (f32, f32, f32, f32),
    entries: usize,
    text_chars: usize,
    rows: usize,
    glyphs: usize,
}

/// One fully measured font stamp which has not acquired its UI4 destination
/// yet. Keeping the owned geometry here lets the broker allocate only the
/// fitted glyph extent instead of ten full-scanout double buffers.
pub(crate) struct PreparedGpuFontStamp<'a> {
    request: GpuFontTextRequest<'a>,
    font: GpuFontFace,
    rgba: GpuFontRgba,
    built: BuiltGpuFontJob,
    scanout_width: u32,
    scanout_height: u32,
    size_percent: u32,
    stamp_width: u32,
    stamp_height: u32,
}

impl PreparedGpuFontStamp<'_> {
    pub(crate) const fn width(&self) -> u32 {
        self.stamp_width
    }

    pub(crate) const fn height(&self) -> u32 {
        self.stamp_height
    }
}

/// Result of the current GuC font producer writing one exact UI4 allocation.
/// The release fence is deliberately opaque and can only be consumed by the
/// UI4 frame-pool publication contract for this same physical surface.
pub(crate) struct GpuFontUi4Stamp {
    pub(crate) summary: FontTesselSummary,
    pub(crate) render: crate::intel::render::RenderJokerResult,
    pub(crate) text_chars: usize,
    pub(crate) rows: usize,
    pub(crate) scanout_width: u32,
    pub(crate) scanout_height: u32,
    pub(crate) size_percent: u32,
    pub(crate) stamp_width: u32,
    pub(crate) stamp_height: u32,
    pub(crate) producer_path: &'static str,
    pub(crate) release: crate::intel::render::ResidentSceneReleaseFence,
}

/// One wrapped logical document retained independently of its UI4 buffers.
///
/// Geometry is uploaded once into render PPGTT.  A viewport pan subsequently
/// changes only fixed-function translation; neither line layout nor glyph
/// geometry is rebuilt on the interactive path.
pub(crate) struct GpuFontUi4Document {
    mesh: crate::intel::render::ResidentTriangleMesh,
    color: [u8; 4],
    pub(crate) font_name: &'static str,
    pub(crate) text_chars: usize,
    pub(crate) rows: usize,
    pub(crate) glyphs: usize,
    pub(crate) size_percent: u32,
    pub(crate) font_pixels: f32,
    pub(crate) document_width: u32,
    pub(crate) document_height: u32,
}

pub(crate) struct GpuFontUi4DocumentFrame {
    pub(crate) render: crate::intel::render::RenderJokerResult,
    pub(crate) producer_path: &'static str,
    pub(crate) release: crate::intel::render::ResidentSceneReleaseFence,
}

struct CachedGpuFont {
    generation: u64,
    mesh: FontTesselMesh,
}

struct ResidentGpuFontJobRecord {
    id: u64,
    generation: u64,
    tag: GpuFontResidencyTag,
    font: GpuFontFace,
    mesh: crate::intel::render::ResidentFontMesh,
    native_scale: u32,
    entries: usize,
    text_chars: usize,
    rows: usize,
    glyphs: usize,
    submits: u64,
    in_flight: bool,
    quarantined: bool,
}

struct KernelGpuFontService {
    default_font: Option<Arc<CachedGpuFont>>,
    generation: u64,
    warm_requests: u64,
    cache_hits: u64,
    cache_misses: u64,
    build_failures: u64,
    invalidations: u64,
    resident_generation: u64,
    next_resident_id: u64,
    resident_jobs: Vec<ResidentGpuFontJobRecord>,
    resident_uploads: u64,
    resident_submit_attempts: u64,
    resident_retired_submits: u64,
    resident_releases: u64,
    resident_release_failures: u64,
}

impl KernelGpuFontService {
    const fn new() -> Self {
        Self {
            default_font: None,
            generation: 0,
            warm_requests: 0,
            cache_hits: 0,
            cache_misses: 0,
            build_failures: 0,
            invalidations: 0,
            resident_generation: 0,
            next_resident_id: 1,
            resident_jobs: Vec::new(),
            resident_uploads: 0,
            resident_submit_attempts: 0,
            resident_retired_submits: 0,
            resident_releases: 0,
            resident_release_failures: 0,
        }
    }
}

static GPU_FONT_SERVICE: Mutex<KernelGpuFontService> = Mutex::new(KernelGpuFontService::new());
static TRANSIENT_FONT_STAMP_READBACK: Mutex<Vec<u8>> = Mutex::new(Vec::new());

struct PersistentGpuFontAnimation {
    lease: PersistentGpuFontJob,
    color_program: GpuFontColorProgram,
    started_ms: u64,
    last_submitted: Option<GpuFontRgba>,
    engine_frame_requests: u64,
    submitted_frames: u64,
    failures: u64,
    halted: bool,
}

struct PersistentGpuFontGridCell {
    lease: PersistentGpuFontJob,
    color_program: GpuFontColorProgram,
    started_ms: u64,
    last_submitted: Option<GpuFontRgba>,
    readback: Option<crate::intel::render::FontRenderTargetReadback>,
    failures: u64,
    halted: bool,
}

struct PersistentGpuFontGrid {
    cells: Vec<PersistentGpuFontGridCell>,
    engine_frame_requests: u64,
    presented_frames: u64,
    exact_color_check: bool,
    color_proof_pixels: u64,
    color_proof_mismatches: u64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PersistentGpuFontAnimationStatus {
    pub(crate) id: u64,
    pub(crate) generation: u64,
    pub(crate) color_program: GpuFontColorProgram,
    pub(crate) elapsed_ms: u64,
    pub(crate) last_submitted: Option<GpuFontRgba>,
    pub(crate) engine_frame_requests: u64,
    pub(crate) submitted_frames: u64,
    pub(crate) failures: u64,
    pub(crate) halted: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PersistentGpuFontGridStatus {
    pub(crate) cells: usize,
    pub(crate) engine_frame_requests: u64,
    pub(crate) presented_frames: u64,
    pub(crate) failures: u64,
    pub(crate) halted_cells: usize,
    pub(crate) exact_color_check: bool,
    pub(crate) color_proof_pixels: u64,
    pub(crate) color_proof_mismatches: u64,
}

static PERSISTENT_GPU_FONT_ANIMATION: Mutex<Option<PersistentGpuFontAnimation>> = Mutex::new(None);
static PERSISTENT_GPU_FONT_GRID: Mutex<Option<PersistentGpuFontGrid>> = Mutex::new(None);

/// Return the single statically attributable shell-animation identity.
///
/// Shell persistence is replace-in-place at the service boundary: the active
/// lease must retire and release before this tag can be acquired again. If a
/// draw has uncertain retirement, the one record remains quarantined and the
/// slot stays unavailable instead of accumulating replacement allocations.
pub(crate) fn next_persistent_font_animation_tag() -> Result<GpuFontResidencyTag, &'static str> {
    const TAG: GpuFontResidencyTag = GpuFontResidencyTag::new("shell2", "font-persist-0");

    let service = GPU_FONT_SERVICE.lock();
    if service.resident_jobs.iter().any(|record| record.tag == TAG) {
        Err("resident-animation-slot-in-use")
    } else {
        Ok(TAG)
    }
}

pub(crate) fn persistent_font_demo_grid_tag(
    index: usize,
) -> Result<GpuFontResidencyTag, &'static str> {
    const TAGS: [GpuFontResidencyTag; 9] = [
        GpuFontResidencyTag::new("shell2", "font-demo-grid-1"),
        GpuFontResidencyTag::new("shell2", "font-demo-grid-2"),
        GpuFontResidencyTag::new("shell2", "font-demo-grid-3"),
        GpuFontResidencyTag::new("shell2", "font-demo-grid-4"),
        GpuFontResidencyTag::new("shell2", "font-demo-grid-5"),
        GpuFontResidencyTag::new("shell2", "font-demo-grid-6"),
        GpuFontResidencyTag::new("shell2", "font-demo-grid-7"),
        GpuFontResidencyTag::new("shell2", "font-demo-grid-8"),
        GpuFontResidencyTag::new("shell2", "font-demo-grid-9"),
    ];
    let tag = *TAGS.get(index).ok_or("font-demo-grid-index")?;
    let service = GPU_FONT_SERVICE.lock();
    if service.resident_jobs.iter().any(|record| record.tag == tag) {
        Err("resident-animation-slot-in-use")
    } else {
        Ok(tag)
    }
}

pub(crate) fn install_persistent_font_demo_grid(
    jobs: Vec<(PersistentGpuFontJob, GpuFontColorProgram)>,
    exact_color_check: bool,
) -> Result<(), &'static str> {
    if jobs.len() != 9 {
        return Err("font-demo-grid-count");
    }
    if jobs.iter().any(|(_, program)| {
        matches!(program, GpuFontColorProgram::Transition(transition) if transition.duration_ms == 0)
    }) {
        return Err("font-color-duration-zero");
    }
    let started_ms = Instant::now().as_millis();
    let cells = jobs
        .into_iter()
        .map(|(lease, color_program)| PersistentGpuFontGridCell {
            lease,
            color_program,
            started_ms,
            last_submitted: None,
            readback: None,
            failures: 0,
            halted: false,
        })
        .collect();
    let old = PERSISTENT_GPU_FONT_GRID
        .lock()
        .replace(PersistentGpuFontGrid {
            cells,
            engine_frame_requests: 0,
            presented_frames: 0,
            exact_color_check,
            color_proof_pixels: 0,
            color_proof_mismatches: 0,
        });
    drop(old);
    crate::log_info!(
        target: "render";
        "intel/gpu-font: demo-grid-install cells=9 layout=3x3 policy=elapsed-time-sampled-per-engine-frame overlay_commits=one-per-grid-frame exact_color_check={} authority=kernel-service-owned\n",
        exact_color_check as u8,
    );
    Ok(())
}

/// Atomically replace the active animation after its new resident mesh exists.
/// The old lease is dropped outside the animation lock and releases its PPGTT
/// mapping through the normal retirement-aware path.
pub(crate) fn install_persistent_font_animation(
    lease: PersistentGpuFontJob,
    color_program: GpuFontColorProgram,
) -> Result<PersistentGpuFontAnimationStatus, &'static str> {
    if let GpuFontColorProgram::Transition(transition) = color_program
        && transition.duration_ms == 0
    {
        return Err("font-color-duration-zero");
    }
    let id = lease.id();
    let generation = lease.generation();
    let replacement = PersistentGpuFontAnimation {
        lease,
        color_program,
        started_ms: Instant::now().as_millis(),
        last_submitted: None,
        engine_frame_requests: 0,
        submitted_frames: 0,
        failures: 0,
        halted: false,
    };
    let old = {
        let mut active = PERSISTENT_GPU_FONT_ANIMATION.lock();
        active.replace(replacement)
    };
    let replaced = old.is_some();
    drop(old);
    crate::log_info!(
        target: "render";
        "intel/gpu-font: animation-install id={} generation={} color_program={} policy=elapsed-time-sampled-per-engine-frame replaced={} authority=kernel-service-owned\n",
        id,
        generation,
        color_program.name(),
        replaced as u8,
    );
    persistent_font_animation_status().ok_or("font-animation-install")
}

/// Replace only the volatile color contract of the active resident geometry.
/// No outline parsing, tessellation, allocation, mapping, or VB/IB upload is
/// performed by this operation.
pub(crate) fn set_persistent_font_color_program(
    color_program: GpuFontColorProgram,
) -> Result<PersistentGpuFontAnimationStatus, &'static str> {
    if let GpuFontColorProgram::Transition(transition) = color_program
        && transition.duration_ms == 0
    {
        return Err("font-color-duration-zero");
    }
    let (id, generation) = {
        let mut active = PERSISTENT_GPU_FONT_ANIMATION.lock();
        let animation = active.as_mut().ok_or("font-animation-inactive")?;
        if animation.halted {
            return Err("font-animation-halted");
        }
        animation.color_program = color_program;
        animation.started_ms = Instant::now().as_millis();
        (animation.lease.id(), animation.lease.generation())
    };
    crate::log_info!(
        target: "render";
        "intel/gpu-font: animation-update id={} generation={} color_program={} geometry_uploads=0 authority=kernel-service-owned\n",
        id,
        generation,
        color_program.name(),
    );
    persistent_font_animation_status().ok_or("font-animation-update")
}

pub(crate) fn stop_persistent_font_animation() -> Result<bool, &'static str> {
    let old = PERSISTENT_GPU_FONT_ANIMATION.lock().take();
    let mut grid = PERSISTENT_GPU_FONT_GRID.lock().take();
    if old.is_none() && grid.is_none() {
        return Ok(false);
    }
    let mut first_error = None;
    if let Some(animation) = old {
        let id = animation.lease.id();
        let generation = animation.lease.generation();
        match animation.lease.release() {
            Ok(()) => crate::log_info!(
                target: "render";
                "intel/gpu-font: animation-stop id={} generation={} authority=kernel-service->unmapped->released\n",
                id,
                generation,
            ),
            Err(reason) => {
                first_error = Some(reason);
                crate::log_error!(
                    target: "render";
                    "intel/gpu-font: animation-stop id={} generation={} released=0 reason={} authority=gpu-quarantine\n",
                    id,
                    generation,
                    reason,
                );
            }
        }
    }
    if let Some(grid) = grid.as_mut() {
        for cell in grid.cells.drain(..) {
            if let Err(reason) = cell.lease.release()
                && first_error.is_none()
            {
                first_error = Some(reason);
            }
        }
        crate::log_info!(
            target: "render";
            "intel/gpu-font: demo-grid-stop cells=9 authority=kernel-service->unmapped->released-or-quarantined\n",
        );
    }
    if let Some(reason) = first_error {
        Err(reason)
    } else {
        Ok(true)
    }
}

pub(crate) fn persistent_font_animation_status() -> Option<PersistentGpuFontAnimationStatus> {
    let active = PERSISTENT_GPU_FONT_ANIMATION.lock();
    let animation = active.as_ref()?;
    Some(PersistentGpuFontAnimationStatus {
        id: animation.lease.id(),
        generation: animation.lease.generation(),
        color_program: animation.color_program,
        elapsed_ms: Instant::now()
            .as_millis()
            .saturating_sub(animation.started_ms),
        last_submitted: animation.last_submitted,
        engine_frame_requests: animation.engine_frame_requests,
        submitted_frames: animation.submitted_frames,
        failures: animation.failures,
        halted: animation.halted,
    })
}

pub(crate) fn persistent_font_demo_grid_status() -> Option<PersistentGpuFontGridStatus> {
    let active = PERSISTENT_GPU_FONT_GRID.lock();
    let grid = active.as_ref()?;
    Some(PersistentGpuFontGridStatus {
        cells: grid.cells.len(),
        engine_frame_requests: grid.engine_frame_requests,
        presented_frames: grid.presented_frames,
        failures: grid.cells.iter().map(|cell| cell.failures).sum(),
        halted_cells: grid.cells.iter().filter(|cell| cell.halted).count(),
        exact_color_check: grid.exact_color_check,
        color_proof_pixels: grid.color_proof_pixels,
        color_proof_mismatches: grid.color_proof_mismatches,
    })
}

/// Sample elapsed time and submit at most one color for one render-engine
/// frame. Animation time is monotonic rather than submission-count based, so a
/// slow draw reduces sampling frequency without slowing the requested effect.
pub(crate) fn submit_persistent_font_animation_engine_frame() {
    if submit_persistent_font_demo_grid_engine_frame() {
        return;
    }
    let mut active = PERSISTENT_GPU_FONT_ANIMATION.lock();
    let Some(animation) = active.as_mut() else {
        return;
    };
    animation.engine_frame_requests = animation.engine_frame_requests.saturating_add(1);
    if animation.halted {
        return;
    }
    let elapsed_ms = Instant::now()
        .as_millis()
        .saturating_sub(animation.started_ms);
    let rgba = animation.color_program.sample(elapsed_ms);
    if animation.last_submitted == Some(rgba) {
        return;
    }

    match animation.lease.submit_rgba(rgba) {
        Ok(render) if render.completed => {
            animation.last_submitted = Some(rgba);
            animation.submitted_frames = animation.submitted_frames.saturating_add(1);
            crate::log_info!(
                target: "render";
                "intel/gpu-font: animation-frame id={} frame={} submitted={} elapsed_ms={} program={} rgba=[{},{},{},{}] geometry_uploads=0\n",
                animation.lease.id(),
                animation.engine_frame_requests,
                animation.submitted_frames,
                elapsed_ms,
                animation.color_program.name(),
                rgba.r,
                rgba.g,
                rgba.b,
                rgba.a,
            );
        }
        Ok(_) => {
            animation.failures = animation.failures.saturating_add(1);
            animation.halted = true;
            crate::log_error!(
                target: "render";
                "intel/gpu-font: animation-frame halted=1 id={} reason=retirement-uncertain failures={} program_retained={}\n",
                animation.lease.id(),
                animation.failures,
                animation.color_program.name(),
            );
        }
        Err(reason) => {
            animation.failures = animation.failures.saturating_add(1);
            if animation.failures <= 4 || animation.failures.is_multiple_of(60) {
                crate::log_error!(
                    target: "render";
                    "intel/gpu-font: animation-frame submitted=0 id={} reason={} failures={} program_retained={}\n",
                    animation.lease.id(),
                    reason,
                    animation.failures,
                    animation.color_program.name(),
                );
            }
        }
    }
}

fn submit_persistent_font_demo_grid_engine_frame() -> bool {
    let mut active = PERSISTENT_GPU_FONT_GRID.lock();
    let Some(grid) = active.as_mut() else {
        return false;
    };
    grid.engine_frame_requests = grid.engine_frame_requests.saturating_add(1);
    let now_ms = Instant::now().as_millis();
    let mut changed = false;
    let exact_color_check = grid.exact_color_check;
    let mut proof_pixels = 0u64;
    let mut proof_mismatches = 0u64;
    for (cell_index, cell) in grid.cells.iter_mut().enumerate() {
        if cell.halted {
            continue;
        }
        let elapsed_ms = now_ms.saturating_sub(cell.started_ms);
        let rgba = cell.color_program.sample(elapsed_ms);
        if cell.last_submitted == Some(rgba) && cell.readback.is_some() {
            continue;
        }
        match cell.lease.submit_rgba_readback(rgba) {
            Ok((render, Some(captured))) if render.completed => {
                if exact_color_check {
                    let (pixels, mismatches) = exact_font_readback_color(&captured, rgba);
                    proof_pixels = proof_pixels.saturating_add(pixels);
                    proof_mismatches = proof_mismatches.saturating_add(mismatches);
                    crate::log_info!(
                        target: "render";
                        "intel/gpu-font: color-contract-rt-proof cell={} requested={:02X}{:02X}{:02X}{:02X} written_pixels={} mismatches={} exact={} stage=post-ps-linear-rgba8-readback\n",
                        cell_index + 1,
                        rgba.r,
                        rgba.g,
                        rgba.b,
                        rgba.a,
                        pixels,
                        mismatches,
                        (pixels != 0 && mismatches == 0) as u8,
                    );
                }
                cell.last_submitted = Some(rgba);
                cell.readback = Some(captured);
                changed = true;
            }
            Ok((render, _)) if !render.completed => {
                cell.failures = cell.failures.saturating_add(1);
                cell.halted = true;
            }
            Ok(_) => {
                cell.failures = cell.failures.saturating_add(1);
                cell.halted = true;
                crate::log_error!(
                    target: "render";
                    "intel/gpu-font: demo-grid-cell halted=1 id={} reason=completed-without-readback failures={}\n",
                    cell.lease.id(),
                    cell.failures,
                );
            }
            Err(reason) => {
                cell.failures = cell.failures.saturating_add(1);
                if cell.failures <= 4 || cell.failures.is_multiple_of(60) {
                    crate::log_error!(
                        target: "render";
                        "intel/gpu-font: demo-grid-cell submitted=0 id={} reason={} failures={}\n",
                        cell.lease.id(),
                        reason,
                        cell.failures,
                    );
                }
            }
        }
    }
    grid.color_proof_pixels = grid.color_proof_pixels.saturating_add(proof_pixels);
    grid.color_proof_mismatches = grid.color_proof_mismatches.saturating_add(proof_mismatches);
    if !changed || grid.cells.iter().any(|cell| cell.readback.is_none()) {
        return true;
    }

    let Some((scanout_w, scanout_h)) = crate::intel::active_scanout_dimensions() else {
        return true;
    };
    let cell_w = scanout_w / 3;
    let cell_h = scanout_h / 3;
    if cell_w == 0 || cell_h == 0 {
        return true;
    }
    let mut tiles = Vec::with_capacity(9);
    for (index, cell) in grid.cells.iter().enumerate() {
        let captured = cell.readback.as_ref().expect("checked above");
        let column = index as u32 % 3;
        let row = index as u32 / 3;
        let x = column
            .saturating_mul(cell_w)
            .saturating_add(cell_w.saturating_sub(captured.width) / 2);
        let y = row
            .saturating_mul(cell_h)
            .saturating_add(cell_h.saturating_sub(captured.height) / 2);
        tiles.push(crate::intel::display::RgbaOverlayTile {
            x,
            y,
            width: captured.width,
            height: captured.height,
            source_width: captured.width,
            source_height: captured.height,
            pitch_bytes: captured.width as usize * 4,
            pixels: captured.pixels.as_slice(),
            gpgpu_surface: None,
            gpgpu_scanout_cache: false,
            opacity: u8::MAX,
            known_opaque: false,
            expected_rgba: if exact_color_check {
                cell.last_submitted
            } else {
                None
            },
        });
    }
    if crate::intel::display::present_rgba_overlay_tiles(tiles.as_slice(), "font-persist-demo-grid")
    {
        grid.presented_frames = grid.presented_frames.saturating_add(1);
        if grid.presented_frames <= 8 || grid.presented_frames.is_multiple_of(60) {
            crate::log_info!(
                target: "render";
                "intel/gpu-font: demo-grid-frame frame={} engine_requests={} cells=9 layout=3x3 scanout={}x{} overlay_commits=1 clock=monotonic-elapsed geometry_uploads=0\n",
                grid.presented_frames,
                grid.engine_frame_requests,
                scanout_w,
                scanout_h,
            );
        }
    }
    true
}

fn exact_font_readback_color(
    captured: &crate::intel::render::FontRenderTargetReadback,
    expected: GpuFontRgba,
) -> (u64, u64) {
    let expected = [expected.r, expected.g, expected.b, expected.a];
    let mut written = 0u64;
    let mut mismatches = 0u64;
    for pixel in captured.pixels.chunks_exact(4) {
        if pixel[3] == 0 {
            continue;
        }
        written = written.saturating_add(1);
        if pixel != expected {
            mismatches = mismatches.saturating_add(1);
        }
    }
    (written, mismatches)
}

fn acquire_default_font() -> Result<(Arc<CachedGpuFont>, bool), &'static str> {
    // Keep the lock during the first build. It is a one-time boot operation,
    // and doing so guarantees that concurrent first users cannot tessellate the
    // same font twice. The returned Arc lets all later users drop the lock.
    let mut service = GPU_FONT_SERVICE.lock();
    service.warm_requests = service.warm_requests.saturating_add(1);
    if let Some(cached) = service.default_font.as_ref().map(Arc::clone) {
        service.cache_hits = service.cache_hits.saturating_add(1);
        return Ok((cached, true));
    }

    service.cache_misses = service.cache_misses.saturating_add(1);
    let mesh = crate::graphics::font::tessellate_default_text_mesh();
    if mesh.summary.status != "ok"
        || mesh.summary.tessellate_failures != 0
        || mesh.vertices.is_empty()
        || mesh.indices.is_empty()
        || !mesh.indices.len().is_multiple_of(3)
    {
        service.build_failures = service.build_failures.saturating_add(1);
        crate::log_error!(
            target: "render";
            "intel/gpu-font: warm failed reason={} font={} file={} text=\"{}\"\n",
            mesh.summary.reason,
            mesh.summary.font_name,
            mesh.summary.font_file,
            mesh.summary.text,
        );
        return Err(mesh.summary.reason);
    }

    service.generation = service.generation.saturating_add(1).max(1);
    let cached = Arc::new(CachedGpuFont {
        generation: service.generation,
        mesh,
    });
    crate::log_info!(
        target: "render";
        "intel/gpu-font: warm ok=1 cache_hit=0 generation={} font={} file={} text=\"{}\" base_px={} vertices={} indices={} geometry_bytes={} coverage=uncolored-vector-fill size_policy=draw-time\n",
        cached.generation,
        cached.mesh.summary.font_name,
        cached.mesh.summary.font_file,
        cached.mesh.summary.text,
        cached.mesh.summary.px_size as u32,
        cached.mesh.summary.vertices,
        cached.mesh.summary.indices,
        cached.mesh.summary.geometry_bytes,
    );
    service.default_font = Some(Arc::clone(&cached));
    Ok((cached, false))
}

/// Warm the embedded font and its default GPU-ready mesh exactly once.
///
/// This is safe to call both during boot and lazily from a first consumer.
pub(crate) fn warm_default_font_once() -> Result<GpuFontWarmResult, &'static str> {
    let (cached, cache_hit) = acquire_default_font()?;
    let summary = cached.mesh.summary.clone();
    Ok(GpuFontWarmResult {
        cache_hit,
        generation: cached.generation,
        font_name: summary.font_name,
        font_file: summary.font_file,
        text: summary.text,
        base_px: summary.px_size,
        vertices: summary.vertices,
        indices: summary.indices,
        geometry_bytes: summary.geometry_bytes,
    })
}

/// Use the cached base mesh without copying its vertex or index buffers.
pub(crate) fn with_default_font_geometry<R>(
    use_geometry: impl FnOnce(GpuFontGeometry<'_>) -> R,
) -> Result<R, &'static str> {
    let (cached, _) = acquire_default_font()?;
    let summary = &cached.mesh.summary;
    let bounds = (summary.min_x, summary.min_y, summary.max_x, summary.max_y);
    Ok(use_geometry(GpuFontGeometry {
        summary,
        vertices: cached.mesh.vertices.as_slice(),
        indices: cached.mesh.indices.as_slice(),
        bounds,
    }))
}

/// Convenient current consumer: draw the cached geometry at a native size.
///
/// The scale changes the render target and viewport, not the cached geometry.
/// Color remains a render-state concern and is deliberately absent here.
pub(crate) fn render_default_font(
    native_scale: u32,
) -> Result<crate::intel::render::RenderJokerResult, &'static str> {
    with_default_font_geometry(|geometry| {
        crate::intel::render::submit_font_mesh_once_scaled(
            geometry.vertices,
            geometry.indices,
            geometry.bounds,
            native_scale,
        )
    })?
}

/// Tessellate one caller-provided string from the warmed outline registry,
/// submit it immediately, and drop the invocation-specific mesh afterwards.
pub(crate) fn render_text_once(
    request: GpuFontTextRequest<'_>,
    native_scale: u32,
) -> Result<GpuFontTextRender, &'static str> {
    render_text_once_with_font(request, GpuFontFace::Default, native_scale)
}

pub(crate) fn render_text_once_with_font(
    request: GpuFontTextRequest<'_>,
    font: GpuFontFace,
    native_scale: u32,
) -> Result<GpuFontTextRender, &'static str> {
    let layout = match request {
        GpuFontTextRequest::SingleLine(_) => GpuFontTextLayout::SingleLine,
        GpuFontTextRequest::Rows(_) => GpuFontTextLayout::Rows,
    };
    let entry = GpuFontJobEntry {
        text: request,
        position: [0.0, 0.0],
        font_pixels: crate::graphics::font::FONT_TESSEL_BASE_PX,
        slant: 0.0,
    };
    let job = render_font_job_once(GpuFontJob {
        entries: core::slice::from_ref(&entry),
        font,
        native_scale,
    })?;
    let mut summaries = job.summaries;
    let summary = summaries.pop().ok_or("font-job-summary")?;
    Ok(GpuFontTextRender {
        summary,
        render: job.render,
        layout,
        text_chars: job.text_chars,
        rows: job.rows,
    })
}

fn render_analytical_font_stamp_readback(
    request: GpuFontTextRequest<'_>,
    font: GpuFontFace,
    bounds: (f32, f32, f32, f32),
    target_width: u32,
    target_height: u32,
    padding_pixels: u32,
) -> Option<(crate::intel::render::RenderJokerResult, crate::intel::render::FontRenderTargetReadback)>
{
    let GpuFontTextRequest::SingleLine(_) = request else {
        return None;
    };
    let width = (bounds.2 - bounds.0).max(1.0);
    let height = (bounds.3 - bounds.1).max(1.0);
    let padding_pixels = padding_pixels
        .min(target_width.saturating_sub(1) / 2)
        .min(target_height.saturating_sub(1) / 2);
    let content_width = target_width
        .saturating_sub(padding_pixels.saturating_mul(2))
        .max(1);
    let content_height = target_height
        .saturating_sub(padding_pixels.saturating_mul(2))
        .max(1);
    let pixel_scale = (content_width as f32 / width).min(content_height as f32 / height);
    let entry = GpuFontJobEntry {
        text: request,
        position: [target_width as f32 * 0.5, target_height as f32 * 0.5],
        font_pixels: crate::graphics::font::FONT_TESSEL_BASE_PX * pixel_scale,
        slant: 0.0,
    };
    let coverage = match create_gpu_font_centered_coverage_mask_at_raster(
        core::slice::from_ref(&entry),
        font,
        target_width,
        target_height,
        target_width,
        target_height,
    ) {
        Ok(coverage) => coverage,
        Err(reason) => {
            crate::log_warn!(
                target: "render";
                "intel/gpu-font: stamp analytical coverage unavailable font={} target={}x{} ppem={:.2} reason={} action=resident-triangle-fallback\n",
                font.registry_name(),
                target_width,
                target_height,
                entry.font_pixels,
                reason,
            );
            return None;
        }
    };
    let origin = coverage.origin_px();
    let draw = crate::intel::render::ResidentSceneCoverageDraw {
        mask: coverage.surface(),
        mask_rect: coverage.full_rect(),
        dst_xy: crate::intel::gpgpu::GpgpuPoint::new(origin[0], origin[1]),
        color_rgba: u32::from_le_bytes([u8::MAX; 4]),
    };
    let captured = match crate::intel::render::capture_resident_triangle_scene_frame_premultiplied_at_extent_msaa4_with_coverage(
        &[],
        core::slice::from_ref(&draw),
        Some([0, 0, 0, 0]),
        target_width,
        target_height,
        false,
    ) {
        Ok(captured)
            if captured.completed_draws == 1
                && captured.requested_draws == 1
                && captured.rgba.is_some() => captured,
        _ => {
            crate::log_warn!(
                target: "render";
                "intel/gpu-font: stamp analytical composite unavailable font={} target={}x{} action=resident-triangle-fallback\n",
                font.registry_name(),
                target_width,
                target_height,
            );
            return None;
        }
    };
    Some((
        crate::intel::render::RenderJokerResult {
            variant: "gpgpu-font-outline-coverage-r8",
            submit_name: "font-stamp-analytical",
            target: "font-stamp-readback",
            completed: true,
            vs_counter: false,
            ps_state_marker: false,
            raster_packet: false,
            clip_counter: false,
            ps_observed: false,
        },
        crate::intel::render::FontRenderTargetReadback {
            width: target_width,
            height: target_height,
            pixels: captured.rgba.expect("analytical stamp readback checked"),
        },
    ))
}

fn composite_font_stamp_readback(
    readback: &mut crate::intel::render::FontRenderTargetReadback,
    rgba: GpuFontRgba,
    _dst_x: i32,
    _dst_y: i32,
    _stamp_width: u32,
    _stamp_height: u32,
) -> (bool, u32, u32) {
    let mut source_width = 0;
    let mut source_height = 0;
    let stamped =
        visible_font_target_bounds(readback.pixels.as_slice(), readback.width, readback.height)
            .is_some_and(|(_, _, visible_width, visible_height)| {
                source_width = visible_width;
                source_height = visible_height;
                recolor_transient_font_target(readback.pixels.as_mut_slice(), rgba);
                true
            });
    (stamped, source_width, source_height)
}

/// Measure and tessellate one stamp before its UI4 slot acquires a back
/// buffer. The returned object owns the invocation geometry and borrows only
/// the command text for the duration of the synchronous presentation call.
pub(crate) fn prepare_text_stamp_for_ui4<'a>(
    request: GpuFontTextRequest<'a>,
    font: GpuFontFace,
    size_percent: u32,
    rgba: GpuFontRgba,
) -> Result<PreparedGpuFontStamp<'a>, &'static str> {
    if !(MIN_FONT_STAMP_SIZE_PERCENT..=MAX_FONT_STAMP_SIZE_PERCENT).contains(&size_percent) {
        return Err("font-size-percent-range-1-to-100");
    }
    let (scanout_width, scanout_height) =
        crate::intel::active_scanout_dimensions().ok_or("no-active-scanout-dimensions")?;
    let entry = GpuFontJobEntry {
        text: request,
        position: [0.0, 0.0],
        font_pixels: crate::graphics::font::FONT_TESSEL_BASE_PX,
        slant: 0.0,
    };
    let built = build_font_job_mesh(core::slice::from_ref(&entry), font)?;
    let source_width = libm::ceilf((built.bounds.2 - built.bounds.0).max(1.0)) as u32;
    let source_height = libm::ceilf((built.bounds.3 - built.bounds.1).max(1.0)) as u32;
    let (stamp_width, stamp_height) = fit_font_stamp_to_scanout(
        source_width,
        source_height,
        scanout_width,
        scanout_height,
        size_percent,
    );
    Ok(PreparedGpuFontStamp {
        request,
        font,
        rgba,
        built,
        scanout_width,
        scanout_height,
        size_percent,
        stamp_width,
        stamp_height,
    })
}

/// Render a prepared font stamp into the exact GPU address owned by a UI4
/// write lease. Both the analytical-mask path and the resident-triangle
/// fallback use the current GuC clients; neither invokes the legacy execlist
/// helpers or reads pixels back through the CPU.
pub(crate) fn render_prepared_text_stamp_to_ui4(
    mut prepared: PreparedGpuFontStamp<'_>,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
) -> Result<GpuFontUi4Stamp, &'static str> {
    if !destination.is_valid()
        || destination.width != prepared.stamp_width
        || destination.height != prepared.stamp_height
    {
        return Err("font-ui4-destination-shape");
    }

    let alpha = prepared.rgba.a;
    let premultiply =
        |channel: u8| -> u8 { ((u16::from(channel) * u16::from(alpha) + 127) / 255) as u8 };
    let color = [
        premultiply(prepared.rgba.r),
        premultiply(prepared.rgba.g),
        premultiply(prepared.rgba.b),
        alpha,
    ];

    let target_width = prepared.stamp_width;
    let target_height = prepared.stamp_height;
    let mesh_width = (prepared.built.bounds.2 - prepared.built.bounds.0).max(1.0);
    let mesh_height = (prepared.built.bounds.3 - prepared.built.bounds.1).max(1.0);
    let padding = NATIVE_FONT_STAMP_PADDING_PIXELS
        .min(target_width.saturating_sub(1) / 2)
        .min(target_height.saturating_sub(1) / 2);
    let content_width = target_width
        .saturating_sub(padding.saturating_mul(2))
        .max(1);
    let content_height = target_height
        .saturating_sub(padding.saturating_mul(2))
        .max(1);
    let pixel_scale = (content_width as f32 / mesh_width).min(content_height as f32 / mesh_height);
    let analytical_entry = GpuFontJobEntry {
        text: prepared.request,
        position: [target_width as f32 * 0.5, target_height as f32 * 0.5],
        font_pixels: crate::graphics::font::FONT_TESSEL_BASE_PX * pixel_scale,
        slant: 0.0,
    };

    let (frame, producer_path) = match create_gpu_font_centered_coverage_mask_at_raster(
        core::slice::from_ref(&analytical_entry),
        prepared.font,
        target_width,
        target_height,
        target_width,
        target_height,
    ) {
        Ok(coverage) => {
            let origin = coverage.origin_px();
            let draw = crate::intel::render::ResidentSceneCoverageDraw {
                mask: coverage.surface(),
                mask_rect: coverage.full_rect(),
                dst_xy: crate::intel::gpgpu::GpgpuPoint::new(origin[0], origin[1]),
                color_rgba: u32::from_le_bytes(color),
            };
            let frame = crate::intel::render::render_resident_triangle_scene_frame_premultiplied_msaa4_with_coverage_to_surface(
                &[],
                core::slice::from_ref(&draw),
                Some([0, 0, 0, 0]),
                destination,
                false,
            )?;
            (frame, "skrifa-gpgpu-r8")
        }
        Err(reason) => {
            if reason == "font-coverage-retirement-uncertain" {
                crate::log_error!(
                    target: "render";
                    "intel/gpu-font: ui4 analytical retirement uncertain font={} target={}x{} action=fail-closed-no-triangle-submit\n",
                    prepared.font.registry_name(),
                    target_width,
                    target_height,
                );
                return Err(reason);
            }
            crate::log_warn!(
                target: "render";
                "intel/gpu-font: ui4 analytical unavailable font={} target={}x{} reason={} action=resident-triangle-fallback\n",
                prepared.font.registry_name(),
                target_width,
                target_height,
                reason,
            );
            let mesh = crate::intel::render::create_resident_font_mesh(
                prepared.built.vertices.as_slice(),
                prepared.built.indices.as_slice(),
                prepared.built.bounds,
            )?;
            let draw = crate::intel::render::ResidentSceneDraw {
                mesh: &mesh,
                rgba: color,
                viewport_translation_px: [0.0, 0.0],
            };
            let frame = match crate::intel::render::render_resident_triangle_scene_frame_premultiplied_msaa4_with_coverage_to_surface(
                core::slice::from_ref(&draw),
                &[],
                Some([0, 0, 0, 0]),
                destination,
                false,
            ) {
                Ok(frame) => frame,
                Err(error) => {
                    // The submit may have crossed the hardware boundary. Do
                    // not free its resident pages without an exact retirement
                    // proof; the caller quarantines the destination lease too.
                    crate::log_error!(
                        target: "render";
                        "intel/gpu-font: ui4 triangle submit failed target={}x{} reason={} action=quarantine-resident-mesh+destination\n",
                        target_width,
                        target_height,
                        error,
                    );
                    return Err(error);
                }
            };
            if !frame.frame_complete || frame.release_fence.is_none() {
                return Err("font-ui4-triangle-incomplete");
            }
            if !crate::intel::render::release_resident_font_mesh(&mesh) {
                return Err("font-ui4-triangle-release");
            }
            (frame, "resident-triangles")
        }
    };

    if frame.completed_draws != frame.requested_draws || !frame.frame_complete {
        return Err("font-ui4-render-incomplete");
    }
    let release = frame
        .release_fence
        .filter(|release| release.matches(destination.phys, destination.bytes))
        .ok_or("font-ui4-release-fence")?;
    let summary = prepared.built.summaries.pop().ok_or("font-job-summary")?;
    let render = crate::intel::render::RenderJokerResult {
        variant: producer_path,
        submit_name: "font-stamp-ui4",
        target: "ui4-font-frame",
        completed: true,
        vs_counter: false,
        ps_state_marker: false,
        raster_packet: false,
        clip_counter: false,
        ps_observed: false,
    };
    crate::log_info!(
        target: "render";
        "intel/gpu-font: ui4-stamp-ready font={} text_chars={} rows={} target={}x{} path={} release_sequence={} producer=guc exact_surface=1 cpu_readback=0 cpu_frame_copy=0 legacy_execlist=0\n",
        prepared.font.registry_name(),
        prepared.built.text_chars,
        prepared.built.rows,
        target_width,
        target_height,
        producer_path,
        release.sequence(),
    );
    Ok(GpuFontUi4Stamp {
        summary,
        render,
        text_chars: prepared.built.text_chars,
        rows: prepared.built.rows,
        scanout_width: prepared.scanout_width,
        scanout_height: prepared.scanout_height,
        size_percent: prepared.size_percent,
        stamp_width: target_width,
        stamp_height: target_height,
        producer_path,
        release,
    })
}

/// Lay out a shell font request as a fixed-size logical document and retain
/// its complete triangle mesh.  The UI4 viewport is intentionally smaller
/// than the document: vertices outside it are clipped by RCS and become
/// visible through draw-time viewport translation.
pub(crate) fn prepare_ui4_font_document(
    request: GpuFontTextRequest<'_>,
    font: GpuFontFace,
    size_percent: u32,
    rgba: GpuFontRgba,
    viewport_width: u32,
    viewport_height: u32,
    document_width: u32,
    document_height: u32,
) -> Result<GpuFontUi4Document, &'static str> {
    if !(MIN_FONT_STAMP_SIZE_PERCENT..=MAX_FONT_STAMP_SIZE_PERCENT).contains(&size_percent) {
        return Err("font-size-percent-range-1-to-100");
    }
    if viewport_width == 0
        || viewport_height == 0
        || document_width < viewport_width
        || document_height < viewport_height
        || document_width <= UI4_DOCUMENT_PADDING_PIXELS.saturating_mul(2)
    {
        return Err("font-document-shape");
    }
    let font_pixels = UI4_DOCUMENT_MIN_FONT_PIXELS
        + (UI4_DOCUMENT_MAX_FONT_PIXELS - UI4_DOCUMENT_MIN_FONT_PIXELS)
            * (size_percent.saturating_sub(1) as f32)
            / (MAX_FONT_STAMP_SIZE_PERCENT - MIN_FONT_STAMP_SIZE_PERCENT) as f32;
    let registry_name = match ensure_font_face_available(font) {
        Ok(()) => font.registry_name(),
        Err(_) => {
            ensure_font_face_available(GpuFontFace::Default)?;
            GpuFontFace::Default.registry_name()
        }
    };
    let content_width =
        document_width.saturating_sub(UI4_DOCUMENT_PADDING_PIXELS.saturating_mul(2)) as f32;
    let wrapped = wrap_ui4_document_rows(request, registry_name, font_pixels, content_width)?;
    let line_height = libm::ceilf(font_pixels * UI4_DOCUMENT_LINE_HEIGHT_SCALE).max(1.0);
    let mut entries = Vec::new();
    for (row, text) in wrapped.iter().enumerate() {
        if text.trim().is_empty() {
            continue;
        }
        entries.push(GpuFontJobEntry {
            text: GpuFontTextRequest::SingleLine(text.as_str()),
            position: [
                UI4_DOCUMENT_PADDING_PIXELS as f32,
                UI4_DOCUMENT_PADDING_PIXELS as f32 + row as f32 * line_height,
            ],
            font_pixels,
            slant: 0.0,
        });
    }
    if entries.is_empty() {
        return Err("text-empty");
    }
    // Map document coordinates against the physical viewport. Coordinates
    // beyond 768x512 remain outside clip until pan translation exposes them;
    // this preserves a strict one-document-pixel to one-frame-pixel mapping.
    let mesh =
        create_resident_font_scene_mesh(entries.as_slice(), font, viewport_width, viewport_height)?;
    let text_chars = wrapped
        .iter()
        .fold(0usize, |total, row| total.saturating_add(row.chars().count()));
    let glyphs = wrapped.iter().fold(0usize, |total, row| {
        total.saturating_add(row.chars().filter(|ch| !ch.is_whitespace()).count())
    });
    let alpha = rgba.a;
    let premultiply =
        |channel: u8| -> u8 { ((u16::from(channel) * u16::from(alpha) + 127) / 255) as u8 };
    crate::log_info!(
        target: "render";
        "intel/gpu-font: ui4-document-ready font={} chars={} rows={} glyphs={} document={}x{} viewport={}x{} font_px={:.2} size={}percent mesh_vertices={} mesh_indices={} residency=render-ppgtt pan=viewport-translation retessellate_on_pan=0 cpu_frame_copy=0\n",
        registry_name,
        text_chars,
        wrapped.len(),
        glyphs,
        document_width,
        document_height,
        viewport_width,
        viewport_height,
        font_pixels,
        size_percent,
        mesh.vertex_count,
        mesh.index_count,
    );
    Ok(GpuFontUi4Document {
        mesh,
        color: [
            premultiply(rgba.r),
            premultiply(rgba.g),
            premultiply(rgba.b),
            alpha,
        ],
        font_name: registry_name,
        text_chars,
        rows: wrapped.len(),
        glyphs,
        size_percent,
        font_pixels,
        document_width,
        document_height,
    })
}

fn wrap_ui4_document_rows(
    request: GpuFontTextRequest<'_>,
    registry_name: &'static str,
    font_pixels: f32,
    maximum_width: f32,
) -> Result<Vec<String>, &'static str> {
    let single;
    let source_rows: &[&str] = match request {
        GpuFontTextRequest::SingleLine(text) => {
            single = [text];
            &single
        }
        GpuFontTextRequest::Rows(rows) => rows,
    };
    let mut wrapped = Vec::new();
    for source in source_rows {
        wrap_ui4_document_paragraph(
            source,
            registry_name,
            font_pixels,
            maximum_width,
            &mut wrapped,
        )?;
    }
    Ok(wrapped)
}

fn wrap_ui4_document_paragraph(
    source: &str,
    registry_name: &'static str,
    font_pixels: f32,
    maximum_width: f32,
    output: &mut Vec<String>,
) -> Result<(), &'static str> {
    if source.trim().is_empty() {
        output.push(String::new());
        return Ok(());
    }
    let mut line = String::new();
    for word in source.split_whitespace() {
        let mut candidate = line.clone();
        if !candidate.is_empty() {
            candidate.push(' ');
        }
        candidate.push_str(word);
        if crate::graphics::font::text_advance_width(
            registry_name,
            candidate.as_str(),
            font_pixels,
        )? <= maximum_width
        {
            line = candidate;
            continue;
        }
        if !line.is_empty() {
            output.push(core::mem::take(&mut line));
        }
        let mut fragment = String::new();
        for ch in word.chars() {
            let mut next = fragment.clone();
            next.push(ch);
            if !fragment.is_empty()
                && crate::graphics::font::text_advance_width(
                    registry_name,
                    next.as_str(),
                    font_pixels,
                )? > maximum_width
            {
                output.push(core::mem::take(&mut fragment));
                fragment.push(ch);
            } else {
                fragment = next;
            }
        }
        line = fragment;
    }
    if !line.is_empty() {
        output.push(line);
    }
    Ok(())
}

/// Draw one crop of a retained font document into the exact UI4 lease.
pub(crate) fn render_ui4_font_document_view(
    document: &GpuFontUi4Document,
    pan_x: u32,
    pan_y: u32,
    destination: crate::intel::gpgpu::GpgpuRgba8Surface,
) -> Result<GpuFontUi4DocumentFrame, &'static str> {
    if !destination.is_valid()
        || pan_x > document.document_width.saturating_sub(destination.width)
        || pan_y > document.document_height.saturating_sub(destination.height)
    {
        return Err("font-document-viewport");
    }
    let draw = crate::intel::render::ResidentSceneDraw {
        mesh: &document.mesh,
        rgba: document.color,
        viewport_translation_px: [-(pan_x as f32), -(pan_y as f32)],
    };
    let frame = crate::intel::render::render_resident_triangle_scene_frame_premultiplied_msaa4_with_coverage_to_surface(
        core::slice::from_ref(&draw),
        &[],
        Some([0, 0, 0, 0]),
        destination,
        false,
    )?;
    if frame.completed_draws != 1 || frame.requested_draws != 1 || !frame.frame_complete {
        return Err("font-document-render-incomplete");
    }
    let release = frame
        .release_fence
        .filter(|release| release.matches(destination.phys, destination.bytes))
        .ok_or("font-document-release-fence")?;
    let producer_path = "resident-document-triangles";
    Ok(GpuFontUi4DocumentFrame {
        render: crate::intel::render::RenderJokerResult {
            variant: producer_path,
            submit_name: "font-document-ui4",
            target: "ui4-font-frame",
            completed: true,
            vs_counter: false,
            ps_state_marker: false,
            raster_packet: false,
            clip_counter: false,
            ps_observed: false,
        },
        producer_path,
        release,
    })
}

pub(crate) fn release_ui4_font_document(document: &GpuFontUi4Document) -> bool {
    crate::intel::render::release_resident_font_mesh(&document.mesh)
}

/// Retained readback-only implementation used while comparing the old
/// tessellated result with the direct-to-UI4 producer. It has no display or
/// broker entry point; live font stamps use [`prepare_text_stamp_for_ui4`] and
/// [`render_prepared_text_stamp_to_ui4`].
fn render_legacy_font_stamp_readback_centered(
    request: GpuFontTextRequest<'_>,
    font: GpuFontFace,
    size_percent: u32,
    rgba: GpuFontRgba,
) -> Result<GpuFontTextStamp, &'static str> {
    if !(MIN_FONT_STAMP_SIZE_PERCENT..=MAX_FONT_STAMP_SIZE_PERCENT).contains(&size_percent) {
        return Err("font-size-percent-range-1-to-100");
    }
    let (scanout_width, scanout_height) =
        crate::intel::active_scanout_dimensions().ok_or("no-active-scanout-dimensions")?;
    let (requested_chars, requested_rows) = match request {
        GpuFontTextRequest::SingleLine(text) => (text.chars().count(), 1),
        GpuFontTextRequest::Rows(rows) => (
            rows.iter()
                .fold(0usize, |count, row| count.saturating_add(row.chars().count())),
            rows.len(),
        ),
    };
    crate::log_info!(
        target: "render";
        "intel/gpu-font: job-stamp-begin font_id={} font={} text_chars={} rows={} size_percent={} scanout={}x{} preferred_path=skrifa-gpgpu-r8 fallback=resident-triangles layout_bounds=cpu-font-metrics geometry_persistence=0\n",
        font.id(),
        font.registry_name(),
        requested_chars,
        requested_rows,
        size_percent,
        scanout_width,
        scanout_height,
    );
    let layout = match request {
        GpuFontTextRequest::SingleLine(_) => GpuFontTextLayout::SingleLine,
        GpuFontTextRequest::Rows(_) => GpuFontTextLayout::Rows,
    };
    let entry = GpuFontJobEntry {
        text: request,
        position: [0.0, 0.0],
        font_pixels: crate::graphics::font::FONT_TESSEL_BASE_PX,
        slant: 0.0,
    };
    let mut built = build_font_job_mesh(core::slice::from_ref(&entry), font)?;
    let mesh_width = libm::ceilf((built.bounds.2 - built.bounds.0).max(1.0)) as u32;
    let mesh_height = libm::ceilf((built.bounds.3 - built.bounds.1).max(1.0)) as u32;
    let (stamp_width, stamp_height) = fit_font_stamp_to_scanout(
        mesh_width,
        mesh_height,
        scanout_width,
        scanout_height,
        size_percent,
    );
    let requested_tessellation_tolerance = native_font_fill_tolerance(
        built.bounds,
        stamp_width,
        stamp_height,
        NATIVE_FONT_STAMP_PADDING_PIXELS,
    );
    let dst_x = (scanout_width.saturating_sub(stamp_width) / 2) as i32;
    let dst_y = (scanout_height.saturating_sub(stamp_height) / 2) as i32;
    if let Some((render, mut readback)) = render_analytical_font_stamp_readback(
        request,
        font,
        built.bounds,
        stamp_width,
        stamp_height,
        NATIVE_FONT_STAMP_PADDING_PIXELS,
    ) {
        let (stamped, source_width, source_height) = composite_font_stamp_readback(
            &mut readback,
            rgba,
            dst_x,
            dst_y,
            stamp_width,
            stamp_height,
        );
        recycle_transient_font_readback(core::mem::take(&mut readback.pixels));
        let summary = built.summaries.pop().ok_or("font-job-summary")?;
        crate::log_info!(
            target: "render";
            "intel/gpu-font: job-stamp stamped={} completed={} font_id={} font={} text_chars={} rows={} size_percent={} render_target={}x{} padding_pixels={} path=kernel-font-stamp-default/skrifa-gpgpu-r8 bounds_source=font-layout-only triangles_submitted=0 scanout={}x{} visible_source={}x{} stamp={}x{} dst={},{} placement=centered fit=contain scale_path=native-target-1to1 rgba=[{},{},{},{}] submits=coverage+composite mask_cache=invocation outline_cache=warmed color_path=cpu-readback-1to1\n",
            stamped as u8,
            render.completed as u8,
            font.id(),
            font.registry_name(),
            built.text_chars,
            built.rows,
            size_percent,
            stamp_width,
            stamp_height,
            NATIVE_FONT_STAMP_PADDING_PIXELS,
            scanout_width,
            scanout_height,
            source_width,
            source_height,
            stamp_width,
            stamp_height,
            dst_x,
            dst_y,
            rgba.r,
            rgba.g,
            rgba.b,
            rgba.a,
        );
        return Ok(GpuFontTextStamp {
            summary,
            render,
            layout,
            text_chars: built.text_chars,
            rows: built.rows,
            stamped,
            dst_x,
            dst_y,
            scanout_width,
            scanout_height,
            size_percent,
            source_width,
            source_height,
            render_target_width: stamp_width,
            render_target_height: stamp_height,
            tessellation_tolerance: 0.0,
            stamp_width,
            stamp_height,
        });
    }
    let upload_capacity = crate::intel::render::transient_font_mesh_upload_capacity_bytes();
    let base_upload_bytes = crate::intel::render::transient_font_mesh_upload_bytes(
        built.vertices.len(),
        built.indices.len(),
    );
    // Reject an unrenderable default mesh before attempting the optional,
    // finer tessellation. In particular, repeated contour-heavy glyphs must
    // not spend more CPU/allocation work on a mesh that cannot be uploaded.
    if !base_upload_bytes.is_some_and(|bytes| bytes <= upload_capacity) {
        crate::log_warn!(
            target: "render";
            "intel/gpu-font: transient-mesh-capacity warning=stamp-rejected required_bytes={} soft_cap_bytes={} vertices={} indices={} target={}x{} tolerance={:.4} resolution_scope=1440p future_scope=4k-8k action=raise-or-grow-transient-staging\n",
            base_upload_bytes.unwrap_or(usize::MAX),
            upload_capacity,
            built.vertices.len(),
            built.indices.len(),
            stamp_width,
            stamp_height,
            DEFAULT_FONT_FILL_TOLERANCE,
        );
        return Err("font-mesh-upload-capacity");
    }

    let mut tessellation_tolerance = DEFAULT_FONT_FILL_TOLERANCE;
    let mut quality_capacity_limited = false;
    if requested_tessellation_tolerance < DEFAULT_FONT_FILL_TOLERANCE {
        let refinement_budget = crate::intel::render::transient_font_mesh_refinement_budget_bytes();
        if base_upload_bytes.is_some_and(|bytes| bytes <= refinement_budget) {
            let candidate = build_font_job_mesh_with_tolerance(
                core::slice::from_ref(&entry),
                font,
                requested_tessellation_tolerance,
            )?;
            let candidate_upload_bytes = crate::intel::render::transient_font_mesh_upload_bytes(
                candidate.vertices.len(),
                candidate.indices.len(),
            );
            if candidate_upload_bytes.is_some_and(|bytes| bytes <= refinement_budget) {
                built = candidate;
                tessellation_tolerance = requested_tessellation_tolerance;
            } else {
                quality_capacity_limited = true;
            }
        } else {
            quality_capacity_limited = true;
        }
    }

    let reusable_pixels = core::mem::take(&mut *TRANSIENT_FONT_STAMP_READBACK.lock());
    let (render, mut readback) =
        crate::intel::render::submit_font_mesh_readback_once_at_extent_reusing(
            built.vertices.as_slice(),
            built.indices.as_slice(),
            built.bounds,
            stamp_width,
            stamp_height,
            NATIVE_FONT_STAMP_PADDING_PIXELS,
            reusable_pixels,
        )?;
    let (stamped, source_width, source_height) =
        composite_font_stamp_readback(&mut readback, rgba, dst_x, dst_y, stamp_width, stamp_height);
    recycle_transient_font_readback(core::mem::take(&mut readback.pixels));

    let mut summaries = built.summaries;
    let summary = summaries.pop().ok_or("font-job-summary")?;
    crate::log_info!(
        target: "render";
        "intel/gpu-font: job-stamp stamped={} completed={} font_id={} font={} text_chars={} rows={} size_percent={} render_target={}x{} padding_pixels={} tessellation_tolerance={:.4} requested_tolerance={:.4} curve_error_target_px={:.2} quality_capacity_limited={} vertices={} indices={} scanout={}x{} visible_source={}x{} stamp={}x{} dst={},{} placement=centered fit=contain scale_path=native-target-1to1 rgba=[{},{},{},{}] submits=1 mesh_cache=none outline_cache=warmed geometry_persistence=0 readback_buffers=1 readback_allocation=reused color_path=cpu-readback-1to1\n",
        stamped as u8,
        render.completed as u8,
        font.id(),
        font.registry_name(),
        built.text_chars,
        built.rows,
        size_percent,
        stamp_width,
        stamp_height,
        NATIVE_FONT_STAMP_PADDING_PIXELS,
        tessellation_tolerance,
        requested_tessellation_tolerance,
        NATIVE_FONT_CURVE_ERROR_PIXELS,
        quality_capacity_limited as u8,
        built.vertices.len(),
        built.indices.len(),
        scanout_width,
        scanout_height,
        source_width,
        source_height,
        stamp_width,
        stamp_height,
        dst_x,
        dst_y,
        rgba.r,
        rgba.g,
        rgba.b,
        rgba.a,
    );
    Ok(GpuFontTextStamp {
        summary,
        render,
        layout,
        text_chars: built.text_chars,
        rows: built.rows,
        stamped,
        dst_x,
        dst_y,
        scanout_width,
        scanout_height,
        size_percent,
        source_width,
        source_height,
        render_target_width: stamp_width,
        render_target_height: stamp_height,
        tessellation_tolerance,
        stamp_width,
        stamp_height,
    })
}

fn native_font_fill_tolerance(
    bounds: (f32, f32, f32, f32),
    target_width: u32,
    target_height: u32,
    padding_pixels: u32,
) -> f32 {
    let mesh_width = (bounds.2 - bounds.0).max(1.0);
    let mesh_height = (bounds.3 - bounds.1).max(1.0);
    let padding_pixels = padding_pixels
        .min(target_width.saturating_sub(1) / 2)
        .min(target_height.saturating_sub(1) / 2);
    let content_width = target_width
        .saturating_sub(padding_pixels.saturating_mul(2))
        .max(1);
    let content_height = target_height
        .saturating_sub(padding_pixels.saturating_mul(2))
        .max(1);
    let pixel_scale = (content_width as f32 / mesh_width).min(content_height as f32 / mesh_height);
    if !pixel_scale.is_finite() || pixel_scale <= 0.0 {
        return DEFAULT_FONT_FILL_TOLERANCE;
    }
    (NATIVE_FONT_CURVE_ERROR_PIXELS / pixel_scale)
        .clamp(MIN_NATIVE_FONT_FILL_TOLERANCE, DEFAULT_FONT_FILL_TOLERANCE)
}

fn recycle_transient_font_readback(mut pixels: Vec<u8>) {
    pixels.clear();
    let mut recycled = TRANSIENT_FONT_STAMP_READBACK.lock();
    if pixels.capacity() >= recycled.capacity() {
        *recycled = pixels;
    }
}

fn visible_font_target_bounds(
    pixels: &[u8],
    width: u32,
    height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let pitch = width as usize * 4;
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0;
    let mut max_y = 0;
    let mut found = false;
    for y in 0..height {
        for x in 0..width {
            let alpha_offset = y as usize * pitch + x as usize * 4 + 3;
            if pixels.get(alpha_offset).copied().unwrap_or(0) == 0 {
                continue;
            }
            found = true;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    found.then(|| {
        (
            min_x,
            min_y,
            max_x.saturating_sub(min_x).saturating_add(1),
            max_y.saturating_sub(min_y).saturating_add(1),
        )
    })
}

fn fit_font_stamp_to_scanout(
    source_width: u32,
    source_height: u32,
    scanout_width: u32,
    scanout_height: u32,
    size_percent: u32,
) -> (u32, u32) {
    let max_width = ((u64::from(scanout_width) * u64::from(size_percent)) / 100).max(1);
    let max_height = ((u64::from(scanout_height) * u64::from(size_percent)) / 100).max(1);
    let source_width = u64::from(source_width.max(1));
    let source_height = u64::from(source_height.max(1));
    let (width, height) =
        if source_width.saturating_mul(max_height) >= source_height.saturating_mul(max_width) {
            let height = source_height
                .saturating_mul(max_width)
                .saturating_add(source_width / 2)
                / source_width;
            (max_width, height.max(1))
        } else {
            let width = source_width
                .saturating_mul(max_height)
                .saturating_add(source_height / 2)
                / source_height;
            (width.max(1), max_height)
        };
    (width as u32, height as u32)
}

fn recolor_transient_font_target(pixels: &mut [u8], rgba: GpuFontRgba) {
    for pixel in pixels.chunks_exact_mut(4) {
        let coverage = pixel[3];
        if coverage == 0 {
            continue;
        }
        pixel[0] = rgba.r;
        pixel[1] = rgba.g;
        pixel[2] = rgba.b;
        pixel[3] = ((u16::from(coverage) * u16::from(rgba.a) + 127) / 255) as u8;
    }
}

/// Build all positioned text groups into one mesh and issue one indexed draw.
///
/// Each entry retains the 256-character text-request limit. A job has no
/// aggregate character cap, allowing callers to compose many independently
/// positioned lines/row groups without multiplying GPU submissions.
pub(crate) fn render_font_job_once(job: GpuFontJob<'_>) -> Result<GpuFontJobRender, &'static str> {
    let native_scale = job.native_scale;
    let font = job.font;
    let built = build_font_job_mesh(job.entries, font)?;
    let render = crate::intel::render::submit_font_mesh_once_scaled(
        built.vertices.as_slice(),
        built.indices.as_slice(),
        built.bounds,
        native_scale,
    )?;
    crate::log_info!(
        target: "render";
        "intel/gpu-font: job-render ok=1 font_id={} font={} entries={} text_chars={} rows={} native_scale={} vertices={} indices={} submits=1 mesh_cache=none\n",
        font.id(),
        font.registry_name(),
        built.entries,
        built.text_chars,
        built.rows,
        native_scale,
        built.vertices.len(),
        built.indices.len(),
    );
    Ok(GpuFontJobRender {
        summaries: built.summaries,
        render,
        entries: built.entries,
        text_chars: built.text_chars,
        rows: built.rows,
        glyphs: built.glyphs,
        vertices: built.vertices.len(),
        indices: built.indices.len(),
    })
}

/// Build one positioned text-row job and return its transparent native-size
/// render target to a kernel compositor instead of presenting it directly.
///
/// The outline registry remains warm and size-independent. Invocation-specific
/// geometry is dropped after the synchronous submission; the returned pixel
/// allocation can be recycled with [`recycle_font_job_readback`].
pub(crate) fn render_font_job_readback_once(
    job: GpuFontJob<'_>,
) -> Result<crate::intel::render::FontRenderTargetReadback, &'static str> {
    let target_pixels = crate::intel::render::font_native_scale_target_pixels(job.native_scale)
        .ok_or("font-native-scale-range")?;
    let built = build_font_job_mesh(job.entries, job.font)?;
    let reusable_pixels = core::mem::take(&mut *TRANSIENT_FONT_STAMP_READBACK.lock());
    let (render, readback) =
        crate::intel::render::submit_font_mesh_readback_once_at_extent_reusing(
            built.vertices.as_slice(),
            built.indices.as_slice(),
            built.bounds,
            target_pixels,
            target_pixels,
            target_pixels / 20,
            reusable_pixels,
        )?;
    if !render.completed {
        recycle_transient_font_readback(readback.pixels);
        return Err("font-render-incomplete");
    }
    Ok(readback)
}

/// Render positioned text directly in a UI scene's pixel coordinate space.
/// Unlike the stamp path, the complete mesh is not normalized to its own
/// bounds: `(0, 0)..(width, height)` maps one-to-one onto the target.
pub(crate) fn render_font_scene_readback_once(
    job: GpuFontJob<'_>,
    width: u32,
    height: u32,
) -> Result<crate::intel::render::FontRenderTargetReadback, &'static str> {
    if width == 0 || height == 0 {
        return Err("font-scene-empty");
    }
    // The shared kernel font service prefers the Skrifa -> GPGPU coverage
    // route at every supported scale.  Consumers of this longstanding
    // readback API inherit it automatically; triangles remain its transparent
    // fallback for rows, unsupported ppem, or a failed integrity audit.
    if let Ok(coverage) = create_gpu_font_scene_coverage_mask_at_raster(
        job.entries,
        job.font,
        width,
        height,
        width,
        height,
    ) {
        let origin = coverage.origin_px();
        let draw = crate::intel::render::ResidentSceneCoverageDraw {
            mask: coverage.surface(),
            mask_rect: coverage.full_rect(),
            dst_xy: crate::intel::gpgpu::GpgpuPoint::new(origin[0], origin[1]),
            color_rgba: u32::from_le_bytes([u8::MAX; 4]),
        };
        match crate::intel::render::capture_resident_triangle_scene_frame_premultiplied_at_extent_msaa4_with_coverage(
            &[],
            core::slice::from_ref(&draw),
            Some([0, 0, 0, 0]),
            width,
            height,
            false,
        ) {
            Ok(captured)
                if captured.completed_draws == captured.requested_draws
                    && captured.requested_draws == 1
                    && captured.rgba.is_some() =>
            {
                crate::log_info!(
                    target: "render";
                    "intel/gpu-font: scene-readback path=kernel-font-stamp-default/skrifa-gpgpu-r8 font={} entries={} target={}x{} mask_gpu=0x{:X} fallback=resident-triangles\n",
                    job.font.registry_name(),
                    job.entries.len(),
                    width,
                    height,
                    coverage.surface().gpu,
                );
                return Ok(crate::intel::render::FontRenderTargetReadback {
                    width,
                    height,
                    pixels: captured.rgba.expect("coverage readback checked"),
                });
            }
            Ok(_) | Err(_) => {
                crate::log_warn!(
                    target: "render";
                    "intel/gpu-font: scene-readback analytical composite unavailable font={} entries={} target={}x{} action=resident-triangle-fallback\n",
                    job.font.registry_name(),
                    job.entries.len(),
                    width,
                    height,
                );
            }
        }
    }
    let built = build_font_job_mesh(job.entries, job.font)?;
    let upload_bytes = crate::intel::render::transient_font_mesh_upload_bytes(
        built.vertices.len(),
        built.indices.len(),
    )
    .ok_or("font-mesh-staging-overflow")?;
    let upload_capacity = crate::intel::render::transient_font_mesh_upload_capacity_bytes();
    if upload_bytes > upload_capacity {
        crate::log_info!(
            target: "render";
            "intel/gpu-font: scene mesh split required entries={} text_chars={} vertices={} indices={} upload_bytes={} capacity_bytes={}\n",
            built.entries,
            built.text_chars,
            built.vertices.len(),
            built.indices.len(),
            upload_bytes,
            upload_capacity,
        );
        return Err("font-mesh-staging-capacity");
    }
    let reusable_pixels = core::mem::take(&mut *TRANSIENT_FONT_STAMP_READBACK.lock());
    let (render, readback) =
        crate::intel::render::submit_font_mesh_readback_once_at_extent_reusing(
            built.vertices.as_slice(),
            built.indices.as_slice(),
            (0.0, 0.0, width as f32, height as f32),
            width,
            height,
            0,
            reusable_pixels,
        )?;
    if !render.completed {
        recycle_transient_font_readback(readback.pixels);
        return Err("font-render-incomplete");
    }
    Ok(readback)
}

pub(crate) fn recycle_font_job_readback(readback: crate::intel::render::FontRenderTargetReadback) {
    recycle_transient_font_readback(readback.pixels);
}

/// Build positioned font geometry once and upload it as a scene-owned mesh.
///
/// Unlike a font stamp, positions are mapped against the caller's complete
/// viewport. The returned allocation can be included in resident scene draws
/// until the owning service explicitly releases it.
pub(crate) fn create_resident_font_scene_mesh(
    entries: &[GpuFontJobEntry<'_>],
    font: GpuFontFace,
    viewport_width: u32,
    viewport_height: u32,
) -> Result<crate::intel::render::ResidentTriangleMesh, &'static str> {
    create_resident_font_scene_mesh_with_positioning(
        entries,
        font,
        viewport_width,
        viewport_height,
        GpuFontJobPositioning::Origin,
        None,
    )
}

/// Build a resident font scene whose entry positions denote the center of each
/// entry's actual tessellated bounds. This is visual centering, independent of
/// advance width, side bearings, ascenders, descenders, or the chosen face.
pub(crate) fn create_resident_font_centered_scene_mesh(
    entries: &[GpuFontJobEntry<'_>],
    font: GpuFontFace,
    viewport_width: u32,
    viewport_height: u32,
) -> Result<crate::intel::render::ResidentTriangleMesh, &'static str> {
    create_resident_font_scene_mesh_with_positioning(
        entries,
        font,
        viewport_width,
        viewport_height,
        GpuFontJobPositioning::VisualBoundsCenter,
        None,
    )
}

fn gpu_font_raster_quality(
    viewport_width: u32,
    viewport_height: u32,
    raster_width: u32,
    raster_height: u32,
) -> Option<GpuFontRasterQuality> {
    if viewport_width == 0 || viewport_height == 0 || raster_width == 0 || raster_height == 0 {
        return None;
    }
    Some(GpuFontRasterQuality {
        pixels_per_unit_x: raster_width as f32 / viewport_width as f32,
        pixels_per_unit_y: raster_height as f32 / viewport_height as f32,
    })
}

fn analytical_coverage_ppem(font_units: f32, quality: GpuFontRasterQuality) -> Option<f32> {
    if !quality.pixels_per_unit_x.is_finite()
        || !quality.pixels_per_unit_y.is_finite()
        || quality.pixels_per_unit_x <= 0.0
        || quality.pixels_per_unit_y <= 0.0
    {
        return None;
    }
    let ppem = font_units * quality.pixels_per_unit_y;
    (ppem.is_finite()
        && (ANALYTICAL_COVERAGE_MIN_RASTER_PX..=ANALYTICAL_COVERAGE_MAX_RASTER_PX).contains(&ppem))
    .then_some(ppem)
}

/// True when every entry can use the production analytical coverage path.
/// This is the shared font-scene default at both native and magnified scales;
/// resident triangles remain a correctness fallback rather than a size fork.
pub(crate) fn gpu_font_entries_use_analytical_coverage(
    entries: &[GpuFontJobEntry<'_>],
    viewport_width: u32,
    viewport_height: u32,
    raster_width: u32,
    raster_height: u32,
) -> bool {
    let Some(quality) =
        gpu_font_raster_quality(viewport_width, viewport_height, raster_width, raster_height)
    else {
        return false;
    };
    !entries.is_empty()
        && entries.iter().all(|entry| {
            matches!(entry.text, GpuFontTextRequest::SingleLine(_))
                && analytical_coverage_ppem(entry.font_pixels, quality).is_some()
        })
}

fn small_font_optical_bias_px(ppem: f32) -> f32 {
    let low_scale = ((SMALL_FONT_HINT_MAX_RASTER_PX - ppem)
        / (SMALL_FONT_HINT_MAX_RASTER_PX - SMALL_FONT_HINT_MIN_RASTER_PX))
        .clamp(0.0, 1.0);
    0.04 + 0.18 * low_scale
}

fn outline_point_x_words(kind: u32) -> Option<&'static [usize]> {
    const NONE: [usize; 0] = [];
    const END: [usize; 1] = [1];
    const QUAD: [usize; 2] = [1, 3];
    const CUBIC: [usize; 3] = [1, 3, 5];
    match kind {
        0 | 1 => Some(&END),
        2 => Some(&QUAD),
        3 => Some(&CUBIC),
        4 => Some(&NONE),
        _ => None,
    }
}

fn include_coverage_point(bounds: &mut Option<(f32, f32, f32, f32)>, x: f32, y: f32) {
    *bounds = Some(match *bounds {
        Some((min_x, min_y, max_x, max_y)) => {
            (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
        }
        None => (x, y, x, y),
    });
}

/// Return the exact segment envelope consumed by the analytical coverage
/// kernel. Control points deliberately do not participate by themselves:
/// they can sit well outside a quadratic or cubic while the flattened curve
/// remains safely inside them. Keeping those points in the output-edge audit
/// made an otherwise valid outline fail only after its ppem was increased.
fn flattened_outline_bounds(
    ops: &[[u32; 8]],
    subdivisions: u32,
) -> Result<(f32, f32, f32, f32), &'static str> {
    if !(1..=16).contains(&subdivisions) {
        return Err("font-coverage-subdivisions");
    }

    let point = |op: &[u32; 8], word: usize| -> Result<[f32; 2], &'static str> {
        let value = [f32::from_bits(op[word]), f32::from_bits(op[word + 1])];
        value
            .iter()
            .all(|component| component.is_finite())
            .then_some(value)
            .ok_or("font-coverage-outline-point")
    };
    let mut bounds = None;
    let mut current = [0.0f32; 2];
    let mut contour_start = current;
    let mut have_current = false;
    for op in ops {
        match op[0] {
            0 => {
                current = point(op, 1)?;
                contour_start = current;
                have_current = true;
            }
            1 if have_current => {
                let next = point(op, 1)?;
                include_coverage_point(&mut bounds, current[0], current[1]);
                include_coverage_point(&mut bounds, next[0], next[1]);
                current = next;
            }
            2 if have_current => {
                let start = current;
                let p0 = point(op, 1)?;
                let p1 = point(op, 3)?;
                include_coverage_point(&mut bounds, current[0], current[1]);
                for step in 1..=subdivisions {
                    let t = step as f32 / subdivisions as f32;
                    let one = 1.0 - t;
                    let next = [
                        one * one * start[0] + 2.0 * one * t * p0[0] + t * t * p1[0],
                        one * one * start[1] + 2.0 * one * t * p0[1] + t * t * p1[1],
                    ];
                    include_coverage_point(&mut bounds, next[0], next[1]);
                    current = next;
                }
            }
            3 if have_current => {
                let start = current;
                let p0 = point(op, 1)?;
                let p1 = point(op, 3)?;
                let p2 = point(op, 5)?;
                include_coverage_point(&mut bounds, current[0], current[1]);
                for step in 1..=subdivisions {
                    let t = step as f32 / subdivisions as f32;
                    let one = 1.0 - t;
                    let next = [
                        one * one * one * start[0]
                            + 3.0 * one * one * t * p0[0]
                            + 3.0 * one * t * t * p1[0]
                            + t * t * t * p2[0],
                        one * one * one * start[1]
                            + 3.0 * one * one * t * p0[1]
                            + 3.0 * one * t * t * p1[1]
                            + t * t * t * p2[1],
                    ];
                    include_coverage_point(&mut bounds, next[0], next[1]);
                    current = next;
                }
            }
            4 if have_current => {
                include_coverage_point(&mut bounds, current[0], current[1]);
                include_coverage_point(&mut bounds, contour_start[0], contour_start[1]);
                current = contour_start;
                have_current = false;
            }
            0..=4 => {}
            _ => return Err("font-coverage-outline-op"),
        }
    }
    bounds.ok_or("font-coverage-outline-bounds")
}

fn transform_outline_to_raster(
    source: &[[u32; 8]],
    units_per_em: u16,
    entry: GpuFontJobEntry<'_>,
    quality: GpuFontRasterQuality,
    ppem: f32,
    positioning: GpuFontJobPositioning,
) -> Result<(Vec<[u32; 8]>, (f32, f32, f32, f32), (f32, f32, f32, f32)), &'static str> {
    if source.is_empty() || units_per_em == 0 {
        return Err("font-coverage-outline-empty");
    }
    let scale = ppem / f32::from(units_per_em);
    let baseline_y = match positioning {
        GpuFontJobPositioning::Origin => ppem,
        GpuFontJobPositioning::VisualBoundsCenter => 0.0,
    };
    let mut scaled_bounds = None;
    for op in source {
        let point_words = outline_point_x_words(op[0]).ok_or("font-coverage-outline-op")?;
        for &x_word in point_words {
            let x = f32::from_bits(op[x_word]) * scale;
            let y = baseline_y - f32::from_bits(op[x_word + 1]) * scale;
            if !x.is_finite() || !y.is_finite() {
                return Err("font-coverage-outline-point");
            }
            include_coverage_point(&mut scaled_bounds, x, y);
        }
    }
    let scaled_bounds = scaled_bounds.ok_or("font-coverage-outline-bounds")?;
    let shear_center_y = (scaled_bounds.1 + scaled_bounds.3) * 0.5;
    let shear_aspect = quality.pixels_per_unit_x / quality.pixels_per_unit_y;
    let mut transformed = Vec::with_capacity(source.len());
    let mut local_bounds = None;
    for source_op in source {
        let point_words = outline_point_x_words(source_op[0]).ok_or("font-coverage-outline-op")?;
        let mut op = *source_op;
        for &x_word in point_words {
            let y = baseline_y - f32::from_bits(source_op[x_word + 1]) * scale;
            let x = f32::from_bits(source_op[x_word]) * scale
                + entry.slant * shear_aspect * (shear_center_y - y);
            op[x_word] = x.to_bits();
            op[x_word + 1] = y.to_bits();
            include_coverage_point(&mut local_bounds, x, y);
        }
        transformed.push(op);
    }
    let local_bounds = local_bounds.ok_or("font-coverage-outline-bounds")?;
    let position_px = [
        entry.position[0] * quality.pixels_per_unit_x,
        entry.position[1] * quality.pixels_per_unit_y,
    ];
    let origin_px = match positioning {
        GpuFontJobPositioning::Origin => {
            [libm::roundf(position_px[0]), libm::roundf(position_px[1])]
        }
        GpuFontJobPositioning::VisualBoundsCenter => [
            libm::roundf(position_px[0] - (local_bounds.0 + local_bounds.2) * 0.5),
            libm::roundf(position_px[1] - (local_bounds.1 + local_bounds.3) * 0.5),
        ],
    };
    if !origin_px[0].is_finite() || !origin_px[1].is_finite() {
        return Err("font-coverage-origin");
    }
    let mut bounds = None;
    for op in &mut transformed {
        let point_words = outline_point_x_words(op[0]).ok_or("font-coverage-outline-op")?;
        for &x_word in point_words {
            let x = f32::from_bits(op[x_word]) + origin_px[0];
            let y = f32::from_bits(op[x_word + 1]) + origin_px[1];
            op[x_word] = x.to_bits();
            op[x_word + 1] = y.to_bits();
            include_coverage_point(&mut bounds, x, y);
        }
    }
    let conservative_bounds = bounds.ok_or("font-coverage-outline-bounds")?;
    let flattened_bounds =
        flattened_outline_bounds(transformed.as_slice(), ANALYTICAL_COVERAGE_CURVE_SUBDIVISIONS)?;
    Ok((transformed, conservative_bounds, flattened_bounds))
}

fn coverage_integer_rect(
    bounds: (f32, f32, f32, f32),
    optical_bias_px: f32,
) -> Result<(i32, i32, i32, i32), &'static str> {
    let padding = 1.0 + optical_bias_px;
    let values = [
        libm::floorf(bounds.0 - padding),
        libm::floorf(bounds.1 - padding),
        libm::ceilf(bounds.2 + padding),
        libm::ceilf(bounds.3 + padding),
    ];
    if values
        .iter()
        .any(|value| !value.is_finite() || *value < i32::MIN as f32 || *value > i32::MAX as f32)
    {
        return Err("font-coverage-rect-range");
    }
    let rect = (values[0] as i32, values[1] as i32, values[2] as i32, values[3] as i32);
    (rect.2 > rect.0 && rect.3 > rect.1)
        .then_some(rect)
        .ok_or("font-coverage-rect-empty")
}

fn translate_coverage_ops(ops: &mut [[u32; 8]], dx: f32, dy: f32) {
    for op in ops {
        let Some(point_words) = outline_point_x_words(op[0]) else {
            continue;
        };
        for &x_word in point_words {
            op[x_word] = (f32::from_bits(op[x_word]) + dx).to_bits();
            op[x_word + 1] = (f32::from_bits(op[x_word + 1]) + dy).to_bits();
        }
    }
}

/// Build and retain the production Skrifa-afterpath coverage mask.
///
/// This is not CPU tessellation and it is not a stretched scanout. The CPU
/// only positions warmed outline commands; the iGPU evaluates winding,
/// distance, fractional coverage, and bounded low-ppem optical expansion.
pub(crate) fn create_gpu_font_centered_coverage_mask_at_raster(
    entries: &[GpuFontJobEntry<'_>],
    font: GpuFontFace,
    viewport_width: u32,
    viewport_height: u32,
    raster_width: u32,
    raster_height: u32,
) -> Result<GpuFontCoverageMask, &'static str> {
    create_gpu_font_coverage_mask_at_raster(
        entries,
        font,
        viewport_width,
        viewport_height,
        raster_width,
        raster_height,
        GpuFontJobPositioning::VisualBoundsCenter,
    )
}

/// Build the same default analytical mask for origin-positioned font scenes.
/// This is the path used by the generic kernel font readback/stamp service and
/// therefore by the Draw3D TCP waiting scene.
pub(crate) fn create_gpu_font_scene_coverage_mask_at_raster(
    entries: &[GpuFontJobEntry<'_>],
    font: GpuFontFace,
    viewport_width: u32,
    viewport_height: u32,
    raster_width: u32,
    raster_height: u32,
) -> Result<GpuFontCoverageMask, &'static str> {
    create_gpu_font_coverage_mask_at_raster(
        entries,
        font,
        viewport_width,
        viewport_height,
        raster_width,
        raster_height,
        GpuFontJobPositioning::Origin,
    )
}

fn create_gpu_font_coverage_mask_at_raster(
    entries: &[GpuFontJobEntry<'_>],
    font: GpuFontFace,
    viewport_width: u32,
    viewport_height: u32,
    raster_width: u32,
    raster_height: u32,
    positioning: GpuFontJobPositioning,
) -> Result<GpuFontCoverageMask, &'static str> {
    let quality =
        gpu_font_raster_quality(viewport_width, viewport_height, raster_width, raster_height)
            .ok_or("font-raster-empty")?;
    if !gpu_font_entries_use_analytical_coverage(
        entries,
        viewport_width,
        viewport_height,
        raster_width,
        raster_height,
    ) {
        return Err("font-coverage-ineligible");
    }
    ensure_font_face_available(font)?;

    let mut prepared = Vec::with_capacity(entries.len());
    let mut union_rect: Option<(i32, i32, i32, i32)> = None;
    let mut audit_union_rect: Option<(i32, i32, i32, i32)> = None;
    let mut ppem_min = f32::MAX;
    let mut ppem_max = 0.0f32;
    let mut optical_bias_max_px = 0.0f32;
    for &entry in entries {
        if !entry.position[0].is_finite()
            || !entry.position[1].is_finite()
            || !entry.font_pixels.is_finite()
            || !entry.slant.is_finite()
            || entry.font_pixels <= 0.0
            || entry.slant.abs() > 1.0
        {
            return Err("font-job-position");
        }
        let GpuFontTextRequest::SingleLine(text) = entry.text else {
            return Err("font-coverage-layout");
        };
        let ppem = analytical_coverage_ppem(entry.font_pixels, quality)
            .ok_or("font-coverage-ineligible")?;
        let optical_bias_px = small_font_optical_bias_px(ppem);
        let outline = crate::graphics::font::gpu_outline_for_text(font.registry_name(), text)?;
        let (ops, bounds, flattened_bounds) = transform_outline_to_raster(
            outline.ops.as_slice(),
            outline.units_per_em,
            entry,
            quality,
            ppem,
            positioning,
        )?;
        let rect = coverage_integer_rect(bounds, optical_bias_px)?;
        let audit_rect = coverage_integer_rect(flattened_bounds, optical_bias_px)?;
        union_rect = Some(match union_rect {
            Some(union) => {
                (union.0.min(rect.0), union.1.min(rect.1), union.2.max(rect.2), union.3.max(rect.3))
            }
            None => rect,
        });
        audit_union_rect = Some(match audit_union_rect {
            Some(union) => (
                union.0.min(audit_rect.0),
                union.1.min(audit_rect.1),
                union.2.max(audit_rect.2),
                union.3.max(audit_rect.3),
            ),
            None => audit_rect,
        });
        ppem_min = ppem_min.min(ppem);
        ppem_max = ppem_max.max(ppem);
        optical_bias_max_px = optical_bias_max_px.max(optical_bias_px);
        prepared.push(PreparedGpuFontCoverageEntry {
            ops,
            rect,
            optical_bias_px,
        });
    }

    let union = union_rect.ok_or("font-coverage-empty")?;
    let width = u32::try_from(i64::from(union.2) - i64::from(union.0))
        .map_err(|_| "font-coverage-mask-range")?;
    let height = u32::try_from(i64::from(union.3) - i64::from(union.1))
        .map_err(|_| "font-coverage-mask-range")?;
    let estimated_segment_evaluations = prepared.iter().try_fold(0u64, |total, entry| {
        let width = u64::try_from(i64::from(entry.rect.2) - i64::from(entry.rect.0)).ok()?;
        let height = u64::try_from(i64::from(entry.rect.3) - i64::from(entry.rect.1)).ok()?;
        let per_pixel = (entry.ops.len() as u64)
            .checked_mul(u64::from(ANALYTICAL_COVERAGE_CURVE_SUBDIVISIONS))?;
        total.checked_add(width.checked_mul(height)?.checked_mul(per_pixel)?)
    });
    let Some(estimated_segment_evaluations) = estimated_segment_evaluations else {
        return Err("font-coverage-workload");
    };
    if estimated_segment_evaluations > ANALYTICAL_COVERAGE_MAX_SEGMENT_EVALUATIONS {
        crate::log_info!(
            target: "render";
            "intel/gpu-font: analytical admission=triangle entries={} mask={}x{} estimated_segment_evaluations={} limit={} submitted=0 reason=bounded-direct-rcs-latency\n",
            prepared.len(),
            width,
            height,
            estimated_segment_evaluations,
            ANALYTICAL_COVERAGE_MAX_SEGMENT_EVALUATIONS,
        );
        return Err("font-coverage-workload");
    }
    let storage = crate::intel::gpgpu::allocate_font_coverage_mask(width, height)
        .ok_or("font-coverage-mask-alloc")?;
    for entry in &mut prepared {
        translate_coverage_ops(entry.ops.as_mut_slice(), -(union.0 as f32), -(union.1 as f32));
        let rect = crate::intel::gpgpu::GpgpuRect::new(
            entry.rect.0 - union.0,
            entry.rect.1 - union.1,
            u32::try_from(entry.rect.2 - entry.rect.0).map_err(|_| "font-coverage-rect-range")?,
            u32::try_from(entry.rect.3 - entry.rect.1).map_err(|_| "font-coverage-rect-range")?,
        );
        match crate::intel::gpgpu::font_outline_coverage_r8(
            &storage,
            entry.ops.as_slice(),
            rect,
            ANALYTICAL_COVERAGE_CURVE_SUBDIVISIONS,
            entry.optical_bias_px,
        ) {
            crate::intel::gpgpu::GpgpuDispatchRetirement::Complete => {}
            crate::intel::gpgpu::GpgpuDispatchRetirement::NotSubmitted => {
                return Err("font-coverage-dispatch");
            }
            crate::intel::gpgpu::GpgpuDispatchRetirement::SubmittedIncomplete => {
                // Hardware can still reference this mask. Its unique PPGTT VA
                // and DMA pages stay quarantined together with the direct-RCS
                // context until reboot.
                core::mem::forget(storage);
                return Err("font-coverage-retirement-uncertain");
            }
        }
    }

    let audit = storage
        .nonzero_audit()
        .ok_or("font-coverage-empty-output")?;
    let expected = audit_union_rect.ok_or("font-coverage-empty")?;
    let expected_local = (
        i64::from(expected.0) - i64::from(union.0),
        i64::from(expected.1) - i64::from(union.1),
        i64::from(expected.2) - i64::from(union.0),
        i64::from(expected.3) - i64::from(union.1),
    );
    let occupied = (
        i64::from(audit.bounds.x),
        i64::from(audit.bounds.y),
        i64::from(audit.bounds.x) + i64::from(audit.bounds.width),
        i64::from(audit.bounds.y) + i64::from(audit.bounds.height),
    );
    const EDGE_AUDIT_SLOP_PX: i64 = 2;
    if occupied.0 > expected_local.0 + EDGE_AUDIT_SLOP_PX
        || occupied.1 > expected_local.1 + EDGE_AUDIT_SLOP_PX
        || occupied.2 + EDGE_AUDIT_SLOP_PX < expected_local.2
        || occupied.3 + EDGE_AUDIT_SLOP_PX < expected_local.3
    {
        return Err("font-coverage-truncated-output");
    }

    crate::log_info!(
        target: "render";
        "intel/gpu-font: analytical-coverage font={} entries={} positioning={} mask={}x{} mask_gpu=0x{:X} origin={},{} occupied={},{},{}x{} expected_local={},{},{},{} nonzero={} ppem_range={:.2}..={:.2} bias_max_px={:.3} outline=skrifa-warm-ops fill=gpgpu-nonzero-winding edge=signed-distance-r8 subdivisions={} va=unique-resident audit=flattened-edge-span fallback=resident-triangles\n",
        font.registry_name(),
        prepared.len(),
        match positioning {
            GpuFontJobPositioning::Origin => "origin",
            GpuFontJobPositioning::VisualBoundsCenter => "visual-center",
        },
        width,
        height,
        storage.surface().gpu,
        union.0,
        union.1,
        audit.bounds.x,
        audit.bounds.y,
        audit.bounds.width,
        audit.bounds.height,
        expected_local.0,
        expected_local.1,
        expected_local.2,
        expected_local.3,
        audit.nonzero_pixels,
        ppem_min,
        ppem_max,
        optical_bias_max_px,
        ANALYTICAL_COVERAGE_CURVE_SUBDIVISIONS,
    );
    Ok(GpuFontCoverageMask {
        storage,
        origin_px: [union.0, union.1],
    })
}

/// Build the resident-triangle fallback with knowledge of the final physical
/// target. The shared analytical mask is the preferred scene path; these
/// triangles remain available if its generation or composition audit fails.
pub(crate) fn create_resident_font_centered_scene_mesh_at_raster(
    entries: &[GpuFontJobEntry<'_>],
    font: GpuFontFace,
    viewport_width: u32,
    viewport_height: u32,
    raster_width: u32,
    raster_height: u32,
) -> Result<crate::intel::render::ResidentTriangleMesh, &'static str> {
    if raster_width == 0 || raster_height == 0 {
        return Err("font-raster-empty");
    }
    let quality = GpuFontRasterQuality {
        pixels_per_unit_x: raster_width as f32 / viewport_width.max(1) as f32,
        pixels_per_unit_y: raster_height as f32 / viewport_height.max(1) as f32,
    };
    create_resident_font_scene_mesh_with_positioning(
        entries,
        font,
        viewport_width,
        viewport_height,
        GpuFontJobPositioning::VisualBoundsCenter,
        Some(quality),
    )
}

fn create_resident_font_scene_mesh_with_positioning(
    entries: &[GpuFontJobEntry<'_>],
    font: GpuFontFace,
    viewport_width: u32,
    viewport_height: u32,
    positioning: GpuFontJobPositioning,
    raster_quality: Option<GpuFontRasterQuality>,
) -> Result<crate::intel::render::ResidentTriangleMesh, &'static str> {
    if viewport_width == 0 || viewport_height == 0 {
        return Err("font-scene-empty");
    }
    let built = build_font_job_mesh_inner(entries, font, None, positioning, raster_quality)?;
    let hinted_entries = built
        .summaries
        .iter()
        .filter(|summary| summary.outline_source == "skrifa-size-hinted-outline")
        .count();
    if hinted_entries != 0 {
        let quality = raster_quality.ok_or("font-raster-quality")?;
        crate::log_info!(
            target: "render";
            "intel/gpu-font: small-raster-quality font={} entries={} hinted_entries={} raster_scale={:.4},{:.4} ppem_range={:.1}..={:.1} outline=skrifa-smooth-hinted origin=pixel-snapped coverage=single-sample-next-rung geometry_uploads=1\n",
            font.registry_name(),
            built.entries,
            hinted_entries,
            quality.pixels_per_unit_x,
            quality.pixels_per_unit_y,
            SMALL_FONT_HINT_MIN_RASTER_PX,
            SMALL_FONT_HINT_MAX_RASTER_PX,
        );
    }
    let width = viewport_width as f32;
    let height = viewport_height as f32;
    let mut vertices = Vec::with_capacity(built.vertices.len());
    for source in &built.vertices {
        vertices.push([
            source[0] * 2.0 / width - 1.0,
            1.0 - source[1] * 2.0 / height,
            0.5,
        ]);
    }
    let mut indices = Vec::with_capacity(built.indices.len());
    for triangle in built.indices.chunks_exact(3) {
        let v0 = vertices[triangle[0] as usize];
        let v1 = vertices[triangle[1] as usize];
        let v2 = vertices[triangle[2] as usize];
        let area2 = (v1[0] - v0[0]) * (v2[1] - v0[1]) - (v1[1] - v0[1]) * (v2[0] - v0[0]);
        if area2 < 0.0 {
            indices.extend_from_slice(&[triangle[0], triangle[2], triangle[1]]);
        } else {
            indices.extend_from_slice(triangle);
        }
    }
    crate::intel::render::create_resident_triangle_mesh(&vertices, &indices)
}

fn build_font_job_mesh(
    entries: &[GpuFontJobEntry<'_>],
    font: GpuFontFace,
) -> Result<BuiltGpuFontJob, &'static str> {
    build_font_job_mesh_inner(entries, font, None, GpuFontJobPositioning::Origin, None)
}

fn build_font_job_mesh_with_tolerance(
    entries: &[GpuFontJobEntry<'_>],
    font: GpuFontFace,
    tolerance: f32,
) -> Result<BuiltGpuFontJob, &'static str> {
    build_font_job_mesh_inner(entries, font, Some(tolerance), GpuFontJobPositioning::Origin, None)
}

fn small_font_hint_ppem(
    font_units: f32,
    quality: GpuFontRasterQuality,
    allow_hinting: bool,
) -> Option<f32> {
    if !allow_hinting
        || !quality.pixels_per_unit_x.is_finite()
        || !quality.pixels_per_unit_y.is_finite()
        || quality.pixels_per_unit_x <= 0.0
        || quality.pixels_per_unit_y <= 0.0
    {
        return None;
    }
    let ppem = font_units * quality.pixels_per_unit_y;
    (ppem.is_finite()
        && (SMALL_FONT_HINT_MIN_RASTER_PX..=SMALL_FONT_HINT_MAX_RASTER_PX).contains(&ppem))
    .then_some(ppem)
}

fn build_font_job_mesh_inner(
    entries: &[GpuFontJobEntry<'_>],
    font: GpuFontFace,
    tolerance: Option<f32>,
    positioning: GpuFontJobPositioning,
    raster_quality: Option<GpuFontRasterQuality>,
) -> Result<BuiltGpuFontJob, &'static str> {
    if entries.is_empty() {
        return Err("font-job-empty");
    }
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut summaries = Vec::with_capacity(entries.len());
    let mut bounds: Option<(f32, f32, f32, f32)> = None;
    let mut text_chars = 0usize;
    let mut rows = 0usize;
    let mut glyphs = 0usize;

    for entry in entries {
        if !entry.position[0].is_finite()
            || !entry.position[1].is_finite()
            || !entry.font_pixels.is_finite()
            || !entry.slant.is_finite()
            || entry.font_pixels <= 0.0
            || entry.font_pixels > 256.0
            || entry.slant.abs() > 1.0
        {
            return Err("font-job-position");
        }
        let hinted_ppem = raster_quality.and_then(|quality| {
            small_font_hint_ppem(entry.font_pixels, quality, tolerance.is_none())
        });
        let tessellation_px = hinted_ppem.unwrap_or(crate::graphics::font::FONT_TESSEL_BASE_PX);
        let (mesh, entry_chars, entry_rows) = tessellate_text_request_with_tolerance(
            entry.text,
            font,
            tolerance,
            tessellation_px,
            hinted_ppem.is_some(),
        )?;
        if vertices.len() > u32::MAX as usize {
            return Err("font-job-vertex-range");
        }
        let base_index = vertices.len() as u32;
        let next_vertex_len = vertices
            .len()
            .checked_add(mesh.vertices.len())
            .ok_or("font-job-vertex-overflow")?;
        if next_vertex_len > u32::MAX as usize {
            return Err("font-job-vertex-range");
        }
        let (entry_scale_x, entry_scale_y) = if hinted_ppem.is_some() {
            let quality = raster_quality.ok_or("font-raster-quality")?;
            (quality.pixels_per_unit_x.recip(), quality.pixels_per_unit_y.recip())
        } else {
            let scale = entry.font_pixels / crate::graphics::font::FONT_TESSEL_BASE_PX;
            (scale, scale)
        };
        let shear_center_y = (mesh.summary.min_y + mesh.summary.max_y) * entry_scale_y * 0.5;
        let mut local_bounds: Option<(f32, f32, f32, f32)> = None;
        for vertex in &mesh.vertices {
            let y = vertex[1] * entry_scale_y;
            let x = vertex[0] * entry_scale_x + entry.slant * (shear_center_y - y);
            local_bounds = Some(match local_bounds {
                Some((min_x, min_y, max_x, max_y)) => {
                    (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
                }
                None => (x, y, x, y),
            });
        }
        let local_bounds = local_bounds.ok_or("font-job-bounds")?;
        let mut entry_origin = match positioning {
            GpuFontJobPositioning::Origin => entry.position,
            GpuFontJobPositioning::VisualBoundsCenter => [
                entry.position[0] - (local_bounds.0 + local_bounds.2) * 0.5,
                entry.position[1] - (local_bounds.1 + local_bounds.3) * 0.5,
            ],
        };
        if hinted_ppem.is_some() {
            let quality = raster_quality.ok_or("font-raster-quality")?;
            entry_origin[0] = libm::roundf(entry_origin[0] * quality.pixels_per_unit_x)
                / quality.pixels_per_unit_x;
            entry_origin[1] = libm::roundf(entry_origin[1] * quality.pixels_per_unit_y)
                / quality.pixels_per_unit_y;
        }
        vertices.reserve(mesh.vertices.len());
        for vertex in &mesh.vertices {
            let y = vertex[1] * entry_scale_y;
            let x = vertex[0] * entry_scale_x + entry.slant * (shear_center_y - y);
            vertices.push([x + entry_origin[0], y + entry_origin[1]]);
        }
        indices.reserve(mesh.indices.len());
        for index in &mesh.indices {
            indices.push(
                base_index
                    .checked_add(*index)
                    .ok_or("font-job-index-overflow")?,
            );
        }

        let entry_bounds = (
            local_bounds.0 + entry_origin[0],
            local_bounds.1 + entry_origin[1],
            local_bounds.2 + entry_origin[0],
            local_bounds.3 + entry_origin[1],
        );
        bounds = Some(match bounds {
            Some((min_x, min_y, max_x, max_y)) => (
                min_x.min(entry_bounds.0),
                min_y.min(entry_bounds.1),
                max_x.max(entry_bounds.2),
                max_y.max(entry_bounds.3),
            ),
            None => entry_bounds,
        });
        text_chars = text_chars.saturating_add(entry_chars);
        rows = rows.saturating_add(entry_rows);
        glyphs = glyphs.saturating_add(mesh.summary.glyphs);
        summaries.push(mesh.summary);
    }

    Ok(BuiltGpuFontJob {
        summaries,
        vertices,
        indices,
        bounds: bounds.ok_or("font-job-bounds")?,
        entries: entries.len(),
        text_chars,
        rows,
        glyphs,
    })
}

/// Prepare a font job once and retain its final indexed geometry in dedicated
/// render-PPGTT pages until the returned authority lease is released.
///
/// `(owner, name)` must be unique among live jobs. This keeps every resident
/// allocation attributable and prevents accidental replacement from orphaning
/// the old GPU mapping.
pub(crate) fn persist_font_job(
    tag: GpuFontResidencyTag,
    job: GpuFontJob<'_>,
) -> Result<PersistentGpuFontJob, &'static str> {
    if tag.owner.trim().is_empty() || tag.name.trim().is_empty() {
        return Err("resident-tag-empty");
    }
    if !crate::intel::render::font_native_scale_supported(job.native_scale) {
        return Err("font-native-scale-range");
    }
    // Serialize the full create transaction. Once pages are mapped there is no
    // fallible ownership handoff: this registry immediately receives them.
    let mut service = GPU_FONT_SERVICE.lock();
    if service.resident_jobs.iter().any(|record| record.tag == tag) {
        return Err("resident-tag-in-use");
    }
    let id = service.next_resident_id;
    let Some(next_id) = id.checked_add(1) else {
        return Err("resident-id-exhausted");
    };
    service.next_resident_id = next_id;
    service.resident_generation = service.resident_generation.saturating_add(1).max(1);
    let generation = service.resident_generation;

    let native_scale = job.native_scale;
    let font = job.font;
    let built = build_font_job_mesh(job.entries, font)?;
    let mesh = crate::intel::render::create_resident_font_mesh(
        built.vertices.as_slice(),
        built.indices.as_slice(),
        built.bounds,
    )?;
    let resident_bytes = mesh.storage_bytes;
    let gpu_base = mesh.gpu_base;
    service.resident_jobs.push(ResidentGpuFontJobRecord {
        id,
        generation,
        tag,
        font,
        mesh,
        native_scale,
        entries: built.entries,
        text_chars: built.text_chars,
        rows: built.rows,
        glyphs: built.glyphs,
        submits: 0,
        in_flight: false,
        quarantined: false,
    });
    service.resident_uploads = service.resident_uploads.saturating_add(1);
    crate::log_info!(
        target: "render";
        "intel/gpu-font: resident-create ok=1 id={} generation={} owner={} name={} font_id={} font={} authority=cpu-build->gpu-resident entries={} text_chars={} rows={} glyphs={} vertices={} indices={} native_scale={} gpu=0x{:X} bytes=0x{:X} geometry_uploads=1\n",
        id,
        generation,
        tag.owner,
        tag.name,
        font.id(),
        font.registry_name(),
        built.entries,
        built.text_chars,
        built.rows,
        built.glyphs,
        built.vertices.len(),
        built.indices.len(),
        native_scale,
        gpu_base,
        resident_bytes,
    );
    Ok(PersistentGpuFontJob {
        id,
        generation,
        tag,
        released: false,
    })
}

/// Reuse a persistent job's resident VB/IB directly for one synchronous draw.
pub(crate) fn submit_persistent_font_job(
    lease: &PersistentGpuFontJob,
) -> Result<crate::intel::render::RenderJokerResult, &'static str> {
    submit_persistent_font_job_rgba(lease, GPU_FONT_DEFAULT_RGBA)
}

pub(crate) fn submit_persistent_font_job_rgba(
    lease: &PersistentGpuFontJob,
    rgba: GpuFontRgba,
) -> Result<crate::intel::render::RenderJokerResult, &'static str> {
    submit_persistent_font_job_inner(lease, None, rgba, None)
}

pub(crate) fn submit_persistent_font_job_at_scale(
    lease: &PersistentGpuFontJob,
    native_scale: u32,
) -> Result<crate::intel::render::RenderJokerResult, &'static str> {
    submit_persistent_font_job_at_scale_rgba(lease, native_scale, GPU_FONT_DEFAULT_RGBA)
}

pub(crate) fn submit_persistent_font_job_at_scale_rgba(
    lease: &PersistentGpuFontJob,
    native_scale: u32,
    rgba: GpuFontRgba,
) -> Result<crate::intel::render::RenderJokerResult, &'static str> {
    if !crate::intel::render::font_native_scale_supported(native_scale) {
        return Err("font-native-scale-range");
    }
    submit_persistent_font_job_inner(lease, Some(native_scale), rgba, None)
}

fn submit_persistent_font_job_inner(
    lease: &PersistentGpuFontJob,
    native_scale_override: Option<u32>,
    rgba: GpuFontRgba,
    readback: Option<&mut Option<crate::intel::render::FontRenderTargetReadback>>,
) -> Result<crate::intel::render::RenderJokerResult, &'static str> {
    if lease.released {
        return Err("resident-lease-released");
    }

    // The lock deliberately remains held through synchronous submission. It
    // makes release and submission mutually exclusive without making the
    // physical allocation reference-counted or exposing a CPU pointer.
    let mut service = GPU_FONT_SERVICE.lock();
    let Some(position) = service.resident_jobs.iter().position(|record| {
        record.id == lease.id && record.generation == lease.generation && record.tag == lease.tag
    }) else {
        return Err("resident-lease-stale");
    };
    {
        let record = &mut service.resident_jobs[position];
        if record.quarantined {
            return Err("resident-job-quarantined");
        }
        if record.in_flight {
            return Err("resident-job-in-flight");
        }
        record.in_flight = true;
        record.submits = record.submits.saturating_add(1);
    }
    service.resident_submit_attempts = service.resident_submit_attempts.saturating_add(1);

    let native_scale =
        native_scale_override.unwrap_or(service.resident_jobs[position].native_scale);
    let result = {
        let record = &service.resident_jobs[position];
        if let Some(output) = readback {
            match crate::intel::render::submit_resident_font_mesh_readback_once(
                &record.mesh,
                native_scale,
                rgba,
            ) {
                Ok((render, captured)) => {
                    *output = captured;
                    Ok(render)
                }
                Err(reason) => Err(reason),
            }
        } else {
            crate::intel::render::submit_resident_font_mesh_once(&record.mesh, native_scale, rgba)
        }
    };
    let completed = result.as_ref().is_ok_and(|render| render.completed);
    let (submit_count, gpu_base, resident_bytes) = {
        let record = &mut service.resident_jobs[position];
        record.in_flight = false;
        if result.as_ref().is_ok_and(|render| !render.completed) {
            // A timeout is not permission to free pages potentially still
            // referenced by the engine. Keep them tracked and non-reusable.
            record.quarantined = true;
        }
        (record.submits, record.mesh.gpu_base, record.mesh.storage_bytes)
    };
    if completed {
        service.resident_retired_submits = service.resident_retired_submits.saturating_add(1);
    }
    crate::log_info!(
        target: "render";
        "intel/gpu-font: resident-submit id={} generation={} owner={} name={} authority=borrowed-gpu-resident cpu_geometry_copy=0 geometry_uploads=0 color_authority=per-submit-transient rgba=[{},{},{},{}] attempt={} native_scale={} result={} retired={} quarantined={} gpu=0x{:X} bytes=0x{:X}\n",
        lease.id,
        lease.generation,
        lease.tag.owner,
        lease.tag.name,
        rgba.r,
        rgba.g,
        rgba.b,
        rgba.a,
        submit_count,
        native_scale,
        if result.is_ok() { "draw-returned" } else { "pre-submit-error" },
        completed as u8,
        (result.is_ok() && !completed) as u8,
        gpu_base,
        resident_bytes,
    );
    result
}

fn release_persistent_font_job(
    id: u64,
    generation: u64,
    tag: GpuFontResidencyTag,
) -> Result<(), &'static str> {
    let mut service = GPU_FONT_SERVICE.lock();
    let Some(position) = service
        .resident_jobs
        .iter()
        .position(|record| record.id == id && record.generation == generation && record.tag == tag)
    else {
        return Err("resident-lease-stale");
    };
    if service.resident_jobs[position].in_flight {
        service.resident_release_failures = service.resident_release_failures.saturating_add(1);
        return Err("resident-job-in-flight");
    }
    if service.resident_jobs[position].quarantined {
        service.resident_release_failures = service.resident_release_failures.saturating_add(1);
        crate::log_error!(
            target: "render";
            "intel/gpu-font: resident-release refused id={} generation={} owner={} name={} reason=retirement-uncertain authority=gpu-quarantine tracked=1\n",
            id,
            generation,
            tag.owner,
            tag.name,
        );
        return Err("resident-job-quarantined");
    }

    let gpu_base = service.resident_jobs[position].mesh.gpu_base;
    let resident_bytes = service.resident_jobs[position].mesh.storage_bytes;
    if !crate::intel::render::release_resident_font_mesh(&service.resident_jobs[position].mesh) {
        service.resident_jobs[position].quarantined = true;
        service.resident_release_failures = service.resident_release_failures.saturating_add(1);
        crate::log_error!(
            target: "render";
            "intel/gpu-font: resident-release failed id={} generation={} owner={} name={} reason=ppgtt-unmap authority=gpu-quarantine tracked=1 gpu=0x{:X} bytes=0x{:X}\n",
            id,
            generation,
            tag.owner,
            tag.name,
            gpu_base,
            resident_bytes,
        );
        return Err("resident-unmap-failed");
    }

    let record = service.resident_jobs.swap_remove(position);
    service.resident_releases = service.resident_releases.saturating_add(1);
    crate::log_info!(
        target: "render";
        "intel/gpu-font: resident-release ok=1 id={} generation={} owner={} name={} authority=gpu-resident->unmapped->freed submits={} gpu=0x{:X} bytes=0x{:X} tracked=0\n",
        id,
        generation,
        tag.owner,
        tag.name,
        record.submits,
        gpu_base,
        resident_bytes,
    );
    Ok(())
}

pub(crate) fn resident_status() -> GpuFontResidentStatus {
    let service = GPU_FONT_SERVICE.lock();
    GpuFontResidentStatus {
        active_jobs: service.resident_jobs.len(),
        resident_bytes: service
            .resident_jobs
            .iter()
            .fold(0usize, |total, record| total.saturating_add(record.mesh.storage_bytes)),
        quarantined_jobs: service
            .resident_jobs
            .iter()
            .filter(|record| record.quarantined)
            .count(),
        uploads: service.resident_uploads,
        submit_attempts: service.resident_submit_attempts,
        retired_submits: service.resident_retired_submits,
        releases: service.resident_releases,
        release_failures: service.resident_release_failures,
    }
}

/// Snapshot every live allocation together with its accountable owner tag.
pub(crate) fn resident_audit() -> Vec<GpuFontResidentAuditEntry> {
    GPU_FONT_SERVICE
        .lock()
        .resident_jobs
        .iter()
        .map(|record| GpuFontResidentAuditEntry {
            id: record.id,
            generation: record.generation,
            tag: record.tag,
            font: record.font,
            gpu_base: record.mesh.gpu_base,
            resident_bytes: record.mesh.storage_bytes,
            entries: record.entries,
            text_chars: record.text_chars,
            rows: record.rows,
            glyphs: record.glyphs,
            submits: record.submits,
            in_flight: record.in_flight,
            quarantined: record.quarantined,
        })
        .collect()
}

fn tessellate_text_request(
    request: GpuFontTextRequest<'_>,
    font: GpuFontFace,
) -> Result<(FontTesselMesh, usize, usize), &'static str> {
    tessellate_text_request_with_tolerance(
        request,
        font,
        None,
        crate::graphics::font::FONT_TESSEL_BASE_PX,
        false,
    )
}

fn tessellate_text_request_with_tolerance(
    request: GpuFontTextRequest<'_>,
    font: GpuFontFace,
    tolerance: Option<f32>,
    px_size: f32,
    hinted: bool,
) -> Result<(FontTesselMesh, usize, usize), &'static str> {
    let (layout, normalized, row_lengths) = normalize_text_request(request)?;
    let char_count = normalized.chars().count();
    if normalized.trim().is_empty() {
        return Err("text-empty");
    }
    if char_count > MAX_DYNAMIC_TEXT_CHARS {
        return Err("text-too-long");
    }
    let rows = row_lengths.len();
    let registry_name = if font != GpuFontFace::Default
        && crate::graphics::font::font_summary(font.registry_name()).is_none()
    {
        GpuFontFace::Default.registry_name()
    } else {
        font.registry_name()
    };
    let mesh = match (layout, tolerance, hinted) {
        (GpuFontTextLayout::SingleLine, _, true) => {
            crate::graphics::font::tessellate_text_mesh_hinted(
                registry_name,
                normalized.as_str(),
                px_size,
            )
        }
        (GpuFontTextLayout::Rows, _, true) => {
            crate::graphics::font::tessellate_text_rows_mesh_hinted(
                registry_name,
                normalized.as_str(),
                px_size,
                row_lengths.as_slice(),
            )
        }
        (GpuFontTextLayout::SingleLine, Some(tolerance), false) => {
            crate::graphics::font::tessellate_text_mesh_with_tolerance(
                registry_name,
                normalized.as_str(),
                px_size,
                tolerance,
            )
        }
        (GpuFontTextLayout::Rows, Some(tolerance), false) => {
            crate::graphics::font::tessellate_text_rows_mesh_with_tolerance(
                registry_name,
                normalized.as_str(),
                px_size,
                row_lengths.as_slice(),
                tolerance,
            )
        }
        (GpuFontTextLayout::SingleLine, None, false) => {
            crate::graphics::font::tessellate_text_mesh(registry_name, normalized.as_str(), px_size)
        }
        (GpuFontTextLayout::Rows, None, false) => crate::graphics::font::tessellate_text_rows_mesh(
            registry_name,
            normalized.as_str(),
            px_size,
            row_lengths.as_slice(),
        ),
    };
    if mesh.summary.status != "ok"
        || mesh.summary.tessellate_failures != 0
        || mesh.vertices.is_empty()
        || mesh.indices.is_empty()
        || !mesh.indices.len().is_multiple_of(3)
    {
        return Err(mesh.summary.reason);
    }
    Ok((mesh, char_count, rows))
}

fn normalize_text_request(
    request: GpuFontTextRequest<'_>,
) -> Result<(GpuFontTextLayout, String, Vec<usize>), &'static str> {
    let single_row;
    let (layout, rows): (GpuFontTextLayout, &[&str]) = match request {
        GpuFontTextRequest::SingleLine(text) => {
            single_row = [text];
            (GpuFontTextLayout::SingleLine, &single_row)
        }
        GpuFontTextRequest::Rows(rows) => {
            if rows.is_empty() {
                return Err("rows-empty");
            }
            (GpuFontTextLayout::Rows, rows)
        }
    };

    let capacity = rows
        .iter()
        .fold(0usize, |total, row| total.saturating_add(row.len()));
    let mut normalized = String::with_capacity(capacity);
    let mut row_lengths = Vec::with_capacity(rows.len());
    for row in rows {
        let row_start = normalized.chars().count();
        for ch in row.chars() {
            if is_line_separator(ch) {
                continue;
            }
            if ch.is_control() {
                return Err("text-control-character");
            }
            normalized.push(ch);
        }
        let row_len = normalized.chars().count().saturating_sub(row_start);
        if row_len == 0 {
            return Err("row-empty");
        }
        row_lengths.push(row_len);
    }
    Ok((layout, normalized, row_lengths))
}

const fn is_line_separator(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{000B}' | '\u{000C}' | '\u{0085}' | '\u{2028}' | '\u{2029}')
}

pub(crate) fn cached_default_font_summary() -> Option<FontTesselSummary> {
    GPU_FONT_SERVICE
        .lock()
        .default_font
        .as_ref()
        .map(|cached| cached.mesh.summary.clone())
}

pub(crate) fn cache_status() -> GpuFontCacheStatus {
    let service = GPU_FONT_SERVICE.lock();
    GpuFontCacheStatus {
        ready: service.default_font.is_some(),
        generation: service.generation,
        warm_requests: service.warm_requests,
        cache_hits: service.cache_hits,
        cache_misses: service.cache_misses,
        build_failures: service.build_failures,
        invalidations: service.invalidations,
        geometry_bytes: service
            .default_font
            .as_ref()
            .map(|cached| cached.mesh.summary.geometry_bytes)
            .unwrap_or(0),
    }
}

/// Invalidate only geometry derived from `font_name`.
///
/// A future external-font loader should call this after replacing a registered
/// font. Existing draws remain safe because active users retain an Arc.
pub(crate) fn invalidate_font(font_name: &str, reason: &str) -> bool {
    let mut service = GPU_FONT_SERVICE.lock();
    let matches = service
        .default_font
        .as_ref()
        .is_some_and(|cached| cached.mesh.summary.font_name == font_name);
    if !matches {
        return false;
    }
    service.default_font = None;
    service.invalidations = service.invalidations.saturating_add(1);
    crate::log_info!(
        target: "render";
        "intel/gpu-font: invalidate font={} reason={} invalidations={}\n",
        font_name,
        reason,
        service.invalidations,
    );
    true
}

/// Drop every geometry entry after the underlying font registry changes.
///
/// Use this for font replacement because the new font may not have the same
/// name as the entry currently cached here.
pub(crate) fn invalidate_all(reason: &str) -> bool {
    let mut service = GPU_FONT_SERVICE.lock();
    let Some(cached) = service.default_font.take() else {
        return false;
    };
    service.invalidations = service.invalidations.saturating_add(1);
    crate::log_info!(
        target: "render";
        "intel/gpu-font: invalidate-all previous_font={} reason={} invalidations={}\n",
        cached.mesh.summary.font_name,
        reason,
        service.invalidations,
    );
    true
}

/// Rebuild after changed font data or a future tessellation-policy change.
pub(crate) fn rebuild_default_font(reason: &str) -> Result<GpuFontWarmResult, &'static str> {
    let _ = invalidate_all(reason);
    warm_default_font_once()
}

#[cfg(test)]
mod tests {
    use super::{
        GpuFontJobEntry, GpuFontJobPositioning, GpuFontRasterQuality, GpuFontTextRequest,
        SMALL_FONT_HINT_MAX_RASTER_PX, SMALL_FONT_HINT_MIN_RASTER_PX,
        gpu_font_entries_use_analytical_coverage, small_font_hint_ppem, small_font_optical_bias_px,
        transform_outline_to_raster,
    };

    #[test]
    fn small_font_hint_policy_uses_final_raster_ppem() {
        let quality = GpuFontRasterQuality {
            pixels_per_unit_x: 810.0 / 189.0,
            pixels_per_unit_y: 1_153.0 / 269.0,
        };
        let scene_em = 24.0 / quality.pixels_per_unit_y;
        let ppem = small_font_hint_ppem(scene_em, quality, true).expect("24 px is hinted");
        assert!((ppem - 24.0).abs() < 0.001);

        let min = small_font_hint_ppem(
            SMALL_FONT_HINT_MIN_RASTER_PX / quality.pixels_per_unit_y,
            quality,
            true,
        )
        .expect("lower bound is inclusive");
        assert!((min - SMALL_FONT_HINT_MIN_RASTER_PX).abs() < 0.001);
        let max = small_font_hint_ppem(
            SMALL_FONT_HINT_MAX_RASTER_PX / quality.pixels_per_unit_y,
            quality,
            true,
        )
        .expect("upper bound is inclusive");
        assert!((max - SMALL_FONT_HINT_MAX_RASTER_PX).abs() < 0.001);
        assert!(small_font_hint_ppem(scene_em, quality, false).is_none());
    }

    #[test]
    fn small_font_hint_policy_rejects_large_or_invalid_targets() {
        let quality = GpuFontRasterQuality {
            pixels_per_unit_x: 4.0,
            pixels_per_unit_y: 4.0,
        };
        assert!(small_font_hint_ppem(12.0, quality, true).is_none());
        assert!(
            small_font_hint_ppem(
                6.0,
                GpuFontRasterQuality {
                    pixels_per_unit_x: 0.0,
                    pixels_per_unit_y: 4.0,
                },
                true,
            )
            .is_none()
        );
    }

    #[test]
    fn analytical_coverage_policy_separates_native_and_magnified_gridpaper() {
        let quality = GpuFontRasterQuality {
            pixels_per_unit_x: 810.0 / 189.0,
            pixels_per_unit_y: 1_153.0 / 269.0,
        };
        let native_scene_em = 24.0 / quality.pixels_per_unit_y;
        let native = [GpuFontJobEntry {
            text: GpuFontTextRequest::SingleLine("a"),
            position: [20.0, 30.0],
            font_pixels: native_scene_em,
            slant: 0.0,
        }];
        assert!(gpu_font_entries_use_analytical_coverage(&native, 189, 269, 810, 1_153,));

        let magnified = [GpuFontJobEntry {
            font_pixels: native_scene_em * 2.0,
            ..native[0]
        }];
        assert!(gpu_font_entries_use_analytical_coverage(&magnified, 189, 269, 810, 1_153,));
    }

    #[test]
    fn optical_bias_is_bounded_and_stronger_at_lower_ppem() {
        let low = small_font_optical_bias_px(8.0);
        let regular = small_font_optical_bias_px(24.0);
        let high = small_font_optical_bias_px(32.0);
        assert!(low > regular && regular > high);
        assert!((0.0..=0.35).contains(&low));
        assert!((regular - 0.10).abs() < 0.001);
        assert!((high - 0.04).abs() < 0.001);
    }

    #[test]
    fn warmed_outline_is_centered_and_flipped_into_physical_pixels() {
        let f = f32::to_bits;
        let source = [
            [0, f(0.0), f(0.0), 0, 0, 0, 0, 0],
            [1, f(1_000.0), f(0.0), 0, 0, 0, 0, 0],
            [1, f(1_000.0), f(1_000.0), 0, 0, 0, 0, 0],
            [1, f(0.0), f(1_000.0), 0, 0, 0, 0, 0],
            [4, 0, 0, 0, 0, 0, 0, 0],
        ];
        let entry = GpuFontJobEntry {
            text: GpuFontTextRequest::SingleLine("box"),
            position: [100.0, 50.0],
            font_pixels: 20.0,
            slant: 0.0,
        };
        let (_ops, bounds, flattened_bounds) = transform_outline_to_raster(
            &source,
            1_000,
            entry,
            GpuFontRasterQuality {
                pixels_per_unit_x: 1.0,
                pixels_per_unit_y: 1.0,
            },
            20.0,
            GpuFontJobPositioning::VisualBoundsCenter,
        )
        .unwrap();
        assert_eq!(bounds, (90.0, 40.0, 110.0, 60.0));
        assert_eq!(flattened_bounds, bounds);
    }

    #[test]
    fn flattened_bounds_ignore_cubic_control_point_overshoot() {
        let f = f32::to_bits;
        let ops = [
            [0, f(0.0), f(0.0), 0, 0, 0, 0, 0],
            [3, f(100.0), f(0.0), f(100.0), f(100.0), f(0.0), f(100.0), 0],
            [4, 0, 0, 0, 0, 0, 0, 0],
        ];
        let bounds =
            flattened_outline_bounds(&ops, ANALYTICAL_COVERAGE_CURVE_SUBDIVISIONS).unwrap();
        assert_eq!(bounds, (0.0, 0.0, 75.0, 100.0));
    }
}
