const REPLACEMENT: char = '\u{FFFD}';

#[derive(Clone, Copy)]
pub(crate) struct Decoder {
    bytes: [u8; 4],
    len: u8,
    expected_len: u8,
}

impl Decoder {
    pub(crate) const fn new() -> Self {
        Self {
            bytes: [0; 4],
            len: 0,
            expected_len: 0,
        }
    }

    pub(crate) const fn is_pending(&self) -> bool {
        self.len != 0
    }

    pub(crate) fn reset(&mut self) {
        *self = Self::new();
    }

    pub(crate) fn finish_lossy(&mut self) -> Option<char> {
        if !self.is_pending() {
            return None;
        }
        self.reset();
        Some(REPLACEMENT)
    }

    pub(crate) fn push(&mut self, byte: u8) -> Decoded {
        if !self.is_pending() {
            return self.start(byte);
        }

        if (byte & 0xC0) != 0x80 {
            self.reset();
            let next = self.start(byte);
            return Decoded([Some(REPLACEMENT), next.0[0]]);
        }

        self.bytes[usize::from(self.len)] = byte;
        self.len += 1;
        if self.len < self.expected_len {
            return Decoded::pending();
        }

        let len = usize::from(self.expected_len);
        let ch = core::str::from_utf8(&self.bytes[..len])
            .ok()
            .and_then(|text| text.chars().next())
            .unwrap_or(REPLACEMENT);
        self.reset();
        Decoded::one(ch)
    }

    fn start(&mut self, byte: u8) -> Decoded {
        if byte.is_ascii() {
            return Decoded::one(char::from(byte));
        }

        let expected_len = match byte {
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => return Decoded::one(REPLACEMENT),
        };
        self.bytes[0] = byte;
        self.len = 1;
        self.expected_len = expected_len;
        Decoded::pending()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Decoded([Option<char>; 2]);

impl Decoded {
    const fn pending() -> Self {
        Self([None, None])
    }

    const fn one(ch: char) -> Self {
        Self([Some(ch), None])
    }

    pub(crate) fn chars(self) -> impl Iterator<Item = char> {
        self.0.into_iter().flatten()
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use super::Decoder;

    fn decode(bytes: &[u8]) -> alloc::string::String {
        let mut decoder = Decoder::new();
        let mut out = alloc::string::String::new();
        for &byte in bytes {
            out.extend(decoder.push(byte).chars());
        }
        if let Some(ch) = decoder.finish_lossy() {
            out.push(ch);
        }
        out
    }

    #[test]
    fn decodes_international_utf8() {
        let text = "font \"§ Ü 中国 العربية 한국어 🦀\"";
        assert_eq!(decode(text.as_bytes()), text);
    }

    #[test]
    fn retains_a_scalar_across_reads() {
        let mut decoder = Decoder::new();
        let bytes = "🌍".as_bytes();
        for &byte in &bytes[..bytes.len() - 1] {
            assert_eq!(decoder.push(byte).chars().next(), None);
            assert!(decoder.is_pending());
        }
        assert_eq!(decoder.push(bytes[3]).chars().next(), Some('🌍'));
        assert!(!decoder.is_pending());
    }

    #[test]
    fn malformed_sequence_does_not_consume_following_ascii() {
        assert_eq!(decode(&[0xE4, 0xB8, b'X']), "\u{FFFD}X");
    }

    #[test]
    fn malformed_input_uses_replacement() {
        assert_eq!(decode(&[0xFF]), "\u{FFFD}");
        assert_eq!(decode(&[0xC0, 0xAF]), "\u{FFFD}\u{FFFD}");
        assert_eq!(decode(&[0xED, 0xA0, 0x80]), "\u{FFFD}");
        assert_eq!(decode(&[0xF4, 0x90, 0x80, 0x80]), "\u{FFFD}");
        assert_eq!(decode(&[0xE4, 0xB8]), "\u{FFFD}");
    }
}
