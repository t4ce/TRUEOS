use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

const COM1: u16 = 0x3F8;
static INIT: AtomicBool = AtomicBool::new(false);

pub(crate) fn init() {
    if INIT.swap(true, Ordering::AcqRel) {
        return;
    }
    unsafe {
        crate::portio::outb(COM1 + 1, 0x00); // disable IRQs
        crate::portio::outb(COM1 + 3, 0x80); // DLAB on
        crate::portio::outb(COM1 + 0, 0x01); // divisor low (115200)
        crate::portio::outb(COM1 + 1, 0x00); // divisor high
        crate::portio::outb(COM1 + 3, 0x03); // 8N1
        crate::portio::outb(COM1 + 2, 0xC7); // FIFO enable
        crate::portio::outb(COM1 + 4, 0x0B); // IRQs, RTS/DSR
    }
}

#[inline]
pub(crate) fn write_byte(b: u8) {
    if !INIT.load(Ordering::Acquire) {
        init();
    }
    unsafe {
        while (crate::portio::inb(COM1 + 5) & 0x20) == 0 {}
        crate::portio::outb(COM1, b);
    }
}

pub(crate) fn write_str(s: &str) {
    for &b in s.as_bytes() {
        if b == b'\n' {
            write_byte(b'\r');
        }
        write_byte(b);
    }
}

pub(crate) fn write_bytes(bytes: &[u8]) {
    for &b in bytes {
        write_byte(b);
    }
}

pub(crate) fn write_fmt(args: fmt::Arguments<'_>) {
    use core::fmt::Write;

    struct Writer;

    impl fmt::Write for Writer {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            for &b in s.as_bytes() {
                if b == b'\n' {
                    write_byte(b'\r');
                }
                write_byte(b);
            }
            Ok(())
        }
    }

    let _ = Writer.write_fmt(args);
}

pub(crate) fn write_char(ch: char) {
    let mut buf = [0u8; 4];
    let s = ch.encode_utf8(&mut buf);
    write_str(s);
}

pub(crate) fn read_byte() -> Option<u8> {
    if !INIT.load(Ordering::Acquire) {
        init();
    }
    unsafe {
        if (crate::portio::inb(COM1 + 5) & 0x01) != 0 {
            Some(crate::portio::inb(COM1))
        } else {
            None
        }
    }
}
