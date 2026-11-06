//! HID input queues and compatibility exports.

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

#[derive(Copy, Clone, Debug)]
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
    pub has_wheel: bool, // true=4-byte, false=3-byte
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TabletEvent {
    pub slot_id: u32,
    pub buttons: u8,
    pub report_id: u8,
    pub x_raw: u16,
    pub y_raw: u16,
    pub x_norm_q15: u16,
    pub y_norm_q15: u16,
    pub flags: u8,
}

#[derive(Copy, Clone, Debug)]
pub enum InputEvent {
    Keyboard(KeyboardEvent),
    Mouse(MouseEvent),
    Tablet(TabletEvent),
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

#[inline]
fn event_slot_id(event: &InputEvent) -> u32 {
    match event {
        InputEvent::Keyboard(event) => event.slot_id,
        InputEvent::Mouse(event) => event.slot_id,
        InputEvent::Tablet(event) => event.slot_id,
    }
}

pub(crate) fn remove_slot(slot_id: u32) -> usize {
    let mut removed = 0usize;
    {
        let mut queue = INPUT_QUEUE.lock();
        let mut index = 0usize;
        while index < queue.len() {
            if event_slot_id(&queue[index]) == slot_id {
                let _ = queue.remove(index);
                removed = removed.saturating_add(1);
            } else {
                index += 1;
            }
        }
    }

    {
        let mut mouse = MOUSE_ACCUM.lock();
        if mouse.slot_id == slot_id {
            mouse.slot_id = 0;
            mouse.dx = 0;
            mouse.dy = 0;
            mouse.wheel = 0;
            mouse.buttons = 0;
        }
    }

    removed
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

pub fn pop_tablet_event() -> Option<TabletEvent> {
    let mut q = INPUT_QUEUE.lock();
    let mut idx = 0usize;
    while idx < q.len() {
        if let InputEvent::Tablet(t) = q[idx] {
            let _ = q.remove(idx);
            return Some(t);
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
