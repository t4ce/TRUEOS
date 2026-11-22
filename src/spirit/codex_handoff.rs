//! Lilly-owned procedural Codex arrival. UI4 never writes the cursor surface.
//! The cursor producer stamps the native tile after copying the resident sprite
//! into its writable back buffer.
use super::lilly::{self, LillyResidentFrame};
use crate::ui4::{WindowId, WindowOwner};
use spin::Mutex;
#[path = "codex_handoff/geometry.rs"]
pub(crate) mod geometry;
use geometry::{Point, Pose};

pub(crate) const ICON: &[u8; 64 * 64 * 4] = include_bytes!("../../tools/lilly/codex-tile64.rgba");
const TILE_FADE_MS: u64 = 200;
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
    resample_motion: bool,
}
impl Stamp {
    pub(crate) fn apply(self, pixels: &mut [u8], layout: super::SpiritSurfaceLayout) -> bool {
        let anchor = if self.resample_motion && super::gpu_logger::active_snapshot().is_none() {
            super::marker_anchor()
        } else {
            None
        };
        let mut s = STATE.lock();
        if !s
            .pending
            .is_some_and(|(seq, v)| seq == self.sequence && v == self.visual)
        {
            return true; // canceled or replaced before the tile was stamped
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
        let mut visual = self.visual;
        if let Some((_, _, cx, cy, 256, 256)) = anchor
            && let Some(a) = s.active.as_ref().filter(|a| a.key == visual.key)
        {
            // Sample as close as possible to cursor presentation. The
            // software half commits this exact pose at SURFLIVE too.
            visual.pose = geometry::slide(
                a.start,
                Point { x: cx, y: cy },
                a.elapsed(trueos_time::Instant::now().as_millis()),
                a.duration,
            );
            s.pending = Some((self.sequence, visual));
        }
        geometry::cursor_bgra_part(visual.pose, ICON, self.opacity, pixels, pitch);
        true
    }
}

struct Arrival {
    key: Key,
    start: Point,
    started: u64,
    render_ready: bool,
    duration: u64,
    crossed: bool,
    landed: Option<u64>,
    fade_started: Option<u64>,
    opacity: u8,
    entered: bool,
}
impl Arrival {
    fn elapsed(&self, now: u64) -> u64 {
        if self.render_ready {
            now.saturating_sub(self.started)
        } else {
            0
        }
    }
}
struct State {
    active: Option<Arrival>,
    committed: Option<Visual>,
    pending: Option<(u64, Visual)>,
    last_software: Option<Visual>,
}
impl State {
    const fn new() -> Self {
        Self {
            active: None,
            committed: None,
            pending: None,
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
        render_ready: false,
        duration: (distance / 2).clamp(320, 1100),
        crossed: false,
        landed: None,
        fade_started: None,
        opacity,
        entered: false,
    });
    state.last_software = None;
    true
}

/// Start the flight clock once a pristine character frame can support native
/// cursor samples. Later Lily frame updates never gate the tile's movement.
pub(super) fn render_ready() {
    if let Some(a) = STATE.lock().active.as_mut().filter(|a| !a.render_ready) {
        a.render_ready = true;
        a.started = a.started.max(trueos_time::Instant::now().as_millis());
    }
}
pub(crate) fn needs_crossed() -> bool {
    // Lily owns the next natural clip boundary, but the request must follow
    // an actual visible chest landing rather than the start of the flight.
    STATE
        .lock()
        .active
        .as_ref()
        .is_some_and(|a| a.landed.is_some())
}
pub(crate) fn crossed_ready() {
    if let Some(a) = STATE.lock().active.as_mut().filter(|a| a.landed.is_some()) {
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
    let pose = geometry::slide(a.start, Point { x: cx, y: cy }, a.elapsed(now), a.duration);
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

/// A temporary display-rate producer for the moving tile and its final fade.
/// Ordinary Lilly animation keeps its existing cadence and frame timings.
pub(crate) fn fast_updates() -> bool {
    let Some((_, _, cx, cy, 256, 256)) = super::marker_anchor() else {
        return false;
    };
    let now = trueos_time::Instant::now().as_millis();
    let s = STATE.lock();
    s.active.as_ref().is_some_and(|a| {
        a.crossed
            && a.fade_started
                .is_none_or(|t| now.saturating_sub(t) < TILE_FADE_MS)
            || a.landed.is_none()
                && (a.entered
                    || geometry::slide(a.start, Point { x: cx, y: cy }, a.elapsed(now), a.duration)
                        .near_cursor(48))
    })
}
/// Commit the software half only after the matching cursor source is SURFLIVE.
pub(crate) fn presented(sequence: u64) {
    let changed = {
        let mut s = STATE.lock();
        match s.pending {
            Some((seq, v)) if seq == sequence => {
                if let Some(a) = s.active.as_mut().filter(|a| a.key == v.key) {
                    let now = trueos_time::Instant::now().as_millis();
                    let chest = geometry::slide(a.start, v.pose.cursor, a.duration, a.duration);
                    if a.landed.is_none()
                        && !v.hidden
                        && a.elapsed(now) >= a.duration
                        && v.pose == chest
                    {
                        a.landed = Some(now);
                    }
                    if a.crossed && v.hidden && a.fade_started.is_none() {
                        // Start the fade clock only once the static Codex
                        // source beneath the intact tile is actually visible.
                        a.fade_started = Some(now);
                    }
                }
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
        a.fade_started = a.fade_started.map(|t| t.saturating_add(paused_ms));
    }
}

/// Select the resident sprite and tile stamp for this copy.
pub(super) fn source(default: LillyResidentFrame, sequence: u64) -> Option<Source> {
    let now = trueos_time::Instant::now().as_millis();
    let (_, _, cx, cy, 256, 256) = super::marker_anchor()? else {
        return None;
    };
    let mut s = STATE.lock();
    let a = s.active.as_mut()?;
    let elapsed = a.elapsed(now);
    let pose = geometry::slide(a.start, Point { x: cx, y: cy }, elapsed, a.duration);
    let key = a.key;
    let opacity = a.opacity;
    let revealing = a.crossed;
    let wish = a.fade_started.map(|t| now.saturating_sub(t));
    if wish.is_none() && !a.entered && !pose.near_cursor(0) {
        return None;
    }
    a.entered = true;
    // Stage the static Codex chest under the complete tile. Its first visible
    // presentation starts the fade clock.
    let mut path = revealing.then_some(WISH_PATHS[0]);
    if let Some(wish) = wish {
        path = None;
        // Fade the complete native tile over the static Codex chest frame.
        // Only afterwards does the existing Wish C timeline begin.
        let wish = wish.saturating_sub(TILE_FADE_MS);
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
    let surface = match path {
        Some(path) => lilly::resident_frame(path)?,
        None => default,
    };
    let visual = Visual {
        key,
        pose,
        hidden: revealing,
        opacity,
    };
    s.pending = Some((sequence, visual));
    let stamp_opacity = match wish {
        None => Some(opacity),
        Some(t) if t < TILE_FADE_MS => Some(
            ((u64::from(opacity) * (TILE_FADE_MS - t) + TILE_FADE_MS / 2) / TILE_FADE_MS) as u8,
        ),
        _ => None,
    };
    Some(Source {
        surface,
        stamp: stamp_opacity.map(|opacity| Stamp {
            visual,
            sequence,
            opacity,
            resample_motion: !revealing,
        }),
    })
}
