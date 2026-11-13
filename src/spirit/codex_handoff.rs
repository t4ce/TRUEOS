//! Lilly-owned procedural Codex arrival. UI4 never writes the cursor surface.
//! Only the single Spirit producer writes the persistent source scratch, after
//! the preceding GPU source read has retired. No GPU destination is CPU-mapped.
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

struct Arrival {
    key: Key,
    start: Point,
    started: u64,
    duration: u64,
    crossed: bool,
    landed: Option<u64>,
    opacity: u8,
}
struct State {
    active: Option<Arrival>,
    absorbed: Vec<Key>,
    committed: Option<Visual>,
    pending: Option<(u64, Visual)>,
    scratch: Option<crate::gpu::resident::ResidentDmaBuffer>,
    pixels: Vec<u8>,
}
impl State {
    const fn new() -> Self {
        Self {
            active: None,
            absorbed: Vec::new(),
            committed: None,
            pending: None,
            scratch: None,
            pixels: Vec::new(),
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
        started: trueos_time::Instant::now().as_millis(),
        duration: (distance / 2).clamp(320, 1100),
        crossed: false,
        landed: None,
        opacity,
    });
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
    s.absorbed
        .retain(|key| key.owner != owner || key.window != window);
    if s.active
        .as_ref()
        .is_some_and(|a| a.key.owner == owner && a.key.window == window)
    {
        s.active = None;
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
pub(crate) fn visual() -> Option<Visual> {
    STATE.lock().committed
}
pub(crate) fn absorbed(key: Key) -> bool {
    STATE.lock().absorbed.contains(&key)
}

/// The latest folded window owns the circle until restored. The animation may
/// still be playing, but only a presented, fully landed icon is clickable here.
pub(crate) fn folded_target() -> Option<Key> {
    let s = STATE.lock();
    s.committed
        .filter(|v| v.hidden && s.active.as_ref().is_some_and(|a| a.key == v.key))
        .map(|v| v.key)
        .or_else(|| s.absorbed.last().copied())
}

/// Commit the UI4 half only after the matching cursor source is SURFLIVE.
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
pub(super) fn source(default: LillyResidentFrame, sequence: u64) -> Option<LillyResidentFrame> {
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
            if !s.absorbed.contains(&key) {
                s.absorbed.push(key);
            }
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
            return lilly::resident_frame("Static_up/static.png");
        }
    }
    let frame = path.and_then(lilly::resident_frame).unwrap_or(default);
    if s.scratch.is_none() {
        s.scratch = Some(crate::gpu::resident::ResidentDmaBuffer::allocate_zeroed(
            BYTES, 4096,
        )?);
    }
    s.pixels.resize(BYTES, 0);
    if !lilly::copy_frame(frame, &mut s.pixels) {
        return None;
    }
    if wish.is_none() {
        geometry::cursor_part(pose, ICON, opacity, &mut s.pixels);
    } else if wish.is_some_and(|t| t < 100) {
        // Land with the exact gray-backed tile, then blend into the authored
        // background-free, palette-quantized chest stamp without a pop.
        let t = wish.unwrap();
        let mut native = alloc::vec![0;BYTES];
        if !lilly::copy_frame(lilly::resident_frame("Static_up/static.png")?, &mut native) {
            return None;
        }
        geometry::cursor_part(pose, ICON, opacity, &mut native);
        for (out, from) in s.pixels.iter_mut().zip(native) {
            *out = ((u64::from(from) * (100 - t) + u64::from(*out) * t + 50) / 100) as u8;
        }
    }
    let dma = s.scratch.as_ref()?;
    if !dma.write(0, &s.pixels) {
        return None;
    }
    dma.flush();
    let source = LillyResidentFrame {
        phys: dma.phys(),
        gpu: SOURCE_GPU,
        bytes: BYTES,
        width: 256,
        height: 256,
        pitch_bytes: 1024,
    };
    s.pending = Some((
        sequence,
        Visual {
            key,
            pose,
            hidden: wish.is_some(),
            opacity,
        },
    ));
    Some(source)
}
