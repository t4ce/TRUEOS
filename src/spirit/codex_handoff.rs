//! Lilly-owned procedural Codex arrival. UI4 never writes the cursor surface.
//! Only the single Spirit producer writes the persistent source scratch, after
//! the preceding GPU source read has retired. Native logo stamping uses the
//! completed back buffer under Spirit's ordered GPU-then-CPU producer barrier.
extern crate alloc;
use super::lilly::{self, LillyResidentFrame};
use crate::ui4::{WindowId, WindowOwner};
use alloc::vec::Vec;
use spin::Mutex;
#[path = "codex_handoff/geometry.rs"]
pub(crate) mod geometry;
use geometry::{Point, Pose};

pub(crate) const ICON: &[u8; 64 * 64 * 4] = include_bytes!("../../tools/lilly/codex-tile64.rgba");
const SOURCE_GPU: u64 = 0x3ffc_0000;
const BYTES: usize = 256 * 256 * 4;
const WISH_DURATIONS: [u64; 9] = [450, 180, 180, 180, 180, 160, 240, 220, 450];
const WISH_PATHS: [&str; 9] = [
    "Static_up/codex.png",
    "Static_up/wish-c/frame_02.png",
    "Static_up/wish-c/frame_03.png",
    "Static_up/wish-c/frame_04.png",
    "Static_up/wish-c/frame_05.png",
    "Static_up/wish-c/frame_06.png",
    "Static_up/wish-c/frame_07.png",
    "Static_up/wish-c/frame_08.png",
    "Static_up/static.png",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Key {
    pub owner: WindowOwner,
    pub window: WindowId,
    pub icon: crate::ui4::FrameHandle,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Visual {
    pub key: Key,
    pub pose: Pose,
    pub hidden: bool,
    pub opacity: u8,
}

#[derive(Clone, Copy)]
pub(crate) struct Source {
    pub surface: LillyResidentFrame,
    pub stamp: Option<Stamp>,
}

#[derive(Clone, Copy)]
pub(crate) struct Stamp {
    visual: Visual,
    sequence: u64,
    opacity: u8,
}
impl Stamp {
    pub(crate) fn apply(self, pixels: &mut [u8], layout: super::SpiritSurfaceLayout) -> bool {
        let s = STATE.lock();
        if !s
            .pending
            .is_some_and(|(seq, v)| seq == self.sequence && v == self.visual)
        {
            return true; // canceled or replaced while the GPU was producing
        }
        let pitch = layout.pitch_bytes as usize;
        if layout.width != 256
            || layout.height != 256
            || pitch < 1024
            || pitch
                .checked_mul(256)
                .is_none_or(|bytes| bytes > pixels.len() || bytes > layout.byte_len)
        {
            return false;
        }
        geometry::cursor_bgra_part(self.visual.pose, ICON, self.opacity, pixels, pitch);
        true
    }
}

struct Arrival {
    key: Key,
    start: Point,
    started: u64,
    duration: u64,
    crossed: bool,
    landed: Option<u64>,
    opacity: u8,
    entered: bool,
}
struct State {
    active: Option<Arrival>,
    committed: Option<Visual>,
    pending: Option<(u64, Visual)>,
    scratch: Option<crate::gpu::resident::ResidentDmaBuffer>,
    pixels: Vec<u8>,
    last_software: Option<Visual>,
}
impl State {
    const fn new() -> Self {
        Self {
            active: None,
            committed: None,
            pending: None,
            scratch: None,
            pixels: Vec::new(),
            last_software: None,
        }
    }
}
static STATE: Mutex<State> = Mutex::new(State::new());

pub(crate) fn begin(key: Key, start: Point, opacity: u8) -> bool {
    if super::gpu_logger::active_snapshot().is_some() {
        return false;
    }
    let mut state = STATE.lock();
    if state.active.is_some() {
        return false;
    }
    let Some((_, _, x, y, 256, 256)) = super::marker_anchor() else {
        return false;
    };
    let distance = (i64::from(start.x) - (i64::from(x) + i64::from(geometry::CHEST.x)))
        .unsigned_abs()
        .max((i64::from(start.y) - (i64::from(y) + i64::from(geometry::CHEST.y))).unsigned_abs());
    state.active = Some(Arrival {
        key,
        start,
        // Settlement belongs to this detached sprite, never a compact window.
        started: trueos_time::Instant::now().as_millis().saturating_add(500),
        duration: (distance / 2).clamp(320, 1100),
        crossed: false,
        landed: None,
        opacity,
        entered: false,
    });
    state.last_software = None;
    true
}
pub(crate) fn needs_crossed() -> bool {
    STATE.lock().active.is_some()
}
pub(crate) fn crossed_ready() {
    if let Some(a) = STATE.lock().active.as_mut() {
        a.crossed = true;
    }
}
pub(crate) fn active() -> bool {
    STATE.lock().active.is_some()
}
pub(crate) fn cancel(owner: WindowOwner, window: WindowId) {
    let mut s = STATE.lock();
    if s.active
        .as_ref()
        .is_some_and(|a| a.key.owner == owner && a.key.window == window)
    {
        s.active = None;
        s.last_software = None;
    }
    if s.committed
        .is_some_and(|v| v.key.owner == owner && v.key.window == window)
    {
        s.committed = None;
    }
    if s.pending
        .is_some_and(|(_, v)| v.key.owner == owner && v.key.window == window)
    {
        s.pending = None;
    }
}
#[cfg(test)]
pub(crate) fn visual() -> Option<Visual> {
    STATE.lock().committed
}

/// Absolute-time motion sampled by slot4 at its own display cadence. Outside
/// the cursor plane it requires no sprite package, GPU submission or window.
pub(crate) fn software_visual() -> Option<Visual> {
    let now = trueos_time::Instant::now().as_millis();
    let paused = super::gpu_logger::active_snapshot().is_some();
    let (_, _, cx, cy, 256, 256) = super::marker_anchor()? else {
        return None;
    };
    let mut s = STATE.lock();
    let a = s.active.as_ref()?;
    if a.landed.is_some() {
        return s.committed.filter(|v| v.key == a.key && !v.hidden);
    }
    let pose = geometry::slide(
        a.start,
        Point { x: cx, y: cy },
        now.saturating_sub(a.started),
        a.duration,
    );
    let v = Visual {
        key: a.key,
        pose,
        hidden: false,
        opacity: a.opacity,
    };
    if paused {
        return s
            .committed
            .filter(|v| v.key == a.key && !v.hidden)
            .or(s.last_software)
            .or(Some(v));
    }
    if a.entered || pose.near_cursor(0) {
        // Only the short plane overlap follows cursor acknowledgements. Hold
        // the last complete outside pose until the first matching latch.
        return s
            .committed
            .filter(|v| v.key == a.key && !v.hidden)
            .or(s.last_software);
    }
    s.last_software = Some(v);
    Some(v)
}

/// A temporary display-rate producer while the two planes share the sprite.
/// Ordinary Lilly animation keeps its existing cadence and frame timings.
pub(crate) fn fast_updates() -> bool {
    let Some((_, _, cx, cy, 256, 256)) = super::marker_anchor() else {
        return false;
    };
    let s = STATE.lock();
    s.active.as_ref().is_some_and(|a| {
        a.landed.is_none()
            && (a.entered
                || geometry::slide(
                    a.start,
                    Point { x: cx, y: cy },
                    trueos_time::Instant::now()
                        .as_millis()
                        .saturating_sub(a.started),
                    a.duration,
                )
                .near_cursor(48))
    })
}
/// Commit the software half only after the matching cursor source is SURFLIVE.
pub(crate) fn presented(sequence: u64) {
    let changed = {
        let mut s = STATE.lock();
        match s.pending {
            Some((seq, v)) if seq == sequence => {
                s.committed = Some(v);
                s.pending = None;
                Some(v.key)
            }
            _ => None,
        }
    };
    if let Some(key) = changed {
        crate::ui4::codex_handoff_changed(key.owner, key.window);
    }
}
pub(super) fn shift_deadlines(paused_ms: u64) {
    if let Some(a) = STATE.lock().active.as_mut() {
        a.started = a.started.saturating_add(paused_ms);
        a.landed = a.landed.map(|t| t.saturating_add(paused_ms));
    }
}

/// Called by the single normal Spirit producer after its previous GPU read.
pub(super) fn source(default: LillyResidentFrame, sequence: u64) -> Option<Source> {
    let now = trueos_time::Instant::now().as_millis();
    let (_, _, cx, cy, 256, 256) = super::marker_anchor()? else {
        return None;
    };
    let mut s = STATE.lock();
    let a = s.active.as_mut()?;
    let elapsed = now.saturating_sub(a.started);
    let pose = geometry::slide(a.start, Point { x: cx, y: cy }, elapsed, a.duration);
    if elapsed >= a.duration && a.crossed && a.landed.is_none() {
        a.landed = Some(now);
    }
    let key = a.key;
    let opacity = a.opacity;
    let wish = a.landed.map(|t| now.saturating_sub(t));
    if wish.is_none() && !a.entered && !pose.near_cursor(0) {
        return None;
    }
    a.entered = true;
    let mut path = None;
    if let Some(wish) = wish {
        let mut time = 0;
        for (i, d) in WISH_DURATIONS.iter().enumerate() {
            time += d;
            if wish < time {
                path = Some(WISH_PATHS[i]);
                break;
            }
        }
        if path.is_none() {
            s.active = None;
            s.pending = Some((
                sequence,
                Visual {
                    key,
                    pose,
                    hidden: true,
                    opacity,
                },
            ));
            return Some(Source {
                surface: lilly::resident_frame("Static_up/static.png")?,
                stamp: None,
            });
        }
    }
    let frame = path.and_then(lilly::resident_frame).unwrap_or(default);
    let surface = if wish.is_some_and(|t| t < 100) {
        // Crossfade the two character sources before their ordinary VFX.
        // The native logo is stamped afterwards and fades out independently.
        let t = wish.unwrap();
        if s.scratch.is_none() {
            s.scratch = Some(crate::gpu::resident::ResidentDmaBuffer::allocate_zeroed(
                BYTES, 4096,
            )?);
        }
        s.pixels.resize(BYTES, 0);
        if !lilly::copy_frame(frame, &mut s.pixels) {
            return None;
        }
        let mut native = alloc::vec![0;BYTES];
        if !lilly::copy_frame(lilly::resident_frame("Static_up/static.png")?, &mut native) {
            return None;
        }
        for (out, from) in s.pixels.iter_mut().zip(native) {
            *out = ((u64::from(from) * (100 - t) + u64::from(*out) * t + 50) / 100) as u8;
        }
        let dma = s.scratch.as_ref()?;
        if !dma.write(0, &s.pixels) {
            return None;
        }
        dma.flush();
        LillyResidentFrame {
            phys: dma.phys(),
            gpu: SOURCE_GPU,
            bytes: BYTES,
            width: 256,
            height: 256,
            pitch_bytes: 1024,
        }
    } else {
        frame
    };
    let visual = Visual {
        key,
        pose,
        hidden: wish.is_some(),
        opacity,
    };
    s.pending = Some((sequence, visual));
    let stamp_opacity = match wish {
        None => Some(opacity),
        Some(t) if t < 100 => Some(((u64::from(opacity) * (100 - t) + 50) / 100) as u8),
        _ => None,
    };
    Some(Source {
        surface,
        stamp: stamp_opacity.map(|opacity| Stamp {
            visual,
            sequence,
            opacity,
        }),
    })
}
