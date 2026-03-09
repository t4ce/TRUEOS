use core::fmt::Write as _;

use alloc::vec::Vec;
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::String;

mod interface;
mod shell2_cmd;
mod shell2_qjs;
mod shell2_surf;
pub(crate) use interface::{ShellBackend2, ShellIo2};

const DEFAULT_PROMPT: &str = "§ ";
const MAX_LINE: usize = 192;
const LINE_WIDTH: usize = 100;
const BANNER_ROW: usize = 1;
const STATUS_ROW: usize = 2;
const PROMPT_ROW: usize = 3;
const SCROLL_TOP_ROW: usize = 4;

#[derive(Clone, Copy)]
enum LineSource {
    User,
    Shell,
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

    fn aligned_prefix(&self, source: LineSource, text: &str) {
        if matches!(source, LineSource::Shell) {
            let width = text.chars().count();
            let pad = LINE_WIDTH.saturating_sub(width);
            for _ in 0..pad {
                self.io.write_char(' ');
            }
        }
    }

    fn line_at(&self, row: usize, source: LineSource, s: &str) {
        self.move_to(row, 1);
        self.clear_line();
        self.aligned_prefix(source, s);
        self.io.write_str(s);
    }

    fn prompt(&self) {
        self.move_to(PROMPT_ROW, 1);
        self.clear_line();
        self.io.write_str("\x1b[0m");
        self.aligned_prefix(LineSource::User, DEFAULT_PROMPT);
        self.io.write_str(DEFAULT_PROMPT);
    }

    fn user_backspace(&self) {
        self.io.write_str("\x08 \x08");
    }

    fn user_char(&self, ch: char) {
        self.io.write_char(ch);
    }
}

fn clock_bucket_and_text() -> (u64, String<5>) {
    let secs = crate::time::unix_time_seconds().unwrap_or_else(crate::time::uptime_seconds);
    let mins_total = secs / 60;
    let mins_day = mins_total % (24 * 60);
    let hh = mins_day / 60;
    let mm = mins_day % 60;
    let mut text: String<5> = String::new();
    let _ = write!(text, "{:02}:{:02}", hh, mm);
    (mins_total, text)
}

#[embassy_executor::task(pool_size = 2)]
pub async fn task(_spawner: Spawner, io: &'static dyn ShellBackend2) {
    io.init();
    let out = AlignedWriter::new(io);

    out.clear_screen_home();
    out.reset_scroll_region();
    out.line_at(BANNER_ROW, LineSource::User, "TRUE OS §");

    let (mut last_minute_bucket, now_text) = clock_bucket_and_text();
    out.line_at(STATUS_ROW, LineSource::Shell, now_text.as_str());

    out.set_scroll_region(SCROLL_TOP_ROW);
    out.prompt();

    let mut line: String<MAX_LINE> = String::new();
    let mut history: Vec<alloc::string::String> = Vec::new();
    let mut saw_cr = false;

    loop {
        let (minute_bucket, minute_text) = clock_bucket_and_text();
        if minute_bucket != last_minute_bucket {
            last_minute_bucket = minute_bucket;
            out.line_at(STATUS_ROW, LineSource::Shell, minute_text.as_str());
            out.prompt();
        }

        if let Some(b) = io.read_byte() {
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

                        let parse = shell2_cmd::try_parse(submitted);
                        if !parse.handled() {
                            if let Some(url) = shell2_surf::try_parse(submitted) {
                                shell2_surf::prepare_call_with_url(url.as_str());
                            } else {
                                let _ = shell2_qjs::is_likely_valid(submitted);
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
