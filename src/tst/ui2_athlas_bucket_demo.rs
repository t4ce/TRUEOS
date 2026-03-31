use alloc::vec::Vec;

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::gfx::althlasfont::athlasmetrics::{
    self, ATHLAS_BUCKET_COUNT, ATHLAS_FONT_INFO, ATHLAS_VARIANT_JSONS, AthlasVariantJson,
};
use crate::gfx::png_codec::DecodedPng;
use crate::r::ui2::Ui2HostedSurfaceTile;

const UI2_ATHLAS_BUCKET_DEMO_VARIANT_COUNT: usize = ATHLAS_VARIANT_JSONS.len();
const UI2_ATHLAS_BUCKET_DEMO_WINDOW_Z: i16 = 32;
const UI2_ATHLAS_BUCKET_DEMO_CONTENT_ID_BASE: u32 = 40;
const UI2_ATHLAS_BUCKET_DEMO_TILE_TEX_ID_BASE: u32 = 4_800;
const UI2_ATHLAS_BUCKET_DEMO_WINDOW_ALPHA: u8 = 220;
const UI2_ATHLAS_BUCKET_DEMO_WINDOW_SIZE_PX: f32 = 400.0;
const UI2_ATHLAS_BUCKET_DEMO_BG_RGBA: [u8; 4] = [0x00, 0x00, 0x00, 0xFF];
const UI2_ATHLAS_BUCKET_DEMO_LIGHT_BG_RGBA: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];
const UI2_ATHLAS_BUCKET_DEMO_LIGHT_FG_RGBA: [u8; 4] = [0x16, 0x18, 0x1E, 0xFF];
const UI2_ATHLAS_BUCKET_DEMO_DARK_BG_RGBA: [u8; 4] = [0x08, 0x09, 0x0C, 0xFF];
const UI2_ATHLAS_BUCKET_DEMO_DARK_FG_RGBA: [u8; 4] = [0xF4, 0xF6, 0xFA, 0xFF];
const UI2_ATHLAS_BUCKET_DEMO_GAP_PX: u32 = 12;
const UI2_ATHLAS_BUCKET_DEMO_DEFER_MS: u64 = 16;
const UI2_ATHLAS_BUCKET_DEMO_UPLOAD_YIELD_MS: u64 = 1;

const ATHLAS_BUCKET_PNGS: [[&[u8]; ATHLAS_BUCKET_COUNT]; UI2_ATHLAS_BUCKET_DEMO_VARIANT_COUNT] = [
    [
        include_bytes!("../gfx/althlasfont/lucida-half/atlas-g00.png"),
        include_bytes!("../gfx/althlasfont/lucida-half/atlas-g01.png"),
        include_bytes!("../gfx/althlasfont/lucida-half/atlas-g02.png"),
        include_bytes!("../gfx/althlasfont/lucida-half/atlas-g03.png"),
        include_bytes!("../gfx/althlasfont/lucida-half/atlas-g04.png"),
        include_bytes!("../gfx/althlasfont/lucida-half/atlas-g05.png"),
        include_bytes!("../gfx/althlasfont/lucida-half/atlas-g06.png"),
        include_bytes!("../gfx/althlasfont/lucida-half/atlas-g07.png"),
    ],
    [
        include_bytes!("../gfx/althlasfont/lucida-1x/atlas-g00.png"),
        include_bytes!("../gfx/althlasfont/lucida-1x/atlas-g01.png"),
        include_bytes!("../gfx/althlasfont/lucida-1x/atlas-g02.png"),
        include_bytes!("../gfx/althlasfont/lucida-1x/atlas-g03.png"),
        include_bytes!("../gfx/althlasfont/lucida-1x/atlas-g04.png"),
        include_bytes!("../gfx/althlasfont/lucida-1x/atlas-g05.png"),
        include_bytes!("../gfx/althlasfont/lucida-1x/atlas-g06.png"),
        include_bytes!("../gfx/althlasfont/lucida-1x/atlas-g07.png"),
    ],
    [
        include_bytes!("../gfx/althlasfont/lucida-3x/atlas-g00.png"),
        include_bytes!("../gfx/althlasfont/lucida-3x/atlas-g01.png"),
        include_bytes!("../gfx/althlasfont/lucida-3x/atlas-g02.png"),
        include_bytes!("../gfx/althlasfont/lucida-3x/atlas-g03.png"),
        include_bytes!("../gfx/althlasfont/lucida-3x/atlas-g04.png"),
        include_bytes!("../gfx/althlasfont/lucida-3x/atlas-g05.png"),
        include_bytes!("../gfx/althlasfont/lucida-3x/atlas-g06.png"),
        include_bytes!("../gfx/althlasfont/lucida-3x/atlas-g07.png"),
    ],
];

fn athlas_variant(size_case: usize) -> Option<&'static AthlasVariantJson> {
    ATHLAS_VARIANT_JSONS.get(size_case)
}

fn athlas_variant_title(size_case: usize) -> Option<&'static str> {
    let variant = athlas_variant(size_case)?;
    Some(match variant.name {
        "half" => "Athlas Buckets 1/2x",
        "1x" => "Athlas Buckets 1x",
        "3x" => "Athlas Buckets 3x",
        _ => "Athlas Buckets",
    })
}

fn athlas_variant_colors(size_case: usize) -> ([u8; 4], [u8; 4]) {
    if size_case == 0 {
        (UI2_ATHLAS_BUCKET_DEMO_LIGHT_BG_RGBA, UI2_ATHLAS_BUCKET_DEMO_LIGHT_FG_RGBA)
    } else {
        (UI2_ATHLAS_BUCKET_DEMO_DARK_BG_RGBA, UI2_ATHLAS_BUCKET_DEMO_DARK_FG_RGBA)
    }
}

fn athlas_variant_content_id(size_case: usize) -> u32 {
    UI2_ATHLAS_BUCKET_DEMO_CONTENT_ID_BASE.saturating_add(size_case as u32)
}

fn athlas_variant_tile_tex_id(size_case: usize, bucket: usize) -> u32 {
    UI2_ATHLAS_BUCKET_DEMO_TILE_TEX_ID_BASE
        .saturating_add((size_case as u32).saturating_mul(ATHLAS_BUCKET_COUNT as u32))
        .saturating_add(bucket as u32)
}

fn athlas_bucket_png_bytes(size_case: usize, bucket: usize) -> Option<&'static [u8]> {
    ATHLAS_BUCKET_PNGS
        .get(size_case)
        .and_then(|variant| variant.get(bucket).copied())
}

fn decode_athlas_bucket_variant(size_case: usize) -> Option<Vec<DecodedPng>> {
    let variant = athlas_variant(size_case)?;
    let mut decoded = Vec::with_capacity(ATHLAS_BUCKET_COUNT);
    for bucket in 0..ATHLAS_BUCKET_COUNT {
        let Some(bytes) = athlas_bucket_png_bytes(size_case, bucket) else {
            crate::log!(
                "ui2-athlas-bucket-demo: missing variant png size_case={} variant={} bucket={}\n",
                size_case,
                variant.name,
                bucket
            );
            return None;
        };
        let image = match crate::gfx::png_codec::decode_png_rgba(bytes) {
            Ok(image) => image,
            Err(err) => {
                crate::log!(
                    "ui2-athlas-bucket-demo: png decode failed size_case={} variant={} bucket={} code={}\n",
                    size_case,
                    variant.name,
                    bucket,
                    err.code()
                );
                return None;
            }
        };
        decoded.push(image);
    }
    Some(decoded)
}

fn athlas_bucket_content_extent(decoded: &[DecodedPng]) -> (u32, u32) {
    let mut content_w = UI2_ATHLAS_BUCKET_DEMO_GAP_PX * 2;
    let mut content_h = UI2_ATHLAS_BUCKET_DEMO_GAP_PX;
    for (bucket, image) in decoded.iter().enumerate() {
        let stage_padding =
            u32::from(athlasmetrics::athlas_bucket_width_stage(bucket).unwrap_or(0));
        content_w = content_w.max(
            image
                .width
                .saturating_add(UI2_ATHLAS_BUCKET_DEMO_GAP_PX * 2),
        );
        content_h = content_h
            .saturating_add(image.height)
            .saturating_add(UI2_ATHLAS_BUCKET_DEMO_GAP_PX)
            .saturating_add(stage_padding.min(2));
    }
    (content_w, content_h)
}

fn athlas_bucket_origin(decoded: &[DecodedPng], bucket: usize) -> (u32, u32) {
    let mut y = UI2_ATHLAS_BUCKET_DEMO_GAP_PX;
    for (idx, image) in decoded.iter().enumerate() {
        if idx == bucket {
            return (UI2_ATHLAS_BUCKET_DEMO_GAP_PX, y);
        }
        let stage_padding = u32::from(athlasmetrics::athlas_bucket_width_stage(idx).unwrap_or(0));
        y = y
            .saturating_add(image.height)
            .saturating_add(UI2_ATHLAS_BUCKET_DEMO_GAP_PX)
            .saturating_add(stage_padding.min(2));
    }
    (UI2_ATHLAS_BUCKET_DEMO_GAP_PX, UI2_ATHLAS_BUCKET_DEMO_GAP_PX)
}

fn build_athlas_bucket_surface_rgba(
    image: &DecodedPng,
    bg_rgba: [u8; 4],
    fg_rgba: [u8; 4],
) -> Vec<u8> {
    let px_count = (image.width as usize).saturating_mul(image.height as usize);
    let mut rgba = vec![0u8; px_count.saturating_mul(4)];
    for chunk in rgba.chunks_exact_mut(4) {
        chunk.copy_from_slice(&bg_rgba);
    }
    for idx in 0..px_count {
        let src = idx.saturating_mul(4);
        let coverage = image.rgba.get(src).copied().unwrap_or(0) as u16;
        if coverage == 0 {
            continue;
        }
        for chan in 0..4 {
            let bg = rgba[src + chan] as u16;
            let fg = fg_rgba[chan] as u16;
            rgba[src + chan] = (((bg * (255 - coverage)) + (fg * coverage)) / 255) as u8;
        }
    }
    rgba
}

fn athlas_window_origin(size_case: usize) -> (f32, f32) {
    match size_case {
        0 => (0.0, 0.0),
        1 => (50.0, 50.0),
        2 => (100.0, 100.0),
        _ => (0.0, 0.0),
    }
}

async fn run_athlas_bucket_demo(size_case: usize) {
    let Some(variant) = athlas_variant(size_case) else {
        crate::log!(
            "ui2-athlas-bucket-demo: invalid size_case={} variant_count={}\n",
            size_case,
            UI2_ATHLAS_BUCKET_DEMO_VARIANT_COUNT
        );
        return;
    };
    let Some(decoded) = decode_athlas_bucket_variant(size_case) else {
        return;
    };

    let (content_w, content_h) = athlas_bucket_content_extent(decoded.as_slice());
    let content_id = athlas_variant_content_id(size_case);
    let (window_x, window_y) = athlas_window_origin(size_case);
    let (bg_rgba, fg_rgba) = athlas_variant_colors(size_case);
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::from_tiled_content(
        athlas_variant_title(size_case).unwrap_or("Athlas Buckets"),
        crate::r::ui2::Ui2Rect {
            x: window_x,
            y: window_y,
            w: UI2_ATHLAS_BUCKET_DEMO_WINDOW_SIZE_PX,
            h: UI2_ATHLAS_BUCKET_DEMO_WINDOW_SIZE_PX,
        },
        UI2_ATHLAS_BUCKET_DEMO_WINDOW_Z,
        UI2_ATHLAS_BUCKET_DEMO_WINDOW_ALPHA,
        bg_rgba,
    ) else {
        crate::log!("ui2-athlas-bucket-demo: window creation failed size_case={}\n", size_case);
        return;
    };

    if !surface.bind_hosted_scroll_state(content_id, content_w, content_h) {
        crate::log!(
            "ui2-athlas-bucket-demo: hosted scroll bind failed window={} content_id={} size_case={}\n",
            surface.window_id(),
            content_id,
            size_case
        );
        return;
    }

    let mut tiles = Vec::with_capacity(decoded.len());
    for (bucket, image) in decoded.iter().enumerate() {
        let (x, y) = athlas_bucket_origin(decoded.as_slice(), bucket);
        tiles.push(Ui2HostedSurfaceTile {
            tex_id: athlas_variant_tile_tex_id(size_case, bucket),
            x,
            y,
            width: image.width,
            height: image.height,
            blend_enabled: false,
        });
    }
    if !surface.set_tiles(bg_rgba, tiles.as_slice()) {
        crate::log!(
            "ui2-athlas-bucket-demo: tile registration failed window={} size_case={}\n",
            surface.window_id(),
            size_case
        );
        return;
    }

    Timer::after(EmbassyDuration::from_millis(UI2_ATHLAS_BUCKET_DEMO_DEFER_MS)).await;

    for (bucket, image) in decoded.into_iter().enumerate() {
        let rgba = build_athlas_bucket_surface_rgba(&image, bg_rgba, fg_rgba);
        if !crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
            athlas_variant_tile_tex_id(size_case, bucket),
            image.width,
            image.height,
            rgba.as_slice(),
            surface.window_id(),
            variant.dir,
        ) {
            crate::log!(
                "ui2-athlas-bucket-demo: upload failed window={} size_case={} variant={} bucket={} tex={} size={}x{}\n",
                surface.window_id(),
                size_case,
                variant.name,
                bucket,
                athlas_variant_tile_tex_id(size_case, bucket),
                image.width,
                image.height
            );
            return;
        }
        Timer::after(EmbassyDuration::from_millis(UI2_ATHLAS_BUCKET_DEMO_UPLOAD_YIELD_MS)).await;
    }

    crate::log!(
        "ui2-athlas-bucket-demo: window={} size_case={} variant={} viewport={}x{} content={}x{} buckets={} upem={} line_height={}\n",
        surface.window_id(),
        size_case,
        variant.name,
        UI2_ATHLAS_BUCKET_DEMO_WINDOW_SIZE_PX as u32,
        UI2_ATHLAS_BUCKET_DEMO_WINDOW_SIZE_PX as u32,
        content_w,
        content_h,
        ATHLAS_BUCKET_COUNT,
        ATHLAS_FONT_INFO.units_per_em,
        ATHLAS_FONT_INFO.line_height
    );

    loop {
        Timer::after(EmbassyDuration::from_secs(3600)).await;
    }
}

#[embassy_executor::task(pool_size = UI2_ATHLAS_BUCKET_DEMO_VARIANT_COUNT)]
pub async fn ui2_athlas_bucket_demo_task(size_case: usize) {
    run_athlas_bucket_demo(size_case).await;
}
