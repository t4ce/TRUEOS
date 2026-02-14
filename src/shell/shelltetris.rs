use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

use crate::shell::ShellBackend;

struct IoAdapter<'a> {
    io: &'a dyn ShellBackend,
}

impl trueos_tetris::shell::ShellIo for IoAdapter<'_> {
    fn write_str(&self, s: &str) {
        self.io.write_str(s);
    }

    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        self.io.write_fmt(args);
    }
}

pub async fn run(io: &'static dyn ShellBackend, cols: usize, rows: usize) {
    let seed = crate::time::unix_time_seconds()
        .map(|t| t as u32)
        .unwrap_or(0x5445_5452)
        ^ 0xC0DE_CAFE;

    let mut app = trueos_tetris::shell::ShellApp::new(seed, cols, rows);
    app.set_terminal_size(cols, rows);

    let adapter = IoAdapter { io };
    app.draw(&adapter);

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
        }

        Timer::after(EmbassyDuration::from_millis(16)).await;
    }

    io.write_str(crate::ecma48::SHOW_CURSOR);
    io.write_str("\r\n");
}
