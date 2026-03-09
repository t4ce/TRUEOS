#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::{CStr, c_char};
use core::sync::atomic::{AtomicU32, Ordering};
use parry2d::math::Isometry;
use parry2d::query;
use parry2d::shape::Ball;
use spin::Mutex;

use crate as qjs;

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    fn trueos_cabi_gfx_begin_frame(clear_rgb: u32) -> i32;
    fn trueos_cabi_gfx_end_frame() -> i32;
    fn trueos_cabi_gfx_set_blend(
        enabled: u32,
        src_rgb: u32,
        dst_rgb: u32,
        src_alpha: u32,
        dst_alpha: u32,
        eq_rgb: u32,
        eq_alpha: u32,
    ) -> i32;
    fn trueos_cabi_gfx_draw_rgb_triangles_no_present(vtx_ptr: *const u8, vtx_len: usize) -> i32;
    fn trueos_cabi_gfx_draw_tex_triangles_no_present(
        tex_id: u32,
        vtx_ptr: *const u8,
        vtx_len: usize,
    ) -> i32;
    fn trueos_cabi_gfx_upload_texture_rgba(
        tex_id: u32,
        width: u32,
        height: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;
    fn trueos_cabi_gfx_set_sampler(
        wrap_u: u32,
        wrap_v: u32,
        min_filter: u32,
        mag_filter: u32,
    ) -> i32;
    fn trueos_cabi_gfx_draw_lyon_icon_no_present(
        icon_id: u32,
        color_id: u32,
        small_set: u32,
        x: f32,
        y: f32,
        view_w: u32,
        view_h: u32,
    ) -> i32;
    fn trueos_cabi_gfx_bake_lyon_icon_rgba(
        icon_id: u32,
        color_id: u32,
        small_set: u32,
        out_ptr: *mut u8,
        out_len: usize,
    ) -> i32;
    fn trueos_cabi_gfx_present_owner_get() -> u32;
}
static CMD_STREAM_CLEAR_RGB: AtomicU32 = AtomicU32::new(0xFFFFFF);
static CMD_STREAM_VIEW_W: AtomicU32 = AtomicU32::new(1280);
static CMD_STREAM_VIEW_H: AtomicU32 = AtomicU32::new(800);
static CMD_STREAM_BLEND_MODE: AtomicU32 = AtomicU32::new(0);
static CMD_STREAM_PMA: AtomicU32 = AtomicU32::new(0);
static CMD_STREAM_BLEND_ENABLED: AtomicU32 = AtomicU32::new(1);
static CMD_STREAM_NEXT_TEX_ID: AtomicU32 = AtomicU32::new(16);
static CMD_STREAM_TEX_IDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());
static CMD_STREAM_ATLAS_TRACE_LOGS: AtomicU32 = AtomicU32::new(0);
static CMD_STREAM_TEXT_DRAW_LOGS: AtomicU32 = AtomicU32::new(0);
const CMD_STREAM_VERBOSE_TEXT_LOGS: bool = false;
const CMD_STREAM_TEXT_CACHE_CAP: usize = 16;

struct CmdStreamTextMeshCacheEntry {
    kind: u32,
    view_w: u32,
    view_h: u32,
    px_h_bits: u32,
    rgb: u32,
    alpha: u8,
    text: Vec<u8>,
    verts: Arc<[u8]>,
}

static CMD_STREAM_TEXT_MESH_CACHE: Mutex<Vec<CmdStreamTextMeshCacheEntry>> = Mutex::new(Vec::new());

struct CmdStreamAtlasGlyphMeta {
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    glyph_w_px: f32,
    advance_px: f32,
}

struct CmdStreamAtlasGlyphMetaTable {
    width: u32,
    height: u32,
    cell_w: u32,
    cell_h: u32,
    grid_w: u32,
    grid_h: u32,
    index_len: usize,
    widths_len: usize,
    slots_by_char: [u16; 256],
    glyphs: Vec<CmdStreamAtlasGlyphMeta>,
}

static CMD_STREAM_ATLAS_META_SMALL: Mutex<Option<CmdStreamAtlasGlyphMetaTable>> = Mutex::new(None);
static CMD_STREAM_ATLAS_META_LARGE: Mutex<Option<CmdStreamAtlasGlyphMetaTable>> = Mutex::new(None);

struct CmdStreamAtlasTexRecord {
    tex_id: u32,
    kind: u32,
}

static CMD_STREAM_ATLAS_TEX_RECS: Mutex<Vec<CmdStreamAtlasTexRecord>> = Mutex::new(Vec::new());

struct CmdStreamTextBatchRun {
    tex_id: u32,
    verts: Vec<u8>,
}

struct CmdStreamLyonIconTexRecord {
    icon_id: u32,
    color_id: u32,
    small_set: u32,
    tex_id: u32,
    side_px: u32,
}

struct CmdStreamLyonUnitQuadRecord {
    view_w: u32,
    view_h: u32,
    side_px: u32,
    r: u8,
    g: u8,
    b: u8,
    verts: Arc<[u8]>,
}

static CMD_STREAM_TEXT_BATCH_RUNS: Mutex<Vec<CmdStreamTextBatchRun>> = Mutex::new(Vec::new());
static CMD_STREAM_WIDGET_TEXT_ATLAS_TEX_ID: AtomicU32 = AtomicU32::new(0);
static CMD_STREAM_LYON_ICON_TEX_RECS: Mutex<Vec<CmdStreamLyonIconTexRecord>> = Mutex::new(Vec::new());
static CMD_STREAM_LYON_UNIT_QUAD_RECS: Mutex<Vec<CmdStreamLyonUnitQuadRecord>> = Mutex::new(Vec::new());

const CMD_STREAM_DEFAULT_BLEND_MODE: u32 = 0;
const CMD_STREAM_DEFAULT_PMA: u32 = 0;
const CMD_STREAM_DEFAULT_BLEND_ENABLED: u32 = 0;
const CMD_STREAM_DEFAULT_WRAP_U: u32 = 0;
const CMD_STREAM_DEFAULT_WRAP_V: u32 = 0;
const CMD_STREAM_DEFAULT_MIN_FILTER: u32 = 1;
const CMD_STREAM_DEFAULT_MAG_FILTER: u32 = 1;

#[inline]
fn cmd_stream_apply_blend_mode(mode: u32, pma: bool) {
    match mode {
        // Add
        1 => {
            let _ = unsafe { trueos_cabi_gfx_set_blend(1, 0x0302, 1, 1, 1, 0, 0) };
        }
        // Multiply
        2 => {
            let _ = unsafe { trueos_cabi_gfx_set_blend(1, 0x0306, 0x0303, 0x0306, 0x0303, 0, 0) };
        }
        // Screen
        3 => {
            let _ = unsafe { trueos_cabi_gfx_set_blend(1, 1, 0x0301, 1, 0x0301, 0, 0) };
        }
        // Normal
        _ => {
            if pma {
                let _ = unsafe { trueos_cabi_gfx_set_blend(1, 1, 0x0303, 1, 0x0303, 0, 0) };
            } else {
                let _ =
                    unsafe { trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0) };
            }
        }
    }
}

#[inline]
fn cmd_stream_reset_frame_state_defaults() {
    CMD_STREAM_BLEND_MODE.store(CMD_STREAM_DEFAULT_BLEND_MODE, Ordering::Relaxed);
    CMD_STREAM_PMA.store(CMD_STREAM_DEFAULT_PMA, Ordering::Relaxed);
    CMD_STREAM_BLEND_ENABLED.store(CMD_STREAM_DEFAULT_BLEND_ENABLED, Ordering::Relaxed);

    let _ = unsafe {
        trueos_cabi_gfx_set_sampler(
            CMD_STREAM_DEFAULT_WRAP_U,
            CMD_STREAM_DEFAULT_WRAP_V,
            CMD_STREAM_DEFAULT_MIN_FILTER,
            CMD_STREAM_DEFAULT_MAG_FILTER,
        )
    };

    if CMD_STREAM_DEFAULT_BLEND_ENABLED != 0 {
        cmd_stream_apply_blend_mode(CMD_STREAM_DEFAULT_BLEND_MODE, CMD_STREAM_DEFAULT_PMA != 0);
    } else {
        let _ = unsafe { trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0) };
    }
}

#[inline]
fn cmd_stream_alloc_tex_id() -> u32 {
    let id = CMD_STREAM_NEXT_TEX_ID.fetch_add(1, Ordering::AcqRel);
    CMD_STREAM_TEX_IDS.lock().push(id);
    id
}

#[inline]
fn cmd_stream_is_managed_tex(id: u32) -> bool {
    if id == 0 {
        return false;
    }
    CMD_STREAM_TEX_IDS.lock().iter().copied().any(|v| v == id)
}

#[inline]
fn cmd_stream_release_tex_id(id: u32) {
    let mut ids = CMD_STREAM_TEX_IDS.lock();
    if let Some(pos) = ids.iter().position(|v| *v == id) {
        ids.swap_remove(pos);
    }
    let mut atlas = CMD_STREAM_ATLAS_TEX_RECS.lock();
    if let Some(pos) = atlas.iter().position(|r| r.tex_id == id) {
        atlas.swap_remove(pos);
    }
}

#[inline]
unsafe fn cmd_stream_read_f32_slice_from_value(
    ctx: *mut qjs::JSContext,
    value: qjs::JSValueConst,
) -> Option<(*mut f32, usize, qjs::JSValue)> {
    let mut byte_off: usize = 0;
    let mut byte_len: usize = 0;
    let mut bpe: usize = 0;
    let ab = qjs::JS_GetTypedArrayBuffer(
        ctx,
        value,
        &mut byte_off as *mut usize,
        &mut byte_len as *mut usize,
        &mut bpe as *mut usize,
    );

    if ab.is_exception() || ab.tag == qjs::JS_TAG_UNDEFINED || ab.tag == qjs::JS_TAG_NULL {
        if !ab.is_exception() {
            qjs::js_free_value(ctx, ab);
        }
        return None;
    }

    let mut buf_len: usize = 0;
    let ptr = qjs::JS_GetArrayBuffer(ctx, &mut buf_len as *mut usize, ab);
    if ptr.is_null() {
        qjs::js_free_value(ctx, ab);
        return None;
    }

    let usable = core::cmp::min(byte_len, buf_len.saturating_sub(byte_off));
    if usable < 4 || (byte_off & 3) != 0 {
        qjs::js_free_value(ctx, ab);
        return None;
    }
    let f32_len = usable / 4;
    if f32_len == 0 {
        qjs::js_free_value(ctx, ab);
        return None;
    }

    Some((ptr.add(byte_off) as *mut f32, f32_len, ab))
}

#[inline]
fn cmd_stream_select_atlas(kind: u32) -> Option<qjs::FontAtlasView<'static>> {
    if kind == 0 {
        qjs::font_atlas_small_view()
    } else {
        qjs::font_atlas_large_view().or_else(qjs::font_atlas_small_view)
    }
}

#[inline]
fn cmd_stream_atlas_meta_slot(kind: u32) -> &'static Mutex<Option<CmdStreamAtlasGlyphMetaTable>> {
    if kind == 0 {
        &CMD_STREAM_ATLAS_META_SMALL
    } else {
        &CMD_STREAM_ATLAS_META_LARGE
    }
}

#[inline]
fn cmd_stream_atlas_meta_kind(kind: u32) -> u32 {
    if kind == 0 { 0 } else { 1 }
}

#[inline]
fn cmd_stream_atlas_meta_is_compatible(
    table: &CmdStreamAtlasGlyphMetaTable,
    atlas: qjs::FontAtlasView<'static>,
) -> bool {
    table.width == atlas.width
        && table.height == atlas.height
        && table.cell_w == atlas.cell_w
        && table.cell_h == atlas.cell_h
        && table.grid_w == atlas.grid_w
        && table.grid_h == atlas.grid_h
        && table.index_len == atlas.index.len()
        && table.widths_len == atlas.widths.len()
}

fn cmd_stream_build_atlas_meta(atlas: qjs::FontAtlasView<'static>) -> CmdStreamAtlasGlyphMetaTable {
    let aw = (atlas.width.max(1)) as f32;
    let ah = (atlas.height.max(1)) as f32;
    let grid_w = atlas.grid_w.max(1);
    let grid_h = atlas.grid_h.max(1);
    let fallback_slot = atlas.index.get(b'?' as usize).copied().unwrap_or(0);
    let cell_w_f = atlas.cell_w as f32;
    let cell_h_f = atlas.cell_h.max(1) as f32;

    let mut slots_by_char = [fallback_slot; 256];
    for (i, slot) in slots_by_char.iter_mut().enumerate() {
        let mut s = atlas.index.get(i).copied().unwrap_or(fallback_slot);
        if s == u16::MAX {
            s = fallback_slot;
        }
        *slot = s;
    }

    let glyph_count = (grid_w as usize).saturating_mul(grid_h as usize);
    let mut glyphs = Vec::with_capacity(glyph_count);
    for slot in 0..glyph_count {
        let sx = (slot as u32) % grid_w;
        let sy = (slot as u32) / grid_w;
        let glyph_w_px = atlas
            .widths
            .get(slot)
            .copied()
            .unwrap_or(atlas.cell_w as u8) as f32;
        let px0 = (sx as f32) * cell_w_f;
        let py0 = (sy as f32) * cell_h_f;
        let px1 = px0 + glyph_w_px;
        let py1 = py0 + cell_h_f;
        glyphs.push(CmdStreamAtlasGlyphMeta {
            u0: px0 / aw,
            v0: py0 / ah,
            u1: px1 / aw,
            v1: py1 / ah,
            glyph_w_px,
            advance_px: glyph_w_px,
        });
    }

    CmdStreamAtlasGlyphMetaTable {
        width: atlas.width,
        height: atlas.height,
        cell_w: atlas.cell_w,
        cell_h: atlas.cell_h,
        grid_w: atlas.grid_w,
        grid_h: atlas.grid_h,
        index_len: atlas.index.len(),
        widths_len: atlas.widths.len(),
        slots_by_char,
        glyphs,
    }
}

fn cmd_stream_atlas_meta_get_or_build(
    kind: u32,
    atlas: qjs::FontAtlasView<'static>,
) -> spin::MutexGuard<'static, Option<CmdStreamAtlasGlyphMetaTable>> {
    let slot = cmd_stream_atlas_meta_slot(kind);
    let mut guard = slot.lock();
    let rebuild = guard
        .as_ref()
        .map(|t| !cmd_stream_atlas_meta_is_compatible(t, atlas))
        .unwrap_or(true);
    if rebuild {
        *guard = Some(cmd_stream_build_atlas_meta(atlas));
    }
    guard
}

#[inline]
fn cmd_stream_atlas_meta_lookup(
    table: &CmdStreamAtlasGlyphMetaTable,
    ch: u8,
) -> Option<&CmdStreamAtlasGlyphMeta> {
    let slot = table.slots_by_char[ch as usize] as usize;
    table.glyphs.get(slot)
}

#[inline]
fn cmd_stream_upload_atlas_to_tex(tex_id: u32, atlas: qjs::FontAtlasView<'static>) -> bool {
    let px = atlas.alpha.len();
    if px == 0 {
        return false;
    }
    let mut rgba = Vec::with_capacity(px.saturating_mul(4));
    for &a in atlas.alpha.iter() {
        // Atlas-mask upload: keep coverage in red.
        // FS_TEX uses min(sample.r, sample.a), so this stays robust even if
        // sampled alpha handling differs across virgl paths.
        rgba.push(a);
        rgba.push(0);
        rgba.push(0);
        rgba.push(255);
    }
    let rc = unsafe {
        trueos_cabi_gfx_upload_texture_rgba(
            tex_id,
            atlas.width,
            atlas.height,
            rgba.as_ptr(),
            rgba.len(),
        )
    };
    rc == 0
}

#[inline]
fn cmd_stream_mark_atlas_tex(tex_id: u32, kind: u32) {
    let mut recs = CMD_STREAM_ATLAS_TEX_RECS.lock();
    if let Some(rec) = recs.iter_mut().find(|r| r.tex_id == tex_id) {
        rec.kind = kind;
        return;
    }
    recs.push(CmdStreamAtlasTexRecord { tex_id, kind });
}

#[inline]
fn cmd_stream_refresh_atlas_tex_if_needed(tex_id: u32, requested_kind: u32) {
    let mut recs = CMD_STREAM_ATLAS_TEX_RECS.lock();
    let Some(rec) = recs.iter_mut().find(|r| r.tex_id == tex_id) else {
        return;
    };
    if rec.kind == requested_kind {
        return;
    }
    let Some(atlas) = cmd_stream_select_atlas(requested_kind) else {
        return;
    };
    if cmd_stream_upload_atlas_to_tex(tex_id, atlas) {
        rec.kind = requested_kind;
    }
}

#[inline]
fn cmd_stream_push_tex_vtx(
    out: &mut Vec<u8>,
    x: f32,
    y: f32,
    u: f32,
    v: f32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) {
    out.extend_from_slice(&x.to_le_bytes());
    out.extend_from_slice(&y.to_le_bytes());
    out.extend_from_slice(&u.to_le_bytes());
    out.extend_from_slice(&v.to_le_bytes());
    out.push(r);
    out.push(g);
    out.push(b);
    out.push(a);
}


#[inline]
fn cmd_stream_clear_text_batches() {
    CMD_STREAM_TEXT_BATCH_RUNS.lock().clear();
}

#[inline]
fn cmd_stream_push_tex_vertices_with_origin(
    out: &mut Vec<u8>,
    verts: &[u8],
    origin_x_ndc: f32,
    origin_y_ndc: f32,
) {
    const STRIDE: usize = 20;
    let mut off = 0usize;
    while off + STRIDE <= verts.len() {
        let mut xb = [0u8; 4];
        xb.copy_from_slice(&verts[off..off + 4]);
        let mut yb = [0u8; 4];
        yb.copy_from_slice(&verts[off + 4..off + 8]);
        let x = f32::from_le_bytes(xb) + origin_x_ndc;
        let y = f32::from_le_bytes(yb) + origin_y_ndc;
        out.extend_from_slice(&x.to_le_bytes());
        out.extend_from_slice(&y.to_le_bytes());
        out.extend_from_slice(&verts[off + 8..off + STRIDE]);
        off += STRIDE;
    }
}

#[inline]
fn cmd_stream_enqueue_text_batch(tex_id: u32, verts: &[u8], origin_x_ndc: f32, origin_y_ndc: f32) {
    if tex_id == 0 || verts.is_empty() {
        return;
    }
    let mut runs = CMD_STREAM_TEXT_BATCH_RUNS.lock();
    if let Some(last) = runs.last_mut()
        && last.tex_id == tex_id
    {
        cmd_stream_push_tex_vertices_with_origin(
            &mut last.verts,
            verts,
            origin_x_ndc,
            origin_y_ndc,
        );
        return;
    }
    let mut out = Vec::with_capacity(verts.len());
    cmd_stream_push_tex_vertices_with_origin(&mut out, verts, origin_x_ndc, origin_y_ndc);
    runs.push(CmdStreamTextBatchRun { tex_id, verts: out });
}

#[inline]
fn cmd_stream_flush_text_batches() {
    let mut runs = CMD_STREAM_TEXT_BATCH_RUNS.lock();
    for run in runs.iter() {
        if run.tex_id == 0 || run.verts.is_empty() {
            continue;
        }
        let _ = unsafe {
            trueos_cabi_gfx_draw_tex_triangles_no_present(
                run.tex_id,
                run.verts.as_ptr(),
                run.verts.len(),
            )
        };
    }
    runs.clear();
}

#[inline]
fn cmd_stream_enable_alpha_text_blend() {
    // Text atlas quads need standard alpha blending to avoid opaque glyph boxes.
    CMD_STREAM_BLEND_MODE.store(0, Ordering::Relaxed);
    CMD_STREAM_PMA.store(0, Ordering::Relaxed);
    CMD_STREAM_BLEND_ENABLED.store(1, Ordering::Relaxed);
    cmd_stream_apply_blend_mode(0, false);
}

fn cmd_stream_draw_atlas_text_impl(
    tex_id: u32,
    kind: u32,
    x_f: f64,
    y_f: f64,
    text: &[u8],
    px_h: f64,
    rgb_f: f64,
    alpha_f: f64,
) -> bool {
    if tex_id == 0 || text.is_empty() {
        return false;
    }

    cmd_stream_refresh_atlas_tex_if_needed(tex_id, kind);
    let Some(atlas) = cmd_stream_select_atlas(kind) else {
        return false;
    };

    let draw_log_n = CMD_STREAM_TEXT_DRAW_LOGS.fetch_add(1, Ordering::Relaxed);
    if CMD_STREAM_VERBOSE_TEXT_LOGS && draw_log_n < 24 {
        let preview = if let Ok(s) = core::str::from_utf8(text) {
            s
        } else {
            "<non-utf8>"
        };
        let msg = alloc::format!(
            "cmd-stream: draw-text tex={} x={} y={} len={} text=\"{}\"\n",
            tex_id,
            x_f as i32,
            y_f as i32,
            text.len(),
            preview
        );
        log_str(msg.as_str());
    }

    let view_w = CMD_STREAM_VIEW_W.load(Ordering::Relaxed).max(1);
    let view_h = CMD_STREAM_VIEW_H.load(Ordering::Relaxed).max(1);
    let w = view_w as f32;
    let h = view_h as f32;
    let grid_w = atlas.grid_w.max(1);
    let _ = px_h;
    let scale = 1.0f32;
    let rgb = ((rgb_f as i64).max(0) as u32) & 0x00FF_FFFF;
    let r = ((rgb >> 16) & 0xFF) as u8;
    let g = ((rgb >> 8) & 0xFF) as u8;
    let b = (rgb & 0xFF) as u8;
    let a = ((alpha_f as i64).clamp(0, 255)) as u8;
    let px_h_bits = (px_h as f32).to_bits();
    let origin_x_ndc = (2.0 * ((x_f as f32) / w)) - 1.0;
    let origin_y_ndc = 1.0 - (2.0 * ((y_f as f32) / h));

    let verts = cmd_stream_text_cache_get(kind, view_w, view_h, px_h_bits, rgb, a, text)
        .unwrap_or_else(|| {
            let fallback = atlas.index.get(b'?' as usize).copied().unwrap_or(0);
            let mut pen_x = 0.0f32;
            let pen_y = 0.0f32;
            let mut out = Vec::with_capacity(text.len().saturating_mul(6 * 20));
            let atlas_w_f = (atlas.width.max(1)) as f32;
            let atlas_h_f = (atlas.height.max(1)) as f32;
            let atlas_cell_h_u = atlas.cell_h as usize;
            let meta_kind = cmd_stream_atlas_meta_kind(kind);
            let meta_guard = cmd_stream_atlas_meta_get_or_build(meta_kind, atlas);
            let meta_table = meta_guard.as_ref();

            for &ch in text.iter() {
                if ch == b'\n' {
                    pen_x = 0.0;
                    continue;
                }
                if ch == b' ' {
                    pen_x += (atlas.cell_w as f32) * scale * 0.6;
                    continue;
                }
                if let Some(table) = meta_table
                    && let Some(gm) = cmd_stream_atlas_meta_lookup(table, ch)
                {
                    let x0 = pen_x;
                    let y0 = pen_y;
                    let x1 = pen_x + gm.glyph_w_px * scale;
                    let y1 = pen_y + (atlas_cell_h_u as f32).max(1.0) * scale;
                    let nx0 = 2.0 * (x0 / w);
                    let ny0 = -(2.0 * (y0 / h));
                    let nx1 = 2.0 * (x1 / w);
                    let ny1 = -(2.0 * (y1 / h));

                    cmd_stream_push_tex_vtx(&mut out, nx0, ny1, gm.u0, gm.v1, r, g, b, a);
                    cmd_stream_push_tex_vtx(&mut out, nx1, ny1, gm.u1, gm.v1, r, g, b, a);
                    cmd_stream_push_tex_vtx(&mut out, nx1, ny0, gm.u1, gm.v0, r, g, b, a);
                    cmd_stream_push_tex_vtx(&mut out, nx0, ny1, gm.u0, gm.v1, r, g, b, a);
                    cmd_stream_push_tex_vtx(&mut out, nx1, ny0, gm.u1, gm.v0, r, g, b, a);
                    cmd_stream_push_tex_vtx(&mut out, nx0, ny0, gm.u0, gm.v0, r, g, b, a);
                    pen_x += gm.advance_px * scale;
                    continue;
                }

                let mut slot = atlas.index.get(ch as usize).copied().unwrap_or(fallback);
                if slot == u16::MAX {
                    slot = fallback;
                }
                let glyph_w_u = atlas
                    .widths
                    .get(slot as usize)
                    .copied()
                    .unwrap_or(atlas.cell_w as u8) as usize;
                let glyph_h_u = atlas_cell_h_u.max(1);

                let sx = (slot as u32) % grid_w;
                let sy = (slot as u32) / grid_w;
                let px0 = (sx as f32) * (atlas.cell_w as f32);
                let py0 = (sy as f32) * (atlas.cell_h as f32);
                let px1 = px0 + (glyph_w_u as f32);
                let py1 = py0 + (glyph_h_u as f32);

                let u0 = px0 / atlas_w_f;
                let v0 = py0 / atlas_h_f;
                let u1 = px1 / atlas_w_f;
                let v1 = py1 / atlas_h_f;

                let x0 = pen_x;
                let y0 = pen_y;
                let x1 = pen_x + (glyph_w_u as f32) * scale;
                let y1 = pen_y + (glyph_h_u as f32) * scale;
                let nx0 = 2.0 * (x0 / w);
                let ny0 = -(2.0 * (y0 / h));
                let nx1 = 2.0 * (x1 / w);
                let ny1 = -(2.0 * (y1 / h));

                cmd_stream_push_tex_vtx(&mut out, nx0, ny1, u0, v1, r, g, b, a);
                cmd_stream_push_tex_vtx(&mut out, nx1, ny1, u1, v1, r, g, b, a);
                cmd_stream_push_tex_vtx(&mut out, nx1, ny0, u1, v0, r, g, b, a);
                cmd_stream_push_tex_vtx(&mut out, nx0, ny1, u0, v1, r, g, b, a);
                cmd_stream_push_tex_vtx(&mut out, nx1, ny0, u1, v0, r, g, b, a);
                cmd_stream_push_tex_vtx(&mut out, nx0, ny0, u0, v0, r, g, b, a);
                pen_x += glyph_w_u as f32 * scale;
            }

            let cached: Arc<[u8]> = Arc::from(out.into_boxed_slice());
            cmd_stream_text_cache_put(CmdStreamTextMeshCacheEntry {
                kind,
                view_w,
                view_h,
                px_h_bits,
                rgb,
                alpha: a,
                text: text.to_vec(),
                verts: cached.clone(),
            });
            cached
        });

    if verts.is_empty() {
        return false;
    }
    cmd_stream_enqueue_text_batch(tex_id, verts.as_ref(), origin_x_ndc, origin_y_ndc);
    true
}

pub fn draw_text_widget(text: &[u8], x: f32, y: f32) -> bool {
    if text.is_empty() {
        return false;
    }

    let kind = 1u32;
    let Some(tex_id) = cmd_stream_ensure_atlas_tex(kind) else {
        return false;
    };

    cmd_stream_clear_text_batches();
    let clear = CMD_STREAM_CLEAR_RGB.load(Ordering::Relaxed);
    let _ = unsafe { trueos_cabi_gfx_begin_frame(clear) };
    cmd_stream_reset_frame_state_defaults();
    cmd_stream_enable_alpha_text_blend();
    let ok = cmd_stream_draw_atlas_text_impl(
        tex_id,
        kind,
        x as f64,
        y as f64,
        text,
        26.0,
        0x101010 as f64,
        255.0,
    );
    cmd_stream_flush_text_batches();
    let _ = unsafe { trueos_cabi_gfx_end_frame() };
    ok
}

#[inline]
fn cmd_stream_ensure_atlas_tex(kind: u32) -> Option<u32> {
    let cached = CMD_STREAM_WIDGET_TEXT_ATLAS_TEX_ID.load(Ordering::Acquire);
    if cached != 0 {
        return Some(cached);
    }

    let atlas = cmd_stream_select_atlas(kind)?;
    let id = cmd_stream_alloc_tex_id();
    if !cmd_stream_upload_atlas_to_tex(id, atlas) {
        cmd_stream_release_tex_id(id);
        return None;
    }
    cmd_stream_mark_atlas_tex(id, kind);
    let _ = CMD_STREAM_WIDGET_TEXT_ATLAS_TEX_ID.compare_exchange(
        0,
        id,
        Ordering::AcqRel,
        Ordering::Acquire,
    );
    Some(CMD_STREAM_WIDGET_TEXT_ATLAS_TEX_ID.load(Ordering::Acquire))
}

pub fn draw_atlas_text_in_frame(
    text: &[u8],
    x: f32,
    y: f32,
    view_w: u32,
    view_h: u32,
    kind: u32,
    px_h: f64,
    rgb: u32,
    alpha: u8,
) -> bool {
    if text.is_empty() {
        return false;
    }

    let Some(tex_id) = cmd_stream_ensure_atlas_tex(kind) else {
        return false;
    };

    CMD_STREAM_VIEW_W.store(view_w.max(1), Ordering::Relaxed);
    CMD_STREAM_VIEW_H.store(view_h.max(1), Ordering::Relaxed);

    cmd_stream_clear_text_batches();
    cmd_stream_reset_frame_state_defaults();
    cmd_stream_enable_alpha_text_blend();
    let ok = cmd_stream_draw_atlas_text_impl(
        tex_id,
        kind,
        x as f64,
        y as f64,
        text,
        px_h,
        rgb as f64,
        alpha as f64,
    );
    cmd_stream_flush_text_batches();
    ok
}

pub fn draw_lyon_in_frame(
    icon_id: u32,
    x: f32,
    y: f32,
    view_w: u32,
    view_h: u32,
    color_id: u32,
) -> bool {
    CMD_STREAM_VIEW_W.store(view_w.max(1), Ordering::Relaxed);
    CMD_STREAM_VIEW_H.store(view_h.max(1), Ordering::Relaxed);

    // Preserve in-frame ordering with queued atlas text, but do not mutate global
    // frame state here (blend/sampler), since callers may have configured it.
    let Some((tex_id, side_px)) = cmd_stream_ensure_lyon_icon_tex(icon_id, 0, 1) else {
        return false;
    };
    let (r, g, b) = cmd_stream_lyon_palette_rgb(color_id);

    let view_w_u = view_w.max(1);
    let view_h_u = view_h.max(1);
    let view_w_f = view_w_u as f32;
    let view_h_f = view_h_u as f32;
    let origin_x_ndc = (2.0 * (x / view_w_f)) - 1.0;
    let origin_y_ndc = 1.0 - (2.0 * (y / view_h_f));

    let verts = cmd_stream_get_lyon_unit_quad_verts(view_w_u, view_h_u, side_px, r, g, b);
    cmd_stream_enqueue_text_batch(tex_id, verts.as_ref(), origin_x_ndc, origin_y_ndc);
    true
}

#[inline]
fn cmd_stream_lyon_palette_rgb(color_id: u32) -> (u8, u8, u8) {
    match (color_id % 5) as usize {
        0 => (0, 0, 0),
        1 => (217, 46, 46),
        2 => (31, 158, 56),
        3 => (31, 82, 217),
        _ => (242, 140, 31),
    }
}

#[inline]
fn cmd_stream_ensure_lyon_icon_tex(icon_id: u32, color_id: u32, small_set: u32) -> Option<(u32, u32)> {
    {
        let recs = CMD_STREAM_LYON_ICON_TEX_RECS.lock();
        if let Some(rec) = recs
            .iter()
            .find(|r| r.icon_id == icon_id && r.color_id == color_id && r.small_set == small_set)
        {
            return Some((rec.tex_id, rec.side_px));
        }
    }

    let need = unsafe {
        trueos_cabi_gfx_bake_lyon_icon_rgba(icon_id, color_id, small_set, core::ptr::null_mut(), 0)
    };
    if need <= 0 {
        return None;
    }
    let need = need as usize;
    if need % 4 != 0 {
        return None;
    }
    let px_count = need / 4;
    let side = if small_set != 0 { 16usize } else { 32usize };
    if side.saturating_mul(side) != px_count {
        return None;
    }

    let mut rgba = vec![0u8; need];
    let wrote = unsafe {
        trueos_cabi_gfx_bake_lyon_icon_rgba(icon_id, color_id, small_set, rgba.as_mut_ptr(), rgba.len())
    };
    if wrote != need as i32 {
        return None;
    }

    let tex_id = cmd_stream_alloc_tex_id();
    let rc = unsafe {
        trueos_cabi_gfx_upload_texture_rgba(
            tex_id,
            side as u32,
            side as u32,
            rgba.as_ptr(),
            rgba.len(),
        )
    };
    if rc != 0 {
        cmd_stream_release_tex_id(tex_id);
        return None;
    }

    CMD_STREAM_LYON_ICON_TEX_RECS.lock().push(CmdStreamLyonIconTexRecord {
        icon_id,
        color_id,
        small_set,
        tex_id,
        side_px: side as u32,
    });
    Some((tex_id, side as u32))
}

#[inline]
fn cmd_stream_get_lyon_unit_quad_verts(
    view_w: u32,
    view_h: u32,
    side_px: u32,
    r: u8,
    g: u8,
    b: u8,
) -> Arc<[u8]> {
    {
        let recs = CMD_STREAM_LYON_UNIT_QUAD_RECS.lock();
        if let Some(rec) = recs.iter().find(|rec| {
            rec.view_w == view_w
                && rec.view_h == view_h
                && rec.side_px == side_px
                && rec.r == r
                && rec.g == g
                && rec.b == b
        }) {
            return rec.verts.clone();
        }
    }

    let vw = view_w.max(1) as f32;
    let vh = view_h.max(1) as f32;
    let side = side_px.max(1) as f32;
    let dx = 2.0 * (side / vw);
    let dy = 2.0 * (side / vh);

    let mut verts = Vec::with_capacity(6 * 20);
    // Local quad in NDC delta-space with top-left origin at (0, 0).
    cmd_stream_push_tex_vtx(&mut verts, 0.0, -dy, 0.0, 1.0, r, g, b, 255);
    cmd_stream_push_tex_vtx(&mut verts, dx, -dy, 1.0, 1.0, r, g, b, 255);
    cmd_stream_push_tex_vtx(&mut verts, dx, 0.0, 1.0, 0.0, r, g, b, 255);
    cmd_stream_push_tex_vtx(&mut verts, 0.0, -dy, 0.0, 1.0, r, g, b, 255);
    cmd_stream_push_tex_vtx(&mut verts, dx, 0.0, 1.0, 0.0, r, g, b, 255);
    cmd_stream_push_tex_vtx(&mut verts, 0.0, 0.0, 0.0, 0.0, r, g, b, 255);

    let out: Arc<[u8]> = Arc::from(verts.into_boxed_slice());
    let mut recs = CMD_STREAM_LYON_UNIT_QUAD_RECS.lock();
    recs.push(CmdStreamLyonUnitQuadRecord {
        view_w,
        view_h,
        side_px,
        r,
        g,
        b,
        verts: out.clone(),
    });
    if recs.len() > 64 {
        let excess = recs.len() - 64;
        recs.drain(0..excess);
    }
    out
}

#[inline]
fn cmd_stream_draw_tex_quad_no_present(
    tex_id: u32,
    x: f32,
    y: f32,
    w_px: f32,
    h_px: f32,
    r: u8,
    g: u8,
    b: u8,
) -> bool {
    if tex_id == 0 || w_px <= 0.0 || h_px <= 0.0 {
        return false;
    }
    let view_w = CMD_STREAM_VIEW_W.load(Ordering::Relaxed).max(1) as f32;
    let view_h = CMD_STREAM_VIEW_H.load(Ordering::Relaxed).max(1) as f32;

    let nx0 = (2.0 * (x / view_w)) - 1.0;
    let ny0 = 1.0 - (2.0 * (y / view_h));
    let nx1 = (2.0 * ((x + w_px) / view_w)) - 1.0;
    let ny1 = 1.0 - (2.0 * ((y + h_px) / view_h));

    let mut verts = Vec::with_capacity(6 * 20);
    cmd_stream_push_tex_vtx(&mut verts, nx0, ny1, 0.0, 1.0, r, g, b, 255);
    cmd_stream_push_tex_vtx(&mut verts, nx1, ny1, 1.0, 1.0, r, g, b, 255);
    cmd_stream_push_tex_vtx(&mut verts, nx1, ny0, 1.0, 0.0, r, g, b, 255);
    cmd_stream_push_tex_vtx(&mut verts, nx0, ny1, 0.0, 1.0, r, g, b, 255);
    cmd_stream_push_tex_vtx(&mut verts, nx1, ny0, 1.0, 0.0, r, g, b, 255);
    cmd_stream_push_tex_vtx(&mut verts, nx0, ny0, 0.0, 0.0, r, g, b, 255);

    let rc = unsafe { trueos_cabi_gfx_draw_tex_triangles_no_present(tex_id, verts.as_ptr(), verts.len()) };
    rc == 0
}

#[inline]
fn cmd_stream_text_cache_get(
    kind: u32,
    view_w: u32,
    view_h: u32,
    px_h_bits: u32,
    rgb: u32,
    alpha: u8,
    text: &[u8],
) -> Option<Arc<[u8]>> {
    let mut cache = CMD_STREAM_TEXT_MESH_CACHE.lock();
    let pos = cache.iter().position(|e| {
        e.kind == kind
            && e.view_w == view_w
            && e.view_h == view_h
            && e.px_h_bits == px_h_bits
            && e.rgb == rgb
            && e.alpha == alpha
            && e.text.as_slice() == text
    })?;
    let entry = cache.swap_remove(pos);
    let verts = entry.verts.clone();
    cache.push(entry);
    Some(verts)
}

#[inline]
fn cmd_stream_text_cache_put(entry: CmdStreamTextMeshCacheEntry) {
    if entry.verts.is_empty() || entry.text.is_empty() {
        return;
    }
    let mut cache = CMD_STREAM_TEXT_MESH_CACHE.lock();
    cache.push(entry);
    if cache.len() > CMD_STREAM_TEXT_CACHE_CAP {
        let excess = cache.len() - CMD_STREAM_TEXT_CACHE_CAP;
        cache.drain(0..excess);
    }
}

#[inline]
fn log_bytes(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    unsafe { trueos_cabi_write(1, bytes.as_ptr(), bytes.len()) };
}

#[inline]
fn log_str(s: &str) {
    log_bytes(s.as_bytes());
}

#[inline]
pub(crate) unsafe fn try_create_native_module(
    ctx: *mut qjs::JSContext,
    module_name: *const c_char,
) -> *mut qjs::JSModuleDef {
    if ctx.is_null() || module_name.is_null() {
        return core::ptr::null_mut();
    }
    let name = CStr::from_ptr(module_name).to_bytes();
    if name == b"cmd_stream" || name == b"trueos:cmd_stream" {
        unsafe extern "C" fn qjs_cmd_stream_begin_frame(
            _ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            _argc: i32,
            _argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            cmd_stream_clear_text_batches();
            let clear = CMD_STREAM_CLEAR_RGB.load(Ordering::Relaxed);
            let _ = trueos_cabi_gfx_begin_frame(clear);
            cmd_stream_reset_frame_state_defaults();
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_end_frame(
            _ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            _argc: i32,
            _argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            cmd_stream_flush_text_batches();
            let _ = trueos_cabi_gfx_end_frame();
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_cursor_begin_frame(
            _ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            _argc: i32,
            _argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            // Legacy cursor-plane custom command path is disabled.
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_cursor_end_frame(
            _ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            _argc: i32,
            _argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            // Legacy cursor-plane custom command path is disabled.
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_set_clear_rgb(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            if argv.is_null() || argc < 1 {
                return qjs::JSValue::undefined();
            }
            let args = core::slice::from_raw_parts(argv, argc as usize);
            let mut v_f: f64 = 0.0;
            if qjs::JS_ToFloat64(ctx, &mut v_f as *mut f64, args[0]) != 0 {
                return qjs::JSValue::undefined();
            }
            let rgb = (v_f as i64).max(0) as u32 & 0x00FF_FFFF;
            CMD_STREAM_CLEAR_RGB.store(rgb, Ordering::Relaxed);
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_set_viewport(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            if argv.is_null() || argc < 2 {
                return qjs::JSValue::undefined();
            }
            let args = core::slice::from_raw_parts(argv, argc as usize);
            let mut w_f: f64 = 0.0;
            let mut h_f: f64 = 0.0;
            if qjs::JS_ToFloat64(ctx, &mut w_f as *mut f64, args[0]) != 0
                || qjs::JS_ToFloat64(ctx, &mut h_f as *mut f64, args[1]) != 0
            {
                return qjs::JSValue::undefined();
            }
            let w = (w_f as i64).max(1) as u32;
            let h = (h_f as i64).max(1) as u32;
            CMD_STREAM_VIEW_W.store(w, Ordering::Relaxed);
            CMD_STREAM_VIEW_H.store(h, Ordering::Relaxed);
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_set_blend_enabled(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            if argv.is_null() || argc < 1 {
                return qjs::JSValue::undefined();
            }
            let args = core::slice::from_raw_parts(argv, argc as usize);
            let mut enabled_f: f64 = 0.0;
            if qjs::JS_ToFloat64(ctx, &mut enabled_f as *mut f64, args[0]) != 0 {
                return qjs::JSValue::undefined();
            }
            cmd_stream_flush_text_batches();
            if enabled_f != 0.0 {
                CMD_STREAM_BLEND_ENABLED.store(1, Ordering::Relaxed);
                let mode = CMD_STREAM_BLEND_MODE.load(Ordering::Relaxed);
                let pma = CMD_STREAM_PMA.load(Ordering::Relaxed) != 0;
                cmd_stream_apply_blend_mode(mode, pma);
            } else {
                CMD_STREAM_BLEND_ENABLED.store(0, Ordering::Relaxed);
                // disabled
                let _ = trueos_cabi_gfx_set_blend(0, 1, 0, 1, 0, 0, 0);
            }
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_set_sampler(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            if argv.is_null() || argc < 4 {
                return qjs::JSValue::undefined();
            }
            let args = core::slice::from_raw_parts(argv, argc as usize);
            let mut wrap_u = 0.0f64;
            let mut wrap_v = 0.0f64;
            let mut min_f = 0.0f64;
            let mut mag_f = 0.0f64;
            if qjs::JS_ToFloat64(ctx, &mut wrap_u as *mut f64, args[0]) != 0
                || qjs::JS_ToFloat64(ctx, &mut wrap_v as *mut f64, args[1]) != 0
                || qjs::JS_ToFloat64(ctx, &mut min_f as *mut f64, args[2]) != 0
                || qjs::JS_ToFloat64(ctx, &mut mag_f as *mut f64, args[3]) != 0
            {
                return qjs::JSValue::undefined();
            }
            cmd_stream_flush_text_batches();
            let _ = trueos_cabi_gfx_set_sampler(
                (wrap_u as i64).max(0) as u32,
                (wrap_v as i64).max(0) as u32,
                (min_f as i64).max(0) as u32,
                (mag_f as i64).max(0) as u32,
            );
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_set_blend_mode(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            if argv.is_null() || argc < 1 {
                return qjs::JSValue::undefined();
            }
            let args = core::slice::from_raw_parts(argv, argc as usize);
            let mut mode_f: f64 = 0.0;
            if qjs::JS_ToFloat64(ctx, &mut mode_f as *mut f64, args[0]) != 0 {
                return qjs::JSValue::undefined();
            }
            cmd_stream_flush_text_batches();
            let mode = ((mode_f as i64).clamp(0, 3)) as u32;
            CMD_STREAM_BLEND_MODE.store(mode, Ordering::Relaxed);
            if CMD_STREAM_BLEND_ENABLED.load(Ordering::Relaxed) != 0 {
                let pma = CMD_STREAM_PMA.load(Ordering::Relaxed) != 0;
                cmd_stream_apply_blend_mode(mode, pma);
            }
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_set_premultiplied_alpha(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            if argv.is_null() || argc < 1 {
                return qjs::JSValue::undefined();
            }
            let args = core::slice::from_raw_parts(argv, argc as usize);
            let mut pma_f: f64 = 0.0;
            if qjs::JS_ToFloat64(ctx, &mut pma_f as *mut f64, args[0]) != 0 {
                return qjs::JSValue::undefined();
            }
            cmd_stream_flush_text_batches();
            CMD_STREAM_PMA.store(if pma_f != 0.0 { 1 } else { 0 }, Ordering::Relaxed);
            if CMD_STREAM_BLEND_ENABLED.load(Ordering::Relaxed) != 0 {
                let mode = CMD_STREAM_BLEND_MODE.load(Ordering::Relaxed);
                cmd_stream_apply_blend_mode(mode, pma_f != 0.0);
            }
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_draw_triangles_u8(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            if argv.is_null() || argc < 1 {
                return qjs::JSValue::undefined();
            }
            cmd_stream_flush_text_batches();
            let args = core::slice::from_raw_parts(argv, argc as usize);

            let mut byte_off: usize = 0;
            let mut byte_len: usize = 0;
            let mut bpe: usize = 0;
            let ab = qjs::JS_GetTypedArrayBuffer(
                ctx,
                args[0],
                &mut byte_off as *mut usize,
                &mut byte_len as *mut usize,
                &mut bpe as *mut usize,
            );

            if !ab.is_exception() && ab.tag != qjs::JS_TAG_UNDEFINED && ab.tag != qjs::JS_TAG_NULL {
                let mut buf_len: usize = 0;
                let ptr = qjs::JS_GetArrayBuffer(ctx, &mut buf_len as *mut usize, ab);
                if !ptr.is_null() {
                    let usable = core::cmp::min(byte_len, buf_len.saturating_sub(byte_off));
                    let _ = trueos_cabi_gfx_draw_rgb_triangles_no_present(
                        ptr.add(byte_off) as *const u8,
                        usable,
                    );
                }
                qjs::js_free_value(ctx, ab);
                return qjs::JSValue::undefined();
            }
            if !ab.is_exception() {
                qjs::js_free_value(ctx, ab);
            }

            let mut len: usize = 0;
            let ptr = qjs::JS_GetArrayBuffer(ctx, &mut len as *mut usize, args[0]);
            if !ptr.is_null() && len > 0 {
                let _ = trueos_cabi_gfx_draw_rgb_triangles_no_present(ptr as *const u8, len);
            }
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_draw_textured_triangles_u8(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            if argv.is_null() || argc < 2 {
                return qjs::JSValue::undefined();
            }
            cmd_stream_flush_text_batches();
            let args = core::slice::from_raw_parts(argv, argc as usize);

            let mut tex_id_f: f64 = 0.0;
            if qjs::JS_ToFloat64(ctx, &mut tex_id_f as *mut f64, args[0]) != 0 {
                return qjs::JSValue::undefined();
            }
            let tex_id = (tex_id_f as i64).max(0) as u32;
            if tex_id == 0 {
                return qjs::JSValue::undefined();
            }

            let mut byte_off: usize = 0;
            let mut byte_len: usize = 0;
            let mut bpe: usize = 0;
            let ab = qjs::JS_GetTypedArrayBuffer(
                ctx,
                args[1],
                &mut byte_off as *mut usize,
                &mut byte_len as *mut usize,
                &mut bpe as *mut usize,
            );

            if !ab.is_exception() && ab.tag != qjs::JS_TAG_UNDEFINED && ab.tag != qjs::JS_TAG_NULL {
                let mut buf_len: usize = 0;
                let ptr = qjs::JS_GetArrayBuffer(ctx, &mut buf_len as *mut usize, ab);
                if !ptr.is_null() {
                    let usable = core::cmp::min(byte_len, buf_len.saturating_sub(byte_off));
                    let _ = trueos_cabi_gfx_draw_tex_triangles_no_present(
                        tex_id,
                        ptr.add(byte_off) as *const u8,
                        usable,
                    );
                }
                qjs::js_free_value(ctx, ab);
                return qjs::JSValue::undefined();
            }
            if !ab.is_exception() {
                qjs::js_free_value(ctx, ab);
            }

            let mut len: usize = 0;
            let ptr = qjs::JS_GetArrayBuffer(ctx, &mut len as *mut usize, args[1]);
            if !ptr.is_null() && len > 0 {
                let _ =
                    trueos_cabi_gfx_draw_tex_triangles_no_present(tex_id, ptr as *const u8, len);
            }
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_cursor_draw_triangles_u8(
            _ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            _argc: i32,
            _argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            // Legacy cursor-plane custom command path is disabled.
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_cursor_draw_textured_triangles_u8(
            _ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            _argc: i32,
            _argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            // Legacy cursor-plane custom command path is disabled.
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_create_texture_rgba(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            if argv.is_null() || argc < 3 {
                return qjs::JSValue::undefined();
            }
            let args = core::slice::from_raw_parts(argv, argc as usize);
            let mut w_f: f64 = 0.0;
            let mut h_f: f64 = 0.0;
            if qjs::JS_ToFloat64(ctx, &mut w_f as *mut f64, args[0]) != 0
                || qjs::JS_ToFloat64(ctx, &mut h_f as *mut f64, args[1]) != 0
            {
                return qjs::JSValue::undefined();
            }
            let w = (w_f as i64).max(1) as u32;
            let h = (h_f as i64).max(1) as u32;
            let need = (w as usize).saturating_mul(h as usize).saturating_mul(4);
            if need == 0 {
                return qjs::JSValue::undefined();
            }
            let tex_id = cmd_stream_alloc_tex_id();

            let mut byte_off: usize = 0;
            let mut byte_len: usize = 0;
            let mut bpe: usize = 0;
            let ab = qjs::JS_GetTypedArrayBuffer(
                ctx,
                args[2],
                &mut byte_off as *mut usize,
                &mut byte_len as *mut usize,
                &mut bpe as *mut usize,
            );
            let mut uploaded = false;
            if !ab.is_exception() && ab.tag != qjs::JS_TAG_UNDEFINED && ab.tag != qjs::JS_TAG_NULL {
                let mut buf_len: usize = 0;
                let ptr = qjs::JS_GetArrayBuffer(ctx, &mut buf_len as *mut usize, ab);
                if !ptr.is_null() {
                    let usable = core::cmp::min(byte_len, buf_len.saturating_sub(byte_off));
                    if usable >= need {
                        uploaded = true;
                        let _ = trueos_cabi_gfx_upload_texture_rgba(
                            tex_id,
                            w,
                            h,
                            ptr.add(byte_off) as *const u8,
                            need,
                        );
                    }
                }
                qjs::js_free_value(ctx, ab);
            }
            if !uploaded {
                let mut len: usize = 0;
                let ptr = qjs::JS_GetArrayBuffer(ctx, &mut len as *mut usize, args[2]);
                if !ptr.is_null() && len >= need {
                    uploaded = true;
                    let _ =
                        trueos_cabi_gfx_upload_texture_rgba(tex_id, w, h, ptr as *const u8, need);
                }
            }
            if !uploaded {
                cmd_stream_release_tex_id(tex_id);
                return qjs::JSValue::undefined();
            }
            qjs::JS_NewFloat64(ctx, tex_id as f64)
        }

        unsafe extern "C" fn qjs_cmd_stream_update_texture_rgba(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            if argv.is_null() || argc < 4 {
                return qjs::JSValue::undefined();
            }
            let args = core::slice::from_raw_parts(argv, argc as usize);
            let mut tex_id_f: f64 = 0.0;
            let mut w_f: f64 = 0.0;
            let mut h_f: f64 = 0.0;
            if qjs::JS_ToFloat64(ctx, &mut tex_id_f as *mut f64, args[0]) != 0
                || qjs::JS_ToFloat64(ctx, &mut w_f as *mut f64, args[1]) != 0
                || qjs::JS_ToFloat64(ctx, &mut h_f as *mut f64, args[2]) != 0
            {
                return qjs::JSValue::undefined();
            }
            let tex_id = (tex_id_f as i64).max(0) as u32;
            let w = (w_f as i64).max(1) as u32;
            let h = (h_f as i64).max(1) as u32;
            let need = (w as usize).saturating_mul(h as usize).saturating_mul(4);
            if !cmd_stream_is_managed_tex(tex_id) || need == 0 {
                return qjs::JSValue::undefined();
            }

            let mut byte_off: usize = 0;
            let mut byte_len: usize = 0;
            let mut bpe: usize = 0;
            let ab = qjs::JS_GetTypedArrayBuffer(
                ctx,
                args[3],
                &mut byte_off as *mut usize,
                &mut byte_len as *mut usize,
                &mut bpe as *mut usize,
            );
            if !ab.is_exception() && ab.tag != qjs::JS_TAG_UNDEFINED && ab.tag != qjs::JS_TAG_NULL {
                let mut buf_len: usize = 0;
                let ptr = qjs::JS_GetArrayBuffer(ctx, &mut buf_len as *mut usize, ab);
                if !ptr.is_null() {
                    let usable = core::cmp::min(byte_len, buf_len.saturating_sub(byte_off));
                    if usable >= need {
                        let _ = trueos_cabi_gfx_upload_texture_rgba(
                            tex_id,
                            w,
                            h,
                            ptr.add(byte_off) as *const u8,
                            need,
                        );
                    }
                }
                qjs::js_free_value(ctx, ab);
                return qjs::JSValue::undefined();
            }
            if !ab.is_exception() {
                qjs::js_free_value(ctx, ab);
            }

            let mut len: usize = 0;
            let ptr = qjs::JS_GetArrayBuffer(ctx, &mut len as *mut usize, args[3]);
            if !ptr.is_null() && len >= need {
                let _ = trueos_cabi_gfx_upload_texture_rgba(tex_id, w, h, ptr as *const u8, need);
            }
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_destroy_texture(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            if argv.is_null() || argc < 1 {
                return qjs::JSValue::undefined();
            }
            let args = core::slice::from_raw_parts(argv, argc as usize);
            let mut tex_id_f: f64 = 0.0;
            if qjs::JS_ToFloat64(ctx, &mut tex_id_f as *mut f64, args[0]) != 0 {
                return qjs::JSValue::undefined();
            }
            let tex_id = (tex_id_f as i64).max(0) as u32;
            if !cmd_stream_is_managed_tex(tex_id) {
                return qjs::JSValue::undefined();
            }
            let clear = [0u8, 0, 0, 0];
            let _ = trueos_cabi_gfx_upload_texture_rgba(tex_id, 1, 1, clear.as_ptr(), clear.len());
            cmd_stream_release_tex_id(tex_id);
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_create_atlas_texture(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            let mut kind: u32 = 1;
            if !argv.is_null() && argc >= 1 {
                let args = core::slice::from_raw_parts(argv, argc as usize);
                let mut kind_f: f64 = 0.0;
                if qjs::JS_ToFloat64(ctx, &mut kind_f as *mut f64, args[0]) == 0 {
                    kind = (kind_f as i64).max(0) as u32;
                }
            }
            let Some(atlas) = cmd_stream_select_atlas(kind) else {
                return qjs::JSValue::undefined();
            };
            let tex_id = cmd_stream_alloc_tex_id();
            if !cmd_stream_upload_atlas_to_tex(tex_id, atlas) {
                cmd_stream_release_tex_id(tex_id);
                return qjs::JSValue::undefined();
            }
            cmd_stream_mark_atlas_tex(tex_id, kind);
            qjs::JS_NewFloat64(ctx, tex_id as f64)
        }

        unsafe extern "C" fn qjs_cmd_stream_draw_atlas_text(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            if argv.is_null() || argc < 5 {
                return qjs::JSValue::undefined();
            }
            let args = core::slice::from_raw_parts(argv, argc as usize);

            let mut tex_id_f: f64 = 0.0;
            let mut kind_f: f64 = 1.0;
            let mut x_f: f64 = 0.0;
            let mut y_f: f64 = 0.0;
            if qjs::JS_ToFloat64(ctx, &mut tex_id_f as *mut f64, args[0]) != 0
                || qjs::JS_ToFloat64(ctx, &mut kind_f as *mut f64, args[1]) != 0
                || qjs::JS_ToFloat64(ctx, &mut x_f as *mut f64, args[2]) != 0
                || qjs::JS_ToFloat64(ctx, &mut y_f as *mut f64, args[3]) != 0
            {
                return qjs::JSValue::undefined();
            }
            let tex_id = (tex_id_f as i64).max(0) as u32;
            if tex_id == 0 {
                return qjs::JSValue::undefined();
            }
            let kind = (kind_f as i64).max(0) as u32;
            cmd_stream_refresh_atlas_tex_if_needed(tex_id, kind);

            let mut text_len: usize = 0;
            let text_c = qjs::JS_ToCStringLen2(ctx, &mut text_len as *mut usize, args[4], 0);
            if text_c.is_null() || text_len == 0 {
                if !text_c.is_null() {
                    qjs::JS_FreeCString(ctx, text_c);
                }
                return qjs::JSValue::undefined();
            }

            let mut px_h: f64 = 26.0;
            if argc >= 6 {
                let _ = qjs::JS_ToFloat64(ctx, &mut px_h as *mut f64, args[5]);
            }
            let mut rgb_f: f64 = 0x101010 as f64;
            if argc >= 7 {
                let _ = qjs::JS_ToFloat64(ctx, &mut rgb_f as *mut f64, args[6]);
            }
            let mut alpha_f: f64 = 255.0;
            if argc >= 8 {
                let _ = qjs::JS_ToFloat64(ctx, &mut alpha_f as *mut f64, args[7]);
            }

            let Some(atlas) = cmd_stream_select_atlas(kind) else {
                qjs::JS_FreeCString(ctx, text_c);
                return qjs::JSValue::undefined();
            };
            let text = core::slice::from_raw_parts(text_c as *const u8, text_len);
            let draw_log_n = CMD_STREAM_TEXT_DRAW_LOGS.fetch_add(1, Ordering::Relaxed);
            if CMD_STREAM_VERBOSE_TEXT_LOGS && draw_log_n < 24 {
                let preview = if let Ok(s) = core::str::from_utf8(text) {
                    s
                } else {
                    "<non-utf8>"
                };
                let msg = alloc::format!(
                    "cmd-stream: draw-text tex={} x={} y={} len={} text=\"{}\"\n",
                    tex_id,
                    x_f as i32,
                    y_f as i32,
                    text_len,
                    preview
                );
                log_str(msg.as_str());
            }
            if CMD_STREAM_VERBOSE_TEXT_LOGS
                && CMD_STREAM_ATLAS_TRACE_LOGS.load(Ordering::Relaxed) < 8
                && !text.is_empty()
            {
                let first = text[0];
                let fallback_slot = atlas.index.get(b'?' as usize).copied().unwrap_or(0);
                let mut slot = atlas
                    .index
                    .get(first as usize)
                    .copied()
                    .unwrap_or(fallback_slot);
                if slot == u16::MAX {
                    slot = fallback_slot;
                }
                let grid_w = atlas.grid_w.max(1);
                let sx = (slot as u32) % grid_w;
                let sy = (slot as u32) / grid_w;
                let px0 = (sx * atlas.cell_w) as usize;
                let py0 = (sy * atlas.cell_h) as usize;
                let cell_w = atlas.cell_w as usize;
                let cell_h = atlas.cell_h as usize;
                let aw = atlas.width as usize;
                let mut nz = 0usize;
                let mut a_min = 255u8;
                let mut a_max = 0u8;
                for y in 0..cell_h {
                    for x in 0..cell_w {
                        let ix = (py0 + y).saturating_mul(aw).saturating_add(px0 + x);
                        if let Some(&a) = atlas.alpha.get(ix) {
                            if a != 0 {
                                nz += 1;
                            }
                            if a < a_min {
                                a_min = a;
                            }
                            if a > a_max {
                                a_max = a;
                            }
                        }
                    }
                }
                let glyph_w = atlas
                    .widths
                    .get(slot as usize)
                    .copied()
                    .unwrap_or(atlas.cell_w as u8);
                let msg = alloc::format!(
                    "cmd-stream: atlas-trace ch={} slot={} tex={} cell={}x{} gw={} nz={}/{} a=[{},{}]\n",
                    first,
                    slot,
                    tex_id,
                    atlas.cell_w,
                    atlas.cell_h,
                    glyph_w,
                    nz,
                    cell_w.saturating_mul(cell_h),
                    a_min,
                    a_max
                );
                log_str(msg.as_str());
                CMD_STREAM_ATLAS_TRACE_LOGS.fetch_add(1, Ordering::Relaxed);
            }
            let view_w = CMD_STREAM_VIEW_W.load(Ordering::Relaxed).max(1);
            let view_h = CMD_STREAM_VIEW_H.load(Ordering::Relaxed).max(1);
            let w = view_w as f32;
            let h = view_h as f32;
            let grid_w = atlas.grid_w.max(1);
            // Keep atlas-native text size for now; ignore requested fontSize.
            let _ = px_h;
            let scale = 1.0f32;
            let rgb = ((rgb_f as i64).max(0) as u32) & 0x00FF_FFFF;
            let r = ((rgb >> 16) & 0xFF) as u8;
            let g = ((rgb >> 8) & 0xFF) as u8;
            let b = (rgb & 0xFF) as u8;
            let a = ((alpha_f as i64).clamp(0, 255)) as u8;
            let px_h_bits = (px_h as f32).to_bits();
            let origin_x_ndc = (2.0 * ((x_f as f32) / w)) - 1.0;
            let origin_y_ndc = 1.0 - (2.0 * ((y_f as f32) / h));

            let verts = cmd_stream_text_cache_get(kind, view_w, view_h, px_h_bits, rgb, a, text)
                .unwrap_or_else(|| {
                    let fallback = atlas.index.get(b'?' as usize).copied().unwrap_or(0);
                    let mut pen_x = 0.0f32;
                    let pen_y = 0.0f32;
                    let mut out = Vec::with_capacity(text.len().saturating_mul(6 * 20));
                    let atlas_w_f = (atlas.width.max(1)) as f32;
                    let atlas_h_f = (atlas.height.max(1)) as f32;
                    let atlas_cell_h_u = atlas.cell_h as usize;
                    let meta_kind = cmd_stream_atlas_meta_kind(kind);
                    let meta_guard = cmd_stream_atlas_meta_get_or_build(meta_kind, atlas);
                    let meta_table = meta_guard.as_ref();

                    for &ch in text.iter() {
                        if ch == b'\n' {
                            pen_x = 0.0;
                            continue;
                        }
                        if ch == b' ' {
                            pen_x += (atlas.cell_w as f32) * scale * 0.6;
                            continue;
                        }
                        if let Some(table) = meta_table
                            && let Some(gm) = cmd_stream_atlas_meta_lookup(table, ch)
                        {
                            let x0 = pen_x;
                            let y0 = pen_y;
                            let x1 = pen_x + gm.glyph_w_px * scale;
                            let y1 = pen_y + (atlas_cell_h_u as f32).max(1.0) * scale;
                            let nx0 = 2.0 * (x0 / w);
                            let ny0 = -(2.0 * (y0 / h));
                            let nx1 = 2.0 * (x1 / w);
                            let ny1 = -(2.0 * (y1 / h));

                            cmd_stream_push_tex_vtx(&mut out, nx0, ny1, gm.u0, gm.v1, r, g, b, a);
                            cmd_stream_push_tex_vtx(&mut out, nx1, ny1, gm.u1, gm.v1, r, g, b, a);
                            cmd_stream_push_tex_vtx(&mut out, nx1, ny0, gm.u1, gm.v0, r, g, b, a);
                            cmd_stream_push_tex_vtx(&mut out, nx0, ny1, gm.u0, gm.v1, r, g, b, a);
                            cmd_stream_push_tex_vtx(&mut out, nx1, ny0, gm.u1, gm.v0, r, g, b, a);
                            cmd_stream_push_tex_vtx(&mut out, nx0, ny0, gm.u0, gm.v0, r, g, b, a);
                            pen_x += gm.advance_px * scale;
                            continue;
                        }
                        // Fallback to direct atlas math if the meta table is unavailable.
                        let mut slot = atlas.index.get(ch as usize).copied().unwrap_or(fallback);
                        if slot == u16::MAX {
                            slot = fallback;
                        }
                        let glyph_w_u = atlas
                            .widths
                            .get(slot as usize)
                            .copied()
                            .unwrap_or(atlas.cell_w as u8)
                            as usize;
                        let glyph_h_u = atlas_cell_h_u.max(1);

                        let sx = (slot as u32) % grid_w;
                        let sy = (slot as u32) / grid_w;
                        let px0 = (sx as f32) * (atlas.cell_w as f32);
                        let py0 = (sy as f32) * (atlas.cell_h as f32);
                        let px1 = px0 + (glyph_w_u as f32);
                        let py1 = py0 + (glyph_h_u as f32);

                        let u0 = px0 / atlas_w_f;
                        let v0 = py0 / atlas_h_f;
                        let u1 = px1 / atlas_w_f;
                        let v1 = py1 / atlas_h_f;

                        let x0 = pen_x;
                        let y0 = pen_y;
                        let x1 = pen_x + (glyph_w_u as f32) * scale;
                        let y1 = pen_y + (glyph_h_u as f32) * scale;
                        let nx0 = 2.0 * (x0 / w);
                        let ny0 = -(2.0 * (y0 / h));
                        let nx1 = 2.0 * (x1 / w);
                        let ny1 = -(2.0 * (y1 / h));

                        cmd_stream_push_tex_vtx(&mut out, nx0, ny1, u0, v1, r, g, b, a);
                        cmd_stream_push_tex_vtx(&mut out, nx1, ny1, u1, v1, r, g, b, a);
                        cmd_stream_push_tex_vtx(&mut out, nx1, ny0, u1, v0, r, g, b, a);
                        cmd_stream_push_tex_vtx(&mut out, nx0, ny1, u0, v1, r, g, b, a);
                        cmd_stream_push_tex_vtx(&mut out, nx1, ny0, u1, v0, r, g, b, a);
                        cmd_stream_push_tex_vtx(&mut out, nx0, ny0, u0, v0, r, g, b, a);
                        pen_x += glyph_w_u as f32 * scale;
                    }

                    let cached: Arc<[u8]> = Arc::from(out.into_boxed_slice());
                    cmd_stream_text_cache_put(CmdStreamTextMeshCacheEntry {
                        kind,
                        view_w,
                        view_h,
                        px_h_bits,
                        rgb,
                        alpha: a,
                        text: text.to_vec(),
                        verts: cached.clone(),
                    });
                    cached
                });

            if verts.is_empty() {
                qjs::JS_FreeCString(ctx, text_c);
                return qjs::JSValue::undefined();
            }
            cmd_stream_enqueue_text_batch(tex_id, verts.as_ref(), origin_x_ndc, origin_y_ndc);
            qjs::JS_FreeCString(ctx, text_c);
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_draw_lyon_icon_in_frame(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            if argv.is_null() || argc < 3 {
                return qjs::JSValue::undefined();
            }
            let args = core::slice::from_raw_parts(argv, argc as usize);

            let mut icon_id_f: f64 = 0.0;
            let mut x_f: f64 = 0.0;
            let mut y_f: f64 = 0.0;
            if qjs::JS_ToFloat64(ctx, &mut icon_id_f as *mut f64, args[0]) != 0
                || qjs::JS_ToFloat64(ctx, &mut x_f as *mut f64, args[1]) != 0
                || qjs::JS_ToFloat64(ctx, &mut y_f as *mut f64, args[2]) != 0
            {
                return qjs::JSValue::undefined();
            }

            let mut color_id_f: f64 = 0.0;
            if argc >= 4 {
                let _ = qjs::JS_ToFloat64(ctx, &mut color_id_f as *mut f64, args[3]);
            }

            let icon_id = (icon_id_f as i64).max(0) as u32;
            let color_id = (color_id_f as i64).max(0) as u32;
            let view_w = CMD_STREAM_VIEW_W.load(Ordering::Relaxed).max(1);
            let view_h = CMD_STREAM_VIEW_H.load(Ordering::Relaxed).max(1);

            let _ = draw_lyon_in_frame(
                icon_id,
                x_f as f32,
                y_f as f32,
                view_w,
                view_h,
                color_id,
            );
            qjs::JSValue::undefined()
        }

        unsafe extern "C" fn qjs_cmd_stream_step_icon_collisions(
            ctx: *mut qjs::JSContext,
            _this_val: qjs::JSValueConst,
            argc: i32,
            argv: *const qjs::JSValueConst,
        ) -> qjs::JSValue {
            if argv.is_null() || argc < 5 {
                return qjs::JS_NewFloat64(ctx, 0.0);
            }
            let args = core::slice::from_raw_parts(argv, argc as usize);

            let Some((pos_ptr, pos_len, pos_ab)) = cmd_stream_read_f32_slice_from_value(ctx, args[0]) else {
                return qjs::JS_NewFloat64(ctx, 0.0);
            };
            let Some((vel_ptr, vel_len, vel_ab)) = cmd_stream_read_f32_slice_from_value(ctx, args[1]) else {
                qjs::js_free_value(ctx, pos_ab);
                return qjs::JS_NewFloat64(ctx, 0.0);
            };

            let mut dt_ms_f: f64 = 16.0;
            let mut icon_size_f: f64 = 16.0;
            let mut restitution_f: f64 = 0.82;
            if qjs::JS_ToFloat64(ctx, &mut dt_ms_f as *mut f64, args[2]) != 0
                || qjs::JS_ToFloat64(ctx, &mut icon_size_f as *mut f64, args[3]) != 0
                || qjs::JS_ToFloat64(ctx, &mut restitution_f as *mut f64, args[4]) != 0
            {
                qjs::js_free_value(ctx, vel_ab);
                qjs::js_free_value(ctx, pos_ab);
                return qjs::JS_NewFloat64(ctx, 0.0);
            }

            let icon_size = (icon_size_f as f32).max(1.0);
            let radius = (icon_size * 0.5).max(0.5);
            let dt = ((dt_ms_f as f32) / 1000.0).clamp(0.0, 0.05);
            let restitution = (restitution_f as f32).clamp(0.0, 1.0);
            let n = core::cmp::min(pos_len, vel_len) / 2;
            if n < 2 {
                qjs::js_free_value(ctx, vel_ab);
                qjs::js_free_value(ctx, pos_ab);
                return qjs::JS_NewFloat64(ctx, 0.0);
            }

            let pos = core::slice::from_raw_parts_mut(pos_ptr, n * 2);
            let vel = core::slice::from_raw_parts_mut(vel_ptr, n * 2);
            let view_w = CMD_STREAM_VIEW_W.load(Ordering::Relaxed).max(1) as f32;
            let view_h = CMD_STREAM_VIEW_H.load(Ordering::Relaxed).max(1) as f32;

            for i in 0..n {
                let b = i * 2;
                pos[b] += vel[b] * dt;
                pos[b + 1] += vel[b + 1] * dt;

                if pos[b] < 0.0 {
                    pos[b] = 0.0;
                    vel[b] = vel[b].abs() * restitution;
                } else if pos[b] + icon_size > view_w {
                    pos[b] = (view_w - icon_size).max(0.0);
                    vel[b] = -vel[b].abs() * restitution;
                }

                if pos[b + 1] < 0.0 {
                    pos[b + 1] = 0.0;
                    vel[b + 1] = vel[b + 1].abs() * restitution;
                } else if pos[b + 1] + icon_size > view_h {
                    pos[b + 1] = (view_h - icon_size).max(0.0);
                    vel[b + 1] = -vel[b + 1].abs() * restitution;
                }
            }

            let shape = Ball::new(radius);
            let mut contacts = 0u32;
            for i in 0..n {
                let ib = i * 2;
                let ci_x = pos[ib] + radius;
                let ci_y = pos[ib + 1] + radius;
                let pi = Isometry::translation(ci_x, ci_y);

                for j in (i + 1)..n {
                    let jb = j * 2;
                    let cj_x = pos[jb] + radius;
                    let cj_y = pos[jb + 1] + radius;
                    let pj = Isometry::translation(cj_x, cj_y);

                    let Ok(Some(c)) = query::contact(&pi, &shape, &pj, &shape, 0.0) else {
                        continue;
                    };

                    if c.dist >= 0.0 {
                        continue;
                    }
                    contacts = contacts.saturating_add(1);

                    let nrm = c.normal1.into_inner();
                    let nx = nrm.x;
                    let ny = nrm.y;

                    let rvx = vel[jb] - vel[ib];
                    let rvy = vel[jb + 1] - vel[ib + 1];
                    let rel = (rvx * nx) + (rvy * ny);
                    if rel < 0.0 {
                        let impulse = -((1.0 + restitution) * rel) * 0.5;
                        vel[ib] -= impulse * nx;
                        vel[ib + 1] -= impulse * ny;
                        vel[jb] += impulse * nx;
                        vel[jb + 1] += impulse * ny;
                    }

                    let penetration = (-c.dist).max(0.0);
                    if penetration > 0.0 {
                        let corr = (penetration * 0.5) + 0.01;
                        pos[ib] -= corr * nx;
                        pos[ib + 1] -= corr * ny;
                        pos[jb] += corr * nx;
                        pos[jb + 1] += corr * ny;
                    }
                }
            }

            qjs::js_free_value(ctx, vel_ab);
            qjs::js_free_value(ctx, pos_ab);
            qjs::JS_NewFloat64(ctx, contacts as f64)
        }

        unsafe extern "C" fn qjs_cmd_stream_module_init(
            ctx: *mut qjs::JSContext,
            m: *mut qjs::JSModuleDef,
        ) -> i32 {
            macro_rules! export_fn {
                ($name:literal, $func:expr, $argc:expr) => {{
                    let k = concat!($name, "\0");
                    let f = qjs::JS_NewCFunction2(
                        ctx,
                        Some($func),
                        k.as_ptr() as *const c_char,
                        $argc,
                        qjs::JS_CFUNC_GENERIC,
                        0,
                    );
                    let _ = qjs::JS_SetModuleExport(ctx, m, k.as_ptr() as *const c_char, f);
                }};
            }
            export_fn!("beginFrame", qjs_cmd_stream_begin_frame, 0);
            export_fn!("endFrame", qjs_cmd_stream_end_frame, 0);
            export_fn!("cursorBeginFrame", qjs_cmd_stream_cursor_begin_frame, 0);
            export_fn!("cursorEndFrame", qjs_cmd_stream_cursor_end_frame, 0);
            export_fn!("setClearRgb", qjs_cmd_stream_set_clear_rgb, 1);
            export_fn!("setViewport", qjs_cmd_stream_set_viewport, 2);
            export_fn!("setBlendEnabled", qjs_cmd_stream_set_blend_enabled, 1);
            export_fn!("setSampler", qjs_cmd_stream_set_sampler, 4);
            export_fn!("setBlendMode", qjs_cmd_stream_set_blend_mode, 1);
            export_fn!(
                "setPremultipliedAlpha",
                qjs_cmd_stream_set_premultiplied_alpha,
                1
            );
            export_fn!("createTextureRgba", qjs_cmd_stream_create_texture_rgba, 3);
            export_fn!("updateTextureRgba", qjs_cmd_stream_update_texture_rgba, 4);
            export_fn!("destroyTexture", qjs_cmd_stream_destroy_texture, 1);
            export_fn!("createAtlasTexture", qjs_cmd_stream_create_atlas_texture, 1);
            export_fn!("drawTrianglesU8", qjs_cmd_stream_draw_triangles_u8, 1);
            export_fn!(
                "cursorDrawTrianglesU8",
                qjs_cmd_stream_cursor_draw_triangles_u8,
                1
            );
            export_fn!(
                "drawTexturedTrianglesU8",
                qjs_cmd_stream_draw_textured_triangles_u8,
                2
            );
            export_fn!(
                "cursorDrawTexturedTrianglesU8",
                qjs_cmd_stream_cursor_draw_textured_triangles_u8,
                2
            );
            export_fn!("drawAtlasText", qjs_cmd_stream_draw_atlas_text, 8);
            export_fn!(
                "drawLyonIconInFrame",
                qjs_cmd_stream_draw_lyon_icon_in_frame,
                4
            );
            export_fn!(
                "stepIconCollisions",
                qjs_cmd_stream_step_icon_collisions,
                5
            );
            0
        }

        let m = qjs::JS_NewCModule(ctx, module_name, Some(qjs_cmd_stream_module_init));
        if m.is_null() {
            return core::ptr::null_mut();
        }
        macro_rules! add_export {
            ($name:literal) => {{
                let k = concat!($name, "\0");
                let _ = qjs::JS_AddModuleExport(ctx, m, k.as_ptr() as *const c_char);
            }};
        }
        add_export!("beginFrame");
        add_export!("endFrame");
        add_export!("cursorBeginFrame");
        add_export!("cursorEndFrame");
        add_export!("setClearRgb");
        add_export!("setViewport");
        add_export!("setBlendEnabled");
        add_export!("setSampler");
        add_export!("setBlendMode");
        add_export!("setPremultipliedAlpha");
        add_export!("createTextureRgba");
        add_export!("updateTextureRgba");
        add_export!("destroyTexture");
        add_export!("createAtlasTexture");
        add_export!("drawTrianglesU8");
        add_export!("cursorDrawTrianglesU8");
        add_export!("drawTexturedTrianglesU8");
        add_export!("cursorDrawTexturedTrianglesU8");
        add_export!("drawAtlasText");
        add_export!("drawLyonIconInFrame");
        add_export!("stepIconCollisions");
        return m;
    }

    core::ptr::null_mut()
}
