use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};

use crate::shell::shellcube::{CubeState, WireShape, CUBE_COLS, CUBE_ROWS};

pub(crate) mod ecma48;

pub(crate) mod shellcube;
pub(crate) mod shellqjs;
pub(crate) mod txtedt;

pub(crate) mod cmd;
pub(crate) mod cmdreg;

pub(crate) mod matrix;
pub(crate) mod statusbar;
pub(crate) mod bench;


mod crlf;

mod interface;
pub(crate) use interface::{ShellBackend, ShellIo};

pub(crate) mod backends;
pub(crate) use backends::{NET_TCP_SHELL_BACKEND, UART1_COM1_BACKEND};

pub(crate) mod uart1_com1;

struct Utf8Decoder {
    buf: [u8; 4],
    len: usize,
    need: usize,
}

impl Utf8Decoder {
    const fn new() -> Self {
        Self {
            buf: [0u8; 4],
            len: 0,
            need: 0,
        }
    }

    fn clear(&mut self) {
        self.len = 0;
        self.need = 0;
    }

    fn push(&mut self, b: u8) -> Option<char> {
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
    io: &dyn ShellIo,
    line: &mut String<128>,
    term_cols: usize,
    term_rows: usize,
    pending_action: Option<PendingAction>,
    install_wizard: Option<InstallWizardStage>,
) {
    if pending_action.is_some() || install_wizard.is_some() {
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
            if crate::shell::cmdreg::usage_text_for_name(cmd, &mut usage) {
                write_overlay_hint(io, term_cols, term_rows, usage.as_str());
            }
        }
        return;
    }

    let mut cmds: heapless::Vec<&'static str, 64> = heapless::Vec::new();
    crate::shell::cmdreg::list_command_names(&mut cmds);
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
        if crate::shell::cmdreg::usage_text_for_name(target, &mut usage) {
            write_overlay_hint(io, term_cols, term_rows, usage.as_str());
        } else {
            write_overlay_hint(io, term_cols, term_rows, "");
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
    write_overlay_hint(io, term_cols, term_rows, msg.as_str());
}


#[inline]
fn write_overlay_hint(io: &dyn ShellIo, term_cols: usize, term_rows: usize, text: &str) {
    if term_cols == 0 || term_rows < 3 {
        return;
    }

    if text.is_empty() {
        return;
    }

    // SCROLLBAR_GLYPH is defined later, so we use the literal here to avoid ordering issues.
    // Width reserved: 1 (bar) + 1 (space) = 2.
    let reserved_cols = 2usize;
    let max_text_cols = term_cols.saturating_sub(reserved_cols);

    let mut clipped: String<256> = String::new();
    let mut cols = 0usize;
    for ch in text.chars() {
        if cols >= max_text_cols {
            break;
        }
        if clipped.push(ch).is_err() {
            break;
        }
        cols += 1;
    }

    io.write_str(crate::ecma48::SAVE_CURSOR);
    io.write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));
    io.write_str("\x1b[L"); 
    // Manual scrollbar draw for this injected line
    io.write_str("\x1b[38;2;80;80;80m│\x1b[0m ");
    io.write_str(clipped.as_str());
    io.write_str(crate::ecma48::RESTORE_CURSOR);
}




struct PrependingShellIo<'a> {
    inner: &'a dyn ShellBackend,
    term_cols: usize,
    line_buf: core::cell::RefCell<alloc::string::String>,
}

impl<'a> PrependingShellIo<'a> {
    fn new(inner: &'a dyn ShellBackend, term_cols: usize) -> Self {
        Self { 
            inner, 
            term_cols,
            line_buf: core::cell::RefCell::new(alloc::string::String::new()),
        }
    }

    fn do_write(&self, s: &str) {
        let mut buf = self.line_buf.borrow_mut();
        
        if !s.contains('\n') {
            buf.push_str(s);
            return;
        }

        let parts: alloc::vec::Vec<&str> = s.split('\n').collect();
        for (i, part) in parts.iter().enumerate() {
            if i == 0 {
                buf.push_str(part);
                self.flush_line(&buf);
                buf.clear();
            } else if i == parts.len() - 1 {
                buf.push_str(part);
            } else {
                self.flush_line(part);
            }
        }
    }
    
    fn flush_line(&self, s: &str) {
        // Output System Message (Right Aligned)
        self.inner.write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));
        self.inner.write_str("\x1b[L");
        self.inner.write_str(SCROLLBAR_GLYPH);
        self.inner.write_str(" ");

        let content_len = s.chars().count();
        let max_width = self.term_cols.saturating_sub(2);
        let padding = max_width.saturating_sub(content_len);
        
        for _ in 0..padding {
            self.inner.write_str(" ");
        }
        // Clock color: (120, 210, 255)
        self.inner.write_str("\x1b[38;2;120;210;255m");
        self.inner.write_str(s);
        self.inner.write_str("\x1b[0m");
    }
}

impl<'a> Drop for PrependingShellIo<'a> {
    fn drop(&mut self) {
        let buf = self.line_buf.borrow();
        if !buf.is_empty() {
             self.flush_line(&buf);
        }
    }
}

impl ShellIo for PrependingShellIo<'_> {
    fn write_str(&self, s: &str) {
        self.do_write(s);
    }

    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        use core::fmt::Write;
        struct Adapter<'a>(&'a PrependingShellIo<'a>);
        impl<'a> Write for Adapter<'a> {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                self.0.write_str(s);
                Ok(())
            }
        }
        let _ = Adapter(self).write_fmt(args);
    }

    fn write_char(&self, ch: char) {
        let mut b = [0u8; 4];
        self.write_str(ch.encode_utf8(&mut b));
    }

    fn write_byte(&self, b: u8) {
        if b == b'\n' {
            self.write_str("\n");
        } else {
            // Very inefficient for byte streams but necessary for buffering logic
             let mut buf = [0u8; 1];
             buf[0] = b;
             if let Ok(s) = core::str::from_utf8(&buf) {
                 self.write_str(s);
             }
        }
    }
}

impl ShellBackend for PrependingShellIo<'_> {
    fn read_byte(&self) -> Option<u8> {
        self.inner.read_byte()
    }
    fn init(&self) {
        self.inner.init()
    }
}

#[inline]
fn output_bottom_row(term_rows: usize) -> usize {
    core::cmp::max(3, term_rows.saturating_sub(1))
}

#[inline]
pub(crate) fn apply_shell_scroll_region(io: &dyn ShellIo, term_rows: usize) {
    let top = 3usize;
    let bottom = output_bottom_row(term_rows);
    // HIDE cursor to prevent "jump to 1,1" artifact during DECSTBM.
    io.write_str(crate::ecma48::HIDE_CURSOR);
    io.write_fmt(format_args!("\x1b[{};{}r", top, bottom));
    
    // Draw dummy scrollbar static indicator for the entire scroll region.
    // This fills Column 1 with the scrollbar glyph from top to bottom of the region.
    // We use a dimmed color.
    io.write_str("\x1b[38;2;80;80;80m"); // Dim Gray
    for r in top..=bottom {
        io.write_fmt(format_args!("{}", crate::ecma48::pos(r, 1)));
        io.write_str("│");
    }
    io.write_str("\x1b[0m"); // Reset

    // Immediately force cursor to (3,3) (Top of region, +2 margin for scrollbar + space)
    // We set cursor to 3,1 for internal logic, but effectively we want text to start after the bar.
    // However, typical shell operations assume (row, 1). To fake the margin, we just need to rely on PrependingShellIo adding it,
    // OR we shift the cursor to 3.
    // Let's stick to 3,1 but let inserted lines handle the margin.
    // Wait, if we just drew a bar at 3,1, putting current cursor at 3,1 and writing will Overwrite it.
    // So if the cursor is at 3,1, we are on top of the bar.
    // We should move to 3,3 (1 char bar + 1 char space + 1).
    io.write_fmt(format_args!("{}", crate::ecma48::pos(3, 3)));
    
    io.write_str(crate::ecma48::SHOW_CURSOR);
}

#[inline]
fn append_output_cursor(io: &dyn ShellIo, term_rows: usize) {
    // Legacy helper - no longer used in Reverse Shell mode?
    // If used, ensure safe cursor.
    let row = output_bottom_row(term_rows);
    io.write_fmt(format_args!("{}", crate::ecma48::pos(row, 1)));
    io.write_str("\r\n");
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

    let right_col = term_cols.saturating_sub(10).saturating_add(1);
    let left_col = 1usize;

    io.write_str(crate::ecma48::HIDE_CURSOR);
    io.write_str(crate::ecma48::SAVE_CURSOR);
    io.write_fmt(format_args!("{}", crate::ecma48::pos(term_rows, 1)));

    // White background for status bar
    let bar_bg = (255, 255, 255);
    io.write_fmt(format_args!("\x1b[48;2;{};{};{}m", bar_bg.0, bar_bg.1, bar_bg.2));
    for _ in 0..term_cols {
        io.write_byte(b' ');
    }
    io.write_str(crate::ecma48::RESET);

    io.write_fmt(format_args!("{}", crate::ecma48::pos(term_rows, left_col)));
    for c in indicators {
        // Adjust indicator color 7 (white) to be dark so it's visible on white bg
        let fg = if c == 7 { (0, 0, 0) } else { indicator_rgb(c) };
        io.write_fmt(format_args!("{}", crate::ecma48::style("o").fg(fg).bg(bar_bg)));
    }
    io.write_fmt(format_args!("{}", crate::ecma48::style(" ").bg(bar_bg)));
    
    // Left text: dark grey on white
    io.write_fmt(format_args!("{}", crate::ecma48::style(left.as_str()).dim().fg((50, 50, 50)).bg(bar_bg)));

    io.write_fmt(format_args!("{}", crate::ecma48::pos(term_rows, right_col)));
    // Right text: darker pink on white
    io.write_fmt(format_args!("{}", crate::ecma48::style(right.as_str()).bold().fg((200, 50, 150)).bg(bar_bg)));

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
    Mv { src: String<160>, dst: String<160> },
    Qjs { src: String<192> },
}





#[embassy_executor::task(pool_size = 3)]
pub async fn task(spawner: Spawner, io: &'static dyn ShellBackend) {
    io.init();

    // Ensure the registry is populated before the shell starts.
    self::cmdreg::init_builtin_shell_commands();

    let mut term_cols: usize = DEFAULT_TERM_COLS;
    let mut term_rows: usize = DEFAULT_TERM_ROWS;

    write_banner(io, term_cols);
    apply_shell_scroll_region(io, term_rows);
    write_prompt(io);

    let mut line: String<128> = String::new();
    let mut utf8 = Utf8Decoder::new();
    let mut next_matrix_refresh: Instant = Instant::now() + EmbassyDuration::from_millis(60000);
    let mut pending_action: Option<PendingAction> = None;
    let mut pending_deadline: Option<Instant> = None;
    let mut install_wizard: Option<InstallWizardStage> = None;
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
            match b {
                b'\r' | b'\n' | b' ' if matches!(pending_action, Some(PendingAction::AcpiReset | PendingAction::AcpiState(_))) => {
                    utf8.clear();
                    // Other pending actions: Enter/Space cancels.
                    pending_action = None;
                    pending_deadline = None;
                    line.clear();
                    io.write_str("\r\n");
                    write_prompt_for_state(io, pending_action, install_wizard);
                    continue;
                }
                b if matches!(pending_action, Some(PendingAction::FormatConfirm { .. })) && b != b'\r' && b != b'\n' => {
                    // Destructive format confirmation: Enter confirms, any other key cancels.
                    utf8.clear();
                    pending_action = None;
                    pending_deadline = None;
                    line.clear();
                    io.write_str("\r\nformat: cancelled\r\n");
                    write_prompt_for_state(io, pending_action, install_wizard);
                    continue;
                }
                b if matches!(pending_action, Some(PendingAction::InstallConfirm { .. })) && b != b'\r' && b != b'\n' => {
                    // Destructive install confirmation: Enter confirms, any other key cancels.
                    utf8.clear();
                    pending_action = None;
                    pending_deadline = None;
                    line.clear();
                    io.write_str("\r\ninstall: cancelled\r\n");
                    write_prompt_for_state(io, pending_action, install_wizard);
                    continue;
                }
                b'\r' | b'\n' => {
                    utf8.clear();

                    // Confirmation gate for destructive `format`.
                    if let Some(PendingAction::FormatConfirm { disc_id }) = pending_action {
                        let do_format = line.is_empty();
                        line.clear();
                        pending_action = None;
                        pending_deadline = None;

                        if do_format {
                            let target = crate::disc::block::device_handles()
                                .into_iter()
                                .find(|h| h.parent().is_none() && h.id().raw() == disc_id);
                            let Some(handle) = target else {
                                io.write_str("\r\nformat: no such disk\r\n");
                                write_prompt(io);
                                continue;
                            };

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

                            let gpt_result = crate::disc::install::gpt::write_gpt_layout_with_log(
                                handle,
                                &parts,
                                &mut log,
                            )
                            .await;

                            match gpt_result {
                                Ok(_ranges) => {
                                    match crate::v::disc::partition::register_gpt_partitions(handle).await {
                                        Ok(reg) => {
                                            let Some(first) = reg.first() else {
                                                io.write_str("format: no partitions registered\r\n");
                                                write_prompt(io);
                                                continue;
                                            };

                                            let Some(part_handle) = crate::disc::block::device_handle(first.id) else {
                                                io.write_str("format: partition handle lookup failed\r\n");
                                                write_prompt(io);
                                                continue;
                                            };

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
                                                Err(e) => {
                                                    io.write_fmt(format_args!("format: TRUEOSFS failed ({:?})\r\n", e));
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            io.write_fmt(format_args!("format: partition register failed ({:?})\r\n", e));
                                        }
                                    }
                                }
                                Err(e) => {
                                    io.write_fmt(format_args!("format: GPT write failed ({:?})\r\n", e));
                                }
                            }
                        } else {
                            io.write_str("\r\nformat: cancelled\r\n");
                        }

                        write_prompt_for_state(io, pending_action, install_wizard);
                        continue;
                    }

                    // Confirmation gate for destructive `install`.
                    if let Some(PendingAction::InstallConfirm { disc_id }) = pending_action {
                        let do_install = line.is_empty();
                        line.clear();
                        pending_action = None;
                        pending_deadline = None;

                        if do_install {
                            let target = crate::disc::block::device_handles()
                                .into_iter()
                                .find(|h| h.parent().is_none() && h.id().raw() == disc_id);
                            let Some(handle) = target else {
                                io.write_str("\r\ninstall: no such disk\r\n");
                                write_prompt(io);
                                continue;
                            };

                            let Some(kernel) = crate::limine::install_kernel_bytes() else {
                                io.write_str("\r\ninstall: kernel file missing\r\n");
                                io.write_str(
                                    "install: expected Limine to provide the kernel file\r\n",
                                );
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };

                            let Some(bootx64) = crate::limine::install_bootx64_bytes() else {
                                io.write_str("\r\ninstall: BOOTX64.EFI module missing\r\n");
                                io.write_str(
                                    "install: expected Limine module_string trueos.install.bootx64\r\n",
                                );
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };

                            io.write_str("\r\ninstall: starting...\r\n");
                            match crate::matrix::alloc_slot(alloc::format!("install disc{:03}", disc_id).as_str()) {
                                Some(slot) => {
                                    let _ = spawner.spawn(crate::matrix::install_matrix_job(
                                            slot,
                                            handle,
                                            bootx64,
                                            kernel,
                                        ),
                                    );
                                    io.write_fmt(format_args!(
                                        "install: started §{} (dump logs with § {})\r\n",
                                        slot + 1,
                                        slot + 1
                                    ));
                                    refresh_title_bar(io, term_cols);
                                }
                                None => {
                                    io.write_str("install: matrix full\r\n");
                                }
                            }
                        } else {
                            io.write_str("\r\ninstall: cancelled\r\n");
                        }

                        write_prompt_for_state(io, pending_action, install_wizard);
                        continue;
                    }

                    // Confirmation gate for `update` (network fetch + refresh installed boot files).
                    if let Some(PendingAction::UpdateConfirm { disc_id }) = pending_action {
                        let do_update = line.is_empty();
                        line.clear();
                        pending_action = None;
                        pending_deadline = None;

                        if do_update {
                            let target = crate::disc::block::device_handles()
                                .into_iter()
                                .find(|h| h.parent().is_none() && h.id().raw() == disc_id);
                            let Some(handle) = target else {
                                io.write_str("\r\nupdate: no such disk\r\n");
                                write_prompt(io);
                                continue;
                            };

                            io.write_str("\r\nupdate: starting...\r\n");
                            match crate::matrix::alloc_slot(alloc::format!("update disc{:03}", disc_id).as_str()) {
                                Some(slot) => {
                                    let _ = spawner.spawn(crate::matrix::update_matrix_job(slot, handle),
                                    );
                                    io.write_fmt(format_args!(
                                        "update: started §{} (dump logs with § {})\r\n",
                                        slot + 1,
                                        slot + 1
                                    ));
                                    refresh_title_bar(io, term_cols);
                                }
                                None => {
                                    io.write_str("update: matrix full\r\n");
                                }
                            }
                        } else {
                            io.write_str("\r\nupdate: cancelled\r\n");
                        }

                        write_prompt_for_state(io, pending_action, install_wizard);
                        continue;
                    }

                    // Interactive install wizard consumes whole-line input (including empty line).
                    if pending_action.is_none() {
                        if let Some(InstallWizardStage::SelectDisk) = install_wizard {
                            let mut s = line.as_str().trim();
                            // Accept inputs like `install 1` as well as just `1`.
                            if let Some(rest) = s.strip_prefix("install") {
                                s = rest.trim();
                            }
                            if s.is_empty() || s.eq_ignore_ascii_case("q") || s.eq_ignore_ascii_case("quit") {
                                line.clear();
                                install_wizard = None;
                                io.write_str("\r\ninstall: cancelled\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let raw_id = parse_disc_id_raw(s).unwrap_or(0);
                            line.clear();
                            if raw_id == 0 {
                                io.write_str("\r\ninstall: invalid id\r\n");
                                io.write_str("install: enter a disk id (e.g. 1 or disc001) or 'q'\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let target = crate::disc::block::device_handles()
                                .into_iter()
                                .find(|h| h.parent().is_none() && h.id().raw() == raw_id);
                            let Some(handle) = target else {
                                io.write_str("\r\ninstall: no such disk\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };

                            let info = handle.info();
                            let status = crate::v::disc::detect::detect_physical_disk(handle).await;
                            io.write_fmt(format_args!(
                                "\r\ninstall: target id={} ({}) blocks={} bs={} writable={} label={:?} status={}\r\n",
                                info.id.raw(),
                                info.id,
                                info.block_count,
                                info.block_size,
                                info.writable,
                                info.label,
                                status.short(),
                            ));
                            io.write_str("install: DANGER: this may REPARTITION and FORMAT the disk\r\n");
                            io.write_str("install: press Enter to confirm (any other key cancels)\r\n");

                            install_wizard = None;
                            pending_action = Some(PendingAction::InstallConfirm { disc_id: raw_id });
                            pending_deadline = None;
                            write_prompt_for_state(io, pending_action, install_wizard);
                            continue;
                        }

                        if let Some(InstallWizardStage::UpdateSelectDisk) = install_wizard {
                            let mut s = line.as_str().trim();
                            // Accept inputs like `update 1` as well as just `1`.
                            if let Some(rest) = s.strip_prefix("update") {
                                s = rest.trim();
                            }
                            if s.is_empty() || s.eq_ignore_ascii_case("q") || s.eq_ignore_ascii_case("quit") {
                                line.clear();
                                install_wizard = None;
                                io.write_str("\r\nupdate: cancelled\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let raw_id = parse_disc_id_raw(s).unwrap_or(0);
                            line.clear();
                            if raw_id == 0 {
                                io.write_str("\r\nupdate: invalid id\r\n");
                                io.write_str("update: enter a disk id (e.g. 1 or disc001) or 'q'\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let target = crate::disc::block::device_handles()
                                .into_iter()
                                .find(|h| h.parent().is_none() && h.id().raw() == raw_id);
                            let Some(handle) = target else {
                                io.write_str("\r\nupdate: no such disk\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };

                            let info = handle.info();
                            let status = crate::v::disc::detect::detect_physical_disk(handle).await;
                            io.write_fmt(format_args!(
                                "\r\nupdate: target id={} ({}) blocks={} bs={} writable={} label={:?} status={}\r\n",
                                info.id.raw(),
                                info.id,
                                info.block_count,
                                info.block_size,
                                info.writable,
                                info.label,
                                status.short(),
                            ));

                            // Safety: `update` is intended to refresh boot files on an already-installed TRUEOS disk.
                            // Refuse to proceed if the disk doesn't already look like TRUEOSFS.
                            if !matches!(status, crate::v::disc::detect::DiscStatus::Trueos { .. }) {
                                io.write_str("update: refused (selected disk is not a TRUEOS disk)\r\n");
                                io.write_str("update: use `install` for a fresh install\r\n");
                                io.write_str("update: choose another disk id (or 'q' to cancel)\r\n");
                                print_update_disk_table(io).await;
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            io.write_str("update: downloads BOOTX64.EFI + TRUEOS.elf and refreshes ESP boot files\r\n");
                            io.write_str("update: will NOT repartition/format (refuses if TRUEOSFS is not detected)\r\n");
                            io.write_str("update: press Enter to confirm (any other key cancels)\r\n");

                            install_wizard = None;
                            pending_action = Some(PendingAction::UpdateConfirm { disc_id: raw_id });
                            pending_deadline = None;
                            write_prompt_for_state(io, pending_action, install_wizard);
                            continue;
                        }

                        if let Some(InstallWizardStage::FormatSelectDisk) = install_wizard {
                            let mut s = line.as_str().trim();
                            // Accept inputs like `format 1` as well as just `1`.
                            if let Some(rest) = s.strip_prefix("format") {
                                s = rest.trim();
                            }
                            if s.is_empty() || s.eq_ignore_ascii_case("q") || s.eq_ignore_ascii_case("quit") {
                                line.clear();
                                install_wizard = None;
                                io.write_str("\r\nformat: cancelled\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let raw_id = parse_disc_id_raw(s).unwrap_or(0);
                            line.clear();
                            if raw_id == 0 {
                                io.write_str("\r\nformat: invalid id\r\n");
                                io.write_str("format: enter a disk id (e.g. 1 or disc001) or 'q'\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let target = crate::disc::block::device_handles()
                                .into_iter()
                                .find(|h| h.parent().is_none() && h.id().raw() == raw_id);
                            let Some(handle) = target else {
                                io.write_str("\r\nformat: no such disk\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };

                            let info = handle.info();
                            let status = crate::v::disc::detect::detect_physical_disk(handle).await;
                            io.write_fmt(format_args!(
                                "\r\nformat: target id={} ({}) blocks={} bs={} writable={} label={:?} status={}\r\n",
                                info.id.raw(),
                                info.id,
                                info.block_count,
                                info.block_size,
                                info.writable,
                                info.label,
                                status.short(),
                            ));
                            io.write_str("format: DANGER: this destroys all data on the disk\r\n");
                            io.write_str("format: press Enter to confirm (any other key cancels)\r\n");

                            install_wizard = None;
                            pending_action = Some(PendingAction::FormatConfirm { disc_id: raw_id });
                            pending_deadline = None;
                            write_prompt_for_state(io, pending_action, install_wizard);
                            continue;
                        }

                        if let Some(InstallWizardStage::FileSelectMount) = install_wizard {
                            let mut s = line.as_str().trim();
                            // Accept inputs like `file 0` as well as just `0`.
                            if let Some(rest) = s.strip_prefix("file") {
                                s = rest.trim();
                            }

                            if s.is_empty() || s.eq_ignore_ascii_case("q") || s.eq_ignore_ascii_case("quit") {
                                line.clear();
                                install_wizard = None;
                                io.write_str("\r\nfile: cancelled\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            if s.eq_ignore_ascii_case("ls") || s.eq_ignore_ascii_case("list") {
                                line.clear();
                                print_trueosfs_mount_table(io).await;
                                io.write_str("file: enter mount index or disk id (blank/q cancels)\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let roots = crate::v::fs::trueosfs::list_roots();

                            // Prefer the mount table index when it is in range.
                            let (handle, shown_id) = if let Ok(idx) = s.parse::<usize>() {
                                if let Some(r) = roots.get(idx) {
                                    (
                                        crate::disc::block::device_handle(r.disk_id),
                                        Some(r.disk_id.raw()),
                                    )
                                } else {
                                    (None, None)
                                }
                            } else {
                                let raw_id = parse_disc_id_raw(s).unwrap_or(0);
                                if raw_id == 0 {
                                    (None, None)
                                } else {
                                    let target = crate::disc::block::device_handles()
                                        .into_iter()
                                        .find(|h| h.parent().is_none() && h.id().raw() == raw_id);
                                    (target, Some(raw_id))
                                }
                            };

                            line.clear();

                            let Some(handle) = handle else {
                                io.write_str("\r\nfile: invalid mount id (try 'ls')\r\n");
                                io.write_str("file: enter mount index or disk id (blank/q cancels)\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };

                            io.write_fmt(format_args!(
                                "\r\nfile: printing tree for {} (raw={})\r\n",
                                handle.id(),
                                shown_id.unwrap_or(handle.id().raw())
                            ));

                            print_trueosfs_tree_25(io, handle).await;
                            io.write_str("\r\nfile: enter another mount index/id, 'ls', or 'q'\r\n");
                            write_prompt_for_state(io, pending_action, install_wizard);
                            continue;
                        }

                        if let Some(InstallWizardStage::BenchSelectDisk) = install_wizard {
                            let mut s = line.as_str().trim();
                            if let Some(rest) = s.strip_prefix("bench") {
                                s = rest.trim();
                            }
                            if s.is_empty() || s.eq_ignore_ascii_case("q") || s.eq_ignore_ascii_case("quit") {
                                line.clear();
                                install_wizard = None;
                                io.write_str("\r\nbench: cancelled\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let raw_id = parse_disc_id_raw(s).unwrap_or(0);
                            line.clear();
                            if raw_id == 0 {
                                io.write_str("\r\nbench: invalid id\r\n");
                                io.write_str("bench: enter a TRUEOSFS disk id or 'q'\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let target = crate::disc::block::device_handles()
                                .into_iter()
                                .find(|h| h.parent().is_none() && h.id().raw() == raw_id);
                            let Some(handle) = target else {
                                io.write_str("\r\nbench: no such disk\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };

                            let (status, err) = crate::v::disc::detect::detect_physical_disk_detail(handle).await;
                            if !matches!(status, crate::v::disc::detect::DiscStatus::Trueos { .. }) {
                                io.write_fmt(format_args!(
                                    "\r\nbench: refused (id={} is not TRUEOSFS; status={}{} )\r\n",
                                    raw_id,
                                    status.short(),
                                    match (&status, err) {
                                        (crate::v::disc::detect::DiscStatus::Unknown, Some(e)) => alloc::format!(" err={:?}", e),
                                        _ => alloc::string::String::new(),
                                    }
                                ));
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            install_wizard = None;
                            run_bench_fs(io, handle).await;
                            write_prompt_for_state(io, pending_action, install_wizard);
                            continue;
                        }

                        if let Some(InstallWizardStage::NetbenchSelectNic) = install_wizard {
                            let mut s = line.as_str().trim();
                            if let Some(rest) = s.strip_prefix("netbench") {
                                s = rest.trim();
                            }
                            if s.is_empty() || s.eq_ignore_ascii_case("q") || s.eq_ignore_ascii_case("quit") {
                                line.clear();
                                install_wizard = None;
                                io.write_str("\r\nnetbench: cancelled\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            let nic_index = s.parse::<usize>().ok();
                            line.clear();
                            let Some(nic_index) = nic_index else {
                                io.write_str("\r\nnetbench: invalid nic id\r\n");
                                io.write_str("netbench: enter a nic id or 'q'\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            };
                            if nic_index >= crate::net::device_count() {
                                io.write_str("\r\nnetbench: no such nic\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                                continue;
                            }

                            install_wizard = None;
                            if netbench_start(&spawner, nic_index) {
                                let slot = NETBENCH_STATUS_SLOT.load(Ordering::Relaxed);
                                io.write_fmt(format_args!(
                                    "\r\nnetbench: started nic={} §{} url={}\r\n",
                                    nic_index,
                                    slot.saturating_add(1),
                                    NETBENCH_URL
                                ));
                                let _ = crate::shell::statusbar::set_left_active("netbench");
                                let _ = crate::shell::statusbar::set_right_active("0kb/s");
                                for i in 0..crate::shell::statusbar::INDICATOR_COUNT {
                                    let _ = crate::shell::statusbar::set_indicator_active(i, 2);
                                }
                                next_netbench_update = Instant::now() + EmbassyDuration::from_millis(NETBENCH_UPDATE_MS);
                                last_netbench_state = NETBENCH_RUNNING;
                                write_prompt_for_state(io, pending_action, install_wizard);
                            } else {
                                io.write_str("\r\nnetbench: already running\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            continue;
                        }
                    }

                    if line.is_empty() && pending_action.is_none() {
                        // Empty command: do nothing (no newline, no prompt).
                        continue;
                    }
                    if !line.is_empty() {
                        // Enter uses the same prefix matching behavior as Tab:
                        // unique -> expand and execute, ambiguous -> show matches and stay in-place.
                        if pending_action.is_none() {
                            let mut input_buf: String<128> = String::new();
                            let _ = input_buf.push_str(line.as_str());
                            let input = input_buf.as_str();
                            if !input.chars().any(|ch| ch.is_whitespace()) {
                                let mut cmds: heapless::Vec<&'static str, 64> = heapless::Vec::new();
                                crate::shell::cmdreg::list_command_names(&mut cmds);
                                let mut matches = 0usize;
                                let mut unique: Option<&'static str> = None;
                                for name in cmds.iter().copied() {
                                    if starts_with_ignore_ascii_case(name, input) {
                                        matches += 1;
                                        if unique.is_none() {
                                            unique = Some(name);
                                        }
                                    }
                                }
                                if matches > 1 {
                                    utf8.clear();
                                    handle_tab_completion(
                                        io,
                                        &mut line,
                                        term_cols,
                                        term_rows,
                                        pending_action,
                                        install_wizard,
                                    );
                                    continue;
                                }
                                if matches == 1 {
                                    if let Some(full) = unique {
                                        if !full.eq_ignore_ascii_case(input) {
                                            line.clear();
                                            let _ = line.push_str(full);
                                        }
                                    }
                                }
                            }
                        }

                        // append_output_cursor(io, term_rows);
                        let action = handle_line(
                            &line,
                            &spawner,
                            io,
                            &mut term_cols,
                            &mut term_rows,
                            &mut install_wizard,
                        );
                        let cmd_echo = line.clone();
                        line.clear();

                        echo_command(io, cmd_echo.trim(), term_cols);

                        match action {
                            CommandAction::Pending(action) => {
                                pending_action = Some(action);
                                pending_deadline = match action {
                                    PendingAction::AcpiReset |
                                    PendingAction::AcpiState(_) => {
                                        Some(Instant::now() + EmbassyDuration::from_secs(5))
                                    }
                                    PendingAction::FormatConfirm { .. } => None,
                                    PendingAction::InstallConfirm { .. } => None,
                                    PendingAction::UpdateConfirm { .. } => None,
                                };
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::ShowInstallDiskTable => {
                                print_install_disk_table(io).await;
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::ShowFormatDiskTable => {
                                print_format_disk_table(io).await;
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::ShowUpdateDiskTable => {
                                print_update_disk_table(io).await;
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::ShowFileMountTable => {
                                print_trueosfs_mount_table(io).await;
                                io.write_str("file: enter mount index or disk id (blank/q cancels)\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::ShowBenchDiskTable => {
                                print_bench_disk_table(io).await;
                                io.write_str("bench: enter TRUEOSFS disk id (blank/q cancels)\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::ShowNetbenchNicTable => {
                                print_netbench_nic_table(io).await;
                                io.write_str("netbench: enter nic id (blank/q cancels)\r\n");
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::Mv { src, dst } => {
                                match crate::surface::io::kfs::rename_async(src.as_str(), dst.as_str()).await {
                                    Ok(()) => io.write_str("mv: ok\r\n"),
                                    Err(e) => io.write_fmt(format_args!("mv: failed ({:?})\r\n", e)),
                                }
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::Qjs { src } => {
                                if trueos_qjs::async_fs::ensure_service_started(&spawner) {
                                } else {
                                    io.write_str("qjs: async fs service unavailable\r\n");
                                    write_prompt_for_state(io, pending_action, install_wizard);
                                    continue;
                                }
                                crate::shell::shellqjs::run(io, src.as_str()).await;
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                            CommandAction::EnterCube => {
                                cube_mode = true;
                                cube.set_shape(WireShape::Cube);
                                cube.reset();
                                enter_cube_mode(io, &mut term_cols, &mut term_rows);
                            }
                            CommandAction::EnterIco => {
                                cube_mode = true;
                                cube.set_shape(WireShape::Icosidodecahedron);
                                cube.reset();
                                enter_cube_mode(io, &mut term_cols, &mut term_rows);
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
                                write_prompt(io);
                            }
                            CommandAction::EnterTxtEdt { filename, slot_id } => {
                                cube_mode = false;
                                let cols = term_cols;
                                let rows = term_rows;

                                // Edit the slot blob in-place (no auto-capture into a new slot).
                                let Some(buf) = crate::matrix::take_blob(slot_id) else {
                                    io.write_str("\r\ntxt: invalid slot\r\n");
                                    io.write_str(crate::ecma48::CLEAR_SCREEN);
                                    io.write_str(crate::ecma48::HOME);
                                    write_banner(io, term_cols);
                                    apply_shell_scroll_region(io, term_rows);
                                    write_prompt(io);
                                    continue;
                                };

                                crate::matrix::set_state(slot_id, crate::matrix::SlotState::Running);
                                let out_buf = crate::shell::txtedt::run(io, cols, rows, filename.as_str(), buf).await;
                                let _ = crate::matrix::set_blob_owned_with_preview(slot_id, out_buf);
                                crate::matrix::set_state(slot_id, crate::matrix::SlotState::Done);
                                io.write_fmt(format_args!("\r\ntxt: updated §{}\r\n", slot_id + 1));
                                refresh_title_bar(io, term_cols);

                                io.write_str(crate::ecma48::CLEAR_SCREEN);
                                io.write_str(crate::ecma48::HOME);
                                write_banner(io, term_cols);
                                apply_shell_scroll_region(io, term_rows);
                                write_prompt(io);
                            }
                            CommandAction::None => {
                                write_prompt_for_state(io, pending_action, install_wizard);
                            }
                        }
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
                    utf8.clear();
                    line.clear();
                    io.write_str("^C\r\n");
                    write_prompt(io);
                }
                b'\t' => {
                    utf8.clear();
                    handle_tab_completion(
                        io,
                        &mut line,
                        term_cols,
                        term_rows,
                        pending_action,
                        install_wizard,
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

            /*
            // Keep header symbols in sync with background job state transitions.
            if Instant::now() >= next_matrix_refresh {
                refresh_title_bar(io, term_cols);
                next_matrix_refresh = Instant::now() + EmbassyDuration::from_millis(60000);
            }
            */

            if let (Some(action), Some(deadline)) = (pending_action, pending_deadline) {
                if Instant::now() >= deadline {
                    pending_action = None;
                    pending_deadline = None;
                    match action {
                        PendingAction::AcpiReset => {
                            if crate::efi::acpi::facp::reset_system().is_err() {
                                io.write_str("\r\nacpi reset failed\r\n");
                                write_prompt(io);
                            }
                        }
                        PendingAction::AcpiState(level) => {
                            if crate::efi::acpi::facp::enter_named_sleep_state(level).is_err() {
                                io.write_fmt(format_args!("\r\nacpi s{} failed\r\n", level));
                                write_prompt(io);
                            }
                        }
                        PendingAction::FormatConfirm { .. } => {}
                        PendingAction::InstallConfirm { .. } => {}
                        PendingAction::UpdateConfirm { .. } => {}
                    }
                    continue;
                }
            }
            Timer::after(EmbassyDuration::from_millis(2)).await;
        }
    }
}

const SCROLLBAR_GLYPH: &str = "\x1b[38;2;80;80;80m│\x1b[0m";

fn echo_command(io: &dyn ShellIo, cmd: &str, term_cols: usize) {
    if cmd.is_empty() { return; }
    io.write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));
    io.write_str("\x1b[L");
    io.write_str(SCROLLBAR_GLYPH);
    io.write_str(" \x1b[37m"); // White
    io.write_str(cmd);
    io.write_str("\x1b[0m");
}

fn handle_line(
    line: &str,
    spawner: &Spawner,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
    install_wizard: &mut Option<InstallWizardStage>,
) -> CommandAction {
    let cmd = line.trim();
    if cmd.is_empty() {
        return CommandAction::None;
    }

    // Ensure scrolling region is set correctly (e.g. if term resized).
    apply_shell_scroll_region(io, *term_rows);

    let p_io = PrependingShellIo::new(io, *term_cols);

    // New-style registered commands (typed args + validation + introspection).
    {
        let mut ctx = crate::shell::cmdreg::ShellCommandCtx {
            line: cmd,
            spawner,
            io: &p_io,
            term_cols,
            term_rows,
            install_wizard,
        };
        if let Some(action) = crate::shell::cmdreg::dispatch_line(&mut ctx) {
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
    shellcube::enter_mode(io);
}
