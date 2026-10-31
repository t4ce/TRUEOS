#![allow(dead_code)]
extern crate alloc;
extern crate self as zeroize;
extern crate self as spin;
extern crate self as trueos_terminal;

use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};
use std::sync::{LazyLock, Mutex as StdMutex, MutexGuard};

pub trait Zeroize { fn zeroize(&mut self); }
impl Zeroize for String { fn zeroize(&mut self) { self.clear(); } }
impl<T: Default + Copy> Zeroize for [T] { fn zeroize(&mut self) { self.fill(T::default()); } }
impl<T> Zeroize for Vec<T> { fn zeroize(&mut self) { self.clear(); } }
pub struct Zeroizing<T: Zeroize>(T);
impl<T: Zeroize> Zeroizing<T> { pub fn new(value: T) -> Self { Self(value) } }
impl<T: Zeroize> Deref for Zeroizing<T> { type Target = T; fn deref(&self) -> &T { &self.0 } }
impl<T: Zeroize> DerefMut for Zeroizing<T> { fn deref_mut(&mut self) -> &mut T { &mut self.0 } }
impl<T: Zeroize> Drop for Zeroizing<T> { fn drop(&mut self) { self.0.zeroize(); } }
pub struct Mutex<T>(StdMutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value: T) -> Self { Self(StdMutex::new(value)) }
    pub fn lock(&self) -> MutexGuard<'_, T> { self.0.lock().unwrap() }
}

#[derive(Clone, Copy, Default, PartialEq)]
pub enum MouseTracking { #[default] Off, Buttons, Drag, Any }
#[derive(Clone, Copy, Default, PartialEq)]
pub enum MouseEncoding { #[default] Legacy, Sgr, Urxvt }
#[derive(Clone, Copy, Default, PartialEq)]
pub struct MouseOptions { pub tracking: MouseTracking, pub encoding: MouseEncoding }

mod crypt { pub fn authenticated_history_identity(_: u8) -> Option<(u64, u64)> { None } }
mod user_input_record { pub fn capture(_: u8, _: &str) { panic!("paste recorded as keyboard history"); } }
mod r {
    pub mod keyboard {
        pub const KEYBOARD_OUTPUT_KIND_KEY: u8 = 1;
        pub const KEYBOARD_OUTPUT_KIND_TEXT: u8 = 2;
        pub const KEYBOARD_OUTPUT_FLAG_PRESS: u8 = 1;
        pub const KEYBOARD_KEY_TAB: u16 = 1;
        pub const KEYBOARD_KEY_ENTER: u16 = 2;
        pub const KEYBOARD_KEY_BACKSPACE: u16 = 3;
        #[derive(Default)]
        pub struct TrueosKeyboardOutputEvent {
            pub kind: u8, pub key_code: u16, pub codepoint: u32,
            pub utf8: [u8; 4], pub utf8_len: u8, pub flags: u8,
        }
    }
}

mod shell3 {
    use super::*;
    #[derive(Clone, Copy, PartialEq)]
    pub(super) enum RgbaColor {
        Pink, BlackTransparent,
        Blinking { foreground: [u8; 4], background: Option<[u8; 4]>, underline: bool },
        Terminal { foreground: [u8; 4], background: [u8; 4], underline: bool },
    }
    impl RgbaColor {
        fn rgba(self) -> [u8; 4] { [0; 4] }
        fn background(self) -> Option<[u8; 4]> { None }
        fn underline(self) -> bool { false }
        fn blink(self) -> bool { false }
    }
    struct MetaFmtStr { text: String, color: Option<RgbaColor>, underline: bool, blink: bool }
    impl MetaFmtStr {
        fn new(text: impl Into<String>) -> Self { Self { text: text.into(), color: None, underline: false, blink: false } }
        fn color(mut self, color: RgbaColor) -> Self { self.color = Some(color); self }
    }
    enum SpecialRows { TitleRow, StatusRow, PromtRow, MatrixRow(usize) }
    mod update {
        use super::*;
        pub type RenderedLine = Vec<(char, Option<RgbaColor>)>;
        pub const MATRIX_BACKGROUND: [u8; 4] = [0; 4];
        pub const CONTROL_BACKGROUND: [u8; 4] = [0; 4];
        pub fn cell_color(_: Option<RgbaColor>, _: [u8; 4]) -> RgbaColor { RgbaColor::Pink }
        pub struct Update { pub row: SpecialRows, pub offset: usize, pub text: String, pub colors: Vec<Option<RgbaColor>>, pub remove: usize }
        pub fn diff_rendered_lines(_: Option<&Vec<RenderedLine>>, _: &Vec<RenderedLine>) -> Vec<Update> { Vec::new() }
    }
    struct Snapshot;
    impl Snapshot { fn rendered_lines(self) -> Vec<update::RenderedLine> { Vec::new() } }
    pub(super) mod matrix_target {
        pub const TRANSPORT_LOCAL_SCOPE: u8 = 0;
        pub const TRANSPORT_NET_TCP_SCOPE: u8 = 1;
    }
    #[path = "@TRUEOS_ROOT@/src/shell3/command_history.rs"]
    pub(super) mod command_history;
    pub(super) mod service { pub fn notify_work() {} }
    #[derive(Default)]
    struct MatrixSlotsState {
        lifetimes: Vec<(String, u64)>,
        echoes: Vec<(Option<String>, VecDeque<String>)>,
        generation: u64,
    }
    static MATRIX_SLOTS: LazyLock<Mutex<MatrixSlotsState>> =
        LazyLock::new(|| Mutex::new(MatrixSlotsState::default()));
    fn matrix_slots() -> &'static Mutex<MatrixSlotsState> { &MATRIX_SLOTS }
    pub(super) struct MatrixSlots;
    impl MatrixSlots {
        @MATRIX_PASTE_METHOD@
    }

    pub(super) mod tui {
        pub(super) @REMOTE_RESET@
        #[path = "@TRUEOS_ROOT@/src/shell3/tui/remote.rs"]
        mod remote;
        pub(super) use remote::Drain as RemoteDrain;
        #[derive(Default)]
        pub struct State {
            pub remote: bool, pub native: bool, pub painted: bool,
            pub raw: Vec<u8>, pub drain: Option<RemoteDrain>,
            pub drain_limit: Option<usize>, queue: Option<remote::Output>,
        }
        pub static STATE: std::sync::LazyLock<super::Mutex<State>> =
            std::sync::LazyLock::new(|| super::Mutex::new(State::default()));
        pub fn remote_active(_: u64) -> bool { STATE.lock().remote }
        pub fn active(_: u64, _: Option<&str>) -> bool { let state = STATE.lock(); state.remote || state.native }
        pub fn input(_: u64, _: Option<&str>, bytes: &[u8]) -> bool {
            let mut state = STATE.lock();
            if state.remote || state.native { state.raw.extend_from_slice(bytes); true } else { false }
        }
        pub fn take_remote_output(_: u64, available: usize) -> Option<RemoteDrain> {
            let mut state = STATE.lock();
            if let Some(drain) = state.drain.take() { return Some(drain); }
            let active = state.remote;
            let available = state.drain_limit.unwrap_or(available).min(available);
            Some(state.queue.as_mut()?.take(available, active))
        }
        pub fn begin_remote() {
            let mut state = STATE.lock();
            let mut output = remote::Output::new();
            assert!(output.begin());
            state.queue = Some(output);
            state.remote = true;
        }
        pub fn write_remote(bytes: &[u8]) {
            assert_eq!(STATE.lock().queue.as_mut().unwrap().write(bytes), bytes.len());
        }
        pub fn release_remote() {
            let mut state = STATE.lock();
            state.remote = false;
            state.queue.as_mut().unwrap().end();
        }
        pub fn snapshot(_: u64, _: Option<&str>) -> Option<Vec<super::update::RenderedLine>> {
            let state = STATE.lock();
            ((state.remote || state.native) && state.painted).then(Vec::new)
        }
        pub fn mouse_options(_: u64, _: Option<&str>) -> crate::MouseOptions { Default::default() }
    }
    struct Shell3 {
        tui_frontend: u64,
        active_matrix_slot: Option<String>,
        active_matrix_lifetime: Option<u64>,
        prompt: String,
        keyboard: Vec<(u16, u32)>,
        replay: Vec<String>,
        notices: Vec<String>,
        scroll: Vec<i32>,
        drag_calls: usize,
        mode: u8,
        vmx: bool,
        matrix_scroll: i32,
        matrix_column: usize,
        matrix_drag: Option<()>,
    }
    impl Shell3 {
        fn prompt(&self) -> &str { &self.prompt }
        fn set_prompt(&mut self, text: &str) { self.prompt = text.into(); }
        fn set_cursor(&mut self, _: usize) {}
        fn parse_name(&self, _: &str) -> bool { false }
        fn handle_keyboard_with_latch(&mut self, event: &r::keyboard::TrueosKeyboardOutputEvent) -> (bool, bool) {
            self.keyboard.push((event.key_code, event.codepoint));
            if event.kind == r::keyboard::KEYBOARD_OUTPUT_KIND_TEXT {
                self.prompt.push(char::from_u32(event.codepoint).unwrap());
            }
            (true, false)
        }
        fn replay_terminal_line(&mut self, text: &str) { self.replay.push(text.into()); }
        fn get_mode(&self) -> u8 { self.mode }
        fn set_mode(&mut self, mode: u8) { self.mode = mode; }
        fn active_vmx_app(&self) -> Option<u8> { self.vmx.then_some(1) }
        fn active_matrix_slot_name(&self) -> Option<String> { self.active_matrix_slot.clone() }
        fn get_size(&self) -> (usize, usize) { (80, 24) }
        fn capture_matrix_snapshot(&self) -> Snapshot { Snapshot }
        fn scroll_matrix(&mut self, rows: i32) { self.scroll.push(rows); }
        fn drag_matrix(&mut self, _: (i32, i32), _: bool, _: bool, _: bool, _: (usize, usize)) { self.drag_calls += 1; }
        fn record_terminal_notice(&mut self, text: &str) { self.notices.push(text.into()); }
        @SHELL_PASTE_METHOD@
    }

    pub(super) mod tty {
        use super::{Shell3, MetaFmtStr, RgbaColor, SpecialRows, command_history::CommandHistory};
        const LINE_LIMIT: usize = 1024;
        pub(super) const OUTPUT_LIMIT: usize = 1024 * 1024;
        #[path = "@TRUEOS_ROOT@/src/shell3/tty/ansi.rs"]
        mod ansi;
        #[path = "@TRUEOS_ROOT@/src/shell3/tty/paste.rs"]
        mod paste;
        #[path = "@TRUEOS_ROOT@/src/shell3/tty/input.rs"]
        mod input;
        pub(super) struct Terminal {
            pub shell: Shell3,
            pub line: String,
            pub line_overflow: bool,
            pub history: CommandHistory,
            decoder: input::InputDecoder,
            view: ansi::AnsiView,
            prompt_name: String,
            pub output: Vec<u8>,
            pub closing: bool,
            pub overflow: bool,
        }
        impl Terminal {
            pub(super) fn new(matrix: bool) -> Self {
                Self {
                    shell: Shell3 { tui_frontend: 1, active_matrix_slot: Some("first".into()),
                        active_matrix_lifetime: Some(10), prompt: "keep this prompt".into(),
                        keyboard: Vec::new(), replay: Vec::new(), notices: Vec::new(),
                        scroll: Vec::new(), drag_calls: 0, mode: 1, vmx: false,
                        matrix_scroll: 0, matrix_column: 0, matrix_drag: None },
                    line: "keep this prompt".into(), line_overflow: false,
                    history: CommandHistory::default(), decoder: input::InputDecoder::default(),
                    view: ansi::AnsiView::new(matrix), prompt_name: "first".into(), output: Vec::new(),
                    closing: false, overflow: false,
                }
            }
            fn write(&mut self, bytes: &[u8]) { self.output.extend_from_slice(bytes); }
            fn notice(&mut self, text: &str) { self.shell.record_terminal_notice(text); }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        const START: &[u8] = b"\x1b[200~";
        const END: &[u8] = b"\x1b[201~";
        fn setup(matrix: bool) -> tty::Terminal {
            *matrix_slots().lock() = MatrixSlotsState {
                lifetimes: vec![("first".into(), 10), ("second".into(), 20)],
                ..Default::default()
            };
            *tui::STATE.lock() = tui::State::default();
            tty::Terminal::new(matrix)
        }
        fn pasted(text: &[u8]) -> Vec<u8> { [START, text, END].concat() }
        fn echoes(name: Option<&str>) -> Vec<String> {
            matrix_slots().lock().echoes.iter()
                .find(|(id, _)| id.as_deref() == name)
                .map(|(_, lines)| lines.iter().cloned().collect()).unwrap_or_default()
        }
        fn context(name: Option<&str>, text: &str) {
            let mut expected = vec!["Pasted context:".to_string()];
            expected.extend(text.split('\n').map(str::to_string));
            assert_eq!(echoes(name), expected);
        }
        fn untouched(terminal: &tty::Terminal) {
            assert_eq!(terminal.shell.prompt, "keep this prompt");
            assert_eq!(terminal.line, "keep this prompt");
            assert!(terminal.shell.keyboard.is_empty());
            assert!(terminal.shell.replay.is_empty());
            assert!(terminal.shell.scroll.is_empty());
            assert_eq!(terminal.shell.drag_calls, 0);
            assert!(terminal.history.entries.is_empty());
            assert!(!terminal.closing);
            assert!(!terminal.output.windows(b"\x1b[2J".len()).any(|bytes| bytes == b"\x1b[2J"));
            assert_eq!(terminal.shell.mode, 1);
        }

        #[test]
        fn fragmented_utf8_paste_is_one_context_and_never_keyboard_or_commands() {
            let text = "exit\r\nclear\ntab\nram\t§ Grüß 🦉\x03\x04\x15\x1b[A\x1b[<1;1;4M\x1b[200~nested";
            let rendered = "exit\nclear\ntab\nram     § Grüß 🦉\\u{3}\\u{4}\\u{15}\\u{1b}[A\\u{1b}[<1;1;4M\\u{1b}[200~nested";
            let bytes = pasted(text.as_bytes());
            for split in 0..=bytes.len() {
                let mut terminal = setup(true);
                terminal.input(&bytes[..split]);
                terminal.input(&bytes[split..]);
                untouched(&terminal);
                context(Some("first"), rendered);
                assert!(echoes(Some("second")).is_empty());
                assert!(echoes(None).is_empty());
            }
            let mut terminal = setup(true);
            for byte in bytes { terminal.input(&[byte]); }
            untouched(&terminal);
            context(Some("first"), rendered);
        }

        #[test]
        fn paste_uses_the_slot_selected_at_start() {
            let mut terminal = setup(true);
            terminal.input(START);
            terminal.shell.active_matrix_slot = Some("second".into());
            terminal.shell.active_matrix_lifetime = Some(20);
            terminal.input(b"whole context");
            assert!(echoes(Some("first")).is_empty());
            terminal.input(END);
            untouched(&terminal);
            context(Some("first"), "whole context");
            assert!(echoes(Some("second")).is_empty());
            assert!(echoes(None).is_empty());
        }

        #[test]
        fn stale_slot_lifetime_rejects_context_without_falling_back_to_bare_slot() {
            let mut terminal = setup(true);
            terminal.input(START);
            matrix_slots().lock().lifetimes[0].1 = 11;
            terminal.input(b"not for replacement");
            terminal.input(END);
            untouched(&terminal);
            assert!(matrix_slots().lock().echoes.is_empty());
        }

        #[test]
        fn bare_slot_gets_pasted_context() {
            let mut terminal = setup(true);
            terminal.shell.active_matrix_slot = None;
            terminal.shell.active_matrix_lifetime = None;
            terminal.input(&pasted(b"bare context"));
            untouched(&terminal);
            context(None, "bare context");
            assert!(echoes(Some("first")).is_empty());
        }

        #[test]
        fn overflow_discards_everything_until_end_then_next_paste_works() {
            let mut terminal = setup(true);
            terminal.input(START);
            for _ in 0..65 { terminal.input(&[b'x'; 1024]); }
            terminal.input(b"\r\nexit\x03\x04\x1b[A");
            assert!(matrix_slots().lock().echoes.is_empty());
            terminal.input(END);
            untouched(&terminal);
            assert!(matrix_slots().lock().echoes.is_empty());
            assert!(!terminal.shell.notices.is_empty());
            terminal.input(&pasted(b"recovered"));
            context(Some("first"), "recovered");
        }

        #[test]
        fn exactly_64_kib_is_accepted_and_one_more_byte_discards_it() {
            for length in [64 * 1024, 64 * 1024 + 1] {
                let mut terminal = setup(true);
                let text = "x".repeat(length);
                terminal.input(&pasted(text.as_bytes()));
                untouched(&terminal);
                if length == 64 * 1024 {
                    context(Some("first"), &text);
                    assert!(terminal.shell.notices.is_empty());
                } else {
                    assert!(matrix_slots().lock().echoes.is_empty());
                    assert_eq!(terminal.shell.notices.len(), 1);
                }
            }
        }

        #[test]
        fn partial_end_markers_and_overlapping_escape_bytes_remain_payload() {
            let mut terminal = setup(true);
            let bytes = pasted(b"a\x1b[20x\x1b\x1b[201x\x1b[201");
            for byte in &bytes { terminal.input(&[*byte]); }
            untouched(&terminal);
            context(Some("first"), "a\\u{1b}[20x\\u{1b}\\u{1b}[201x\\u{1b}[201");
        }

        #[test]
        fn ss3_is_not_a_bracketed_paste_start() {
            let mut terminal = setup(true);
            for byte in b"\x1bO200~literal\x1b[201~" { terminal.input(&[*byte]); }
            assert!(matrix_slots().lock().echoes.is_empty());
            assert_eq!(terminal.shell.prompt, "keep this promptliteral");
            assert_eq!(terminal.shell.keyboard.len(), 7);
            assert!(!terminal.closing);
        }

        #[test]
        fn plain_tcp_retains_line_input_behavior() {
            let mut terminal = setup(false);
            terminal.input(&pasted(b"literal"));
            assert!(matrix_slots().lock().echoes.is_empty());
            assert_eq!(terminal.shell.prompt, "keep this promptliteral");
            assert_eq!(terminal.line, "keep this promptliteral");
            assert!(terminal.shell.keyboard.is_empty());
            assert!(!terminal.output.windows(b"\x1b[?2004h".len()).any(|bytes| bytes == b"\x1b[?2004h"));
        }

        #[test]
        fn empty_paste_does_not_create_context() {
            let mut terminal = setup(true);
            terminal.input(&pasted(b""));
            untouched(&terminal);
            assert!(matrix_slots().lock().echoes.is_empty());
        }

        #[test]
        fn malformed_utf8_is_replaced_and_control_bytes_are_rendered_as_text() {
            let mut terminal = setup(true);
            terminal.input(&pasted(b"\xff\xf0\x9f\r\none\ttwo\rthree\x00\x7f\x1b[2J"));
            untouched(&terminal);
            context(Some("first"), "��\none     two\nthree\\u{0}\\u{7f}\\u{1b}[2J");
        }

        #[test]
        fn transcript_limit_keeps_header_and_first_255_pasted_rows() {
            let mut terminal = setup(true);
            let text = (0..300).map(|row| format!("row {row}")).collect::<Vec<_>>().join("\n");
            terminal.input(&pasted(text.as_bytes()));
            untouched(&terminal);
            let lines = echoes(Some("first"));
            assert_eq!(lines.len(), 256);
            assert_eq!(lines[0], "Pasted context (first 255 of 300 lines):");
            assert_eq!(lines[1], "row 0");
            assert_eq!(lines[255], "row 254");
        }

        #[test]
        fn remote_app_keeps_every_byte_including_split_unicode_and_delimiters() {
            let mut terminal = setup(true);
            tui::STATE.lock().remote = true;
            let bytes = pasted("exit\r\n§ 🦉\x03\x04".as_bytes());
            for byte in &bytes { terminal.input(&[*byte]); }
            assert_eq!(tui::STATE.lock().raw, bytes);
            assert!(matrix_slots().lock().echoes.is_empty());
            assert!(terminal.shell.keyboard.is_empty());
            assert!(!terminal.closing);
        }

        #[test]
        fn native_app_keeps_ascii_paste_bytes() {
            let mut terminal = setup(true);
            tui::STATE.lock().native = true;
            let bytes = pasted(b"exit\r\nclear\t\x03\x04");
            for byte in &bytes { terminal.input(&[*byte]); }
            assert_eq!(tui::STATE.lock().raw, bytes);
            assert!(matrix_slots().lock().echoes.is_empty());
            assert!(terminal.shell.keyboard.is_empty());
            assert!(!terminal.closing);
        }

        #[test]
        fn in_progress_shell_paste_cannot_leak_into_newly_active_app() {
            let mut terminal = setup(true);
            terminal.input(START);
            terminal.input(b"before ");
            tui::STATE.lock().remote = true;
            terminal.input(b"after\r\nexit\x03\x04");
            terminal.input(END);
            assert!(tui::STATE.lock().raw.is_empty());
            context(Some("first"), "before after\nexit\\u{3}\\u{4}");
            assert!(terminal.shell.keyboard.is_empty());
            assert!(!terminal.closing);
        }

        #[test]
        fn same_frame_text_after_paste_returns_to_normal_prompt_input() {
            let mut terminal = setup(true);
            let bytes = [pasted(b"context"), b"z".to_vec()].concat();
            terminal.input(&bytes);
            context(Some("first"), "context");
            assert_eq!(terminal.shell.prompt, "keep this promptz");
            assert_eq!(terminal.shell.keyboard, vec![(0, b'z' as u32)]);
            assert!(terminal.history.entries.is_empty());
        }

        #[test]
        fn transport_eof_cancels_unfinished_paste_and_closes() {
            let mut terminal = setup(true);
            terminal.input(START);
            terminal.input(b"unfinished context");
            terminal.input_eof();
            assert!(terminal.closing);
            assert!(terminal.shell.prompt.is_empty());
            assert!(terminal.line.is_empty());
            assert!(matrix_slots().lock().echoes.is_empty());
            assert!(terminal.shell.keyboard.is_empty());
            assert!(terminal.history.entries.is_empty());
            assert!(terminal.output.windows(b"\x1b[?2004l".len()).any(|bytes| bytes == b"\x1b[?2004l"));
        }

        fn mode_count(terminal: &tty::Terminal, enabled: bool) -> usize {
            let marker: &[u8] = if enabled { b"\x1b[?2004h" } else { b"\x1b[?2004l" };
            terminal.output.windows(marker.len()).filter(|bytes| *bytes == marker).count()
        }

        #[test]
        fn bracketed_paste_mode_tracks_native_app_ownership_and_return() {
            let mut terminal = setup(true);
            terminal.input(b"");
            assert_eq!(mode_count(&terminal, true), 1);
            terminal.input(b"");
            assert_eq!(mode_count(&terminal, true), 1);
            tui::STATE.lock().native = true;
            terminal.input(b"");
            assert_eq!(mode_count(&terminal, false), 1);
            assert_eq!(mode_count(&terminal, true), 1);
            // Native input ownership begins before its first rendered frame.
            tui::STATE.lock().painted = true;
            terminal.input(b"");
            assert_eq!(mode_count(&terminal, false), 1);
            assert_eq!(mode_count(&terminal, true), 1);
            tui::STATE.lock().native = false;
            terminal.input(b"");
            assert_eq!(mode_count(&terminal, true), 2);
            terminal.input(b"");
            assert_eq!(mode_count(&terminal, true), 2);
        }

        #[test]
        fn bracketed_paste_mode_is_restored_after_remote_reset_and_release() {
            let mut terminal = setup(true);
            terminal.input(b"");
            assert_eq!(mode_count(&terminal, true), 1);
            {
                let mut state = tui::STATE.lock();
                state.remote = true;
                state.drain = Some(tui::RemoteDrain {
                    bytes: tui::RESET.to_vec(), active: true, pending: false, failed: false, repaint: false,
                });
            }
            terminal.input(b"");
            assert_eq!(mode_count(&terminal, false), 1);
            assert_eq!(mode_count(&terminal, true), 1);
            {
                let mut state = tui::STATE.lock();
                state.remote = false;
                state.drain = Some(tui::RemoteDrain {
                    bytes: tui::RESET.to_vec(), active: false, pending: false, failed: false, repaint: true,
                });
            }
            terminal.input(b"");
            assert_eq!(mode_count(&terminal, false), 2);
            assert_eq!(mode_count(&terminal, true), 2);
            terminal.input(b"");
            assert_eq!(mode_count(&terminal, true), 2);
        }

        #[test]
        fn raw_remote_output_retains_its_own_bracketed_paste_mode() {
            let mut terminal = setup(true);
            terminal.input(b"");
            terminal.output.clear();
            tui::begin_remote();
            terminal.input(b"");
            assert_eq!(terminal.output, tui::RESET);
            terminal.output.clear();
            let app = b"app frame\x1b[?2004h";
            tui::write_remote(app);
            terminal.input(b"");
            assert_eq!(terminal.output, app);
            terminal.output.clear();
            terminal.input(b"");
            assert!(terminal.output.is_empty());
        }

        #[test]
        fn remote_release_reenables_paste_only_after_the_entire_reset_drains() {
            let mut terminal = setup(true);
            terminal.input(b"");
            tui::begin_remote();
            terminal.input(b"");
            terminal.output.clear();
            let last_frame = b"last app frame";
            tui::write_remote(last_frame);
            tui::release_remote();
            let pending = [last_frame.as_slice(), tui::RESET].concat();
            // Leave just the final reset byte queued after this refresh.
            tui::STATE.lock().drain_limit = Some(pending.len() - 1);
            terminal.input(b"");
            assert_eq!(terminal.output, pending[..pending.len() - 1]);
            assert_eq!(mode_count(&terminal, false), 1);
            assert_eq!(mode_count(&terminal, true), 0);
            // Another refresh with no transport space must not restore it.
            tui::STATE.lock().drain_limit = Some(0);
            terminal.input(b"");
            assert_eq!(terminal.output, pending[..pending.len() - 1]);
            tui::STATE.lock().drain_limit = None;
            terminal.input(b"");
            assert!(terminal.output.starts_with(&pending));
            assert_eq!(mode_count(&terminal, true), 1);
            let enable = terminal.output.windows(b"\x1b[?2004h".len())
                .position(|bytes| bytes == b"\x1b[?2004h").unwrap();
            assert!(enable >= pending.len());
            terminal.input(b"");
            assert_eq!(mode_count(&terminal, true), 1);
        }

        #[test]
        fn terminal_exit_and_empty_ctrl_d_disable_bracketed_paste() {
            for command in ["exit", ""] {
                let mut terminal = setup(true);
                terminal.line = command.into();
                terminal.shell.prompt = command.into();
                terminal.input(b"");
                terminal.input(if command.is_empty() { b"\x04" } else { b"\r" });
                assert!(terminal.closing);
                assert_eq!(mode_count(&terminal, false), 1);
                let enabled = mode_count(&terminal, true);
                terminal.input(b"");
                assert_eq!(mode_count(&terminal, true), enabled);
            }
        }
    }
}
