use heapless::Vec;
use spin::Mutex;

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

const INPUT_QUEUE_CAP: usize = 64;
static INPUT_QUEUE: Mutex<Vec<InputEvent, INPUT_QUEUE_CAP>> = Mutex::new(Vec::new());

pub fn push_event(evt: InputEvent) {
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