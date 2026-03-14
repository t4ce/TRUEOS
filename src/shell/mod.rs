use alloc::string::String as AllocString;
use alloc::collections::VecDeque;
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String;
use spin::Mutex;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::shell::cube::{CUBE_COLS, CUBE_ROWS, CubeState, WireShape};

pub(crate) mod ecma48;

mod actions;
pub(crate) mod bench;
pub(crate) mod cmd;
pub(crate) mod cube;
pub(crate) mod matrix;
pub(crate) mod shelltetris;
pub(crate) mod statusbar;
pub(crate) mod table;
pub(crate) mod txt;
pub(crate) mod wizards;
pub use wizards::*;
pub(crate) mod output;
pub(crate) use output::{ReverseOutput, apply_shell_scroll_region};
mod crlf;
mod interface;
pub(crate) use interface::{ShellBackend, ShellIo};
pub(crate) mod backends;
pub(crate) use backends::{NET_TCP_SHELL_BACKEND, UART1_COM1_BACKEND};
pub(crate) mod uart1_com1;

pub(crate) struct Utf8Decoder {
    buf: [u8; 4],
    len: usize,
    need: usize,
}

impl Utf8Decoder {
    pub(crate) const fn new() -> Self {
        Self {
            buf: [0u8; 4],
            len: 0,
            need: 0,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.len = 0;
        self.need = 0;
    }

    pub(crate) fn push(&mut self, b: u8) -> Option<char> {
        if self.len == 0 {
            if b < 0x80 {
                return Some(b as char);
            }
            let need = match b {
                0xC2..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF4 => 4,
                _ => {
                    // Invalid leading byte.
                    return None;
                }
            };
            self.buf[0] = b;
            self.len = 1;
            self.need = need;
            return None;
        }

        // Continuation byte.
        if (b & 0xC0) != 0x80 {
            self.clear();
            return None;
        }

        self.buf[self.len] = b;
        self.len += 1;
        if self.len < self.need {
            return None;
        }

        let s = core::str::from_utf8(&self.buf[..self.need]).ok()?;
        let ch = s.chars().next()?;
        self.clear();
        Some(ch)
    }
}

pub(crate) const PROMPT_RGB: (u8, u8, u8) = (255, 55, 255);
const MATRIX_RUNNING_GLYPHS: [char; 9] = ['⢈', '⡈', '⡐', '⡠', '⣀', '⢄', '⢂', '⢁', '⡁'];
const DEFAULT_TERM_COLS: usize = 100;
const DEFAULT_TERM_ROWS: usize = 30;
const SHELL1_HISTORY_LIMIT: usize = 512;

static SHELL1_HISTORY_TAIL: Mutex<VecDeque<AllocString>> = Mutex::new(VecDeque::new());
static SHELL1_HISTORY_TOTAL_LINES: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn record_history_line(line: &str) {
    let total = SHELL1_HISTORY_TOTAL_LINES.fetch_add(1, Ordering::Relaxed) + 1;
    let mut tail = SHELL1_HISTORY_TAIL.lock();
    tail.push_back(AllocString::from(line));
    while tail.len() > SHELL1_HISTORY_LIMIT {
        let _ = tail.pop_front();
    }
    if total < tail.len() {
        SHELL1_HISTORY_TOTAL_LINES.store(tail.len(), Ordering::Relaxed);
    }
}

pub(crate) fn history_total_lines() -> usize {
    SHELL1_HISTORY_TOTAL_LINES.load(Ordering::Relaxed)
}

pub(crate) fn history_text_since(start_line: usize, max_lines: usize) -> AllocString {
    let total = history_total_lines();
    let tail = SHELL1_HISTORY_TAIL.lock();
    let retained = tail.len();
    let oldest_line = total.saturating_sub(retained);
    let start = start_line.max(oldest_line);
    let available = total.saturating_sub(start);
    let take = if max_lines == 0 {
        available
    } else {
        core::cmp::min(available, max_lines)
    };

    let mut out = AllocString::new();
    for idx in 0..take {
        let absolute_line = start + idx;
        let rel = absolute_line.saturating_sub(oldest_line);
        if let Some(line) = tail.get(rel) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(line.as_str());
        }
    }
    out
}

pub(crate) async fn handle_command_action_for_tools(
    action: CommandAction,
    mode: &mut ShellMode,
    cube_mode: &mut bool,
    cube: &mut cube::CubeState,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
    spawner: &Spawner,
    history: &mut alloc::vec::Vec<alloc::string::String>,
) {
    let mut pre_cube_term: Option<(usize, usize)> = None;
    actions::handle_command_action(
        action,
        mode,
        cube_mode,
        cube,
        io,
        &mut pre_cube_term,
        term_cols,
        term_rows,
        spawner,
        history,
    )
    .await;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_shell1_command_registry_json(
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    let json: AllocString = crate::shell::cmd::registry::command_registry_json();
    let bytes = json.as_bytes();

    if out_ptr.is_null() || out_cap == 0 {
        return bytes.len() as isize;
    }

    let copy_len = core::cmp::min(bytes.len(), out_cap);
    core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr, copy_len);
    copy_len as isize
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_shell1_history_total_lines() -> usize {
    history_total_lines()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_shell1_history_text_since(
    start_line: usize,
    max_lines: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    let text = history_text_since(start_line, max_lines);
    let bytes = text.as_bytes();

    if out_ptr.is_null() || out_cap == 0 {
        return bytes.len() as isize;
    }

    let copy_len = core::cmp::min(bytes.len(), out_cap);
    core::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr, copy_len);
    copy_len as isize
}

#[inline]
fn write_prompt(io: &dyn ShellIo) {
    io.write_fmt(format_args!("{}", crate::ecma48::pos(2, 1)));
    io.write_str(crate::ecma48::CLEAR_LINE);
    // Ensure cursor is a blinking block (user preference)
    io.write_str(crate::ecma48::CURSOR_BLINKING_BLOCK);
    io.write_fmt(format_args!("{}", crate::ecma48::color("§ ", PROMPT_RGB)));
}

#[inline]
pub(crate) fn write_submitted_prompt(io: &dyn ShellIo, text: &str) {
    io.write_fmt(format_args!("{}", crate::ecma48::pos(2, 1)));
    io.write_str(crate::ecma48::CLEAR_LINE);
    io.write_str(crate::ecma48::CURSOR_BLINKING_BLOCK);
    io.write_str("\x1b[38;2;150;150;150m§ ");
    io.write_str(text);
    io.write_str("\x1b[0m");
}

#[inline]
fn starts_with_ignore_ascii_case(s: &str, prefix: &str) -> bool {
    if prefix.len() > s.len() {
        return false;
    }
    s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

fn handle_tab_completion(
    io: &dyn ShellBackend,
    line: &mut String<128>,
    term_cols: usize,
    term_rows: usize,
    mode: &ShellMode,
    history: &mut alloc::vec::Vec<alloc::string::String>,
) {
    if !matches!(mode, ShellMode::Idle) {
        return;
    }

    let mut input_buf: String<128> = String::new();
    let _ = input_buf.push_str(line.as_str());
    let input = input_buf.as_str();
    if input.is_empty() {
        return;
    }

    // If the command token is already present, Tab shows that command's usage line.
    if input.chars().any(|ch| ch.is_whitespace()) {
        let cmd = input.split_whitespace().next().unwrap_or("");
        if !cmd.is_empty() {
            let mut usage: String<192> = String::new();
            if crate::shell::cmd::registry::usage_text_for_name(cmd, &mut usage) {
                ReverseOutput::new(io, term_cols, term_rows, history)
                    .write_overlay_hint(usage.as_str());
            }
        }
        return;
    }

    let mut cmds: heapless::Vec<&'static str, 64> = heapless::Vec::new();
    crate::shell::cmd::registry::list_command_names(&mut cmds);
    cmds.as_mut_slice().sort_unstable();

    let mut shown: heapless::Vec<&'static str, 5> = heapless::Vec::new();
    let mut match_count = 0usize;
    let mut unique: Option<&'static str> = None;

    for name in cmds.iter().copied() {
        if starts_with_ignore_ascii_case(name, input) {
            match_count += 1;
            if unique.is_none() {
                unique = Some(name);
            }
            if shown.len() < 5 {
                let _ = shown.push(name);
            }
        }
    }

    if match_count == 0 {
        return;
    }

    if match_count == 1 {
        let target = unique.unwrap_or(input);
        if target.len() > input.len() {
            let suffix = &target[input.len()..];
            for ch in suffix.chars() {
                if line.push(ch).is_err() {
                    break;
                }
                io.write_char(ch);
            }
        }
        if !line.as_str().ends_with(' ') && line.push(' ').is_ok() {
            io.write_char(' ');
        }
        let mut usage: String<192> = String::new();
        if crate::shell::cmd::registry::usage_text_for_name(target, &mut usage) {
            ReverseOutput::new(io, term_cols, term_rows, history)
                .write_overlay_hint(usage.as_str());
        } else {
            ReverseOutput::new(io, term_cols, term_rows, history).write_overlay_hint("");
        }
        return;
    }

    let mut msg: String<192> = String::new();
    // let _ = msg.push_str("matches: ");
    for (idx, name) in shown.iter().enumerate() {
        if idx != 0 {
            let _ = msg.push(' ');
        }
        let _ = msg.push_str(name);
    }
    if match_count > shown.len() {
        let _ = msg.push_str(" ...");
    }
    ReverseOutput::new(io, term_cols, term_rows, history).write_overlay_hint(msg.as_str());
}

#[inline]
fn write_banner(io: &dyn ShellIo, term_cols: usize) {
    io.write_str(crate::ecma48::HIDE_CURSOR);
    refresh_title_bar(io, term_cols);
    // Draw Prompt here to ensure layout? No, prompt is drawn by write_prompt.
    // Just restore cursor.
    io.write_str(crate::ecma48::SHOW_CURSOR);
}

#[inline]
fn refresh_title_bar(io: &dyn ShellIo, term_cols: usize) {
    if term_cols == 0 {
        return;
    }

    // Atomic update of Row 1
    io.write_str(crate::ecma48::HIDE_CURSOR);
    io.write_str(crate::ecma48::SAVE_CURSOR);

    io.write_fmt(format_args!("{}", crate::ecma48::pos(1, 1)));
    io.write_str(crate::ecma48::CLEAR_LINE);
    io.write_fmt(format_args!("{}", crate::ecma48::bold("TRUEOS")));

    crate::matrix::refresh_matrix_symbols(io, term_cols);

    let mut time_buf: heapless::String<32> = heapless::String::new();
    if let Some(ts) =
        crate::v::net::ntp::current_unix_seconds().or_else(crate::time::unix_time_seconds)
    {
        let (_year, _month, _day, hour, minute, _second) = unix_timestamp_to_ymdhms(ts);
        let _ = core::fmt::write(&mut time_buf, format_args!("{:02}:{:02}", hour, minute));
    } else {
        let _ = time_buf.push_str("time unavailable");
    }

    let text_len = time_buf.as_str().len();
    if text_len > 0 {
        let start_col = term_cols.saturating_sub(text_len).saturating_add(1);
        io.write_fmt(format_args!("{}", crate::ecma48::pos(1, start_col)));
        io.write_fmt(format_args!(
            "{}",
            crate::ecma48::style(time_buf.as_str())
                .bold()
                .fg((120, 210, 255))
        ));
    }

    io.write_str(crate::ecma48::RESTORE_CURSOR);
    io.write_str(crate::ecma48::SHOW_CURSOR);
}

// refresh_status_bar moved to statusbar.rs

pub(crate) enum CommandAction {
    None,
    Pending(PendingAction),
    SetTermSize { cols: usize, rows: usize },
    ShowInstallDiskTable,
    ShowFormatDiskTable,
    ShowUpdateDiskTable,
    ShowFileMountTable,
    ShowBenchDiskTable,
    ShowNetbenchNicTable,
    EnterCube,
    EnterIco,
    EnterGo,
    EnterGoTwo,
    EnterRain,
    EnterTetris,
    EnterTxtEdt { filename: String<48>, slot_id: u8 },
    RunNetbench { nic_index: usize },
    RunBenchFs { disk_id: u32 },
    DoFormat { disc_id: u32 },
    DoInstall { disc_id: u32 },
    DoUpdate { disc_id: u32 },
}

#[embassy_executor::task(pool_size = 3)]
pub async fn task(spawner: Spawner, io: &'static dyn ShellBackend) {
    io.init();

    // Ensure the registry is populated before the shell starts.
    self::cmd::registry::init_builtin_shell_commands();

    let mut term_cols: usize = DEFAULT_TERM_COLS;
    let mut term_rows: usize = DEFAULT_TERM_ROWS;

    let mut history: alloc::vec::Vec<alloc::string::String> = alloc::vec::Vec::new();
    let mut scroll_offset: usize = 0;

    write_banner(io, term_cols);
    output::apply_shell_scroll_region(io, term_rows);
    write_prompt(io);

    let mut line: String<128> = String::new();
    let mut utf8 = Utf8Decoder::new();
    let mut mode = ShellMode::Idle;

    let mut cube_mode = true;
    let mut cube = CubeState::new();
    cube.set_shape(WireShape::Cube);
    cube.reset();
    enter_cube_mode(io, &mut term_cols, &mut term_rows);

    // When entering cube/ico modes from the normal shell, we save the previous
    // terminal size so we can restore it on exit. This matters if the user
    // previously used `set <cols> <rows>`.
    let mut pre_cube_term: Option<(usize, usize)> = None;

    // Treat CRLF as a single Enter (common on serial/USB bridges).
    let mut saw_cr: bool = false;
    let mut next_status_refresh: Instant = Instant::now() + EmbassyDuration::from_millis(250);
    let mut esc_state = 0;

    // Initial status bar draw
    crate::shell::statusbar::refresh(io, term_cols, term_rows);

    loop {
        if Instant::now() >= next_status_refresh {
            crate::shell::statusbar::refresh(io, term_cols, term_rows);
            next_status_refresh = Instant::now() + EmbassyDuration::from_millis(250);
        }

        if let Some(b) = io.read_byte() {
            if saw_cr && b == b'\n' {
                saw_cr = false;
                continue;
            }
            saw_cr = b == b'\r';
            if cube_mode {
                if b == b'\r' || b == b'\n' {
                    cube_mode = false;
                    let (cols, rows) = pre_cube_term
                        .take()
                        .unwrap_or((DEFAULT_TERM_COLS, DEFAULT_TERM_ROWS));
                    term_cols = cols;
                    term_rows = rows;
                    io.write_str(crate::ecma48::CLEAR_SCREEN);
                    io.write_str(crate::ecma48::HOME);
                    write_banner(io, term_cols);
                    apply_shell_scroll_region(io, term_rows);
                    write_prompt(io);
                }
                continue;
            }

            if esc_state == 1 {
                if b == b'[' {
                    esc_state = 2;
                } else {
                    esc_state = 0;
                }
                continue;
            } else if esc_state == 2 {
                match b {
                    b'A' => {
                        // Scroll Up (Back in history) - "Older"
                        let top = 3usize;
                        let bottom = output::output_bottom_row(term_rows);
                        let height = bottom.saturating_sub(top).saturating_add(1);
                        let max_offset = history.len().saturating_sub(height);

                        if scroll_offset < max_offset {
                            scroll_offset = scroll_offset.saturating_add(1);
                            output::redraw_view(io, &history, scroll_offset, term_cols, term_rows);
                        }
                    }
                    b'B' => {
                        // Scroll Down (Forward in history) - "Newer"
                        if scroll_offset > 0 {
                            scroll_offset = scroll_offset.saturating_sub(1);
                            output::redraw_view(io, &history, scroll_offset, term_cols, term_rows);
                        }
                    }
                    _ => {}
                }
                esc_state = 0;
                continue;
            }

            // Handle special keys
            match b {
                0x1B => {
                    esc_state = 1;
                    continue;
                }
                b'\r' | b'\n' => {
                    utf8.clear();

                    let wizard_rev_io = matches!(
                        mode,
                        ShellMode::Wizard(
                            wizards::InstallWizardStage::SelectDisk
                                | wizards::InstallWizardStage::FormatSelectDisk
                                | wizards::InstallWizardStage::UpdateSelectDisk
                                | wizards::InstallWizardStage::BenchSelectDisk
                        ) | ShellMode::Confirm(
                            wizards::PendingAction::InstallConfirm { .. }
                                | wizards::PendingAction::FormatConfirm { .. }
                                | wizards::PendingAction::UpdateConfirm { .. }
                        )
                    );

                    // In wizard/confirm modes that print status lines, route both user submissions
                    // and wizard output into the ReverseOutput scrollback (right-aligned), keeping
                    // the prompt row clean and avoiding cursor-position surprises.
                    let result = if wizard_rev_io {
                        let submitted = line.trim();
                        if !submitted.is_empty() {
                            ReverseOutput::new(io, term_cols, term_rows, &mut history)
                                .echo_command(submitted);
                        }
                        let wiz_io = ReverseOutput::new(io, term_cols, term_rows, &mut history);
                        mode.process_input(&wiz_io, &line, &spawner).await
                    } else {
                        mode.process_input(io, &line, &spawner).await
                    };
                    match result {
                        InputResult::Handled => {
                            line.clear();
                            wizards::write_prompt_for_state(io, &mode);
                            continue;
                        }
                        InputResult::Transition(new_mode) => {
                            mode = new_mode;
                            line.clear();
                            wizards::write_prompt_for_state(io, &mode);
                            continue;
                        }
                        InputResult::RunAction(action) => {
                            handle_command_action(
                                action,
                                &mut mode,
                                &mut cube_mode,
                                &mut cube,
                                io,
                                &mut pre_cube_term,
                                &mut term_cols,
                                &mut term_rows,
                                &spawner,
                                &mut history,
                            )
                            .await;
                            line.clear();
                            wizards::write_prompt_for_state(io, &mode);
                            continue;
                        }
                        InputResult::ProcessCommand => {
                            // Fall through to standard shell command processing
                        }
                    }

                    if !line.is_empty() {
                        write_submitted_prompt(io, line.as_str());
                        // Enter uses the same prefix matching behavior as Tab:
                        // unique -> expand and execute, ambiguous -> show matches and stay in-place.
                        let mut input_buf: String<128> = String::new();
                        let _ = input_buf.push_str(line.as_str());
                        let input = input_buf.as_str();
                        if !input.chars().any(|ch| ch.is_whitespace()) {
                            // ... existing tab completion logic ...
                        }

                        // Execute command
                        let action = handle_line(
                            &line,
                            &spawner,
                            io,
                            &mut term_cols,
                            &mut term_rows,
                            &mut mode,
                            &mut history,
                        );
                        let cmd_echo = line.clone();
                        line.clear();

                        // We reset scroll offset on new command? Usually yes.
                        scroll_offset = 0;

                        ReverseOutput::new(io, term_cols, term_rows, &mut history)
                            .echo_command(cmd_echo.trim());
                        handle_command_action(
                            action,
                            &mut mode,
                            &mut cube_mode,
                            &mut cube,
                            io,
                            &mut pre_cube_term,
                            &mut term_cols,
                            &mut term_rows,
                            &spawner,
                            &mut history,
                        )
                        .await;
                        wizards::write_prompt_for_state(io, &mode);
                    } else {
                        // Empty line
                        line.clear();
                        // Echo empty line? "ReverseOutput" handles newline?
                        // io.write_str("\r\n");
                        // But we want it in history maybe? No.
                        // Standard shell just prints another prompt.
                        // But we should probably advance the line physically?
                        // If we just reprint prompt at same location it looks like nothing happened.
                        // But handle_line usually scrolls output?

                        // If we just print \r\n, it bypasses our history.
                        // We should use ReverseOutput to "print nothing" but trigger a line shift?
                        // Or just echo empty string?
                        // ReverseOutput::echo_command("") returns early.

                        // Let's just do io.write_str("\r\n") but it might mess up if we aren't careful.
                        // Actually, write_prompt puts cursor at (2,1).
                        // If we \r\n, cursor goes to (3,1).
                        // We should validly consume a line in history buffer?
                        // Let's just write \r\n for now.
                        io.write_str("\r\n");
                        wizards::write_prompt_for_state(io, &mode);
                    }
                }
                0x08 | 0x7F => {
                    utf8.clear();
                    if !line.is_empty() {
                        line.pop();
                        io.write_str("\x08 \x08");
                    }
                }
                0x03 => {
                    // Ctrl+C
                    utf8.clear();
                    line.clear();
                    io.write_str("^C\r\n");
                    mode = ShellMode::Idle;
                    wizards::write_prompt_for_state(io, &mode);
                }
                b'\t' => {
                    utf8.clear();
                    handle_tab_completion(io, &mut line, term_cols, term_rows, &mode, &mut history);
                }
                _ => {
                    if b >= 0x20
                        && let Some(ch) = utf8.push(b)
                        && line.push(ch).is_ok()
                    {
                        io.write_char(ch);
                    }
                }
            }
        } else {
            if cube_mode {
                cube.draw_frame(io);
                Timer::after(EmbassyDuration::from_millis(333)).await;
                continue;
            }

            if let ShellMode::Wait { deadline, action } = &mode
                && Instant::now() >= *deadline
            {
                // Check specific actions on timeout
                match action {
                    PendingAction::AcpiReset => {
                        if let Err(e) = crate::efi::acpi::facp::reset_system() {
                            io.write_fmt(format_args!("\r\nacpi reset failed: {:?}\r\n", e));
                        }
                    }
                    PendingAction::AcpiState(level) => {
                        if let Err(e) = crate::efi::acpi::facp::enter_named_sleep_state(*level) {
                            io.write_fmt(format_args!("\r\nacpi s{} failed: {:?}\r\n", level, e));
                        }
                    }
                    _ => {}
                }
                mode = ShellMode::Idle;
                wizards::write_prompt_for_state(io, &mode);
            }

            Timer::after(EmbassyDuration::from_millis(2)).await;
        }
    }
}

fn handle_line(
    line: &str,
    spawner: &Spawner,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
    mode: &mut ShellMode,
    history: &mut alloc::vec::Vec<alloc::string::String>,
) -> CommandAction {
    let cmd = line.trim();
    if cmd.is_empty() {
        return CommandAction::None;
    }

    // Ensure scrolling region is set correctly (e.g. if term resized).
    output::apply_shell_scroll_region(io, *term_rows);

    let p_io = ReverseOutput::new(io, *term_cols, *term_rows, history);

    // New-style registered commands (typed args + validation + introspection).
    {
        let mut ctx = crate::shell::cmd::registry::ShellCommandCtx {
            line: cmd,
            spawner,
            io: &p_io,
            term_cols,
            term_rows,
            mode,
        };
        if let Some(action) = crate::shell::cmd::registry::dispatch_line(&mut ctx) {
            return action;
        }
    }

    p_io.write_str("unknown: ");
    p_io.write_str(cmd);
    CommandAction::None
}

fn unix_timestamp_to_ymdhms(ts: u64) -> (u32, u8, u8, u8, u8, u8) {
    const SECS_PER_MIN: u64 = 60;
    const SECS_PER_HOUR: u64 = 60 * SECS_PER_MIN;
    const SECS_PER_DAY: u64 = 24 * SECS_PER_HOUR;

    let mut days = ts / SECS_PER_DAY;
    let mut rem = ts % SECS_PER_DAY;

    let hour = (rem / SECS_PER_HOUR) as u8;
    rem %= SECS_PER_HOUR;
    let minute = (rem / SECS_PER_MIN) as u8;
    let second = (rem % SECS_PER_MIN) as u8;

    let mut year: u32 = 1970;
    loop {
        let days_in_year = if is_leap_year(year) { 366u64 } else { 365u64 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let month_lengths = month_lengths(year);
    let mut month_idx = 0;
    while month_idx < month_lengths.len() {
        let len = month_lengths[month_idx] as u64;
        if days < len {
            let day = (days + 1) as u8;
            return (year, (month_idx + 1) as u8, day, hour, minute, second);
        }
        days -= len;
        month_idx += 1;
    }

    (year, 12, 31, hour, minute, second)
}

fn month_lengths(year: u32) -> [u8; 12] {
    if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    }
}

fn is_leap_year(year: u32) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

pub(crate) fn draw_corners(io: &dyn ShellIo, cols: usize, rows: usize) {
    if cols == 0 || rows == 0 {
        return;
    }
    io.write_str(crate::ecma48::SAVE_CURSOR);
    // top-right
    write_pos(io, 1, cols);
    io.write_byte(b'O');
    // bottom-left
    write_pos(io, rows, 1);
    io.write_byte(b'O');
    // bottom-right
    write_pos(io, rows, cols);
    io.write_byte(b'O');
    io.write_str(crate::ecma48::RESTORE_CURSOR);
}

#[inline]
fn write_pos(io: &dyn ShellIo, row: usize, col: usize) {
    io.write_fmt(format_args!("{}", crate::ecma48::pos(row, col)));
}

fn enter_cube_mode(io: &dyn ShellIo, term_cols: &mut usize, term_rows: &mut usize) {
    *term_cols = CUBE_COLS;
    *term_rows = CUBE_ROWS;
    draw_corners(io, CUBE_COLS, CUBE_ROWS);
    cube::enter_mode(io);
}

async fn handle_command_action(
    action: CommandAction,
    mode: &mut ShellMode,
    cube_mode: &mut bool,
    cube: &mut CubeState,
    io: &'static dyn ShellBackend,
    pre_cube_term: &mut Option<(usize, usize)>,
    term_cols: &mut usize,
    term_rows: &mut usize,
    spawner: &Spawner,
    history: &mut alloc::vec::Vec<alloc::string::String>,
) {
    actions::handle_command_action(
        action,
        mode,
        cube_mode,
        cube,
        io,
        pre_cube_term,
        term_cols,
        term_rows,
        spawner,
        history,
    )
    .await;
}
