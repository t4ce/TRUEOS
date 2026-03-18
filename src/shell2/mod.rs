use alloc::collections::VecDeque;
use alloc::string::String as AllocString;
use core::fmt::Write as _;
use core::sync::atomic::{AtomicU8, Ordering};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::String as HString;
pub(crate) mod backends;
mod cmds;
mod ecma48;
mod interface;
mod matrix;
mod shell2_ai;
mod shell2_cmd;
mod shell2_qjs;
mod shell2_surf;
#[allow(unused_imports)]
pub(crate) use crate::shell2::backends::{
    crlf, uart1_com1, NET_TCP_SHELL_BACKEND, UART1_COM1_BACKEND,
};
pub(crate) use interface::{ShellBackend2, ShellIo2};
use shell2_ai::AiPromptMode;
use shell2_qjs::QjsPromptMode;

const DEFAULT_PROMPT: &str = "§ ";
const MAX_LINE: usize = 192;
const BANNER_ROW: usize = 1;
const STATUS_ROW: usize = 2;
const PROMPT_ROW: usize = 3;
const SCROLL_TOP_ROW: usize = 4;
const STATUS_SELECTED_RGB: (u8, u8, u8) = (255, 55, 255);
const FUNCTION_KEY_RGB: (u8, u8, u8) = (255, 255, 255);
const SYSTEM_TEXT_RGB: (u8, u8, u8) = (60, 183, 161);
const LINE_WIDTH: usize = 100;
pub(crate) const OUTPUT_UART1_MASK: u8 = 1 << 0;
pub(crate) const OUTPUT_NET_TCP_MASK: u8 = 1 << 1;
const MAX_TRANSCRIPT_HISTORY: usize = 256;
const MAX_RENDERED_TRANSCRIPT_LINES: usize = 20;

static REGISTERED_OUTPUTS: AtomicU8 = AtomicU8::new(0);
static UART1_PENDING_TRANSCRIPT: spin::Mutex<VecDeque<QueuedTranscriptEntry>> =
    spin::Mutex::new(VecDeque::new());
static NET_TCP_PENDING_TRANSCRIPT: spin::Mutex<VecDeque<QueuedTranscriptEntry>> =
    spin::Mutex::new(VecDeque::new());

#[derive(Clone, Copy)]
pub(crate) enum LineSource {
    User,
    System,
}

#[derive(Clone)]
pub(crate) struct TranscriptEntry {
    pub(crate) source: LineSource,
    pub(crate) text: AllocString,
}

#[derive(Clone)]
struct QueuedTranscriptEntry {
    slot_id: matrix::MatrixSlotId,
    entry: TranscriptEntry,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShellMode2 {
    Surf,
    Ai,
    Qjs,
    Cmd,
}

impl ShellMode2 {
    const fn next(self) -> Self {
        match self {
            Self::Surf => Self::Ai,
            Self::Ai => Self::Qjs,
            Self::Qjs => Self::Cmd,
            Self::Cmd => Self::Surf,
        }
    }
}

#[derive(Clone, Copy)]
enum EscState {
    None,
    Esc,
    Csi,
    Ss3,
}

struct AlignedWriter<'a> {
    io: &'a dyn ShellIo2,
}

impl<'a> AlignedWriter<'a> {
    fn new(io: &'a dyn ShellIo2) -> Self {
        Self { io }
    }

    fn clear_screen_home(&self) {
        self.io.write_str("\x1b[2J\x1b[H");
    }

    fn set_scroll_region(&self, top: usize) {
        // Reserve header rows by scrolling only in [top..bottom].
        self.io.write_fmt(format_args!("\x1b[{};999r", top.max(1)));
    }

    fn reset_scroll_region(&self) {
        self.io.write_str("\x1b[r");
    }

    fn move_to(&self, row: usize, col: usize) {
        self.io
            .write_fmt(format_args!("\x1b[{};{}H", row.max(1), col.max(1)));
    }

    fn clear_line(&self) {
        self.io.write_str("\x1b[2K");
    }

    fn transcript_line_at(&self, row: usize, source: LineSource, s: &str) {
        self.move_to(row, 1);
        self.clear_line();
        self.io.write_str(ecma48::RESET);

        match source {
            LineSource::User => {
                self.io.write_str(s);
            }
            LineSource::System => {
                let width = ecma48::visible_width(s);
                let col = LINE_WIDTH.saturating_sub(width).saturating_add(1);
                self.move_to(row, col);
                self.io.write_fmt(format_args!(
                    "{}",
                    ecma48::style(s).fg(SYSTEM_TEXT_RGB)
                ));
            }
        }
    }

    fn render_transcript(&self, transcript: &VecDeque<TranscriptEntry>) {
        self.io.write_str(ecma48::SAVE_CURSOR);
        self.move_to(SCROLL_TOP_ROW, 1);
        self.io.write_str("\x1b[J");

        for (idx, entry) in transcript
            .iter()
            .rev()
            .take(MAX_RENDERED_TRANSCRIPT_LINES)
            .enumerate()
        {
            let row = SCROLL_TOP_ROW + idx;
            self.transcript_line_at(row, entry.source, entry.text.as_str());
        }
        self.io.write_str(ecma48::RESTORE_CURSOR);
    }

    fn banner(&self, mode: ShellMode2, time_text: &str) {
        self.move_to(BANNER_ROW, 1);
        self.clear_line();
        self.io.write_str("TRUE OS §");
        self.center_text(BANNER_ROW, self.main_mode_text(mode).as_str());
        self.right_text(BANNER_ROW, time_text);
    }

    fn mode_status(&self, mode: ShellMode2, ai_mode: AiPromptMode, qjs_mode: QjsPromptMode) {
        self.move_to(STATUS_ROW, 1);
        self.clear_line();
        if mode == ShellMode2::Ai {
            self.ai_status(ai_mode);
        } else if mode == ShellMode2::Qjs {
            self.qjs_status(qjs_mode);
        }
        self.io.write_str(ecma48::RESET);
    }

    fn main_mode_text(&self, mode: ShellMode2) -> AllocString {
        let mut text = AllocString::new();
        self.push_function_key_label(&mut text, "[TAB]");
        self.push_plain(&mut text, " ");
        self.push_mode_token(&mut text, "surf", mode == ShellMode2::Surf);
        self.push_plain(&mut text, " - ");
        self.push_mode_token(&mut text, "ai", mode == ShellMode2::Ai);
        self.push_plain(&mut text, " - ");
        self.push_mode_token(&mut text, "qjs", mode == ShellMode2::Qjs);
        self.push_plain(&mut text, " - ");
        self.push_mode_token(&mut text, "cmd", mode == ShellMode2::Cmd);
        text
    }

    fn ai_status(&self, ai_mode: AiPromptMode) {
        let mut text = AllocString::new();
        self.push_function_key_label(&mut text, "[F1]");
        self.push_plain(&mut text, " ");
        self.push_ai_token(&mut text, "normal", ai_mode == AiPromptMode::Normal);
        self.push_plain(&mut text, " - ");
        self.push_ai_token(&mut text, "web", ai_mode == AiPromptMode::WebSearch);
        self.push_plain(&mut text, " - ");
        self.push_ai_token(&mut text, "file", ai_mode == AiPromptMode::FileSearch);
        self.push_plain(&mut text, " - ");
        self.push_ai_token(&mut text, "newchat", ai_mode == AiPromptMode::NewChat);
        self.push_plain(&mut text, " - ");
        self.push_ai_token(&mut text, "ai-pc", ai_mode == AiPromptMode::AiPc);
        self.right_text(STATUS_ROW, text.as_str());
    }

    fn qjs_status(&self, qjs_mode: QjsPromptMode) {
        let mut text = AllocString::new();
        self.push_function_key_label(&mut text, "[F1]");
        self.push_plain(&mut text, " ");
        self.push_ai_token(&mut text, "repl", qjs_mode == QjsPromptMode::Repl);
        self.push_plain(&mut text, " - ");
        self.push_ai_token(&mut text, "eval", qjs_mode == QjsPromptMode::Eval);
        self.right_text(STATUS_ROW, text.as_str());
    }

    fn push_plain(&self, out: &mut AllocString, text: &str) {
        out.push_str(text);
    }

    fn push_function_key_label(&self, out: &mut AllocString, text: &str) {
        let styled = alloc::format!("{}", ecma48::style(text).bold().fg(FUNCTION_KEY_RGB));
        out.push_str(styled.as_str());
    }

    fn push_ai_token(&self, out: &mut AllocString, text: &str, selected: bool) {
        if selected {
            let styled = alloc::format!(
                "{}",
                ecma48::style(text).bold().fg(STATUS_SELECTED_RGB)
            );
            out.push_str(styled.as_str());
        } else {
            out.push_str(text);
        }
    }

    fn push_mode_token(&self, out: &mut AllocString, text: &str, selected: bool) {
        if selected {
            let styled = alloc::format!(
                "{}",
                ecma48::style(text).bold().fg(STATUS_SELECTED_RGB)
            );
            out.push_str(styled.as_str());
        } else {
            out.push_str(text);
        }
    }

    fn write_mode_token(&self, text: &str, selected: bool) {
        if selected {
            self.io.write_fmt(format_args!(
                "{}",
                ecma48::style(text).bold().fg(STATUS_SELECTED_RGB)
            ));
        } else {
            self.io.write_str(text);
        }
    }

    fn prompt(&self) {
        self.move_to(PROMPT_ROW, 1);
        self.clear_line();
        self.io.write_str("\x1b[0m");
        self.io.write_str(DEFAULT_PROMPT);
    }

    fn user_backspace(&self) {
        self.io.write_str("\x08 \x08");
    }

    fn user_char(&self, ch: char) {
        self.io.write_char(ch);
    }

    fn right_text(&self, row: usize, text: &str) {
        let width = ecma48::visible_width(text);
        let col = LINE_WIDTH.saturating_sub(width).saturating_add(1);
        self.move_to(row, col);
        self.io.write_str(text);
    }

    fn center_text(&self, row: usize, text: &str) {
        let width = ecma48::visible_width(text);
        let col = LINE_WIDTH
            .saturating_sub(width)
            .checked_div(2)
            .unwrap_or(0)
            .saturating_add(1);
        self.move_to(row, col);
        self.io.write_str(text);
    }
}

fn clock_bucket_and_text() -> (u64, HString<5>) {
    let secs = crate::time::unix_time_seconds().unwrap_or_else(crate::time::uptime_seconds);
    let mins_total = secs / 60;
    let mins_day = mins_total % (24 * 60);
    let hh = mins_day / 60;
    let mm = mins_day % 60;
    let mut text: HString<5> = HString::new();
    let _ = write!(text, "{:02}:{:02}", hh, mm);
    (mins_total, text)
}

pub(crate) fn print_shell_line(io: &dyn ShellIo2, text: &str) {
    enqueue_transcript_line(io, LineSource::System, text);
}

fn same_backend_io(io: &dyn ShellIo2, target: &'static dyn ShellIo2) -> bool {
    (io as *const dyn ShellIo2 as *const ()) == (target as *const dyn ShellIo2 as *const ())
}

fn same_backend_task(io: &'static dyn ShellBackend2, target: &'static dyn ShellIo2) -> bool {
    (io as *const dyn ShellBackend2 as *const ()) == (target as *const dyn ShellIo2 as *const ())
}

fn register_output(io: &'static dyn ShellIo2) {
    let uart_io: &'static dyn ShellIo2 = &UART1_COM1_BACKEND;
    if same_backend_io(io, uart_io) {
        REGISTERED_OUTPUTS.fetch_or(OUTPUT_UART1_MASK, Ordering::Relaxed);
        return;
    }
    let net_io: &'static dyn ShellIo2 = &NET_TCP_SHELL_BACKEND;
    if same_backend_io(io, net_io) {
        REGISTERED_OUTPUTS.fetch_or(OUTPUT_NET_TCP_MASK, Ordering::Relaxed);
    }
}

pub(crate) fn print_broadcast_line(text: &str) {
    let outputs = REGISTERED_OUTPUTS.load(Ordering::Relaxed);
    if (outputs & OUTPUT_UART1_MASK) != 0 {
        print_shell_line(&UART1_COM1_BACKEND, text);
    }
    if (outputs & OUTPUT_NET_TCP_MASK) != 0 {
        print_shell_line(&NET_TCP_SHELL_BACKEND, text);
    }
}

pub(crate) fn print_targeted_line(target_mask: u8, text: &str) {
    if (target_mask & OUTPUT_UART1_MASK) != 0 {
        print_shell_line(&UART1_COM1_BACKEND, text);
    }
    if (target_mask & OUTPUT_NET_TCP_MASK) != 0 {
        print_shell_line(&NET_TCP_SHELL_BACKEND, text);
    }
    if target_mask == 0 {
        print_broadcast_line(text);
    }
}

pub(crate) fn output_target_for_backend(io: &'static dyn ShellBackend2) -> u8 {
    let uart_io: &'static dyn ShellIo2 = &UART1_COM1_BACKEND;
    if same_backend_task(io, uart_io) {
        return OUTPUT_UART1_MASK;
    }

    let net_io: &'static dyn ShellIo2 = &NET_TCP_SHELL_BACKEND;
    if same_backend_task(io, net_io) {
        return OUTPUT_NET_TCP_MASK;
    }

    0
}

pub(crate) fn history_total_lines() -> usize {
    matrix::history_total_lines()
}

pub(crate) fn history_lines_text(start_line: usize, max_lines: usize) -> AllocString {
    matrix::history_lines_text(start_line, max_lines)
}

pub(crate) fn command_registry_json() -> AllocString {
    cmds::command_registry_json()
}

fn append_transcript_entry(transcript: &mut VecDeque<TranscriptEntry>, entry: TranscriptEntry) {
    if transcript.len() >= MAX_TRANSCRIPT_HISTORY {
        let _ = transcript.pop_front();
    }
    transcript.push_back(entry);
}

fn output_mask_for_io(io: &dyn ShellIo2) -> u8 {
    let uart_io: &'static dyn ShellIo2 = &UART1_COM1_BACKEND;
    if same_backend_io(io, uart_io) {
        return OUTPUT_UART1_MASK;
    }

    let net_io: &'static dyn ShellIo2 = &NET_TCP_SHELL_BACKEND;
    if same_backend_io(io, net_io) {
        return OUTPUT_NET_TCP_MASK;
    }

    0
}

fn queue_transcript_line(
    queue: &spin::Mutex<VecDeque<QueuedTranscriptEntry>>,
    slot_id: matrix::MatrixSlotId,
    source: LineSource,
    text: &str,
) {
    let mut pending = queue.lock();
    pending.push_back(QueuedTranscriptEntry {
        slot_id,
        entry: TranscriptEntry {
            source,
            text: AllocString::from(text),
        },
    });
}

fn enqueue_transcript_line(io: &dyn ShellIo2, source: LineSource, text: &str) {
    let output_mask = output_mask_for_io(io);
    if output_mask == 0 {
        return;
    }

    let slot_id = matrix::record_line_for_output(output_mask, source, text);
    if (output_mask & OUTPUT_UART1_MASK) != 0 {
        queue_transcript_line(&UART1_PENDING_TRANSCRIPT, slot_id.clone(), source, text);
    }
    if (output_mask & OUTPUT_NET_TCP_MASK) != 0 {
        queue_transcript_line(&NET_TCP_PENDING_TRANSCRIPT, slot_id, source, text);
    }
}

fn drain_pending_transcript(
    io: &'static dyn ShellBackend2,
    transcript: &mut VecDeque<TranscriptEntry>,
) -> bool {
    let active_slot = matrix::active_slot_id(output_target_for_backend(io));
    let uart_io: &'static dyn ShellIo2 = &UART1_COM1_BACKEND;
    if same_backend_task(io, uart_io) {
        return drain_pending_transcript_queue(&UART1_PENDING_TRANSCRIPT, &active_slot, transcript);
    }

    let net_io: &'static dyn ShellIo2 = &NET_TCP_SHELL_BACKEND;
    if same_backend_task(io, net_io) {
        return drain_pending_transcript_queue(&NET_TCP_PENDING_TRANSCRIPT, &active_slot, transcript);
    }

    false
}

fn current_transcript_for_task(io: &'static dyn ShellBackend2) -> VecDeque<TranscriptEntry> {
    matrix::active_lines(output_target_for_backend(io))
}

fn record_user_line_for_active_slot(io: &'static dyn ShellBackend2, submitted: &str) {
    let _ = matrix::record_line_for_output(output_target_for_backend(io), LineSource::User, submitted);
}

fn handle_matrix_operator(io: &'static dyn ShellBackend2, submitted: &str) {
    matrix::record_line_in_default(LineSource::User, submitted);
    let requested = submitted.strip_prefix('§').unwrap_or("");
    let _ = matrix::switch_active_slot(output_target_for_backend(io), requested);
}

fn drain_pending_transcript_queue(
    queue: &spin::Mutex<VecDeque<QueuedTranscriptEntry>>,
    active_slot: &matrix::MatrixSlotId,
    transcript: &mut VecDeque<TranscriptEntry>,
) -> bool {
    let mut pending = queue.lock();
    if pending.is_empty() {
        return false;
    }

    let mut changed = false;
    while let Some(queued) = pending.pop_front() {
        if queued.slot_id == *active_slot {
            append_transcript_entry(transcript, queued.entry);
            changed = true;
        }
    }
    changed
}

fn handle_submit(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    mode: ShellMode2,
    ai_mode: AiPromptMode,
    qjs_mode: QjsPromptMode,
    submitted: &str,
) {
    match mode {
        ShellMode2::Cmd => {
            let _ = shell2_cmd::try_parse(spawner, io, submitted);
        }
        ShellMode2::Surf => {
            if let Some(html) = shell2_surf::try_inline_html(submitted) {
                shell2_surf::load_inline_html(io, html);
            } else if let Some(file_ref) = shell2_surf::try_file_reference(submitted) {
                shell2_surf::load_file_reference(io, file_ref.as_str());
            } else if let Some(url) = shell2_surf::try_parse(submitted) {
                if shell2_surf::prepare_call_with_url(spawner, io, url.as_str()).is_err() {
                    print_shell_line(io, "surf: spawn failed");
                }
            }
        }
        ShellMode2::Qjs => {
            shell2_qjs::submit(io, qjs_mode, submitted);
        }
        ShellMode2::Ai => {
            shell2_ai::submit(spawner, io, ai_mode, submitted);
        }
    }
}

#[embassy_executor::task(pool_size = 2)]
pub async fn task(spawner: Spawner, io: &'static dyn ShellBackend2) {
    io.init();
    register_output(io);
    let out = AlignedWriter::new(io);

    out.clear_screen_home();
    out.reset_scroll_region();
    let (mut last_minute_bucket, time_text) = clock_bucket_and_text();
    let mut mode = ShellMode2::Cmd;
    out.banner(mode, time_text.as_str());
    let mut ai_mode = AiPromptMode::Normal;
    let mut qjs_mode = QjsPromptMode::Repl;
    out.mode_status(mode, ai_mode, qjs_mode);

    out.set_scroll_region(SCROLL_TOP_ROW);
    out.prompt();

    let mut line: HString<MAX_LINE> = HString::new();
    let mut transcript: VecDeque<TranscriptEntry> = current_transcript_for_task(io);
    let mut saw_cr = false;
    let mut esc = EscState::None;
    let mut csi_param: u16 = 0;

    loop {
        if drain_pending_transcript(io, &mut transcript) {
            out.render_transcript(&transcript);
        }

        let (minute_bucket, minute_text) = clock_bucket_and_text();
        if minute_bucket != last_minute_bucket {
            last_minute_bucket = minute_bucket;
            out.banner(mode, minute_text.as_str());
            out.mode_status(mode, ai_mode, qjs_mode);
            out.render_transcript(&transcript);
            out.prompt();
            for ch in line.chars() {
                out.user_char(ch);
            }
        }

        if let Some(b) = io.read_byte() {
            match esc {
                EscState::None => {
                    if b == 0x1b {
                        esc = EscState::Esc;
                        continue;
                    }
                }
                EscState::Esc => {
                    match b {
                        b'[' => {
                            esc = EscState::Csi;
                            csi_param = 0;
                        }
                        b'O' => {
                            esc = EscState::Ss3;
                        }
                        _ => {
                            esc = EscState::None;
                        }
                    }
                    continue;
                }
                EscState::Csi => {
                    match b {
                        b'0'..=b'9' => {
                            let digit = (b - b'0') as u16;
                            csi_param = csi_param.saturating_mul(10).saturating_add(digit);
                        }
                        b'~' if csi_param == 11 => {
                            if mode == ShellMode2::Ai {
                                ai_mode = ai_mode.next();
                                out.mode_status(mode, ai_mode, qjs_mode);
                                out.prompt();
                                for ch in line.chars() {
                                    out.user_char(ch);
                                }
                            } else if mode == ShellMode2::Qjs {
                                qjs_mode = qjs_mode.next();
                                out.mode_status(mode, ai_mode, qjs_mode);
                                out.prompt();
                                for ch in line.chars() {
                                    out.user_char(ch);
                                }
                            }
                            esc = EscState::None;
                        }
                        _ => {
                            esc = EscState::None;
                        }
                    }
                    continue;
                }
                EscState::Ss3 => {
                    if b == b'P' {
                        if mode == ShellMode2::Ai {
                            ai_mode = ai_mode.next();
                            out.mode_status(mode, ai_mode, qjs_mode);
                            out.prompt();
                            for ch in line.chars() {
                                out.user_char(ch);
                            }
                        } else if mode == ShellMode2::Qjs {
                            qjs_mode = qjs_mode.next();
                            out.mode_status(mode, ai_mode, qjs_mode);
                            out.prompt();
                            for ch in line.chars() {
                                out.user_char(ch);
                            }
                        }
                    } else if b == b'Q' {
                        // Keep SS3-Q mapped to the old secondary toggle sequence if a terminal sends it.
                    }
                    esc = EscState::None;
                    continue;
                }
            }

            if saw_cr && b == b'\n' {
                saw_cr = false;
                continue;
            }
            saw_cr = b == b'\r';

            match b {
                b'\t' => {
                    mode = mode.next();
                    out.banner(mode, minute_text.as_str());
                    out.mode_status(mode, ai_mode, qjs_mode);
                    out.prompt();
                    for ch in line.chars() {
                        out.user_char(ch);
                    }
                }
                b'\r' | b'\n' => {
                    let submitted_raw = line.as_str();
                    let submitted = submitted_raw.trim();
                    if !submitted.is_empty() {
                        if submitted_raw.starts_with('§') {
                            handle_matrix_operator(io, submitted);
                            mode = ShellMode2::Cmd;
                            out.banner(mode, minute_text.as_str());
                            out.mode_status(mode, ai_mode, qjs_mode);
                            transcript = current_transcript_for_task(io);
                            out.render_transcript(&transcript);
                        } else {
                            record_user_line_for_active_slot(io, submitted);
                            transcript = current_transcript_for_task(io);
                            out.render_transcript(&transcript);
                            handle_submit(&spawner, io, mode, ai_mode, qjs_mode, submitted);
                            if drain_pending_transcript(io, &mut transcript) {
                                out.render_transcript(&transcript);
                            }
                        }
                    }
                    line.clear();
                    out.prompt();
                }
                0x08 | 0x7F => {
                    if line.pop().is_some() {
                        out.user_backspace();
                    }
                }
                0x20..=0x7E => {
                    let ch = b as char;
                    if line.push(ch).is_ok() {
                        out.user_char(ch);
                    }
                }
                _ => {}
            }
        } else {
            Timer::after(EmbassyDuration::from_millis(5)).await;
        }
    }
}
