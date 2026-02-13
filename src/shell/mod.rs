use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String;
use core::sync::atomic::Ordering;

use crate::shell::cube::{CubeState, WireShape, CUBE_COLS, CUBE_ROWS};

pub(crate) mod ecma48;

pub(crate) mod cube;
pub(crate) mod shellqjs;
pub(crate) mod txt;
pub(crate) mod table;

pub(crate) mod cmd;
pub(crate) mod matrix;
pub(crate) mod statusbar;
pub(crate) mod bench;
pub(crate) use bench::*;
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
const MATRIX_RUNNING_GLYPH: char = '⣿';
const DEFAULT_TERM_COLS: usize = 100;
const DEFAULT_TERM_ROWS: usize = 30;


#[inline]
fn write_prompt(io: &dyn ShellIo) {
    io.write_fmt(format_args!("{}", crate::ecma48::pos(2, 1)));
    io.write_str(crate::ecma48::CLEAR_LINE);
    // Ensure cursor is a blinking block (user preference)
    io.write_str(crate::ecma48::CURSOR_BLINKING_BLOCK);
    io.write_fmt(format_args!("{}", crate::ecma48::color("§ ", PROMPT_RGB)));
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
                ReverseOutput::new(io, term_cols, term_rows, history).write_overlay_hint(usage.as_str());
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
        if !line.as_str().ends_with(' ') {
            if line.push(' ').is_ok() {
                io.write_char(' ');
            }
        }
        let mut usage: String<192> = String::new();
        if crate::shell::cmd::registry::usage_text_for_name(target, &mut usage) {
            ReverseOutput::new(io, term_cols, term_rows, history).write_overlay_hint(usage.as_str());
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
    if let Some(ts) = crate::time::unix_time_seconds() {
        let (_year, _month, _day, hour, minute, _second) = unix_timestamp_to_ymdhms(ts);
        let _ = core::fmt::write(
            &mut time_buf,
            format_args!(
                "{:02}:{:02}",
                hour, minute
            ),
        );
    } else {
        let _ = time_buf.push_str("time unavailable");
    }

    let text_len = time_buf.as_str().len();
    if text_len > 0 {
        let start_col = term_cols
            .saturating_sub(text_len)
            .saturating_add(1);
        io.write_fmt(format_args!("{}", crate::ecma48::pos(1, start_col)));
        io.write_fmt(format_args!(
            "{}",
            crate::ecma48::style(time_buf.as_str()).bold().fg((120, 210, 255))
        ));
    }

    io.write_str(crate::ecma48::RESTORE_CURSOR);
    io.write_str(crate::ecma48::SHOW_CURSOR);
}

#[inline]
fn refresh_status_bar(io: &dyn ShellIo, term_cols: usize, term_rows: usize) {
    if term_cols == 0 || term_rows == 0 {
        return;
    }

    #[inline]
    fn indicator_rgb(code: u8) -> (u8, u8, u8) {
        match code {
            1 => (230, 70, 70),   // red
            2 => (70, 210, 90),   // green
            3 => (230, 190, 70),  // yellow
            4 => (70, 130, 230),  // blue
            5 => (80, 200, 210),  // cyan
            6 => (200, 90, 210),  // magenta
            7 => (210, 210, 210), // white
            _ => (90, 90, 90),    // off/idle
        }
    }

    fn fit_10(src: &str) -> heapless::String<10> {
        let mut out: heapless::String<10> = heapless::String::new();
        for ch in src.chars() {
            if out.push(ch).is_err() {
                break;
            }
        }
        while out.len() < 10 {
            let _ = out.push(' ');
        }
        out
    }

    let (indicators, left, right) = if let Some(s) = crate::shell::statusbar::snapshot_active() {
        (
            s.indicators,
            fit_10(s.left.as_str()),
            fit_10(s.right.as_str()),
        )
    } else {
        (
            [0u8; crate::shell::statusbar::INDICATOR_COUNT],
            fit_10(""),
            fit_10(""),
        )
    };

    let left: heapless::String<10> = left;
    let right: heapless::String<10> = right;

    let right_col = term_cols.saturating_sub(10).saturating_add(1);
    let left_col = 1usize;

    io.write_str(crate::ecma48::HIDE_CURSOR);
    io.write_str(crate::ecma48::SAVE_CURSOR);
    io.write_fmt(format_args!("{}", crate::ecma48::pos(term_rows, 1)));

    // White background for status bar
    let bar_bg = (255u8, 255u8, 255u8);
    io.write_fmt(format_args!("\x1b[48;2;{};{};{}m", bar_bg.0, bar_bg.1, bar_bg.2));
    for _ in 0..term_cols {
        io.write_byte(b' ');
    }
    io.write_str(crate::ecma48::RESET);

    io.write_fmt(format_args!("{}", crate::ecma48::pos(term_rows, left_col)));
    for c in indicators {
        // Adjust indicator color 7 (white) to be dark so it's visible on white bg
        let fg = if c == 7 { (0u8, 0u8, 0u8) } else { indicator_rgb(c) };
        io.write_fmt(format_args!("{}", crate::ecma48::style("o").fg(fg).bg(bar_bg)));
    }
    io.write_fmt(format_args!("{}", crate::ecma48::style(" ").bg(bar_bg)));
    
    // Left text: dark grey on white
    // io.write_fmt(format_args!("{}", crate::ecma48::style(left.as_str()).dim().fg((50, 50, 50)).bg(bar_bg)));
    io.write_str(left.as_str());

    io.write_fmt(format_args!("{}", crate::ecma48::pos(term_rows, right_col)));
    // Right text: darker pink on white
    // io.write_fmt(format_args!("{}", crate::ecma48::style(right.as_str()).bold().fg((200, 50, 150)).bg(bar_bg)));
    io.write_str(right.as_str());

    io.write_str(crate::ecma48::RESTORE_CURSOR);
    io.write_str(crate::ecma48::SHOW_CURSOR);
}








pub(crate) enum CommandAction {
    None,
    Pending(PendingAction),
    ShowInstallDiskTable,
    ShowFormatDiskTable,
    ShowUpdateDiskTable,
    ShowFileMountTable,
    ShowBenchDiskTable,
    ShowNetbenchNicTable,
    EnterCube,
    EnterIco,
    EnterGo,
    EnterTxtEdt { filename: String<48>, slot_id: u8 },
    Qjs { src: String<192> },
    NetbenchStarted,
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

    // Treat CRLF as a single Enter (common on serial/USB bridges).
    let mut saw_cr: bool = false;
    let mut next_netbench_update: Instant = Instant::now() + EmbassyDuration::from_millis(NETBENCH_UPDATE_MS);
    let mut last_netbench_state: u8 = NETBENCH_STATE.load(Ordering::Relaxed);
    let mut next_status_refresh: Instant = Instant::now() + EmbassyDuration::from_millis(250);
    let mut esc_state = 0;

    // Initial status bar draw
    refresh_status_bar(io, term_cols, term_rows);

    loop {
        let netbench_state = NETBENCH_STATE.load(Ordering::Relaxed);
        
        if Instant::now() >= next_status_refresh {
            refresh_status_bar(io, term_cols, term_rows);
            next_status_refresh = Instant::now() + EmbassyDuration::from_millis(250);
        }
        
        if netbench_state == NETBENCH_RUNNING && Instant::now() >= next_netbench_update {
            let now_tick = embassy_time_driver::now();
            let start_tick = NETBENCH_START_TICK.load(Ordering::Relaxed);
            let elapsed_ticks = now_tick.saturating_sub(start_tick);
            let hz = embassy_time_driver::TICK_HZ as u64;
            let elapsed_ms = if hz == 0 {
                0
            } else {
                elapsed_ticks.saturating_mul(1000) / hz
            };
            let bytes = NETBENCH_BYTES.load(Ordering::Relaxed);
            let bps = if elapsed_ms == 0 {
                0
            } else {
                bytes.saturating_mul(1000) / elapsed_ms
            };
            let mut speed: heapless::String<10> = heapless::String::new();
            let _ = core::fmt::Write::write_fmt(&mut speed, format_args!("{}kb/s", bps / 1024));
            let _ = crate::shell::statusbar::set_right_active(speed.as_str());
            
            // Explicitly refresh status bar during netbench so the speed update is visible
            refresh_status_bar(io, term_cols, term_rows);

            next_netbench_update = Instant::now() + EmbassyDuration::from_millis(NETBENCH_UPDATE_MS);
        }

        if netbench_state != last_netbench_state {
            if netbench_state != NETBENCH_RUNNING {
                match netbench_state {
                    NETBENCH_DONE => {
                        let _ = crate::shell::statusbar::set_left_active("done");
                        let _ = crate::shell::statusbar::set_right_active("ok");
                        for i in 0..crate::shell::statusbar::INDICATOR_COUNT {
                            let _ = crate::shell::statusbar::set_indicator_active(i, 2);
                        }
                    }
                    NETBENCH_ABORTED => {
                        let _ = crate::shell::statusbar::set_left_active("aborted");
                        let _ = crate::shell::statusbar::set_right_active("stopped");
                        for i in 0..crate::shell::statusbar::INDICATOR_COUNT {
                            let _ = crate::shell::statusbar::set_indicator_active(i, 3);
                        }
                    }
                    NETBENCH_FAILED => {
                        let code = NETBENCH_FAIL_CODE.load(Ordering::Relaxed);
                        io.write_fmt(format_args!("netbench: failed ({})\r\n", netbench_fail_text(code)));
                        let _ = crate::shell::statusbar::set_left_active("failed");
                        let mut right: heapless::String<10> = heapless::String::new();
                        let _ = core::fmt::Write::write_fmt(&mut right, format_args!("e{}", code));
                        let _ = crate::shell::statusbar::set_right_active(right.as_str());
                        for i in 0..crate::shell::statusbar::INDICATOR_COUNT {
                            let _ = crate::shell::statusbar::set_indicator_active(i, 1);
                        }
                    }
                    _ => {}
                }
            }
            last_netbench_state = netbench_state;
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
                    term_cols = DEFAULT_TERM_COLS;
                    term_rows = DEFAULT_TERM_ROWS;
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
                    },
                    b'B' => {
                        // Scroll Down (Forward in history) - "Newer"
                        if scroll_offset > 0 {
                            scroll_offset = scroll_offset.saturating_sub(1);
                            output::redraw_view(io, &history, scroll_offset, term_cols, term_rows);
                        }
                    },
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
                    
                    let result = mode.process_input(io, &line, &spawner).await;
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
                             handle_command_action(action, &mut mode, &mut cube_mode, &mut cube, io, &mut term_cols, &mut term_rows, &spawner).await;
                             line.clear();
                             wizards::write_prompt_for_state(io, &mode);
                             continue;
                        }
                        InputResult::ProcessCommand => {
                            // Fall through to standard shell command processing
                        }
                    }

                    if !line.is_empty() {
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
                        
                        ReverseOutput::new(io, term_cols, term_rows, &mut history).echo_command(cmd_echo.trim());
                        handle_command_action(action, &mut mode, &mut cube_mode, &mut cube, io, &mut term_cols, &mut term_rows, &spawner).await;
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
                0x03 => { // Ctrl+C
                    utf8.clear();
                    line.clear();
                    io.write_str("^C\r\n");
                    mode = ShellMode::Idle;
                    wizards::write_prompt_for_state(io, &mode);
                }
                b'\t' => {
                    utf8.clear();
                    handle_tab_completion(
                        io,
                        &mut line,
                        term_cols,
                        term_rows,
                        &mode,
                        &mut history,
                    );
                }
                _ => {
                    if b >= 0x20 {
                        if let Some(ch) = utf8.push(b) {
                            if line.push(ch).is_ok() {
                                io.write_char(ch);
                            }
                        }
                    }
                }
            }

        } else {
            if cube_mode {
                cube.draw_frame(io);
                Timer::after(EmbassyDuration::from_millis(333)).await;
                continue;
            }

             if let ShellMode::Wait { deadline, action } = &mode {
                if Instant::now() >= *deadline {
                     // Check specific actions on timeout
                     match action {
                        PendingAction::AcpiReset => {
                            if crate::efi::acpi::facp::reset_system().is_err() {
                                io.write_str("\r\nacpi reset failed\r\n");
                            }
                        }
                        PendingAction::AcpiState(level) => {
                            if crate::efi::acpi::facp::enter_named_sleep_state(*level).is_err() {
                                io.write_fmt(format_args!("\r\nacpi s{} failed\r\n", level));
                            }
                        }
                        _ => {}
                    }
                    mode = ShellMode::Idle;
                    wizards::write_prompt_for_state(io, &mode);
                }
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
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
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
    term_cols: &mut usize,
    term_rows: &mut usize,
    spawner: &Spawner,
) {
    match action {
        CommandAction::Pending(pending) => {
            match pending {
                PendingAction::AcpiReset | PendingAction::AcpiState(_) => {
                    *mode = ShellMode::Wait {
                        action: pending,
                        deadline: Instant::now() + EmbassyDuration::from_secs(5),
                    };
                }
                 _ => {
                    *mode = ShellMode::Confirm(pending);
                 }
            }
        }
        CommandAction::ShowInstallDiskTable => {
            print_install_disk_table(io).await;
        }
        CommandAction::ShowFormatDiskTable => {
            print_format_disk_table(io).await;
        }
        CommandAction::ShowUpdateDiskTable => {
            print_update_disk_table(io).await;
        }
        CommandAction::ShowFileMountTable => {
            print_trueosfs_mount_table(io).await;
            io.write_str("file: enter mount index or disk id (blank/q cancels)\r\n");
        }
        CommandAction::ShowBenchDiskTable => {
            print_bench_disk_table(io).await;
            io.write_str("bench: enter TRUEOSFS disk id (blank/q cancels)\r\n");
        }
        CommandAction::ShowNetbenchNicTable => {
            print_netbench_nic_table(io).await;
            io.write_str("netbench: enter nic id (blank/q cancels)\r\n");
        }
        CommandAction::Qjs { src } => {
            if trueos_qjs::async_fs::ensure_service_started(spawner) {
                 crate::shell::shellqjs::run(io, src.as_str()).await;
            } else {
                io.write_str("qjs: async fs service unavailable\r\n");
            }
        }
        CommandAction::EnterCube => {
            *cube_mode = true;
            cube.set_shape(WireShape::Cube);
            cube.reset();
            enter_cube_mode(io, term_cols, term_rows);
        }
        CommandAction::EnterIco => {
            *cube_mode = true;
            cube.set_shape(WireShape::Icosidodecahedron);
            cube.reset();
            enter_cube_mode(io, term_cols, term_rows);
        }
        CommandAction::EnterGo => {
            const GO_CHARS: [char; 9] = ['⣿', '⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];
            let mut go_idx = 0;
            io.write_str(crate::ecma48::HIDE_CURSOR);
            loop {
                    if io.read_byte().is_some() {
                        break;
                    }
                    let ch = GO_CHARS[go_idx];
                    go_idx = (go_idx + 1) % GO_CHARS.len();
                    io.write_str("\r");
                    write_prompt(io);
                    io.write_char(ch);
                    Timer::after(EmbassyDuration::from_millis(160)).await;
            }
            io.write_str(crate::ecma48::SHOW_CURSOR);
            io.write_str("\r\n");
            // No need to write prompt here, loop will do it if not cube
        }
        CommandAction::EnterTxtEdt { filename, slot_id } => {
            *cube_mode = false;
            let cols = *term_cols;
            let rows = *term_rows;

            if let Some(buf) = crate::matrix::take_blob(slot_id) {
                crate::matrix::set_state(slot_id, crate::matrix::SlotState::Running);
                let out_buf = crate::shell::txt::run(io, cols, rows, filename.as_str(), buf).await;
                let _ = crate::matrix::set_blob_owned_with_preview(slot_id, out_buf);
                crate::matrix::set_state(slot_id, crate::matrix::SlotState::Done);
                io.write_fmt(format_args!("\r\ntxt: updated §{}\r\n", slot_id + 1));
                refresh_title_bar(io, cols);
            } else {
                io.write_str("\r\ntxt: invalid slot\r\n");
            }
            io.write_str(crate::ecma48::CLEAR_SCREEN);
            io.write_str(crate::ecma48::HOME);
            write_banner(io, *term_cols);
            apply_shell_scroll_region(io, *term_rows);
        }
        CommandAction::DoFormat { disc_id } => {
             let target = crate::disc::block::device_handles()
                .into_iter()
                .find(|h| h.parent().is_none() && h.id().raw() == disc_id);
            if let Some(handle) = target {
                io.write_str("\r\nformat: creating 1 partition + TRUEOSFS...\r\n");
                 let parts = [crate::disc::install::gpt::GptPartitionSpec {
                    type_guid: crate::v::disc::partition::GPT_TYPE_LINUX_FILESYSTEM_BYTES,
                    name: "TRUEOS",
                    size: crate::disc::install::gpt::PartitionSize::Remaining,
                    attributes: 0,
                }];
                let mut log = |msg: &str| {
                    io.write_str(msg);
                    io.write_str("\r\n");
                };

                match crate::disc::install::gpt::write_gpt_layout_with_log(handle, &parts, &mut log).await {
                     Ok(_) => {
                         if let Ok(reg) = crate::v::disc::partition::register_gpt_partitions(handle).await {
                             if let Some(first) = reg.first() {
                                 if let Some(part_handle) = crate::disc::block::device_handle(first.id) {
                                     match crate::v::fs::trueosfs::format_blank_partition_async(part_handle).await {
                                         Ok(()) => {
                                             let (status, err) = crate::v::disc::detect::detect_physical_disk_detail(handle).await;
                                             io.write_fmt(format_args!(
                                                 "format: ok (status now: {}{})\r\n",
                                                 status.short(),
                                                 match (&status, err) {
                                                     (crate::v::disc::detect::DiscStatus::Unknown, Some(e)) => alloc::format!("; err={:?}", e),
                                                     _ => alloc::string::String::new(),
                                                 }
                                             ));
                                         }
                                         Err(e) => io.write_fmt(format_args!("format: TRUEOSFS failed ({:?})\r\n", e))                                     }
                                 }
                             }
                         }
                     }
                     Err(e) => io.write_fmt(format_args!("format: GPT write failed ({:?})\r\n", e))
                }
            } else {
                 io.write_str("\r\nformat: no such disk\r\n");
            }
            *mode = ShellMode::Idle;
        }
        CommandAction::DoInstall { disc_id } => {
            let target = crate::disc::block::device_handles()
                .into_iter()
                .find(|h| h.parent().is_none() && h.id().raw() == disc_id);
            if let Some(handle) = target {
                 if let (Some(kernel), Some(bootx64)) = (crate::limine::install_kernel_bytes(), crate::limine::install_bootx64_bytes()) {
                     io.write_str("\r\ninstall: starting...\r\n");
                     match crate::matrix::alloc_slot(alloc::format!("install disc{:03}", disc_id).as_str()) {
                        Some(slot) => {
                            let _ = spawner.spawn(crate::matrix::install_matrix_job(slot, handle, bootx64, kernel));
                            io.write_fmt(format_args!("install: started §{} (dump logs with §{})\r\n", slot + 1, slot + 1));
                            refresh_title_bar(io, *term_cols);
                        }
                        None => io.write_str("install: matrix full\r\n")
                     }
                 } else {
                     io.write_str("\r\ninstall: kernel or BOOTX64.EFI missing\r\n");
                 }
            } else {
                 io.write_str("\r\ninstall: no such disk\r\n");
            }
            *mode = ShellMode::Idle;
        }
        CommandAction::DoUpdate { disc_id } => {
             let target = crate::disc::block::device_handles()
                .into_iter()
                .find(|h| h.parent().is_none() && h.id().raw() == disc_id);
            if let Some(handle) = target {
                io.write_str("\r\nupdate: starting...\r\n");
                 match crate::matrix::alloc_slot(alloc::format!("update disc{:03}", disc_id).as_str()) {
                    Some(slot) => {
                        let _ = spawner.spawn(crate::matrix::update_matrix_job(slot, handle));
                        io.write_fmt(format_args!("update: started §{} (dump logs with §{})\r\n", slot + 1, slot + 1));
                        refresh_title_bar(io, *term_cols);
                    }
                    None => io.write_str("update: matrix full\r\n")
                 }
            } else {
                io.write_str("\r\nupdate: no such disk\r\n");
            }
            *mode = ShellMode::Idle;
        }
        CommandAction::NetbenchStarted => {
            *mode = ShellMode::Idle;
        }
        CommandAction::None => {}
    }
}
