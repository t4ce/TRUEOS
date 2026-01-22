use crate::ecma48;
use crate::shell::uart1_com1;

pub const CUBE_COLS: usize = 100;
pub const CUBE_ROWS: usize = 100;
const CUBE_SIZE: usize = CUBE_COLS * CUBE_ROWS;
const CUBE_SCALE: i32 = 1024;
const CUBE_DIST: i32 = CUBE_SCALE;
const CUBE_PINK: (u8, u8, u8) = (255, 55, 255);

pub struct CubeState {
    phase: u8,
    prev: [u8; CUBE_SIZE],
    prev_color: [u8; CUBE_SIZE],
}

impl CubeState {
    pub fn new() -> Self {
        Self {
            phase: 0,
            prev: [b' '; CUBE_SIZE],
            prev_color: [0u8; CUBE_SIZE],
        }
    }

    pub fn reset(&mut self) {
        self.phase = 0;
        for b in self.prev.iter_mut() {
            *b = b' ';
        }
        for c in self.prev_color.iter_mut() {
            *c = 0;
        }
    }

    pub fn draw_frame(&mut self) {
        draw_cube_frame(self.phase, &mut self.prev, &mut self.prev_color);
        self.phase = self.phase.wrapping_add(2);
    }
}

pub fn enter_mode() {
    uart1_com1::write_str(ecma48::CLEAR_SCREEN);
    uart1_com1::write_str(ecma48::HOME);
}

fn draw_cube_frame(phase: u8, prev: &mut [u8; CUBE_SIZE], prev_color: &mut [u8; CUBE_SIZE]) {
    let mut curr = [b' '; CUBE_SIZE];
    let mut curr_color = [0u8; CUBE_SIZE];

    let angle = (phase & 63) as usize;
    let angle2 = ((phase >> 1) & 63) as usize;
    let (sin_y, cos_y) = (SIN_LUT[angle], COS_LUT[angle]);
    let (sin_x, cos_x) = (SIN_LUT[angle2], COS_LUT[angle2]);

    let mut verts2d = [(0i32, 0i32); 8];
    let center_x = (CUBE_COLS as i32 - 1) / 2;
    let center_y = (CUBE_ROWS as i32 - 1) / 2 - (CUBE_ROWS as i32 / 4);
    for (i, (x, y, z)) in CUBE_VERTS.iter().copied().enumerate() {
        let x = x * CUBE_SCALE / 2;
        let y = y * CUBE_SCALE / 2;
        let z = z * CUBE_SCALE / 2;

        let x1 = (x * cos_y + z * sin_y) / CUBE_SCALE;
        let z1 = (-x * sin_y + z * cos_y) / CUBE_SCALE;
        let y1 = (y * cos_x - z1 * sin_x) / CUBE_SCALE;
        let z2 = (y * sin_x + z1 * cos_x) / CUBE_SCALE;

        let denom = (z2 + CUBE_DIST).max(CUBE_SCALE / 2);
        let px = (x1 * 40 / denom) + center_x;
        let py = (y1 * 40 / denom) + center_y;
        verts2d[i] = (px, py);
    }

    let edge_count = CUBE_EDGES.len().max(1) as u8;
    let edge_denom = if edge_count > 1 { edge_count as u16 - 1 } else { 1 };
    for (i, &(a, b)) in CUBE_EDGES.iter().enumerate() {
        let (x0, y0) = verts2d[a as usize];
        let (x1, y1) = verts2d[b as usize];
        let t = (i as u16 * 255 / edge_denom).min(255) as u8;
        draw_line(&mut curr, &mut curr_color, x0, y0, x1, y1, b'#', t);
    }

    for col in 0..CUBE_COLS {
        curr[col] = b'.';
        curr[(CUBE_ROWS - 1) * CUBE_COLS + col] = b'.';
    }
    for row in 0..CUBE_ROWS {
        curr[row * CUBE_COLS] = b'.';
        curr[row * CUBE_COLS + (CUBE_COLS - 1)] = b'.';
    }

    for idx in 0..CUBE_SIZE {
        let now = curr[idx];
        let color_now = curr_color[idx];
        if now != prev[idx] || (now == b'#' && color_now != prev_color[idx]) {
            let row = idx / CUBE_COLS;
            let col = idx % CUBE_COLS;
            write_pos(row + 1, col + 1);
            if now == b'#' {
                let (r, g, b) = line_rgb(color_now);
                uart1_com1::write_fmt(format_args!("{}", ecma48::color("§", (r, g, b))));
            } else if now == b'.' {
                uart1_com1::write_byte(b'.');
            } else {
                uart1_com1::write_byte(b' ');
            }
            prev[idx] = now;
            prev_color[idx] = color_now;
        }
    }
}

#[inline]
fn line_rgb(t: u8) -> (u8, u8, u8) {
    let g = 255u16 - ((255u16 - CUBE_PINK.1 as u16) * t as u16 / 255u16);
    (255, g as u8, 255)
}

fn draw_line(
    buf: &mut [u8; CUBE_SIZE],
    colors: &mut [u8; CUBE_SIZE],
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    ch: u8,
    color: u8,
) {
    let mut x0 = x0;
    let mut y0 = y0;
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        plot(buf, colors, x0, y0, ch, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

#[inline]
fn plot(buf: &mut [u8; CUBE_SIZE], colors: &mut [u8; CUBE_SIZE], x: i32, y: i32, ch: u8, color: u8) {
    if x < 0 || y < 0 {
        return;
    }
    let x = x as usize;
    let y = y as usize;
    if x >= CUBE_COLS || y >= CUBE_ROWS {
        return;
    }
    let idx = y * CUBE_COLS + x;
    buf[idx] = ch;
    colors[idx] = color;
}

#[inline]
fn write_pos(row: usize, col: usize) {
    uart1_com1::write_fmt(format_args!("{}", ecma48::pos(row, col)));
}

const CUBE_VERTS: [(i32, i32, i32); 8] = [
    (-1, -1, -1),
    (1, -1, -1),
    (1, 1, -1),
    (-1, 1, -1),
    (-1, -1, 1),
    (1, -1, 1),
    (1, 1, 1),
    (-1, 1, 1),
];

const CUBE_EDGES: [(u8, u8); 12] = [
    (0, 1),
    (1, 2),
    (2, 3),
    (3, 0),
    (4, 5),
    (5, 6),
    (6, 7),
    (7, 4),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

const SIN_LUT: [i32; 64] = [
    0, 100, 200, 297, 392, 483, 569, 650,
    724, 792, 851, 904, 946, 979, 1004, 1019,
    1024, 1019, 1004, 979, 946, 904, 851, 792,
    724, 650, 569, 483, 392, 297, 200, 100,
    0, -100, -200, -297, -392, -483, -569, -650,
    -724, -792, -851, -904, -946, -979, -1004, -1019,
    -1024, -1019, -1004, -979, -946, -904, -851, -792,
    -724, -650, -569, -483, -392, -297, -200, -100,
];

const COS_LUT: [i32; 64] = [
    1024, 1019, 1004, 979, 946, 904, 851, 792,
    724, 650, 569, 483, 392, 297, 200, 100,
    0, -100, -200, -297, -392, -483, -569, -650,
    -724, -792, -851, -904, -946, -979, -1004, -1019,
    -1024, -1019, -1004, -979, -946, -904, -851, -792,
    -724, -650, -569, -483, -392, -297, -200, -100,
    0, 100, 200, 297, 392, 483, 569, 650,
    724, 792, 851, 904, 946, 979, 1004, 1019,
];
