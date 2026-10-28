//! Bounded, allocation-free reader for terminal program lifecycle reports (OSC 7501).
//!
//! The `app` field is an untrusted display label and is deliberately not returned.
//! Callers associate reports with the owner of the output stream instead.

const MAX_STATUS_BYTES: usize = 128;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramStatus {
    Idle,
    Working,
    Blocked(ProgramStatusKind),
    Done,
    Error,
    Clear,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramStatusKind {
    Permission,
    Question,
    Auth,
    Unknown,
}

#[derive(Clone, Copy)]
enum State {
    Ground,
    Escape,
    Osc,
    OscEscape,
    OtherString,
    OtherStringEscape,
}

/// Observes output without consuming or modifying bytes passed to the terminal.
///
/// Reports can span any number of `feed` calls. Consecutive identical statuses
/// are emitted once; invalid sequences do not change that cache. Reset whenever
/// the stream's owner changes, so a partial escape or old status cannot leak
/// into another app's reports.
pub struct ProgramStatusParser {
    state: State,
    bytes: [u8; MAX_STATUS_BYTES],
    len: usize,
    overflow: bool,
    last: Option<ProgramStatus>,
}

impl Default for ProgramStatusParser {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgramStatusParser {
    pub const fn new() -> Self {
        Self {
            state: State::Ground,
            bytes: [0; MAX_STATUS_BYTES],
            len: 0,
            overflow: false,
            last: None,
        }
    }

    pub fn reset(&mut self) {
        self.state = State::Ground;
        self.len = 0;
        self.overflow = false;
        self.last = None;
    }

    /// Latest valid report for this stream, until its owner resets the parser.
    pub fn current(&self) -> Option<ProgramStatus> {
        self.last
    }

    pub fn feed(&mut self, chunk: &[u8], mut on_status: impl FnMut(ProgramStatus)) {
        for &byte in chunk {
            match self.state {
                State::Ground => {
                    if byte == 0x1b {
                        self.state = State::Escape;
                    }
                }
                State::Escape => match byte {
                    b']' => {
                        self.len = 0;
                        self.overflow = false;
                        self.state = State::Osc;
                    }
                    b'P' | b'X' | b'^' | b'_' => self.state = State::OtherString,
                    0x1b => {}
                    _ => self.state = State::Ground,
                },
                State::Osc => match byte {
                    0x07 => self.finish(&mut on_status),
                    0x1b => self.state = State::OscEscape,
                    0x18 | 0x1a => self.state = State::Ground,
                    0x20..=0x7e if self.len < self.bytes.len() => {
                        self.bytes[self.len] = byte;
                        self.len += 1;
                    }
                    _ => self.overflow = true,
                },
                State::OscEscape => match byte {
                    b'\\' => self.finish(&mut on_status),
                    0x18 | 0x1a => self.state = State::Ground,
                    _ => {
                        // An ESC inside the payload is not part of this protocol.
                        self.overflow = true;
                        if byte == 0x07 {
                            self.finish(&mut on_status);
                        } else if byte != 0x1b {
                            self.state = State::Osc;
                        }
                    }
                },
                State::OtherString => match byte {
                    0x1b => self.state = State::OtherStringEscape,
                    0x18 | 0x1a => self.state = State::Ground,
                    _ => {}
                },
                State::OtherStringEscape => match byte {
                    b'\\' | 0x18 | 0x1a => self.state = State::Ground,
                    0x1b => {}
                    _ => self.state = State::OtherString,
                },
            }
        }
    }

    fn finish(&mut self, on_status: &mut impl FnMut(ProgramStatus)) {
        self.state = State::Ground;
        if !self.overflow
            && let Some(status) = parse_status(&self.bytes[..self.len])
            && self.last != Some(status)
        {
            self.last = Some(status);
            on_status(status);
        }
    }
}

fn parse_status(payload: &[u8]) -> Option<ProgramStatus> {
    let fields = core::str::from_utf8(payload).ok()?.strip_prefix("7501;")?;
    let mut state = None;
    let mut app = None;
    let mut kind = None;
    for field in fields.split(':') {
        let (key, value) = field.split_once('=')?;
        match key {
            "state" if state.is_none() => state = Some(value),
            "app" if app.is_none() && !value.is_empty() => app = Some(value),
            "kind" if kind.is_none() && !value.is_empty() => kind = Some(value),
            _ => return None,
        }
    }
    if state != Some("blocked") && kind.is_some() {
        return None;
    }
    if state == Some("clear") {
        return Some(ProgramStatus::Clear);
    }
    app?;
    match state? {
        "idle" => Some(ProgramStatus::Idle),
        "working" => Some(ProgramStatus::Working),
        "done" => Some(ProgramStatus::Done),
        "error" => Some(ProgramStatus::Error),
        "blocked" => Some(ProgramStatus::Blocked(match kind {
            Some("permission") => ProgramStatusKind::Permission,
            Some("question") => ProgramStatusKind::Question,
            Some("auth") => ProgramStatusKind::Auth,
            _ => ProgramStatusKind::Unknown,
        })),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use std::vec;
    use std::vec::Vec;

    const WORKING: &[u8] = b"\x1b]7501;state=working:app=codex\x1b\\";
    const ERROR: &[u8] = b"\x1b]7501;state=error:app=codex\x07";

    #[test]
    fn reads_each_status_and_blocking_reason() {
        let mut statuses = Vec::new();
        let mut parser = ProgramStatusParser::new();
        parser.feed(
            b"\x1b]7501;state=idle:app=codex\x07\
              \x1b]7501;state=working:app=codex\x1b\\\
              \x1b]7501;state=blocked:app=codex:kind=permission\x07\
              \x1b]7501;state=blocked:app=codex:kind=question\x07\
              \x1b]7501;state=blocked:app=codex:kind=auth\x07\
              \x1b]7501;state=blocked:app=other\x07\
              \x1b]7501;state=done:app=codex\x07\
              \x1b]7501;state=error:app=codex\x07\
              \x1b]7501;state=clear\x1b\\",
            |status| statuses.push(status),
        );
        assert_eq!(
            statuses,
            vec![
                ProgramStatus::Idle,
                ProgramStatus::Working,
                ProgramStatus::Blocked(ProgramStatusKind::Permission),
                ProgramStatus::Blocked(ProgramStatusKind::Question),
                ProgramStatus::Blocked(ProgramStatusKind::Auth),
                ProgramStatus::Blocked(ProgramStatusKind::Unknown),
                ProgramStatus::Done,
                ProgramStatus::Error,
                ProgramStatus::Clear,
            ]
        );
    }

    #[test]
    fn handles_every_two_chunk_split_and_single_byte_chunks() {
        for (message, expected) in [
            (WORKING, ProgramStatus::Working),
            (ERROR, ProgramStatus::Error),
            (&b"\x1b]7501;state=clear\x1b\\"[..], ProgramStatus::Clear),
        ] {
            for split in 0..=message.len() {
                let mut parser = ProgramStatusParser::default();
                let mut statuses = Vec::new();
                parser.feed(&message[..split], |s| statuses.push(s));
                assert!(split == message.len() || statuses.is_empty());
                parser.feed(&message[split..], |s| statuses.push(s));
                assert_eq!(statuses, vec![expected], "split {split}");
            }
            let mut parser = ProgramStatusParser::new();
            let mut statuses = Vec::new();
            for byte in message.chunks(1) {
                parser.feed(byte, |s| statuses.push(s));
            }
            assert_eq!(statuses, vec![expected]);
        }
    }

    #[test]
    fn preserves_transition_order_and_deduplicates_until_reset() {
        let mut parser = ProgramStatusParser::new();
        let mut statuses = Vec::new();
        for chunk in [WORKING, WORKING, ERROR, ERROR, WORKING] {
            parser.feed(chunk, |s| statuses.push(s));
        }
        assert_eq!(
            statuses,
            vec![
                ProgramStatus::Working,
                ProgramStatus::Error,
                ProgramStatus::Working
            ]
        );
        parser.reset();
        parser.feed(WORKING, |s| statuses.push(s));
        assert_eq!(statuses.last(), Some(&ProgramStatus::Working));
        assert_eq!(statuses.len(), 4);
    }

    #[test]
    fn ignores_unrelated_escapes_and_control_string_contents() {
        let mut parser = ProgramStatusParser::new();
        let mut statuses = Vec::new();
        parser.feed(b"text\x1b[31m\x1b]0;state=error:app=codex\x07", |s| {
            statuses.push(s)
        });
        for opener in [b'P', b'X', b'^', b'_'] {
            parser.feed(&[0x1b, opener], |s| statuses.push(s));
            parser.feed(ERROR, |s| statuses.push(s));
            parser.feed(b"\x1b\\", |s| statuses.push(s));
        }
        assert!(statuses.is_empty());
        parser.feed(WORKING, |s| statuses.push(s));
        assert_eq!(statuses, vec![ProgramStatus::Working]);
    }

    #[test]
    fn ignores_malformed_unknown_and_cancelled_reports() {
        let cases: &[&[u8]] = &[
            b"\x1b]7501;state=error\x07",
            b"\x1b]7501;state=error:app=\x07",
            b"\x1b]7501;state=unknown:app=codex\x07",
            b"\x1b]7501;state=error:app=codex:app=other\x07",
            b"\x1b]7501;state=error:state=done:app=codex\x07",
            b"\x1b]7501;state=error:app=codex:kind=auth\x07",
            b"\x1b]7501;state=error:app=codex:extra=value\x07",
            b"\x1b]7501;state=blocked:app=codex:kind=\x07",
            b"\x1b]7501;state=error:app=codex\n\x07",
            b"\x1b]7501;state=error:app=codex\xff\x07",
            b"\x1b]7501;state=error:app=codex\x1b?\x07",
            b"\x1b]7501;state=error:app=codex\x18",
            b"\x1b]7501;state=error:app=codex\x1a",
        ];
        for message in cases {
            let mut parser = ProgramStatusParser::new();
            let mut statuses = Vec::new();
            parser.feed(message, |s| statuses.push(s));
            assert!(statuses.is_empty(), "{message:?}");
            parser.feed(WORKING, |s| statuses.push(s));
            assert_eq!(statuses, vec![ProgramStatus::Working]);
        }
    }

    #[test]
    fn caps_oversized_payload_and_recovers_only_at_terminator() {
        let mut parser = ProgramStatusParser::new();
        let mut statuses = Vec::new();
        parser.feed(b"\x1b]7501;state=error:app=", |s| statuses.push(s));
        parser.feed(&[b'x'; MAX_STATUS_BYTES * 4], |s| statuses.push(s));
        assert_eq!(parser.len, MAX_STATUS_BYTES);
        assert!(parser.overflow);
        parser.feed(WORKING, |s| statuses.push(s));
        assert!(statuses.is_empty());
        parser.feed(ERROR, |s| statuses.push(s));
        assert_eq!(statuses, vec![ProgramStatus::Error]);
    }

    #[test]
    fn accepts_exact_limit_and_rejects_one_byte_over() {
        let prefix = b"7501;state=working:app=";
        for extra in [0, 1] {
            let mut parser = ProgramStatusParser::new();
            let mut statuses = Vec::new();
            parser.feed(b"\x1b]", |s| statuses.push(s));
            parser.feed(prefix, |s| statuses.push(s));
            parser.feed(&vec![b'x'; MAX_STATUS_BYTES - prefix.len() + extra], |s| {
                statuses.push(s)
            });
            parser.feed(b"\x07", |s| statuses.push(s));
            if extra == 0 {
                assert_eq!(statuses, vec![ProgramStatus::Working]);
            } else {
                assert!(statuses.is_empty());
            }
        }
    }

    #[test]
    fn reset_discards_incomplete_escape_and_status() {
        let mut parser = ProgramStatusParser::new();
        let mut statuses = Vec::new();
        parser.feed(b"\x1b]7501;state=error:app=codex\x1b", |s| statuses.push(s));
        parser.reset();
        parser.feed(b"\\", |s| statuses.push(s));
        parser.feed(WORKING, |s| statuses.push(s));
        assert_eq!(statuses, vec![ProgramStatus::Working]);
    }

    #[test]
    fn ignores_app_label_and_accepts_future_blocking_kind() {
        let mut parser = ProgramStatusParser::new();
        let mut statuses = Vec::new();
        parser.feed(
            b"\x1b]7501;app=not-authority:kind=network:state=blocked\x07",
            |s| statuses.push(s),
        );
        assert_eq!(
            statuses,
            vec![ProgramStatus::Blocked(ProgramStatusKind::Unknown)]
        );
    }
}
