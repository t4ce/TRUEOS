//! Font color sampling and instance animation descriptions.
//! GPU allocation, job leases, and frame scheduling belong to the font service.

use super::GpuFontRgba;

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

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const RED: Self = Self(Self::RED_BIT);
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const GREEN: Self = Self(Self::GREEN_BIT);
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub(crate) const BLUE: Self = Self(Self::BLUE_BIT);
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    Static(GpuFontRgba),
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    Transition(GpuFontColorTransition),
    Keyframes(GpuFontColorKeyframes),
}

impl GpuFontColorProgram {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

