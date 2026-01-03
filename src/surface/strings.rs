//! String helpers that are small, allocation-friendly, and kernel-centric.

use alloc::format;

use crate::surface::{
    fmt::{self, Write},
    string::String,
    vec::Vec,
};

pub fn smoke_test() {
    crate::log!("string smoke test begin\n");

    let stats0 = crate::allocators::heap_stats();
    crate::log!(
        "heap before: free_bytes={} largest_free={} free_blocks={} init={}\n",
        stats0.free_bytes,
        stats0.largest_free_block,
        stats0.free_blocks,
        stats0.initialized
    );

    let ascii: &str = "Hello, FalseOS!";
    crate::log!("&str='{}' len={}\n", ascii, ascii.len());
    match ensure_ascii(ascii) {
        Ok(()) => crate::log!("ensure_ascii(ascii)=Ok\n"),
        Err(e) => crate::log!(
            "ensure_ascii(ascii)=Err index={} byte=0x{:02X}\n",
            e.index,
            e.byte
        ),
    }

    let non_ascii: &str = "Grüße";
    match ensure_ascii(non_ascii) {
        Ok(()) => crate::log!("ensure_ascii(non_ascii)=Ok (unexpected)\n"),
        Err(e) => crate::log!(
            "ensure_ascii(non_ascii)=Err index={} byte=0x{:02X}\n",
            e.index,
            e.byte
        ),
    }

    let sanitized = sanitize_ascii("A\tB\nC\rD");
    crate::log!("sanitize_ascii='{}'\n", sanitized);

    let mut heap_string = String::from("heap String");
    heap_string.push(' ');
    heap_string.push_str("OK");
    heap_string.push_str(&format!(
        " (len={}, cap={})",
        heap_string.len(),
        heap_string.capacity()
    ));
    crate::log!("String='{}'\n", heap_string);

    let dump = hex_dump(&[0x00, 0x01, 0x41, 0x7F, 0x80, 0xFF]);
    crate::log!("hex_dump:\n{}", dump);

    let stats1 = crate::allocators::heap_stats();
    crate::log!(
        "heap after:  free_bytes={} largest_free={} free_blocks={} init={}\n",
        stats1.free_bytes,
        stats1.largest_free_block,
        stats1.free_blocks,
        stats1.initialized
    );

    crate::log!("string smoke test end\n");
}

/// Error returned when non-ASCII data is encountered in an ASCII-only routine.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct NonAsciiError {
    pub index: usize,
    pub byte: u8,
}

/// Ensure every byte in `input` is strictly ASCII.
pub fn ensure_ascii(input: &str) -> Result<(), NonAsciiError> {
    for (idx, byte) in input.bytes().enumerate() {
        if byte >= 0x80 {
            return Err(NonAsciiError { index: idx, byte });
        }
    }
    Ok(())
}

/// Replace non-printable bytes with '.' so strings can be logged safely.
pub fn sanitize_ascii(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        if (0x20..=0x7E).contains(&b) {
            out.push(b as char);
        } else {
            out.push('.');
        }
    }
    out
}

/// Return true if the string is empty or contains only ASCII whitespace.
pub fn is_blank(input: &str) -> bool {
    input
        .bytes()
        .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\x0C'))
}

/// Split `line` on the first `=` sign (common for bootloader key/value blobs).
pub fn split_key_value(line: &str) -> Option<(&str, &str)> {
    line.split_once('=')
        .map(|(k, v)| (k.trim(), v.trim()))
        .filter(|(k, _)| !k.is_empty())
}

/// Render a classic hex + ASCII dump for quick debugging.
pub fn hex_dump(data: &[u8]) -> String {
    const BYTES_PER_ROW: usize = 16;
    if data.is_empty() {
        return String::new();
    }

    let mut buf = String::with_capacity(data.len() * 4);
    for (row_idx, chunk) in data.chunks(BYTES_PER_ROW).enumerate() {
        let _ = write!(buf, "{:04X}: ", row_idx * BYTES_PER_ROW);

        for i in 0..BYTES_PER_ROW {
            if let Some(byte) = chunk.get(i) {
                let _ = write!(buf, "{:02X} ", byte);
            } else {
                buf.push_str("   ");
            }
        }

        buf.push('|');
        for byte in chunk {
            buf.push(if (0x20..=0x7E).contains(byte) {
                *byte as char
            } else {
                '.'
            });
        }
        buf.push('|');
        buf.push('\n');
    }

    buf
}

/// Collect UTF-8 bytes from `input` into a `Vec<u8>` without reallocating.
pub fn into_bytes(input: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    out.extend_from_slice(input.as_bytes());
    out
}

/// Append a single line with left/right padding into `buffer`.
pub fn append_padded_line(buffer: &mut String, left: &str, right: &str, width: usize) {
    let mut line = String::with_capacity(width);
    line.push_str(left);
    if left.len() < width {
        let padding = width - left.len().min(width);
        for _ in 0..padding.saturating_sub(right.len()) {
            line.push(' ');
        }
    }
    line.push_str(right);
    if !line.ends_with('\n') {
        line.push('\n');
    }
    buffer.push_str(&line);
}
