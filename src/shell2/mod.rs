use alloc::collections::VecDeque;
use alloc::string::String as AllocString;
use alloc::vec::Vec;
use core::cell::Cell;
use core::fmt::Write as _;
use core::sync::atomic::{AtomicU16, Ordering};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::String as HString;
use unicode_segmentation::UnicodeSegmentation;
pub(crate) mod backends;
pub(crate) mod cmds;
mod ecma48;
mod interface;
mod matrix;
pub(crate) mod qjs_workbench;
mod shell2_apps;
mod shell2_cmd;
mod shell2_cmd_registry;
mod shell2_dl;
mod shell2_surf;
mod term_style;
mod utf8;
#[allow(unused_imports)]
pub(crate) use crate::shell2::backends::{
    CONTAINER_SHELL_BACKEND, NET_TCP_SHELL_BACKEND, container_shell_drain_output,
    container_shell_read_output_byte, container_shell_submit_input, crlf,
};
pub(crate) use interface::{ShellBackend2, ShellIo2, TerminalHandoffOwner};

const MAX_LINE: usize = 1024;
const BANNER_ROW: usize = 1;
const STATUS_ROW: usize = 2;
const PROMPT_ROW: usize = 3;
const SCROLL_TOP_ROW: usize = 4;
const DEFAULT_TRANSCRIPT_VIEW_ROWS: usize = 48;
const STATUS_SELECTED_RGB: (u8, u8, u8) = (255, 55, 255);
const FUNCTION_KEY_RGB: (u8, u8, u8) = (255, 255, 255);
const TITLE_COUNT_RGB: (u8, u8, u8) = (255, 255, 255);
const SYSTEM_TEXT_RGB: (u8, u8, u8) = (60, 183, 161);
const VMX_STATUS_RGB: (u8, u8, u8) = (120, 210, 255);
const VMX_TUI_RGB: (u8, u8, u8) = (255, 90, 90);
pub(crate) type OutputMask = u16;
pub(crate) const OUTPUT_NET_TCP_MASK: OutputMask = 1 << 0;
pub(crate) const LOCAL_SHELL_SESSION_CAP: usize = 9;
pub(crate) const LOCAL_SHELL_SESSION_FIRST_BIT: usize = 1;
pub(crate) const OUTPUT_LOCAL_MASK: OutputMask =
    ((1 << LOCAL_SHELL_SESSION_CAP) - 1) << LOCAL_SHELL_SESSION_FIRST_BIT;
pub(crate) const OUTPUT_CONTAINER_MASK: OutputMask = 1 << 10;
pub(crate) const OUTPUT_SYSTEM_MASK: OutputMask = 1 << 11;
pub(crate) const OUTPUT_SCOPE_COUNT: usize = 12;

pub(crate) const TRANSPORT_NET_TCP_SCOPE: u8 = 1 << 0;
pub(crate) const TRANSPORT_LOCAL_SCOPE: u8 = 1 << 1;
pub(crate) const TRANSPORT_CONTAINER_SCOPE: u8 = 1 << 2;

pub(crate) const fn local_shell_session_output_mask(index: usize) -> OutputMask {
    if index < LOCAL_SHELL_SESSION_CAP {
        1 << (LOCAL_SHELL_SESSION_FIRST_BIT + index)
    } else {
        0
    }
}
const SECTION_STATUS_TEXT: &str = "t4ce is with you";
const SECTION_STATUS_HOLD_MS: u64 = 1000;
const SECTION_RAINBOW_FRAME_MS: u64 = 120;
const SECTION_RAINBOW_COLORS: [u8; 8] = [199, 208, 227, 121, 51, 39, 99, 201];
const STATUS_NORMAL_RGB: (u8, u8, u8) = (255, 255, 255);
const BANNER_TITLE_TEXT: &str = "TRUE OS";
const BANNER_CLOCK_WIDTH: usize = 5;
const BANNER_GROUP_GAP_WIDTH: usize = 1;
const TERMINAL_SIZE_QUERY: &str = "\x1b[18t";
const TERMINAL_SIZE_QUERY_IDLE_TICKS: u16 = 100;
const CRY_APP_LABEL: &str = "cry";
pub(crate) const LOCAL_ESCAPE_KEY_BYTE: u8 = 0x1d;
pub(crate) const LOCAL_UNMAPPED_KEY_BYTE: u8 = 0x1e;

static REGISTERED_OUTPUTS: AtomicU16 = AtomicU16::new(0);

#[derive(Clone)]
pub(crate) struct TranscriptEntry {
    pub(crate) text: AllocString,
    pub(crate) transient: bool,
}

#[derive(Clone)]
struct CommandSession {
    slot_id: matrix::MatrixSlotId,
    slot_lifetime_generation: u64,
    kind: shell2_cmd::CommandSessionKind,
}

#[derive(Clone, Copy)]
pub(crate) enum CommandSessionInputResult {
    CompleteIdle,
    CompleteRunning,
    KeepRunning,
}

#[derive(Clone)]
pub(crate) struct MatrixTarget {
    output_mask: OutputMask,
    local_session_generation: Option<u64>,
    slot_id: matrix::MatrixSlotId,
    slot_lifetime_generation: u64,
    interrupt_generation: u64,
}

fn with_output_scope_lease<R>(
    output_mask: OutputMask,
    operation: impl FnOnce(Option<u64>) -> R,
) -> Option<R> {
    if (output_mask & OUTPUT_LOCAL_MASK) == 0 {
        return Some(operation(None));
    }
    backends::session_pool::with_active_generation_for_output_mask(output_mask, |generation| {
        operation(Some(generation))
    })
}

fn with_matrix_target_lease<R>(target: &MatrixTarget, operation: impl FnOnce() -> R) -> Option<R> {
    if (target.output_mask & OUTPUT_LOCAL_MASK) == 0 {
        return Some(operation());
    }
    let generation = target.local_session_generation?;
    backends::session_pool::with_generation_for_output_mask(target.output_mask, generation, |_| {
        operation()
    })
}

fn with_matrix_target_geometry<R>(
    target: &MatrixTarget,
    operation: impl FnOnce((usize, usize)) -> R,
) -> Option<R> {
    let generation = target.local_session_generation?;
    backends::session_pool::with_generation_for_output_mask(
        target.output_mask,
        generation,
        operation,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShellMode2 {
    Apps,
    Cmd,
}

impl ShellMode2 {
    const fn next(self) -> Self {
        match self {
            Self::Apps => Self::Cmd,
            Self::Cmd => Self::Apps,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EscState {
    None,
    Esc,
    Csi,
    Ss3,
}

#[derive(Clone, Copy)]
struct CsiInput {
    params: [u16; 4],
    index: usize,
    has_digit: bool,
}

impl CsiInput {
    const fn new() -> Self {
        Self {
            params: [0; 4],
            index: 0,
            has_digit: false,
        }
    }

    fn reset(&mut self) {
        *self = Self::new();
    }

    fn push_digit(&mut self, digit: u8) {
        let idx = self.index.min(self.params.len().saturating_sub(1));
        self.params[idx] = self.params[idx]
            .saturating_mul(10)
            .saturating_add(u16::from(digit));
        self.has_digit = true;
    }

    fn push_separator(&mut self) {
        if self.index + 1 < self.params.len() {
            self.index += 1;
        }
        self.has_digit = false;
    }

    fn terminal_size(&self) -> Option<(usize, usize)> {
        if self.params[0] == 8 && self.params[1] > 0 && self.params[2] > 0 {
            Some((usize::from(self.params[2]), usize::from(self.params[1])))
        } else {
            None
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct ChromeState {
    line_width: usize,
    active_slot: matrix::MatrixSlotId,
    active_slot_activity: matrix::MatrixSlotActivity,
    app_label: Option<AllocString>,
    is_vmx: bool,
    mode: ShellMode2,
}

struct AlignedWriter<'a> {
    io: &'a dyn ShellIo2,
    line_width: Cell<usize>,
    transcript_view_rows: Cell<usize>,
}

impl<'a> AlignedWriter<'a> {
    fn new(io: &'a dyn ShellIo2) -> Self {
        Self {
            io,
            line_width: Cell::new(matrix::DEFAULT_MATRIX_SLOT_LINE_WIDTH),
            transcript_view_rows: Cell::new(DEFAULT_TRANSCRIPT_VIEW_ROWS),
        }
    }

    fn line_width(&self) -> usize {
        self.line_width.get()
    }

    fn set_line_width(&self, width: usize) {
        self.line_width.set(width);
    }

    fn set_transcript_view_rows(&self, rows: usize) {
        self.transcript_view_rows.set(rows.max(1));
    }

    fn clear_screen_home(&self) {
        self.io.raw_write_str("\x1b[2J\x1b[H");
    }

    fn set_scroll_region(&self, top: usize) {
        // Reserve header rows by scrolling only in [top..bottom].
        self.io.raw_write_fmt(format_args!("\x1b[{};r", top.max(1)));
    }

    fn reset_scroll_region(&self) {
        self.io.raw_write_str("\x1b[r");
    }

    fn move_to(&self, row: usize, col: usize) {
        self.io
            .raw_write_fmt(format_args!("\x1b[{};{}H", row.max(1), col.max(1)));
    }

    fn clear_line(&self) {
        self.io.raw_write_str("\x1b[2K");
    }

    fn transcript_line_at(&self, row: usize, s: &str) {
        self.move_to(row, 1);
        self.clear_line();
        // Transcript output has one neutral presentation. Reset any style
        // inherited from the fixed chrome, then preserve the application's
        // bytes without attaching source-specific alignment or color.
        self.io.raw_write_str(ecma48::RESET);
        self.io.raw_write_str(s);
    }

    fn render_transcript(&self, transcript: &VecDeque<TranscriptEntry>) {
        self.render_transcript_at(SCROLL_TOP_ROW, transcript);
    }

    fn render_transcript_at(&self, top_row: usize, transcript: &VecDeque<TranscriptEntry>) {
        self.io.raw_write_str(ecma48::SAVE_CURSOR);
        self.move_to(top_row, 1);
        self.io.raw_write_str("\x1b[J");
        let view_rows = self.transcript_view_rows.get().max(1);

        for (idx, entry) in transcript.iter().rev().take(view_rows).enumerate() {
            let row = top_row + idx;
            self.transcript_line_at(row, entry.text.as_str());
        }
        self.io.raw_write_str(ecma48::RESTORE_CURSOR);
    }

    fn push_transcript_line(&self, top_row: usize, entry: &TranscriptEntry) {
        self.io.raw_write_str(ecma48::SAVE_CURSOR);
        self.move_to(top_row, 1);
        self.io.raw_write_str("\x1b[L");
        self.transcript_line_at(top_row, entry.text.as_str());
        self.io.raw_write_str(ecma48::RESTORE_CURSOR);
    }

    fn banner(&self, output_mask: OutputMask, mode: ShellMode2, time_text: &str) {
        self.move_to(BANNER_ROW, 1);
        self.clear_line();
        self.banner_left(output_mask, time_text);
        self.right_text(BANNER_ROW, self.banner_right_text(output_mask, mode).as_str());
    }

    fn banner_left(&self, output_mask: OutputMask, time_text: &str) {
        self.move_to(BANNER_ROW, 1);
        self.io.raw_write_str(BANNER_TITLE_TEXT);
        self.io.raw_write_char(' ');
        self.io.raw_write_str(time_text);
        let count_text = alloc::format!(" CNT {}", crate::release_count::RELEASE_COUNT);
        let styled_count =
            alloc::format!("{}", term_style::paint(count_text.as_str()).color(TITLE_COUNT_RGB));
        self.io.raw_write_str(styled_count.as_str());
        if active_matrix_slot_is_vmx(output_mask)
            && let Some(label) = matrix::active_slot_app_label(output_mask)
        {
            let styled_label = alloc::format!(
                " {}",
                term_style::paint(label.as_str())
                    .bold()
                    .color(VMX_STATUS_RGB)
            );
            self.io.raw_write_str(styled_label.as_str());
        }
    }

    fn mode_status(&self, output_mask: OutputMask, running_go2_phase: usize) {
        self.move_to(STATUS_ROW, 1);
        self.clear_line();
        let slot_text = self.slot_status_text(output_mask, running_go2_phase);
        if !slot_text.is_empty() {
            self.left_text(STATUS_ROW, slot_text.as_str());
        }
        self.io.raw_write_str(ecma48::RESET);
    }

    fn banner_right_text(&self, output_mask: OutputMask, mode: ShellMode2) -> AllocString {
        let mut text = AllocString::new();
        if active_matrix_slot_is_vmx(output_mask) {
            let styled =
                alloc::format!("{}", term_style::paint("VMX").bold().color(VMX_STATUS_RGB));
            self.push_plain(&mut text, styled.as_str());
            for (command, color) in [
                ("tui", Some(VMX_TUI_RGB)),
                ("env", None),
                ("smp", None),
                ("leave", None),
            ] {
                self.push_plain(&mut text, " ");
                if let Some(color) = color {
                    let styled = alloc::format!("{}", term_style::paint(command).color(color));
                    self.push_plain(&mut text, styled.as_str());
                } else {
                    self.push_plain(&mut text, command);
                }
            }
            self.push_function_key_label(&mut text, "[ESC]");
            for command in ["stop", "pause", "snapshot", "preserve"] {
                self.push_plain(&mut text, " ");
                let styled = alloc::format!("{}", term_style::paint(command).color(VMX_STATUS_RGB));
                self.push_plain(&mut text, styled.as_str());
            }
        } else {
            self.push_plain(&mut text, self.mode_commands_text(mode).as_str());
        }
        text
    }

    fn mode_commands_text(&self, mode: ShellMode2) -> AllocString {
        match mode {
            ShellMode2::Apps => shell2_apps::command_names_text(),
            ShellMode2::Cmd => command_names_status_text(),
        }
    }

    fn push_plain(&self, out: &mut AllocString, text: &str) {
        out.push_str(text);
    }

    fn push_function_key_label(&self, out: &mut AllocString, text: &str) {
        let styled = alloc::format!("{}", term_style::paint(text).color(FUNCTION_KEY_RGB));
        out.push_str(styled.as_str());
    }

    fn slot_status_text(&self, output_mask: OutputMask, _running_go2_phase: usize) -> AllocString {
        let slots = matrix::slot_views(output_mask);
        let mut out = AllocString::new();
        for (idx, slot) in slots.iter().enumerate() {
            if idx != 0 {
                out.push(' ');
            }

            let mut label = AllocString::from("§");
            label.push_str(slot.id.as_str());

            if slot.selected {
                let styled = alloc::format!(
                    "{}",
                    term_style::paint(label.as_str())
                        .bold()
                        .color(STATUS_SELECTED_RGB)
                );
                out.push_str(styled.as_str());
            } else if slot.activity == matrix::MatrixSlotActivity::Running {
                let styled = alloc::format!(
                    "{}",
                    term_style::paint(label.as_str())
                        .bold()
                        .color(SYSTEM_TEXT_RGB)
                );
                out.push_str(styled.as_str());
            } else {
                let styled = alloc::format!(
                    "{}",
                    term_style::paint(label.as_str())
                        .bold()
                        .color(STATUS_NORMAL_RGB)
                );
                out.push_str(styled.as_str());
            }
        }
        out
    }

    fn prompt(&self, output_mask: OutputMask) {
        self.move_to(PROMPT_ROW, 1);
        self.clear_line();
        self.io.raw_write_str("\x1b[0m");
        self.io.raw_write_str(ecma48::SHOW_CURSOR);
        self.io.raw_write_str(ecma48::CURSOR_COLOR_GRAY);
        self.io.raw_write_str(ecma48::CURSOR_BLINKING_BLOCK);
    }

    fn user_char(&self, ch: char) {
        self.io.raw_write_char(ch);
    }

    fn left_text(&self, row: usize, text: &str) {
        self.move_to(row, 1);
        self.io.raw_write_str(text);
    }

    fn right_text(&self, row: usize, text: &str) {
        let width = ecma48::visible_width(text);
        if width > self.line_width() {
            return;
        }
        let col = self.line_width().saturating_sub(width).saturating_add(1);
        self.move_to(row, col);
        self.io.raw_write_str(text);
    }

    fn center_text(&self, row: usize, text: &str) {
        let width = ecma48::visible_width(text);
        let col = self
            .line_width()
            .saturating_sub(width)
            .checked_div(2)
            .unwrap_or(0)
            .saturating_add(1);
        self.move_to(row, col);
        self.io.raw_write_str(text);
    }
}

fn clock_bucket_and_text() -> (u64, HString<5>) {
    let utc_secs =
        crate::chronos::best_effort_unix_time_seconds().unwrap_or_else(crate::time::uptime_seconds);
    let local_secs = crate::locale::local_unix_time_seconds(utc_secs);
    let mins_day = (local_secs / 60) % (24 * 60);
    let hh = mins_day / 60;
    let mm = mins_day % 60;
    let mut text: HString<5> = HString::new();
    let _ = write!(text, "{:02}:{:02}", hh, mm);
    (utc_secs / 60, text)
}

pub(crate) fn print_shell_line(io: &dyn ShellIo2, text: &str) {
    enqueue_transcript_line(io, text);
}

pub(crate) fn print_native_line(io: &dyn ShellIo2, text: &str) {
    enqueue_transcript_line(io, text);
}

fn same_backend_io(io: &dyn ShellIo2, target: &'static dyn ShellIo2) -> bool {
    (io as *const dyn ShellIo2 as *const ()) == (target as *const dyn ShellIo2 as *const ())
}

fn same_backend_task(io: &'static dyn ShellBackend2, target: &'static dyn ShellIo2) -> bool {
    (io as *const dyn ShellBackend2 as *const ()) == (target as *const dyn ShellIo2 as *const ())
}

fn register_output(io: &'static dyn ShellIo2) {
    let declared = io.output_mask();
    if declared != 0 {
        REGISTERED_OUTPUTS.fetch_or(declared, Ordering::Relaxed);
        return;
    }
    let net_io: &'static dyn ShellIo2 = &NET_TCP_SHELL_BACKEND;
    if same_backend_io(io, net_io) {
        REGISTERED_OUTPUTS.fetch_or(OUTPUT_NET_TCP_MASK, Ordering::Relaxed);
        return;
    }

    let container_io: &'static dyn ShellIo2 = &CONTAINER_SHELL_BACKEND;
    if same_backend_io(io, container_io) {
        REGISTERED_OUTPUTS.fetch_or(OUTPUT_CONTAINER_MASK, Ordering::Relaxed);
    }
}

pub(crate) fn line_width_for_backend(io: &'static dyn ShellBackend2) -> usize {
    line_width_for_output(output_target_for_backend(io))
}

pub(crate) fn set_line_width_for_backend(io: &'static dyn ShellBackend2, width: usize) {
    set_line_width_for_output(output_target_for_backend(io), width);
}

pub(crate) fn apply_reported_terminal_size_for_backend(
    io: &'static dyn ShellBackend2,
    cols: usize,
    rows: usize,
) -> bool {
    apply_reported_terminal_size(output_target_for_backend(io), cols, rows)
}

/// Make the local UI frontend the geometry authority for the shared net shell.
///
/// Starting a Blueprint selects its Matrix application slot. The shell frontend
/// is itself the terminal, so attaching it returns the shared net view to the
/// default CLI slot before repainting that one canonical shell2 session.
pub(crate) fn activate_net_shell_frontend_view(cols: usize, rows: usize) {
    let _ = matrix::switch_active_slot(OUTPUT_NET_TCP_MASK, "");
    let _ = apply_reported_terminal_size(OUTPUT_NET_TCP_MASK, cols, rows);
    if !backends::net_tcp::net_shell_direct_active() {
        repaint_backend_screen(&NET_TCP_SHELL_BACKEND);
    }
}

pub(crate) fn configure_local_shell_session_view(
    index: usize,
    generation: u64,
    cols: usize,
    rows: usize,
) {
    let output_mask = local_shell_session_output_mask(index);
    if output_mask != 0 {
        let _ = backends::session_pool::with_generation_for_output_mask(
            output_mask,
            generation,
            |_| apply_reported_terminal_size(output_mask, cols, rows),
        );
    }
}

pub(crate) fn initialize_local_shell_session_view(
    index: usize,
    generation: u64,
    cols: usize,
    rows: usize,
) {
    let output_mask = local_shell_session_output_mask(index);
    if output_mask != 0 {
        let _ = backends::session_pool::with_generation_for_output_mask(
            output_mask,
            generation,
            |_| {
                let _ = matrix::switch_active_slot(output_mask, "");
                let _ = apply_reported_terminal_size(output_mask, cols, rows);
            },
        );
    }
}

pub(crate) fn minimum_line_width_for_backend(io: &'static dyn ShellBackend2) -> usize {
    minimum_line_width_for_output(output_target_for_backend(io))
}

pub(crate) fn net_shell_terminal_size() -> (usize, usize) {
    (
        line_width_for_output(OUTPUT_NET_TCP_MASK),
        matrix::active_terminal_rows(OUTPUT_NET_TCP_MASK).max(1),
    )
}

fn line_width_for_output(output_mask: OutputMask) -> usize {
    matrix::active_line_width(output_mask)
        .max(minimum_line_width_for_output(output_mask))
        .max(1)
}

fn transcript_view_rows_for_output(output_mask: OutputMask) -> usize {
    let top_row = slot_content_top_row(output_mask);
    matrix::active_terminal_rows(output_mask)
        .saturating_sub(top_row.saturating_sub(1))
        .max(1)
}

fn slot_content_top_row(_output_mask: OutputMask) -> usize {
    SCROLL_TOP_ROW
}

fn slot_content_rows_for_output(output_mask: OutputMask) -> usize {
    transcript_view_rows_for_output(output_mask)
}

fn render_active_slot_content(
    out: &AlignedWriter<'_>,
    output_mask: OutputMask,
    transcript: &VecDeque<TranscriptEntry>,
) -> bool {
    out.render_transcript_at(slot_content_top_row(output_mask), transcript);
    false
}

fn configure_output_view(out: &AlignedWriter<'_>, output_mask: OutputMask) {
    out.set_line_width(line_width_for_output(output_mask));
    out.set_transcript_view_rows(transcript_view_rows_for_output(output_mask));
}

fn current_chrome_state(output_mask: OutputMask, mode: ShellMode2) -> ChromeState {
    ChromeState {
        line_width: line_width_for_output(output_mask),
        active_slot: matrix::active_slot_id(output_mask),
        active_slot_activity: matrix::active_slot_activity(output_mask),
        app_label: matrix::active_slot_app_label(output_mask),
        is_vmx: active_matrix_slot_is_vmx(output_mask),
        mode,
    }
}

fn set_line_width_for_output(output_mask: OutputMask, width: usize) {
    let mut width = width.max(minimum_line_width_for_output(output_mask));
    if (output_mask & OUTPUT_LOCAL_MASK) != 0
        && let Some((frontend_cols, _)) =
            backends::session_pool::terminal_size_for_output_mask(output_mask)
    {
        width = width.min(frontend_cols.max(1));
    }
    matrix::set_active_line_width(output_mask, width.max(1));
}

fn apply_reported_terminal_size(output_mask: OutputMask, cols: usize, rows: usize) -> bool {
    if cols == 0 {
        return false;
    }
    let mut changed = false;
    let cols = cols.max(minimum_line_width_for_output(output_mask));
    if matrix::active_line_width(output_mask) != cols {
        matrix::set_active_line_width(output_mask, cols);
        changed = true;
    }
    if rows > 0 {
        let rows = rows.max(1);
        if matrix::active_terminal_rows(output_mask) != rows {
            matrix::set_active_terminal_rows(output_mask, rows);
            changed = true;
        }
    }
    changed
}

fn minimum_line_width_for_output(output_mask: OutputMask) -> usize {
    // A local UI frontend has a fixed cell budget. Its wide command legend is
    // optional (`right_text` elides it when it does not fit), while the shared
    // Matrix slot overview remains visible on row two.
    if (output_mask & OUTPUT_LOCAL_MASK) != 0 {
        return 50;
    }
    let left = banner_left_visible_width(output_mask);
    left.saturating_add(BANNER_GROUP_GAP_WIDTH)
        .saturating_add(banner_right_visible_width(output_mask))
}

fn banner_left_visible_width(output_mask: OutputMask) -> usize {
    let mut width = ecma48::visible_width(BANNER_TITLE_TEXT)
        .saturating_add(1)
        .saturating_add(BANNER_CLOCK_WIDTH)
        .saturating_add(ecma48::visible_width(" CNT "))
        .saturating_add(ecma48::visible_width(
            alloc::format!("{}", crate::release_count::RELEASE_COUNT).as_str(),
        ));
    if active_matrix_slot_is_vmx(output_mask)
        && let Some(label) = matrix::active_slot_app_label(output_mask)
    {
        width = width
            .saturating_add(1)
            .saturating_add(ecma48::visible_width(label.as_str()));
    }
    width
}

fn banner_right_visible_width(output_mask: OutputMask) -> usize {
    if active_matrix_slot_is_vmx(output_mask) {
        return ecma48::visible_width("VMX tui env smp leave[ESC] stop pause snapshot preserve");
    }

    let cmd_width = ecma48::visible_width(command_names_status_text().as_str());
    let apps_width = ecma48::visible_width(shell2_apps::command_names_text().as_str());
    cmd_width.max(apps_width)
}

pub(crate) fn output_target_for_backend(io: &'static dyn ShellBackend2) -> OutputMask {
    let declared = io.output_mask();
    if declared != 0 {
        return declared;
    }
    let net_io: &'static dyn ShellIo2 = &NET_TCP_SHELL_BACKEND;
    if same_backend_task(io, net_io) {
        return OUTPUT_NET_TCP_MASK;
    }

    let container_io: &'static dyn ShellIo2 = &CONTAINER_SHELL_BACKEND;
    if same_backend_task(io, container_io) {
        return OUTPUT_CONTAINER_MASK;
    }

    0
}

pub(crate) fn transport_scope_for_backend(io: &'static dyn ShellBackend2) -> u8 {
    let declared = io.transport_scope();
    if declared != 0 {
        return declared;
    }
    let net_io: &'static dyn ShellIo2 = &NET_TCP_SHELL_BACKEND;
    if same_backend_task(io, net_io) {
        return TRANSPORT_NET_TCP_SCOPE;
    }
    let container_io: &'static dyn ShellIo2 = &CONTAINER_SHELL_BACKEND;
    if same_backend_task(io, container_io) {
        return TRANSPORT_CONTAINER_SCOPE;
    }
    TRANSPORT_LOCAL_SCOPE
}

fn matrix_target_from_slot_id(
    output_mask: OutputMask,
    local_session_generation: Option<u64>,
    slot_id: matrix::MatrixSlotId,
) -> MatrixTarget {
    let slot_lifetime_generation = matrix::slot_lifetime_generation(&slot_id);
    let interrupt_generation = matrix::slot_interrupt_generation(&slot_id);
    MatrixTarget {
        output_mask,
        local_session_generation,
        slot_id,
        slot_lifetime_generation,
        interrupt_generation,
    }
}

fn matrix_target_for_slot_name_unpinned(
    output_mask: OutputMask,
    local_session_generation: Option<u64>,
    requested: &str,
) -> MatrixTarget {
    matrix_target_from_slot_id(
        output_mask,
        local_session_generation,
        matrix::slot_id_from_name(requested),
    )
}

pub(crate) fn matrix_target_for_backend(io: &'static dyn ShellBackend2) -> MatrixTarget {
    let output_mask = output_target_for_backend(io);
    with_output_scope_lease(output_mask, |local_session_generation| {
        matrix_target_from_slot_id(
            output_mask,
            local_session_generation,
            matrix::active_slot_id(output_mask),
        )
    })
    .unwrap_or_else(|| {
        matrix_target_from_slot_id(output_mask, None, matrix::active_slot_id(output_mask))
    })
}

pub(crate) fn matrix_target_routes_to(target: &MatrixTarget, output_mask: OutputMask) -> bool {
    with_matrix_target_lease(target, || (target.output_mask & output_mask) != 0).unwrap_or(false)
}

pub(crate) fn matrix_target_for_slot_name(
    output_mask: OutputMask,
    requested: &str,
) -> MatrixTarget {
    with_output_scope_lease(output_mask, |local_session_generation| {
        matrix_target_for_slot_name_unpinned(output_mask, local_session_generation, requested)
    })
    .unwrap_or_else(|| matrix_target_for_slot_name_unpinned(output_mask, None, requested))
}

pub(crate) fn submit_online_to_target(
    spawner: &Spawner,
    target: MatrixTarget,
    args: Vec<AllocString>,
) -> Result<(), embassy_executor::SpawnError> {
    let width = with_matrix_target_lease(&target, || line_width_for_output(target.output_mask))
        .unwrap_or(matrix::DEFAULT_MATRIX_SLOT_LINE_WIDTH);
    shell2_dl::submit_online_to_target(spawner, target, width, args)
}

pub(crate) fn matrix_target_for_slot_name_selected(
    output_mask: OutputMask,
    requested: &str,
) -> MatrixTarget {
    with_output_scope_lease(output_mask, |local_session_generation| {
        matrix_target_from_slot_id(
            output_mask,
            local_session_generation,
            matrix::switch_active_slot(output_mask, requested),
        )
    })
    .unwrap_or_else(|| matrix_target_for_slot_name_unpinned(output_mask, None, requested))
}

pub(crate) fn reserve_matrix_target_for_vm_slot_selected(
    source: &MatrixTarget,
    requested: &str,
) -> MatrixTarget {
    with_matrix_target_lease(source, || {
        matrix_target_from_slot_id(
            source.output_mask,
            source.local_session_generation,
            matrix::reserve_available_vm_slot_selected(source.output_mask, requested),
        )
    })
    .unwrap_or_else(|| source.clone())
}

pub(crate) fn claim_matrix_target_for_app_slot_selected(
    source: &MatrixTarget,
    requested: &str,
    app_label: &str,
) -> MatrixTarget {
    with_matrix_target_lease(source, || {
        matrix_target_from_slot_id(
            source.output_mask,
            source.local_session_generation,
            matrix::claim_available_app_slot_selected(source.output_mask, requested, app_label),
        )
    })
    .unwrap_or_else(|| source.clone())
}

pub(crate) fn switch_matrix_target_slot(target: &MatrixTarget, requested: &str) -> MatrixTarget {
    with_matrix_target_lease(target, || {
        matrix_target_from_slot_id(
            target.output_mask,
            target.local_session_generation,
            matrix::switch_active_slot(target.output_mask, requested),
        )
    })
    .unwrap_or_else(|| target.clone())
}

pub(crate) fn spawn_app_vm_run_queue(spawner: Spawner) -> Result<(), embassy_executor::SpawnError> {
    match cmds::run::app_vm_run_queue_task(spawner) {
        Ok(token) => {
            spawner.spawn(token);
            Ok(())
        }
        Err(err) => Err(err),
    }
}

fn matrix_target_for_slot(
    output_mask: OutputMask,
    slot_id: &matrix::MatrixSlotId,
    slot_lifetime_generation: u64,
) -> MatrixTarget {
    let build = |local_session_generation| MatrixTarget {
        output_mask,
        local_session_generation,
        slot_id: slot_id.clone(),
        slot_lifetime_generation,
        interrupt_generation: matrix::live_slot_interrupt_generation(
            slot_id,
            slot_lifetime_generation,
        )
        .unwrap_or(0),
    };
    with_output_scope_lease(output_mask, build).unwrap_or_else(|| build(None))
}

// The operations below belong to a shared Matrix page rather than a frontend
// view. They intentionally remain valid after a local frontend detaches, while
// the slot lifetime prevents delayed work from touching a deleted/recreated
// page with the same compact id.
pub(crate) fn set_matrix_target_active(target: &MatrixTarget, active: bool) {
    if active {
        let _ = matrix::begin_live_slot_running(&target.slot_id, target.slot_lifetime_generation);
    } else {
        let _ = matrix::end_live_slot_running(&target.slot_id, target.slot_lifetime_generation);
    }
}

pub(crate) fn matrix_target_interrupted(target: &MatrixTarget) -> bool {
    matrix::live_slot_interrupt_generation(&target.slot_id, target.slot_lifetime_generation)
        != Some(target.interrupt_generation)
}

pub(crate) fn matrix_targets_same_live_slot(left: &MatrixTarget, right: &MatrixTarget) -> bool {
    matrix_targets_same_slot_lifetime(left, right)
        && matrix::live_slot_interrupt_generation(&left.slot_id, left.slot_lifetime_generation)
            .is_some()
}

pub(crate) fn matrix_targets_same_slot_lifetime(left: &MatrixTarget, right: &MatrixTarget) -> bool {
    left.slot_id == right.slot_id && left.slot_lifetime_generation == right.slot_lifetime_generation
}

pub(crate) fn bind_matrix_target_vm(target: &MatrixTarget, vm_id: u8) {
    let _ =
        matrix::bind_live_slot_vm(&target.slot_id, target.slot_lifetime_generation, vm_id, false);
}

pub(crate) fn bind_matrix_target_vm_input(target: &MatrixTarget, vm_id: u8) {
    let _ =
        matrix::bind_live_slot_vm(&target.slot_id, target.slot_lifetime_generation, vm_id, true);
}

pub(crate) fn unbind_matrix_target_vm(target: &MatrixTarget, vm_id: u8) {
    let _ = matrix::unbind_live_slot_vm(&target.slot_id, target.slot_lifetime_generation, vm_id);
}

pub(crate) fn set_matrix_target_app_label(target: &MatrixTarget, label: &str) {
    let _ =
        matrix::set_live_slot_app_label(&target.slot_id, target.slot_lifetime_generation, label);
}

pub(crate) fn release_matrix_target_vm_reservation(target: &MatrixTarget) {
    let _ = matrix::release_vm_slot_reservation(&target.slot_id, target.slot_lifetime_generation);
}

pub(crate) fn active_matrix_vm_input_id(output_mask: OutputMask) -> Option<u8> {
    matrix::active_slot_vm_input_id(output_mask)
}

pub(crate) fn active_matrix_vm_id(output_mask: OutputMask) -> Option<u8> {
    matrix::active_slot_vm_id(output_mask)
}

fn active_matrix_slot_is_vmx(output_mask: OutputMask) -> bool {
    active_matrix_vm_id(output_mask).is_some()
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

fn command_names_status_text() -> AllocString {
    shell2_cmd_registry::command_names_status_text()
}

fn output_mask_for_io(io: &dyn ShellIo2) -> OutputMask {
    let declared = io.output_mask();
    if declared != 0 {
        return declared;
    }
    let net_io: &'static dyn ShellIo2 = &NET_TCP_SHELL_BACKEND;
    if same_backend_io(io, net_io) {
        return OUTPUT_NET_TCP_MASK;
    }

    let container_io: &'static dyn ShellIo2 = &CONTAINER_SHELL_BACKEND;
    if same_backend_io(io, container_io) {
        return OUTPUT_CONTAINER_MASK;
    }

    0
}

fn enqueue_transcript_line(io: &dyn ShellIo2, text: &str) {
    let output_mask = output_mask_for_io(io);
    if output_mask == 0 {
        return;
    }

    let _ = matrix::record_line_for_output(output_mask, text);
}

pub(crate) fn print_matrix_target_line(target: &MatrixTarget, line: &str) {
    let _ =
        matrix::record_line_in_live_slot(&target.slot_id, target.slot_lifetime_generation, line);
}

pub(crate) fn print_matrix_target_progress_line(target: &MatrixTarget, line: &str) {
    let _ = matrix::record_transient_line_in_live_slot(
        &target.slot_id,
        target.slot_lifetime_generation,
        line,
    );
}

pub(crate) fn replace_matrix_target_transient_lines(target: &MatrixTarget, lines: &[AllocString]) {
    let _ = matrix::replace_transient_lines_in_live_slot(
        &target.slot_id,
        target.slot_lifetime_generation,
        lines,
    );
}

pub(crate) fn clear_matrix_target_transient_lines(target: &MatrixTarget) {
    let _ = matrix::clear_transient_lines_in_live_slot(
        &target.slot_id,
        target.slot_lifetime_generation,
    );
}

pub(crate) fn print_matrix_target_system_line(target: &MatrixTarget, line: &str) {
    let _ =
        matrix::record_line_in_live_slot(&target.slot_id, target.slot_lifetime_generation, line);
}

pub(crate) fn raw_write_matrix_target(target: &MatrixTarget, bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }

    if (target.output_mask & (OUTPUT_NET_TCP_MASK | OUTPUT_CONTAINER_MASK)) != 0 {
        let io: &'static dyn ShellIo2 = if (target.output_mask & OUTPUT_NET_TCP_MASK) != 0 {
            &NET_TCP_SHELL_BACKEND
        } else {
            &CONTAINER_SHELL_BACKEND
        };

        match core::str::from_utf8(bytes) {
            Ok(text) => io.raw_write_str(text),
            Err(_) => {
                for &b in bytes {
                    io.raw_write_byte(b);
                }
            }
        }
    } else if (target.output_mask & OUTPUT_LOCAL_MASK) != 0 {
        let Some(generation) = target.local_session_generation else {
            return bytes.len();
        };
        let _ = backends::session_pool::write_for_output_mask_generation(
            target.output_mask,
            generation,
            bytes,
        );
    }
    bytes.len()
}

pub(crate) fn raw_write_matrix_target_owned(target: &MatrixTarget, bytes: &[u8]) -> usize {
    let _ = target;
    bytes.len()
}

pub(crate) fn konsole_viewport_size_for_target(target: &MatrixTarget) -> (usize, usize) {
    let viewport = || {
        let width = line_width_for_output(target.output_mask).max(1);
        let rows = slot_content_rows_for_output(target.output_mask).max(1);
        (width, rows)
    };
    if (target.output_mask & OUTPUT_LOCAL_MASK) != 0 {
        // The backend state lock pins both the reported geometry and the
        // Matrix scope incarnation while its view geometry is sampled.
        with_matrix_target_geometry(target, |_| viewport()).unwrap_or((1, 1))
    } else {
        viewport()
    }
}

pub(crate) fn konsole_begin_frame_for_target(
    target: &MatrixTarget,
    cols: usize,
    rows: usize,
    _terminal_handoff: bool,
) -> (usize, usize) {
    let (viewport_cols, viewport_rows) = konsole_viewport_size_for_target(target);
    (cols.max(1).min(viewport_cols.max(1)), rows.max(1).min(viewport_rows.max(1)))
}

pub(crate) fn read_matrix_target_byte(target: &MatrixTarget) -> Option<u8> {
    if (target.output_mask & OUTPUT_NET_TCP_MASK) != 0 {
        NET_TCP_SHELL_BACKEND.read_byte()
    } else if (target.output_mask & OUTPUT_CONTAINER_MASK) != 0 {
        CONTAINER_SHELL_BACKEND.read_byte()
    } else if (target.output_mask & OUTPUT_LOCAL_MASK) != 0 {
        backends::session_pool::read_byte_for_output_mask_generation(
            target.output_mask,
            target.local_session_generation?,
        )
    } else {
        None
    }
}

pub(crate) fn read_matrix_target_pending_len(target: &MatrixTarget) -> usize {
    if (target.output_mask & OUTPUT_NET_TCP_MASK) != 0 {
        backends::net_tcp::net_shell_readable_len()
    } else if (target.output_mask & OUTPUT_LOCAL_MASK) != 0 {
        target
            .local_session_generation
            .and_then(|generation| {
                backends::session_pool::readable_len_for_output_mask_generation(
                    target.output_mask,
                    generation,
                )
            })
            .unwrap_or(0)
    } else {
        0
    }
}

fn current_transcript_for_task(io: &'static dyn ShellBackend2) -> VecDeque<TranscriptEntry> {
    matrix::active_lines(output_target_for_backend(io))
}

pub(crate) fn repaint_backend_screen(io: &'static dyn ShellBackend2) {
    register_output(io);
    let out = AlignedWriter::new(io);
    let output_mask = output_target_for_backend(io);
    configure_output_view(&out, output_mask);
    out.clear_screen_home();
    out.reset_scroll_region();

    let (_, time_text) = clock_bucket_and_text();
    let mode = ShellMode2::Cmd;

    out.banner(output_mask, mode, time_text.as_str());
    out.mode_status(output_mask, 0);
    out.set_scroll_region(slot_content_top_row(output_mask));

    let transcript = current_transcript_for_task(io);
    render_active_slot_content(&out, output_mask, &transcript);
    out.prompt(output_mask);
}

fn appended_transcript_line<'a>(
    prev: &VecDeque<TranscriptEntry>,
    next: &'a VecDeque<TranscriptEntry>,
) -> Option<&'a TranscriptEntry> {
    if next.len() != prev.len().saturating_add(1) {
        return None;
    }

    for (prev_entry, next_entry) in prev.iter().zip(next.iter()) {
        if prev_entry.text != next_entry.text {
            return None;
        }
    }

    next.back()
}

fn record_user_line_for_active_slot(io: &'static dyn ShellBackend2, submitted: &str) {
    let output_mask = output_target_for_backend(io);
    let recorded = user_submission_for_recording(output_mask, submitted);
    let _ = matrix::record_line_for_output(output_mask, recorded);
}

fn user_submission_for_recording(output_mask: OutputMask, submitted: &str) -> &str {
    let mut args = submitted.split_whitespace();
    let first = args.next();
    let second = args.next();
    let third = args.next();
    let fourth = args.next();

    let named_login = matches!((first, second, third, fourth),
        (Some(command), Some(login), Some(code), None)
            if command.eq_ignore_ascii_case("cry")
                && login.eq_ignore_ascii_case("login")
                && is_six_digit_code(code));
    let named_root_login = matches!((first, second, third, fourth),
        (Some(command), Some(login), Some(account), Some(code))
            if command.eq_ignore_ascii_case("cry")
                && login.eq_ignore_ascii_case("login")
                && account.eq_ignore_ascii_case("root")
                && is_six_digit_code(code));
    let bare_cry_code = first.is_some_and(is_six_digit_code)
        && second.is_none()
        && matrix::active_slot_app_label(output_mask).as_deref() == Some(CRY_APP_LABEL);

    if bare_cry_code {
        "******"
    } else if named_login {
        "cry login ******"
    } else if named_root_login {
        "cry login root ******"
    } else {
        submitted
    }
}

fn is_six_digit_code(value: &str) -> bool {
    value.len() == 6 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn handle_matrix_operator(io: &'static dyn ShellBackend2, submitted: &str) {
    // The paired prefix belongs to clear/online Apps and must never free the default slot.
    if submitted.starts_with("§§") {
        return;
    }
    matrix::record_line_in_default(submitted);
    if submitted
        .strip_prefix('§')
        .and_then(|rest| rest.strip_suffix('§'))
        .is_some()
    {
        let (freed_id, vm_ids) = matrix::free_slot(submitted);
        for vm_id in vm_ids {
            match crate::hv::stop(vm_id) {
                Ok(true) => matrix::record_line_in_default(
                    alloc::format!("matrix: freed slot §{}§; vm{} stop requested", freed_id, vm_id)
                        .as_str(),
                ),
                Ok(false) => matrix::record_line_in_default(
                    alloc::format!(
                        "matrix: freed slot §{}§; vm{} already stopped",
                        freed_id,
                        vm_id
                    )
                    .as_str(),
                ),
                Err(_) => matrix::record_line_in_default(
                    alloc::format!("matrix: freed slot §{}§; vm{} stop failed", freed_id, vm_id)
                        .as_str(),
                ),
            }
        }
    } else {
        let requested = submitted.strip_prefix('§').unwrap_or("");
        let _ = matrix::switch_active_slot(output_target_for_backend(io), requested);
    }
}

fn is_matrix_operator(submitted: &str) -> bool {
    if submitted.starts_with("§§") {
        return false;
    }
    if submitted == "§" {
        return true;
    }
    let Some(rest) = submitted.strip_prefix('§') else {
        return false;
    };
    !rest.strip_suffix('§').unwrap_or(rest).trim().is_empty()
}

enum DoubleSectionOperator<'a> {
    Clear,
    Online(&'a str),
}

fn parse_double_section_operator(submitted: &str) -> Option<DoubleSectionOperator<'_>> {
    let requested = submitted.strip_prefix("§§")?.trim();
    if requested.is_empty() {
        Some(DoubleSectionOperator::Clear)
    } else {
        Some(DoubleSectionOperator::Online(requested))
    }
}

fn is_vmx_control_command(submitted: &str) -> bool {
    matches!(
        submitted.split_whitespace().next().unwrap_or(""),
        "vmx_env"
            | "vmx_smp"
            | "vmx_help"
            | "vmx_stop"
            | "vmx_pause"
            | "vmx_snapshot"
            | "vmx_preserve"
    )
}

fn is_vmx_tui_command(submitted: &str) -> bool {
    matches!(submitted.split_whitespace().next().unwrap_or(""), "vmx_tui")
}

fn is_vmx_leave_command(submitted: &str) -> bool {
    matches!(submitted.split_whitespace().next().unwrap_or(""), "vmx_leave")
}

fn redraw_active_view(
    out: &AlignedWriter<'_>,
    io: &'static dyn ShellBackend2,
    output_mask: OutputMask,
    mode: ShellMode2,
    running_go2_phase: usize,
    minute_text: &str,
) -> VecDeque<TranscriptEntry> {
    configure_output_view(out, output_mask);
    out.clear_screen_home();
    out.reset_scroll_region();
    out.banner(output_mask, mode, minute_text);
    out.mode_status(output_mask, running_go2_phase);
    out.set_scroll_region(slot_content_top_row(output_mask));
    let transcript = current_transcript_for_task(io);
    render_active_slot_content(out, output_mask, &transcript);
    out.prompt(output_mask);
    transcript
}

fn rainbow_status_text(phase: usize) -> AllocString {
    let mut out = AllocString::new();
    for (idx, ch) in SECTION_STATUS_TEXT.chars().enumerate() {
        if ch == ' ' {
            out.push(' ');
            continue;
        }

        let glyph = alloc::format!("{}", ch);
        let color = SECTION_RAINBOW_COLORS[(idx + phase) % SECTION_RAINBOW_COLORS.len()];
        let styled = if ((idx + phase) & 1) == 0 {
            alloc::format!(
                "{}",
                term_style::paint(glyph.as_str())
                    .bold()
                    .underline()
                    .color(color)
            )
        } else {
            alloc::format!("{}", term_style::paint(glyph.as_str()).bold().color(color))
        };
        out.push_str(styled.as_str());
    }
    out
}

fn show_status_row_message(out: &AlignedWriter<'_>, text: &str) {
    out.move_to(STATUS_ROW, 1);
    out.clear_line();
    out.center_text(STATUS_ROW, text);
    out.io.raw_write_str(ecma48::RESET);
}

async fn run_plain_section_status(
    out: &AlignedWriter<'_>,
    output_mask: OutputMask,
    mode: ShellMode2,
    running_go2_phase: usize,
) {
    let white = alloc::format!(
        "{}",
        term_style::paint(SECTION_STATUS_TEXT)
            .bold()
            .color((255, 255, 255))
    );
    show_status_row_message(out, white.as_str());
    Timer::after(EmbassyDuration::from_millis(SECTION_STATUS_HOLD_MS)).await;

    for phase in 0..SECTION_RAINBOW_COLORS.len() {
        let rainbow = rainbow_status_text(phase);
        show_status_row_message(out, rainbow.as_str());
        Timer::after(EmbassyDuration::from_millis(SECTION_RAINBOW_FRAME_MS)).await;
    }

    out.mode_status(output_mask, running_go2_phase);
}

fn handle_submit(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    mode: ShellMode2,
    submitted: &str,
) -> HandleSubmitResult {
    match mode {
        ShellMode2::Cmd => match shell2_cmd::try_parse(spawner, io, submitted) {
            shell2_cmd::ParseOutcome::SetLineWidth(width) => {
                HandleSubmitResult::SetLineWidth(width)
            }
            shell2_cmd::ParseOutcome::StartSession(kind) => HandleSubmitResult::StartSession(kind),
            _ => HandleSubmitResult::None,
        },
        ShellMode2::Apps => {
            shell2_apps::submit(spawner, io, submitted);
            HandleSubmitResult::None
        }
    }
}

enum HandleSubmitResult {
    None,
    SetLineWidth(usize),
    StartSession(shell2_cmd::CommandSessionKind),
}

fn find_command_session_index(
    sessions: &[CommandSession],
    slot_id: &matrix::MatrixSlotId,
    slot_lifetime_generation: u64,
) -> Option<usize> {
    sessions.iter().position(|session| {
        session.slot_id == *slot_id && session.slot_lifetime_generation == slot_lifetime_generation
    })
}

fn find_command_session_indexes(
    sessions: &[CommandSession],
    slot_id: &matrix::MatrixSlotId,
    slot_lifetime_generation: u64,
) -> alloc::vec::Vec<usize> {
    sessions
        .iter()
        .enumerate()
        .filter_map(|(idx, session)| {
            (session.slot_id == *slot_id
                && session.slot_lifetime_generation == slot_lifetime_generation)
                .then_some(idx)
        })
        .collect()
}

fn prune_finished_command_sessions(sessions: &mut alloc::vec::Vec<CommandSession>) {
    sessions.retain(|session| {
        let keep = match session.kind {
            shell2_cmd::CommandSessionKind::RemoveSure(id) => {
                crate::shell2::cmds::rm::session_exists(id)
            }
            shell2_cmd::CommandSessionKind::FormatSure(_) => true,
        };
        if !keep && session.kind.shows_session_activity() {
            matrix::set_slot_activity(&session.slot_id, matrix::MatrixSlotActivity::Idle);
        }
        keep
    });
}

fn retire_command_sessions(sessions: &mut alloc::vec::Vec<CommandSession>) {
    for session in sessions.drain(..) {
        if session.kind.shows_session_activity() {
            matrix::set_slot_activity(&session.slot_id, matrix::MatrixSlotActivity::Idle);
        }
    }
}

fn handle_command_session_input(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    session: &CommandSession,
    submitted: &str,
    output_mask: OutputMask,
) -> CommandSessionInputResult {
    let target =
        matrix_target_for_slot(output_mask, &session.slot_id, session.slot_lifetime_generation);
    match session.kind {
        shell2_cmd::CommandSessionKind::FormatSure(disc_id) => {
            crate::shell2::cmds::format::handle_session_input(
                spawner, io, &target, submitted, disc_id,
            )
        }
        shell2_cmd::CommandSessionKind::RemoveSure(session_id) => {
            crate::shell2::cmds::rm::handle_session_input(spawner, &target, submitted, session_id)
        }
    }
}

fn apply_mode_toggle(
    out: &AlignedWriter<'_>,
    output_mask: OutputMask,
    mode: ShellMode2,
    running_go2_phase: usize,
    line: &HString<MAX_LINE>,
    minute_text: &str,
) {
    out.banner(output_mask, mode, minute_text);
    out.mode_status(output_mask, running_go2_phase);
    render_prompt_line(out, output_mask, line);
}

fn redraw_clock_preserving_cursor(
    out: &AlignedWriter<'_>,
    output_mask: OutputMask,
    time_text: &str,
) {
    out.io.raw_write_str(ecma48::SAVE_CURSOR);
    out.io.raw_write_str(ecma48::RESET);
    out.banner_left(output_mask, time_text);
    out.io.raw_write_str(ecma48::RESET);
    out.io.raw_write_str(ecma48::RESTORE_CURSOR);
}

fn apply_matrix_operator_and_refresh(
    out: &AlignedWriter<'_>,
    io: &'static dyn ShellBackend2,
    output_mask: OutputMask,
    mode: &mut ShellMode2,
    running_go2_phase: usize,
    minute_text: &str,
    submitted: &str,
) -> VecDeque<TranscriptEntry> {
    handle_matrix_operator(io, submitted);
    *mode = ShellMode2::Cmd;
    configure_output_view(out, output_mask);
    out.banner(output_mask, *mode, minute_text);
    out.mode_status(output_mask, running_go2_phase);
    let transcript = current_transcript_for_task(io);
    render_active_slot_content(out, output_mask, &transcript);
    transcript
}

fn push_input_char(out: &AlignedWriter<'_>, line: &mut HString<MAX_LINE>, ch: char) {
    if line.push(ch).is_ok() {
        out.user_char(ch);
    }
}

fn pop_input_grapheme(line: &mut HString<MAX_LINE>) -> bool {
    let Some((start, _)) = line.as_str().grapheme_indices(true).next_back() else {
        return false;
    };
    let old_len = line.len();
    line.truncate(start);
    // `truncate` changes the logical length but does not scrub the removed
    // bytes from heapless' inline storage.
    let bytes = unsafe { line.as_mut_vec() };
    for offset in start..old_len {
        unsafe { core::ptr::write_volatile(bytes.as_mut_ptr().add(offset), 0) };
    }
    core::sync::atomic::compiler_fence(Ordering::SeqCst);
    true
}

fn zeroize_input_line(line: &mut HString<MAX_LINE>) {
    // The inline storage can contain bytes beyond the current logical length
    // after editing or history replacement, so overwrite its full capacity.
    let bytes = unsafe { line.as_mut_vec() };
    for offset in 0..bytes.capacity() {
        unsafe { core::ptr::write_volatile(bytes.as_mut_ptr().add(offset), 0) };
    }
    core::sync::atomic::compiler_fence(Ordering::SeqCst);
    line.clear();
}

fn render_prompt_line(out: &AlignedWriter<'_>, output_mask: OutputMask, line: &HString<MAX_LINE>) {
    out.prompt(output_mask);
    for ch in line.chars() {
        out.user_char(ch);
    }
}

fn set_input_line(
    out: &AlignedWriter<'_>,
    output_mask: OutputMask,
    line: &mut HString<MAX_LINE>,
    text: &str,
) {
    zeroize_input_line(line);
    for ch in text.chars() {
        if line.push(ch).is_err() {
            break;
        }
    }
    render_prompt_line(out, output_mask, line);
}

fn handle_control_c(
    io: &'static dyn ShellBackend2,
    out: &AlignedWriter<'_>,
    output_mask: OutputMask,
    line: &mut HString<MAX_LINE>,
) -> VecDeque<TranscriptEntry> {
    let active_slot = matrix::active_slot_id(output_mask);
    matrix::record_line_in_slot(&active_slot, "^C");
    let (_, vm_id) = matrix::request_slot_interrupt(&active_slot);
    if let Some(vm_id) = vm_id {
        match crate::hv::stop(vm_id) {
            Ok(true) => {
                matrix::record_line_in_slot(
                    &active_slot,
                    alloc::format!("interrupt: vm{} stop requested", vm_id).as_str(),
                );
            }
            Ok(false) => {
                matrix::record_line_in_slot(
                    &active_slot,
                    alloc::format!("interrupt: vm{} is not running", vm_id).as_str(),
                );
            }
            Err(_) => {
                matrix::record_line_in_slot(
                    &active_slot,
                    alloc::format!("interrupt: vm{} stop failed", vm_id).as_str(),
                );
            }
        }
    }

    zeroize_input_line(line);
    let transcript = current_transcript_for_task(io);
    render_active_slot_content(out, output_mask, &transcript);
    out.prompt(output_mask);
    transcript
}

fn cycle_live_history(up: bool, cursor: &mut Option<usize>) -> Option<AllocString> {
    let history = matrix::live_user_input_record();
    if history.is_empty() {
        *cursor = None;
        return None;
    }

    let len = history.len();
    let next = match (*cursor, up) {
        (None, true) => len - 1,
        (None, false) => 0,
        (Some(idx), true) => idx.checked_sub(1).unwrap_or(len - 1),
        (Some(idx), false) => {
            if idx + 1 >= len {
                0
            } else {
                idx + 1
            }
        }
    };
    *cursor = Some(next);
    Some(history[next].text.clone())
}

#[embassy_executor::task(pool_size = 4)]
pub async fn task(spawner: Spawner, io: &'static dyn ShellBackend2) {
    run_shell2(spawner, io, None).await;
}

#[embassy_executor::task(pool_size = 9)]
pub async fn local_shell_session_worker(spawner: Spawner, index: usize) {
    loop {
        let generation = backends::session_pool::wait_for_lease(index).await;
        if let Some(io) = backends::session_pool::backend(index) {
            run_shell2(spawner, io, Some((index, generation))).await;
        }
        backends::session_pool::acknowledge_closed(index, generation);
    }
}

pub(crate) fn spawn_local_shell_session_workers(spawner: Spawner) -> usize {
    // Reserve the complete static task pool before starting any worker. A
    // partial carrier set would make admission permanently nondeterministic.
    let mut tokens = Vec::with_capacity(LOCAL_SHELL_SESSION_CAP);
    for index in 0..LOCAL_SHELL_SESSION_CAP {
        match local_shell_session_worker(spawner, index) {
            Ok(token) => tokens.push(token),
            Err(error) => {
                crate::log_error!(target: "shell2";
                    "shell2-session: worker reservation failed index={} err={:?}\n",
                    index,
                    error
                );
                backends::session_pool::set_pool_ready(false);
                return 0;
            }
        }
    }
    let spawned = tokens.len();
    for token in tokens {
        spawner.spawn(token);
    }
    backends::session_pool::set_pool_ready(true);
    spawned
}

async fn run_shell2(
    spawner: Spawner,
    io: &'static dyn ShellBackend2,
    local_lease: Option<(usize, u64)>,
) {
    io.init();
    register_output(io);
    let out = AlignedWriter::new(io);
    let output_mask = output_target_for_backend(io);
    configure_output_view(&out, output_mask);

    out.clear_screen_home();
    out.reset_scroll_region();
    let (mut last_minute_bucket, time_text) = clock_bucket_and_text();
    let mut mode = ShellMode2::Cmd;
    out.banner(output_mask, mode, time_text.as_str());
    let mut command_sessions: alloc::vec::Vec<CommandSession> = alloc::vec::Vec::new();
    let running_go2_phase = 0usize;
    out.mode_status(output_mask, running_go2_phase);

    out.set_scroll_region(slot_content_top_row(output_mask));
    out.prompt(output_mask);

    let mut line: HString<MAX_LINE> = HString::new();
    let mut transcript: VecDeque<TranscriptEntry> = current_transcript_for_task(io);
    let mut last_matrix_revision = matrix::visible_revision(output_mask);
    let mut saw_cr = false;
    let mut esc = EscState::None;
    let mut csi_input = CsiInput::new();
    let mut text_decode = utf8::Decoder::new();
    let mut live_history_cursor: Option<usize> = None;
    let mut input_bytes_since_yield = 0usize;
    let mut terminal_size_query_idle_ticks = TERMINAL_SIZE_QUERY_IDLE_TICKS;
    let mut last_chrome_state = current_chrome_state(output_mask, mode);
    if (output_mask & (OUTPUT_LOCAL_MASK | OUTPUT_NET_TCP_MASK)) == 0 {
        out.io.raw_write_str(TERMINAL_SIZE_QUERY);
    }

    loop {
        if let Some((index, generation)) = local_lease {
            if !backends::session_pool::generation_active(index, generation) {
                zeroize_input_line(&mut line);
                retire_command_sessions(&mut command_sessions);
                return;
            }
        }

        // Handoff owns input and output atomically. In particular, do not
        // consume a deferred repaint request until that owner releases it.
        if io.terminal_handoff_active() {
            Timer::after(EmbassyDuration::from_millis(10)).await;
            continue;
        }

        if let Some((index, generation)) = local_lease {
            if backends::session_pool::take_repaint_request(index, generation) {
                let (_, repaint_time) = clock_bucket_and_text();
                transcript = redraw_active_view(
                    &out,
                    io,
                    output_mask,
                    mode,
                    running_go2_phase,
                    repaint_time.as_str(),
                );
                render_prompt_line(&out, output_mask, &line);
                last_matrix_revision = matrix::visible_revision(output_mask);
                last_chrome_state = current_chrome_state(output_mask, mode);
            }
        }

        // Async command preparation can fail after the parser has established
        // a confirmation session. Retire those completed sessions without
        // consuming the user's next command as stale confirmation input.
        prune_finished_command_sessions(&mut command_sessions);

        let Some(matrix_revision) = matrix::try_visible_revision(output_mask) else {
            // A spin lock must not monopolize this executor: NetShell's TCP
            // bridge runs cooperatively and needs a chance to drain output.
            Timer::after(EmbassyDuration::from_millis(1)).await;
            continue;
        };
        if matrix_revision != last_matrix_revision {
            last_matrix_revision = matrix_revision;
            configure_output_view(&out, output_mask);
            let next_transcript = current_transcript_for_task(io);
            let chrome_state = current_chrome_state(output_mask, mode);
            if chrome_state != last_chrome_state {
                out.io.raw_write_str(ecma48::SAVE_CURSOR);
                out.io.raw_write_str(ecma48::RESET);
                let (_, header_time_text) = clock_bucket_and_text();
                out.banner(output_mask, mode, header_time_text.as_str());
                out.mode_status(output_mask, running_go2_phase);
                out.io.raw_write_str(ecma48::RESTORE_CURSOR);
                last_chrome_state = chrome_state;
            }
            if let Some(entry) = appended_transcript_line(&transcript, &next_transcript) {
                out.push_transcript_line(slot_content_top_row(output_mask), entry);
            } else {
                render_active_slot_content(&out, output_mask, &next_transcript);
            }
            render_prompt_line(&out, output_mask, &line);
            transcript = next_transcript;
        }

        let (minute_bucket, minute_text) = clock_bucket_and_text();
        if minute_bucket != last_minute_bucket {
            last_minute_bucket = minute_bucket;
            redraw_clock_preserving_cursor(&out, output_mask, minute_text.as_str());
        }

        if let Some(b) = io.read_byte() {
            if let Some(vm_id) = active_matrix_vm_id(output_mask)
                && crate::hv::blueprint_console_submit_tui_demo_input(vm_id, b)
            {
                esc = EscState::None;
                text_decode.reset();
                live_history_cursor = None;
                zeroize_input_line(&mut line);
                continue;
            }
            if b == LOCAL_ESCAPE_KEY_BYTE {
                esc = EscState::None;
                text_decode.reset();
                live_history_cursor = None;
                if active_matrix_slot_is_vmx(output_mask) {
                    zeroize_input_line(&mut line);
                    transcript = apply_matrix_operator_and_refresh(
                        &out,
                        io,
                        output_mask,
                        &mut mode,
                        running_go2_phase,
                        minute_text.as_str(),
                        "§",
                    );
                    last_chrome_state = current_chrome_state(output_mask, mode);
                    last_matrix_revision = matrix::visible_revision(output_mask);
                }
                continue;
            }
            if b == 0x1c && !active_matrix_slot_is_vmx(output_mask) {
                esc = EscState::None;
                text_decode.reset();
                live_history_cursor = None;
                continue;
            }
            if b == LOCAL_UNMAPPED_KEY_BYTE {
                continue;
            }
            if b == 0x03 {
                esc = EscState::None;
                text_decode.reset();
                live_history_cursor = None;
                transcript = handle_control_c(io, &out, output_mask, &mut line);
                continue;
            }
            match esc {
                EscState::None => {
                    if b == 0x1b {
                        text_decode.reset();
                        esc = EscState::Esc;
                        continue;
                    }
                }
                EscState::Esc => {
                    match b {
                        b'[' => {
                            esc = EscState::Csi;
                            csi_input.reset();
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
                        b'A' => {
                            if let Some(entry) = cycle_live_history(true, &mut live_history_cursor)
                            {
                                set_input_line(&out, output_mask, &mut line, entry.as_str());
                            }
                            esc = EscState::None;
                        }
                        b'B' => {
                            if let Some(entry) = cycle_live_history(false, &mut live_history_cursor)
                            {
                                set_input_line(&out, output_mask, &mut line, entry.as_str());
                            }
                            esc = EscState::None;
                        }
                        b'0'..=b'9' => {
                            let digit = (b - b'0') as u16;
                            csi_input.push_digit(digit as u8);
                        }
                        b';' => {
                            csi_input.push_separator();
                        }
                        b'~' => {
                            esc = EscState::None;
                        }
                        b't' => {
                            if let Some((cols, rows)) = csi_input.terminal_size()
                                && apply_reported_terminal_size(output_mask, cols, rows)
                            {
                                transcript = redraw_active_view(
                                    &out,
                                    io,
                                    output_mask,
                                    mode,
                                    running_go2_phase,
                                    minute_text.as_str(),
                                );
                                render_prompt_line(&out, output_mask, &line);
                                last_chrome_state = current_chrome_state(output_mask, mode);
                                last_matrix_revision = matrix::visible_revision(output_mask);
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
                    text_decode.reset();
                    if active_matrix_vm_id(output_mask).is_some() {
                        continue;
                    }
                    mode = mode.next();
                    apply_mode_toggle(
                        &out,
                        output_mask,
                        mode,
                        running_go2_phase,
                        &line,
                        minute_text.as_str(),
                    );
                }
                b'\r' | b'\n' => {
                    if let Some(ch) = text_decode.finish_lossy() {
                        push_input_char(&out, &mut line, ch);
                    }
                    live_history_cursor = None;
                    let submitted_raw = line.as_str();
                    matrix::record_user_input(
                        transport_scope_for_backend(io),
                        user_submission_for_recording(output_mask, submitted_raw),
                    );
                    let submitted = submitted_raw.trim();
                    out.prompt(output_mask);
                    let active_slot = matrix::active_slot_id(output_mask);
                    let active_slot_lifetime_generation =
                        matrix::slot_lifetime_generation(&active_slot);
                    let session_indexes = find_command_session_indexes(
                        command_sessions.as_slice(),
                        &active_slot,
                        active_slot_lifetime_generation,
                    );
                    let has_broadcast_sessions = session_indexes
                        .iter()
                        .any(|idx| command_sessions[*idx].kind.accepts_broadcast_input());
                    if let Some(operator) = parse_double_section_operator(submitted) {
                        match operator {
                            DoubleSectionOperator::Clear => {
                                matrix::clear_active_lines(output_mask);
                            }
                            DoubleSectionOperator::Online(requested) => {
                                record_user_line_for_active_slot(io, submitted);
                                shell2_apps::submit_online(&spawner, io, requested);
                            }
                        }
                        transcript = current_transcript_for_task(io);
                        render_active_slot_content(&out, output_mask, &transcript);
                    } else if is_matrix_operator(submitted) {
                        transcript = apply_matrix_operator_and_refresh(
                            &out,
                            io,
                            output_mask,
                            &mut mode,
                            running_go2_phase,
                            minute_text.as_str(),
                            submitted,
                        );
                        last_chrome_state = current_chrome_state(output_mask, mode);
                    } else if active_matrix_slot_is_vmx(output_mask)
                        && is_vmx_leave_command(submitted)
                    {
                        transcript = apply_matrix_operator_and_refresh(
                            &out,
                            io,
                            output_mask,
                            &mut mode,
                            running_go2_phase,
                            minute_text.as_str(),
                            "§",
                        );
                        last_chrome_state = current_chrome_state(output_mask, mode);
                    } else if let Some(vm_id) = active_matrix_vm_input_id(output_mask) {
                        if !submitted.is_empty() {
                            record_user_line_for_active_slot(io, submitted);
                            if is_vmx_tui_command(submitted) || is_vmx_control_command(submitted) {
                                let _ = crate::hv::blueprint_console_submit_control_line(
                                    vm_id, submitted,
                                );
                                transcript = current_transcript_for_task(io);
                                render_active_slot_content(&out, output_mask, &transcript);
                            } else {
                                let mut input = alloc::vec::Vec::from(submitted.as_bytes());
                                input.push(b'\n');
                                let _ = crate::hv::blueprint_console_submit_stdin(vm_id, &input);
                                transcript = current_transcript_for_task(io);
                                render_active_slot_content(&out, output_mask, &transcript);
                            }
                        }
                    } else if let Some(vm_id) = active_matrix_vm_id(output_mask) {
                        if !submitted.is_empty() {
                            record_user_line_for_active_slot(io, submitted);
                            if is_vmx_tui_command(submitted) || is_vmx_control_command(submitted) {
                                let _ = crate::hv::blueprint_console_submit_control_line(
                                    vm_id, submitted,
                                );
                                transcript = current_transcript_for_task(io);
                                render_active_slot_content(&out, output_mask, &transcript);
                            } else {
                                let mut input = alloc::vec::Vec::from(submitted.as_bytes());
                                input.push(b'\n');
                                let _ = crate::hv::blueprint_console_submit_stdin(vm_id, &input);
                                transcript = current_transcript_for_task(io);
                                render_active_slot_content(&out, output_mask, &transcript);
                            }
                        }
                    } else if has_broadcast_sessions {
                        if !submitted.is_empty() {
                            record_user_line_for_active_slot(io, submitted);
                            transcript = current_transcript_for_task(io);
                            render_active_slot_content(&out, output_mask, &transcript);
                        }
                        let mut remove_indexes: alloc::vec::Vec<usize> = alloc::vec::Vec::new();
                        for session_idx in session_indexes {
                            if !command_sessions[session_idx].kind.accepts_broadcast_input() {
                                continue;
                            }
                            match handle_command_session_input(
                                &spawner,
                                io,
                                &command_sessions[session_idx],
                                submitted,
                                output_mask,
                            ) {
                                CommandSessionInputResult::CompleteIdle => {
                                    if command_sessions[session_idx].kind.shows_session_activity() {
                                        matrix::set_slot_activity(
                                            &command_sessions[session_idx].slot_id,
                                            matrix::MatrixSlotActivity::Idle,
                                        );
                                    }
                                    remove_indexes.push(session_idx);
                                }
                                CommandSessionInputResult::CompleteRunning => {
                                    remove_indexes.push(session_idx);
                                }
                                CommandSessionInputResult::KeepRunning => {}
                            }
                        }
                        remove_indexes.sort_unstable();
                        remove_indexes.dedup();
                        for session_idx in remove_indexes.into_iter().rev() {
                            let _ = command_sessions.remove(session_idx);
                        }
                    } else if let Some(session_idx) = find_command_session_index(
                        command_sessions.as_slice(),
                        &active_slot,
                        active_slot_lifetime_generation,
                    ) {
                        if !submitted.is_empty() {
                            record_user_line_for_active_slot(io, submitted);
                            transcript = current_transcript_for_task(io);
                            render_active_slot_content(&out, output_mask, &transcript);
                        }
                        match handle_command_session_input(
                            &spawner,
                            io,
                            &command_sessions[session_idx],
                            submitted,
                            output_mask,
                        ) {
                            CommandSessionInputResult::CompleteIdle => {
                                matrix::set_slot_activity(
                                    &command_sessions[session_idx].slot_id,
                                    matrix::MatrixSlotActivity::Idle,
                                );
                                let _ = command_sessions.remove(session_idx);
                            }
                            CommandSessionInputResult::CompleteRunning => {
                                let _ = command_sessions.remove(session_idx);
                            }
                            CommandSessionInputResult::KeepRunning => {}
                        }
                    } else if !submitted.is_empty() {
                        if is_matrix_operator(submitted) {
                            handle_matrix_operator(io, submitted);
                            mode = ShellMode2::Cmd;
                            configure_output_view(&out, output_mask);
                            out.banner(output_mask, mode, minute_text.as_str());
                            out.mode_status(output_mask, running_go2_phase);
                            transcript = current_transcript_for_task(io);
                            render_active_slot_content(&out, output_mask, &transcript);
                            last_chrome_state = current_chrome_state(output_mask, mode);
                        } else {
                            if !submitted.is_empty() {
                                record_user_line_for_active_slot(io, submitted);
                                transcript = current_transcript_for_task(io);
                                render_active_slot_content(&out, output_mask, &transcript);
                            }
                            let submit_result = handle_submit(&spawner, io, mode, submitted);
                            match submit_result {
                                HandleSubmitResult::SetLineWidth(width) => {
                                    set_line_width_for_output(output_mask, width);
                                    configure_output_view(&out, output_mask);
                                    out.banner(output_mask, mode, minute_text.as_str());
                                    out.mode_status(output_mask, running_go2_phase);
                                    transcript = current_transcript_for_task(io);
                                    render_active_slot_content(&out, output_mask, &transcript);
                                    last_chrome_state = current_chrome_state(output_mask, mode);
                                }
                                HandleSubmitResult::StartSession(kind) => {
                                    let slot_id = matrix::active_slot_id(output_mask);
                                    let slot_lifetime_generation =
                                        matrix::slot_lifetime_generation(&slot_id);
                                    if kind.shows_session_activity() {
                                        matrix::set_slot_activity(
                                            &slot_id,
                                            matrix::MatrixSlotActivity::Session,
                                        );
                                    }
                                    command_sessions.retain(|session| {
                                        !(session.slot_id == slot_id && session.kind == kind)
                                    });
                                    command_sessions.push(CommandSession {
                                        slot_id,
                                        slot_lifetime_generation,
                                        kind,
                                    });
                                }
                                HandleSubmitResult::None => {}
                            }
                        }
                    }
                    zeroize_input_line(&mut line);
                    out.prompt(output_mask);
                    input_bytes_since_yield = 0;
                    if (output_mask & OUTPUT_NET_TCP_MASK) != 0 {
                        Timer::after(EmbassyDuration::from_millis(10)).await;
                    } else {
                        Timer::after(EmbassyDuration::from_micros(0)).await;
                    }
                }
                0x08 | 0x7F => {
                    if text_decode.is_pending() {
                        text_decode.reset();
                    } else if pop_input_grapheme(&mut line) {
                        render_prompt_line(&out, output_mask, &line);
                    }
                }
                0x20..=0x7E | 0x80..=0xFF => {
                    for ch in text_decode.push(b).chars() {
                        push_input_char(&out, &mut line, ch);
                    }
                }
                _ => {
                    text_decode.reset();
                }
            }

            input_bytes_since_yield = input_bytes_since_yield.saturating_add(1);
            if input_bytes_since_yield >= 64 {
                input_bytes_since_yield = 0;
                Timer::after(EmbassyDuration::from_micros(0)).await;
            }
        } else {
            if let Some(vm_id) = active_matrix_vm_id(output_mask) {
                let _ = crate::hv::blueprint_console_tui_demo_idle(vm_id);
            }
            if (output_mask & (OUTPUT_LOCAL_MASK | OUTPUT_NET_TCP_MASK)) == 0 {
                if terminal_size_query_idle_ticks == 0 {
                    out.io.raw_write_str(TERMINAL_SIZE_QUERY);
                    terminal_size_query_idle_ticks = TERMINAL_SIZE_QUERY_IDLE_TICKS;
                } else {
                    terminal_size_query_idle_ticks =
                        terminal_size_query_idle_ticks.saturating_sub(1);
                }
            }
            Timer::after(EmbassyDuration::from_millis(5)).await;
        }
    }
}
