use alloc::vec::Vec;
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::String;

mod interface;
mod shell2_cmd;
mod shell2_qjs;
mod shell2_surf;
#[allow(unused_imports)]
pub(crate) use crate::shell::backends::{NET_TCP_SHELL_BACKEND, UART1_COM1_BACKEND};
pub(crate) use interface::{ShellBackend2, ShellIo2};

const DEFAULT_PROMPT: &str = "§ ";
const MAX_LINE: usize = 192;
const BANNER_ROW: usize = 1;
const STATUS_ROW: usize = 2;
const PROMPT_ROW: usize = 3;
const SCROLL_TOP_ROW: usize = 4;
const STATUS_SELECTED_RGB: (u8, u8, u8) = crate::shell::PROMPT_RGB;

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
        self.io
            .write_fmt(format_args!("\x1b[{};999r", top.max(1)));
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

    fn line_at(&self, row: usize, s: &str) {
        self.move_to(row, 1);
        self.clear_line();
        self.io.write_str(s);
    }

    fn mode_status(&self, mode: ShellMode2) {
        self.move_to(STATUS_ROW, 1);
        self.clear_line();
        self.write_mode_token("F1 surf", mode == ShellMode2::Surf);
        self.io.write_str(" - ");
        self.write_mode_token("ai", mode == ShellMode2::Ai);
        self.io.write_str(" - ");
        self.write_mode_token("qjs", mode == ShellMode2::Qjs);
        self.io.write_str(" - ");
        self.write_mode_token("cmd", mode == ShellMode2::Cmd);
        self.io.write_str(crate::ecma48::RESET);
    }

    fn write_mode_token(&self, text: &str, selected: bool) {
        if selected {
            self.io.write_fmt(format_args!(
                "{}",
                crate::ecma48::style(text).bold().fg(STATUS_SELECTED_RGB)
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
}

pub(crate) fn print_shell_line(io: &dyn ShellIo2, text: &str) {
    io.write_str(crate::ecma48::SAVE_CURSOR);
    io.write_fmt(format_args!("{}", crate::ecma48::pos(999, 1)));
    io.write_str(crate::ecma48::CLEAR_LINE);
    io.write_str(crate::ecma48::RESET);
    io.write_str(text);
    io.write_str("\r\n");
    io.write_str(crate::ecma48::RESTORE_CURSOR);
}

fn handle_submit(spawner: &Spawner, io: &'static dyn ShellBackend2, mode: ShellMode2, submitted: &str) {
    match mode {
        ShellMode2::Cmd => {
            let _ = shell2_cmd::try_parse(submitted);
        }
        ShellMode2::Surf => {
            if let Some(url) = shell2_surf::try_parse(submitted) {
                if shell2_surf::prepare_call_with_url(spawner, io, url.as_str()).is_err() {
                    print_shell_line(io, "surf: spawn failed");
                }
            }
        }
        ShellMode2::Qjs => {
            let _ = shell2_qjs::is_likely_valid(submitted);
        }
        ShellMode2::Ai => {
            // Reserved for the AI path; the mode bar and loop dispatch are in place first.
        }
    }
}

#[embassy_executor::task(pool_size = 2)]
 pub async fn task(spawner: Spawner, io: &'static dyn ShellBackend2) {
    io.init();
    let out = AlignedWriter::new(io);

    out.clear_screen_home();
    out.reset_scroll_region();
    out.line_at(BANNER_ROW, "TRUE OS §");
    let mut mode = ShellMode2::Cmd;
    out.mode_status(mode);

    out.set_scroll_region(SCROLL_TOP_ROW);
    out.prompt();

    let mut line: String<MAX_LINE> = String::new();
    let mut history: Vec<alloc::string::String> = Vec::new();
    let mut saw_cr = false;
    let mut esc = EscState::None;
    let mut csi_param: u16 = 0;

    loop {
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
                            mode = mode.next();
                            out.mode_status(mode);
                            out.prompt();
                            for ch in line.chars() {
                                out.user_char(ch);
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
                        mode = mode.next();
                        out.mode_status(mode);
                        out.prompt();
                        for ch in line.chars() {
                            out.user_char(ch);
                        }
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
                b'\r' | b'\n' => {
                    let submitted = line.as_str().trim();
                    if !submitted.is_empty() {
                        history.push(alloc::string::String::from(submitted));
                        handle_submit(&spawner, io, mode, submitted);
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
