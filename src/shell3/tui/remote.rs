//! Bounded, opaque terminal output for remote frontends. No ANSI interpretation.
use alloc::{collections::VecDeque, vec::Vec};

const LIMIT: usize = super::super::tty::OUTPUT_LIMIT;
// Re-establish Shell3's alternate screen even if the app exited without its guard.
const RESET: &[u8] = b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1004l\x1b[?1006l\x1b[?1015l\x1b[?2004l\x1b[?7h\x1b[0m\x1b[r\x1b[?25h\x1b[?1049h\x1b[2J\x1b[H";

pub(super) struct Output {
    bytes: VecDeque<u8>,
    repaint: bool,
    failed: bool,
}

pub(crate) struct Drain {
    pub bytes: Vec<u8>,
    pub pending: bool,
    pub repaint: bool,
    pub failed: bool,
    pub active: bool,
}

impl Output {
    pub const fn new() -> Self {
        Self { bytes: VecDeque::new(), repaint: false, failed: false }
    }

    pub fn begin(&mut self) -> bool {
        // Always reserve the release reset, including for an abandoned app.
        if self.failed || self.bytes.len() + RESET.len() * 2 > LIMIT { return false; }
        self.bytes.extend(RESET.iter().copied());
        true
    }

    pub fn write(&mut self, bytes: &[u8]) -> usize {
        if self.failed { return 0; }
        if bytes.len() > LIMIT.saturating_sub(RESET.len() + self.bytes.len()) {
            self.failed = true;
            self.end();
            return 0;
        }
        self.bytes.extend(bytes.iter().copied());
        bytes.len()
    }

    pub fn end(&mut self) {
        if self.bytes.len() + RESET.len() <= LIMIT {
            self.bytes.extend(RESET.iter().copied());
        } else { self.failed = true; }
        self.repaint = true;
    }

    pub fn take(&mut self, available: usize, active: bool) -> Drain {
        let count = available.min(self.bytes.len());
        let bytes = self.bytes.drain(..count).collect();
        let pending = !self.bytes.is_empty();
        // Shell3 must not paint until all app bytes and its release reset drain.
        let repaint = !pending && core::mem::take(&mut self.repaint);
        Drain {bytes, pending, repaint, failed: self.failed, active}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_preserves_fragmented_ansi_unicode_and_queries() {
        let mut output = Output::new();
        assert!(output.begin());
        assert_eq!(output.take(LIMIT, true).bytes, RESET);
        for part in [b"\x1b[38;5;8m".as_slice(), "🗺 §".as_bytes(), b"\x1b[18", b"t\x1b[6n"] {
            assert_eq!(output.write(part), part.len());
        }
        assert_eq!(output.take(LIMIT, true).bytes, "\x1b[38;5;8m🗺 §\x1b[18t\x1b[6n".as_bytes());
    }

    #[test]
    fn fast_claim_write_release_drains_before_shell_repaint() {
        let mut output = Output::new();
        assert!(output.begin());
        output.write(b"last app frame");
        output.end();
        let first = output.take(3, false);
        assert!(first.pending && !first.repaint);
        let final_part = output.take(LIMIT, false);
        assert!(final_part.repaint && !final_part.pending);
        let mut all = first.bytes; all.extend(final_part.bytes);
        assert_eq!(all, [RESET, b"last app frame", RESET].concat());
        assert!(!output.take(LIMIT, false).repaint);
    }

    #[test]
    fn overflow_is_bounded_and_release_still_has_room() {
        let mut output = Output::new();
        assert!(output.begin());
        let payload = alloc::vec![b'x'; LIMIT - RESET.len() * 2];
        assert_eq!(output.write(&payload), payload.len());
        assert_eq!(output.write(b"overflow"), 0);
        output.end();
        let drain = output.take(LIMIT, false);
        assert!(drain.failed && drain.repaint);
        assert_eq!(drain.bytes.len(), LIMIT);
        assert!(drain.bytes.ends_with(RESET));
        assert!(!output.begin());
    }
}
