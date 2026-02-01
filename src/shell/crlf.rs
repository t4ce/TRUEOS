use core::sync::atomic::{AtomicBool, Ordering};

/// Writes `bytes` to `write()` while normalizing newlines to CRLF.
///
/// Rules:
/// - `\n` becomes `\r\n` unless it is already preceded by `\r` (even across calls).
/// - Existing `\r\n` is preserved.
///
/// `last_was_cr` tracks whether the previous call ended with `\r`.
#[inline]
pub(crate) fn write_bytes_crlf(
    bytes: &[u8],
    last_was_cr: &AtomicBool,
    mut write: impl FnMut(&[u8]),
) {
    if bytes.is_empty() {
        return;
    }

    let mut prev_cr = last_was_cr.load(Ordering::Relaxed);
    let mut start = 0usize;

    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' && !prev_cr {
            if start < i {
                write(&bytes[start..i]);
            }
            write(b"\r\n");
            start = i + 1;
            prev_cr = false;
            continue;
        }

        prev_cr = b == b'\r';
    }

    if start < bytes.len() {
        write(&bytes[start..]);
    }

    last_was_cr.store(prev_cr, Ordering::Relaxed);
}
