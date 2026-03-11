use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::{Mutex, Once};

const COM1: u16 = 0x3F8;
static INIT: AtomicBool = AtomicBool::new(false);
static AI_INPUT: Once<Mutex<VecDeque<u8>>> = Once::new();

fn ai_input() -> &'static Mutex<VecDeque<u8>> {
    AI_INPUT.call_once(|| Mutex::new(VecDeque::new()))
}

pub(crate) fn init() {
    if INIT.swap(true, Ordering::AcqRel) {
        return;
    }
    unsafe {
        crate::portio::outb(COM1 + 1, 0x00); // disable IRQs
        crate::portio::outb(COM1 + 3, 0x80); // DLAB on
        crate::portio::outb(COM1, 0x01); // divisor low (115200)
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

pub(crate) fn write_bytes(bytes: &[u8]) {
    for &b in bytes {
        write_byte(b);
    }
}

pub(crate) fn inject_bytes(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    let mut queue = ai_input().lock();
    for &b in bytes {
        queue.push_back(b);
    }
    bytes.len()
}

pub(crate) fn read_byte() -> Option<u8> {
    if !INIT.load(Ordering::Acquire) {
        init();
    }
    if let Some(b) = ai_input().lock().pop_front() {
        return Some(b);
    }
    unsafe {
        if (crate::portio::inb(COM1 + 5) & 0x01) != 0 {
            Some(crate::portio::inb(COM1))
        } else {
            None
        }
    }
}
