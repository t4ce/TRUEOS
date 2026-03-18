use trueos_v::vled::Rgb8;

use crate::{Game, Lcg32, RandomSource, Rotation, TetrisEvents};

const BOARD_W: usize = 12;
const BOARD_H: usize = 28;
const BOARD_HIDDEN: usize = 4;
const VIEW_H: usize = BOARD_H - BOARD_HIDDEN;
const STATS_WIDTH: usize = 20;
const MAX_CLEARED_ROW_OVERLAYS: usize = 4;

pub trait ShellIo {
    fn write_str(&self, s: &str);
    fn write_fmt(&self, args: core::fmt::Arguments<'_>);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellControl {
    Continue,
    Exit,
}

#[derive(Clone, Copy)]
struct ClearedRowOverlay {
    visible_row: usize,
    colors: [Option<Rgb8>; BOARD_W],
}

struct ShellEvents {
    cleared_rows: [Option<ClearedRowOverlay>; MAX_CLEARED_ROW_OVERLAYS],
}

impl ShellEvents {
    const fn new() -> Self {
        Self {
            cleared_rows: [None; MAX_CLEARED_ROW_OVERLAYS],
        }
    }

    fn clear_overlays(&mut self) {
        self.cleared_rows = [None; MAX_CLEARED_ROW_OVERLAYS];
    }

    fn overlay_color_at(&self, visible_row: usize, x: usize) -> Option<Rgb8> {
        self.cleared_rows
            .iter()
            .flatten()
            .find(|overlay| overlay.visible_row == visible_row)
            .and_then(|overlay| overlay.colors.get(x).copied().flatten())
    }
}

impl TetrisEvents for ShellEvents {
    fn on_block_placed(&mut self, _color: Rgb8, _x: usize, _y: usize) {
        self.clear_overlays();
    }

    fn on_row_deleted(&mut self, row: usize, colors: &[Option<Rgb8>]) {
        if row < BOARD_HIDDEN {
            return;
        }

        let mut snapshot = [None; BOARD_W];
        for (idx, color) in colors.iter().copied().take(BOARD_W).enumerate() {
            snapshot[idx] = color;
        }

        let overlay = ClearedRowOverlay {
            visible_row: row - BOARD_HIDDEN,
            colors: snapshot,
        };

        if let Some(slot) = self.cleared_rows.iter_mut().find(|entry| entry.is_none()) {
            *slot = Some(overlay);
        } else {
            self.cleared_rows[MAX_CLEARED_ROW_OVERLAYS - 1] = Some(overlay);
        }
    }
}

pub struct ShellApp {
    game: Game<BOARD_W, BOARD_H, BOARD_HIDDEN>,
    rng: Lcg32,
    events: ShellEvents,
    cols: usize,
    rows: usize,
    viewport_top_row: usize,
    esc_state: u8,
    saw_cr: bool,
    paused: bool,
    redraw: bool,
    drop_accum_ms: u32,
    prev_cells: [[Option<crate::CellView>; VIEW_H]; BOARD_W],
    prev_level: u8,
    prev_rows: u32,
    prev_points: u32,
    prev_paused: bool,
    prev_game_over: bool,
    prev_valid: bool,
}

impl ShellApp {
    pub fn new(seed: u32, cols: usize, rows: usize) -> Self {
        let mut rng = Lcg32::new(seed.max(1));
        let mut events = ShellEvents::new();
        let game = Game::new(&mut rng, &mut events);

        Self {
            game,
            rng,
            events,
            cols: cols.max(24),
            rows: rows.max(14),
            viewport_top_row: 1,
            esc_state: 0,
            saw_cr: false,
            paused: false,
            redraw: true,
            drop_accum_ms: 0,
            prev_cells: [[None; VIEW_H]; BOARD_W],
            prev_level: 0,
            prev_rows: 0,
            prev_points: 0,
            prev_paused: false,
            prev_game_over: false,
            prev_valid: false,
        }
    }

    pub fn set_terminal_size(&mut self, cols: usize, rows: usize) {
        self.cols = cols.max(24);
        self.rows = rows.max(14);
        self.redraw = true;
        self.prev_valid = false;
    }

    pub fn set_viewport_top_row(&mut self, top_row: usize) {
        self.viewport_top_row = top_row.max(1);
        self.redraw = true;
        self.prev_valid = false;
    }

    pub fn consume_redraw(&mut self) -> bool {
        let out = self.redraw || self.game.consume_changed();
        self.redraw = false;
        out
    }

    pub fn handle_input_byte(&mut self, b: u8) -> ShellControl {
        if self.saw_cr && b == b'\n' {
            self.saw_cr = false;
            return ShellControl::Continue;
        }
        self.saw_cr = b == b'\r';

        if self.esc_state == 1 {
            if b == b'[' {
                self.esc_state = 2;
            } else {
                self.esc_state = 0;
            }
            return ShellControl::Continue;
        }

        if self.esc_state == 2 {
            self.esc_state = 0;
            match b {
                b'A' => {
                    self.game.rotate(Rotation::Cw);
                    self.redraw = true;
                }
                b'B' => {
                    self.game.rotate(Rotation::Ccw);
                    self.redraw = true;
                }
                b'C' => {
                    self.game.move_right();
                    self.redraw = true;
                }
                b'D' => {
                    self.game.move_left();
                    self.redraw = true;
                }
                _ => {}
            }
            return ShellControl::Continue;
        }

        match b {
            0x1B => {
                self.esc_state = 1;
            }
            b'q' | b'Q' | 0x03 | 0x04 => {
                return ShellControl::Exit;
            }
            b'r' | b'R' => {
                self.reset_game();
                self.redraw = true;
            }
            b'p' | b'P' => {
                self.paused = !self.paused;
                self.redraw = true;
            }
            b'a' | b'h' | b'H' => {
                self.game.move_left();
                self.redraw = true;
            }
            b'd' | b'l' | b'L' => {
                self.game.move_right();
                self.redraw = true;
            }
            b's' | b'j' | b'J' => {
                let _ = self.game.soft_drop(&mut self.rng, &mut self.events);
                self.redraw = true;
            }
            b'w' | b'k' | b'K' => {
                self.game.rotate(Rotation::Cw);
                self.redraw = true;
            }
            b'z' | b'Z' => {
                self.game.rotate(Rotation::Ccw);
                self.redraw = true;
            }
            b' ' => {
                let _ = self.game.hard_drop(&mut self.rng, &mut self.events);
                self.redraw = true;
            }
            _ => {}
        }

        ShellControl::Continue
    }

    pub fn tick(&mut self, elapsed_ms: u32) {
        if self.paused || self.game.is_game_over() {
            return;
        }

        self.drop_accum_ms = self.drop_accum_ms.saturating_add(elapsed_ms);
        let step_ms = self.game.level.level_speed_seconds();

        while self.drop_accum_ms >= step_ms {
            self.drop_accum_ms -= step_ms;
            let _ = self.game.soft_drop(&mut self.rng, &mut self.events);
            self.redraw = true;
            if self.game.is_game_over() {
                break;
            }
        }
    }

    pub fn draw(&self, io: &dyn ShellIo) {
        let board_inner_cols = BOARD_W * 2;
        let board_rows = self.game.visible_height() + 2;
        let panel_cols = 1 + board_inner_cols + 1 + STATS_WIDTH + 1;

        let start_col = (self.cols.saturating_sub(panel_cols) / 2).max(1);
        let start_row = (self.rows.saturating_sub(board_rows) / 2).max(1);
        let divider_col = start_col + 1 + board_inner_cols;
        let stats_col = divider_col + 2;
        let right_col = start_col + panel_cols - 1;

        io.write_str("\x1b[?25l");

        if !self.prev_valid {
            io.write_fmt(format_args!("\x1b[{};1H\x1b[J", self.viewport_top_row));
            self.draw_top_border(io, start_row, start_col, board_inner_cols, STATS_WIDTH);

            for view_y in 0..self.game.visible_height() {
                let row = start_row + 1 + view_y;
                self.write_at(io, row, start_col, "│");
                self.write_at(io, row, divider_col, "│");
                self.write_at(io, row, right_col, "│");
                self.write_at(io, row, divider_col + 1, "                    ");
            }

            self.draw_bottom_border(
                io,
                start_row + 1 + self.game.visible_height(),
                start_col,
                board_inner_cols,
                STATS_WIDTH,
            );
        }

        for view_y in 0..self.game.visible_height() {
            let row = start_row + 1 + view_y;
            let board_y = BOARD_HIDDEN + view_y;
            for x in 0..BOARD_W {
                if let Some(color) = self.events.overlay_color_at(view_y, x) {
                    let col = start_col + 1 + x * 2;
                    self.write_at_fmt(
                        io,
                        row,
                        col,
                        format_args!(
                            "\x1b[5;1;38;2;{};{};{}m██\x1b[0m",
                            color.r, color.g, color.b
                        ),
                    );
                    continue;
                }

                let current = self.game.cell_view_at(x, board_y, true);
                let prev = self.prev_cells[x][view_y];
                if !self.prev_valid || current != prev {
                    let col = start_col + 1 + x * 2;
                    match current {
                        Some(cell) => {
                            let r = cell.color.r;
                            let g = cell.color.g;
                            let b = cell.color.b;
                            match cell.layer {
                                crate::Layer::Ghost => {
                                    self.write_at_fmt(
                                        io,
                                        row,
                                        col,
                                        format_args!("\x1b[38;2;{};{};{}m░░\x1b[0m", r, g, b),
                                    );
                                }
                                _ => {
                                    self.write_at_fmt(
                                        io,
                                        row,
                                        col,
                                        format_args!("\x1b[48;2;{};{};{}m  \x1b[0m", r, g, b),
                                    );
                                }
                            }
                        }
                        None => self.write_at(io, row, col, "  "),
                    }
                }
            }
        }

        let stats_row = start_row + 1;
        if !self.prev_valid || self.prev_level != self.game.level.current_level {
            self.write_status_line(
                io,
                stats_row,
                stats_col,
                format_args!("level   {}", self.game.level.current_level),
            );
        }
        if !self.prev_valid || self.prev_rows != self.game.level.rows_deleted {
            self.write_status_line(
                io,
                stats_row + 1,
                stats_col,
                format_args!("rows    {}", self.game.level.rows_deleted),
            );
        }
        if !self.prev_valid || self.prev_points != self.game.level.total_points {
            self.write_status_line(
                io,
                stats_row + 2,
                stats_col,
                format_args!("points  {}", self.game.level.total_points),
            );
        }

        if !self.prev_valid
            || self.prev_paused != self.paused
            || self.prev_game_over != self.game.is_game_over()
        {
            self.clear_status_line(io, stats_row + 4, stats_col);
            self.clear_status_line(io, stats_row + 5, stats_col);
            if self.paused {
                self.write_at(io, stats_row + 4, stats_col, "[PAUSED]");
            } else if self.game.is_game_over() {
                self.write_at(io, stats_row + 4, stats_col, "[GAME OVER]");
                self.write_at(io, stats_row + 5, stats_col, "press R to restart");
            }
        }
    }

    fn draw_top_border(
        &self,
        io: &dyn ShellIo,
        row: usize,
        start_col: usize,
        board_inner_cols: usize,
        stats_width: usize,
    ) {
        self.write_at(io, row, start_col, "┌");
        for idx in 0..board_inner_cols {
            self.write_at(io, row, start_col + 1 + idx, "─");
        }
        self.write_at(io, row, start_col + 1 + board_inner_cols, "┬");
        for idx in 0..stats_width {
            self.write_at(io, row, start_col + 2 + board_inner_cols + idx, "─");
        }
        self.write_at(io, row, start_col + 2 + board_inner_cols + stats_width, "┐");
    }

    fn draw_bottom_border(
        &self,
        io: &dyn ShellIo,
        row: usize,
        start_col: usize,
        board_inner_cols: usize,
        stats_width: usize,
    ) {
        self.write_at(io, row, start_col, "└");
        for idx in 0..board_inner_cols {
            self.write_at(io, row, start_col + 1 + idx, "─");
        }
        self.write_at(io, row, start_col + 1 + board_inner_cols, "┴");
        for idx in 0..stats_width {
            self.write_at(io, row, start_col + 2 + board_inner_cols + idx, "─");
        }
        self.write_at(io, row, start_col + 2 + board_inner_cols + stats_width, "┘");
    }

    fn reset_game(&mut self) {
        let seed = self.rng.next_u32().max(1);
        self.rng = Lcg32::new(seed);
        self.events.clear_overlays();
        self.game = Game::new(&mut self.rng, &mut self.events);
        self.paused = false;
        self.drop_accum_ms = 0;
        self.prev_valid = false;
    }

    pub fn finalize_frame(&mut self) {
        for view_y in 0..self.game.visible_height() {
            let board_y = BOARD_HIDDEN + view_y;
            for x in 0..BOARD_W {
                self.prev_cells[x][view_y] = self.game.cell_view_at(x, board_y, true);
            }
        }
        self.prev_level = self.game.level.current_level;
        self.prev_rows = self.game.level.rows_deleted;
        self.prev_points = self.game.level.total_points;
        self.prev_paused = self.paused;
        self.prev_game_over = self.game.is_game_over();
        self.prev_valid = true;
    }

    #[inline]
    fn write_at(&self, io: &dyn ShellIo, row: usize, col: usize, text: &str) {
        let abs_row = self.viewport_top_row.saturating_add(row.max(1)).saturating_sub(1);
        io.write_fmt(format_args!("\x1b[{};{}H", abs_row, col.max(1)));
        io.write_str(text);
    }

    #[inline]
    fn write_at_fmt(
        &self,
        io: &dyn ShellIo,
        row: usize,
        col: usize,
        args: core::fmt::Arguments<'_>,
    ) {
        let abs_row = self.viewport_top_row.saturating_add(row.max(1)).saturating_sub(1);
        io.write_fmt(format_args!("\x1b[{};{}H", abs_row, col.max(1)));
        io.write_fmt(args);
    }

    #[inline]
    fn clear_status_line(&self, io: &dyn ShellIo, row: usize, col: usize) {
        self.write_at(io, row, col, "                   ");
    }

    #[inline]
    fn write_status_line(
        &self,
        io: &dyn ShellIo,
        row: usize,
        col: usize,
        args: core::fmt::Arguments<'_>,
    ) {
        self.clear_status_line(io, row, col);
        self.write_at_fmt(io, row, col, args);
    }
}
