use embassy_time_driver::{TICK_HZ, now};
use heapless::Vec;
use spin::Mutex;

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
        }
    }
}

static MOUSE_ACCUM: Mutex<MouseAccum> = Mutex::new(MouseAccum::new());

#[derive(Clone, Debug, Default)]
struct QjsMousePipe {
    x: i32,
    y: i32,
    dx: i32,
    dy: i32,
    #[allow(dead_code)]
    wheel: i32,
    buttons: u32,
    seq: u32,
    slot_id: u32,
    last_motion_emit_ms: u64,
    // For motion: last time we emitted a motion update.

    // For buttons: track state transitions and buffer them.
    #[allow(dead_code)]
    last_buttons_seen: u32,

    // For motion consumers: last button state we returned (not strictly needed, but useful).
    last_sent_buttons: u32,

    // Queue "valuable" events (button transitions + wheel) so they don't get overwritten.
    queued: Vec<QjsQueuedEvent, 10>,
}

#[derive(Copy, Clone, Debug, Default)]
struct QjsQueuedEvent {
    x: i32,
    y: i32,
    buttons: u32,
    wheel: i32,
    seq: u32,
    slot_id: u32,
}

impl QjsMousePipe {
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
            last_motion_emit_ms: 0,
            last_buttons_seen: 0,
            last_sent_buttons: 0,
            queued: Vec::new(),
        }
    }
}

static QJS_MOUSE_PIPE: Mutex<QjsMousePipe> = Mutex::new(QjsMousePipe::new());

#[inline]
fn now_ms() -> u64 {
    let ticks = now();
    let hz = TICK_HZ as u64;
    if hz == 0 {
        return 0;
    }
    (ticks as u64).saturating_mul(1000) / hz
}

#[derive(Copy, Clone, Debug)]
#[allow(dead_code)]
pub struct KeyboardEvent {
    pub slot_id: u32,
    pub modifiers: u8,
    pub keys: [u8; 6],
    pub ascii: [u8; 6],
}

#[derive(Copy, Clone, Debug)]
pub struct MouseEvent {
    pub slot_id: u32,
    pub buttons: u8,
    pub dx: i8,
    pub dy: i8,
    pub wheel: i8,
    #[allow(dead_code)]
    pub has_wheel: bool, // true=4-byte, false=3-byte
}

#[derive(Copy, Clone, Debug)]
#[allow(dead_code)]
pub enum InputEvent {
    Keyboard(KeyboardEvent),
    Mouse(MouseEvent),
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

pub fn mouse_poll() -> TrueosMouseState {
    let mut st = MOUSE_ACCUM.lock();
    let out = TrueosMouseState {
        x: st.x,
        y: st.y,
        dx: st.dx,
        dy: st.dy,
        wheel: st.wheel,
        buttons: st.buttons,
        seq: st.seq,
        slot_id: st.slot_id,
    };
    st.dx = 0;
    st.dy = 0;
    st.wheel = 0;
    out
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_mouse_poll(out: *mut TrueosMouseState) -> i32 {
    if out.is_null() {
        return -1;
    }
    *out = mouse_poll();
    0
}

/// Feed a mouse report into the QJS-facing capped/coalesced stream.
///
/// Policy:
/// - Motion (dx/dy) is coalesced and only emitted at most once per 25ms.
/// - Wheel and button transitions are emitted immediately.
#[allow(dead_code)]
pub fn qjs_mouse_offer(m: MouseEvent) {
    let mut p = QJS_MOUSE_PIPE.lock();

    p.slot_id = m.slot_id;
    p.buttons = m.buttons as u32;

    let dx = m.dx as i32;
    let dy = m.dy as i32;
    let wh = m.wheel as i32;

    p.dx = p.dx.saturating_add(dx);
    p.dy = p.dy.saturating_add(dy);
    p.wheel = p.wheel.saturating_add(wh);

    p.x = p.x.saturating_add(dx);
    p.y = p.y.saturating_add(dy);
    if p.x < 0 {
        p.x = 0;
    }
    if p.y < 0 {
        p.y = 0;
    }

    p.seq = p.seq.wrapping_add(1);

    // Buffer button transitions ("TCP").
    if p.buttons != p.last_buttons_seen {
        let ev = QjsQueuedEvent {
            x: p.x,
            y: p.y,
            buttons: p.buttons,
            wheel: 0,
            seq: p.seq,
            slot_id: p.slot_id,
        };
        if p.queued.push(ev).is_err() {
            let _ = p.queued.remove(0);
            let _ = p.queued.push(ev);
        }
        p.last_buttons_seen = p.buttons;
    }

    // Buffer wheel deltas ("TCP").
    if wh != 0 {
        let ev = QjsQueuedEvent {
            x: p.x,
            y: p.y,
            buttons: p.buttons,
            wheel: wh,
            seq: p.seq,
            slot_id: p.slot_id,
        };
        if p.queued.push(ev).is_err() {
            let _ = p.queued.remove(0);
            let _ = p.queued.push(ev);
        }
        // Wheel itself should not accumulate forever if we're also queueing.
        // (motion pop won't include wheel)
        p.wheel = 0;
    }
}

fn qjs_mouse_try_pop() -> Option<TrueosMouseState> {
    let mut p = QJS_MOUSE_PIPE.lock();
    let now = now_ms();

    // First drain queued button/wheel events (reliable, not rate-limited).
    if !p.queued.is_empty() {
        let ev = p.queued.remove(0);
        p.last_sent_buttons = ev.buttons;
        return Some(TrueosMouseState {
            x: ev.x,
            y: ev.y,
            dx: 0,
            dy: 0,
            wheel: ev.wheel,
            buttons: ev.buttons,
            seq: ev.seq,
            slot_id: ev.slot_id,
        });
    }

    let motion_changed = p.dx != 0 || p.dy != 0;
    let due_motion = motion_changed && now.saturating_sub(p.last_motion_emit_ms) >= 25;

    if !due_motion {
        return None;
    }

    let out = TrueosMouseState {
        x: p.x,
        y: p.y,
        dx: p.dx,
        dy: p.dy,
        wheel: 0,
        buttons: p.buttons,
        seq: p.seq,
        slot_id: p.slot_id,
    };

    p.dx = 0;
    p.dy = 0;
    p.last_motion_emit_ms = now;
    p.last_sent_buttons = p.buttons;

    Some(out)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_qjs_mouse_pop(out: *mut TrueosMouseState) -> i32 {
    if out.is_null() {
        return -1;
    }
    if let Some(ev) = qjs_mouse_try_pop() {
        *out = ev;
        0
    } else {
        1
    }
}
