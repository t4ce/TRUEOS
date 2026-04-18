use alloc::vec::Vec;

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::gfx::althlasfont;
use crate::gfx::althlasfont::athlasmetrics::{
    self, ATHLAS_BUCKET_COUNT, ATHLAS_FONT_INFO, ATHLAS_VARIANT_JSONS, AthlasVariantJson,
};
use crate::gfx::althlasfont::twemoji;
use crate::gfx::png_codec::DecodedPng;

use super::{
    Ui2HostedSurfaceTile, Ui2Rect, Ui2SurfaceWindow, minimize_window,
    request_window_content_present,
};

const UI2_ATHLAS_BUCKET_DEMO_VARIANT_COUNT: usize = ATHLAS_VARIANT_JSONS.len();
const UI2_PALATINO_BUCKET_DEMO_VARIANT_COUNT: usize = 1;
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
const UI2_ATHLAS_BUCKET_DEMO_READY_WAIT_MS: u64 = 16;
const UI2_ATHLAS_BUCKET_DEMO_VISIBLE_MAX_H_PX: f32 = 720.0;

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
        include_bytes!("../gfx/althlasfont/lucida-2x/atlas-g00.png"),
        include_bytes!("../gfx/althlasfont/lucida-2x/atlas-g01.png"),
        include_bytes!("../gfx/althlasfont/lucida-2x/atlas-g02.png"),
        include_bytes!("../gfx/althlasfont/lucida-2x/atlas-g03.png"),
        include_bytes!("../gfx/althlasfont/lucida-2x/atlas-g04.png"),
        include_bytes!("../gfx/althlasfont/lucida-2x/atlas-g05.png"),
        include_bytes!("../gfx/althlasfont/lucida-2x/atlas-g06.png"),
        include_bytes!("../gfx/althlasfont/lucida-2x/atlas-g07.png"),
    ],
    [
        include_bytes!("../gfx/althlasfont/lucida-third/atlas-g00.png"),
        include_bytes!("../gfx/althlasfont/lucida-third/atlas-g01.png"),
        include_bytes!("../gfx/althlasfont/lucida-third/atlas-g02.png"),
        include_bytes!("../gfx/althlasfont/lucida-third/atlas-g03.png"),
        include_bytes!("../gfx/althlasfont/lucida-third/atlas-g04.png"),
        include_bytes!("../gfx/althlasfont/lucida-third/atlas-g05.png"),
        include_bytes!("../gfx/althlasfont/lucida-third/atlas-g06.png"),
        include_bytes!("../gfx/althlasfont/lucida-third/atlas-g07.png"),
    ],
];

const PALATINO_BUCKET_PNGS: [[&[u8]; ATHLAS_BUCKET_COUNT]; UI2_PALATINO_BUCKET_DEMO_VARIANT_COUNT] =
    [[
        include_bytes!("../gfx/althlasfont/palatino-1x/atlas-g00.png"),
        include_bytes!("../gfx/althlasfont/palatino-1x/atlas-g01.png"),
        include_bytes!("../gfx/althlasfont/palatino-1x/atlas-g02.png"),
        include_bytes!("../gfx/althlasfont/palatino-1x/atlas-g03.png"),
        include_bytes!("../gfx/althlasfont/palatino-1x/atlas-g04.png"),
        include_bytes!("../gfx/althlasfont/palatino-1x/atlas-g05.png"),
        include_bytes!("../gfx/althlasfont/palatino-1x/atlas-g06.png"),
        include_bytes!("../gfx/althlasfont/palatino-1x/atlas-g07.png"),
    ]];

struct BucketDemoSpec {
    title: &'static str,
    family_name: &'static str,
    variant_name: &'static str,
    variant_dir: &'static str,
    content_id: u32,
    tile_tex_id_base: u32,
    window_origin: (f32, f32),
    start_minimized: bool,
    ready_size_case: Option<usize>,
}

fn athlas_variant(size_case: usize) -> Option<&'static AthlasVariantJson> {
    ATHLAS_VARIANT_JSONS.get(size_case)
}

fn athlas_variant_title(size_case: usize) -> Option<&'static str> {
    let variant = athlas_variant(size_case)?;
    Some(match variant.name {
        "half" => "Athlas Buckets 1/2x",
        "1x" => "Athlas Buckets 1x",
        "2x" => "Athlas Buckets 2x",
        "third" => "Athlas Buckets 1/3x",
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

fn bucket_demo_tile_tex_id(spec: &BucketDemoSpec, bucket: usize) -> u32 {
    spec.tile_tex_id_base.saturating_add(bucket as u32)
}

fn athlas_bucket_png_bytes(size_case: usize, bucket: usize) -> Option<&'static [u8]> {
    ATHLAS_BUCKET_PNGS
        .get(size_case)
        .and_then(|variant| variant.get(bucket).copied())
}

fn palatino_bucket_png_bytes(bucket: usize) -> Option<&'static [u8]> {
    PALATINO_BUCKET_PNGS
        .first()
        .and_then(|variant| variant.get(bucket).copied())
}

#[inline]
fn twemoji_texture_drawable() -> bool {
    crate::r::io::cabi::trueos_cabi_gfx_texture_status(twemoji::TWEMOJI_TEX_ID) == 2
}

#[inline]
fn athlas_bucket_texture_drawable(size_case: usize, bucket: usize) -> bool {
    const ASYNC_TEX_STATUS_READY: i32 = 2;
    crate::r::io::cabi::trueos_cabi_gfx_texture_status(athlas_variant_tile_tex_id(
        size_case, bucket,
    )) == ASYNC_TEX_STATUS_READY
}

fn athlas_tier_textures_drawable(size_case: usize) -> bool {
    (0..ATHLAS_BUCKET_COUNT).all(|bucket| athlas_bucket_texture_drawable(size_case, bucket))
}

pub(crate) fn ui2_font_bucketproducer_decode_variant(size_case: usize) -> Option<Vec<DecodedPng>> {
    let variant = athlas_variant(size_case)?;
    let mut decoded = Vec::with_capacity(ATHLAS_BUCKET_COUNT);
    for bucket in 0..ATHLAS_BUCKET_COUNT {
        let Some(bytes) = athlas_bucket_png_bytes(size_case, bucket) else {
            crate::log!(
                "ui2-font-bucketproducer: missing variant png size_case={} variant={} bucket={}\n",
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
                    "ui2-font-bucketproducer: png decode failed size_case={} variant={} bucket={} code={}\n",
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

fn athlas_window_origin(size_case: usize) -> (f32, f32) {
    match size_case {
        0 => (50.0, 50.0),
        1 => (100.0, 100.0),
        2 => (150.0, 150.0),
        3 => (0.0, 0.0),
        _ => (0.0, 0.0),
    }
}

fn palatino_window_origin() -> (f32, f32) {
    (200.0, 32.0)
}

fn palatino_demo_spec() -> BucketDemoSpec {
    BucketDemoSpec {
        title: "Palatino Buckets 1x",
        family_name: "palatino",
        variant_name: "1x",
        variant_dir: "palatino-1x",
        content_id: UI2_ATHLAS_BUCKET_DEMO_CONTENT_ID_BASE
            .saturating_add(UI2_ATHLAS_BUCKET_DEMO_VARIANT_COUNT as u32),
        tile_tex_id_base: UI2_ATHLAS_BUCKET_DEMO_TILE_TEX_ID_BASE.saturating_add(
            (UI2_ATHLAS_BUCKET_DEMO_VARIANT_COUNT as u32)
                .saturating_mul(ATHLAS_BUCKET_COUNT as u32),
        ),
        window_origin: palatino_window_origin(),
        start_minimized: true,
        ready_size_case: None,
    }
}

fn lucida_demo_spec(size_case: usize, variant: &'static AthlasVariantJson) -> BucketDemoSpec {
    BucketDemoSpec {
        title: athlas_variant_title(size_case).unwrap_or("Athlas Buckets"),
        family_name: "lucida",
        variant_name: variant.name,
        variant_dir: variant.dir,
        content_id: athlas_variant_content_id(size_case),
        tile_tex_id_base: UI2_ATHLAS_BUCKET_DEMO_TILE_TEX_ID_BASE
            .saturating_add((size_case as u32).saturating_mul(ATHLAS_BUCKET_COUNT as u32)),
        window_origin: athlas_window_origin(size_case),
        start_minimized: true,
        ready_size_case: Some(size_case),
    }
}

fn bucket_demo_textures_drawable(spec: &BucketDemoSpec) -> bool {
    match spec.ready_size_case {
        Some(size_case) => athlas_tier_textures_drawable(size_case),
        None => (0..ATHLAS_BUCKET_COUNT).all(|bucket| {
            crate::r::io::cabi::trueos_cabi_gfx_texture_status(bucket_demo_tile_tex_id(
                spec, bucket,
            )) == 2
        }),
    }
}

fn bucket_demo_window_size(spec: &BucketDemoSpec, content_w: u32, content_h: u32) -> (f32, f32) {
    if spec.start_minimized {
        return (UI2_ATHLAS_BUCKET_DEMO_WINDOW_SIZE_PX, UI2_ATHLAS_BUCKET_DEMO_WINDOW_SIZE_PX);
    }

    (
        content_w.max(1) as f32,
        (content_h.max(1) as f32).min(UI2_ATHLAS_BUCKET_DEMO_VISIBLE_MAX_H_PX),
    )
}

fn bucket_demo_window_alpha(spec: &BucketDemoSpec) -> u8 {
    if spec.start_minimized {
        UI2_ATHLAS_BUCKET_DEMO_WINDOW_ALPHA
    } else {
        0xFF
    }
}

async fn run_bucket_demo(
    spec: BucketDemoSpec,
    decoded: Vec<DecodedPng>,
    bg_rgba: [u8; 4],
    fg_rgba: [u8; 4],
) {
    let (content_w, content_h) = athlas_bucket_content_extent(decoded.as_slice());
    let (window_x, window_y) = spec.window_origin;
    let (window_w, window_h) = bucket_demo_window_size(&spec, content_w, content_h);
    let Some(surface) = Ui2SurfaceWindow::from_tiled_content(
        spec.title,
        Ui2Rect {
            x: window_x,
            y: window_y,
            w: window_w,
            h: window_h,
        },
        UI2_ATHLAS_BUCKET_DEMO_WINDOW_Z,
        bucket_demo_window_alpha(&spec),
        bg_rgba,
    ) else {
        crate::log!(
            "ui2-font-bucketproducer: window creation failed family={} variant={}\n",
            spec.family_name,
            spec.variant_name
        );
        return;
    };

    super::ui2_win::set_window_title_twemoji(surface.window_id(), '\u{1F524}');

    if !surface.bind_hosted_scroll_state(spec.content_id, content_w, content_h) {
        crate::log!(
            "ui2-font-bucketproducer: hosted scroll bind failed window={} content_id={} family={} variant={}\n",
            surface.window_id(),
            spec.content_id,
            spec.family_name,
            spec.variant_name
        );
        return;
    }

    let mut tiles = Vec::with_capacity(decoded.len());
    for (bucket, image) in decoded.iter().enumerate() {
        let (x, y) = athlas_bucket_origin(decoded.as_slice(), bucket);
        tiles.push(Ui2HostedSurfaceTile {
            tex_id: bucket_demo_tile_tex_id(&spec, bucket),
            x,
            y,
            width: image.width,
            height: image.height,
            blend_enabled: true,
        });
    }
    if !surface.set_tiles(bg_rgba, fg_rgba, tiles.as_slice()) {
        crate::log!(
            "ui2-font-bucketproducer: tile registration failed window={} family={} variant={}\n",
            surface.window_id(),
            spec.family_name,
            spec.variant_name
        );
        return;
    }

    let repaint_window_id = if spec.start_minimized {
        let _ = minimize_window(surface.window_id());
        0
    } else {
        surface.window_id()
    };

    Timer::after(EmbassyDuration::from_millis(UI2_ATHLAS_BUCKET_DEMO_DEFER_MS)).await;

    for (bucket, image) in decoded.into_iter().enumerate() {
        if !crate::r::io::cabi::queue_texture_mask_image_upload_copy(
            bucket_demo_tile_tex_id(&spec, bucket),
            image.width,
            image.height,
            image.rgba.as_slice(),
            repaint_window_id,
            spec.variant_dir,
        ) {
            crate::log!(
                "ui2-font-bucketproducer: upload failed window={} family={} variant={} bucket={} tex={} size={}x{}\n",
                surface.window_id(),
                spec.family_name,
                spec.variant_name,
                bucket,
                bucket_demo_tile_tex_id(&spec, bucket),
                image.width,
                image.height
            );
            return;
        }
        if let Some(size_case) = spec.ready_size_case {
            let _ = althlasfont::athlas_register_bucket_texture(
                size_case,
                bucket,
                bucket_demo_tile_tex_id(&spec, bucket),
                image.width,
                image.height,
            );
        }
        Timer::after(EmbassyDuration::from_millis(UI2_ATHLAS_BUCKET_DEMO_UPLOAD_YIELD_MS)).await;
    }

    loop {
        if bucket_demo_textures_drawable(&spec) {
            break;
        }
        Timer::after(EmbassyDuration::from_millis(UI2_ATHLAS_BUCKET_DEMO_READY_WAIT_MS)).await;
    }

    let ready_seq = spec
        .ready_size_case
        .and_then(|size_case| althlasfont::athlas_mark_tier_ready(size_case))
        .unwrap_or(0);
    if !spec.start_minimized {
        let _ = request_window_content_present(surface.window_id(), "ui2-athlas-bucket-ready");
    }

    crate::log!(
        "ui2-font-bucketproducer: window={} family={} variant={} viewport={}x{} content={}x{} buckets={} upem={} line_height={} ready_seq={} minimized={}\n",
        surface.window_id(),
        spec.family_name,
        spec.variant_name,
        window_w as u32,
        window_h as u32,
        content_w,
        content_h,
        ATHLAS_BUCKET_COUNT,
        ATHLAS_FONT_INFO.units_per_em,
        ATHLAS_FONT_INFO.line_height,
        ready_seq,
        spec.start_minimized
    );

    loop {
        Timer::after(EmbassyDuration::from_secs(3600)).await;
    }
}

async fn run_athlas_bucket_demo(size_case: usize) {
    let Some(variant) = athlas_variant(size_case) else {
        crate::log!(
            "ui2-font-bucketproducer: invalid size_case={} variant_count={}\n",
            size_case,
            UI2_ATHLAS_BUCKET_DEMO_VARIANT_COUNT
        );
        return;
    };
    let _ = althlasfont::athlas_reset_tier_state(size_case);
    let Some(decoded) = ui2_font_bucketproducer_decode_variant(size_case) else {
        return;
    };
    let (bg_rgba, fg_rgba) = athlas_variant_colors(size_case);
    run_bucket_demo(lucida_demo_spec(size_case, variant), decoded, bg_rgba, fg_rgba).await;
}

fn decode_palatino_bucket_variant() -> Option<Vec<DecodedPng>> {
    let spec = palatino_demo_spec();
    let mut decoded = Vec::with_capacity(ATHLAS_BUCKET_COUNT);
    for bucket in 0..ATHLAS_BUCKET_COUNT {
        let Some(bytes) = palatino_bucket_png_bytes(bucket) else {
            crate::log!(
                "ui2-font-bucketproducer: missing palatino variant png variant={} bucket={}\n",
                spec.variant_name,
                bucket
            );
            return None;
        };
        let image = match crate::gfx::png_codec::decode_png_rgba(bytes) {
            Ok(image) => image,
            Err(err) => {
                crate::log!(
                    "ui2-font-bucketproducer: png decode failed family={} variant={} bucket={} code={}\n",
                    spec.family_name,
                    spec.variant_name,
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

#[embassy_executor::task(pool_size = UI2_ATHLAS_BUCKET_DEMO_VARIANT_COUNT)]
pub async fn ui2_font_bucketproducer_demo_task(size_case: usize) {
    run_athlas_bucket_demo(size_case).await;
}

async fn run_palatino_bucket_demo() {
    let Some(decoded) = decode_palatino_bucket_variant() else {
        return;
    };
    let (bg_rgba, fg_rgba) =
        (UI2_ATHLAS_BUCKET_DEMO_LIGHT_BG_RGBA, UI2_ATHLAS_BUCKET_DEMO_LIGHT_FG_RGBA);
    run_bucket_demo(palatino_demo_spec(), decoded, bg_rgba, fg_rgba).await;
}

#[embassy_executor::task(pool_size = UI2_PALATINO_BUCKET_DEMO_VARIANT_COUNT)]
pub async fn ui2_font_bucketproducer_palatino_demo_task() {
    run_palatino_bucket_demo().await;
}

#[embassy_executor::task(pool_size = UI2_PALATINO_BUCKET_DEMO_VARIANT_COUNT)]
pub async fn ui2_font_bucketproducer_palatino_bw_demo_task() {
    run_palatino_bucket_demo().await;
}

#[embassy_executor::task]
pub async fn ui2_font_twemoji_loader_task() {
    twemoji::twemoji_reset_state();

    let decoded = match crate::gfx::png_codec::decode_png_rgba(twemoji::TWEMOJI_ATLAS_PNG) {
        Ok(decoded) => decoded,
        Err(err) => {
            crate::log!("ui2-font-bucketproducer: twemoji decode failed code={}\n", err.code());
            return;
        }
    };

    if !crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
        twemoji::TWEMOJI_TEX_ID,
        decoded.width,
        decoded.height,
        decoded.rgba.as_slice(),
        0,
        "twemoji-1x",
    ) {
        crate::log!(
            "ui2-font-bucketproducer: twemoji upload failed tex={} size={}x{}\n",
            twemoji::TWEMOJI_TEX_ID,
            decoded.width,
            decoded.height
        );
        return;
    }

    twemoji::twemoji_register_texture(twemoji::TWEMOJI_TEX_ID, decoded.width, decoded.height);

    loop {
        if twemoji_texture_drawable() {
            break;
        }
        Timer::after(EmbassyDuration::from_millis(UI2_ATHLAS_BUCKET_DEMO_READY_WAIT_MS)).await;
    }

    let ready_seq = twemoji::twemoji_mark_ready().unwrap_or(0);
    crate::log!(
        "ui2-font-bucketproducer: twemoji ready tex={} size={}x{} line_height={} ready_seq={}\n",
        twemoji::TWEMOJI_TEX_ID,
        decoded.width,
        decoded.height,
        twemoji::twemoji_cell_height_px(),
        ready_seq
    );

    loop {
        Timer::after(EmbassyDuration::from_secs(3600)).await;
    }
}
