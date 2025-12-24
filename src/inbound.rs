use spin::Mutex;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum HidKind {
    Unknown = 0,
    Keyboard = 1,
    Mouse = 2,
}

impl From<u8> for HidKind {
    fn from(value: u8) -> Self {
        match value {
            1 => HidKind::Keyboard,
            2 => HidKind::Mouse,
            _ => HidKind::Unknown,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct InboundEvent {
    pub kind: HidKind,
    pub payload: [u8; 8],
}

impl InboundEvent {
    const fn empty() -> Self {
        Self {
            kind: HidKind::Unknown,
            payload: [0; 8],
        }
    }
}

const QUEUE_LEN: usize = 1024;

struct InboundRing {
    buf: [InboundEvent; QUEUE_LEN],
    head: usize,
    tail: usize,
    full: bool,
}

impl InboundRing {
    const fn new() -> Self {
        Self {
            buf: [InboundEvent::empty(); QUEUE_LEN],
            head: 0,
            tail: 0,
            full: false,
        }
    }

    fn push(&mut self, evt: InboundEvent) {
        self.buf[self.head] = evt;
        self.head = (self.head + 1) % QUEUE_LEN;
        if self.full {
            self.tail = (self.tail + 1) % QUEUE_LEN;
        }
        self.full = self.head == self.tail;
    }
}

static RING: Mutex<InboundRing> = Mutex::new(InboundRing::new());

pub fn push_report(kind: HidKind, data: &[u8]) {
    let mut payload = [0u8; 8];
    let copy_len = core::cmp::min(payload.len(), data.len());
    payload[..copy_len].copy_from_slice(&data[..copy_len]);

    let evt = InboundEvent { kind, payload };
    let mut ring = RING.lock();
    ring.push(evt);
}
