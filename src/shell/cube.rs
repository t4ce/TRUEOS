use crate::ecma48;
use crate::shell::interface::ShellIo;

pub const CUBE_COLS: usize = 75;
pub const CUBE_ROWS: usize = 75;
const CUBE_SIZE: usize = CUBE_COLS * CUBE_ROWS;
const CUBE_SCALE: i32 = 700;
const CUBE_DIST: i32 = CUBE_SCALE * 1.8 as i32;
const CUBE_PINK: (u8, u8, u8) = (255, 55, 255);
const BORDER_INSET: usize = 0;
const DRAW_SIZE: usize = 65;
const DRAW_Y_SHIFT: isize = -(DRAW_SIZE as isize / 2);
const CENTER_Y_OFFSET: i32 = 10; // because i cant math
const MAX_VERTS: usize = 30;
const MAX_EDGES: usize = 60;

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum WireShape {
    Cube,
    Icosidodecahedron,
}

pub struct CubeState {
    phase: u8,
    prev: [u8; CUBE_SIZE],
    prev_color: [u8; CUBE_SIZE],
    shape: WireShape,
    verts: [(i32, i32, i32); MAX_VERTS],
    vert_count: usize,
    edges: [(u8, u8); MAX_EDGES],
    edge_count: usize,
}

impl CubeState {
    pub fn new() -> Self {
        let mut state = Self {
            phase: 0,
            prev: [b' '; CUBE_SIZE],
            prev_color: [0u8; CUBE_SIZE],
            shape: WireShape::Cube,
            verts: [(0, 0, 0); MAX_VERTS],
            vert_count: 0,
            edges: [(0, 0); MAX_EDGES],
            edge_count: 0,
        };
        state.set_shape(WireShape::Cube);
        state
    }

    pub fn set_shape(&mut self, shape: WireShape) {
        self.shape = shape;
        match shape {
            WireShape::Cube => {
                for (i, v) in CUBE_VERTS.iter().copied().enumerate() {
                    self.verts[i] = v;
                }
                self.vert_count = CUBE_VERTS.len();
                for (i, e) in CUBE_EDGES.iter().copied().enumerate() {
                    self.edges[i] = e;
                }
                self.edge_count = CUBE_EDGES.len();
            }
            WireShape::Icosidodecahedron => {
                build_icosidodecahedron(
                    &mut self.verts,
                    &mut self.vert_count,
                    &mut self.edges,
                    &mut self.edge_count,
                );
            }
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

    pub fn draw_frame(&mut self, io: &dyn ShellIo) {
        draw_poly_frame(
            io,
            self.phase,
            self.shape,
            &self.verts,
            self.vert_count,
            &self.edges,
            self.edge_count,
            &mut self.prev,
            &mut self.prev_color,
        );
        self.phase = self.phase.wrapping_add(2);
    }
}

pub fn enter_mode(io: &dyn ShellIo) {
    io.write_str(ecma48::CLEAR_SCREEN);
    io.write_str(ecma48::HOME);
}

fn draw_poly_frame(
    io: &dyn ShellIo,
    phase: u8,
    shape: WireShape,
    verts: &[(i32, i32, i32); MAX_VERTS],
    vert_count: usize,
    edges: &[(u8, u8); MAX_EDGES],
    edge_count: usize,
    prev: &mut [u8; CUBE_SIZE],
    prev_color: &mut [u8; CUBE_SIZE],
) {
    let mut curr = [b' '; CUBE_SIZE];
    let mut curr_color = [0u8; CUBE_SIZE];

    if vert_count == 0 || edge_count == 0 {
        return;
    }

    let (draw_min_x, draw_min_y, draw_max_x, draw_max_y) = draw_bounds();
    let angle = (phase & 63) as usize;
    let angle2 = ((phase >> 1) & 63) as usize;
    let (sin_y, cos_y) = (SIN_LUT[angle], COS_LUT[angle]);
    let (sin_x, cos_x) = (SIN_LUT[angle2], COS_LUT[angle2]);

    let mut verts2d = [(0i32, 0i32); MAX_VERTS];
    let center_x = (draw_min_x + draw_max_x) as i32 / 2;
    let center_y = (draw_min_y + draw_max_y) as i32 / 2 - CENTER_Y_OFFSET;
    let proj_scale = (DRAW_SIZE as i32 / 2).saturating_sub(1).max(1);
    let mut max_abs = 0i32;
    for &(x, y, z) in verts.iter().take(vert_count) {
        max_abs = max_abs.max(x.abs()).max(y.abs()).max(z.abs());
    }
    if max_abs == 0 {
        return;
    }
    let (shape_scale_num, shape_scale_den) = match shape {
        WireShape::Cube => (1i64, 4i64),
        WireShape::Icosidodecahedron => (1i64, 1i64),
    };
    for (i, (x, y, z)) in verts.iter().copied().take(vert_count).enumerate() {
        let mut x = (x as i64 * (CUBE_SCALE as i64 / 2)) / max_abs as i64;
        let mut y = (y as i64 * (CUBE_SCALE as i64 / 2)) / max_abs as i64;
        let mut z = (z as i64 * (CUBE_SCALE as i64 / 2)) / max_abs as i64;
        x = x * shape_scale_num / shape_scale_den;
        y = y * shape_scale_num / shape_scale_den;
        z = z * shape_scale_num / shape_scale_den;
        let x = x as i32;
        let y = y as i32;
        let z = z as i32;

        let x1 = (x * cos_y + z * sin_y) / CUBE_SCALE;
        let z1 = (-x * sin_y + z * cos_y) / CUBE_SCALE;
        let y1 = (y * cos_x - z1 * sin_x) / CUBE_SCALE;
        // Fixed-scale (no perspective): rotate only, no z-based "zoom".
        let denom = CUBE_DIST.max(CUBE_SCALE / 2);
        let px = (x1 * proj_scale / denom) + center_x;
        let py = (y1 * proj_scale / denom) + center_y;
        verts2d[i] = (px, py);
    }

    let edge_count = edge_count as u8;
    let edge_denom = if edge_count > 1 {
        edge_count as u16 - 1
    } else {
        1
    };
    for (i, &(a, b)) in edges.iter().take(edge_count as usize).enumerate() {
        let (x0, y0) = verts2d[a as usize];
        let (x1, y1) = verts2d[b as usize];
        let t = (i as u16 * 255 / edge_denom).min(255) as u8;
        draw_line(&mut curr, &mut curr_color, x0, y0, x1, y1, b'#', t);
    }

    for idx in 0..CUBE_SIZE {
        let now = curr[idx];
        let color_now = curr_color[idx];
        if now != prev[idx] || (now == b'#' && color_now != prev_color[idx]) {
            let row = idx / CUBE_COLS;
            let col = idx % CUBE_COLS;
            write_pos(io, row + 1, col + 1);
            if now == b'#' {
                let (r, g, b) = line_rgb(color_now);
                io.write_fmt(format_args!("{}", ecma48::color("§", (r, g, b))));
            } else if now == b'.' {
                io.write_byte(b'.');
            } else {
                io.write_byte(b' ');
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
fn plot(
    buf: &mut [u8; CUBE_SIZE],
    colors: &mut [u8; CUBE_SIZE],
    x: i32,
    y: i32,
    ch: u8,
    color: u8,
) {
    if x < 0 || y < 0 {
        return;
    }
    let x = x as usize;
    let y = y as usize;
    if x < BORDER_INSET || y < BORDER_INSET {
        return;
    }
    if x >= CUBE_COLS - BORDER_INSET || y >= CUBE_ROWS - BORDER_INSET {
        return;
    }
    let (draw_min_x, draw_min_y, draw_max_x, draw_max_y) = draw_bounds();
    if x < draw_min_x || x > draw_max_x || y < draw_min_y || y > draw_max_y {
        return;
    }
    let idx = y * CUBE_COLS + x;
    buf[idx] = ch;
    colors[idx] = color;
}

#[inline]
fn write_pos(io: &dyn ShellIo, row: usize, col: usize) {
    io.write_fmt(format_args!("{}", ecma48::pos(row, col)));
}

#[inline]
fn draw_bounds() -> (usize, usize, usize, usize) {
    let safe_min_x = BORDER_INSET;
    let safe_min_y = BORDER_INSET;
    let safe_max_x = CUBE_COLS - BORDER_INSET - 1;
    let safe_max_y = CUBE_ROWS - BORDER_INSET - 1;
    let safe_w = safe_max_x.saturating_sub(safe_min_x) + 1;
    let safe_h = safe_max_y.saturating_sub(safe_min_y) + 1;

    let base_x = safe_min_x + safe_w.saturating_sub(DRAW_SIZE) / 2;
    let base_y = safe_min_y + safe_h.saturating_sub(DRAW_SIZE) / 2;
    let mut y = base_y as isize + DRAW_Y_SHIFT;
    let min_y = safe_min_y as isize;
    let max_y = (safe_max_y + 1).saturating_sub(DRAW_SIZE) as isize;
    if y < min_y {
        y = min_y;
    }
    if y > max_y {
        y = max_y;
    }
    let draw_min_x = base_x;
    let draw_min_y = y as usize;
    let draw_max_x = draw_min_x + DRAW_SIZE - 1;
    let draw_max_y = draw_min_y + DRAW_SIZE - 1;
    (draw_min_x, draw_min_y, draw_max_x, draw_max_y)
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

const ICO_EDGES: [(f64, f64, f64, f64, f64, f64); 60] = [
    (0.000000, 0.000000, 1.618034, 0.809017, 0.500000, 1.309017),
    (0.000000, 0.000000, 1.618034, -0.809017, 0.500000, 1.309017),
    (0.000000, 0.000000, 1.618034, 0.809017, -0.500000, 1.309017),
    (0.000000, 0.000000, 1.618034, -0.809017, -0.500000, 1.309017),
    (0.500000, 1.309017, 0.809017, -0.500000, 1.309017, 0.809017),
    (0.500000, 1.309017, 0.809017, 0.809017, 0.500000, 1.309017),
    (0.500000, 1.309017, 0.809017, 1.309017, 0.809017, 0.500000),
    (0.500000, 1.309017, 0.809017, 0.000000, 1.618034, 0.000000),
    (-0.500000, 1.309017, 0.809017, -0.809017, 0.500000, 1.309017),
    (-0.500000, 1.309017, 0.809017, -1.309017, 0.809017, 0.500000),
    (-0.500000, 1.309017, 0.809017, 0.000000, 1.618034, 0.000000),
    (0.809017, 0.500000, 1.309017, 0.809017, -0.500000, 1.309017),
    (0.809017, 0.500000, 1.309017, 1.309017, 0.809017, 0.500000),
    (
        -0.809017, 0.500000, 1.309017, -0.809017, -0.500000, 1.309017,
    ),
    (-0.809017, 0.500000, 1.309017, -1.309017, 0.809017, 0.500000),
    (1.309017, 0.809017, 0.500000, 1.309017, 0.809017, -0.500000),
    (1.309017, 0.809017, 0.500000, 1.618034, 0.000000, 0.000000),
    (
        -1.309017, 0.809017, 0.500000, -1.309017, 0.809017, -0.500000,
    ),
    (-1.309017, 0.809017, 0.500000, -1.618034, 0.000000, 0.000000),
    (
        0.500000, 1.309017, -0.809017, -0.500000, 1.309017, -0.809017,
    ),
    (0.500000, 1.309017, -0.809017, 0.809017, 0.500000, -1.309017),
    (0.500000, 1.309017, -0.809017, 1.309017, 0.809017, -0.500000),
    (0.500000, 1.309017, -0.809017, 0.000000, 1.618034, 0.000000),
    (
        -0.500000, 1.309017, -0.809017, -0.809017, 0.500000, -1.309017,
    ),
    (
        -0.500000, 1.309017, -0.809017, -1.309017, 0.809017, -0.500000,
    ),
    (-0.500000, 1.309017, -0.809017, 0.000000, 1.618034, 0.000000),
    (
        0.809017, 0.500000, -1.309017, 0.809017, -0.500000, -1.309017,
    ),
    (0.809017, 0.500000, -1.309017, 1.309017, 0.809017, -0.500000),
    (
        -0.809017, 0.500000, -1.309017, -0.809017, -0.500000, -1.309017,
    ),
    (
        -0.809017, 0.500000, -1.309017, -1.309017, 0.809017, -0.500000,
    ),
    (0.000000, 0.000000, -1.618034, 0.809017, 0.500000, -1.309017),
    (
        0.000000, 0.000000, -1.618034, -0.809017, 0.500000, -1.309017,
    ),
    (
        0.000000, 0.000000, -1.618034, 0.809017, -0.500000, -1.309017,
    ),
    (
        0.000000, 0.000000, -1.618034, -0.809017, -0.500000, -1.309017,
    ),
    (
        0.500000, -1.309017, 0.809017, -0.500000, -1.309017, 0.809017,
    ),
    (0.500000, -1.309017, 0.809017, 0.809017, -0.500000, 1.309017),
    (0.500000, -1.309017, 0.809017, 1.309017, -0.809017, 0.500000),
    (0.500000, -1.309017, 0.809017, 0.000000, -1.618034, 0.000000),
    (
        -0.500000, -1.309017, 0.809017, -0.809017, -0.500000, 1.309017,
    ),
    (
        -0.500000, -1.309017, 0.809017, -1.309017, -0.809017, 0.500000,
    ),
    (
        -0.500000, -1.309017, 0.809017, 0.000000, -1.618034, 0.000000,
    ),
    (0.809017, -0.500000, 1.309017, 1.309017, -0.809017, 0.500000),
    (
        -0.809017, -0.500000, 1.309017, -1.309017, -0.809017, 0.500000,
    ),
    (
        1.309017, -0.809017, 0.500000, 1.309017, -0.809017, -0.500000,
    ),
    (1.309017, -0.809017, -0.500000, 1.618034, 0.000000, 0.000000),
    (1.309017, 0.809017, -0.500000, 1.618034, 0.000000, 0.000000),
    (
        0.500000, -1.309017, -0.809017, -0.500000, -1.309017, -0.809017,
    ),
    (
        0.500000, -1.309017, -0.809017, 0.809017, -0.500000, -1.309017,
    ),
    (
        0.500000, -1.309017, -0.809017, 1.309017, -0.809017, -0.500000,
    ),
    (
        0.500000, -1.309017, -0.809017, 0.000000, -1.618034, 0.000000,
    ),
    (
        -0.500000, -1.309017, -0.809017, -0.809017, -0.500000, -1.309017,
    ),
    (
        -0.500000, -1.309017, -0.809017, -1.309017, -0.809017, -0.500000,
    ),
    (
        -0.500000, -1.309017, -0.809017, 0.000000, -1.618034, 0.000000,
    ),
    (
        0.809017, -0.500000, -1.309017, 1.309017, -0.809017, -0.500000,
    ),
    (
        -0.809017, -0.500000, -1.309017, -1.309017, -0.809017, -0.500000,
    ),
    (
        -1.309017, -0.809017, 0.500000, -1.309017, -0.809017, -0.500000,
    ),
    (
        -1.309017, -0.809017, 0.500000, -1.618034, 0.000000, 0.000000,
    ),
    (1.309017, -0.809017, 0.500000, 1.618034, 0.000000, 0.000000),
    (
        -1.309017, 0.809017, -0.500000, -1.618034, 0.000000, 0.000000,
    ),
    (
        -1.309017, -0.809017, -0.500000, -1.618034, 0.000000, 0.000000,
    ),
];

fn build_icosidodecahedron(
    verts: &mut [(i32, i32, i32); MAX_VERTS],
    vert_count: &mut usize,
    edges: &mut [(u8, u8); MAX_EDGES],
    edge_count: &mut usize,
) {
    *vert_count = 0;
    *edge_count = 0;

    const V_SCALE: f64 = 1_000_000.0;

    #[inline]
    fn quantize(v: f64) -> i32 {
        // Avoid relying on libm-backed float rounding in early kernel code.
        let scaled = v * V_SCALE;
        if scaled >= 0.0 {
            (scaled + 0.5) as i32
        } else {
            (scaled - 0.5) as i32
        }
    }

    #[inline]
    fn find_or_add(
        verts: &mut [(i32, i32, i32); MAX_VERTS],
        vert_count: &mut usize,
        v: (i32, i32, i32),
    ) -> Option<u8> {
        for (i, &cur) in verts.iter().take(*vert_count).enumerate() {
            if cur == v {
                return Some(i as u8);
            }
        }
        if *vert_count >= MAX_VERTS {
            return None;
        }
        let idx = *vert_count;
        verts[idx] = v;
        *vert_count += 1;
        Some(idx as u8)
    }

    for &(x0, y0, z0, x1, y1, z1) in ICO_EDGES.iter() {
        if *edge_count >= MAX_EDGES {
            break;
        }
        let v0 = (quantize(x0), quantize(y0), quantize(z0));
        let v1 = (quantize(x1), quantize(y1), quantize(z1));

        let Some(i0) = find_or_add(verts, vert_count, v0) else {
            continue;
        };
        let Some(i1) = find_or_add(verts, vert_count, v1) else {
            continue;
        };

        if i0 == i1 {
            continue;
        }

        edges[*edge_count] = (i0, i1);
        *edge_count += 1;
    }
}

const SIN_LUT: [i32; 64] = [
    0, 100, 200, 297, 392, 483, 569, 650, 724, 792, 851, 904, 946, 979, 1004, 1019, 1024, 1019,
    1004, 979, 946, 904, 851, 792, 724, 650, 569, 483, 392, 297, 200, 100, 0, -100, -200, -297,
    -392, -483, -569, -650, -724, -792, -851, -904, -946, -979, -1004, -1019, -1024, -1019, -1004,
    -979, -946, -904, -851, -792, -724, -650, -569, -483, -392, -297, -200, -100,
];

const COS_LUT: [i32; 64] = [
    1024, 1019, 1004, 979, 946, 904, 851, 792, 724, 650, 569, 483, 392, 297, 200, 100, 0, -100,
    -200, -297, -392, -483, -569, -650, -724, -792, -851, -904, -946, -979, -1004, -1019, -1024,
    -1019, -1004, -979, -946, -904, -851, -792, -724, -650, -569, -483, -392, -297, -200, -100, 0,
    100, 200, 297, 392, 483, 569, 650, 724, 792, 851, 904, 946, 979, 1004, 1019,
];
