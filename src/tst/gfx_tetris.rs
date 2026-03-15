use alloc::vec::Vec;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use trueos_tetris::{Game, Lcg32, NoopEvents};

const UI2_TETRIS_TEX_ID: u32 = 4_701;
const UI2_TETRIS_WINDOW_X: f32 = 640.0;
const UI2_TETRIS_WINDOW_Y: f32 = 110.0;
const UI2_TETRIS_WINDOW_Z: i16 = 32;

const BOARD_W: usize = 20;
const BOARD_H: usize = 48;
const BOARD_HIDDEN: usize = 8;
const VIEW_H: usize = BOARD_H - BOARD_HIDDEN;

const CELL_PX: u32 = 8;
const FRAME_PX: u32 = 2;
const PAD_X: u32 = 14;
const PAD_Y: u32 = 14;
const HEADER_H: u32 = 18;

const UI2_TETRIS_RT_W: u32 = PAD_X * 2 + FRAME_PX * 2 + (BOARD_W as u32 * CELL_PX);
const UI2_TETRIS_RT_H: u32 = HEADER_H + PAD_Y * 2 + FRAME_PX * 2 + (VIEW_H as u32 * CELL_PX);

const BG_COLOR: [u8; 4] = [0x09, 0x10, 0x16, 0xFF];
const HEADER_COLOR: [u8; 4] = [0x11, 0x1B, 0x24, 0xFF];
const BOARD_BG_COLOR: [u8; 4] = [0x05, 0x08, 0x0C, 0xFF];
const FRAME_COLOR: [u8; 4] = [0x3A, 0x4C, 0x5F, 0xFF];
const GRID_COLOR: [u8; 4] = [0x0D, 0x13, 0x1B, 0xFF];
const GAME_OVER_TINT: [u8; 4] = [0x2A, 0x08, 0x08, 0xFF];
const GAME_OVER_RESET_MS: u32 = 1_500;
const BG_CLEAR_RGB: u32 = 0x091016;

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct RgbVertex {
    x: f32,
    y: f32,
    r: u8,
    g: u8,
    b: u8,
    pad: u8,
}

struct GfxTetrisApp {
    game: Game<BOARD_W, BOARD_H, BOARD_HIDDEN>,
    rng: Lcg32,
    events: NoopEvents,
    drop_accum_ms: u32,
    game_over_accum_ms: u32,
}

impl GfxTetrisApp {
    fn new(seed: u32) -> Self {
        let mut rng = Lcg32::new(seed.max(1));
        let mut events = NoopEvents;
        let game = Game::new(&mut rng, &mut events);
        Self {
            game,
            rng,
            events,
            drop_accum_ms: 0,
            game_over_accum_ms: 0,
        }
    }

    fn reset(&mut self) {
        self.game = Game::new(&mut self.rng, &mut self.events);
        self.drop_accum_ms = 0;
        self.game_over_accum_ms = 0;
    }

    fn tick(&mut self, elapsed_ms: u32) -> bool {
        if self.game.is_game_over() {
            self.game_over_accum_ms = self.game_over_accum_ms.saturating_add(elapsed_ms);
            if self.game_over_accum_ms >= GAME_OVER_RESET_MS {
                self.reset();
                return true;
            }
            return false;
        }

        self.drop_accum_ms = self.drop_accum_ms.saturating_add(elapsed_ms);
        let step_ms = self.game.level.level_speed_seconds().max(1);
        let mut changed = false;

        while self.drop_accum_ms >= step_ms {
            self.drop_accum_ms -= step_ms;
            let _ = self.game.soft_drop(&mut self.rng, &mut self.events);
            changed = true;
            if self.game.is_game_over() {
                self.game_over_accum_ms = 0;
                break;
            }
        }

        changed
    }
}

fn push_rect(vertices: &mut Vec<RgbVertex>, x: u32, y: u32, w: u32, h: u32, color: [u8; 4]) {
    if w == 0 || h == 0 {
        return;
    }
    let x0 = (x as f32 / UI2_TETRIS_RT_W as f32) * 2.0 - 1.0;
    let y0 = (y as f32 / UI2_TETRIS_RT_H as f32) * 2.0 - 1.0;
    let x1 = ((x + w) as f32 / UI2_TETRIS_RT_W as f32) * 2.0 - 1.0;
    let y1 = ((y + h) as f32 / UI2_TETRIS_RT_H as f32) * 2.0 - 1.0;
    let mk = |x: f32, y: f32| RgbVertex {
        x,
        y,
        r: color[0],
        g: color[1],
        b: color[2],
        pad: color[3],
    };
    vertices.extend_from_slice(&[
        mk(x0, y0),
        mk(x1, y0),
        mk(x1, y1),
        mk(x0, y0),
        mk(x1, y1),
        mk(x0, y1),
    ]);
}

fn build_frame_vertices(app: &GfxTetrisApp) -> Vec<RgbVertex> {
    let mut vertices = Vec::with_capacity(6 * (3 + VIEW_H + BOARD_W + (BOARD_W * VIEW_H)));
    push_rect(&mut vertices, 0, 0, UI2_TETRIS_RT_W, HEADER_H, HEADER_COLOR);

    let board_x = PAD_X;
    let board_y = HEADER_H + PAD_Y;
    let board_w = FRAME_PX * 2 + (BOARD_W as u32 * CELL_PX);
    let board_h = FRAME_PX * 2 + (VIEW_H as u32 * CELL_PX);

    push_rect(&mut vertices, board_x, board_y, board_w, board_h, FRAME_COLOR);
    push_rect(
        &mut vertices,
        board_x + FRAME_PX,
        board_y + FRAME_PX,
        board_w - FRAME_PX * 2,
        board_h - FRAME_PX * 2,
        if app.game.is_game_over() {
            GAME_OVER_TINT
        } else {
            BOARD_BG_COLOR
        },
    );

    for y in 0..VIEW_H {
        let py = board_y + FRAME_PX + (y as u32 * CELL_PX);
        push_rect(
            &mut vertices,
            board_x + FRAME_PX,
            py,
            BOARD_W as u32 * CELL_PX,
            1,
            GRID_COLOR,
        );
    }
    for x in 0..BOARD_W {
        let px = board_x + FRAME_PX + (x as u32 * CELL_PX);
        push_rect(
            &mut vertices,
            px,
            board_y + FRAME_PX,
            1,
            VIEW_H as u32 * CELL_PX,
            GRID_COLOR,
        );
    }

    for view_y in 0..VIEW_H {
        let board_cell_y = BOARD_HIDDEN + view_y;
        for x in 0..BOARD_W {
            let Some(cell) = app.game.cell_view_at(x, board_cell_y, true) else {
                continue;
            };
            let mut rgba = [cell.color.r, cell.color.g, cell.color.b, 0xFF];
            match cell.layer {
                trueos_tetris::Layer::Placed => {}
                trueos_tetris::Layer::Current => {
                    rgba[0] = brighten(rgba[0], 28);
                    rgba[1] = brighten(rgba[1], 28);
                    rgba[2] = brighten(rgba[2], 28);
                }
                trueos_tetris::Layer::Ghost => {
                    rgba[0] = dim(rgba[0], 35);
                    rgba[1] = dim(rgba[1], 35);
                    rgba[2] = dim(rgba[2], 35);
                }
            }

            let px = board_x + FRAME_PX + (x as u32 * CELL_PX);
            let py = board_y + FRAME_PX + (view_y as u32 * CELL_PX);
            let inset = if matches!(cell.layer, trueos_tetris::Layer::Ghost) {
                3
            } else {
                1
            };
            let side = CELL_PX.saturating_sub(inset * 2).max(1);
            push_rect(&mut vertices, px + inset, py + inset, side, side, rgba);
        }
    }

    vertices
}

fn brighten(channel: u8, amount: u8) -> u8 {
    channel.saturating_add(amount)
}

fn dim(channel: u8, percent: u8) -> u8 {
    let keep = 100_u16.saturating_sub(percent as u16);
    ((channel as u16 * keep) / 100) as u8
}

#[embassy_executor::task]
pub async fn ui2_gfx_tetris_task() {
    let seed = crate::time::unix_time_seconds()
        .map(|t| t as u32)
        .unwrap_or(0x5445_5452)
        ^ 0xA11C_E123;
    let mut app = GfxTetrisApp::new(seed);
    let Some(surface) = crate::v::ui2::Ui2SurfaceWindow::new(
        "Gfx Tetris",
        crate::v::ui2::Ui2Rect {
            x: UI2_TETRIS_WINDOW_X,
            y: UI2_TETRIS_WINDOW_Y,
            w: UI2_TETRIS_RT_W as f32,
            h: UI2_TETRIS_RT_H as f32,
        },
        UI2_TETRIS_WINDOW_Z,
        255,
        UI2_TETRIS_TEX_ID,
        false,
        BG_COLOR,
    ) else {
        return;
    };
    let window_id = surface.window_id();
    let (surface_w, surface_h) = surface.size();
    crate::log!(
        "gfx-tetris: window={} tex={} size={}x{}\n",
        window_id,
        surface.tex_id(),
        surface_w,
        surface_h
    );
    let init_vertices = build_frame_vertices(&app);
    let init_bytes = unsafe {
        core::slice::from_raw_parts(
            init_vertices.as_ptr() as *const u8,
            init_vertices.len() * core::mem::size_of::<RgbVertex>(),
        )
    };
    let _ = surface.render_rgb_triangles(BG_CLEAR_RGB, init_bytes, "gfx-tetris-init");

    let mut last_tick = Instant::now();
    loop {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(last_tick);
        last_tick = now;

        let elapsed_ms = elapsed.as_millis() as u32;
        let changed = app.tick(elapsed_ms) || app.game.consume_changed();
        if changed {
            let vertices = build_frame_vertices(&app);
            let bytes = unsafe {
                core::slice::from_raw_parts(
                    vertices.as_ptr() as *const u8,
                    vertices.len() * core::mem::size_of::<RgbVertex>(),
                )
            };
            let _ = surface.render_rgb_triangles(BG_CLEAR_RGB, bytes, "gfx-tetris");
        }

        Timer::after(EmbassyDuration::from_millis(16)).await;
    }
}
