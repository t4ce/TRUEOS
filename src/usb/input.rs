use heapless::Vec;
use spin::Mutex;
use embassy_time_driver::{now, TICK_HZ};

use embassy_time_driver::{now, TICK_HZ};

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosMouseState {
    pub x: i32,
    pub y: i32,
    pub dx: i32,
    pub dy: i32,
    pub wheel: i32,
    pub buttons: u32,
    pub seq: u32,
    pub slot_id: u32,
}

#[derive(Copy, Clone, Debug, Default)]
struct MouseAccum {
    x: i32,
    y: i32,
    dx: i32,
    dy: i32,
    wheel: i32,
    buttons: u32,
    seq: u32,
    slot_id: u32,

    // Kernel-owned coalescing/rate limit.
    last_emit_ms: u64,
    last_sent_buttons: u32,
}

static MOUSE_ACCUM: Mutex<MouseAccum> = Mutex::new(MouseAccum::new());

#[derive(Copy, Clone, Debug, Default)]
struct QjsMousePipe {
    pending: Option<TrueosMouseState>,
    last_emit_ms: u64,
    last_buttons: u32,
}

static QJS_MOUSE_PIPE: Mutex<QjsMousePipe> = Mutex::new(QjsMousePipe::default());

#[inline]
fn now_ms() -> u64 {
    let ticks = now();
    let hz = TICK_HZ as u64;
    if hz == 0 {
        return 0;
    }
    // Floor is fine for rate limiting.
    (ticks as u64).saturating_mul(1000) / hz
}

#[derive(Copy, Clone, Debug)]
pub struct KeyboardEvent {
    pub slot_id: u32,
    pub modifiers: u8,
    pub keys: [u8; 6],
    // ASCII translation of `keys` (US layout, shift-aware). 0 means "no key / not representable".
    pub ascii: [u8; 6],
}

#[derive(Copy, Clone, Debug)]
pub struct MouseEvent {
    pub slot_id: u32,
    pub buttons: u8,
    pub dx: i8,
    pub dy: i8,
    pub wheel: i8,
}

#[derive(Copy, Clone, Debug)]
pub enum InputEvent {
    Keyboard(KeyboardEvent),
    Mouse(MouseEvent),
}

impl MouseAccum {
    const fn new() -> Self {
        Self {
            x: 0,
            y: 0,
            dx: 0,
            dy: 0,
            wheel: 0,
            buttons: 0,
            seq: 0,
            slot_id: 0,

            last_emit_ms: 0,
            last_sent_buttons: 0,
        }
    }
}

#[inline]
fn now_ms() -> u64 {
    let ticks = now();
    let hz = TICK_HZ as u64;
    if hz == 0 {
        return 0;
    }
    (ticks as u64).saturating_mul(1000) / hz
}

const INPUT_QUEUE_CAP: usize = 64;
static INPUT_QUEUE: Mutex<Vec<InputEvent, INPUT_QUEUE_CAP>> = Mutex::new(Vec::new());

pub fn push_event(evt: InputEvent) {
    if let InputEvent::Mouse(m) = evt {
        let mut st = MOUSE_ACCUM.lock();
        st.slot_id = m.slot_id;
        st.buttons = m.buttons as u32;
        let dx = m.dx as i32;
        let dy = m.dy as i32;
        let wh = m.wheel as i32;
        st.dx = st.dx.saturating_add(dx);
        st.dy = st.dy.saturating_add(dy);
        st.wheel = st.wheel.saturating_add(wh);
        st.x = st.x.saturating_add(dx);
        st.y = st.y.saturating_add(dy);
        st.seq = st.seq.wrapping_add(1);
    }
    let mut q = INPUT_QUEUE.lock();
    if q.push(evt).is_err() {
        let _ = q.remove(0);
        let _ = q.push(evt);
    }
}

/// Offer a mouse event to the QJS-facing pipe.
///
/// Policy:
/// - Motion is coalesced and rate-limited: at most 1 emitted event every 25ms.
/// - Button changes and wheel deltas are emitted immediately (and can carry any coalesced motion).
/// - Only 1 pending event is stored; newer input merges into it.
pub fn qjs_mouse_offer(m: MouseEvent) {
    let mut pipe = QJS_MOUSE_PIPE.lock();

    let now_ms = now_ms();
    let dx = m.dx as i32;
    let dy = m.dy as i32;
    let wheel = m.wheel as i32;
    let buttons = m.buttons as u32;

    let buttons_changed = buttons != pipe.last_buttons;
    let wheel_changed = wheel != 0;
    let motion_changed = dx != 0 || dy != 0;

    let due_motion = motion_changed && now_ms.saturating_sub(pipe.last_emit_ms) >= 25;
    let should_emit = buttons_changed || wheel_changed || due_motion;

    // Merge into pending snapshot (or start one).
    let mut snap = pipe.pending.unwrap_or_default();
    snap.slot_id = m.slot_id;
    snap.buttons = buttons;
    snap.dx = snap.dx.saturating_add(dx);
    snap.dy = snap.dy.saturating_add(dy);
    snap.wheel = snap.wheel.saturating_add(wheel);
    snap.x = snap.x.saturating_add(dx);
    snap.y = snap.y.saturating_add(dy);
    snap.seq = snap.seq.wrapping_add(1);
    pipe.pending = Some(snap);

    if should_emit {
        pipe.last_emit_ms = now_ms;
        pipe.last_buttons = buttons;
    }
}

/// Pop at most one coalesced mouse event for QJS.
///
/// Return:
/// - `Some(state)` if an event is available now
/// - `None` if rate-limited and no button/wheel changes happened
pub fn qjs_mouse_pop() -> Option<TrueosMouseState> {
    let mut pipe = QJS_MOUSE_PIPE.lock();
    let Some(p) = pipe.pending else {
        return None;
    };

    let now_ms = now_ms();
    let due = now_ms.saturating_sub(pipe.last_emit_ms) >= 25;
    let buttons_changed = p.buttons != pipe.last_buttons;
    let wheel_changed = p.wheel != 0;

    // Motion-only events are rate-limited; button/wheel changes are immediate.
    if !(buttons_changed || wheel_changed || due) {
        return None;
    }

    pipe.pending = None;
    pipe.last_emit_ms = now_ms;
    pipe.last_buttons = p.buttons;
    Some(p)
}

#[no_mangle]
pub unsafe extern "C" fn trueos_cabi_qjs_mouse_pop(out: *mut TrueosMouseState) -> i32 {
    if out.is_null() {
        return -1;
    }
    match qjs_mouse_pop() {
        Some(v) => {
            *out = v;
            0
        }
        None => 1,
    }
}

pub fn mouse_poll() -> TrueosMouseState {
    let mut st = MOUSE_ACCUM.lock();

    let ms = now_ms();
    let buttons_changed = st.buttons != st.last_sent_buttons;
    let wheel_pending = st.wheel != 0;
    let move_pending = st.dx != 0 || st.dy != 0;

    let can_emit_move = st.last_emit_ms == 0 || ms.saturating_sub(st.last_emit_ms) >= 25;
    let emit_move = move_pending && can_emit_move;
    let emit_wheel = wheel_pending;
    let emit_buttons = buttons_changed;

    if emit_move || emit_wheel || emit_buttons {
        st.seq = st.seq.wrapping_add(1);
        if emit_move {
            st.last_emit_ms = ms;
        }
        if emit_buttons {
            st.last_sent_buttons = st.buttons;
        }
    }

    let out = TrueosMouseState {
        x: st.x,
        y: st.y,
        dx: if emit_move { st.dx } else { 0 },
        dy: if emit_move { st.dy } else { 0 },
        wheel: if emit_wheel { st.wheel } else { 0 },
        buttons: st.buttons,
        seq: st.seq,
        slot_id: st.slot_id,
    };

    // Deltas are one-shot for the things we emitted.
    if emit_move {
        st.dx = 0;
        st.dy = 0;
    }
    if emit_wheel {
        st.wheel = 0;
    }
    out
}

#[no_mangle]
pub unsafe extern "C" fn trueos_cabi_mouse_poll(out: *mut TrueosMouseState) -> i32 {
    if out.is_null() {
        return -1;
    }
    *out = mouse_poll();
    0
}

pub fn pop_event() -> Option<InputEvent> {
    let mut q = INPUT_QUEUE.lock();
    if q.is_empty() {
        None
    } else {
        Some(q.remove(0))
    }
}

pub fn pop_mouse_event() -> Option<MouseEvent> {
    let mut q = INPUT_QUEUE.lock();
    let mut idx = 0usize;
    while idx < q.len() {
        if let InputEvent::Mouse(m) = q[idx] {
            let _ = q.remove(idx);
            return Some(m);
        }
        idx += 1;
    }
    None
}