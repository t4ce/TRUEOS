//! Rolling background samples. The last image spans the boundary between
//! batches, so there is no wrap blend or reversal.

use alloc::collections::VecDeque;
use trueos_time::Instant;

use super::*;
use crate::intel::gpgpu::{
    SPIRIT_BACKGROUND_SEGMENT_FRAMES as BATCH, bake_spirit_background_segment,
};

const MAX_READY: usize = 2 * BATCH + 1;
const RETRY_MS: u64 = 8;
const REPORT_MS: u64 = 5_000;

#[derive(Clone)]
struct FrameView {
    buffer: Frame,
    offset: usize,
}

struct Stream {
    recipe: Recipe,
    generation: u64,
    frames: VecDeque<FrameView>,
    // Position in display samples relative to the front of frames.
    position: f32,
    next_phase: f64,
    speed: f32,
    selected_at_ms: u64,
    last_sample_ms: u64,
    waiting: bool,
    underruns: u32,
    held_updates: u32,
    cached_updates: u32,
    batches: u32,
    last_report_ms: u64,
}

struct State {
    stream: Option<Stream>,
    dial: Option<(Recipe, FrameView)>,
    generation: u64,
    stopped: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    stream: None,
    dial: None,
    generation: 0,
    stopped: false,
});

impl State {
    fn activate(&mut self, recipe: Recipe, now_ms: u64, start_phase: f64, speed: f32) {
        if self
            .stream
            .as_ref()
            .is_some_and(|stream| stream.recipe == recipe)
        {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        let mut frames = VecDeque::new();
        if let Some((cached, frame)) = &self.dial
            && *cached == recipe
        {
            frames.push_back(frame.clone());
        }
        self.stream = Some(Stream {
            recipe,
            generation: self.generation,
            frames,
            position: 0.0,
            next_phase: start_phase,
            speed,
            selected_at_ms: now_ms,
            last_sample_ms: now_ms,
            waiting: false,
            underruns: 0,
            held_updates: 0,
            cached_updates: 0,
            batches: 0,
            last_report_ms: now_ms,
        });
    }
}

impl Stream {
    fn sample(&mut self, now_ms: u64, speed: f32, gain: u32) -> Option<Sample> {
        let elapsed_ms = now_ms.saturating_sub(self.last_sample_ms);
        self.last_sample_ms = now_ms;
        self.speed = speed;
        if self.frames.is_empty() {
            return None;
        }

        if speed > 0.0 && self.recipe.mode != 11 {
            let target = self.position + elapsed_ms as f32 * (SAMPLE_HZ / 1_000.0);
            let end = (self.frames.len() - 1) as f32;
            // Hold the most recent image when the next segment is late.
            // Resume from that image without dropping the unplayed motion.
            if target > end {
                self.held_updates = self.held_updates.saturating_add(1);
                if !self.waiting {
                    self.underruns = self.underruns.saturating_add(1);
                }
                self.waiting = true;
            } else {
                self.waiting = false;
            }
            self.position = target.min(end);
        }
        let consumed = self.position as usize;
        self.frames.drain(..consumed);
        self.position -= consumed as f32;
        let first = self.frames.front().unwrap();
        let second = self.frames.get(1).unwrap_or(first);
        self.cached_updates = self.cached_updates.saturating_add(1);
        Some(Sample {
            first: first.buffer.clone(),
            second: second.buffer.clone(),
            first_offset: first.offset,
            second_offset: second.offset,
            blend: (self.position * 256.0) as u32,
            gain,
        })
    }
}

pub(super) fn select(control: SpiritVfxControl, now_ms: u64) -> Selection {
    let mut state = STATE.lock();
    if !matches!(control.background_mode, 2..=13) {
        state.stream = None;
        return Selection::Clean;
    }
    let recipe = Recipe::from_control(control);
    let speed = control.background_speed.clamp(0.0, 4.0);
    let phase = f64::from(control.background_phase_override.unwrap_or(0.0).max(0.0));
    state.activate(recipe, now_ms, phase, speed);
    let stopped = state.stopped;
    let stream = state.stream.as_mut().unwrap();
    let gain = (control.background_opacity.clamp(0.0, 1.0)
        * (control.background_intensity / BASE_INTENSITY).clamp(0.0, 2.0)
        * 256.0) as u32;
    let sample = stream.sample(now_ms, speed, gain);
    if now_ms.saturating_sub(stream.last_report_ms) >= REPORT_MS {
        crate::log_info!(target: "gfx";
            "trueos-spirit: background segment playback effect={} window_ms={} batches={} selected_samples={} buffered={} underruns={} held_updates={}\n",
            recipe.mode, now_ms.saturating_sub(stream.last_report_ms), stream.batches,
            stream.cached_updates, stream.frames.len(), stream.underruns, stream.held_updates);
        stream.last_report_ms = now_ms;
        stream.batches = 0;
        stream.cached_updates = 0;
        stream.underruns = 0;
        stream.held_updates = 0;
    }
    if gain == 0 {
        return Selection::Clean;
    }
    match sample {
        Some(sample) => Selection::Cached(sample),
        None if stopped => Selection::Procedural,
        None => Selection::Clean,
    }
}

#[derive(Copy, Clone)]
struct Request {
    recipe: Recipe,
    generation: u64,
    phase: f64,
    step: f64,
    frames: usize,
}

fn request() -> Option<Request> {
    let state = STATE.lock();
    let stream = state.stream.as_ref()?;
    let still = stream.recipe.mode == 11 || stream.speed <= 0.0;
    if stream.frames.len() > MAX_READY - BATCH || (still && !stream.frames.is_empty()) {
        return None;
    }
    Some(Request {
        recipe: stream.recipe,
        generation: stream.generation,
        phase: stream.next_phase,
        step: f64::from(stream.speed) / f64::from(SAMPLE_HZ),
        frames: if still { 1 } else { BATCH },
    })
}

fn publish(request: Request, pixels: Vec<u8>, now_ms: u64) {
    let mut state = STATE.lock();
    let Some(stream) = state
        .stream
        .as_mut()
        .filter(|stream| stream.generation == request.generation)
    else {
        return;
    };
    let buffer = Arc::new(pixels);
    if stream.frames.is_empty() {
        // The first sample is visible as soon as the GPU publishes it.
        stream.last_sample_ms = now_ms;
        crate::log_info!(target: "gfx";
            "trueos-spirit: background segment primed effect={} frames={} startup_ms={}\n",
            request.recipe.mode, request.frames, now_ms.saturating_sub(stream.selected_at_ms));
    }
    for frame in 0..request.frames {
        stream.frames.push_back(FrameView {
            buffer: buffer.clone(),
            offset: frame * FRAME_BYTES,
        });
    }
    stream.next_phase = request.phase + request.step * request.frames as f64;
    stream.batches = stream.batches.saturating_add(1);
    if request.recipe.mode == 11 {
        state.dial = Some((request.recipe, FrameView { buffer, offset: 0 }));
    }
}

pub(super) async fn run() {
    let mut pixels = Vec::new();
    loop {
        if !super::super::codex_handoff::active()
            && super::super::gpu_logger::active_snapshot().is_none()
            && let Some(request) = request()
        {
            let bytes = request.frames * FRAME_BYTES;
            if pixels
                .try_reserve_exact(bytes.saturating_sub(pixels.len()))
                .is_ok()
            {
                pixels.resize(bytes, 0);
                let phases: [f32; BATCH] =
                    core::array::from_fn(|i| (request.phase + request.step * i as f64) as f32);
                BAKING.store(true, Ordering::Release);
                let result = bake_spirit_background_segment(
                    request.recipe.bake_control(),
                    &phases[..request.frames],
                    &mut pixels,
                );
                BAKING.store(false, Ordering::Release);
                match result {
                    Ok(()) => publish(
                        request,
                        core::mem::take(&mut pixels),
                        Instant::now().as_millis(),
                    ),
                    Err(-16) => {}
                    Err(error) => {
                        STATE.lock().stopped = true;
                        crate::log_warn!(target: "gfx";
                            "trueos-spirit: background segment stopped effect={} error={}\n",
                            request.recipe.mode, error);
                        return;
                    }
                }
            }
        }
        Timer::after(Duration::from_millis(RETRY_MS)).await;
    }
}
