use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

const COM1: u16 = 0x3F8;
const TX_READY: u8 = 1 << 5;
const TX_POLL_LIMIT: usize = 100_000;

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static LAST_WAS_CR: AtomicBool = AtomicBool::new(false);

pub(crate) fn init() {
    if INITIALIZED.swap(true, Ordering::AcqRel) {
        return;
    }

    unsafe {
        crate::portio::outb(COM1 + 1, 0x00); // Disable UART interrupts.
        crate::portio::outb(COM1 + 3, 0x80); // Enable divisor latch access.
        crate::portio::outb(COM1, 0x01); // 115200 baud.
        crate::portio::outb(COM1 + 1, 0x00);
        crate::portio::outb(COM1 + 3, 0x03); // 8 data bits, no parity, 1 stop bit.
        crate::portio::outb(COM1 + 2, 0xC7); // Enable and clear FIFOs.
        crate::portio::outb(COM1 + 4, 0x03); // DTR + RTS; polling mode only.
    }
}

#[inline]
fn write_raw_byte(byte: u8) {
    if !INITIALIZED.load(Ordering::Acquire) {
        init();
    }

    for _ in 0..TX_POLL_LIMIT {
        let status = unsafe { crate::portio::inb(COM1 + 5) };
        if status == u8::MAX {
            return;
        }
        if (status & TX_READY) != 0 {
            unsafe { crate::portio::outb(COM1, byte) };
            return;
        }
        core::hint::spin_loop();
    }
}

pub(crate) fn write_bytes(bytes: &[u8]) {
    for &byte in bytes {
        if byte == b'\n' && !LAST_WAS_CR.load(Ordering::Relaxed) {
            write_raw_byte(b'\r');
        }
        write_raw_byte(byte);
        LAST_WAS_CR.store(byte == b'\r', Ordering::Relaxed);
    }
}

pub(crate) fn write_fmt(args: fmt::Arguments<'_>) {
    struct Writer;

    impl fmt::Write for Writer {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            write_bytes(text.as_bytes());
            Ok(())
        }
    }

    let _ = fmt::write(&mut Writer, args);
}
