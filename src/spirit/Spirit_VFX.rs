//! Stable control-panel contract for Spirit's visual presentation.
//!
//! Names, ranges, and defaults mirror `preview.html`.  The control model is
//! deliberately independent of the current 256x256 Intel cursor-plane backend:
//! UI/service code publishes the complete panel while the bounded 256x256
//! artifacts implement the retained background set and every named Sprite
//! shader from `preview.html`.

extern crate alloc;

use alloc::{format, string::String};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_time::Instant;
use serde::{Deserialize, Serialize};
use spin::Mutex;

#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct SpiritVfxSliderSpec {
    pub(crate) name: &'static str,
    pub(crate) min: f32,
    pub(crate) max: f32,
    pub(crate) step: f32,
    pub(crate) default: f32,
    pub(crate) suffix: &'static str,
}

const fn slider(
    name: &'static str,
    min: f32,
    max: f32,
    step: f32,
    default: f32,
    suffix: &'static str,
) -> SpiritVfxSliderSpec {
    SpiritVfxSliderSpec {
        name,
        min,
        max,
        step,
        default,
        suffix,
    }
}

pub(crate) const TRANSFORM_CONTROLS: [SpiritVfxSliderSpec; 3] = [
    slider("Position X", -0.35, 0.35, 0.005, 0.0, ""),
    slider("Position Y", -0.35, 0.35, 0.005, 0.0, ""),
    slider("Rotation", -360.0, 360.0, 0.5, 180.0, "deg"),
];

pub(crate) const ALPHA_CUTOFF_CONTROL: SpiritVfxSliderSpec =
    slider("Alpha cutoff", 0.0, 0.3, 0.01, 0.02, "");

pub(crate) const EDGE_FADE_CONTROL: SpiritVfxSliderSpec =
    slider("Edge feather", 0.0, 16.0, 0.5, 12.0, " px");

pub(crate) const ALPHA_BACKGROUND_CONTROLS: [SpiritVfxSliderSpec; 4] = [
    slider("Opacity", 0.0, 1.0, 0.01, 0.0, ""),
    slider("Scale", 0.25, 3.0, 0.01, 1.0, "x"),
    slider("Speed", 0.0, 4.0, 0.01, 1.0, "x"),
    slider("Intensity", 0.1, 2.5, 0.01, 1.0, "x"),
];

pub(crate) const PARTICLE_CONTROLS: [SpiritVfxSliderSpec; 4] = [
    slider("Amount", 0.0, 90.0, 1.0, 28.0, ""),
    slider("Size", 0.25, 2.5, 0.01, 1.0, "x"),
    slider("Speed", 0.0, 2.5, 0.01, 0.8, "x"),
    slider("Opacity", 0.0, 1.0, 0.01, 0.75, ""),
];

const NO_EFFECT_CONTROLS: [SpiritVfxSliderSpec; 0] = [];
const AURA_BLOOM_CONTROLS: [SpiritVfxSliderSpec; 4] = [
    slider("Radius", 2.0, 30.0, 0.5, 12.0, " px"),
    slider("Strength", 0.0, 2.5, 0.01, 1.15, "x"),
    slider("Pulse", 0.0, 4.0, 0.01, 1.2, "x"),
    slider("Brighten", 0.0, 1.0, 0.01, 0.18, ""),
];
const NEON_EDGE_CONTROLS: [SpiritVfxSliderSpec; 4] = [
    slider("Width", 0.5, 12.0, 0.1, 3.2, " px"),
    slider("Intensity", 0.0, 2.5, 0.01, 1.35, "x"),
    slider("Flow speed", 0.0, 4.0, 0.01, 1.1, "x"),
    slider("Fill tint", 0.0, 1.0, 0.01, 0.12, ""),
];
const FIRE_RIM_CONTROLS: [SpiritVfxSliderSpec; 4] = [
    slider("Rim width", 1.0, 12.0, 0.1, 3.1, " px"),
    slider("Flame height", 2.0, 34.0, 0.5, 16.0, " px"),
    slider("Turbulence", 0.0, 4.0, 0.01, 1.7, "x"),
    slider("Heat", 0.0, 2.5, 0.01, 1.25, "x"),
];
const ICE_SHIMMER_CONTROLS: [SpiritVfxSliderSpec; 4] = [
    slider("Edge width", 0.5, 12.0, 0.1, 3.4, " px"),
    slider("Shimmer", 0.0, 3.0, 0.01, 1.2, "x"),
    slider("Crystal scale", 2.0, 24.0, 0.1, 10.0, "x"),
    slider("Tint", 0.0, 1.0, 0.01, 0.28, ""),
];
const HOLOGRAM_CONTROLS: [SpiritVfxSliderSpec; 4] = [
    slider("Scan density", 20.0, 220.0, 1.0, 95.0, ""),
    slider("Jitter", 0.0, 12.0, 0.1, 3.5, " px"),
    slider("Flicker", 0.0, 4.0, 0.01, 1.4, "x"),
    slider("Opacity", 0.1, 1.0, 0.01, 0.82, ""),
];
const RGB_GLITCH_CONTROLS: [SpiritVfxSliderSpec; 4] = [
    slider("Separation", 0.0, 10.0, 0.1, 2.7, " px"),
    slider("Slice shift", 0.0, 24.0, 0.1, 8.0, " px"),
    slider("Speed", 0.0, 8.0, 0.01, 2.8, "x"),
    slider("Chaos", 0.0, 1.0, 0.01, 0.36, ""),
];
const DISSOLVE_CONTROLS: [SpiritVfxSliderSpec; 4] = [
    slider("Progress", 0.0, 1.0, 0.01, 0.42, ""),
    slider("Edge width", 0.01, 0.28, 0.005, 0.08, ""),
    slider("Noise scale", 2.0, 22.0, 0.1, 9.5, "x"),
    slider("Emission", 0.0, 3.0, 0.01, 1.45, "x"),
];
const GHOST_TRAIL_CONTROLS: [SpiritVfxSliderSpec; 4] = [
    slider("Trail distance", 1.0, 30.0, 0.5, 11.0, " px"),
    slider("Trail strength", 0.0, 1.8, 0.01, 0.8, "x"),
    slider("Waviness", 0.0, 4.0, 0.01, 1.2, "x"),
    slider("Body opacity", 0.05, 1.0, 0.01, 0.68, ""),
];
const ELECTRIC_ARC_CONTROLS: [SpiritVfxSliderSpec; 4] = [
    slider("Width", 1.0, 14.0, 0.1, 4.1, " px"),
    slider("Arc scale", 2.0, 30.0, 0.1, 14.0, "x"),
    slider("Speed", 0.0, 6.0, 0.01, 2.4, "x"),
    slider("Intensity", 0.0, 3.0, 0.01, 1.65, "x"),
];
const RAINBOW_PRISM_CONTROLS: [SpiritVfxSliderSpec; 4] = [
    slider("Hue speed", 0.0, 3.0, 0.01, 0.55, "x"),
    slider("Band scale", 0.5, 14.0, 0.1, 5.5, "x"),
    slider("Color mix", 0.0, 1.0, 0.01, 0.58, ""),
    slider("Edge width", 0.0, 10.0, 0.1, 2.2, " px"),
];
const HIT_FLASH_CONTROLS: [SpiritVfxSliderSpec; 4] = [
    slider("Flash", 0.0, 1.0, 0.01, 0.82, ""),
    slider("Pulse speed", 0.0, 8.0, 0.01, 2.2, "x"),
    slider("Rim width", 0.0, 14.0, 0.1, 4.5, " px"),
    slider("Shake", 0.0, 8.0, 0.1, 1.5, " px"),
];
const PIXEL_WAVE_CONTROLS: [SpiritVfxSliderSpec; 4] = [
    slider("Amplitude", 0.0, 12.0, 0.1, 3.2, " px"),
    slider("Frequency", 1.0, 30.0, 0.1, 11.0, "x"),
    slider("Speed", 0.0, 6.0, 0.01, 1.7, "x"),
    slider("Color steps", 2.0, 16.0, 1.0, 7.0, ""),
];
const TOON_INK_CONTROLS: [SpiritVfxSliderSpec; 4] = [
    slider("Color steps", 2.0, 16.0, 1.0, 6.0, ""),
    slider("Ink width", 0.5, 8.0, 0.1, 1.7, " px"),
    slider("Saturation", 0.0, 2.0, 0.01, 1.18, "x"),
    slider("Outer rim", 0.0, 8.0, 0.1, 1.2, " px"),
];
const LIQUID_WARP_CONTROLS: [SpiritVfxSliderSpec; 4] = [
    slider("Warp", 0.0, 14.0, 0.1, 4.3, " px"),
    slider("Noise scale", 1.0, 20.0, 0.1, 7.5, "x"),
    slider("Speed", 0.0, 4.0, 0.01, 1.1, "x"),
    slider("Chroma", 0.0, 7.0, 0.1, 1.6, " px"),
];
const DREAM_BLOOM_CONTROLS: [SpiritVfxSliderSpec; 4] = [
    slider("Bloom radius", 2.0, 30.0, 0.5, 13.0, " px"),
    slider("Softness", 0.0, 2.0, 0.01, 0.9, "x"),
    slider("Float speed", 0.0, 3.0, 0.01, 0.65, "x"),
    slider("Pastel mix", 0.0, 1.0, 0.01, 0.28, ""),
];

#[repr(u8)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SpiritVfxSampling {
    #[default]
    Nearest,
    Linear,
}

impl SpiritVfxSampling {
    pub(crate) const fn ui_name(self) -> &'static str {
        match self {
            Self::Nearest => "Pixel crisp",
            Self::Linear => "Smooth",
        }
    }

    const fn wire_name(self) -> &'static str {
        match self {
            Self::Nearest => "nearest",
            Self::Linear => "linear",
        }
    }

    fn from_wire_name(name: &str) -> Option<Self> {
        match name {
            "nearest" => Some(Self::Nearest),
            "linear" => Some(Self::Linear),
            _ => None,
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SpiritVfxEffect {
    #[default]
    OriginalClean = 0,
    AuraBloom = 1,
    NeonEdge = 2,
    FireRim = 3,
    IceShimmer = 4,
    Hologram = 5,
    RgbGlitch = 6,
    Dissolve = 7,
    GhostTrail = 8,
    ElectricArc = 9,
    RainbowPrism = 10,
    HitFlash = 11,
    PixelWave = 12,
    ToonInk = 13,
    LiquidWarp = 14,
    DreamBloom = 15,
}

impl SpiritVfxEffect {
    pub(crate) const fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::OriginalClean),
            1 => Some(Self::AuraBloom),
            2 => Some(Self::NeonEdge),
            3 => Some(Self::FireRim),
            4 => Some(Self::IceShimmer),
            5 => Some(Self::Hologram),
            6 => Some(Self::RgbGlitch),
            7 => Some(Self::Dissolve),
            8 => Some(Self::GhostTrail),
            9 => Some(Self::ElectricArc),
            10 => Some(Self::RainbowPrism),
            11 => Some(Self::HitFlash),
            12 => Some(Self::PixelWave),
            13 => Some(Self::ToonInk),
            14 => Some(Self::LiquidWarp),
            15 => Some(Self::DreamBloom),
            _ => None,
        }
    }

    pub(crate) const fn ui_name(self) -> &'static str {
        match self {
            Self::OriginalClean => "Original / clean",
            Self::AuraBloom => "Aura bloom",
            Self::NeonEdge => "Neon edge",
            Self::FireRim => "Fire rim",
            Self::IceShimmer => "Ice shimmer",
            Self::Hologram => "Hologram",
            Self::RgbGlitch => "RGB glitch",
            Self::Dissolve => "Dissolve",
            Self::GhostTrail => "Ghost trail",
            Self::ElectricArc => "Electric arc",
            Self::RainbowPrism => "Rainbow prism",
            Self::HitFlash => "Hit flash",
            Self::PixelWave => "Pixel wave",
            Self::ToonInk => "Toon ink",
            Self::LiquidWarp => "Liquid warp",
            Self::DreamBloom => "Dream bloom",
        }
    }

    pub(crate) const fn hint(self) -> &'static str {
        match self {
            Self::OriginalClean => "clean pass",
            Self::AuraBloom => "soft animated halo",
            Self::NeonEdge => "two-color rim light",
            Self::FireRim => "rising silhouette flame",
            Self::IceShimmer => "crystalline cyan rim",
            Self::Hologram => "scanlines and dropout",
            Self::RgbGlitch => "channel split and slices",
            Self::Dissolve => "noise cutout with hot edge",
            Self::GhostTrail => "layered spectral echoes",
            Self::ElectricArc => "noisy energized outline",
            Self::RainbowPrism => "animated hue bands",
            Self::HitFlash => "pulsing impact highlight",
            Self::PixelWave => "sine warp and posterize",
            Self::ToonInk => "posterized fill and ink rim",
            Self::LiquidWarp => "organic chromatic refraction",
            Self::DreamBloom => "floaty soft-focus aura",
        }
    }

    pub(crate) const fn controls(self) -> &'static [SpiritVfxSliderSpec] {
        match self {
            Self::OriginalClean => &NO_EFFECT_CONTROLS,
            Self::AuraBloom => &AURA_BLOOM_CONTROLS,
            Self::NeonEdge => &NEON_EDGE_CONTROLS,
            Self::FireRim => &FIRE_RIM_CONTROLS,
            Self::IceShimmer => &ICE_SHIMMER_CONTROLS,
            Self::Hologram => &HOLOGRAM_CONTROLS,
            Self::RgbGlitch => &RGB_GLITCH_CONTROLS,
            Self::Dissolve => &DISSOLVE_CONTROLS,
            Self::GhostTrail => &GHOST_TRAIL_CONTROLS,
            Self::ElectricArc => &ELECTRIC_ARC_CONTROLS,
            Self::RainbowPrism => &RAINBOW_PRISM_CONTROLS,
            Self::HitFlash => &HIT_FLASH_CONTROLS,
            Self::PixelWave => &PIXEL_WAVE_CONTROLS,
            Self::ToonInk => &TOON_INK_CONTROLS,
            Self::LiquidWarp => &LIQUID_WARP_CONTROLS,
            Self::DreamBloom => &DREAM_BLOOM_CONTROLS,
        }
    }

    pub(crate) const fn demo_colors(self) -> (SpiritVfxRgb8, SpiritVfxRgb8) {
        match self {
            Self::OriginalClean => {
                (SpiritVfxRgb8::rgb(0x9A, 0x7C, 0xFF), SpiritVfxRgb8::rgb(0x5E, 0xE7, 0xFF))
            }
            Self::AuraBloom => {
                (SpiritVfxRgb8::rgb(0x8D, 0x6C, 0xFF), SpiritVfxRgb8::rgb(0x5E, 0xE7, 0xFF))
            }
            Self::NeonEdge => {
                (SpiritVfxRgb8::rgb(0xFF, 0x53, 0xD1), SpiritVfxRgb8::rgb(0x5E, 0xE7, 0xFF))
            }
            Self::FireRim => {
                (SpiritVfxRgb8::rgb(0xFF, 0x4D, 0x2E), SpiritVfxRgb8::rgb(0xFF, 0xD3, 0x5A))
            }
            Self::IceShimmer => {
                (SpiritVfxRgb8::rgb(0x70, 0xEA, 0xFF), SpiritVfxRgb8::rgb(0xD7, 0xFB, 0xFF))
            }
            Self::Hologram => {
                (SpiritVfxRgb8::rgb(0x36, 0xE7, 0xFF), SpiritVfxRgb8::rgb(0x85, 0x6C, 0xFF))
            }
            Self::RgbGlitch => {
                (SpiritVfxRgb8::rgb(0xFF, 0x3F, 0x9F), SpiritVfxRgb8::rgb(0x39, 0xF4, 0xFF))
            }
            Self::Dissolve => {
                (SpiritVfxRgb8::rgb(0xFF, 0x6A, 0x2B), SpiritVfxRgb8::rgb(0xFF, 0xE6, 0x6E))
            }
            Self::GhostTrail => {
                (SpiritVfxRgb8::rgb(0xB5, 0x96, 0xFF), SpiritVfxRgb8::rgb(0x59, 0xED, 0xFF))
            }
            Self::ElectricArc => {
                (SpiritVfxRgb8::rgb(0x7B, 0x6C, 0xFF), SpiritVfxRgb8::rgb(0xD8, 0xFB, 0xFF))
            }
            Self::RainbowPrism => {
                (SpiritVfxRgb8::rgb(0xFF, 0x5C, 0xCF), SpiritVfxRgb8::rgb(0x58, 0xEA, 0xFF))
            }
            Self::HitFlash => {
                (SpiritVfxRgb8::rgb(0xFF, 0xFF, 0xFF), SpiritVfxRgb8::rgb(0xFF, 0x4F, 0x76))
            }
            Self::PixelWave => {
                (SpiritVfxRgb8::rgb(0xA8, 0x79, 0xFF), SpiritVfxRgb8::rgb(0x50, 0xE7, 0xFF))
            }
            Self::ToonInk => {
                (SpiritVfxRgb8::rgb(0x3B, 0x27, 0x4F), SpiritVfxRgb8::rgb(0xD9, 0x4D, 0xFF))
            }
            Self::LiquidWarp => {
                (SpiritVfxRgb8::rgb(0x57, 0xF0, 0xDE), SpiritVfxRgb8::rgb(0x8D, 0x6C, 0xFF))
            }
            Self::DreamBloom => {
                (SpiritVfxRgb8::rgb(0xFF, 0x8D, 0xDD), SpiritVfxRgb8::rgb(0x7D, 0xE8, 0xFF))
            }
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SpiritVfxBackgroundEffect {
    #[default]
    Transparent = 0,
    EnergyRing = 2,
    MagicCircle = 3,
    NebulaSmoke = 4,
    CyberGrid = 5,
    PortalVortex = 6,
    SpeedLines = 7,
    BokehField = 8,
    WaterRipples = 9,
    PixelBurst = 10,
}

impl SpiritVfxBackgroundEffect {
    pub(crate) const fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(Self::Transparent),
            2 => Some(Self::EnergyRing),
            3 => Some(Self::MagicCircle),
            4 => Some(Self::NebulaSmoke),
            5 => Some(Self::CyberGrid),
            6 => Some(Self::PortalVortex),
            7 => Some(Self::SpeedLines),
            8 => Some(Self::BokehField),
            9 => Some(Self::WaterRipples),
            10 => Some(Self::PixelBurst),
            _ => None,
        }
    }

    pub(crate) const fn ui_name(self) -> &'static str {
        match self {
            Self::Transparent => "Transparent",
            Self::EnergyRing => "Energy ring",
            Self::MagicCircle => "Magic circle",
            Self::NebulaSmoke => "Nebula smoke",
            Self::CyberGrid => "Cyber grid",
            Self::PortalVortex => "Portal vortex",
            Self::SpeedLines => "Speed lines",
            Self::BokehField => "Bokeh field",
            Self::WaterRipples => "Water ripples",
            Self::PixelBurst => "Pixel burst",
        }
    }

    /// Every retained procedural background is present in the bounded Spirit
    /// OpenCL artifact under its original stable ID.
    pub(crate) const fn artifact_mode(self) -> Option<u32> {
        match self {
            Self::EnergyRing => Some(2),
            Self::MagicCircle => Some(3),
            Self::NebulaSmoke => Some(4),
            Self::CyberGrid => Some(5),
            Self::PortalVortex => Some(6),
            Self::SpeedLines => Some(7),
            Self::BokehField => Some(8),
            Self::WaterRipples => Some(9),
            Self::PixelBurst => Some(10),
            Self::Transparent => None,
        }
    }

    pub(crate) const fn demo_style(self) -> (f32, SpiritVfxRgb8, SpiritVfxRgb8) {
        match self {
            Self::Transparent => {
                (1.0, SpiritVfxRgb8::rgb(0x6F, 0x4C, 0xFF), SpiritVfxRgb8::rgb(0x4D, 0xE7, 0xFF))
            }
            Self::EnergyRing => {
                (1.0, SpiritVfxRgb8::rgb(0xFF, 0x4D, 0xB8), SpiritVfxRgb8::rgb(0x60, 0xED, 0xFF))
            }
            Self::MagicCircle => {
                (1.0, SpiritVfxRgb8::rgb(0x8D, 0x68, 0xFF), SpiritVfxRgb8::rgb(0x6C, 0xF2, 0xFF))
            }
            Self::NebulaSmoke => {
                (1.1, SpiritVfxRgb8::rgb(0x88, 0x3D, 0xFF), SpiritVfxRgb8::rgb(0x30, 0xC8, 0xFF))
            }
            Self::CyberGrid => {
                (1.1, SpiritVfxRgb8::rgb(0x7F, 0x5D, 0xFF), SpiritVfxRgb8::rgb(0x42, 0xEA, 0xFF))
            }
            Self::PortalVortex => {
                (1.0, SpiritVfxRgb8::rgb(0xF1, 0x5F, 0xFF), SpiritVfxRgb8::rgb(0x61, 0xEA, 0xFF))
            }
            Self::SpeedLines => {
                (1.0, SpiritVfxRgb8::rgb(0xFF, 0x4F, 0x8D), SpiritVfxRgb8::rgb(0xFF, 0xE8, 0x6B))
            }
            Self::BokehField => {
                (1.0, SpiritVfxRgb8::rgb(0xFF, 0x8E, 0xDC), SpiritVfxRgb8::rgb(0x75, 0xEA, 0xFF))
            }
            Self::WaterRipples => {
                (1.0, SpiritVfxRgb8::rgb(0x4F, 0x8D, 0xFF), SpiritVfxRgb8::rgb(0x6E, 0xFF, 0xE4))
            }
            Self::PixelBurst => {
                (1.0, SpiritVfxRgb8::rgb(0xB0, 0x6C, 0xFF), SpiritVfxRgb8::rgb(0x5D, 0xEE, 0xFF))
            }
        }
    }
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SpiritVfxParticleType {
    #[default]
    None = 0,
    SmileyFloat = 1,
    Sparkles = 2,
    Hearts = 3,
    Embers = 4,
    Snow = 5,
    MagicOrbs = 6,
    PixelBits = 7,
    Confetti = 8,
}

impl SpiritVfxParticleType {
    pub(crate) const fn ui_name(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::SmileyFloat => "Smiley float",
            Self::Sparkles => "Sparkles",
            Self::Hearts => "Hearts",
            Self::Embers => "Embers",
            Self::Snow => "Snow",
            Self::MagicOrbs => "Magic orbs",
            Self::PixelBits => "Pixel bits",
            Self::Confetti => "Confetti",
        }
    }

    const fn wire_name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::SmileyFloat => "smiley",
            Self::Sparkles => "sparkles",
            Self::Hearts => "hearts",
            Self::Embers => "embers",
            Self::Snow => "snow",
            Self::MagicOrbs => "orbs",
            Self::PixelBits => "pixels",
            Self::Confetti => "confetti",
        }
    }

    fn from_wire_name(name: &str) -> Option<Self> {
        match name {
            "none" => Some(Self::None),
            "smiley" => Some(Self::SmileyFloat),
            "sparkles" => Some(Self::Sparkles),
            "hearts" => Some(Self::Hearts),
            "embers" => Some(Self::Embers),
            "snow" => Some(Self::Snow),
            "orbs" => Some(Self::MagicOrbs),
            "pixels" => Some(Self::PixelBits),
            "confetti" => Some(Self::Confetti),
            _ => None,
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SpiritVfxParticleLayer {
    #[default]
    Back,
    Front,
}

impl SpiritVfxParticleLayer {
    pub(crate) const fn ui_name(self) -> &'static str {
        match self {
            Self::Back => "Behind shader",
            Self::Front => "In front",
        }
    }

    const fn wire_name(self) -> &'static str {
        match self {
            Self::Back => "back",
            Self::Front => "front",
        }
    }

    fn from_wire_name(name: &str) -> Option<Self> {
        match name {
            "back" => Some(Self::Back),
            "front" => Some(Self::Front),
            _ => None,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct SpiritVfxRgb8 {
    pub(crate) red: u8,
    pub(crate) green: u8,
    pub(crate) blue: u8,
}

impl SpiritVfxRgb8 {
    pub(crate) const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }

    pub(crate) const fn packed_rgb(self) -> u32 {
        u32::from_le_bytes([self.red, self.green, self.blue, 0])
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SpiritVfxTransform {
    pub(crate) position_x: f32,
    pub(crate) position_y: f32,
    pub(crate) rotation_degrees: f32,
}

impl Default for SpiritVfxTransform {
    fn default() -> Self {
        Self {
            position_x: 0.0,
            position_y: 0.0,
            // Lilly's resident PNG convention is opposite to the cursor
            // surface's presentation orientation. Keep the correction in the
            // ordinary rotation transform so callers retain the full API.
            rotation_degrees: 180.0,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SpiritVfxSpriteShader {
    pub(crate) effect: SpiritVfxEffect,
    pub(crate) parameters: [f32; 4],
    pub(crate) fx_color_a: SpiritVfxRgb8,
    pub(crate) fx_color_b: SpiritVfxRgb8,
}

impl Default for SpiritVfxSpriteShader {
    fn default() -> Self {
        Self {
            effect: SpiritVfxEffect::OriginalClean,
            parameters: [0.0; 4],
            fx_color_a: SpiritVfxRgb8::rgb(0x9A, 0x7C, 0xFF),
            fx_color_b: SpiritVfxRgb8::rgb(0x5E, 0xE7, 0xFF),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SpiritVfxAlphaBackground {
    pub(crate) effect: SpiritVfxBackgroundEffect,
    pub(crate) opacity: f32,
    pub(crate) scale: f32,
    pub(crate) speed: f32,
    pub(crate) intensity: f32,
    pub(crate) bg_color_a: SpiritVfxRgb8,
    pub(crate) bg_color_b: SpiritVfxRgb8,
}

impl SpiritVfxAlphaBackground {
    pub(crate) const NEBULA_SMOKE: Self = Self {
        effect: SpiritVfxBackgroundEffect::NebulaSmoke,
        opacity: 0.58,
        scale: 1.1,
        speed: 0.45,
        intensity: 1.2,
        bg_color_a: SpiritVfxRgb8::rgb(0x88, 0x3D, 0xFF),
        bg_color_b: SpiritVfxRgb8::rgb(0x30, 0xC8, 0xFF),
    };

    /// Transient background selected by the existing Spirit move API. It is
    /// deliberately not published into the user's persistent control panel.
    const MOVE_PORTAL: Self = Self {
        effect: SpiritVfxBackgroundEffect::PortalVortex,
        opacity: 0.70,
        scale: 1.0,
        speed: 2.0,
        intensity: 2.0,
        bg_color_a: SpiritVfxRgb8::rgb(0xF1, 0x5F, 0xFF),
        bg_color_b: SpiritVfxRgb8::rgb(0x61, 0xEA, 0xFF),
    };
}

impl Default for SpiritVfxAlphaBackground {
    fn default() -> Self {
        Self {
            effect: SpiritVfxBackgroundEffect::Transparent,
            opacity: 0.0,
            scale: 1.0,
            speed: 1.0,
            intensity: 1.0,
            bg_color_a: SpiritVfxRgb8::rgb(0x6F, 0x4C, 0xFF),
            bg_color_b: SpiritVfxRgb8::rgb(0x4D, 0xE7, 0xFF),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SpiritVfxParticlePass {
    pub(crate) particle_type: SpiritVfxParticleType,
    pub(crate) layer: SpiritVfxParticleLayer,
    pub(crate) amount: f32,
    pub(crate) size: f32,
    pub(crate) speed: f32,
    pub(crate) opacity: f32,
    pub(crate) particle_color: SpiritVfxRgb8,
    pub(crate) additive_glow_blend: bool,
}

impl Default for SpiritVfxParticlePass {
    fn default() -> Self {
        Self {
            particle_type: SpiritVfxParticleType::None,
            layer: SpiritVfxParticleLayer::Back,
            amount: 28.0,
            size: 1.0,
            speed: 0.8,
            opacity: 0.75,
            particle_color: SpiritVfxRgb8::rgb(0xFF, 0xE0, 0x6B),
            additive_glow_blend: true,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SpiritVfxControlPanel {
    /// Zero-based `Sprite 1` through `Sprite 4` selection.
    pub(crate) sprite: u8,
    pub(crate) transform: SpiritVfxTransform,
    pub(crate) sampling: SpiritVfxSampling,
    pub(crate) alpha_cutoff: f32,
    /// Final whole-surface premultiplied-alpha feather in cursor pixels.
    pub(crate) edge_fade_pixels: f32,
    pub(crate) sprite_shader: SpiritVfxSpriteShader,
    pub(crate) alpha_background: SpiritVfxAlphaBackground,
    pub(crate) particle_pass: SpiritVfxParticlePass,
    pub(crate) paused: bool,
    pub(crate) output_resolution: u16,
}

impl SpiritVfxControlPanel {
    /// Preview UI defaults, before a recipe or randomization is selected.
    pub(crate) fn preview_default() -> Self {
        Self::default()
    }

    /// Live Spirit starts as the unmodified Lilly presentation. Procedural
    /// background and sprite effects remain available through the same panel.
    pub(crate) fn spirit_live_default() -> Self {
        Self::default()
    }

    fn sanitize(&mut self) {
        self.sprite = self.sprite.min(3);
        self.transform.position_x = bounded(self.transform.position_x, -0.35, 0.35, 0.0);
        self.transform.position_y = bounded(self.transform.position_y, -0.35, 0.35, 0.0);
        self.transform.rotation_degrees =
            bounded(self.transform.rotation_degrees, -360.0, 360.0, 180.0);
        self.alpha_cutoff = bounded(self.alpha_cutoff, 0.0, 0.3, 0.02);
        self.edge_fade_pixels = bounded(self.edge_fade_pixels, 0.0, 16.0, 12.0);

        for (value, spec) in self
            .sprite_shader
            .parameters
            .iter_mut()
            .zip(self.sprite_shader.effect.controls().iter())
        {
            *value = bounded(*value, spec.min, spec.max, spec.default);
        }

        let background = &mut self.alpha_background;
        background.opacity = bounded(background.opacity, 0.0, 1.0, 0.0);
        background.scale = bounded(background.scale, 0.25, 3.0, 1.0);
        background.speed = bounded(background.speed, 0.0, 4.0, 1.0);
        background.intensity = bounded(background.intensity, 0.1, 2.5, 1.0);

        let particles = &mut self.particle_pass;
        particles.amount = bounded(particles.amount, 0.0, 90.0, 28.0);
        particles.size = bounded(particles.size, 0.25, 2.5, 1.0);
        particles.speed = bounded(particles.speed, 0.0, 2.5, 0.8);
        particles.opacity = bounded(particles.opacity, 0.0, 1.0, 0.75);
        self.output_resolution = match self.output_resolution {
            512 | 768 | 1024 => self.output_resolution,
            _ => 512,
        };
    }
}

impl Default for SpiritVfxControlPanel {
    fn default() -> Self {
        Self {
            sprite: 0,
            transform: SpiritVfxTransform::default(),
            sampling: SpiritVfxSampling::Nearest,
            alpha_cutoff: 0.02,
            edge_fade_pixels: 12.0,
            sprite_shader: SpiritVfxSpriteShader::default(),
            alpha_background: SpiritVfxAlphaBackground::default(),
            particle_pass: SpiritVfxParticlePass::default(),
            paused: false,
            output_resolution: 512,
        }
    }
}

/// JSON-facing control contract. Architectural presentation properties such as
/// Lilly's fixed hardware scale are intentionally absent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpiritVfxUiConfig {
    pub(crate) version: u8,
    pub(crate) source_layout: String,
    pub(crate) sprite: u8,
    pub(crate) transform: SpiritVfxUiTransform,
    pub(crate) shader: SpiritVfxUiLayer,
    pub(crate) background: SpiritVfxUiLayer,
    pub(crate) particles: SpiritVfxUiParticles,
    pub(crate) output: SpiritVfxUiOutput,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpiritVfxUiTransform {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) rotation_radians: f32,
    pub(crate) alpha_cutoff: f32,
    pub(crate) sampling: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SpiritVfxUiLayer {
    pub(crate) id: u8,
    pub(crate) name: String,
    pub(crate) params: [f32; 4],
    pub(crate) color_a: String,
    pub(crate) color_b: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SpiritVfxUiParticles {
    #[serde(rename = "type")]
    pub(crate) particle_type: String,
    pub(crate) layer: String,
    pub(crate) params: [f32; 4],
    pub(crate) color: String,
    pub(crate) additive: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SpiritVfxUiOutput {
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) alpha: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum SpiritVfxUiConfigError {
    InvalidJson,
    SerializationFailed,
    UnsupportedVersion,
    UnsupportedSourceLayout,
    InvalidSprite,
    InvalidSampling,
    InvalidShader,
    InvalidBackground,
    InvalidParticleType,
    InvalidParticleLayer,
    InvalidColor,
    InvalidOutput,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum SpiritVfxControlError {
    NonFiniteRotation,
    NegativeRotationDelta,
    NonFiniteEdgeFade,
    InvalidBackgroundMode,
    InvalidShaderMode,
}

impl SpiritVfxUiConfig {
    pub(crate) fn from_control_panel(panel: SpiritVfxControlPanel) -> Self {
        let shader = panel.sprite_shader;
        let background = panel.alpha_background;
        let particles = panel.particle_pass;
        Self {
            version: 1,
            source_layout: String::from("2x2"),
            sprite: panel.sprite + 1,
            transform: SpiritVfxUiTransform {
                x: panel.transform.position_x,
                y: panel.transform.position_y,
                rotation_radians: panel.transform.rotation_degrees.to_radians(),
                alpha_cutoff: panel.alpha_cutoff,
                sampling: String::from(panel.sampling.wire_name()),
            },
            shader: SpiritVfxUiLayer {
                id: shader.effect as u8,
                name: String::from(shader.effect.ui_name()),
                params: shader.parameters,
                color_a: css_color(shader.fx_color_a),
                color_b: css_color(shader.fx_color_b),
            },
            background: SpiritVfxUiLayer {
                id: background.effect as u8,
                name: String::from(background.effect.ui_name()),
                params: [
                    background.opacity,
                    background.scale,
                    background.speed,
                    background.intensity,
                ],
                color_a: css_color(background.bg_color_a),
                color_b: css_color(background.bg_color_b),
            },
            particles: SpiritVfxUiParticles {
                particle_type: String::from(particles.particle_type.wire_name()),
                layer: String::from(particles.layer.wire_name()),
                params: [
                    particles.amount,
                    particles.size,
                    particles.speed,
                    particles.opacity,
                ],
                color: css_color(particles.particle_color),
                additive: particles.additive_glow_blend,
            },
            output: SpiritVfxUiOutput {
                width: panel.output_resolution,
                height: panel.output_resolution,
                alpha: true,
            },
        }
    }

    pub(crate) fn into_control_panel(
        self,
    ) -> Result<SpiritVfxControlPanel, SpiritVfxUiConfigError> {
        if self.version != 1 {
            return Err(SpiritVfxUiConfigError::UnsupportedVersion);
        }
        if self.source_layout != "2x2" {
            return Err(SpiritVfxUiConfigError::UnsupportedSourceLayout);
        }
        if !(1..=4).contains(&self.sprite) {
            return Err(SpiritVfxUiConfigError::InvalidSprite);
        }
        let sampling = SpiritVfxSampling::from_wire_name(&self.transform.sampling)
            .ok_or(SpiritVfxUiConfigError::InvalidSampling)?;
        let effect = SpiritVfxEffect::from_id(self.shader.id)
            .filter(|effect| self.shader.name == effect.ui_name())
            .ok_or(SpiritVfxUiConfigError::InvalidShader)?;
        let background_effect = SpiritVfxBackgroundEffect::from_id(self.background.id)
            .filter(|effect| self.background.name == effect.ui_name())
            .ok_or(SpiritVfxUiConfigError::InvalidBackground)?;
        let particle_type = SpiritVfxParticleType::from_wire_name(&self.particles.particle_type)
            .ok_or(SpiritVfxUiConfigError::InvalidParticleType)?;
        let particle_layer = SpiritVfxParticleLayer::from_wire_name(&self.particles.layer)
            .ok_or(SpiritVfxUiConfigError::InvalidParticleLayer)?;
        if self.output.width != self.output.height
            || !matches!(self.output.width, 512 | 768 | 1024)
            || !self.output.alpha
        {
            return Err(SpiritVfxUiConfigError::InvalidOutput);
        }

        let mut panel = SpiritVfxControlPanel {
            sprite: self.sprite - 1,
            transform: SpiritVfxTransform {
                position_x: self.transform.x,
                position_y: self.transform.y,
                rotation_degrees: self.transform.rotation_radians.to_degrees(),
            },
            sampling,
            alpha_cutoff: self.transform.alpha_cutoff,
            edge_fade_pixels: 12.0,
            sprite_shader: SpiritVfxSpriteShader {
                effect,
                parameters: self.shader.params,
                fx_color_a: parse_css_color(&self.shader.color_a)?,
                fx_color_b: parse_css_color(&self.shader.color_b)?,
            },
            alpha_background: SpiritVfxAlphaBackground {
                effect: background_effect,
                opacity: self.background.params[0],
                scale: self.background.params[1],
                speed: self.background.params[2],
                intensity: self.background.params[3],
                bg_color_a: parse_css_color(&self.background.color_a)?,
                bg_color_b: parse_css_color(&self.background.color_b)?,
            },
            particle_pass: SpiritVfxParticlePass {
                particle_type,
                layer: particle_layer,
                amount: self.particles.params[0],
                size: self.particles.params[1],
                speed: self.particles.params[2],
                opacity: self.particles.params[3],
                particle_color: parse_css_color(&self.particles.color)?,
                additive_glow_blend: self.particles.additive,
            },
            paused: false,
            output_resolution: self.output.width,
        };
        panel.sanitize();
        Ok(panel)
    }
}

pub(crate) fn control_panel_ui_json() -> Result<String, SpiritVfxUiConfigError> {
    let (_, panel) = control_panel_snapshot();
    serde_json::to_string(&SpiritVfxUiConfig::from_control_panel(panel))
        .map_err(|_| SpiritVfxUiConfigError::SerializationFailed)
}

pub(crate) fn publish_control_panel_ui_json(json: &str) -> Result<u64, SpiritVfxUiConfigError> {
    let config: SpiritVfxUiConfig =
        serde_json::from_str(json).map_err(|_| SpiritVfxUiConfigError::InvalidJson)?;
    Ok(publish_control_panel(config.into_control_panel()?))
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub(super) struct SpiritVfxGpuSnapshot {
    pub(super) revision: u64,
    pub(super) background_mode: u32,
    pub(super) opacity: f32,
    pub(super) background_scale: f32,
    pub(super) speed: f32,
    pub(super) intensity: f32,
    pub(super) color_a: u32,
    pub(super) color_b: u32,
    pub(super) position_x: f32,
    pub(super) position_y: f32,
    pub(super) rotation_radians: f32,
    pub(super) alpha_cutoff: f32,
    pub(super) edge_fade_pixels: f32,
    pub(super) sampling: u32,
    pub(super) shader_mode: u32,
    pub(super) shader_parameters: [f32; 4],
    pub(super) fx_color_a: u32,
    pub(super) fx_color_b: u32,
}

static CONTROL_PANEL: Mutex<Option<SpiritVfxControlPanel>> = Mutex::new(None);
static CONTROL_PANEL_REVISION: AtomicU64 = AtomicU64::new(1);
static MOVE_PORTAL_ACTIVE: AtomicBool = AtomicBool::new(false);
static MOVE_PORTAL_STARTED_MS: AtomicU64 = AtomicU64::new(0);

pub(crate) fn control_panel_snapshot() -> (u64, SpiritVfxControlPanel) {
    let panel = CONTROL_PANEL
        .lock()
        .unwrap_or_else(SpiritVfxControlPanel::spirit_live_default);
    (CONTROL_PANEL_REVISION.load(Ordering::Acquire), panel)
}

/// Atomically publish one complete UI snapshot.  Values are bounded at this
/// service edge; the GPGPU control-page writer validates them again.
pub(crate) fn publish_control_panel(mut panel: SpiritVfxControlPanel) -> u64 {
    panel.sanitize();
    *CONTROL_PANEL.lock() = Some(panel);
    CONTROL_PANEL_REVISION.fetch_add(1, Ordering::AcqRel) + 1
}

/// Select one stable C++ Spirit combination with the authored demo defaults.
/// This is intentionally a control-panel publication, so the live 60 Hz task
/// observes it without changing ownership, walker shape, or cursor lifecycle.
pub(crate) fn select_cpp_repass(
    background_id: u8,
    shader_id: u8,
) -> Result<u64, SpiritVfxControlError> {
    let background = SpiritVfxBackgroundEffect::from_id(background_id)
        .ok_or(SpiritVfxControlError::InvalidBackgroundMode)?;
    let shader =
        SpiritVfxEffect::from_id(shader_id).ok_or(SpiritVfxControlError::InvalidShaderMode)?;
    let mut panel = control_panel_snapshot().1;
    let (scale, bg_color_a, bg_color_b) = background.demo_style();
    panel.alpha_background = SpiritVfxAlphaBackground {
        effect: background,
        opacity: if background == SpiritVfxBackgroundEffect::Transparent {
            0.0
        } else {
            0.82
        },
        scale,
        speed: 1.0,
        intensity: 1.0,
        bg_color_a,
        bg_color_b,
    };

    let mut parameters = [0.0; 4];
    for (value, spec) in parameters.iter_mut().zip(shader.controls().iter()) {
        *value = spec.default;
    }
    let (fx_color_a, fx_color_b) = shader.demo_colors();
    panel.sprite_shader = SpiritVfxSpriteShader {
        effect: shader,
        parameters,
        fx_color_a,
        fx_color_b,
    };
    Ok(publish_control_panel(panel))
}

pub(crate) fn reset_cpp_repass() -> u64 {
    publish_control_panel(SpiritVfxControlPanel::spirit_live_default())
}

/// Temporarily replace only the background presented to the GPU. The movement
/// state machine owns this bit; persistent UI/VFX settings are restored
/// automatically when the transition ends.
pub(super) fn set_move_portal_transition(active: bool) {
    if active {
        MOVE_PORTAL_STARTED_MS.store(Instant::now().as_millis(), Ordering::Release);
    }
    if MOVE_PORTAL_ACTIVE.swap(active, Ordering::AcqRel) != active {
        CONTROL_PANEL_REVISION.fetch_add(1, Ordering::AcqRel);
    }
}

/// Set an absolute Spirit sprite rotation. Any finite number of turns is
/// accepted and reduced to one signed revolution before the next VFX frame.
pub(crate) fn set_rotation_degrees(degrees: f32) -> Result<u64, SpiritVfxControlError> {
    if !degrees.is_finite() {
        return Err(SpiritVfxControlError::NonFiniteRotation);
    }
    Ok(update_rotation_degrees(canonical_rotation_degrees(degrees)))
}

/// Rotate counter-clockwise relative to the latest published panel.
pub(crate) fn rotate_left_degrees(degrees: f32) -> Result<u64, SpiritVfxControlError> {
    rotate_relative_degrees(-validate_rotation_delta(degrees)?)
}

/// Rotate clockwise relative to the latest published panel.
pub(crate) fn rotate_right_degrees(degrees: f32) -> Result<u64, SpiritVfxControlError> {
    rotate_relative_degrees(validate_rotation_delta(degrees)?)
}

pub(crate) fn rotation_degrees() -> f32 {
    control_panel_snapshot().1.transform.rotation_degrees
}

/// Set the width of the final whole-cursor alpha feather. Zero disables it;
/// values above sixteen pixels are intentionally bounded at the service edge.
pub(crate) fn set_edge_fade_pixels(pixels: f32) -> Result<u64, SpiritVfxControlError> {
    if !pixels.is_finite() {
        return Err(SpiritVfxControlError::NonFiniteEdgeFade);
    }
    let mut panel_slot = CONTROL_PANEL.lock();
    let mut panel = panel_slot.unwrap_or_else(SpiritVfxControlPanel::spirit_live_default);
    panel.edge_fade_pixels = pixels;
    panel.sanitize();
    *panel_slot = Some(panel);
    Ok(CONTROL_PANEL_REVISION.fetch_add(1, Ordering::AcqRel) + 1)
}

pub(super) fn gpu_snapshot() -> SpiritVfxGpuSnapshot {
    let (revision, panel) = control_panel_snapshot();
    let move_portal_active = MOVE_PORTAL_ACTIVE.load(Ordering::Acquire);
    let background = if move_portal_active {
        let elapsed_ms = Instant::now()
            .as_millis()
            .saturating_sub(MOVE_PORTAL_STARTED_MS.load(Ordering::Acquire));
        let mut background = SpiritVfxAlphaBackground::MOVE_PORTAL;
        let ramp = move_portal_ramp(elapsed_ms);
        background.scale = 0.25 + (background.scale - 0.25) * ramp;
        background.speed *= ramp;
        background.intensity = 0.5 + (background.intensity - 0.5) * ramp;
        background
    } else {
        panel.alpha_background
    };
    SpiritVfxGpuSnapshot {
        revision,
        background_mode: background.effect.artifact_mode().unwrap_or(0),
        opacity: background.opacity,
        background_scale: background.scale,
        speed: background.speed,
        intensity: background.intensity,
        color_a: background.bg_color_a.packed_rgb(),
        color_b: background.bg_color_b.packed_rgb(),
        position_x: panel.transform.position_x,
        position_y: panel.transform.position_y,
        rotation_radians: panel.transform.rotation_degrees.to_radians(),
        alpha_cutoff: panel.alpha_cutoff,
        edge_fade_pixels: panel.edge_fade_pixels,
        sampling: panel.sampling as u32,
        shader_mode: panel.sprite_shader.effect as u32,
        shader_parameters: panel.sprite_shader.parameters,
        fx_color_a: panel.sprite_shader.fx_color_a.packed_rgb(),
        fx_color_b: panel.sprite_shader.fx_color_b.packed_rgb(),
    }
}

fn move_portal_ramp(elapsed_ms: u64) -> f32 {
    if elapsed_ms < super::SPIRIT_MOVE_PORTAL_RAMP_MS {
        elapsed_ms as f32 / super::SPIRIT_MOVE_PORTAL_RAMP_MS as f32
    } else if elapsed_ms < super::SPIRIT_MOVE_PORTAL_RAMP_MS + super::SPIRIT_MOVE_PORTAL_HOLD_MS {
        1.0
    } else if elapsed_ms < super::SPIRIT_MOVE_PORTAL_TOTAL_MS {
        (super::SPIRIT_MOVE_PORTAL_TOTAL_MS - elapsed_ms) as f32
            / super::SPIRIT_MOVE_PORTAL_RAMP_MS as f32
    } else {
        0.0
    }
}

fn bounded(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

fn validate_rotation_delta(degrees: f32) -> Result<f32, SpiritVfxControlError> {
    if !degrees.is_finite() {
        return Err(SpiritVfxControlError::NonFiniteRotation);
    }
    if degrees < 0.0 {
        return Err(SpiritVfxControlError::NegativeRotationDelta);
    }
    Ok(degrees)
}

fn rotate_relative_degrees(delta: f32) -> Result<u64, SpiritVfxControlError> {
    let mut panel_slot = CONTROL_PANEL.lock();
    let mut panel = panel_slot.unwrap_or_else(SpiritVfxControlPanel::spirit_live_default);
    panel.transform.rotation_degrees =
        canonical_rotation_degrees(panel.transform.rotation_degrees + delta);
    panel.sanitize();
    *panel_slot = Some(panel);
    Ok(CONTROL_PANEL_REVISION.fetch_add(1, Ordering::AcqRel) + 1)
}

fn update_rotation_degrees(degrees: f32) -> u64 {
    let mut panel_slot = CONTROL_PANEL.lock();
    let mut panel = panel_slot.unwrap_or_else(SpiritVfxControlPanel::spirit_live_default);
    panel.transform.rotation_degrees = degrees;
    panel.sanitize();
    *panel_slot = Some(panel);
    CONTROL_PANEL_REVISION.fetch_add(1, Ordering::AcqRel) + 1
}

fn canonical_rotation_degrees(degrees: f32) -> f32 {
    let wrapped = degrees % 360.0;
    if wrapped == -0.0 { 0.0 } else { wrapped }
}

fn css_color(color: SpiritVfxRgb8) -> String {
    format!("#{:02x}{:02x}{:02x}", color.red, color.green, color.blue)
}

fn parse_css_color(value: &str) -> Result<SpiritVfxRgb8, SpiritVfxUiConfigError> {
    let bytes = value.as_bytes();
    if bytes.len() != 7 || bytes[0] != b'#' {
        return Err(SpiritVfxUiConfigError::InvalidColor);
    }
    let byte = |high, low| {
        let high = hex_nibble(bytes[high])?;
        let low = hex_nibble(bytes[low])?;
        Some((high << 4) | low)
    };
    Ok(SpiritVfxRgb8::rgb(
        byte(1, 2).ok_or(SpiritVfxUiConfigError::InvalidColor)?,
        byte(3, 4).ok_or(SpiritVfxUiConfigError::InvalidColor)?,
        byte(5, 6).ok_or(SpiritVfxUiConfigError::InvalidColor)?,
    ))
}

const fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}
