use alloc::collections::VecDeque;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::shell::{ShellBackend, ShellIo};

pub(crate) struct QjsShellBackend;

pub(crate) static QJS_SHELL_BACKEND: QjsShellBackend = QjsShellBackend;

const MAX_QJS_SHELL_RX: usize = 64 * 1024;
const MAX_QJS_SHELL_TX: usize = 64 * 1024;

struct QjsShellState {
    rx: VecDeque<u8>,
    tx: VecDeque<u8>,
}

static QJS_SHELL_LAST_WAS_CR: AtomicBool = AtomicBool::new(false);
static QJS_SHELL_STATE: spin::Mutex<QjsShellState> = spin::Mutex::new(QjsShellState {
    rx: VecDeque::new(),
    tx: VecDeque::new(),
});

fn qjs_shell_write_bytes_raw(bytes: &[u8]) {
    let mut st = QJS_SHELL_STATE.lock();
    for &b in bytes {
        if st.tx.len() >= MAX_QJS_SHELL_TX {
            let _ = st.tx.pop_front();
        }
        st.tx.push_back(b);
    }
}

pub(crate) fn qjs_shell_reset() {
    let mut st = QJS_SHELL_STATE.lock();
    st.rx.clear();
    st.tx.clear();
    QJS_SHELL_LAST_WAS_CR.store(false, Ordering::Release);
}

pub(crate) fn qjs_shell_push_input(bytes: &[u8]) -> usize {
    let mut st = QJS_SHELL_STATE.lock();
    let mut wrote = 0usize;
    for &b in bytes {
        if st.rx.len() >= MAX_QJS_SHELL_RX {
            let _ = st.rx.pop_front();
        }
        st.rx.push_back(b);
        wrote += 1;
    }
    wrote
}

pub(crate) fn qjs_shell_push_input_byte(byte: u8) {
    let _ = qjs_shell_push_input(&[byte]);
}

pub(crate) fn qjs_shell_take_output(out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }

    let mut st = QJS_SHELL_STATE.lock();
    let mut n = 0usize;
    while n < out.len() {
        let Some(b) = st.tx.pop_front() else {
            break;
        };
        out[n] = b;
        n += 1;
    }
    n
}

pub(crate) fn qjs_shell_take_output_byte() -> Option<u8> {
    QJS_SHELL_STATE.lock().tx.pop_front()
}

impl ShellIo for QjsShellBackend {
    #[inline]
    fn write_str(&self, s: &str) {
        crate::shell::crlf::write_bytes_crlf(s.as_bytes(), &QJS_SHELL_LAST_WAS_CR, |chunk| {
            qjs_shell_write_bytes_raw(chunk);
        });
    }

    #[inline]
    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        use core::fmt::Write;

        struct Writer;

        impl Write for Writer {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                crate::shell::crlf::write_bytes_crlf(
                    s.as_bytes(),
                    &QJS_SHELL_LAST_WAS_CR,
                    qjs_shell_write_bytes_raw,
                );
                Ok(())
            }
        }

        let _ = Writer.write_fmt(args);
    }

    #[inline]
    fn write_char(&self, ch: char) {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        crate::shell::crlf::write_bytes_crlf(s.as_bytes(), &QJS_SHELL_LAST_WAS_CR, |chunk| {
            qjs_shell_write_bytes_raw(chunk);
        });
    }

    #[inline]
    fn write_byte(&self, b: u8) {
        crate::shell::crlf::write_bytes_crlf(&[b], &QJS_SHELL_LAST_WAS_CR, |chunk| {
            qjs_shell_write_bytes_raw(chunk);
        });
    }
}

impl ShellBackend for QjsShellBackend {
    #[inline]
    fn init(&self) {
        qjs_shell_reset();
    }

    #[inline]
    fn read_byte(&self) -> Option<u8> {
        QJS_SHELL_STATE.lock().rx.pop_front()
    }
}
