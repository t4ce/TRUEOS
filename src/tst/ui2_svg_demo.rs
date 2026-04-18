use alloc::{format, vec::Vec};

use embassy_time::{Duration as EmbassyDuration, Timer};

const UI2_SVG_DEMO_TEX_ID: u32 = crate::tst_ui2_ids::Ui2DemoTexId::Svg.get();
const UI2_SVG_DEMO_WINDOW_X: f32 = 980.0;
const UI2_SVG_DEMO_WINDOW_Y: f32 = 96.0;
const UI2_SVG_DEMO_WINDOW_Z: i16 = 35;
const UI2_SVG_DEMO_GRID_COLS: usize = 4;
const UI2_SVG_DEMO_ICON_SIZE: u32 = 64;
const UI2_SVG_DEMO_GRID_GAP: u32 = 4;
const UI2_SVG_DEMO_GRID_PAD: u32 = 4;
const UI2_SVG_DEMO_BG_RGBA: [u8; 4] = [0x0A, 0x0E, 0x14, 0xFF];
const UI2_SVG_DEMO_TILE_RGBA: [u8; 4] = [0x14, 0x19, 0x22, 0xFF];
const UI2_SVG_DEMO_TILE_BORDER_RGBA: [u8; 4] = [0x25, 0x2C, 0x38, 0xFF];

#[derive(Copy, Clone)]
struct SvgDemoAsset {
    name: &'static str,
    svg: &'static str,
}

fn svg_demo_grid_extent(count: usize) -> (u32, u32) {
    let cols = UI2_SVG_DEMO_GRID_COLS.max(1);
    let rows = ((count.max(1) + cols - 1) / cols) as u32;
    let cols_u32 = cols as u32;
    let width = UI2_SVG_DEMO_GRID_PAD.saturating_mul(2)
        + cols_u32.saturating_mul(UI2_SVG_DEMO_ICON_SIZE)
        + cols_u32
            .saturating_sub(1)
            .saturating_mul(UI2_SVG_DEMO_GRID_GAP);
    let height = UI2_SVG_DEMO_GRID_PAD.saturating_mul(2)
        + rows.saturating_mul(UI2_SVG_DEMO_ICON_SIZE)
        + rows.saturating_sub(1).saturating_mul(UI2_SVG_DEMO_GRID_GAP);
    (width.max(1), height.max(1))
}

fn fill_rgba(rgba: &mut [u8], color: [u8; 4]) {
    for chunk in rgba.chunks_exact_mut(4) {
        chunk.copy_from_slice(&color);
    }
}

fn put_rgba_pixel(rgba: &mut [u8], width: u32, x: u32, y: u32, color: [u8; 4]) {
    let idx = ((y as usize)
        .saturating_mul(width as usize)
        .saturating_add(x as usize))
    .saturating_mul(4);
    if idx + 4 <= rgba.len() {
        rgba[idx..idx + 4].copy_from_slice(&color);
    }
}

fn paint_tile(rgba: &mut [u8], width: u32, height: u32, x: u32, y: u32, w: u32, h: u32) {
    for py in 0..h {
        let dst_y = y.saturating_add(py);
        if dst_y >= height {
            break;
        }
        for px in 0..w {
            let dst_x = x.saturating_add(px);
            if dst_x >= width {
                break;
            }
            let edge = px == 0 || py == 0 || px + 1 == w || py + 1 == h;
            put_rgba_pixel(
                rgba,
                width,
                dst_x,
                dst_y,
                if edge {
                    UI2_SVG_DEMO_TILE_BORDER_RGBA
                } else {
                    UI2_SVG_DEMO_TILE_RGBA
                },
            );
        }
    }
}

fn blend_rgba_over(dst: &mut [u8], src: [u8; 4]) {
    let src_a = src[3] as u32;
    if src_a == 0 {
        return;
    }
    let inv_a = 255u32.saturating_sub(src_a);
    let dst_a = dst[3] as u32;
    dst[0] = (((src[0] as u32).saturating_mul(src_a) + (dst[0] as u32).saturating_mul(inv_a) + 127)
        / 255) as u8;
    dst[1] = (((src[1] as u32).saturating_mul(src_a) + (dst[1] as u32).saturating_mul(inv_a) + 127)
        / 255) as u8;
    dst[2] = (((src[2] as u32).saturating_mul(src_a) + (dst[2] as u32).saturating_mul(inv_a) + 127)
        / 255) as u8;
    dst[3] = (src_a + ((dst_a.saturating_mul(inv_a) + 127) / 255)).min(255) as u8;
}

fn blit_scaled_rgba_fit(
    dst: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    box_x: u32,
    box_y: u32,
    box_w: u32,
    box_h: u32,
    src: &[u8],
    src_width: u32,
    src_height: u32,
) {
    if src_width == 0 || src_height == 0 || box_w == 0 || box_h == 0 {
        return;
    }

    let (draw_w, draw_h) = if src_width >= src_height {
        (
            box_w,
            ((src_height as u64)
                .saturating_mul(box_w as u64)
                .checked_div(src_width as u64)
                .unwrap_or(0)
                .max(1)) as u32,
        )
    } else {
        (
            ((src_width as u64)
                .saturating_mul(box_h as u64)
                .checked_div(src_height as u64)
                .unwrap_or(0)
                .max(1)) as u32,
            box_h,
        )
    };

    let offset_x = box_x.saturating_add(box_w.saturating_sub(draw_w) / 2);
    let offset_y = box_y.saturating_add(box_h.saturating_sub(draw_h) / 2);

    for dy in 0..draw_h {
        let dst_y = offset_y.saturating_add(dy);
        if dst_y >= dst_height {
            break;
        }
        let src_y = ((dy as u64)
            .saturating_mul(src_height as u64)
            .checked_div(draw_h as u64)
            .unwrap_or(0)) as u32;
        for dx in 0..draw_w {
            let dst_x = offset_x.saturating_add(dx);
            if dst_x >= dst_width {
                break;
            }
            let src_x = ((dx as u64)
                .saturating_mul(src_width as u64)
                .checked_div(draw_w as u64)
                .unwrap_or(0)) as u32;
            let src_idx = ((src_y as usize)
                .saturating_mul(src_width as usize)
                .saturating_add(src_x as usize))
            .saturating_mul(4);
            let dst_idx = ((dst_y as usize)
                .saturating_mul(dst_width as usize)
                .saturating_add(dst_x as usize))
            .saturating_mul(4);
            if src_idx + 4 > src.len() || dst_idx + 4 > dst.len() {
                continue;
            }
            blend_rgba_over(
                &mut dst[dst_idx..dst_idx + 4],
                [
                    src[src_idx],
                    src[src_idx + 1],
                    src[src_idx + 2],
                    src[src_idx + 3],
                ],
            );
        }
    }
}

fn compose_svg_demo_grid_rgba(width: u32, height: u32) -> (Vec<u8>, usize) {
    let mut rgba = vec![
        0u8;
        (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
    ];
    fill_rgba(rgba.as_mut_slice(), UI2_SVG_DEMO_BG_RGBA);

    let mut rendered = 0usize;
    for (idx, asset) in SVG_DEMO_ASSETS.iter().enumerate() {
        let col = (idx % UI2_SVG_DEMO_GRID_COLS) as u32;
        let row = (idx / UI2_SVG_DEMO_GRID_COLS) as u32;
        let tile_x = UI2_SVG_DEMO_GRID_PAD
            + col.saturating_mul(UI2_SVG_DEMO_ICON_SIZE + UI2_SVG_DEMO_GRID_GAP);
        let tile_y = UI2_SVG_DEMO_GRID_PAD
            + row.saturating_mul(UI2_SVG_DEMO_ICON_SIZE + UI2_SVG_DEMO_GRID_GAP);
        paint_tile(
            rgba.as_mut_slice(),
            width,
            height,
            tile_x,
            tile_y,
            UI2_SVG_DEMO_ICON_SIZE,
            UI2_SVG_DEMO_ICON_SIZE,
        );
        match crate::gfx::svg::rasterize_svg_text_rgba(asset.svg) {
            Ok((info, pixels)) => {
                blit_scaled_rgba_fit(
                    rgba.as_mut_slice(),
                    width,
                    height,
                    tile_x,
                    tile_y,
                    UI2_SVG_DEMO_ICON_SIZE,
                    UI2_SVG_DEMO_ICON_SIZE,
                    pixels.as_slice(),
                    info.width,
                    info.height,
                );
                rendered = rendered.saturating_add(1);
            }
            Err(code) => {
                crate::log!("ui2-svg-demo: rasterize failed name={} code={}\n", asset.name, code);
            }
        }
    }

    (rgba, rendered)
}

#[embassy_executor::task]
pub async fn ui2_svg_demo_task() {
    Timer::after(EmbassyDuration::from_millis(250)).await;

    let (surface_w, surface_h) = svg_demo_grid_extent(SVG_DEMO_ASSETS.len());
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::new(
        "SVG Demo Grid",
        crate::r::ui2::Ui2Rect {
            x: UI2_SVG_DEMO_WINDOW_X,
            y: UI2_SVG_DEMO_WINDOW_Y,
            w: surface_w as f32,
            h: surface_h as f32,
        },
        UI2_SVG_DEMO_WINDOW_Z,
        220,
        UI2_SVG_DEMO_TEX_ID,
        false,
        UI2_SVG_DEMO_BG_RGBA,
    ) else {
        crate::log!("ui2-svg-demo: window creation failed tex={}\n", UI2_SVG_DEMO_TEX_ID);
        return;
    };

    let (rgba, rendered) = compose_svg_demo_grid_rgba(surface_w, surface_h);
    if !surface.upload_rgba(rgba.as_slice(), "ui2-svg-demo-upload") {
        crate::log!(
            "ui2-svg-demo: upload failed window={} tex={} size={}x{}\n",
            surface.window_id(),
            surface.tex_id(),
            surface_w,
            surface_h
        );
        return;
    }

    let title = format!("SVG Demo Grid ({}/{})", rendered, SVG_DEMO_ASSETS.len());
    let _ = crate::r::ui2::set_window_title(surface.window_id(), title.as_str());
    crate::log!(
        "ui2-svg-demo: window={} tex={} size={}x{} rendered={}/{}\n",
        surface.window_id(),
        surface.tex_id(),
        surface_w,
        surface_h,
        rendered,
        SVG_DEMO_ASSETS.len()
    );

    loop {
        Timer::after(EmbassyDuration::from_secs(3600)).await;
    }
}

const SVG_SUNRISE_LAYERS: SvgDemoAsset = SvgDemoAsset {
    name: "sunrise_layers",
    svg: r##"<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#132a4f"/>
      <stop offset="55%" stop-color="#f26b5b"/>
      <stop offset="100%" stop-color="#ffd27a"/>
    </linearGradient>
    <radialGradient id="sun" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="#fff3bf"/>
      <stop offset="100%" stop-color="#ff9f43"/>
    </radialGradient>
  </defs>
  <rect width="96" height="96" fill="url(#sky)"/>
  <circle cx="48" cy="38" r="18" fill="url(#sun)"/>
  <path d="M0 64 C10 58 20 56 32 60 C42 63 54 66 66 62 C78 58 87 59 96 64 L96 96 L0 96 Z" fill="#553c66"/>
  <path d="M0 74 C10 70 20 67 32 70 C42 73 56 76 70 72 C82 68 90 69 96 72 L96 96 L0 96 Z" fill="#2c2348"/>
  <path d="M0 84 C12 80 23 78 34 81 C46 84 58 87 70 84 C81 81 90 82 96 84 L96 96 L0 96 Z" fill="#161126"/>
</svg>"##,
};

const SVG_RIBBON_FLOWER: SvgDemoAsset = SvgDemoAsset {
    name: "ribbon_flower",
    svg: r##"<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="petal" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#ff8fb1"/>
      <stop offset="100%" stop-color="#ff4d6d"/>
    </linearGradient>
    <radialGradient id="core" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="#fff4b5"/>
      <stop offset="100%" stop-color="#ffb703"/>
    </radialGradient>
  </defs>
  <rect width="96" height="96" fill="#fff7ef"/>
  <g fill="url(#petal)" stroke="#7a284a" stroke-width="2" stroke-linejoin="round">
    <path d="M48 18 C60 22 66 31 66 42 C58 45 52 45 48 42 C44 45 38 45 30 42 C30 31 36 22 48 18 Z"/>
    <path d="M78 48 C74 60 65 66 54 66 C51 58 51 52 54 48 C51 44 51 38 54 30 C65 30 74 36 78 48 Z"/>
    <path d="M48 78 C36 74 30 65 30 54 C38 51 44 51 48 54 C52 51 58 51 66 54 C66 65 60 74 48 78 Z"/>
    <path d="M18 48 C22 36 31 30 42 30 C45 38 45 44 42 48 C45 52 45 58 42 66 C31 66 22 60 18 48 Z"/>
  </g>
  <circle cx="48" cy="48" r="10" fill="url(#core)" stroke="#8c5a00" stroke-width="2"/>
</svg>"##,
};

const SVG_RADAR_PING: SvgDemoAsset = SvgDemoAsset {
    name: "radar_ping",
    svg: r##"<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <radialGradient id="glow" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="#8ff7c8" stop-opacity="0.95"/>
      <stop offset="100%" stop-color="#0d3b2a" stop-opacity="0.15"/>
    </radialGradient>
  </defs>
  <rect width="96" height="96" rx="12" fill="#091a16"/>
  <circle cx="48" cy="48" r="28" fill="url(#glow)"/>
  <circle cx="48" cy="48" r="12" fill="none" stroke="#7df9c1" stroke-width="2"/>
  <circle cx="48" cy="48" r="24" fill="none" stroke="#4dd9a6" stroke-width="2" stroke-opacity="0.8"/>
  <circle cx="48" cy="48" r="36" fill="none" stroke="#2ca67f" stroke-width="2" stroke-opacity="0.6"/>
  <path d="M48 48 L76 34 A32 32 0 0 1 80 48 Z" fill="#8ff7c8" fill-opacity="0.35"/>
  <path d="M48 14 L48 82 M14 48 L82 48" stroke="#74e7b7" stroke-width="1.5" stroke-linecap="round"/>
  <circle cx="48" cy="48" r="4" fill="#d7fff0"/>
</svg>"##,
};

const SVG_CANDY_BADGE: SvgDemoAsset = SvgDemoAsset {
    name: "candy_badge",
    svg: r##"<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="shell" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#8ec5ff"/>
      <stop offset="100%" stop-color="#2d7ff9"/>
    </linearGradient>
    <linearGradient id="spark" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#ffffff" stop-opacity="0.95"/>
      <stop offset="100%" stop-color="#ffffff" stop-opacity="0"/>
    </linearGradient>
  </defs>
  <rect width="96" height="96" fill="#f3f8ff"/>
  <path d="M48 14 L76 28 L76 62 C76 74 64 82 48 86 C32 82 20 74 20 62 L20 28 Z" fill="url(#shell)" stroke="#14439a" stroke-width="3" stroke-linejoin="round"/>
  <path d="M48 26 L66 35 L66 58 C66 66 58 72 48 75 C38 72 30 66 30 58 L30 35 Z" fill="#e9f3ff" fill-opacity="0.35"/>
  <path d="M34 28 C42 24 50 24 58 28 C50 31 42 37 36 48 C33 42 32 35 34 28 Z" fill="url(#spark)"/>
  <path d="M34 54 C38 49 43 46 48 46 C53 46 58 49 62 54 C58 60 53 64 48 66 C43 64 38 60 34 54 Z M43 54 C45 52 46 51 48 51 C50 51 51 52 53 54 C51 56 50 57 48 59 C46 57 45 56 43 54 Z" fill="#ffffff" fill-rule="evenodd"/>
</svg>"##,
};

const SVG_WAVE_TILES: SvgDemoAsset = SvgDemoAsset {
    name: "wave_tiles",
    svg: r##"<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#132238"/>
      <stop offset="100%" stop-color="#214d6b"/>
    </linearGradient>
    <linearGradient id="waveA" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0%" stop-color="#6ee7f9"/>
      <stop offset="100%" stop-color="#3b82f6"/>
    </linearGradient>
    <linearGradient id="waveB" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0%" stop-color="#f9a8d4"/>
      <stop offset="100%" stop-color="#f97316"/>
    </linearGradient>
  </defs>
  <rect width="96" height="96" rx="14" fill="url(#bg)"/>
  <path d="M8 28 C20 16 34 16 46 28 C58 40 72 40 88 28" fill="none" stroke="url(#waveA)" stroke-width="8" stroke-linecap="round"/>
  <path d="M8 48 C20 36 34 36 46 48 C58 60 72 60 88 48" fill="none" stroke="url(#waveB)" stroke-width="8" stroke-linecap="round"/>
  <path d="M8 68 C20 56 34 56 46 68 C58 80 72 80 88 68" fill="none" stroke="url(#waveA)" stroke-width="8" stroke-linecap="round"/>
  <circle cx="20" cy="78" r="4" fill="#f8fafc"/>
  <circle cx="48" cy="18" r="3" fill="#f8fafc" fill-opacity="0.8"/>
  <circle cx="76" cy="78" r="4" fill="#f8fafc"/>
</svg>"##,
};

const SVG_COMET_LOOP: SvgDemoAsset = SvgDemoAsset {
    name: "comet_loop",
    svg: r##"<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <radialGradient id="head" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="#fff6d6"/>
      <stop offset="100%" stop-color="#ffb347"/>
    </radialGradient>
  </defs>
  <rect width="96" height="96" fill="#090b1a"/>
  <path d="M20 72 C16 54 20 34 34 24 C46 16 62 16 72 24 C82 32 82 48 72 56 C62 64 46 64 34 56 C24 49 24 38 32 32 C39 27 49 27 56 32" fill="none" stroke="#7dd3fc" stroke-width="5" stroke-linecap="round" stroke-linejoin="round"/>
  <path d="M18 76 C30 68 42 64 54 64 C44 70 32 78 24 88 Z" fill="#7dd3fc" fill-opacity="0.35"/>
  <circle cx="58" cy="34" r="8" fill="url(#head)" stroke="#ffedd5" stroke-width="1.5"/>
  <circle cx="70" cy="22" r="2" fill="#ffffff"/>
  <circle cx="78" cy="30" r="1.5" fill="#ffffff" fill-opacity="0.8"/>
</svg>"##,
};

const SVG_WEATHER_SUN: SvgDemoAsset = SvgDemoAsset {
    name: "weather_sun",
    svg: r##"<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <radialGradient id="sunCore" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="#fff7cc"/>
      <stop offset="100%" stop-color="#ffb703"/>
    </radialGradient>
  </defs>
  <rect width="96" height="96" rx="18" fill="#e6f6ff"/>
  <circle cx="48" cy="48" r="18" fill="url(#sunCore)" stroke="#d97706" stroke-width="2.5"/>
  <path d="M48 10 L48 22 M48 74 L48 86 M10 48 L22 48 M74 48 L86 48 M21 21 L29 29 M67 67 L75 75 M21 75 L29 67 M67 29 L75 21" stroke="#f59e0b" stroke-width="4" stroke-linecap="round"/>
</svg>"##,
};

const SVG_WEATHER_PARTLY_CLOUDY: SvgDemoAsset = SvgDemoAsset {
    name: "weather_partly_cloudy",
    svg: r##"<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <radialGradient id="smallSun" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0%" stop-color="#fff4bf"/>
      <stop offset="100%" stop-color="#f59e0b"/>
    </radialGradient>
    <linearGradient id="cloud" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#f8fbff"/>
      <stop offset="100%" stop-color="#cbdcf2"/>
    </linearGradient>
  </defs>
  <rect width="96" height="96" rx="18" fill="#dff3ff"/>
  <circle cx="34" cy="32" r="14" fill="url(#smallSun)" stroke="#d97706" stroke-width="2"/>
  <path d="M34 10 L34 16 M34 48 L34 54 M12 32 L18 32 M50 32 L56 32 M18 18 L22 22 M46 42 L50 46 M18 46 L22 42 M46 22 L50 18" stroke="#f59e0b" stroke-width="3" stroke-linecap="round"/>
  <path d="M28 62 C28 53 35 46 44 46 C47 46 50 47 53 49 C56 42 63 38 71 38 C82 38 90 47 90 58 C90 69 82 78 71 78 L44 78 C35 78 28 71 28 62 Z" fill="url(#cloud)" stroke="#7b93b7" stroke-width="2.5" stroke-linejoin="round"/>
</svg>"##,
};

const SVG_WEATHER_CLOUDY: SvgDemoAsset = SvgDemoAsset {
    name: "weather_cloudy",
    svg: r##"<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="cloudBack" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#e7eef8"/>
      <stop offset="100%" stop-color="#b8c6d9"/>
    </linearGradient>
    <linearGradient id="cloudFront" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#ffffff"/>
      <stop offset="100%" stop-color="#d9e5f3"/>
    </linearGradient>
  </defs>
  <rect width="96" height="96" rx="18" fill="#eaf2f8"/>
  <path d="M18 56 C18 48 24 42 32 42 C35 42 38 43 40 45 C43 39 49 35 56 35 C66 35 74 43 74 53 C74 63 66 71 56 71 L32 71 C24 71 18 64 18 56 Z" fill="url(#cloudBack)" stroke="#8a9aad" stroke-width="2"/>
  <path d="M28 62 C28 53 35 46 44 46 C47 46 50 47 53 49 C56 42 63 38 71 38 C82 38 90 47 90 58 C90 69 82 78 71 78 L44 78 C35 78 28 71 28 62 Z" fill="url(#cloudFront)" stroke="#7b93b7" stroke-width="2.5"/>
</svg>"##,
};

const SVG_WEATHER_RAIN: SvgDemoAsset = SvgDemoAsset {
    name: "weather_rain",
    svg: r##"<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="rainCloud" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#f7fbff"/>
      <stop offset="100%" stop-color="#d2ddea"/>
    </linearGradient>
    <linearGradient id="drop" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#7dd3fc"/>
      <stop offset="100%" stop-color="#2563eb"/>
    </linearGradient>
  </defs>
  <rect width="96" height="96" rx="18" fill="#edf6ff"/>
  <path d="M22 52 C22 43 29 36 38 36 C41 36 45 37 48 39 C51 32 58 28 66 28 C77 28 86 37 86 48 C86 60 77 69 66 69 L38 69 C29 69 22 61 22 52 Z" fill="url(#rainCloud)" stroke="#7b93b7" stroke-width="2.5"/>
  <path d="M34 74 C36 68 39 64 42 60 C45 64 48 68 50 74 C50 78 46 82 42 82 C38 82 34 78 34 74 Z" fill="url(#drop)"/>
  <path d="M50 80 C52 74 55 70 58 66 C61 70 64 74 66 80 C66 84 62 88 58 88 C54 88 50 84 50 80 Z" fill="url(#drop)"/>
  <path d="M66 74 C68 68 71 64 74 60 C77 64 80 68 82 74 C82 78 78 82 74 82 C70 82 66 78 66 74 Z" fill="url(#drop)"/>
</svg>"##,
};

const SVG_WEATHER_THUNDER: SvgDemoAsset = SvgDemoAsset {
    name: "weather_thunder",
    svg: r##"<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="stormCloud" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#dde5f1"/>
      <stop offset="100%" stop-color="#97a7bd"/>
    </linearGradient>
    <linearGradient id="bolt" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#fff3a3"/>
      <stop offset="100%" stop-color="#facc15"/>
    </linearGradient>
  </defs>
  <rect width="96" height="96" rx="18" fill="#e8edf5"/>
  <path d="M20 50 C20 41 27 34 36 34 C40 34 43 35 46 37 C49 30 56 26 64 26 C76 26 86 36 86 48 C86 60 76 70 64 70 L36 70 C27 70 20 62 20 50 Z" fill="url(#stormCloud)" stroke="#6f8197" stroke-width="2.5"/>
  <path d="M52 48 L42 66 L50 66 L44 86 L66 60 L56 60 L64 48 Z" fill="url(#bolt)" stroke="#ca8a04" stroke-width="2" stroke-linejoin="round"/>
</svg>"##,
};

const SVG_WEATHER_SNOW: SvgDemoAsset = SvgDemoAsset {
    name: "weather_snow",
    svg: r##"<svg width="96" height="96" viewBox="0 0 96 96" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="snowCloud" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0%" stop-color="#f9fcff"/>
      <stop offset="100%" stop-color="#d7e2ef"/>
    </linearGradient>
  </defs>
  <rect width="96" height="96" rx="18" fill="#eef7ff"/>
  <path d="M22 50 C22 41 29 34 38 34 C41 34 45 35 48 37 C51 30 58 26 66 26 C77 26 86 35 86 46 C86 58 77 67 66 67 L38 67 C29 67 22 59 22 50 Z" fill="url(#snowCloud)" stroke="#88a0bb" stroke-width="2.5"/>
  <path d="M34 76 L42 76 M38 72 L38 80 M35 73 L41 79 M41 73 L35 79" stroke="#67b7ff" stroke-width="2.5" stroke-linecap="round"/>
  <path d="M54 84 L62 84 M58 80 L58 88 M55 81 L61 87 M61 81 L55 87" stroke="#67b7ff" stroke-width="2.5" stroke-linecap="round"/>
  <path d="M70 76 L78 76 M74 72 L74 80 M71 73 L77 79 M77 73 L71 79" stroke="#67b7ff" stroke-width="2.5" stroke-linecap="round"/>
</svg>"##,
};

const SVG_MJS_LOGO: SvgDemoAsset = SvgDemoAsset {
    name: "mjs_logo",
    svg: r##"<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96" viewBox="0 0 96 96">
  <rect width="96" height="96" rx="16" fill="#E6D34D"/>
  <path fill="#2F2F2F" d="M10 72 V24 H22 L30 47 L38 24 H50 V72 H42 V38 L33 63 H27 L18 38 V72 Z"/>
  <path fill="#2F2F2F" d="M54 24 H82 V32 H70 V72 H62 V32 H54 Z"/>
  <path fill="#2F2F2F" d="M52 60 C52 69 46 74 36 74 H30 V66 H35 C40 66 44 64 44 58 V24 H52 Z"/>
  <path fill="#2F2F2F" d="M68 56 C68 63 73 66 80 66 C84 66 88 65 88 61 C88 57 83 56 76 54 C67 52 58 49 58 39 C58 29 66 22 79 22 C87 22 93 24 95 26 L92 34 C89 32 84 30 79 30 C72 30 67 32 67 37 C67 41 71 42 79 44 C89 46 97 49 97 59 C97 69 89 74 79 74 C69 74 61 70 58 66 L63 59 C66 62 72 66 79 66 C85 66 89 64 89 59 C89 55 85 53 76 51 C68 49 60 46 60 37 C60 27 68 22 79 22" transform="translate(-1 0) scale(0.92 1)"/>
</svg>"##,
};

const SVG_RUST_LOGO: SvgDemoAsset = SvgDemoAsset {
    name: "rust_logo",
    svg: r##"<svg version="1.1" height="106" width="106" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
<g id="logo" transform="translate(53, 53)">
  <path id="r" transform="translate(0.5, 0.5)" stroke="black" stroke-width="1" stroke-linejoin="round" d="     M -9,-15 H 4 C 12,-15 12,-7 4,-7 H -9 Z     M -40,22 H 0 V 11 H -9 V 3 H 1 C 12,3 6,22 15,22 H 40     V 3 H 34 V 5 C 34,13 25,12 24,7 C 23,2 19,-2 18,-2 C 33,-10 24,-26 12,-26 H -35     V -15 H -25 V 11 H -40 Z"/>
  <g id="gear" mask="url(#holes)">
    <circle r="43" fill="none" stroke="black" stroke-width="9"/>
    <g id="cogs">
      <polygon id="cog" stroke="black" stroke-width="3" stroke-linejoin="round" points="46,3 51,0 46,-3"/>
      <use xlink:href="#cog" transform="rotate(11.25)"/>
      <use xlink:href="#cog" transform="rotate(22.50)"/>
      <use xlink:href="#cog" transform="rotate(33.75)"/>
      <use xlink:href="#cog" transform="rotate(45.00)"/>
      <use xlink:href="#cog" transform="rotate(56.25)"/>
      <use xlink:href="#cog" transform="rotate(67.50)"/>
      <use xlink:href="#cog" transform="rotate(78.75)"/>
      <use xlink:href="#cog" transform="rotate(90.00)"/>
      <use xlink:href="#cog" transform="rotate(101.25)"/>
      <use xlink:href="#cog" transform="rotate(112.50)"/>
      <use xlink:href="#cog" transform="rotate(123.75)"/>
      <use xlink:href="#cog" transform="rotate(135.00)"/>
      <use xlink:href="#cog" transform="rotate(146.25)"/>
      <use xlink:href="#cog" transform="rotate(157.50)"/>
      <use xlink:href="#cog" transform="rotate(168.75)"/>
      <use xlink:href="#cog" transform="rotate(180.00)"/>
      <use xlink:href="#cog" transform="rotate(191.25)"/>
      <use xlink:href="#cog" transform="rotate(202.50)"/>
      <use xlink:href="#cog" transform="rotate(213.75)"/>
      <use xlink:href="#cog" transform="rotate(225.00)"/>
      <use xlink:href="#cog" transform="rotate(236.25)"/>
      <use xlink:href="#cog" transform="rotate(247.50)"/>
      <use xlink:href="#cog" transform="rotate(258.75)"/>
      <use xlink:href="#cog" transform="rotate(270.00)"/>
      <use xlink:href="#cog" transform="rotate(281.25)"/>
      <use xlink:href="#cog" transform="rotate(292.50)"/>
      <use xlink:href="#cog" transform="rotate(303.75)"/>
      <use xlink:href="#cog" transform="rotate(315.00)"/>
      <use xlink:href="#cog" transform="rotate(326.25)"/>
      <use xlink:href="#cog" transform="rotate(337.50)"/>
      <use xlink:href="#cog" transform="rotate(348.75)"/>
    </g>
    <g id="mounts">
      <polygon id="mount" stroke="black" stroke-width="6" stroke-linejoin="round" points="-7,-42 0,-35 7,-42"/>
      <use xlink:href="#mount" transform="rotate(72)"/>
      <use xlink:href="#mount" transform="rotate(144)"/>
      <use xlink:href="#mount" transform="rotate(216)"/>
      <use xlink:href="#mount" transform="rotate(288)"/>
    </g>
  </g>
  <mask id="holes">
    <rect x="-60" y="-60" width="120" height="120" fill="white"/>
    <circle id="hole" cy="-40" r="3"/>
    <use xlink:href="#hole" transform="rotate(72)"/>
    <use xlink:href="#hole" transform="rotate(144)"/>
    <use xlink:href="#hole" transform="rotate(216)"/>
    <use xlink:href="#hole" transform="rotate(288)"/>
  </mask>
</g>
</svg>"##,
};

const SVG_DEMO_ASSETS: &[SvgDemoAsset] = &[
    SVG_SUNRISE_LAYERS,
    SVG_RIBBON_FLOWER,
    SVG_RADAR_PING,
    SVG_CANDY_BADGE,
    SVG_WAVE_TILES,
    SVG_COMET_LOOP,
    SVG_WEATHER_SUN,
    SVG_WEATHER_PARTLY_CLOUDY,
    SVG_WEATHER_CLOUDY,
    SVG_WEATHER_RAIN,
    SVG_WEATHER_THUNDER,
    SVG_WEATHER_SNOW,
    SVG_MJS_LOGO,
    SVG_RUST_LOGO,
];
