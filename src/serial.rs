use crate::globalog::SerialBackend;
use core::cmp;
use spin::Once;

const COM1_BASE: u16 = 0x3F8;
const COM1_WAIT_SPINS: usize = 100_000;
const COM1_CLOCK_HZ: u32 = 14_745_600;
pub(crate) struct Com1UartBackend {
    init: Once<()>,
}

pub(crate) static COM1_BACKEND: Com1UartBackend = Com1UartBackend::new();

impl Com1UartBackend {
    const fn new() -> Self {
        Self { init: Once::new() }
    }

    fn ensure_init(&self) {
        self.init.call_once(|| unsafe {
            crate::portio::outb(COM1_BASE + 1, 0x00);
            crate::portio::outb(COM1_BASE + 3, 0x80);
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
