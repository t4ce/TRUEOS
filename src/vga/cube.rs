use core::f32::consts::PI;
use core::sync::atomic::{AtomicU32, Ordering};

use libm::{cosf, roundf, sinf};

use crate::vga;

const AREA_MARGIN: usize = 8;
const INNER_PAD_PX: f32 = 2.0;
const EDGE_COLOR: u32 = 0x00_60_FF_D0;

static ANGLE_DEG: AtomicU32 = AtomicU32::new(0);

pub fn tick() {
    let angle = ANGLE_DEG.fetch_add(1, Ordering::Relaxed).wrapping_add(1);
    render(angle as f32);
}

fn render(angle_deg: f32) {
    let (width, height) = match vga::framebuffer_dimensions() {
        Some((w, h)) => (w as i32, h as i32),
        None => return,
    };
    let header_h = vga::header_height().min(height as usize) as i32;
    if header_h <= 0 {
        return;
    }
    let area_size = (header_h as usize).max(16);

    if width < (area_size + AREA_MARGIN) as i32 || height < area_size as i32 {
        return;
    }
    let origin_x = width
        .saturating_sub(area_size as i32)
        .saturating_sub(AREA_MARGIN as i32)
        .max(0);
    let origin_y = ((header_h.saturating_sub(area_size as i32)) / 2).max(0);
    let center_x = origin_x + (area_size as i32 / 2);
    let center_y = origin_y + (area_size as i32 / 2);
    let bg = vga::current_colors().map(|(_, bg, _)| bg).unwrap_or(0);

    vga::clear_rect(
        origin_x as usize,
        origin_y as usize,
        area_size,
        area_size,
        bg,
    );

    let yaw = angle_deg * PI / 180.0;
    let pitch = angle_deg * 0.6 * PI / 180.0;

    let vertices = [
        [-0.5_f32, -0.5_f32, -0.5_f32],
        [0.5, -0.5, -0.5],
        [0.5, 0.5, -0.5],
        [-0.5, 0.5, -0.5],
        [-0.5, -0.5, 0.5],
        [0.5, -0.5, 0.5],
        [0.5, 0.5, 0.5],
        [-0.5, 0.5, 0.5],
    ];

    let mut projected = [(0_i32, 0_i32); 8];
    let (cy, sy) = (cosf(yaw), sinf(yaw));
    let (cp, sp) = (cosf(pitch), sinf(pitch));
    let mut rotated = [(0.0_f32, 0.0_f32); 8];
    let mut max_extent = 0.0_f32;
    for (idx, [x, y, z]) in vertices.iter().enumerate() {
        let x1 = x * cy + z * sy;
        let z1 = -x * sy + z * cy;
        let y2 = y * cp - z1 * sp;
        rotated[idx] = (x1, y2);
        max_extent = max_extent.max(x1.abs()).max(y2.abs());
    }

    if max_extent <= f32::EPSILON {
        return;
    }
    let half = (area_size as f32) * 0.5;
    let usable = (half - INNER_PAD_PX).max(1.0);
    let cube_scale = usable / max_extent;

    for (idx, (x1, y2)) in rotated.iter().copied().enumerate() {
        let x2d = center_x + roundf(x1 * cube_scale) as i32;
        let y2d = center_y + roundf(y2 * cube_scale) as i32;
        projected[idx] = (x2d, y2d);
    }

    const EDGES: [(usize, usize); 12] = [
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

    for &(a, b) in EDGES.iter() {
        let (x0, y0) = projected[a];
        let (x1, y1) = projected[b];
        vga::draw_line(x0, y0, x1, y1, EDGE_COLOR);
    }
}
