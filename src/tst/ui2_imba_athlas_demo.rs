use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use embassy_time::{Duration as EmbassyDuration, Timer};

const UI2_IMBA_ATHLAS_DEMO_TEX_ID: u32 = 4_706;
const UI2_IMBA_ATHLAS_DEMO_WINDOW_X: f32 = 36.0;
const UI2_IMBA_ATHLAS_DEMO_WINDOW_Y: f32 = 84.0;
const UI2_IMBA_ATHLAS_DEMO_WINDOW_Z: i16 = 33;
const UI2_IMBA_ATHLAS_DEMO_MARGIN_X: u32 = 20;
const UI2_IMBA_ATHLAS_DEMO_MARGIN_Y: u32 = 18;
const UI2_IMBA_ATHLAS_DEMO_LABEL_W: u32 = 64;
const UI2_IMBA_ATHLAS_DEMO_ROW_GAP: u32 = 12;
const UI2_IMBA_ATHLAS_DEMO_COL_GAP: u32 = 4;
const UI2_IMBA_ATHLAS_DEMO_BG: [u8; 4] = [0xF7, 0xF4, 0xEC, 0xFF];

struct LoopSvgAsset {
    bytes: &'static [u8],
}

macro_rules! loop_svg_asset {
    ($file:literal) => {
        LoopSvgAsset {
            bytes: include_bytes!(concat!("../gfx/imbafont/imbasvg_loop/", $file)),
        }
    };
}

static IMBASVG_LOOP_ASSETS: &[LoopSvgAsset] = &[
    loop_svg_asset!("0.svg"),
    loop_svg_asset!("1.svg"),
    loop_svg_asset!("2.svg"),
    loop_svg_asset!("3.svg"),
    loop_svg_asset!("4.svg"),
    loop_svg_asset!("5.svg"),
    loop_svg_asset!("6.svg"),
    loop_svg_asset!("7.svg"),
    loop_svg_asset!("8.svg"),
    loop_svg_asset!("9.svg"),
    loop_svg_asset!("U0021.svg"),
    loop_svg_asset!("U0022.svg"),
    loop_svg_asset!("U0023.svg"),
    loop_svg_asset!("U0024.svg"),
    loop_svg_asset!("U0025.svg"),
    loop_svg_asset!("U0026.svg"),
    loop_svg_asset!("U0027.svg"),
    loop_svg_asset!("U0028.svg"),
    loop_svg_asset!("U0029.svg"),
    loop_svg_asset!("U002A.svg"),
    loop_svg_asset!("U002B.svg"),
    loop_svg_asset!("U002C.svg"),
    loop_svg_asset!("U002D.svg"),
    loop_svg_asset!("U002E.svg"),
    loop_svg_asset!("U002F.svg"),
    loop_svg_asset!("U003A.svg"),
    loop_svg_asset!("U003B.svg"),
    loop_svg_asset!("U003C.svg"),
    loop_svg_asset!("U003D.svg"),
    loop_svg_asset!("U003E.svg"),
    loop_svg_asset!("U003F.svg"),
    loop_svg_asset!("U0040.svg"),
    loop_svg_asset!("U005B.svg"),
    loop_svg_asset!("U005C.svg"),
    loop_svg_asset!("U005D.svg"),
    loop_svg_asset!("U005E.svg"),
    loop_svg_asset!("U005F.svg"),
    loop_svg_asset!("U0060.svg"),
    loop_svg_asset!("U007B.svg"),
    loop_svg_asset!("U007C.svg"),
    loop_svg_asset!("U007D.svg"),
    loop_svg_asset!("U007E.svg"),
    loop_svg_asset!("U00A7.svg"),
    loop_svg_asset!("a.svg"),
    loop_svg_asset!("aa.svg"),
    loop_svg_asset!("b.svg"),
    loop_svg_asset!("bb.svg"),
    loop_svg_asset!("c.svg"),
    loop_svg_asset!("cc.svg"),
    loop_svg_asset!("d.svg"),
    loop_svg_asset!("dd.svg"),
    loop_svg_asset!("e.svg"),
    loop_svg_asset!("ee.svg"),
    loop_svg_asset!("f.svg"),
    loop_svg_asset!("ff.svg"),
    loop_svg_asset!("g.svg"),
    loop_svg_asset!("gg.svg"),
    loop_svg_asset!("h.svg"),
    loop_svg_asset!("hh.svg"),
    loop_svg_asset!("i.svg"),
    loop_svg_asset!("ii.svg"),
    loop_svg_asset!("j.svg"),
    loop_svg_asset!("jj.svg"),
    loop_svg_asset!("k.svg"),
    loop_svg_asset!("kk.svg"),
    loop_svg_asset!("l.svg"),
    loop_svg_asset!("ll.svg"),
    loop_svg_asset!("m.svg"),
    loop_svg_asset!("mm.svg"),
    loop_svg_asset!("n.svg"),
    loop_svg_asset!("nn.svg"),
    loop_svg_asset!("o.svg"),
    loop_svg_asset!("oo.svg"),
    loop_svg_asset!("p.svg"),
    loop_svg_asset!("pp.svg"),
    loop_svg_asset!("q.svg"),
    loop_svg_asset!("qq.svg"),
    loop_svg_asset!("r.svg"),
    loop_svg_asset!("rr.svg"),
    loop_svg_asset!("s.svg"),
    loop_svg_asset!("ss.svg"),
    loop_svg_asset!("t.svg"),
    loop_svg_asset!("tt.svg"),
    loop_svg_asset!("u.svg"),
    loop_svg_asset!("uu.svg"),
    loop_svg_asset!("v.svg"),
    loop_svg_asset!("vv.svg"),
    loop_svg_asset!("w.svg"),
    loop_svg_asset!("ww.svg"),
    loop_svg_asset!("x.svg"),
    loop_svg_asset!("xx.svg"),
    loop_svg_asset!("y.svg"),
    loop_svg_asset!("yy.svg"),
    loop_svg_asset!("z.svg"),
    loop_svg_asset!("zz.svg"),
];

#[inline]
fn row_color(size_px: u32) -> (u8, u8, u8) {
    match size_px {
        8 => (0x20, 0x2A, 0x36),
        12 => (0x2E, 0x46, 0x7A),
        16 => (0x2E, 0x6C, 0x60),
        20 => (0x7F, 0x57, 0x1D),
        24 => (0x8A, 0x3B, 0x4A),
        28 => (0x5F, 0x39, 0x84),
        _ => (0x15, 0x15, 0x15),
    }
}

fn fill_rgba_rect(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    rect_w: u32,
    rect_h: u32,
    rgba: [u8; 4],
) {
    let x1 = x.saturating_add(rect_w).min(width);
    let y1 = y.saturating_add(rect_h).min(height);
    for py in y..y1 {
        for px in x..x1 {
            let idx = ((py as usize)
                .saturating_mul(width as usize)
                .saturating_add(px as usize))
            .saturating_mul(4);
            if idx + 3 >= pixels.len() {
                continue;
            }
            pixels[idx] = rgba[0];
            pixels[idx + 1] = rgba[1];
            pixels[idx + 2] = rgba[2];
            pixels[idx + 3] = rgba[3];
        }
    }
}

fn alpha_blit_rgba(
    dst: &mut [u8],
    dst_w: u32,
    dst_h: u32,
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_x: i32,
    dst_y: i32,
) {
    for sy in 0..src_h as usize {
        let py = dst_y + sy as i32;
        if py < 0 || py >= dst_h as i32 {
            continue;
        }
        for sx in 0..src_w as usize {
            let px = dst_x + sx as i32;
            if px < 0 || px >= dst_w as i32 {
                continue;
            }

            let src_idx = (sy.saturating_mul(src_w as usize).saturating_add(sx)).saturating_mul(4);
            let dst_idx = ((py as usize)
                .saturating_mul(dst_w as usize)
                .saturating_add(px as usize))
            .saturating_mul(4);
            if src_idx + 3 >= src.len() || dst_idx + 3 >= dst.len() {
                continue;
            }

            let src_a = src[src_idx + 3] as u32;
            if src_a == 0 {
                continue;
            }
            let inv = 255u32.saturating_sub(src_a);
            dst[dst_idx] = (((src[src_idx] as u32) * src_a + (dst[dst_idx] as u32) * inv + 127)
                / 255)
                .min(255) as u8;
            dst[dst_idx + 1] =
                (((src[src_idx + 1] as u32) * src_a + (dst[dst_idx + 1] as u32) * inv + 127) / 255)
                    .min(255) as u8;
            dst[dst_idx + 2] =
                (((src[src_idx + 2] as u32) * src_a + (dst[dst_idx + 2] as u32) * inv + 127) / 255)
                    .min(255) as u8;
            dst[dst_idx + 3] =
                (src_a + (((dst[dst_idx + 3] as u32) * inv + 127) / 255)).min(255) as u8;
        }
    }
}

fn wrap_svg_for_size(bytes: &[u8], side_px: u32, rgb: (u8, u8, u8)) -> Option<String> {
    let text = core::str::from_utf8(bytes).ok()?;
    let body_start = text.find('>')?.saturating_add(1);
    let body_end = text.rfind("</svg>")?;
    let mut body = String::from(&text[body_start..body_end]);
    if body.contains("fill=\"black\"") {
        let fill = format!("fill=\"#{:02X}{:02X}{:02X}\"", rgb.0, rgb.1, rgb.2);
        body = body.replace("fill=\"black\"", fill.as_str());
    }
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 1 1\" preserveAspectRatio=\"xMidYMid meet\">{}</svg>",
        side_px, side_px, body
    ))
}

fn rasterize_loop_svg(bytes: &[u8], side_px: u32, rgb: (u8, u8, u8)) -> Option<Vec<u8>> {
    let wrapped = wrap_svg_for_size(bytes, side_px, rgb)?;
    let (_, rgba) = crate::gfx::svg::rasterize_svg_text_rgba(wrapped.as_str()).ok()?;
    Some(rgba)
}

fn build_demo_pixels() -> (u32, u32, Vec<u8>) {
    let sizes = [8u32, 12, 16, 20, 24, 28, 32];
    let max_size = *sizes.last().unwrap_or(&32);
    let row_width = UI2_IMBA_ATHLAS_DEMO_LABEL_W
        + (IMBASVG_LOOP_ASSETS.len() as u32)
            .saturating_mul(max_size + UI2_IMBA_ATHLAS_DEMO_COL_GAP)
        + UI2_IMBA_ATHLAS_DEMO_COL_GAP;
    let content_w = UI2_IMBA_ATHLAS_DEMO_MARGIN_X * 2 + row_width;
    let row_block_h = max_size + UI2_IMBA_ATHLAS_DEMO_ROW_GAP + 12;
    let content_h = UI2_IMBA_ATHLAS_DEMO_MARGIN_Y * 2
        + 28
        + (sizes.len() as u32).saturating_mul(row_block_h)
        + UI2_IMBA_ATHLAS_DEMO_ROW_GAP;
    let mut pixels = vec![
        0u8;
        (content_w as usize)
            .saturating_mul(content_h as usize)
            .saturating_mul(4)
    ];

    fill_rgba_rect(
        &mut pixels,
        content_w,
        content_h,
        0,
        0,
        content_w,
        content_h,
        UI2_IMBA_ATHLAS_DEMO_BG,
    );

    let title = b"imba_athlas loop sizes 8..32 step 4";
    let _ = crate::gfx::imba_athlas::blit_imba_athlas_text_rgba(
        &mut pixels,
        content_w,
        content_h,
        title,
        UI2_IMBA_ATHLAS_DEMO_MARGIN_X as i32,
        8,
        (0x23, 0x1F, 0x19, 0xFF),
    );

    for (row_index, size_px) in sizes.iter().copied().enumerate() {
        let color = row_color(size_px);
        let top = UI2_IMBA_ATHLAS_DEMO_MARGIN_Y + 28 + row_index as u32 * row_block_h;
        fill_rgba_rect(
            &mut pixels,
            content_w,
            content_h,
            UI2_IMBA_ATHLAS_DEMO_MARGIN_X,
            top.saturating_sub(4),
            content_w.saturating_sub(UI2_IMBA_ATHLAS_DEMO_MARGIN_X * 2),
            size_px + 10,
            [0xFF, 0xFF, 0xFF, 0x78],
        );
        let label = format!("{}px", size_px);
        let _ = crate::gfx::imba_athlas::blit_imba_athlas_text_rgba(
            &mut pixels,
            content_w,
            content_h,
            label.as_bytes(),
            UI2_IMBA_ATHLAS_DEMO_MARGIN_X as i32,
            top as i32,
            (color.0, color.1, color.2, 0xFF),
        );

        let mut pen_x = UI2_IMBA_ATHLAS_DEMO_MARGIN_X + UI2_IMBA_ATHLAS_DEMO_LABEL_W;
        for asset in IMBASVG_LOOP_ASSETS {
            if let Some(rgba) = rasterize_loop_svg(asset.bytes, size_px, color) {
                alpha_blit_rgba(
                    &mut pixels,
                    content_w,
                    content_h,
                    rgba.as_slice(),
                    size_px,
                    size_px,
                    pen_x as i32,
                    top as i32,
                );
            }
            pen_x = pen_x.saturating_add(size_px + UI2_IMBA_ATHLAS_DEMO_COL_GAP);
        }
    }

    (content_w, content_h, pixels)
}

#[embassy_executor::task]
pub async fn ui2_imba_athlas_demo_task() {
    let (content_w, content_h, pixels) = build_demo_pixels();
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::new(
        "Imba Athlas Loop Demo",
        crate::r::ui2::Ui2Rect {
            x: UI2_IMBA_ATHLAS_DEMO_WINDOW_X,
            y: UI2_IMBA_ATHLAS_DEMO_WINDOW_Y,
            w: content_w as f32,
            h: content_h as f32,
        },
        UI2_IMBA_ATHLAS_DEMO_WINDOW_Z,
        255,
        UI2_IMBA_ATHLAS_DEMO_TEX_ID,
        true,
        UI2_IMBA_ATHLAS_DEMO_BG,
    ) else {
        return;
    };

    let window_id = surface.window_id();
    let _ = surface.upload_rgba(pixels.as_slice(), "ui2-imba-athlas-demo");
    crate::log!(
        "ui2-imba-athlas-demo: window={} tex={} size={}x{} assets={}\n",
        window_id,
        surface.tex_id(),
        content_w,
        content_h,
        IMBASVG_LOOP_ASSETS.len()
    );

    loop {
        Timer::after(EmbassyDuration::from_secs(3600)).await;
    }
}
