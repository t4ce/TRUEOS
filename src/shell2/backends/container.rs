use alloc::collections::VecDeque;
use core::fmt::Write;
use core::sync::atomic::AtomicBool;

use spin::Mutex;

use crate::shell2::{ShellBackend2, ShellIo2};

const MAX_QUEUE_BYTES: usize = 64 * 1024;

pub(crate) struct ContainerShellBackend;

pub(crate) static CONTAINER_SHELL_BACKEND: ContainerShellBackend = ContainerShellBackend;

static CONTAINER_LAST_WAS_CR: AtomicBool = AtomicBool::new(false);

struct ContainerShellState {
    input_rx: VecDeque<u8>,
    output_tx: VecDeque<u8>,
}

static CONTAINER_SHELL_STATE: Mutex<ContainerShellState> = Mutex::new(ContainerShellState {
    input_rx: VecDeque::new(),
    output_tx: VecDeque::new(),
});

fn push_bounded(queue: &mut VecDeque<u8>, byte: u8) {
    if queue.len() >= MAX_QUEUE_BYTES {
        let _ = queue.pop_front();
    }
    queue.push_back(byte);
}

fn push_output_bytes(bytes: &[u8]) {
    let mut state = CONTAINER_SHELL_STATE.lock();
    for &byte in bytes {
        push_bounded(&mut state.output_tx, byte);
    }
}

pub(crate) fn container_shell_submit_input(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    let mut state = CONTAINER_SHELL_STATE.lock();
    for &byte in bytes {
        push_bounded(&mut state.input_rx, byte);
    }
    bytes.len()
}

pub(crate) fn container_shell_read_output_byte() -> Option<u8> {
    CONTAINER_SHELL_STATE.lock().output_tx.pop_front()
}

pub(crate) fn container_shell_drain_output(out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }
    let mut state = CONTAINER_SHELL_STATE.lock();
    let mut count = 0;
    while count < out.len() {
        let Some(byte) = state.output_tx.pop_front() else {
            break;
        };
        out[count] = byte;
        count += 1;
    }
    count
}

impl ShellIo2 for ContainerShellBackend {
    #[inline]
    fn raw_write_str(&self, s: &str) {
        crate::shell2::crlf::write_bytes_crlf(s.as_bytes(), &CONTAINER_LAST_WAS_CR, |chunk| {
            push_output_bytes(chunk);
        });
    }

    #[inline]
    fn raw_write_fmt(&self, args: core::fmt::Arguments<'_>) {
        struct Writer;

        impl Write for Writer {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                crate::shell2::crlf::write_bytes_crlf(
                    s.as_bytes(),
                    &CONTAINER_LAST_WAS_CR,
                    |chunk| {
                        push_output_bytes(chunk);
                    },
                );
                Ok(())
            }
        }

        let _ = Writer.write_fmt(args);
    }

    #[inline]
    fn raw_write_char(&self, ch: char) {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        crate::shell2::crlf::write_bytes_crlf(s.as_bytes(), &CONTAINER_LAST_WAS_CR, |chunk| {
            push_output_bytes(chunk);
        });
    }

    #[inline]
    fn raw_write_byte(&self, b: u8) {
        crate::shell2::crlf::write_bytes_crlf(&[b], &CONTAINER_LAST_WAS_CR, |chunk| {
            push_output_bytes(chunk);
        });
    }
}

impl ShellBackend2 for ContainerShellBackend {
    #[inline]
    fn init(&self) {}

    #[inline]
    fn read_byte(&self) -> Option<u8> {
        CONTAINER_SHELL_STATE.lock().input_rx.pop_front()
    }
}
