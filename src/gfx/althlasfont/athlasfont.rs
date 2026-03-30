use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use libm::{ceilf, fabsf, floorf};
use spin::{Mutex, Once};
use trueos_gfx_core::{Rgba8, TEX_VERTEX_SIZE, ViewTransform, push_tex_quad_px};

pub mod athlasmetrics;

use self::athlasmetrics::{ATHLAS_BUCKET_COUNT, AthlasGlyphLookup};


static IMBA_ATHLAS_PNG_BUCKETS_UPLOADED: AtomicBool = AtomicBool::new(false);
static IMBA_ATHLAS_LOOKUP_INSTALL: Once<AthlasFontLookupInstall> = Once::new();
static IMBA_ATHLAS_SHORT_TEXT_CACHE: Mutex<BTreeMap<u64, AthlasFontShortTextCacheEntry>> =
    Mutex::new(BTreeMap::new());
static IMBA_ATHLAS_SHORT_TEXT_NEXT_TEX_ID: AtomicU32 = AtomicU32::new(30_000);

const IMBA_ATHLAS_GRID: usize = 16;
const IMBA_ATHLAS_LARGE_TILE_H: f32 = 24.0;

const IMBA_ATHLAS_BUCKET_TEX_IDS: [[u32; 8]; 3] = [
    [1100, 1101, 1102, 1103, 1104, 1105, 1106, 1107],
    [1110, 1111, 1112, 1113, 1114, 1115, 1116, 1117],
    [1120, 1121, 1122, 1123, 1124, 1125, 1126, 1127],
];

struct PngBucketAsset {
    size_name: &'static str,
    bucket: usize,
    tex_id: u32,
    png: &'static [u8],
}

#[derive(Clone, Copy, Debug)]
struct AthlasFontShortTextCacheEntry {
    tex_id: u32,
    width: u32,
    height: u32,
}

pub struct AthlasFontLookupInstall {
    pub buckets: [&'static [u32]; ATHLAS_BUCKET_COUNT],
    pub bucket_metrics: [athlasmetrics::AthlasBucketMetrics; ATHLAS_BUCKET_COUNT],
    pub max_bucket_len: usize,
}

const PNG_BUCKET_ASSETS: &[PngBucketAsset] = &[
    PngBucketAsset { size_name: "half", bucket: 0, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[0][0], png: include_bytes!("lucida-half/atlas-g00.png") },
    PngBucketAsset { size_name: "half", bucket: 1, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[0][1], png: include_bytes!("lucida-half/atlas-g01.png") },
    PngBucketAsset { size_name: "half", bucket: 2, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[0][2], png: include_bytes!("lucida-half/atlas-g02.png") },
    PngBucketAsset { size_name: "half", bucket: 3, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[0][3], png: include_bytes!("lucida-half/atlas-g03.png") },
    PngBucketAsset { size_name: "half", bucket: 4, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[0][4], png: include_bytes!("lucida-half/atlas-g04.png") },
    PngBucketAsset { size_name: "half", bucket: 5, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[0][5], png: include_bytes!("lucida-half/atlas-g05.png") },
    PngBucketAsset { size_name: "half", bucket: 6, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[0][6], png: include_bytes!("lucida-half/atlas-g06.png") },
    PngBucketAsset { size_name: "half", bucket: 7, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[0][7], png: include_bytes!("lucida-half/atlas-g07.png") },
    PngBucketAsset { size_name: "1x", bucket: 0, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[1][0], png: include_bytes!("lucida-1x/atlas-g00.png") },
    PngBucketAsset { size_name: "1x", bucket: 1, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[1][1], png: include_bytes!("lucida-1x/atlas-g01.png") },
    PngBucketAsset { size_name: "1x", bucket: 2, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[1][2], png: include_bytes!("lucida-1x/atlas-g02.png") },
    PngBucketAsset { size_name: "1x", bucket: 3, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[1][3], png: include_bytes!("lucida-1x/atlas-g03.png") },
    PngBucketAsset { size_name: "1x", bucket: 4, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[1][4], png: include_bytes!("lucida-1x/atlas-g04.png") },
    PngBucketAsset { size_name: "1x", bucket: 5, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[1][5], png: include_bytes!("lucida-1x/atlas-g05.png") },
    PngBucketAsset { size_name: "1x", bucket: 6, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[1][6], png: include_bytes!("lucida-1x/atlas-g06.png") },
    PngBucketAsset { size_name: "1x", bucket: 7, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[1][7], png: include_bytes!("lucida-1x/atlas-g07.png") },
    PngBucketAsset { size_name: "3x", bucket: 0, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[2][0], png: include_bytes!("lucida-3x/atlas-g00.png") },
    PngBucketAsset { size_name: "3x", bucket: 1, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[2][1], png: include_bytes!("lucida-3x/atlas-g01.png") },
    PngBucketAsset { size_name: "3x", bucket: 2, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[2][2], png: include_bytes!("lucida-3x/atlas-g02.png") },
    PngBucketAsset { size_name: "3x", bucket: 3, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[2][3], png: include_bytes!("lucida-3x/atlas-g03.png") },
    PngBucketAsset { size_name: "3x", bucket: 4, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[2][4], png: include_bytes!("lucida-3x/atlas-g04.png") },
    PngBucketAsset { size_name: "3x", bucket: 5, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[2][5], png: include_bytes!("lucida-3x/atlas-g05.png") },
    PngBucketAsset { size_name: "3x", bucket: 6, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[2][6], png: include_bytes!("lucida-3x/atlas-g06.png") },
    PngBucketAsset { size_name: "3x", bucket: 7, tex_id: IMBA_ATHLAS_BUCKET_TEX_IDS[2][7], png: include_bytes!("lucida-3x/atlas-g07.png") },
];

#[inline]
pub fn imba_athlas_bucket_tex_id(size_case: usize, bucket: usize) -> Option<u32> {
    IMBA_ATHLAS_BUCKET_TEX_IDS
        .get(size_case)
        .and_then(|row| row.get(bucket))
        .copied()
}

pub fn imba_athlas_lookup_install() -> &'static AthlasFontLookupInstall {
    IMBA_ATHLAS_LOOKUP_INSTALL.call_once(|| {
        let buckets = athlasmetrics::ATHLAS_BUCKET_LUTS;
        let bucket_metrics = athlasmetrics::ATHLAS_BUCKET_METRICS;
        let mut max_bucket_len = 0usize;
        for bucket in buckets {
            max_bucket_len = max_bucket_len.max(bucket.len());
        }
        AthlasFontLookupInstall {
            buckets,
            bucket_metrics,
            max_bucket_len,
        }
    })
}

#[inline]
pub fn imba_athlas_lookup_char(ch: char) -> Option<AthlasGlyphLookup> {
    let _ = imba_athlas_lookup_install();
    athlasmetrics::athlas_lookup_char(ch)
}

#[inline]
pub fn imba_athlas_lookup_codepoint(codepoint: u32) -> Option<AthlasGlyphLookup> {
    let _ = imba_athlas_lookup_install();
    athlasmetrics::athlas_lookup_codepoint(codepoint)
}

#[inline]
pub fn imba_athlas_bucket_slot_for_char(ch: char) -> Option<(u8, u16)> {
    imba_athlas_lookup_char(ch).map(|it| (it.bucket, it.slot))
}

#[inline]
pub fn imba_athlas_bucket_tex_for_char(size_case: usize, ch: char) -> Option<(u32, u16)> {
    let glyph = imba_athlas_lookup_char(ch)?;
    let tex_id = imba_athlas_bucket_tex_id(size_case, glyph.bucket as usize)?;
    Some((tex_id, glyph.slot))
}

#[inline]
pub fn imba_athlas_bucket_width_stage(bucket: usize) -> Option<u8> {
    athlasmetrics::athlas_bucket_width_stage(bucket)
}

#[inline]
pub fn imba_athlas_bucket_cell_px(size_case: usize, bucket: usize) -> Option<(u32, u32)> {
    let bucket = decoded_bucket(size_case, bucket)?;
    Some((bucket.cell_w, bucket.cell_h))
}

#[inline]
pub fn imba_athlas_sprite_for_char(
    size_case: usize,
    ch: char,
) -> Option<(u32, [f32; 4], u32, u32)> {
    let glyph = imba_athlas_lookup_char(ch)?;
    let bucket = decoded_bucket(size_case, glyph.bucket as usize)?;
    let slot = glyph.slot as usize;
    let sx = (slot % bucket.grid_w as usize) as f32;
    let sy = (slot / bucket.grid_w as usize) as f32;
    let px0 = sx * bucket.cell_w as f32;
    let py0 = sy * bucket.cell_h as f32;
    let uv = [
        px0 / bucket.width as f32,
        py0 / bucket.height as f32,
        (px0 + bucket.cell_w as f32) / bucket.width as f32,
        (py0 + bucket.cell_h as f32) / bucket.height as f32,
    ];
    Some((bucket.tex_id, uv, bucket.cell_w, bucket.cell_h))
}

#[inline]
pub fn imba_athlas_bucket_width_stage_for_char(ch: char) -> Option<u8> {
    let glyph = imba_athlas_lookup_char(ch)?;
    athlasmetrics::athlas_bucket_width_stage(glyph.bucket as usize)
}

pub fn ensure_imba_athlas_png_buckets_uploaded() -> bool {
    if IMBA_ATHLAS_PNG_BUCKETS_UPLOADED.load(Ordering::Acquire) {
        return true;
    }

    for asset in PNG_BUCKET_ASSETS {
        let decoded = match crate::gfx::png_codec::decode_png_rgba(asset.png) {
            Ok(decoded) => decoded,
            Err(err) => {
                crate::log!(
                    "imba-athlas-png: decode failed size={} bucket={} err={:?}\n",
                    asset.size_name,
                    asset.bucket,
                    err
                );
                return false;
            }
        };

        let rc = crate::r::io::cabi::upload_texture_rgba_mask_no_init(
            asset.tex_id,
            decoded.width,
            decoded.height,
            decoded.rgba.as_slice(),
        );
        if rc != 0 {
            crate::log!(
                "imba-athlas-png: upload failed size={} bucket={} tex={} rc={}\n",
                asset.size_name,
                asset.bucket,
                asset.tex_id,
                rc
            );
            return false;
        }
    }

    IMBA_ATHLAS_PNG_BUCKETS_UPLOADED.store(true, Ordering::Release);
    crate::log!("imba-athlas-png: uploaded {} bucket textures\n", PNG_BUCKET_ASSETS.len());
    true
}

#[inline]
pub fn imba_athlas_png_buckets_uploaded() -> bool {
    IMBA_ATHLAS_PNG_BUCKETS_UPLOADED.load(Ordering::Acquire)
}

#[inline]
fn pack_short_text_cache_key(text: &[u8], px_h: f32) -> Option<u64> {
    if !(2..=3).contains(&text.len()) {
        return None;
    }
    let px_h_u16 = ceilf(px_h.max(1.0)).clamp(1.0, u16::MAX as f32) as u16;
    let mut bytes = [0u8; 3];
    for (idx, &b) in text.iter().enumerate() {
        if b == b'\n' || b == 0 {
            return None;
        }
        bytes[idx] = b;
    }
    Some(
        (bytes[0] as u64)
            | ((bytes[1] as u64) << 8)
            | ((bytes[2] as u64) << 16)
            | ((text.len() as u64) << 24)
            | ((px_h_u16 as u64) << 32),
    )
}

pub fn imba_athlas_cached_short_text_texture_nearest_px(
    text: &[u8],
    px_h: f32,
) -> Option<(u32, u32, u32)> {
    let key = pack_short_text_cache_key(text, px_h)?;
    if let Some(entry) = IMBA_ATHLAS_SHORT_TEXT_CACHE.lock().get(&key).copied() {
        return Some((entry.tex_id, entry.width, entry.height));
    }

    let text_w = ceilf(imba_athlas_text_width_nearest_px(text, px_h)).max(1.0) as u32;
    let text_h = ceilf(px_h.max(1.0)).max(1.0) as u32;
    let rgba_len = (text_w as usize)
        .saturating_mul(text_h as usize)
        .saturating_mul(4);
    let mut rgba = vec![0u8; rgba_len];
    if !blit_imba_athlas_text_rgba_nearest_px(
        rgba.as_mut_slice(),
        text_w,
        text_h,
        text,
        0,
        0,
        px_h,
        (0xFF, 0xFF, 0xFF, 0xFF),
    ) {
        return None;
    }

    let tex_id = IMBA_ATHLAS_SHORT_TEXT_NEXT_TEX_ID.fetch_add(1, Ordering::AcqRel);
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_upload_texture_rgba(
            tex_id,
            text_w,
            text_h,
            rgba.as_ptr(),
            rgba.len(),
        )
    };
    if rc != 0 {
        return None;
    }

    let entry = AthlasFontShortTextCacheEntry {
        tex_id,
        width: text_w,
        height: text_h,
    };
    IMBA_ATHLAS_SHORT_TEXT_CACHE.lock().insert(key, entry);
    Some((entry.tex_id, entry.width, entry.height))
}

pub fn imba_athlas_upload_text_texture_nearest_px(
    tex_id: u32,
    text: &[u8],
    px_h: f32,
    rgba: (u8, u8, u8, u8),
) -> Option<(u32, u32)> {
    if tex_id == 0 || text.is_empty() {
        return None;
    }

    let text_w = ceilf(imba_athlas_text_width_nearest_px(text, px_h)).max(1.0) as u32;
    let text_h = ceilf(px_h.max(1.0)).max(1.0) as u32;
    let rgba_len = (text_w as usize)
        .saturating_mul(text_h as usize)
        .saturating_mul(4);
    let mut rgba_buf = vec![0u8; rgba_len];
    if !blit_imba_athlas_text_rgba_nearest_px(
        rgba_buf.as_mut_slice(),
        text_w,
        text_h,
        text,
        0,
        0,
        px_h,
        rgba,
    ) {
        return None;
    }

    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_upload_texture_rgba(
            tex_id,
            text_w,
            text_h,
            rgba_buf.as_ptr(),
            rgba_buf.len(),
        )
    };
    if rc != 0 {
        return None;
    }

    Some((text_w, text_h))
}

pub fn imba_athlas_alloc_text_texture_nearest_px(
    text: &[u8],
    px_h: f32,
    rgba: (u8, u8, u8, u8),
) -> Option<(u32, u32, u32)> {
    let tex_id = IMBA_ATHLAS_SHORT_TEXT_NEXT_TEX_ID.fetch_add(1, Ordering::AcqRel);
    let (width, height) = imba_athlas_upload_text_texture_nearest_px(tex_id, text, px_h, rgba)?;
    Some((tex_id, width, height))
}

#[derive(Clone, Debug)]
struct AthlasDecodedBucket {
    size_case: usize,
    bucket: usize,
    tex_id: u32,
    alpha: Vec<u8>,
    width: u32,
    height: u32,
    grid_w: u32,
    grid_h: u32,
    cell_w: u32,
    cell_h: u32,
}

static IMBA_ATHLAS_DECODED_BUCKETS: Once<Vec<AthlasDecodedBucket>> = Once::new();

fn decode_athlas_buckets() -> Vec<AthlasDecodedBucket> {
    let mut out = Vec::with_capacity(PNG_BUCKET_ASSETS.len());
    for (size_case, size_name) in ["half", "1x", "3x"].iter().enumerate() {
        for bucket in 0..ATHLAS_BUCKET_COUNT {
            let Some(asset) = PNG_BUCKET_ASSETS
                .iter()
                .find(|it| it.size_name == *size_name && it.bucket == bucket)
            else {
                continue;
            };
            let Ok(decoded) = crate::gfx::png_codec::decode_png_rgba(asset.png) else {
                continue;
            };
            let glyph_count = athlasmetrics::athlas_bucket_codepoints(bucket as u8)
                .map(|it| it.len())
                .unwrap_or(0)
                .max(1);
            let grid_w = IMBA_ATHLAS_GRID as u32;
            let grid_h = glyph_count.div_ceil(IMBA_ATHLAS_GRID).max(1) as u32;
            let cell_w = (decoded.width / grid_w).max(1);
            let cell_h = (decoded.height / grid_h).max(1);
            let px_count = (decoded.width as usize).saturating_mul(decoded.height as usize);
            let mut alpha = vec![0u8; px_count];
            for i in 0..px_count {
                let src = i.saturating_mul(4);
                let coverage = decoded.rgba.get(src).copied().unwrap_or(0);
                alpha[i] = coverage;
            }
            out.push(AthlasDecodedBucket {
                size_case,
                bucket,
                tex_id: asset.tex_id,
                alpha,
                width: decoded.width,
                height: decoded.height,
                grid_w,
                grid_h,
                cell_w,
                cell_h,
            });
        }
    }
    out
}

#[inline]
fn decoded_bucket(size_case: usize, bucket: usize) -> Option<&'static AthlasDecodedBucket> {
    IMBA_ATHLAS_DECODED_BUCKETS
        .call_once(decode_athlas_buckets)
        .iter()
        .find(|it| it.size_case == size_case && it.bucket == bucket)
}

#[inline]
fn imba_athlas_size_case_for_px_h(px_h: f32) -> usize {
    if !px_h.is_finite() || px_h <= 0.0 {
        return 1;
    }

    let target = px_h.max(1.0);
    let variants = [32.0f32, 64.0f32, 192.0f32];
    let mut best_idx = 0usize;
    let mut best_err = f32::MAX;
    for (idx, variant_px_h) in variants.iter().copied().enumerate() {
        let err = fabsf(variant_px_h - target);
        if err < best_err {
            best_err = err;
            best_idx = idx;
        }
    }
    best_idx
}

#[inline]
fn imba_athlas_native_px_h_for_size_case(size_case: usize) -> f32 {
    match size_case {
        0 => 32.0,
        1 => 64.0,
        2 => 192.0,
        _ => 64.0,
    }
}

#[inline]
pub fn imba_athlas_native_px_h(px_h: f32) -> f32 {
    imba_athlas_native_px_h_for_size_case(imba_athlas_size_case_for_px_h(px_h))
}

#[inline]
pub fn imba_athlas_is_native_px_h(px_h: f32) -> bool {
    let snapped = imba_athlas_native_px_h(px_h);
    fabsf(px_h - snapped) < 0.01
}

#[inline]
fn bucket_scale_for_px_h(bucket: &AthlasDecodedBucket, px_h: f32) -> f32 {
    let src_h = bucket.cell_h.max(1) as f32;
    (px_h.max(1.0) / src_h).max(1.0 / src_h)
}

#[inline]
fn glyph_lookup_for_byte(ch: u8) -> Option<AthlasGlyphLookup> {
    imba_athlas_lookup_char(ch as char).or_else(|| imba_athlas_lookup_char('?'))
}

#[inline]
fn draw_bucket_quad(
    tex_id: u32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    uv: [f32; 4],
    rgba: Rgba8,
    view_w: u32,
    view_h: u32,
) -> bool {
    let mut verts = Vec::with_capacity(6 * TEX_VERTEX_SIZE);
    push_tex_quad_px(
        &mut verts,
        ViewTransform {
            width: view_w.max(1) as f32,
            height: view_h.max(1) as f32,
        },
        x0,
        y0,
        x1,
        y1,
        uv,
        rgba,
    );
    let rc = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_draw_tex_triangles_no_present(
            tex_id,
            verts.as_ptr(),
            verts.len(),
        )
    };
    rc == 0
}

pub fn draw_imba_athlas_text_in_frame(
    text: &[u8],
    x: f32,
    y: f32,
    view_w: u32,
    view_h: u32,
) -> bool {
    draw_imba_athlas_text_in_frame_alpha(text, x, y, view_w, view_h, 255)
}

pub fn imba_athlas_text_width_px(text: &[u8]) -> f32 {
    imba_athlas_text_width_nearest_px(text, IMBA_ATHLAS_LARGE_TILE_H)
}

pub fn imba_athlas_text_width_scaled_px(text: &[u8], px_h: f32) -> f32 {
    imba_athlas_text_width_nearest_px(text, px_h)
}

pub fn imba_athlas_text_width_nearest_px(text: &[u8], px_h: f32) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let size_case = imba_athlas_size_case_for_px_h(px_h);
    let mut line_w = 0.0f32;
    let mut max_w = 0.0f32;
    for &ch in text {
        if ch == b'\n' {
            max_w = max_w.max(line_w);
            line_w = 0.0;
            continue;
        }
        let Some(glyph) = glyph_lookup_for_byte(ch) else {
            continue;
        };
        let Some(bucket) = decoded_bucket(size_case, glyph.bucket as usize) else {
            continue;
        };
        let scale = bucket_scale_for_px_h(bucket, px_h);
        line_w += bucket.cell_w as f32 * scale;
    }
    max_w.max(line_w)
}

#[inline]
fn blend_rgba_pixel(dst: &mut [u8], dst_idx: usize, rgba: (u8, u8, u8, u8), coverage: u8) {
    if dst_idx + 3 >= dst.len() || coverage == 0 || rgba.3 == 0 {
        return;
    }

    let src_a = ((rgba.3 as u32) * (coverage as u32) + 127) / 255;
    if src_a == 0 {
        return;
    }
    let inv = 255u32.saturating_sub(src_a);
    let dst_r = dst[dst_idx] as u32;
    let dst_g = dst[dst_idx + 1] as u32;
    let dst_b = dst[dst_idx + 2] as u32;
    let dst_a = dst[dst_idx + 3] as u32;

    dst[dst_idx] = (((rgba.0 as u32) * src_a + dst_r * inv + 127) / 255).min(255) as u8;
    dst[dst_idx + 1] = (((rgba.1 as u32) * src_a + dst_g * inv + 127) / 255).min(255) as u8;
    dst[dst_idx + 2] = (((rgba.2 as u32) * src_a + dst_b * inv + 127) / 255).min(255) as u8;
    dst[dst_idx + 3] = (src_a + ((dst_a * inv + 127) / 255)).min(255) as u8;
}

pub fn blit_imba_athlas_text_rgba(
    dst: &mut [u8],
    dst_w: u32,
    dst_h: u32,
    text: &[u8],
    x: i32,
    y: i32,
    rgba: (u8, u8, u8, u8),
) -> bool {
    blit_imba_athlas_text_rgba_nearest_px(dst, dst_w, dst_h, text, x, y, IMBA_ATHLAS_LARGE_TILE_H, rgba)
}

pub fn blit_imba_athlas_text_rgba_nearest_px(
    dst: &mut [u8],
    dst_w: u32,
    dst_h: u32,
    text: &[u8],
    x: i32,
    y: i32,
    px_h: f32,
    rgba: (u8, u8, u8, u8),
) -> bool {
    if text.is_empty() || dst_w == 0 || dst_h == 0 {
        return false;
    }
    let expected = (dst_w as usize)
        .saturating_mul(dst_h as usize)
        .saturating_mul(4);
    if dst.len() < expected {
        return false;
    }
    let size_case = imba_athlas_size_case_for_px_h(px_h);
    let line_h = ceilf(px_h.max(1.0)).max(1.0) as i32;
    let mut pen_x = x;
    let mut pen_y = y;
    let base_x = x;
    let mut touched = false;

    for &ch in text {
        if ch == b'\n' {
            pen_x = base_x;
            pen_y += line_h;
            continue;
        }
        let Some(glyph) = glyph_lookup_for_byte(ch) else {
            continue;
        };
        let Some(bucket) = decoded_bucket(size_case, glyph.bucket as usize) else {
            continue;
        };
        let scale = bucket_scale_for_px_h(bucket, px_h);
        let draw_w = ceilf(bucket.cell_w as f32 * scale).max(1.0) as i32;
        let draw_h = ceilf(bucket.cell_h as f32 * scale).max(1.0) as i32;
        let advance = draw_w;
        if ch == b' ' {
            pen_x += advance.max(1);
            continue;
        }
        let slot = glyph.slot as usize;
        let cell_x = (slot % bucket.grid_w as usize) as i32 * bucket.cell_w as i32;
        let cell_y = (slot / bucket.grid_w as usize) as i32 * bucket.cell_h as i32;
        for row in 0..draw_h {
            let dst_y_px = pen_y + row;
            if dst_y_px < 0 || dst_y_px >= dst_h as i32 {
                continue;
            }
            let src_row = (floorf((row as f32) / scale) as i32).clamp(0, bucket.cell_h as i32 - 1);
            for col in 0..draw_w {
                let dst_x_px = pen_x + col;
                if dst_x_px < 0 || dst_x_px >= dst_w as i32 {
                    continue;
                }
                let src_col = (floorf((col as f32) / scale) as i32).clamp(0, bucket.cell_w as i32 - 1);
                let src_x = cell_x + src_col;
                let src_y = cell_y + src_row;
                let src_idx = (src_y as usize)
                    .saturating_mul(bucket.width as usize)
                    .saturating_add(src_x as usize);
                let Some(&coverage) = bucket.alpha.get(src_idx) else {
                    continue;
                };
                if coverage == 0 {
                    continue;
                }
                let dst_idx = ((dst_y_px as usize)
                    .saturating_mul(dst_w as usize)
                    .saturating_add(dst_x_px as usize))
                .saturating_mul(4);
                blend_rgba_pixel(dst, dst_idx, rgba, coverage);
                touched = true;
            }
        }
        pen_x += advance.max(1);
    }

    touched
}

pub fn draw_imba_athlas_text_in_frame_alpha(
    text: &[u8],
    x: f32,
    y: f32,
    view_w: u32,
    view_h: u32,
    alpha: u8,
) -> bool {
    draw_imba_athlas_text_in_frame_alpha_nearest_px(
        text,
        x,
        y,
        view_w,
        view_h,
        IMBA_ATHLAS_LARGE_TILE_H,
        alpha,
    )
}

pub fn draw_imba_athlas_text_in_frame_alpha_nearest_px(
    text: &[u8],
    x: f32,
    y: f32,
    view_w: u32,
    view_h: u32,
    px_h: f32,
    alpha: u8,
) -> bool {
    if !imba_athlas_is_native_px_h(px_h) {
        crate::log!(
            "imba-athlas: refused scaled nearest draw requested_px_h={} native_px_h={}\n",
            px_h,
            imba_athlas_native_px_h(px_h)
        );
        return false;
    }

    draw_imba_athlas_text_in_frame_alpha_blend_nearest_px(
        text,
        x,
        y,
        view_w,
        view_h,
        px_h,
        alpha,
        0x0302,
        0x0303,
    )
}

pub fn draw_imba_athlas_text_in_frame_alpha_blend_scaled_px(
    text: &[u8],
    x: f32,
    y: f32,
    view_w: u32,
    view_h: u32,
    px_h: f32,
    alpha: u8,
    src_blend: u32,
    dst_blend: u32,
) -> bool {
    draw_imba_athlas_text_in_frame_alpha_blend_nearest_px(
        text, x, y, view_w, view_h, px_h, alpha, src_blend, dst_blend,
    )
}

pub fn draw_imba_athlas_text_in_frame_alpha_blend_nearest_px(
    text: &[u8],
    x: f32,
    y: f32,
    view_w: u32,
    view_h: u32,
    px_h: f32,
    alpha: u8,
    src_blend: u32,
    dst_blend: u32,
) -> bool {
    if text.is_empty() || !imba_athlas_png_buckets_uploaded() {
        return false;
    }

    let _ = unsafe { crate::r::io::cabi::trueos_cabi_gfx_set_sampler(0, 0, 0, 0) };
    let _ = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_set_blend(
            1, src_blend, dst_blend, src_blend, dst_blend, 0, 0,
        )
    };

    let size_case = imba_athlas_size_case_for_px_h(px_h);
    let line_h = ceilf(px_h.max(1.0)).max(1.0);
    let mut pen_x = x;
    let mut pen_y = y;
    let base_x = x;
    let color = Rgba8::new(0, 0, 0, alpha);
    let mut drew = false;

    for &ch in text {
        if ch == b'\n' {
            pen_x = base_x;
            pen_y += line_h;
            continue;
        }
        let Some(glyph) = glyph_lookup_for_byte(ch) else {
            continue;
        };
        let Some(bucket) = decoded_bucket(size_case, glyph.bucket as usize) else {
            continue;
        };
        let scale = bucket_scale_for_px_h(bucket, px_h);
        let draw_w = (bucket.cell_w as f32 * scale).max(1.0);
        let draw_h = (bucket.cell_h as f32 * scale).max(1.0);
        let advance = draw_w;
        if ch == b' ' {
            pen_x += advance.max(1.0);
            continue;
        }
        let slot = glyph.slot as usize;
        let sx = (slot % bucket.grid_w as usize) as f32;
        let sy = (slot / bucket.grid_w as usize) as f32;
        let px0 = sx * bucket.cell_w as f32;
        let py0 = sy * bucket.cell_h as f32;
        let uv = [
            px0 / bucket.width as f32,
            py0 / bucket.height as f32,
            (px0 + bucket.cell_w as f32) / bucket.width as f32,
            (py0 + bucket.cell_h as f32) / bucket.height as f32,
        ];
        drew |= draw_bucket_quad(
            bucket.tex_id,
            pen_x,
            pen_y,
            pen_x + draw_w,
            pen_y + draw_h,
            uv,
            color,
            view_w,
            view_h,
        );
        pen_x += advance.max(1.0);
    }

    drew
}

pub fn draw_imba_athlas_text_in_frame_alpha_scaled(
    text: &[u8],
    x: f32,
    y: f32,
    view_w: u32,
    view_h: u32,
    px_h: f32,
    alpha: u8,
) -> bool {
    draw_imba_athlas_text_in_frame_alpha_blend_scaled_px(
        text,
        x,
        y,
        view_w,
        view_h,
        px_h,
        alpha,
        0x0302,
        0x0303,
    )
}

pub fn draw_imba_athlas_text(text: &[u8], x: f32, y: f32) -> bool {
    let (view_w, view_h) = crate::limine::framebuffer_response()
        .and_then(|resp| resp.framebuffers().next())
        .map(|fb| (fb.width() as u32, fb.height() as u32))
        .unwrap_or((1024, 768));

    let begin_rc = unsafe { crate::r::io::cabi::trueos_cabi_gfx_begin_frame(0xFFFFFF) };
    if begin_rc != 0 {
        return false;
    }

    let ok = draw_imba_athlas_text_in_frame(text, x, y, view_w, view_h);
    let end_rc = unsafe { crate::r::io::cabi::trueos_cabi_gfx_end_frame() };
    ok && end_rc == 0
}
