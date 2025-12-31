use core::cmp;
use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};
use heapless::Vec;
use spin::{Once, RwLock};

const COM1_BASE: u16 = 0x3F8;
const COM1_WAIT_SPINS: usize = 100_000;
const COM1_CLOCK_HZ: u32 = 14_745_600; // VirtualBox passthrough clock for 921_600 baud
const COM1_BAUD: u32 = 921_600;
const MAX_SERIAL_MIRRORS: usize = 4;

static DESIRED_BAUD: AtomicU32 = AtomicU32::new(COM1_BAUD);

pub trait SerialBackend: Sync {
    fn name(&self) -> &'static str;
    fn try_write_byte(&self, byte: u8) -> bool;

    fn try_write(&self, bytes: &[u8]) -> usize {
        let mut written = 0usize;
        for &b in bytes {
            if self.try_write_byte(b) {
                written += 1;
            } else {
                break;
            }
        }
        written
    }

    fn try_read_byte(&self) -> Option<u8> {
        None
    }

    fn apply_baud(&self, _baud: u32) -> bool {
        false
    }
}

#[derive(Copy, Clone, Debug)]
pub enum BackendRole {
    Preferred,
    Mirror,
}

#[derive(Debug)]
pub enum BackendError {
    TableFull,
}

struct RoutingTable {
    primary: &'static dyn SerialBackend,
    mirrors: Vec<&'static dyn SerialBackend, MAX_SERIAL_MIRRORS>,
}

impl RoutingTable {
    const fn new() -> Self {
        Self {
            primary: &COM1_BACKEND,
            mirrors: Vec::new(),
        }
    }

    fn contains(&self, backend: &'static dyn SerialBackend) -> bool {
        ptr::eq(self.primary, backend)
            || self
                .mirrors
                .iter()
                .any(|registered| ptr::eq(*registered, backend))
    }

    fn add_mirror(&mut self, backend: &'static dyn SerialBackend) -> Result<(), BackendError> {
        if self.mirrors.iter().any(|b| ptr::eq(*b, backend)) {
            return Ok(());
        }
        self.mirrors
            .push(backend)
            .map_err(|_| BackendError::TableFull)
    }

    fn drop_mirror(&mut self, backend: &'static dyn SerialBackend) -> bool {
        let mut idx = 0usize;
        while idx < self.mirrors.len() {
            if ptr::eq(self.mirrors[idx], backend) {
                let _ = self.mirrors.remove(idx);
                return true;
            }
            idx += 1;
        }
        false
    }

    fn promote(&mut self, backend: &'static dyn SerialBackend) -> Result<(), BackendError> {
        if ptr::eq(self.primary, backend) {
            return Ok(());
        }
        let _ = self.drop_mirror(backend);
        if !ptr::eq(self.primary, backend) {
            if !ptr::eq(self.primary, &COM1_BACKEND) {
                self.add_mirror(self.primary)?;
            }
            self.primary = backend;
        }
        Ok(())
    }

    fn remove(&mut self, backend: &'static dyn SerialBackend) -> bool {
        if ptr::eq(self.primary, backend) {
            self.primary = &COM1_BACKEND;
            return true;
        }
        self.drop_mirror(backend)
    }
}

static ROUTING: RwLock<RoutingTable> = RwLock::new(RoutingTable::new());

struct Com1UartBackend {
    init: Once<()>,
}

static COM1_BACKEND: Com1UartBackend = Com1UartBackend::new();

impl Com1UartBackend {
    const fn new() -> Self {
        Self { init: Once::new() }
    }

    fn ensure_init(&self) {
        self.init.call_once(|| unsafe {
            crate::portio::outb(COM1_BASE + 1, 0x00);
            crate::portio::outb(COM1_BASE + 3, 0x80);
            self.write_divisor(Self::divisor_for_baud(COM1_BAUD));
            crate::portio::outb(COM1_BASE + 3, 0x03);
            crate::portio::outb(COM1_BASE + 2, 0xC7);
            crate::portio::outb(COM1_BASE + 4, 0x0B);
        });
    }

    fn write_divisor(&self, divisor: u16) {
        unsafe {
            crate::portio::outb(COM1_BASE + 0, (divisor & 0x00FF) as u8);
            crate::portio::outb(COM1_BASE + 1, (divisor >> 8) as u8);
        }
    }

    fn divisor_for_baud(baud: u32) -> u16 {
        let baud = baud.max(1);
        let raw = COM1_CLOCK_HZ / (16 * baud);
        cmp::max(1, cmp::min(raw, u16::MAX as u32)) as u16
    }

    fn program_baud(&self, baud: u32) -> bool {
        self.ensure_init();
        let divisor = Self::divisor_for_baud(baud);
        unsafe {
            crate::portio::outb(COM1_BASE + 3, 0x80);
            self.write_divisor(divisor);
            crate::portio::outb(COM1_BASE + 3, 0x03);
        }
        true
    }

    fn lsr() -> u8 {
        unsafe { crate::portio::inb(COM1_BASE + 5) }
    }
}

impl SerialBackend for Com1UartBackend {
    fn name(&self) -> &'static str {
        "com1-uart"
    }

    fn try_write_byte(&self, byte: u8) -> bool {
        self.ensure_init();
        for _ in 0..COM1_WAIT_SPINS {
            if (Self::lsr() & 0x20) != 0 {
                unsafe { crate::portio::outb(COM1_BASE + 0, byte) };
                return true;
            }
            core::hint::spin_loop();
        }
        false
    }

    fn apply_baud(&self, baud: u32) -> bool {
        self.program_baud(baud)
    }
}

#[inline(always)]
pub(crate) fn try_write_byte(b: u8) -> bool {
    let guard = ROUTING.read();
    let mut ok = guard.primary.try_write_byte(b);
    for backend in guard.mirrors.iter() {
        ok |= backend.try_write_byte(b);
    }
    ok
}

#[inline(always)]
pub(crate) fn write_str(s: &str) {
    for &b in s.as_bytes() {
        let _ = try_write_byte(b);
    }
}

#[inline(always)]
pub(crate) fn try_read_byte() -> Option<u8> {
    let guard = ROUTING.read();
    if let Some(byte) = guard.primary.try_read_byte() {
        return Some(byte);
    }
    for backend in guard.mirrors.iter() {
        if let Some(byte) = backend.try_read_byte() {
            return Some(byte);
        }
    }
    None
}

pub(crate) fn register_backend(
    backend: &'static dyn SerialBackend,
    role: BackendRole,
) -> Result<(), BackendError> {
    let mut guard = ROUTING.write();
    if guard.contains(backend) {
        return Ok(());
    }
    let result = match role {
        BackendRole::Preferred => guard.promote(backend),
        BackendRole::Mirror => guard.add_mirror(backend),
    };
    if result.is_ok() {
        let baud = desired_baud();
        let _ = backend.apply_baud(baud);
    }
    result
}

pub(crate) fn unregister_backend(backend: &'static dyn SerialBackend) -> bool {
    ROUTING.write().remove(backend)
}

pub(crate) fn desired_baud() -> u32 {
    DESIRED_BAUD.load(Ordering::Relaxed)
}

pub(crate) fn set_desired_baud(baud: u32) {
    let clamped = baud.max(1);
    DESIRED_BAUD.store(clamped, Ordering::Relaxed);
    let guard = ROUTING.read();
    let _ = guard.primary.apply_baud(clamped);
    for backend in guard.mirrors.iter() {
        let _ = backend.apply_baud(clamped);
    }
}
