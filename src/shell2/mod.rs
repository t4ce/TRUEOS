use alloc::collections::VecDeque;
use alloc::string::String as AllocString;
use alloc::vec::Vec;
use core::cell::Cell;
use core::fmt::Write as _;
use core::sync::atomic::{AtomicU8, Ordering};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::String as HString;
pub(crate) mod backends;
pub(crate) mod cmds;
mod ecma48;
mod interface;
mod matrix;
mod shell2_apps;
mod shell2_cmd;
mod shell2_cmd_registry;
mod shell2_lumen;
mod shell2_qjs;
mod shell2_qjs_c4;
mod shell2_qjs_c4_contract;
mod shell2_surf;
mod term_style;
#[allow(unused_imports)]
pub(crate) use crate::shell2::backends::{
    CONTAINER_SHELL_BACKEND, NET_TCP_SHELL_BACKEND, UART1_COM1_BACKEND, UI3_SHELL_BACKEND,
    Ui3ShellCell, Ui3ShellScreenSnapshot, container_shell_drain_output,
    container_shell_read_output_byte, container_shell_submit_input, crlf,
    queue_ui3_keyboard_event as queue_ui3_shell_keyboard_event, uart1_com1,
    ui3_shell_attach_window, ui3_shell_last_rendered_seq, ui3_shell_line_width,
    ui3_shell_mark_rendered, ui3_shell_set_line_width, ui3_shell_snapshot,
};
pub(crate) use interface::{ShellBackend2, ShellIo2};
use shell2_apps::AppsPromptMode;
use shell2_lumen::LumenPromptMode;
use shell2_qjs::QjsPromptMode;
use shell2_surf::SurfPromptPrefix;

const MAX_LINE: usize = 192;
const BANNER_ROW: usize = 1;
const STATUS_ROW: usize = 2;
const PROMPT_ROW: usize = 3;
const SCROLL_TOP_ROW: usize = 4;
const STATUS_SELECTED_RGB: (u8, u8, u8) = (255, 55, 255);
const FUNCTION_KEY_RGB: (u8, u8, u8) = (255, 255, 255);
const SYSTEM_TEXT_RGB: (u8, u8, u8) = (60, 183, 161);
pub(crate) const OUTPUT_UART1_MASK: u8 = 1 << 0;
pub(crate) const OUTPUT_NET_TCP_MASK: u8 = 1 << 1;
pub(crate) const OUTPUT_UI3_MASK: u8 = 1 << 2;
pub(crate) const OUTPUT_CONTAINER_MASK: u8 = 1 << 3;
const SECTION_STATUS_TEXT: &str = "t4ce is with you";
const SECTION_STATUS_HOLD_MS: u64 = 1000;
const SECTION_RAINBOW_FRAME_MS: u64 = 120;
const SECTION_RAINBOW_COLORS: [u8; 8] = [199, 208, 227, 121, 51, 39, 99, 201];
const STATUS_NORMAL_RGB: (u8, u8, u8) = (255, 255, 255);
const VMX_CONSOLE_COMMANDS: &str = "echo hostname homedir env disc thread help exit";
const BANNER_TITLE_TEXT: &str = "TRUE OS";
const BANNER_CLOCK_WIDTH: usize = 5;
const BANNER_GROUP_GAP_WIDTH: usize = 1;

static REGISTERED_OUTPUTS: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineSource {
    User,
    Native,
    System,
}

#[derive(Clone)]
pub(crate) struct TranscriptEntry {
    pub(crate) source: LineSource,
    pub(crate) text: AllocString,
}

#[derive(Clone)]
struct CommandSession {
    slot_id: matrix::MatrixSlotId,
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
    output_mask: u8,
    slot_id: matrix::MatrixSlotId,
    interrupt_generation: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShellMode2 {
    Surf,
    Apps,
    Qjs,
    Cmd,
    #[cfg(feature = "trueos_lumen")]
    Lumen,
}

impl ShellMode2 {
    const fn function_key(self) -> &'static str {
        match self {
            Self::Surf => "F1",
            Self::Apps => "F2",
            Self::Qjs => "F3",
            Self::Cmd => "F4",
            #[cfg(feature = "trueos_lumen")]
            Self::Lumen => "F5",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Surf => "surf",
            Self::Apps => "apps",
            Self::Qjs => "qjs",
            Self::Cmd => "cmd",
            #[cfg(feature = "trueos_lumen")]
            Self::Lumen => "lumen",
        }
    }
}

#[inline]
fn is_lumen_mode(mode: ShellMode2) -> bool {
    #[cfg(feature = "trueos_lumen")]
    {
        mode == ShellMode2::Lumen
    }
    #[cfg(not(feature = "trueos_lumen"))]
    {
        let _ = mode;
        false
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
    line_width: Cell<usize>,
}

impl<'a> AlignedWriter<'a> {
    fn new(io: &'a dyn ShellIo2) -> Self {
        Self {
            io,
            line_width: Cell::new(matrix::DEFAULT_MATRIX_SLOT_LINE_WIDTH),
        }
    }

    fn line_width(&self) -> usize {
        self.line_width.get()
    }

    fn set_line_width(&self, width: usize) {
        self.line_width.set(width);
    }

    fn clear_screen_home(&self) {
        self.io.raw_write_str("\x1b[2J\x1b[H");
    }

    fn set_scroll_region(&self, top: usize) {
        // Reserve header rows by scrolling only in [top..bottom].
        self.io
            .raw_write_fmt(format_args!("\x1b[{};999r", top.max(1)));
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

    fn transcript_line_at(&self, row: usize, source: LineSource, s: &str) {
        self.move_to(row, 1);
        self.clear_line();
        self.io.raw_write_str(ecma48::RESET);

        match source {
            LineSource::User | LineSource::Native => {
                self.io.raw_write_str(s);
            }
            LineSource::System => {
                let width = ecma48::visible_width(s);
                let col = self.line_width().saturating_sub(width).saturating_add(1);
                self.move_to(row, col);
                self.io
                    .raw_write_fmt(format_args!("{}", term_style::paint(s).color(SYSTEM_TEXT_RGB)));
            }
        }
    }

    fn render_transcript(&self, transcript: &VecDeque<TranscriptEntry>) {
        self.io.raw_write_str(ecma48::SAVE_CURSOR);
        self.move_to(SCROLL_TOP_ROW, 1);
        self.io.raw_write_str("\x1b[J");

        if transcript_prefers_chronological_layout(transcript) {
            for (idx, entry) in transcript.iter().enumerate() {
                let row = SCROLL_TOP_ROW + idx;
                self.transcript_line_at(row, entry.source, entry.text.as_str());
            }
        } else {
            for (idx, entry) in transcript.iter().rev().enumerate() {
                let row = SCROLL_TOP_ROW + idx;
                self.transcript_line_at(row, entry.source, entry.text.as_str());
            }
        }
        self.io.raw_write_str(ecma48::RESTORE_CURSOR);
    }

    fn push_transcript_line(&self, entry: &TranscriptEntry) {
        self.io.raw_write_str(ecma48::SAVE_CURSOR);
        self.move_to(SCROLL_TOP_ROW, 1);
        self.io.raw_write_str("\x1b[L");
        self.transcript_line_at(SCROLL_TOP_ROW, entry.source, entry.text.as_str());
        self.io.raw_write_str(ecma48::RESTORE_CURSOR);
    }

    fn banner(&self, output_mask: u8, mode: ShellMode2, time_text: &str) {
        self.move_to(BANNER_ROW, 1);
        self.clear_line();
        self.banner_left(output_mask, time_text);
        if active_matrix_vm_id(output_mask).is_some() {
            self.right_text(BANNER_ROW, self.vmx_console_text().as_str());
        } else {
            self.right_text(BANNER_ROW, self.banner_right_text(output_mask, mode).as_str());
        }
    }

    fn banner_left(&self, output_mask: u8, time_text: &str) {
        self.move_to(BANNER_ROW, 1);
        self.io.raw_write_str(BANNER_TITLE_TEXT);
        self.io.raw_write_char(' ');
        self.io.raw_write_str(time_text);
        if active_matrix_vm_id(output_mask).is_some() {
            let vmx =
                alloc::format!(" {}", term_style::paint("VMX").bold().color(STATUS_SELECTED_RGB));
            self.io.raw_write_str(vmx.as_str());
        }
    }

    fn mode_status(
        &self,
        output_mask: u8,
        mode: ShellMode2,
        qjs_mode: QjsPromptMode,
        apps_mode: AppsPromptMode,
        lumen_mode: LumenPromptMode,
        surf_prefix: SurfPromptPrefix,
        cmd_status_text: Option<&str>,
        running_go2_phase: usize,
    ) {
        self.move_to(STATUS_ROW, 1);
        self.clear_line();
        let slot_text = self.slot_status_text(output_mask, running_go2_phase);
        if !slot_text.is_empty() {
            self.left_text(STATUS_ROW, slot_text.as_str());
        }
        if active_matrix_vm_id(output_mask).is_some() {
            self.vmx_console_status(cmd_status_text);
        } else if mode == ShellMode2::Surf {
            self.surf_status(surf_prefix);
        } else if mode == ShellMode2::Apps {
            self.apps_status(apps_mode);
        } else if mode == ShellMode2::Qjs {
            self.qjs_status(qjs_mode);
        } else if mode == ShellMode2::Cmd {
            self.cmd_status(cmd_status_text);
        } else if is_lumen_mode(mode) {
            #[cfg(feature = "trueos_lumen")]
            self.lumen_status(output_mask, lumen_mode);
        }
        self.io.raw_write_str(ecma48::RESET);
    }

    fn vmx_console_text(&self) -> AllocString {
        let mut text = AllocString::new();
        self.push_ai_token(&mut text, "vmx", true);
        self.push_plain(&mut text, " ");
        self.push_plain(&mut text, VMX_CONSOLE_COMMANDS);
        text
    }

    fn vmx_console_status(&self, _cmd_status_text: Option<&str>) {}

    fn main_mode_text(&self, mode: ShellMode2) -> AllocString {
        let mut text = AllocString::new();
        self.push_mode_choice(&mut text, ShellMode2::Surf, mode == ShellMode2::Surf);
        self.push_plain(&mut text, " - ");
        self.push_mode_choice(&mut text, ShellMode2::Apps, mode == ShellMode2::Apps);
        self.push_plain(&mut text, " - ");
        self.push_mode_choice(&mut text, ShellMode2::Qjs, mode == ShellMode2::Qjs);
        self.push_plain(&mut text, " - ");
        self.push_mode_choice(&mut text, ShellMode2::Cmd, mode == ShellMode2::Cmd);
        #[cfg(feature = "trueos_lumen")]
        {
            self.push_plain(&mut text, " - ");
            self.push_mode_choice(&mut text, ShellMode2::Lumen, mode == ShellMode2::Lumen);
        }
        text
    }

    fn banner_right_text(&self, output_mask: u8, mode: ShellMode2) -> AllocString {
        let mut text = self.active_slot_label(output_mask);
        if !text.is_empty() {
            self.push_plain(&mut text, " ");
        }
        self.push_plain(&mut text, self.main_mode_text(mode).as_str());
        text
    }

    fn active_slot_label(&self, output_mask: u8) -> AllocString {
        let active_slot = matrix::active_slot_id(output_mask);
        let mut label = AllocString::from("§");
        if !active_slot.is_empty() {
            label.push_str(active_slot.as_str());
        }
        alloc::format!(
            "{}",
            term_style::paint(label.as_str())
                .bold()
                .color(STATUS_SELECTED_RGB)
        )
    }

    fn push_mode_choice(&self, out: &mut AllocString, mode: ShellMode2, selected: bool) {
        self.push_function_key_label(out, alloc::format!("[{}]", mode.function_key()).as_str());
        self.push_plain(out, " ");
        self.push_mode_token(out, mode.label(), selected);
    }

    fn surf_status(&self, surf_prefix: SurfPromptPrefix) {
        let mut text = AllocString::new();
        self.push_function_key_label(&mut text, "[TAB]");
        self.push_plain(&mut text, " ");
        self.push_ai_token(
            &mut text,
            SurfPromptPrefix::Https.label(),
            surf_prefix == SurfPromptPrefix::Https,
        );
        self.push_plain(&mut text, " - ");
        self.push_ai_token(
            &mut text,
            SurfPromptPrefix::Http.label(),
            surf_prefix == SurfPromptPrefix::Http,
        );
        self.push_plain(&mut text, " - ");
        self.push_ai_token(
            &mut text,
            SurfPromptPrefix::File.label(),
            surf_prefix == SurfPromptPrefix::File,
        );
        self.push_plain(&mut text, " - ");
        self.push_ai_token(
            &mut text,
            SurfPromptPrefix::Html.label(),
            surf_prefix == SurfPromptPrefix::Html,
        );
        self.right_text(STATUS_ROW, text.as_str());
    }

    fn qjs_status(&self, qjs_mode: QjsPromptMode) {
        let mut text = AllocString::new();
        self.push_function_key_label(&mut text, "[TAB]");
        self.push_plain(&mut text, " ");
        self.push_ai_token(&mut text, "repl", qjs_mode == QjsPromptMode::Repl);
        self.push_plain(&mut text, " - ");
        self.push_ai_token(&mut text, "eval", qjs_mode == QjsPromptMode::Eval);
        self.right_text(STATUS_ROW, text.as_str());
    }

    fn apps_status(&self, apps_mode: AppsPromptMode) {
        let mut text = AllocString::new();
        self.push_function_key_label(&mut text, "[TAB]");
        self.push_plain(&mut text, " ");
        for (idx, mode) in [
            AppsPromptMode::Start,
            AppsPromptMode::Online,
            AppsPromptMode::Pause,
            AppsPromptMode::Unpause,
            AppsPromptMode::Save,
            AppsPromptMode::Load,
            AppsPromptMode::Stop,
            AppsPromptMode::Status,
        ]
        .iter()
        .copied()
        .enumerate()
        {
            if idx > 0 {
                self.push_plain(&mut text, " - ");
            }
            self.push_ai_token(&mut text, mode.label(), apps_mode == mode);
        }
        self.right_text(STATUS_ROW, text.as_str());
    }

    fn cmd_status(&self, cmd_status_text: Option<&str>) {
        let Some(cmd_status_text) = cmd_status_text else {
            return;
        };
        if !cmd_status_text.is_empty() {
            self.right_text(STATUS_ROW, cmd_status_text);
        }
    }

    fn lumen_status(&self, output_mask: u8, lumen_mode: LumenPromptMode) {
        let mut text = AllocString::new();
        let _ = lumen_mode;
        self.push_plain(&mut text, shell2_lumen::lumen_status(output_mask).as_str());
        self.right_text(STATUS_ROW, text.as_str());
    }

    fn push_plain(&self, out: &mut AllocString, text: &str) {
        out.push_str(text);
    }

    fn push_function_key_label(&self, out: &mut AllocString, text: &str) {
        let styled = alloc::format!("{}", term_style::paint(text).color(FUNCTION_KEY_RGB));
        out.push_str(styled.as_str());
    }

    fn push_ai_token(&self, out: &mut AllocString, text: &str, selected: bool) {
        if selected {
            let styled =
                alloc::format!("{}", term_style::paint(text).bold().color(STATUS_SELECTED_RGB));
            out.push_str(styled.as_str());
        } else {
            out.push_str(text);
        }
    }

    fn push_mode_token(&self, out: &mut AllocString, text: &str, selected: bool) {
        if selected {
            let styled =
                alloc::format!("{}", term_style::paint(text).bold().color(STATUS_SELECTED_RGB));
            out.push_str(styled.as_str());
        } else {
            let styled = alloc::format!("{}", term_style::paint(text).bold());
            out.push_str(styled.as_str());
        }
    }

    fn slot_status_text(&self, output_mask: u8, _running_go2_phase: usize) -> AllocString {
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

    fn prompt(&self, _output_mask: u8) {
        self.move_to(PROMPT_ROW, 1);
        self.clear_line();
        self.io.raw_write_str("\x1b[0m");
        self.io.raw_write_str(ecma48::SHOW_CURSOR);
        self.io.raw_write_str(ecma48::CURSOR_COLOR_GRAY);
        self.io.raw_write_str(ecma48::CURSOR_BLINKING_BLOCK);
    }

    fn user_backspace(&self) {
        self.io.raw_write_str("\x08 \x08");
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

pub(crate) fn print_native_line(io: &dyn ShellIo2, text: &str) {
    enqueue_transcript_line(io, LineSource::Native, text);
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
        return;
    }
    let ui3_io: &'static dyn ShellIo2 = &UI3_SHELL_BACKEND;
    if same_backend_io(io, ui3_io) {
        REGISTERED_OUTPUTS.fetch_or(OUTPUT_UI3_MASK, Ordering::Relaxed);
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

pub(crate) fn minimum_line_width_for_backend(io: &'static dyn ShellBackend2) -> usize {
    minimum_line_width_for_output(output_target_for_backend(io))
}

fn line_width_for_output(output_mask: u8) -> usize {
    let min_width = minimum_line_width_for_output(output_mask);
    if (output_mask & OUTPUT_UI3_MASK) != 0 {
        ui3_shell_line_width().max(min_width)
    } else {
        matrix::active_line_width(output_mask).max(min_width)
    }
}

fn set_line_width_for_output(output_mask: u8, width: usize) {
    let width = width.max(minimum_line_width_for_output(output_mask));
    if (output_mask & OUTPUT_UI3_MASK) != 0 {
        ui3_shell_set_line_width(width.max(1));
    } else {
        matrix::set_active_line_width(output_mask, width.max(1));
    }
}

fn minimum_line_width_for_output(output_mask: u8) -> usize {
    let mut left = ecma48::visible_width(BANNER_TITLE_TEXT)
        .saturating_add(1)
        .saturating_add(BANNER_CLOCK_WIDTH);
    if active_matrix_vm_id(output_mask).is_some() {
        left = left
            .saturating_add(1)
            .saturating_add(ecma48::visible_width("VMX"));
    }
    left.saturating_add(BANNER_GROUP_GAP_WIDTH)
        .saturating_add(banner_right_visible_width(output_mask))
}

fn banner_right_visible_width(output_mask: u8) -> usize {
    if active_matrix_vm_id(output_mask).is_some() {
        return ecma48::visible_width("vmx")
            .saturating_add(1)
            .saturating_add(ecma48::visible_width(VMX_CONSOLE_COMMANDS));
    }
    active_slot_label_visible_width(output_mask)
        .saturating_add(1)
        .saturating_add(main_mode_visible_width())
}

fn active_slot_label_visible_width(output_mask: u8) -> usize {
    let active_slot = matrix::active_slot_id(output_mask);
    1usize.saturating_add(ecma48::visible_width(active_slot.as_str()))
}

fn main_mode_visible_width() -> usize {
    #[cfg(feature = "trueos_lumen")]
    let modes = [
        ShellMode2::Surf,
        ShellMode2::Apps,
        ShellMode2::Qjs,
        ShellMode2::Cmd,
        ShellMode2::Lumen,
    ];
    #[cfg(not(feature = "trueos_lumen"))]
    let modes = [
        ShellMode2::Surf,
        ShellMode2::Apps,
        ShellMode2::Qjs,
        ShellMode2::Cmd,
    ];
    let mut width = 0usize;
    for (idx, mode) in modes.iter().copied().enumerate() {
        if idx != 0 {
            width = width.saturating_add(ecma48::visible_width(" - "));
        }
        width = width
            .saturating_add(mode.function_key().len())
            .saturating_add(3)
            .saturating_add(ecma48::visible_width(mode.label()));
    }
    width
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

    let ui3_io: &'static dyn ShellIo2 = &UI3_SHELL_BACKEND;
    if same_backend_task(io, ui3_io) {
        return OUTPUT_UI3_MASK;
    }

    let container_io: &'static dyn ShellIo2 = &CONTAINER_SHELL_BACKEND;
    if same_backend_task(io, container_io) {
        return OUTPUT_CONTAINER_MASK;
    }

    0
}

pub(crate) fn matrix_target_for_backend(io: &'static dyn ShellBackend2) -> MatrixTarget {
    let output_mask = output_target_for_backend(io);
    let slot_id = matrix::active_slot_id(output_mask);
    let interrupt_generation = matrix::slot_interrupt_generation(&slot_id);
    MatrixTarget {
        output_mask,
        slot_id,
        interrupt_generation,
    }
}

pub(crate) fn matrix_target_for_slot_name(output_mask: u8, requested: &str) -> MatrixTarget {
    let slot_id = matrix::slot_id_from_name(requested);
    let interrupt_generation = matrix::slot_interrupt_generation(&slot_id);
    MatrixTarget {
        output_mask,
        slot_id,
        interrupt_generation,
    }
}

pub(crate) fn switch_matrix_target_slot(target: &MatrixTarget, requested: &str) -> MatrixTarget {
    let slot_id = matrix::switch_active_slot(target.output_mask, requested);
    let interrupt_generation = matrix::slot_interrupt_generation(&slot_id);
    MatrixTarget {
        output_mask: target.output_mask,
        slot_id,
        interrupt_generation,
    }
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

fn matrix_target_for_slot(output_mask: u8, slot_id: &matrix::MatrixSlotId) -> MatrixTarget {
    MatrixTarget {
        output_mask,
        slot_id: slot_id.clone(),
        interrupt_generation: matrix::slot_interrupt_generation(slot_id),
    }
}

pub(crate) fn set_matrix_target_active(target: &MatrixTarget, active: bool) {
    if active {
        matrix::begin_slot_running(&target.slot_id);
    } else {
        matrix::end_slot_running(&target.slot_id);
    }
}

pub(crate) fn matrix_target_interrupted(target: &MatrixTarget) -> bool {
    matrix::slot_interrupt_generation(&target.slot_id) != target.interrupt_generation
}

pub(crate) fn bind_matrix_target_vm(target: &MatrixTarget, vm_id: u8) {
    matrix::bind_slot_vm(&target.slot_id, vm_id, false);
}

pub(crate) fn bind_matrix_target_vm_input(target: &MatrixTarget, vm_id: u8) {
    matrix::bind_slot_vm(&target.slot_id, vm_id, true);
}

pub(crate) fn unbind_matrix_target_vm(target: &MatrixTarget, vm_id: u8) {
    matrix::unbind_slot_vm(&target.slot_id, vm_id);
}

pub(crate) fn active_matrix_vm_input_id(output_mask: u8) -> Option<u8> {
    matrix::active_slot_vm_input_id(output_mask)
}

pub(crate) fn active_matrix_vm_id(output_mask: u8) -> Option<u8> {
    matrix::active_slot_vm_id(output_mask)
}

pub(crate) fn history_total_lines() -> usize {
    matrix::history_total_lines()
}

pub(crate) fn history_lines_text(start_line: usize, max_lines: usize) -> AllocString {
    matrix::history_lines_text(start_line, max_lines)
}

pub(crate) fn take_user_input_record() -> Vec<AllocString> {
    matrix::take_user_input_record()
}

pub(crate) fn restore_user_input_record(entries: Vec<AllocString>) {
    matrix::restore_user_input_record(entries)
}

pub(crate) fn command_registry_json() -> AllocString {
    cmds::command_registry_json()
}

fn command_names_status_text() -> AllocString {
    shell2_cmd_registry::command_names_status_text()
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

    let ui3_io: &'static dyn ShellIo2 = &UI3_SHELL_BACKEND;
    if same_backend_io(io, ui3_io) {
        return OUTPUT_UI3_MASK;
    }

    let container_io: &'static dyn ShellIo2 = &CONTAINER_SHELL_BACKEND;
    if same_backend_io(io, container_io) {
        return OUTPUT_CONTAINER_MASK;
    }

    0
}

fn enqueue_transcript_line(io: &dyn ShellIo2, source: LineSource, text: &str) {
    let output_mask = output_mask_for_io(io);
    if output_mask == 0 {
        return;
    }

    let _ = matrix::record_line_for_output(output_mask, source, text);
}

pub(crate) fn print_matrix_target_line(target: &MatrixTarget, text: &str) {
    matrix::record_line_in_slot(&target.slot_id, LineSource::System, text);
}

pub(crate) fn print_matrix_target_native_line(target: &MatrixTarget, text: &str) {
    matrix::record_line_in_slot(&target.slot_id, LineSource::Native, text);
}

pub(crate) fn raw_write_matrix_target(target: &MatrixTarget, bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }

    let io: &'static dyn ShellIo2 = if (target.output_mask & OUTPUT_NET_TCP_MASK) != 0 {
        &NET_TCP_SHELL_BACKEND
    } else if (target.output_mask & OUTPUT_UI3_MASK) != 0 {
        &UI3_SHELL_BACKEND
    } else if (target.output_mask & OUTPUT_CONTAINER_MASK) != 0 {
        &CONTAINER_SHELL_BACKEND
    } else {
        &UART1_COM1_BACKEND
    };

    match core::str::from_utf8(bytes) {
        Ok(text) => io.raw_write_str(text),
        Err(_) => {
            for &b in bytes {
                io.raw_write_byte(b);
            }
        }
    }
    bytes.len()
}

pub(crate) fn read_matrix_target_byte(target: &MatrixTarget) -> Option<u8> {
    if (target.output_mask & OUTPUT_NET_TCP_MASK) != 0 {
        NET_TCP_SHELL_BACKEND.read_byte()
    } else if (target.output_mask & OUTPUT_UI3_MASK) != 0 {
        UI3_SHELL_BACKEND.read_byte()
    } else if (target.output_mask & OUTPUT_CONTAINER_MASK) != 0 {
        CONTAINER_SHELL_BACKEND.read_byte()
    } else {
        UART1_COM1_BACKEND.read_byte()
    }
}

fn current_transcript_for_task(io: &'static dyn ShellBackend2) -> VecDeque<TranscriptEntry> {
    matrix::active_lines(output_target_for_backend(io))
}

pub(crate) fn repaint_backend_screen(io: &'static dyn ShellBackend2) {
    register_output(io);
    let out = AlignedWriter::new(io);
    let output_mask = output_target_for_backend(io);
    out.set_line_width(line_width_for_output(output_mask));
    out.clear_screen_home();
    out.reset_scroll_region();

    let (_, time_text) = clock_bucket_and_text();
    let mode = ShellMode2::Cmd;
    let qjs_mode = QjsPromptMode::Repl;
    let apps_mode = AppsPromptMode::Start;
    let lumen_mode = LumenPromptMode::Default;
    let surf_prefix = SurfPromptPrefix::Https;

    out.banner(output_mask, mode, time_text.as_str());
    out.mode_status(output_mask, mode, qjs_mode, apps_mode, lumen_mode, surf_prefix, None, 0);
    out.set_scroll_region(SCROLL_TOP_ROW);

    let transcript = current_transcript_for_task(io);
    out.render_transcript(&transcript);
    out.prompt(output_mask);
}

fn appended_transcript_line<'a>(
    prev: &VecDeque<TranscriptEntry>,
    next: &'a VecDeque<TranscriptEntry>,
) -> Option<&'a TranscriptEntry> {
    if transcript_prefers_chronological_layout(prev)
        || transcript_prefers_chronological_layout(next)
    {
        return None;
    }

    if next.len() != prev.len().saturating_add(1) {
        return None;
    }

    for (prev_entry, next_entry) in prev.iter().zip(next.iter()) {
        if prev_entry.source != next_entry.source || prev_entry.text != next_entry.text {
            return None;
        }
    }

    next.back()
}

fn transcript_prefers_chronological_layout(transcript: &VecDeque<TranscriptEntry>) -> bool {
    transcript
        .iter()
        .any(|entry| matches!(entry.source, LineSource::Native))
}

fn record_user_line_for_active_slot(io: &'static dyn ShellBackend2, submitted: &str) {
    let _ =
        matrix::record_line_for_output(output_target_for_backend(io), LineSource::User, submitted);
}

fn handle_matrix_operator(io: &'static dyn ShellBackend2, submitted: &str) {
    matrix::record_line_in_default(LineSource::User, submitted);
    if submitted
        .strip_prefix('§')
        .and_then(|rest| rest.strip_suffix('§'))
        .is_some()
    {
        shell2_qjs::free_slot(submitted);
        let _ = matrix::free_slot(submitted);
    } else {
        let requested = submitted.strip_prefix('§').unwrap_or("");
        let _ = matrix::switch_active_slot(output_target_for_backend(io), requested);
    }
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
    output_mask: u8,
    mode: ShellMode2,
    qjs_mode: QjsPromptMode,
    apps_mode: AppsPromptMode,
    lumen_mode: LumenPromptMode,
    surf_prefix: SurfPromptPrefix,
    cmd_status_text: Option<&str>,
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

    out.mode_status(
        output_mask,
        mode,
        qjs_mode,
        apps_mode,
        lumen_mode,
        surf_prefix,
        cmd_status_text,
        running_go2_phase,
    );
}

fn handle_submit(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    mode: ShellMode2,
    qjs_mode: QjsPromptMode,
    apps_mode: AppsPromptMode,
    lumen_mode: LumenPromptMode,
    surf_prefix: SurfPromptPrefix,
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
        ShellMode2::Surf => {
            if let Some(parsed) = shell2_surf::try_parse_with_prefix(submitted, surf_prefix) {
                match parsed {
                    shell2_surf::SurfSubmit::Html(html) => {
                        shell2_surf::load_inline_html(spawner, io, html);
                    }
                    shell2_surf::SurfSubmit::File(file_ref) => {
                        shell2_surf::load_file_reference(spawner, io, file_ref.as_str());
                    }
                    shell2_surf::SurfSubmit::Url(url) => {
                        shell2_surf::prepare_call_with_url(spawner, io, url.as_str());
                    }
                }
            }
            HandleSubmitResult::None
        }
        ShellMode2::Qjs => {
            let target = matrix_target_for_backend(io);
            shell2_qjs::submit(spawner, io, &target, qjs_mode, submitted);
            HandleSubmitResult::None
        }
        ShellMode2::Apps => {
            shell2_apps::submit(spawner, io, apps_mode, submitted);
            HandleSubmitResult::None
        }
        #[cfg(feature = "trueos_lumen")]
        ShellMode2::Lumen => {
            let target = matrix_target_for_backend(io);
            shell2_lumen::submit(io, lumen_mode, &target, submitted);
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
) -> Option<usize> {
    sessions
        .iter()
        .position(|session| session.slot_id == *slot_id)
}

fn find_command_session_indexes(
    sessions: &[CommandSession],
    slot_id: &matrix::MatrixSlotId,
) -> alloc::vec::Vec<usize> {
    sessions
        .iter()
        .enumerate()
        .filter_map(|(idx, session)| (session.slot_id == *slot_id).then_some(idx))
        .collect()
}

fn handle_command_session_input(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    session: &CommandSession,
    submitted: &str,
    output_mask: u8,
) -> CommandSessionInputResult {
    let target = matrix_target_for_slot(output_mask, &session.slot_id);
    match session.kind {
        shell2_cmd::CommandSessionKind::BenchRunning(session_id) => {
            crate::shell2::cmds::bench::handle_session_input(session_id, &target, submitted)
        }
        shell2_cmd::CommandSessionKind::FormatSure(disc_id) => {
            crate::shell2::cmds::format::handle_session_input(
                spawner, io, &target, submitted, disc_id,
            )
        }
        shell2_cmd::CommandSessionKind::GpuCanvasRunning(session_id) => {
            crate::ui3::ui3_canvas::handle_session_input(session_id, &target, submitted)
        }
        shell2_cmd::CommandSessionKind::RemoveSure(session_id) => {
            crate::shell2::cmds::rm::handle_session_input(&target, submitted, session_id)
        }
    }
}

fn mode_from_function_key(index: u16) -> Option<ShellMode2> {
    match index {
        1 => Some(ShellMode2::Surf),
        2 => Some(ShellMode2::Apps),
        3 => Some(ShellMode2::Qjs),
        4 => Some(ShellMode2::Cmd),
        #[cfg(feature = "trueos_lumen")]
        5 => Some(ShellMode2::Lumen),
        _ => None,
    }
}

fn apply_mode_toggle(
    out: &AlignedWriter<'_>,
    output_mask: u8,
    mode: ShellMode2,
    qjs_mode: QjsPromptMode,
    apps_mode: AppsPromptMode,
    lumen_mode: LumenPromptMode,
    surf_prefix: SurfPromptPrefix,
    cmd_status_text: Option<&str>,
    running_go2_phase: usize,
    line: &HString<MAX_LINE>,
    minute_text: &str,
) {
    if is_lumen_mode(mode) {
        #[cfg(feature = "trueos_lumen")]
        shell2_lumen::ensure_lumen_slot(output_mask);
    }
    out.banner(output_mask, mode, minute_text);
    out.mode_status(
        output_mask,
        mode,
        qjs_mode,
        apps_mode,
        lumen_mode,
        surf_prefix,
        cmd_status_text,
        running_go2_phase,
    );
    out.prompt(output_mask);
    for ch in line.chars() {
        out.user_char(ch);
    }
}

fn redraw_status_preserving_cursor(
    out: &AlignedWriter<'_>,
    output_mask: u8,
    mode: ShellMode2,
    qjs_mode: QjsPromptMode,
    apps_mode: AppsPromptMode,
    lumen_mode: LumenPromptMode,
    surf_prefix: SurfPromptPrefix,
    cmd_status_text: Option<&str>,
    running_go2_phase: usize,
) {
    out.io.raw_write_str(ecma48::SAVE_CURSOR);
    out.mode_status(
        output_mask,
        mode,
        qjs_mode,
        apps_mode,
        lumen_mode,
        surf_prefix,
        cmd_status_text,
        running_go2_phase,
    );
    out.io.raw_write_str(ecma48::RESTORE_CURSOR);
}

fn redraw_clock_preserving_cursor(out: &AlignedWriter<'_>, output_mask: u8, time_text: &str) {
    out.io.raw_write_str(ecma48::SAVE_CURSOR);
    out.io.raw_write_str(ecma48::RESET);
    out.banner_left(output_mask, time_text);
    out.io.raw_write_str(ecma48::RESET);
    out.io.raw_write_str(ecma48::RESTORE_CURSOR);
}

fn apply_matrix_operator_and_refresh(
    out: &AlignedWriter<'_>,
    io: &'static dyn ShellBackend2,
    output_mask: u8,
    mode: &mut ShellMode2,
    qjs_mode: QjsPromptMode,
    apps_mode: AppsPromptMode,
    lumen_mode: LumenPromptMode,
    surf_prefix: SurfPromptPrefix,
    cmd_status_text: Option<&str>,
    running_go2_phase: usize,
    minute_text: &str,
    submitted: &str,
) -> VecDeque<TranscriptEntry> {
    handle_matrix_operator(io, submitted);
    *mode = ShellMode2::Cmd;
    out.set_line_width(line_width_for_output(output_mask));
    out.banner(output_mask, *mode, minute_text);
    out.mode_status(
        output_mask,
        *mode,
        qjs_mode,
        apps_mode,
        lumen_mode,
        surf_prefix,
        cmd_status_text,
        running_go2_phase,
    );
    let transcript = current_transcript_for_task(io);
    out.render_transcript(&transcript);
    transcript
}

fn push_input_char(out: &AlignedWriter<'_>, line: &mut HString<MAX_LINE>, ch: char) {
    if line.push(ch).is_ok() {
        out.user_char(ch);
    }
}

fn set_input_line(
    out: &AlignedWriter<'_>,
    output_mask: u8,
    line: &mut HString<MAX_LINE>,
    text: &str,
) {
    line.clear();
    for ch in text.chars() {
        if line.push(ch).is_err() {
            break;
        }
    }
    out.prompt(output_mask);
    for ch in line.chars() {
        out.user_char(ch);
    }
}

fn handle_control_c(
    io: &'static dyn ShellBackend2,
    out: &AlignedWriter<'_>,
    output_mask: u8,
    line: &mut HString<MAX_LINE>,
) -> VecDeque<TranscriptEntry> {
    let active_slot = matrix::active_slot_id(output_mask);
    matrix::record_line_in_slot(&active_slot, LineSource::User, "^C");
    let (_, vm_id) = matrix::request_slot_interrupt(&active_slot);
    if let Some(vm_id) = vm_id {
        match crate::hv::stop(vm_id) {
            Ok(true) => {
                matrix::record_line_in_slot(
                    &active_slot,
                    LineSource::System,
                    alloc::format!("interrupt: vm{} stop requested", vm_id).as_str(),
                );
            }
            Ok(false) => {
                matrix::record_line_in_slot(
                    &active_slot,
                    LineSource::System,
                    alloc::format!("interrupt: vm{} is not running", vm_id).as_str(),
                );
            }
            Err(_) => {
                matrix::record_line_in_slot(
                    &active_slot,
                    LineSource::System,
                    alloc::format!("interrupt: vm{} stop failed", vm_id).as_str(),
                );
            }
        }
    }

    line.clear();
    let transcript = current_transcript_for_task(io);
    out.render_transcript(&transcript);
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
    io.init();
    register_output(io);
    let out = AlignedWriter::new(io);
    let output_mask = output_target_for_backend(io);
    out.set_line_width(line_width_for_output(output_mask));

    out.clear_screen_home();
    out.reset_scroll_region();
    let (mut last_minute_bucket, time_text) = clock_bucket_and_text();
    let mut mode = ShellMode2::Cmd;
    let mut surf_prefix = SurfPromptPrefix::Https;
    out.banner(output_mask, mode, time_text.as_str());
    let mut qjs_mode = QjsPromptMode::Repl;
    let mut apps_mode = AppsPromptMode::Start;
    let lumen_mode = LumenPromptMode::Default;
    let mut cmd_status_text: Option<AllocString> = None;
    let mut command_sessions: alloc::vec::Vec<CommandSession> = alloc::vec::Vec::new();
    let running_go2_phase = 0usize;
    out.mode_status(
        output_mask,
        mode,
        qjs_mode,
        apps_mode,
        lumen_mode,
        surf_prefix,
        cmd_status_text.as_deref(),
        running_go2_phase,
    );

    out.set_scroll_region(SCROLL_TOP_ROW);
    out.prompt(output_mask);

    let mut line: HString<MAX_LINE> = HString::new();
    let mut transcript: VecDeque<TranscriptEntry> = current_transcript_for_task(io);
    let mut last_matrix_revision = matrix::revision();
    let mut saw_cr = false;
    let mut esc = EscState::None;
    let mut csi_param: u16 = 0;
    let mut text_decode = ecma48::InputDecodeState::None;
    let mut live_history_cursor: Option<usize> = None;

    loop {
        command_sessions.retain(|session| match session.kind {
            shell2_cmd::CommandSessionKind::BenchRunning(session_id) => {
                crate::shell2::cmds::bench::session_alive(session_id)
            }
            _ => true,
        });

        let matrix_revision = matrix::revision();
        if matrix_revision != last_matrix_revision {
            last_matrix_revision = matrix_revision;
            let next_transcript = current_transcript_for_task(io);
            redraw_status_preserving_cursor(
                &out,
                output_mask,
                mode,
                qjs_mode,
                apps_mode,
                lumen_mode,
                surf_prefix,
                cmd_status_text.as_deref(),
                running_go2_phase,
            );
            if let Some(entry) = appended_transcript_line(&transcript, &next_transcript) {
                out.push_transcript_line(entry);
            } else {
                out.render_transcript(&next_transcript);
            }
            transcript = next_transcript;
        }

        let (minute_bucket, minute_text) = clock_bucket_and_text();
        if minute_bucket != last_minute_bucket {
            last_minute_bucket = minute_bucket;
            redraw_clock_preserving_cursor(&out, output_mask, minute_text.as_str());
        }

        if let Some(b) = io.read_byte() {
            if b == 0x03 {
                esc = EscState::None;
                text_decode = ecma48::InputDecodeState::None;
                live_history_cursor = None;
                cmd_status_text = None;
                transcript = handle_control_c(io, &out, output_mask, &mut line);
                continue;
            }
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
                        b'A' => {
                            cmd_status_text = None;
                            if let Some(entry) = cycle_live_history(true, &mut live_history_cursor)
                            {
                                set_input_line(&out, output_mask, &mut line, entry.as_str());
                            }
                            esc = EscState::None;
                        }
                        b'B' => {
                            cmd_status_text = None;
                            if let Some(entry) = cycle_live_history(false, &mut live_history_cursor)
                            {
                                set_input_line(&out, output_mask, &mut line, entry.as_str());
                            }
                            esc = EscState::None;
                        }
                        b'0'..=b'9' => {
                            let digit = (b - b'0') as u16;
                            csi_param = csi_param.saturating_mul(10).saturating_add(digit);
                        }
                        b'~' => {
                            if active_matrix_vm_id(output_mask).is_none()
                                && let Some(next_mode) =
                                    csi_param.checked_sub(10).and_then(mode_from_function_key)
                            {
                                mode = next_mode;
                                apply_mode_toggle(
                                    &out,
                                    output_mask,
                                    mode,
                                    qjs_mode,
                                    apps_mode,
                                    lumen_mode,
                                    surf_prefix,
                                    cmd_status_text.as_deref(),
                                    running_go2_phase,
                                    &line,
                                    minute_text.as_str(),
                                );
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
                    let next_mode = if active_matrix_vm_id(output_mask).is_some() {
                        None
                    } else {
                        match b {
                            b'P' => Some(ShellMode2::Surf),
                            b'Q' => Some(ShellMode2::Apps),
                            b'R' => Some(ShellMode2::Qjs),
                            b'S' => Some(ShellMode2::Cmd),
                            #[cfg(feature = "trueos_lumen")]
                            b'T' => Some(ShellMode2::Lumen),
                            _ => None,
                        }
                    };
                    if let Some(next_mode) = next_mode {
                        mode = next_mode;
                        apply_mode_toggle(
                            &out,
                            output_mask,
                            mode,
                            qjs_mode,
                            apps_mode,
                            lumen_mode,
                            surf_prefix,
                            cmd_status_text.as_deref(),
                            running_go2_phase,
                            &line,
                            minute_text.as_str(),
                        );
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
                    if active_matrix_vm_id(output_mask).is_some() {
                        continue;
                    }
                    match mode {
                        ShellMode2::Surf => {
                            cmd_status_text = None;
                            surf_prefix = surf_prefix.next();
                            out.mode_status(
                                output_mask,
                                mode,
                                qjs_mode,
                                apps_mode,
                                lumen_mode,
                                surf_prefix,
                                cmd_status_text.as_deref(),
                                running_go2_phase,
                            );
                            out.prompt(output_mask);
                            for ch in line.chars() {
                                out.user_char(ch);
                            }
                        }
                        ShellMode2::Qjs => {
                            cmd_status_text = None;
                            qjs_mode = qjs_mode.next();
                            out.mode_status(
                                output_mask,
                                mode,
                                qjs_mode,
                                apps_mode,
                                lumen_mode,
                                surf_prefix,
                                cmd_status_text.as_deref(),
                                running_go2_phase,
                            );
                            out.prompt(output_mask);
                            for ch in line.chars() {
                                out.user_char(ch);
                            }
                        }
                        ShellMode2::Apps => {
                            cmd_status_text = None;
                            apps_mode = apps_mode.next();
                            out.mode_status(
                                output_mask,
                                mode,
                                qjs_mode,
                                apps_mode,
                                lumen_mode,
                                surf_prefix,
                                cmd_status_text.as_deref(),
                                running_go2_phase,
                            );
                            out.prompt(output_mask);
                            for ch in line.chars() {
                                out.user_char(ch);
                            }
                        }
                        ShellMode2::Cmd => {
                            if line.is_empty() {
                                cmd_status_text = Some(command_names_status_text());
                                out.mode_status(
                                    output_mask,
                                    mode,
                                    qjs_mode,
                                    apps_mode,
                                    lumen_mode,
                                    surf_prefix,
                                    cmd_status_text.as_deref(),
                                    running_go2_phase,
                                );
                                out.prompt(output_mask);
                            }
                        }
                        #[cfg(feature = "trueos_lumen")]
                        ShellMode2::Lumen => {
                            cmd_status_text = None;
                            out.mode_status(
                                output_mask,
                                mode,
                                qjs_mode,
                                apps_mode,
                                lumen_mode,
                                surf_prefix,
                                cmd_status_text.as_deref(),
                                running_go2_phase,
                            );
                            out.prompt(output_mask);
                            for ch in line.chars() {
                                out.user_char(ch);
                            }
                        }
                    }
                }
                b'\r' | b'\n' => {
                    if matches!(text_decode, ecma48::InputDecodeState::Utf8Seq { .. }) {
                        push_input_char(&out, &mut line, 'Ü');
                        text_decode = ecma48::InputDecodeState::None;
                    }
                    live_history_cursor = None;
                    let submitted_raw = line.as_str();
                    matrix::record_user_input(submitted_raw);
                    let submitted = submitted_raw.trim();
                    cmd_status_text = None;
                    if is_lumen_mode(mode) {
                        #[cfg(feature = "trueos_lumen")]
                        shell2_lumen::ensure_lumen_slot(output_mask);
                    }
                    let active_slot = matrix::active_slot_id(output_mask);
                    let session_indexes =
                        find_command_session_indexes(command_sessions.as_slice(), &active_slot);
                    let has_broadcast_sessions = session_indexes
                        .iter()
                        .any(|idx| command_sessions[*idx].kind.accepts_broadcast_input());
                    if submitted == "§" && mode != ShellMode2::Qjs {
                        transcript = apply_matrix_operator_and_refresh(
                            &out,
                            io,
                            output_mask,
                            &mut mode,
                            qjs_mode,
                            apps_mode,
                            lumen_mode,
                            surf_prefix,
                            cmd_status_text.as_deref(),
                            running_go2_phase,
                            minute_text.as_str(),
                            submitted,
                        );
                        line.clear();
                        out.prompt(output_mask);
                        run_plain_section_status(
                            &out,
                            output_mask,
                            mode,
                            qjs_mode,
                            apps_mode,
                            lumen_mode,
                            surf_prefix,
                            cmd_status_text.as_deref(),
                            running_go2_phase,
                        )
                        .await;
                    } else if submitted_raw.starts_with('§')
                        && !submitted.is_empty()
                        && mode != ShellMode2::Qjs
                    {
                        transcript = apply_matrix_operator_and_refresh(
                            &out,
                            io,
                            output_mask,
                            &mut mode,
                            qjs_mode,
                            apps_mode,
                            lumen_mode,
                            surf_prefix,
                            cmd_status_text.as_deref(),
                            running_go2_phase,
                            minute_text.as_str(),
                            submitted,
                        );
                    } else if let Some(vm_id) = active_matrix_vm_id(output_mask) {
                        if !submitted.is_empty() {
                            record_user_line_for_active_slot(io, submitted);
                            let _ =
                                crate::hv::blueprint_console_submit_control_line(vm_id, submitted);
                            transcript = current_transcript_for_task(io);
                            out.render_transcript(&transcript);
                        }
                    } else if has_broadcast_sessions {
                        if !submitted.is_empty() {
                            record_user_line_for_active_slot(io, submitted);
                            transcript = current_transcript_for_task(io);
                            out.render_transcript(&transcript);
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
                    } else if let Some(session_idx) =
                        find_command_session_index(command_sessions.as_slice(), &active_slot)
                    {
                        if !submitted.is_empty() {
                            record_user_line_for_active_slot(io, submitted);
                            transcript = current_transcript_for_task(io);
                            out.render_transcript(&transcript);
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
                    } else if !submitted.is_empty() || mode == ShellMode2::Apps {
                        if submitted_raw.starts_with('§') && mode != ShellMode2::Qjs {
                            handle_matrix_operator(io, submitted);
                            mode = ShellMode2::Cmd;
                            out.set_line_width(line_width_for_output(output_mask));
                            out.banner(output_mask, mode, minute_text.as_str());
                            out.mode_status(
                                output_mask,
                                mode,
                                qjs_mode,
                                apps_mode,
                                lumen_mode,
                                surf_prefix,
                                cmd_status_text.as_deref(),
                                running_go2_phase,
                            );
                            transcript = current_transcript_for_task(io);
                            out.render_transcript(&transcript);
                        } else {
                            if !submitted.is_empty() {
                                record_user_line_for_active_slot(io, submitted);
                                transcript = current_transcript_for_task(io);
                                out.render_transcript(&transcript);
                            }
                            match handle_submit(
                                &spawner,
                                io,
                                mode,
                                qjs_mode,
                                apps_mode,
                                lumen_mode,
                                surf_prefix,
                                submitted,
                            ) {
                                HandleSubmitResult::SetLineWidth(width) => {
                                    set_line_width_for_output(output_mask, width);
                                    out.set_line_width(line_width_for_output(output_mask));
                                    out.banner(output_mask, mode, minute_text.as_str());
                                    out.mode_status(
                                        output_mask,
                                        mode,
                                        qjs_mode,
                                        apps_mode,
                                        lumen_mode,
                                        surf_prefix,
                                        cmd_status_text.as_deref(),
                                        running_go2_phase,
                                    );
                                    transcript = current_transcript_for_task(io);
                                    out.render_transcript(&transcript);
                                }
                                HandleSubmitResult::StartSession(kind) => {
                                    let slot_id = matrix::active_slot_id(output_mask);
                                    if kind.shows_session_activity() {
                                        matrix::set_slot_activity(
                                            &slot_id,
                                            matrix::MatrixSlotActivity::Session,
                                        );
                                    }
                                    command_sessions.push(CommandSession { slot_id, kind });
                                }
                                HandleSubmitResult::None => {}
                            }
                        }
                    }
                    line.clear();
                    out.prompt(output_mask);
                }
                0x08 | 0x7F => {
                    text_decode = ecma48::InputDecodeState::None;
                    cmd_status_text = None;
                    if line.pop().is_some() {
                        out.user_backspace();
                    }
                }
                0x20..=0x7E => {
                    text_decode = ecma48::InputDecodeState::None;
                    cmd_status_text = None;
                    push_input_char(&out, &mut line, b as char);
                }
                _ => {
                    cmd_status_text = None;
                    if let Some(ch) = ecma48::decode_input_byte_lossy(&mut text_decode, b) {
                        push_input_char(&out, &mut line, ch);
                    }
                }
            }
        } else {
            Timer::after(EmbassyDuration::from_millis(5)).await;
        }
    }
}
