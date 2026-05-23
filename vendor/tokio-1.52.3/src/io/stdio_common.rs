//! Contains utilities for stdout and stderr.
use crate::io::AsyncWrite;
use core::pin::Pin;
use core::task::{Context, Poll};
/// # Windows
/// [`AsyncWrite`] adapter that finds last char boundary in given buffer and does not write the rest,
/// if buffer contents seems to be `utf8`. Otherwise it only trims buffer down to `DEFAULT_MAX_BUF_SIZE`.
/// That's why, wrapped writer will always receive well-formed utf-8 bytes.
/// # Other platforms
/// Passes data to `inner` as is.
#[derive(Debug)]
pub(crate) struct SplitByUtf8BoundaryIfWindows<W> {
    inner: W,
}

impl<W> SplitByUtf8BoundaryIfWindows<W> {
    pub(crate) fn new(inner: W) -> Self {
        Self { inner }
    }
}

// this constant is defined by Unicode standard.
const MAX_BYTES_PER_CHAR: usize = 4;

// Subject for tweaking here
const MAGIC_CONST: usize = 8;

impl<W> crate::io::AsyncWrite for SplitByUtf8BoundaryIfWindows<W>
where
    W: AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        // just a closure to avoid repetitive code
        let mut call_inner = move |buf| Pin::new(&mut self.inner).poll_write(cx, buf);

        // 1. Only windows stdio can suffer from non-utf8.
        // We also check for `test` so that we can write some tests
        // for further code. Since `AsyncWrite` can always shrink
        // buffer at its discretion, excessive (i.e. in tests) shrinking
        // does not break correctness.
        // 2. If buffer is small, it will not be shrunk.
        // That's why, it's "textness" will not change, so we don't have
        // to fixup it.
        if cfg!(not(any(target_os = "windows", test)))
            || buf.len() <= crate::io::blocking::DEFAULT_MAX_BUF_SIZE
        {
            return call_inner(buf);
        }

        buf = &buf[..crate::io::blocking::DEFAULT_MAX_BUF_SIZE];

        // Now there are two possibilities.
        // If caller gave is binary buffer, we **should not** shrink it
        // anymore, because excessive shrinking hits performance.
        // If caller gave as binary buffer, we  **must** additionally
        // shrink it to strip incomplete char at the end of buffer.
        // that's why check we will perform now is allowed to have
        // false-positive.

        // Now let's look at the first MAX_BYTES_PER_CHAR * MAGIC_CONST bytes.
        // if they are (possibly incomplete) utf8, then we can be quite sure
        // that input buffer was utf8.

        let have_to_fix_up = match core::str::from_utf8(&buf[..MAX_BYTES_PER_CHAR * MAGIC_CONST]) {
            Ok(_) => true,
            Err(err) => {
                let incomplete_bytes = MAX_BYTES_PER_CHAR * MAGIC_CONST - err.valid_up_to();
                incomplete_bytes < MAX_BYTES_PER_CHAR
            }
        };

        if have_to_fix_up {
            // We must pop several bytes at the end which form incomplete
            // character. To achieve it, we exploit UTF8 encoding:
            // for any code point, all bytes except first start with 0b10 prefix.
            // see https://en.wikipedia.org/wiki/UTF-8#Encoding for details
            let trailing_incomplete_char_size = buf
                .iter()
                .rev()
                .take(MAX_BYTES_PER_CHAR)
                .position(|byte| *byte < 0b1000_0000 || *byte >= 0b1100_0000)
                .unwrap_or(0)
                + 1;
            buf = &buf[..buf.len() - trailing_incomplete_char_size];
        }

        call_inner(buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

