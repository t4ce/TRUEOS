use core::str::SplitWhitespace;

use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

use super::super::{ShellBackend2, ShellIo2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

pub(crate) const SHELL2_TETRIS_ROWS: usize = 27;
const STATUS_ROW: usize = 2;
const TETRIS_STATUS_CONTROLS: &[(&str, &str)] = &[
    ("A/D", "move"),
    ("W", "rotate"),
    ("Space", "drop"),
    ("P", "pause"),
    ("R", "restart"),
    ("Q", "exit"),
];

struct IoAdapter<'a> {
    io: &'a dyn ShellIo2,
}

impl trueos_tetris::shell::ShellIo for IoAdapter<'_> {
    fn write_str(&self, s: &str) {
        self.io.write_str(s);
    }

    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        self.io.write_fmt(args);
    }
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    if args.next().is_some() {
        print_shell_line(io, "tetris: usage `tetris`");
        return ParseOutcome::Handled;
    }

    ParseOutcome::LaunchTetris
}

pub(crate) fn is_launch_request(submitted: &str) -> bool {
    let mut args = submitted.split_whitespace();
    match (args.next(), args.next()) {
        (Some(cmd), None) => cmd.eq_ignore_ascii_case("tetris"),
        _ => false,
    }
}

fn tetris_status_text() -> alloc::string::String {
    let mut out = alloc::string::String::new();
    for (idx, (key, action)) in TETRIS_STATUS_CONTROLS.iter().enumerate() {
        if idx != 0 {
            out.push_str("  ");
        }
        let styled = alloc::format!(
            "{}",
            super::super::ecma48::style(*key).bold().fg((255, 255, 255))
        );
        out.push_str(styled.as_str());
        out.push(' ');
        out.push_str(action);
    }
    out
}

fn draw_status_controls(io: &'static dyn ShellBackend2, cols: usize) {
    let text = tetris_status_text();
    let width = crate::shell2::ecma48::visible_width(text.as_str());
    let col = cols
        .saturating_sub(width)
        .checked_div(2)
        .unwrap_or(0)
        .saturating_add(1);
    io.write_fmt(format_args!("\x1b[{};1H\x1b[2K", STATUS_ROW));
    io.write_fmt(format_args!("\x1b[{};{}H", STATUS_ROW, col.max(1)));
    io.write_str(text.as_str());
    io.write_str(super::super::ecma48::RESET);
}

pub(crate) async fn run(io: &'static dyn ShellBackend2, cols: usize, rows: usize, top_row: usize) {
    let seed = crate::time::unix_time_seconds()
        .map(|t| t as u32)
        .unwrap_or(0x5445_5452)
        ^ 0xC0DE_CAFE;

    let mut app = trueos_tetris::shell::ShellApp::new(seed, cols, rows);
    app.set_terminal_size(cols, rows);
    app.set_viewport_top_row(top_row);
    draw_status_controls(io, cols);

    let adapter = IoAdapter { io };
    app.draw(&adapter);
    app.finalize_frame();

    let mut last_tick = Instant::now();

    loop {
        if let Some(b) = io.read_byte() {
            match app.handle_input_byte(b) {
                trueos_tetris::shell::ShellControl::Continue => {}
                trueos_tetris::shell::ShellControl::Exit => break,
            }
        }

        let now = Instant::now();
        let elapsed = now.saturating_duration_since(last_tick);
        last_tick = now;

        let elapsed_ms = elapsed.as_millis() as u32;
        if elapsed_ms > 0 {
            app.tick(elapsed_ms);
        }

        if app.consume_redraw() {
            app.draw(&adapter);
            app.finalize_frame();
        }

        Timer::after(EmbassyDuration::from_millis(16)).await;
    }

    io.write_str(super::super::ecma48::SHOW_CURSOR);
}
