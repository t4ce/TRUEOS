#![cfg(feature = "trueos")]

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use spin::{Mutex, Once};

pub const WINDOW_ICON_SIZE: u32 = 32;

#[derive(Clone, Copy)]
pub enum WindowIconKind {
    Close,
    Minimize,
    Maximize,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
}

#[derive(Clone, Copy)]
pub struct SvgLineCmd {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub thickness_px: u8,
    pub color_rgba: u32,
}

pub struct SvgIconBuffer {
    width: u32,
    height: u32,
    cmds: Vec<SvgLineCmd>,
    pixels_rgba: Vec<u32>,
}

pub struct SvgImportedAsset {
    width: u32,
    height: u32,
    pixels_rgba: Vec<u32>,
}

impl SvgIconBuffer {
    pub fn new(width: u32, height: u32) -> Self {
        let w = width.max(1);
        let h = height.max(1);
        let len = (w as usize).saturating_mul(h as usize);
        Self {
            width: w,
            height: h,
            cmds: Vec::new(),
            pixels_rgba: vec![0; len],
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels_rgba(&self) -> &[u32] {
        &self.pixels_rgba
    }

    pub fn cmds(&self) -> &[SvgLineCmd] {
        &self.cmds
    }

    pub fn clear_cmds(&mut self) {
        self.cmds.clear();
    }

    pub fn clear_pixels(&mut self, rgba: u32) {
        for px in &mut self.pixels_rgba {
            *px = rgba;
        }
    }

    pub fn line_norm(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        thickness_px: u8,
        color_rgba: u32,
    ) {
        self.cmds.push(SvgLineCmd {
            x0: clamp01(x0),
            y0: clamp01(y0),
            x1: clamp01(x1),
            y1: clamp01(y1),
            thickness_px: thickness_px.max(1),
            color_rgba,
        });
    }

    pub fn rasterize_once(&mut self) {
        self.clear_pixels(0x00000000);
        let w = self.width.max(1);
        let h = self.height.max(1);
        for cmd in &self.cmds {
            draw_line_cmd(&mut self.pixels_rgba, w, h, *cmd);
        }
    }
}

fn clamp01(v: f32) -> f32 {
    if v <= 0.0 {
        0.0
    } else if v >= 1.0 {
        1.0
    } else {
        v
    }
}

fn draw_line_cmd(dst: &mut [u32], w: u32, h: u32, cmd: SvgLineCmd) {
    let wf = (w.saturating_sub(1)) as f32;
    let hf = (h.saturating_sub(1)) as f32;
    let x0 = cmd.x0 * wf;
    let y0 = cmd.y0 * hf;
    let x1 = cmd.x1 * wf;
    let y1 = cmd.y1 * hf;

    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let steps = ((if dx > dy { dx } else { dy }) as usize).saturating_add(1);
    let radius = (cmd.thickness_px as i32).saturating_sub(1) / 2;

    for i in 0..=steps {
        let t = if steps == 0 {
            0.0
        } else {
            i as f32 / steps as f32
        };
        let xf = x0 + (x1 - x0) * t;
        let yf = y0 + (y1 - y0) * t;
        let xc = xf as i32;
        let yc = yf as i32;

        for oy in -radius..=radius {
            for ox in -radius..=radius {
                let px = xc + ox;
                let py = yc + oy;
                if px < 0 || py < 0 {
                    continue;
                }
                let pxu = px as u32;
                let pyu = py as u32;
                if pxu >= w || pyu >= h {
                    continue;
                }
                let idx = (pyu as usize)
                    .saturating_mul(w as usize)
                    .saturating_add(pxu as usize);
                if idx < dst.len() {
                    dst[idx] = cmd.color_rgba;
                }
            }
        }
    }
}

fn write_px(dst: &mut [u32], w: u32, h: u32, x: i32, y: i32, rgba: u32) {
    if x < 0 || y < 0 {
        return;
    }
    let xu = x as u32;
    let yu = y as u32;
    if xu >= w || yu >= h {
        return;
    }
    let idx = (yu as usize)
        .saturating_mul(w as usize)
        .saturating_add(xu as usize);
    if idx < dst.len() {
        dst[idx] = rgba;
    }
}

fn color_from_f64(v: f64) -> u32 {
    if !v.is_finite() {
        return 0;
    }
    v as u32
}

fn draw_line_norm(
    dst: &mut [u32],
    w: u32,
    h: u32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    thickness_px: i32,
    color_rgba: u32,
) {
    let wf = (w.saturating_sub(1)) as f32;
    let hf = (h.saturating_sub(1)) as f32;
    let px0 = clamp01(x0) * wf;
    let py0 = clamp01(y0) * hf;
    let px1 = clamp01(x1) * wf;
    let py1 = clamp01(y1) * hf;
    let dx = (px1 - px0).abs();
    let dy = (py1 - py0).abs();
    let steps = ((if dx > dy { dx } else { dy }) as usize).saturating_add(1);
    let radius = thickness_px.max(1).saturating_sub(1) / 2;

    for i in 0..=steps {
        let t = if steps == 0 {
            0.0
        } else {
            i as f32 / steps as f32
        };
        let x = px0 + (px1 - px0) * t;
        let y = py0 + (py1 - py0) * t;
        let xi = x as i32;
        let yi = y as i32;
        for oy in -radius..=radius {
            for ox in -radius..=radius {
                write_px(dst, w, h, xi + ox, yi + oy, color_rgba);
            }
        }
    }
}

fn fill_rect_norm(dst: &mut [u32], w: u32, h: u32, x: f32, y: f32, rw: f32, rh: f32, rgba: u32) {
    let x0 = clamp01(x);
    let y0 = clamp01(y);
    let x1 = clamp01(x + rw);
    let y1 = clamp01(y + rh);
    let wf = (w.saturating_sub(1)) as f32;
    let hf = (h.saturating_sub(1)) as f32;
    let sx = (x0 * wf) as i32;
    let ex = (x1 * wf) as i32;
    let sy = (y0 * hf) as i32;
    let ey = (y1 * hf) as i32;
    for py in sy.min(ey)..=sy.max(ey) {
        for px in sx.min(ex)..=sx.max(ex) {
            write_px(dst, w, h, px, py, rgba);
        }
    }
}

fn fill_circle_norm(dst: &mut [u32], w: u32, h: u32, cx: f32, cy: f32, r: f32, rgba: u32) {
    let wf = (w.saturating_sub(1)) as f32;
    let hf = (h.saturating_sub(1)) as f32;
    let px = clamp01(cx) * wf;
    let py = clamp01(cy) * hf;
    let rr = r.max(0.0) * if wf < hf { wf } else { hf };
    let rr2 = rr * rr;
    let min_x = (px - rr) as i32;
    let max_x = (px + rr) as i32;
    let min_y = (py - rr) as i32;
    let max_y = (py + rr) as i32;
    for y2 in min_y..=max_y {
        for x2 in min_x..=max_x {
            let dx = x2 as f32 - px;
            let dy = y2 as f32 - py;
            if dx * dx + dy * dy <= rr2 {
                write_px(dst, w, h, x2, y2, rgba);
            }
        }
    }
}

static WINDOW_SVGS_INIT: Once<()> = Once::new();
static WINDOW_SVGS: Mutex<Option<[SvgIconBuffer; 7]>> = Mutex::new(None);
static IMPORTED_SVGS: Mutex<BTreeMap<u32, SvgImportedAsset>> = Mutex::new(BTreeMap::new());

pub fn init_window_svgs_once() {
    WINDOW_SVGS_INIT.call_once(|| {
        let mut close = SvgIconBuffer::new(WINDOW_ICON_SIZE, WINDOW_ICON_SIZE);
        close.line_norm(0.22, 0.22, 0.78, 0.78, 2, 0xFF202020);
        close.line_norm(0.78, 0.22, 0.22, 0.78, 2, 0xFF202020);
        close.rasterize_once();

        let mut minimize = SvgIconBuffer::new(WINDOW_ICON_SIZE, WINDOW_ICON_SIZE);
        minimize.line_norm(0.20, 0.62, 0.80, 0.62, 2, 0xFF202020);
        minimize.rasterize_once();

        let mut maximize = SvgIconBuffer::new(WINDOW_ICON_SIZE, WINDOW_ICON_SIZE);
        maximize.line_norm(0.24, 0.26, 0.76, 0.26, 2, 0xFF202020);
        maximize.line_norm(0.76, 0.26, 0.76, 0.74, 2, 0xFF202020);
        maximize.line_norm(0.76, 0.74, 0.24, 0.74, 2, 0xFF202020);
        maximize.line_norm(0.24, 0.74, 0.24, 0.26, 2, 0xFF202020);
        maximize.rasterize_once();

        let mut arrow_left = SvgIconBuffer::new(WINDOW_ICON_SIZE, WINDOW_ICON_SIZE);
        arrow_left.line_norm(0.70, 0.24, 0.36, 0.50, 2, 0xFF202020);
        arrow_left.line_norm(0.70, 0.76, 0.36, 0.50, 2, 0xFF202020);
        arrow_left.rasterize_once();

        let mut arrow_right = SvgIconBuffer::new(WINDOW_ICON_SIZE, WINDOW_ICON_SIZE);
        arrow_right.line_norm(0.30, 0.24, 0.64, 0.50, 2, 0xFF202020);
        arrow_right.line_norm(0.30, 0.76, 0.64, 0.50, 2, 0xFF202020);
        arrow_right.rasterize_once();

        let mut arrow_up = SvgIconBuffer::new(WINDOW_ICON_SIZE, WINDOW_ICON_SIZE);
        arrow_up.line_norm(0.22, 0.64, 0.50, 0.34, 2, 0xFF202020);
        arrow_up.line_norm(0.78, 0.64, 0.50, 0.34, 2, 0xFF202020);
        arrow_up.rasterize_once();

        let mut arrow_down = SvgIconBuffer::new(WINDOW_ICON_SIZE, WINDOW_ICON_SIZE);
        arrow_down.line_norm(0.22, 0.36, 0.50, 0.66, 2, 0xFF202020);
        arrow_down.line_norm(0.78, 0.36, 0.50, 0.66, 2, 0xFF202020);
        arrow_down.rasterize_once();

        *WINDOW_SVGS.lock() = Some([
            close,
            minimize,
            maximize,
            arrow_left,
            arrow_right,
            arrow_up,
            arrow_down,
        ]);
    });
}

pub fn with_window_svgs<R>(f: impl FnOnce(&[SvgIconBuffer; 7]) -> R) -> Option<R> {
    init_window_svgs_once();
    let guard = WINDOW_SVGS.lock();
    guard.as_ref().map(f)
}

pub fn with_window_svg<R>(kind: WindowIconKind, f: impl FnOnce(&SvgIconBuffer) -> R) -> Option<R> {
    with_window_svgs(|icons| {
        let idx = match kind {
            WindowIconKind::Close => 0,
            WindowIconKind::Minimize => 1,
            WindowIconKind::Maximize => 2,
            WindowIconKind::ArrowLeft => 3,
            WindowIconKind::ArrowRight => 4,
            WindowIconKind::ArrowUp => 5,
            WindowIconKind::ArrowDown => 6,
        };
        f(&icons[idx])
    })
}

pub fn import_svg_from_flat(asset_id: u32, width: u32, height: u32, flat_cmds: &[f64]) -> bool {
    if asset_id == 0 {
        return false;
    }
    let w = width.clamp(1, 1024);
    let h = height.clamp(1, 1024);
    let mut pixels = vec![0u32; (w as usize).saturating_mul(h as usize)];

    let mut i = 0usize;
    while i < flat_cmds.len() {
        let op = flat_cmds[i] as i32;
        i += 1;
        match op {
            // rect: x y w h fillRGBA strokeRGBA strokeW
            1 => {
                if i + 6 >= flat_cmds.len() {
                    break;
                }
                let x = flat_cmds[i];
                let y = flat_cmds[i + 1];
                let rw = flat_cmds[i + 2];
                let rh = flat_cmds[i + 3];
                let fill = color_from_f64(flat_cmds[i + 4]);
                let stroke = color_from_f64(flat_cmds[i + 5]);
                let sw = (flat_cmds[i + 6] as f32).max(0.0);
                i += 7;
                if (fill >> 24) != 0 {
                    fill_rect_norm(
                        &mut pixels,
                        w,
                        h,
                        x as f32,
                        y as f32,
                        rw as f32,
                        rh as f32,
                        fill,
                    );
                }
                if (stroke >> 24) != 0 && sw > 0.0 {
                    draw_line_norm(
                        &mut pixels,
                        w,
                        h,
                        x as f32,
                        y as f32,
                        (x + rw) as f32,
                        y as f32,
                        sw as i32,
                        stroke,
                    );
                    draw_line_norm(
                        &mut pixels,
                        w,
                        h,
                        (x + rw) as f32,
                        y as f32,
                        (x + rw) as f32,
                        (y + rh) as f32,
                        sw as i32,
                        stroke,
                    );
                    draw_line_norm(
                        &mut pixels,
                        w,
                        h,
                        (x + rw) as f32,
                        (y + rh) as f32,
                        x as f32,
                        (y + rh) as f32,
                        sw as i32,
                        stroke,
                    );
                    draw_line_norm(
                        &mut pixels,
                        w,
                        h,
                        x as f32,
                        (y + rh) as f32,
                        x as f32,
                        y as f32,
                        sw as i32,
                        stroke,
                    );
                }
            }
            // circle: cx cy r fillRGBA strokeRGBA strokeW
            2 => {
                if i + 5 >= flat_cmds.len() {
                    break;
                }
                let cx = flat_cmds[i];
                let cy = flat_cmds[i + 1];
                let r = flat_cmds[i + 2];
                let fill = color_from_f64(flat_cmds[i + 3]);
                let stroke = color_from_f64(flat_cmds[i + 4]);
                let sw = (flat_cmds[i + 5] as f32).max(0.0);
                i += 6;
                if (fill >> 24) != 0 {
                    fill_circle_norm(&mut pixels, w, h, cx as f32, cy as f32, r as f32, fill);
                }
                if (stroke >> 24) != 0 && sw > 0.0 {
                    let segments = 64usize;
                    let mut prev_x = (cx + r) as f32;
                    let mut prev_y = cy as f32;
                    for s in 1..=segments {
                        let t = (s as f32) * core::f32::consts::TAU / (segments as f32);
                        let nx = cx as f32 + r as f32 * libm::cosf(t);
                        let ny = cy as f32 + r as f32 * libm::sinf(t);
                        draw_line_norm(
                            &mut pixels,
                            w,
                            h,
                            prev_x,
                            prev_y,
                            nx,
                            ny,
                            sw as i32,
                            stroke,
                        );
                        prev_x = nx;
                        prev_y = ny;
                    }
                }
            }
            // polyline: pointCount (x y)* strokeRGBA strokeW
            3 => {
                if i >= flat_cmds.len() {
                    break;
                }
                let n = (flat_cmds[i] as i32).max(0) as usize;
                i += 1;
                if n < 2 || i + n * 2 + 1 >= flat_cmds.len() {
                    break;
                }
                let pts_start = i;
                i += n * 2;
                let stroke = color_from_f64(flat_cmds[i]);
                let sw = (flat_cmds[i + 1] as f32).max(0.0);
                i += 2;
                if (stroke >> 24) != 0 && sw > 0.0 {
                    for p in 0..(n - 1) {
                        let a = pts_start + p * 2;
                        let b = a + 2;
                        draw_line_norm(
                            &mut pixels,
                            w,
                            h,
                            flat_cmds[a] as f32,
                            flat_cmds[a + 1] as f32,
                            flat_cmds[b] as f32,
                            flat_cmds[b + 1] as f32,
                            sw as i32,
                            stroke,
                        );
                    }
                }
            }
            _ => break,
        }
    }

    IMPORTED_SVGS.lock().insert(
        asset_id,
        SvgImportedAsset {
            width: w,
            height: h,
            pixels_rgba: pixels,
        },
    );
    true
}

pub fn with_imported_svg<R>(asset_id: u32, f: impl FnOnce(&SvgImportedAsset) -> R) -> Option<R> {
    let guard = IMPORTED_SVGS.lock();
    guard.get(&asset_id).map(f)
}

impl SvgImportedAsset {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels_rgba(&self) -> &[u32] {
        &self.pixels_rgba
    }
}
