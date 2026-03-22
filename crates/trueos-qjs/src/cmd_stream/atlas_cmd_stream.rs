use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use libm::tanf;
use spin::Mutex;

use crate as qjs;

static CMD_STREAM_TEXT_BATCH_RUNS: Mutex<Vec<CmdStreamTextBatchRun>> = Mutex::new(Vec::new());

const CMD_STREAM_TEXT_CACHE_CAP: usize = 16;

struct CmdStreamTextMeshCacheEntry {
    kind: u32,
    view_w: u32,
    view_h: u32,
    px_h_bits: u32,
    rgb: u32,
    alpha: u8,
    italic_tilt_bits: u32,
    bold_mode: u32,
    text: Vec<u8>,
    verts: Arc<[u8]>,
}

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

struct CmdStreamAtlasTexRecord {
    tex_id: u32,
    kind: u32,
}

struct CmdStreamTextBatchRun {
    tex_id: u32,
    verts: Vec<u8>,
}

static CMD_STREAM_TEXT_MESH_CACHE: Mutex<Vec<CmdStreamTextMeshCacheEntry>> = Mutex::new(Vec::new());
static CMD_STREAM_ATLAS_META_SMALL: Mutex<Option<CmdStreamAtlasGlyphMetaTable>> = Mutex::new(None);
static CMD_STREAM_ATLAS_META_LARGE: Mutex<Option<CmdStreamAtlasGlyphMetaTable>> = Mutex::new(None);
static CMD_STREAM_ATLAS_TEX_RECS: Mutex<Vec<CmdStreamAtlasTexRecord>> = Mutex::new(Vec::new());

#[inline]
fn cmd_stream_push_glyph_quad(
    out: &mut Vec<u8>,
    w: f32,
    h: f32,
    x0: f32,
    y0: f32,
    glyph_w_px: f32,
    glyph_h_px: f32,
    slant_px: f32,
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) {
    let x1 = x0 + glyph_w_px;
    let y1 = y0 + glyph_h_px;

    let top_x0 = x0 + slant_px;
    let top_x1 = x1 + slant_px;
    let bottom_x0 = x0;
    let bottom_x1 = x1;

    let nx0_top = 2.0 * (top_x0 / w);
    let ny0 = -(2.0 * (y0 / h));
    let nx1_top = 2.0 * (top_x1 / w);
    let ny1 = -(2.0 * (y1 / h));
    let nx0_bottom = 2.0 * (bottom_x0 / w);
    let nx1_bottom = 2.0 * (bottom_x1 / w);

    super::cmd_stream_push_tex_vtx(out, nx0_bottom, ny1, u0, v1, r, g, b, a);
    super::cmd_stream_push_tex_vtx(out, nx1_bottom, ny1, u1, v1, r, g, b, a);
    super::cmd_stream_push_tex_vtx(out, nx1_top, ny0, u1, v0, r, g, b, a);
    super::cmd_stream_push_tex_vtx(out, nx0_bottom, ny1, u0, v1, r, g, b, a);
    super::cmd_stream_push_tex_vtx(out, nx1_top, ny0, u1, v0, r, g, b, a);
    super::cmd_stream_push_tex_vtx(out, nx0_top, ny0, u0, v0, r, g, b, a);
}

#[inline]
fn cmd_stream_emit_glyph_quads(
    out: &mut Vec<u8>,
    w: f32,
    h: f32,
    x0: f32,
    y0: f32,
    glyph_w_px: f32,
    glyph_h_px: f32,
    slant_px: f32,
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
    bold_mode: u32,
) {
    if bold_mode == 0 {
        cmd_stream_push_glyph_quad(
            out, w, h, x0, y0, glyph_w_px, glyph_h_px, slant_px, u0, v0, u1, v1, r, g, b, a,
        );
        return;
    }

    const BOLD_OFFSETS: [(f32, f32); 3] = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)];
    for (dx, dy) in BOLD_OFFSETS {
        cmd_stream_push_glyph_quad(
            out,
            w,
            h,
            x0 + dx,
            y0 + dy,
            glyph_w_px,
            glyph_h_px,
            slant_px,
            u0,
            v0,
            u1,
            v1,
            r,
            g,
            b,
            a,
        );
    }
}

#[inline]
fn cmd_stream_find_atlas_tex(kind: u32) -> Option<u32> {
    CMD_STREAM_ATLAS_TEX_RECS
        .lock()
        .iter()
        .find(|rec| rec.kind == kind)
        .map(|rec| rec.tex_id)
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
        rgba.push(a);
        rgba.push(0);
        rgba.push(0);
        rgba.push(255);
    }
    let rc = unsafe {
        super::trueos_cabi_gfx_upload_texture_rgba(
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
pub(super) fn clear_text_batches() {
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
pub(super) fn enqueue_text_batch(tex_id: u32, verts: &[u8], origin_x_ndc: f32, origin_y_ndc: f32) {
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
pub(super) fn flush_text_batches() {
    let mut runs = CMD_STREAM_TEXT_BATCH_RUNS.lock();
    for run in runs.iter() {
        if run.tex_id == 0 || run.verts.is_empty() {
            continue;
        }
        let _ = unsafe {
            super::trueos_cabi_gfx_draw_tex_triangles_no_present(
                run.tex_id,
                run.verts.as_ptr(),
                run.verts.len(),
            )
        };
    }
    runs.clear();
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
    italic_tilt_deg: f64,
    bold_mode: u32,
) -> bool {
    if tex_id == 0 || text.is_empty() {
        return false;
    }

    cmd_stream_refresh_atlas_tex_if_needed(tex_id, kind);
    let Some(atlas) = cmd_stream_select_atlas(kind) else {
        return false;
    };

    let view_w = super::CMD_STREAM_VIEW_W.load(Ordering::Relaxed).max(1);
    let view_h = super::CMD_STREAM_VIEW_H.load(Ordering::Relaxed).max(1);
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
    let italic_tilt_bits = (italic_tilt_deg as f32).to_bits();
    let (origin_x_px, origin_y_px) = super::cmd_stream_origin_px();
    let origin_x_ndc = (2.0 * (((x_f as f32) + origin_x_px) / w)) - 1.0;
    let origin_y_ndc = 1.0 - (2.0 * (((y_f as f32) + origin_y_px) / h));
    let tilt_rad = ((italic_tilt_deg as f32).clamp(-30.0, 30.0)) * (core::f32::consts::PI / 180.0);
    let tilt_shear = tanf(tilt_rad);

    let verts = cmd_stream_text_cache_get(
        kind,
        view_w,
        view_h,
        px_h_bits,
        rgb,
        a,
        italic_tilt_bits,
        bold_mode,
        text,
    )
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
            if let Some(table) = meta_table
                && let Some(gm) = cmd_stream_atlas_meta_lookup(table, ch)
            {
                if ch == b' ' {
                    pen_x += gm.advance_px * scale;
                    continue;
                }
                let x0 = pen_x;
                let y0 = pen_y;
                let glyph_h_px = (atlas_cell_h_u as f32).max(1.0) * scale;
                let slant_px = tilt_shear * glyph_h_px;
                cmd_stream_emit_glyph_quads(
                    &mut out,
                    w,
                    h,
                    x0,
                    y0,
                    gm.glyph_w_px * scale,
                    glyph_h_px,
                    slant_px,
                    gm.u0,
                    gm.v0,
                    gm.u1,
                    gm.v1,
                    r,
                    g,
                    b,
                    a,
                    bold_mode,
                );
                pen_x += gm.advance_px * scale;
                continue;
            }

            let mut slot = atlas.index.get(ch as usize).copied().unwrap_or(fallback);
            if slot == u16::MAX {
                slot = fallback;
            }
            if ch == b' ' {
                let glyph_adv_u = atlas
                    .widths
                    .get(slot as usize)
                    .copied()
                    .unwrap_or(atlas.cell_w as u8) as usize;
                pen_x += glyph_adv_u as f32 * scale;
                continue;
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
            let glyph_h_px = (glyph_h_u as f32) * scale;
            let slant_px = tilt_shear * glyph_h_px;
            cmd_stream_emit_glyph_quads(
                &mut out,
                w,
                h,
                x0,
                y0,
                (glyph_w_u as f32) * scale,
                glyph_h_px,
                slant_px,
                u0,
                v0,
                u1,
                v1,
                r,
                g,
                b,
                a,
                bold_mode,
            );
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
            italic_tilt_bits,
            bold_mode,
            text: text.to_vec(),
            verts: cached.clone(),
        });
        cached
    });

    if verts.is_empty() {
        return false;
    }
    enqueue_text_batch(tex_id, verts.as_ref(), origin_x_ndc, origin_y_ndc);
    true
}

#[inline]
fn cmd_stream_text_cache_get(
    kind: u32,
    view_w: u32,
    view_h: u32,
    px_h_bits: u32,
    rgb: u32,
    alpha: u8,
    italic_tilt_bits: u32,
    bold_mode: u32,
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
            && e.italic_tilt_bits == italic_tilt_bits
            && e.bold_mode == bold_mode
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

pub(super) fn release_tex_id(id: u32) {
    let mut atlas = CMD_STREAM_ATLAS_TEX_RECS.lock();
    if let Some(pos) = atlas.iter().position(|r| r.tex_id == id) {
        atlas.swap_remove(pos);
    }
    let mut runs = CMD_STREAM_TEXT_BATCH_RUNS.lock();
    runs.retain(|run| run.tex_id != id);
}

pub(super) unsafe extern "C" fn qjs_cmd_stream_create_atlas_texture(
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
    if let Some(existing_tex_id) = cmd_stream_find_atlas_tex(kind) {
        return qjs::JS_NewFloat64(ctx, existing_tex_id as f64);
    }
    let tex_id = super::cmd_stream_alloc_tex_id();
    if !cmd_stream_upload_atlas_to_tex(tex_id, atlas) {
        super::cmd_stream_release_tex_id(tex_id);
        return qjs::JSValue::undefined();
    }
    cmd_stream_mark_atlas_tex(tex_id, kind);
    qjs::JS_NewFloat64(ctx, tex_id as f64)
}

pub(super) unsafe extern "C" fn qjs_cmd_stream_draw_atlas_text(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    if !super::CMD_STREAM_FRAME_OPEN.load(core::sync::atomic::Ordering::Relaxed) {
        return qjs::JSValue::undefined();
    }
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
    let mut italic_tilt_deg: f64 = 0.0;
    if argc >= 9 {
        let _ = qjs::JS_ToFloat64(ctx, &mut italic_tilt_deg as *mut f64, args[8]);
    }
    let mut bold_mode_f: f64 = 0.0;
    if argc >= 10 {
        let _ = qjs::JS_ToFloat64(ctx, &mut bold_mode_f as *mut f64, args[9]);
    }
    let bold_mode = if bold_mode_f != 0.0 { 1 } else { 0 };

    let Some(_atlas) = cmd_stream_select_atlas(kind) else {
        qjs::JS_FreeCString(ctx, text_c);
        return qjs::JSValue::undefined();
    };
    let text = core::slice::from_raw_parts(text_c as *const u8, text_len);
    let ok = cmd_stream_draw_atlas_text_impl(
        tex_id,
        kind,
        x_f,
        y_f,
        text,
        px_h,
        rgb_f,
        alpha_f,
        italic_tilt_deg,
        bold_mode,
    );
    qjs::JS_FreeCString(ctx, text_c);
    if !ok {
        return qjs::JSValue::undefined();
    }
    qjs::JSValue::undefined()
}
