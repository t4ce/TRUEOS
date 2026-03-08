#![cfg(feature = "trueos")]

use alloc::collections::BTreeMap;
use alloc::string::String;
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
    RadioSelected,
}

#[cfg(feature = "usvg-native")]
type ParsedSvgTree = usvg::Tree;
#[cfg(not(feature = "usvg-native"))]
type ParsedSvgTree = ();

pub struct SvgIconBuffer {
    width: u32,
    height: u32,
    // Flat line list: x0,y0,x1,y1,thickness,color_rgba.
    line_cmds: Vec<f64>,
    pixels_rgba: Vec<u32>,
    svg_source: String,
    parsed: Option<ParsedSvgTree>,
}

pub struct SvgImportedAsset {
    width: u32,
    height: u32,
    pixels_rgba: Vec<u32>,
}

impl SvgIconBuffer {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels_rgba(&self) -> &[u32] {
        &self.pixels_rgba
    }

    pub fn line_cmds(&self) -> &[f64] {
        &self.line_cmds
    }

    pub fn svg_source(&self) -> &str {
        self.svg_source.as_str()
    }

    pub fn is_parsed_with_usvg(&self) -> bool {
        self.parsed.is_some()
    }
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

fn clamp01(v: f32) -> f32 {
    if v <= 0.0 {
        0.0
    } else if v >= 1.0 {
        1.0
    } else {
        v
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

#[inline]
fn color_from_f64(v: f64) -> u32 {
    if !v.is_finite() {
        return 0;
    }
    v as u32
}

fn parse_usvg_tree(svg_source: &str) -> Option<ParsedSvgTree> {
    #[cfg(feature = "usvg-native")]
    {
        let opts = usvg::Options::default();
        return usvg::Tree::from_str(svg_source, &opts).ok();
    }

    #[cfg(not(feature = "usvg-native"))]
    {
        let _ = svg_source;
        None
    }
}

fn push_line(cmds: &mut Vec<f64>, x0: f64, y0: f64, x1: f64, y1: f64, thickness: f64, color: u32) {
    cmds.push(x0);
    cmds.push(y0);
    cmds.push(x1);
    cmds.push(y1);
    cmds.push(thickness);
    cmds.push(color as f64);
}

fn render_line_cmds_to_pixels(width: u32, height: u32, line_cmds: &[f64]) -> Vec<u32> {
    let w = width.max(1);
    let h = height.max(1);
    let mut pixels = vec![0u32; (w as usize).saturating_mul(h as usize)];

    let mut i = 0usize;
    while i + 5 < line_cmds.len() {
        draw_line_norm(
            &mut pixels,
            w,
            h,
            line_cmds[i] as f32,
            line_cmds[i + 1] as f32,
            line_cmds[i + 2] as f32,
            line_cmds[i + 3] as f32,
            (line_cmds[i + 4] as i32).max(1),
            color_from_f64(line_cmds[i + 5]),
        );
        i += 6;
    }

    pixels
}

fn make_icon(svg_source: &str, mut line_cmds: Vec<f64>) -> SvgIconBuffer {
    if line_cmds.len() % 6 != 0 {
        line_cmds.truncate(line_cmds.len().saturating_sub(line_cmds.len() % 6));
    }

    let parsed = parse_usvg_tree(svg_source);
    let pixels_rgba = render_line_cmds_to_pixels(WINDOW_ICON_SIZE, WINDOW_ICON_SIZE, &line_cmds);

    SvgIconBuffer {
        width: WINDOW_ICON_SIZE,
        height: WINDOW_ICON_SIZE,
        line_cmds,
        pixels_rgba,
        svg_source: String::from(svg_source),
        parsed,
    }
}

#[inline]
fn make_icon_from_def(def: crate::icon::WindowIconDef) -> SvgIconBuffer {
    make_icon(def.svg_source, def.line_cmds.to_vec())
}

fn make_radio_selected_icon() -> SvgIconBuffer {
    let mut cmds = Vec::new();
    let segs = crate::icon::RADIO_SELECTED_SEGS;
    let outer_r = crate::icon::RADIO_SELECTED_OUTER_R;
    let inner_r = crate::icon::RADIO_SELECTED_INNER_R;

    for i in 0..segs {
        let a0 = (i as f32) * core::f32::consts::TAU / (segs as f32);
        let a1 = ((i + 1) as f32) * core::f32::consts::TAU / (segs as f32);

        let ox0 = 0.5 + outer_r * libm::cosf(a0);
        let oy0 = 0.5 + outer_r * libm::sinf(a0);
        let ox1 = 0.5 + outer_r * libm::cosf(a1);
        let oy1 = 0.5 + outer_r * libm::sinf(a1);
        push_line(
            &mut cmds,
            ox0 as f64,
            oy0 as f64,
            ox1 as f64,
            oy1 as f64,
            2.0,
            crate::icon::ICON_STROKE_RGBA,
        );

        let ix0 = 0.5 + inner_r * libm::cosf(a0);
        let iy0 = 0.5 + inner_r * libm::sinf(a0);
        let ix1 = 0.5 + inner_r * libm::cosf(a1);
        let iy1 = 0.5 + inner_r * libm::sinf(a1);
        push_line(
            &mut cmds,
            ix0 as f64,
            iy0 as f64,
            ix1 as f64,
            iy1 as f64,
            2.0,
            crate::icon::ICON_STROKE_RGBA,
        );
    }

    make_icon(crate::icon::RADIO_SELECTED_SVG, cmds)
}

static WINDOW_SVGS_INIT: Once<()> = Once::new();
static WINDOW_SVGS: Mutex<Option<[SvgIconBuffer; 8]>> = Mutex::new(None);
static IMPORTED_SVGS: Mutex<BTreeMap<u32, SvgImportedAsset>> = Mutex::new(BTreeMap::new());

pub fn init_window_svgs_once() {
    WINDOW_SVGS_INIT.call_once(|| {
        let close = make_icon_from_def(crate::icon::WINDOW_ICON_DEFS[0]);
        let minimize = make_icon_from_def(crate::icon::WINDOW_ICON_DEFS[1]);
        let maximize = make_icon_from_def(crate::icon::WINDOW_ICON_DEFS[2]);
        let arrow_left = make_icon_from_def(crate::icon::WINDOW_ICON_DEFS[3]);
        let arrow_right = make_icon_from_def(crate::icon::WINDOW_ICON_DEFS[4]);
        let arrow_up = make_icon_from_def(crate::icon::WINDOW_ICON_DEFS[5]);
        let arrow_down = make_icon_from_def(crate::icon::WINDOW_ICON_DEFS[6]);

        let radio_selected = make_radio_selected_icon();

        *WINDOW_SVGS.lock() = Some([
            close,
            minimize,
            maximize,
            arrow_left,
            arrow_right,
            arrow_up,
            arrow_down,
            radio_selected,
        ]);
    });
}

pub fn with_window_svgs<R>(f: impl FnOnce(&[SvgIconBuffer; 8]) -> R) -> Option<R> {
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
            WindowIconKind::RadioSelected => 7,
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
                let x = flat_cmds[i] as f32;
                let y = flat_cmds[i + 1] as f32;
                let rw = flat_cmds[i + 2] as f32;
                let rh = flat_cmds[i + 3] as f32;
                let fill = color_from_f64(flat_cmds[i + 4]);
                let stroke = color_from_f64(flat_cmds[i + 5]);
                let sw = (flat_cmds[i + 6] as i32).max(0);
                i += 7;

                if (fill >> 24) != 0 {
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
                            write_px(&mut pixels, w, h, px, py, fill);
                        }
                    }
                }
                if (stroke >> 24) != 0 && sw > 0 {
                    draw_line_norm(&mut pixels, w, h, x, y, x + rw, y, sw, stroke);
                    draw_line_norm(&mut pixels, w, h, x + rw, y, x + rw, y + rh, sw, stroke);
                    draw_line_norm(&mut pixels, w, h, x + rw, y + rh, x, y + rh, sw, stroke);
                    draw_line_norm(&mut pixels, w, h, x, y + rh, x, y, sw, stroke);
                }
            }
            // circle: cx cy r fillRGBA strokeRGBA strokeW
            2 => {
                if i + 5 >= flat_cmds.len() {
                    break;
                }
                let cx = flat_cmds[i] as f32;
                let cy = flat_cmds[i + 1] as f32;
                let r = flat_cmds[i + 2] as f32;
                let fill = color_from_f64(flat_cmds[i + 3]);
                let stroke = color_from_f64(flat_cmds[i + 4]);
                let sw = (flat_cmds[i + 5] as i32).max(0);
                i += 6;

                if (fill >> 24) != 0 {
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
                                write_px(&mut pixels, w, h, x2, y2, fill);
                            }
                        }
                    }
                }
                if (stroke >> 24) != 0 && sw > 0 {
                    let segments = 64usize;
                    let mut prev_x = cx + r;
                    let mut prev_y = cy;
                    for s in 1..=segments {
                        let t = (s as f32) * core::f32::consts::TAU / (segments as f32);
                        let nx = cx + r * libm::cosf(t);
                        let ny = cy + r * libm::sinf(t);
                        draw_line_norm(&mut pixels, w, h, prev_x, prev_y, nx, ny, sw, stroke);
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
                let sw = (flat_cmds[i + 1] as i32).max(0);
                i += 2;
                if (stroke >> 24) != 0 && sw > 0 {
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
                            sw,
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
