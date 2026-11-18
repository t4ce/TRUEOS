//! Offscreen, low-rate baking and wall-time playback of the Spirit effects.

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;
use trueos_time::{Duration, Timer};

use super::SpiritSurfaceLayout;
use super::spirit_vfx::{SPIRIT_DECORATIVE_CLOCK_SECONDS, SpiritVfxBackgroundEffect};
use crate::intel::gpgpu::{SpiritVfxControl, bake_spirit_background};

mod segments;

const SIZE: usize = 256;
const PITCH: usize = SIZE * 4;
const FRAME_BYTES: usize = SIZE * PITCH;
const FIRST_MODE: u32 = 2;
const MODE_COUNT: usize = 12;
const FRAMES_PER_EFFECT: usize = 16;
const SAMPLE_HZ: f32 = 15.0;
const BAKE_INTERVAL_MS: u64 = 1_000;
const BASE_INTENSITY: f32 = 2.0;
// Keep the finite loop cache available for timing and visual A/B runs.
const USE_ROLLING_SEGMENTS: bool = true;

type Frame = Arc<Vec<u8>>;

#[derive(Copy, Clone, Eq, PartialEq)]
struct Recipe {
    mode: u32,
    scale: u32,
    color_a: u32,
    color_b: u32,
    x: u32,
    y: u32,
}

impl Recipe {
    fn from_control(control: SpiritVfxControl) -> Self {
        Self {
            mode: control.background_mode,
            scale: control.background_scale.to_bits(),
            color_a: control.background_color_a,
            color_b: control.background_color_b,
            x: control.position_x.to_bits(),
            y: control.position_y.to_bits(),
        }
    }

    fn default_for(index: usize) -> Self {
        let mode = FIRST_MODE + index as u32;
        let effect = SpiritVfxBackgroundEffect::from_id(mode as u8).unwrap();
        let (a, b) = effect.demo_colors();
        Self {
            mode,
            scale: effect.default_scale().to_bits(),
            color_a: a.packed_rgb(),
            color_b: b.packed_rgb(),
            x: 0,
            y: 0,
        }
    }

    fn frame_count(self) -> usize {
        // The former clock is a decorative dial. The wall-clock stamp will
        // be a separate producer, so this effect needs only one cached pose.
        if self.mode == 11 {
            1
        } else {
            FRAMES_PER_EFFECT
        }
    }

    fn bake_control(self) -> SpiritVfxControl {
        SpiritVfxControl {
            revision: 0,
            background_mode: self.mode,
            clock_seconds_of_day: SPIRIT_DECORATIVE_CLOCK_SECONDS,
            background_phase_override: None,
            background_scale: f32::from_bits(self.scale),
            background_opacity: 1.0,
            background_speed: 1.0,
            background_intensity: BASE_INTENSITY,
            background_color_a: self.color_a,
            background_color_b: self.color_b,
            position_x: f32::from_bits(self.x),
            position_y: f32::from_bits(self.y),
            rotation_radians: 0.0,
            alpha_cutoff: 0.0,
            edge_fade_pixels: 0.0,
            sampling: 0,
            shader_mode: 0,
            shader_parameters: [0.0; 4],
            fx_color_a: 0,
            fx_color_b: 0,
        }
    }
}

struct Slot {
    recipe: Option<Recipe>,
    frames: [Option<Frame>; FRAMES_PER_EFFECT],
    ready: usize,
}

impl Slot {
    const fn new() -> Self {
        Self {
            recipe: None,
            frames: [const { None }; FRAMES_PER_EFFECT],
            ready: 0,
        }
    }

    fn select(&mut self, recipe: Recipe) {
        if self.recipe != Some(recipe) {
            *self = Self::new();
            self.recipe = Some(recipe);
        }
    }
}

struct Playback {
    recipe: Recipe,
    phase: f32,
    samples: usize,
    last_ms: u64,
    speed: f32,
}

impl Playback {
    fn advance(&mut self, now_ms: u64, speed: f32, ready: usize) {
        let elapsed = now_ms.saturating_sub(self.last_ms) as f32 / 1_000.0;
        self.last_ms = now_ms;
        if self.samples < 2 {
            self.samples = ready;
        } else {
            self.phase += elapsed * SAMPLE_HZ * self.speed;
            let period = ((self.samples - 1) * 2) as f32;
            if self.phase >= period {
                // Adopt newly baked samples at frame zero. Growing the
                // clip in mid-pass would make it jump to a different pose.
                self.samples = ready;
                let extended_period = ((ready - 1) * 2) as f32;
                self.phase = (self.phase - period) % extended_period;
            }
        }
        self.speed = speed;
    }
}

struct Cache {
    slots: [Slot; MODE_COUNT],
    active: Option<Playback>,
    next_warm: usize,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache {
    slots: [const { Slot::new() }; MODE_COUNT],
    active: None,
    next_warm: 0,
});
static STARTED: AtomicBool = AtomicBool::new(false);
static BAKING: AtomicBool = AtomicBool::new(false);
static BAKE_STOPPED: AtomicBool = AtomicBool::new(false);

pub(super) enum Selection {
    Procedural,
    Clean,
    Cached(Sample),
}

pub(super) struct Sample {
    first: Frame,
    second: Frame,
    first_offset: usize,
    second_offset: usize,
    blend: u32,
    gain: u32,
}

impl Sample {
    pub(super) fn copy_into(&self, dst: &mut [u8], layout: SpiritSurfaceLayout) -> bool {
        let pitch = layout.pitch_bytes as usize;
        if layout.width as usize != SIZE
            || layout.height as usize != SIZE
            || pitch < PITCH
            || dst.len() < pitch * SIZE
        {
            return false;
        }
        // Fixed-point blending operates directly on premultiplied BGRA. Use
        // one gain per pixel, capped by alpha so RGB cannot clip separately.
        let gain_by_alpha: [u32; 256] = core::array::from_fn(|alpha| {
            if alpha == 0 {
                0
            } else {
                self.gain.min((255 * 256) / alpha as u32)
            }
        });
        for (y, line) in dst.chunks_exact_mut(pitch).take(SIZE).enumerate() {
            let row_start = y * PITCH;
            let first_start = self.first_offset + row_start;
            let first = &self.first[first_start..first_start + PITCH];
            if self.blend == 0 && self.gain == 256 {
                line[..PITCH].copy_from_slice(first);
                continue;
            }
            let second_start = self.second_offset + row_start;
            let second = &self.second[second_start..second_start + PITCH];
            for (x, pixel) in line[..PITCH].chunks_exact_mut(4).enumerate() {
                let i = x * 4;
                let mix = |ch| {
                    (u32::from(first[i + ch]) * (256 - self.blend)
                        + u32::from(second[i + ch]) * self.blend
                        + 128)
                        >> 8
                };
                let alpha = mix(3);
                let gain = gain_by_alpha[alpha as usize];
                for (channel, byte) in pixel.iter_mut().enumerate() {
                    *byte = ((mix(channel) * gain + 128) >> 8) as u8;
                }
            }
        }
        true
    }
}

/// Bounce through the sampled clip so nonperiodic shaders cannot jump at a
/// wrap. Interpolation keeps the intermediate presentation times continuous.
fn loop_position(phase: f32, count: usize) -> (usize, usize, u32) {
    if count <= 1 {
        return (0, 0, 0);
    }
    let end = (count - 1) as f32;
    let period = end * 2.0;
    let t = phase % period;
    let position = t.min(period - t);
    let first = position as usize;
    (
        first,
        (first + 1).min(count - 1),
        ((position - first as f32) * 256.0) as u32,
    )
}

pub(super) fn select(control: SpiritVfxControl, now_ms: u64) -> Selection {
    if !STARTED.load(Ordering::Acquire) {
        return Selection::Procedural;
    }
    if USE_ROLLING_SEGMENTS {
        return segments::select(control, now_ms);
    }
    let Some(index) = control
        .background_mode
        .checked_sub(FIRST_MODE)
        .map(|index| index as usize)
        .filter(|&index| index < MODE_COUNT)
    else {
        CACHE.lock().active = None;
        return Selection::Clean;
    };
    let recipe = Recipe::from_control(control);
    let mut cache = CACHE.lock();
    cache.slots[index].select(recipe);
    let ready = cache.slots[index].ready;
    let speed = control.background_speed.clamp(0.0, 4.0);
    let playback = match cache.active.take() {
        Some(mut previous) if previous.recipe == recipe => {
            previous.advance(now_ms, speed, ready);
            previous
        }
        _ => Playback {
            recipe,
            phase: 0.0,
            samples: ready,
            last_ms: now_ms,
            speed,
        },
    };
    let sample_position = loop_position(playback.phase, playback.samples);
    cache.active = Some(playback);
    let slot = &cache.slots[index];
    let gain = (control.background_opacity.clamp(0.0, 1.0)
        * (control.background_intensity / BASE_INTENSITY).clamp(0.0, 2.0)
        * 256.0) as u32;
    if gain == 0 {
        return Selection::Clean;
    }
    if slot.ready == 0 {
        return if BAKE_STOPPED.load(Ordering::Acquire) {
            Selection::Procedural
        } else {
            Selection::Clean
        };
    }
    let (first, second, blend) = sample_position;
    Selection::Cached(Sample {
        first: slot.frames[first].as_ref().unwrap().clone(),
        second: slot.frames[second].as_ref().unwrap().clone(),
        first_offset: 0,
        second_offset: 0,
        blend,
        gain,
    })
}

pub(super) fn start() {
    if STARTED.load(Ordering::Acquire) {
        return;
    }
    let Some((_, _, spawner)) = crate::workers::pick_eff_background_spawner_with_slot()
        .or_else(crate::workers::pick_background_spawner_with_slot)
    else {
        return;
    };
    if let Ok(token) = bake_task() {
        STARTED.store(true, Ordering::Release);
        spawner.spawn(token);
        crate::log_info!(target: "gfx";
            "trueos-spirit: background playback policy={} sample_hz={}\n",
            if USE_ROLLING_SEGMENTS { "rolling-segments" } else { "loop-cache" },
            SAMPLE_HZ as u32);
    }
}

pub(super) fn baking() -> bool {
    BAKING.load(Ordering::Acquire)
}

fn next_frame() -> Option<(usize, Recipe, usize)> {
    let mut cache = CACHE.lock();
    if let Some(active) = &cache.active {
        let recipe = active.recipe;
        let index = (recipe.mode - FIRST_MODE) as usize;
        let ready = cache.slots[index].ready;
        if ready < recipe.frame_count() {
            return Some((index, recipe, ready));
        }
    }
    for _ in 0..MODE_COUNT {
        let index = cache.next_warm;
        cache.next_warm = (index + 1) % MODE_COUNT;
        let slot = &mut cache.slots[index];
        let recipe = *slot
            .recipe
            .get_or_insert_with(|| Recipe::default_for(index));
        if slot.ready < recipe.frame_count() {
            return Some((index, recipe, slot.ready));
        }
    }
    None
}

#[trueos_executor::task]
async fn bake_task() {
    if USE_ROLLING_SEGMENTS {
        segments::run().await;
        return;
    }
    // Let the live worker choose its first real background before prewarming.
    Timer::after(Duration::from_millis(BAKE_INTERVAL_MS)).await;
    loop {
        if !super::codex_handoff::active()
            && super::gpu_logger::active_snapshot().is_none()
            && let Some((index, recipe, frame)) = next_frame()
        {
            let mut pixels = Vec::new();
            if pixels.try_reserve_exact(FRAME_BYTES).is_ok() {
                pixels.resize(FRAME_BYTES, 0);
                BAKING.store(true, Ordering::Release);
                let result = bake_spirit_background(
                    recipe.bake_control(),
                    frame as f32 / SAMPLE_HZ,
                    &mut pixels,
                );
                BAKING.store(false, Ordering::Release);
                match result {
                    Ok(()) => {
                        let mut cache = CACHE.lock();
                        let slot = &mut cache.slots[index];
                        if slot.recipe == Some(recipe) && slot.ready == frame {
                            slot.frames[frame] = Some(Arc::new(pixels));
                            slot.ready += 1;
                            if slot.ready == recipe.frame_count() {
                                crate::log_info!(target: "gfx";
                                    "trueos-spirit: background cache ready effect={} frames={} bytes={}\n",
                                    recipe.mode, slot.ready, slot.ready * FRAME_BYTES);
                            }
                        }
                    }
                    Err(-16) => {
                        // Try another frame boundary without consuming the
                        // bake budget. Do not queue behind the live producer.
                        Timer::after(Duration::from_millis(41)).await;
                        continue;
                    }
                    Err(error) => {
                        BAKE_STOPPED.store(true, Ordering::Release);
                        crate::log_warn!(target: "gfx";
                            "trueos-spirit: background bake stopped effect={} error={}\n",
                            recipe.mode, error);
                        return;
                    }
                }
            }
        }
        Timer::after(Duration::from_millis(BAKE_INTERVAL_MS)).await;
    }
}
