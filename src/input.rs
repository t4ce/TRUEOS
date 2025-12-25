use heapless::Vec;
use spin::Mutex;

#[derive(Copy, Clone, Debug)]
pub struct KeyboardEvent {
    pub modifiers: u8,
    pub keys: [u8; 6],
}

#[derive(Copy, Clone, Debug)]
pub struct MouseEvent {
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

/// Translate HID boot-protocol keycode to ASCII (US layout) when possible.
pub fn boot_keycode_to_ascii(code: u8, shift: bool) -> Option<u8> {
    match code {
        // Letters a-z
        0x04..=0x1D => {
            let base = b'a' + (code - 0x04);
            if shift { Some(base.to_ascii_uppercase()) } else { Some(base) }
        }
        // Number row 1-0
        0x1E => Some(if shift { b'!' } else { b'1' }),
        0x1F => Some(if shift { b'@' } else { b'2' }),
        0x20 => Some(if shift { b'#' } else { b'3' }),
        0x21 => Some(if shift { b'$' } else { b'4' }),
        0x22 => Some(if shift { b'%' } else { b'5' }),
        0x23 => Some(if shift { b'^' } else { b'6' }),
        0x24 => Some(if shift { b'&' } else { b'7' }),
        0x25 => Some(if shift { b'*' } else { b'8' }),
        0x26 => Some(if shift { b'(' } else { b'9' }),
        0x27 => Some(if shift { b')' } else { b'0' }),
        // Controls and punctuation
        0x28 => Some(b'\n'),
        0x2A => Some(0x08), // Backspace
        0x2B => Some(b'\t'),
        0x2C => Some(b' '),
        0x2D => Some(if shift { b'_' } else { b'-' }),
        0x2E => Some(if shift { b'+' } else { b'=' }),
        0x2F => Some(if shift { b'{' } else { b'[' }),
        0x30 => Some(if shift { b'}' } else { b']' }),
        0x31 => Some(if shift { b'|' } else { b'\\' }),
        0x33 => Some(if shift { b':' } else { b';' }),
        0x34 => Some(if shift { b'"' } else { b'\'' }),
        0x35 => Some(if shift { b'~' } else { b'`' }),
        0x36 => Some(if shift { b'<' } else { b',' }),
        0x37 => Some(if shift { b'>' } else { b'.' }),
        0x38 => Some(if shift { b'?' } else { b'/' }),
        _ => None,
    }
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

pub fn has_events() -> bool {
    !INPUT_QUEUE.lock().is_empty()
}
