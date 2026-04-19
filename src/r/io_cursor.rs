const CURSOR_TICK_SUPPRESS_AFTER_BASE_MS: u64 = 60;
const CURSOR_HALF_SPAN_VIEWPORT_RATIO: f32 = 0.0065;
const CURSOR_HALF_SPAN_MIN_PX: f32 = 6.0;
const CURSOR_HALF_SPAN_MAX_PX: f32 = 12.0;
const CURSOR_THICKNESS_RATIO: f32 = 0.22;
const CURSOR_THICKNESS_MIN_PX: f32 = 1.0;
const CURSOR_THICKNESS_MAX_PX: f32 = 3.0;

#[inline]
fn cursor_cross_metrics_px(vp_h: u32) -> (f32, f32) {
    let viewport_h = (vp_h as f32).max(1.0);
    let half_span_px = (viewport_h * CURSOR_HALF_SPAN_VIEWPORT_RATIO)
        .clamp(CURSOR_HALF_SPAN_MIN_PX, CURSOR_HALF_SPAN_MAX_PX);
    let half_thickness_px = (half_span_px * CURSOR_THICKNESS_RATIO)
        .clamp(CURSOR_THICKNESS_MIN_PX, CURSOR_THICKNESS_MAX_PX);
    (half_span_px, half_thickness_px)
}

#[inline]
fn append_cursor_cross(
    out: &mut Vec<u8>,
    ndc_x: f32,
    ndc_y: f32,
    vp_w: u32,
    vp_h: u32,
    color: trueos_gfx_core::Rgba8,
) {
    let w = (vp_w as f32).max(1.0);
    let h = (vp_h as f32).max(1.0);
    let (half_span_px, half_thickness_px) = cursor_cross_metrics_px(vp_h);
    let half_span_x = (half_span_px * 2.0) / w;
    let half_span_y = (half_span_px * 2.0) / h;
    let half_thickness_x = (half_thickness_px * 2.0) / w;
    let half_thickness_y = (half_thickness_px * 2.0) / h;

    trueos_gfx_core::push_rgb_quad_ndc(
        out,
        ndc_x - half_span_x,
        ndc_y + half_thickness_y,
        ndc_x + half_span_x,
        ndc_y - half_thickness_y,
        color,
    );
    trueos_gfx_core::push_rgb_quad_ndc(
        out,
        ndc_x - half_thickness_x,
        ndc_y + half_span_y,
        ndc_x + half_thickness_x,
        ndc_y - half_span_y,
        color,
    );
}

#[derive(Default)]
struct CursorOverlayTexBatch {
    tex_id: u32,
    verts: Vec<u8>,
}

#[inline]
fn cursor_overlay_center_px(nx: f32, ny: f32, vp_w: u32, vp_h: u32) -> (f32, f32) {
    let max_x = vp_w.saturating_sub(1).max(1) as f32;
    let max_y = vp_h.saturating_sub(1).max(1) as f32;
    (nx * max_x, ny * max_y)
}

fn cursor_overlay_tex_batch_index(batches: &mut Vec<CursorOverlayTexBatch>, tex_id: u32) -> usize {
    if let Some(idx) = batches.iter().position(|batch| batch.tex_id == tex_id) {
        return idx;
    }
    batches.push(CursorOverlayTexBatch {
        tex_id,
        verts: Vec::new(),
    });
    batches.len() - 1
}

fn append_cursor_glyph(
    out: &mut Vec<u8>,
    center_x_px: f32,
    center_y_px: f32,
    vp_w: u32,
    vp_h: u32,
    glyph: crate::r::ui2::Ui2CursorOverlayGlyphSpec,
) {
    if glyph.tex_id == 0 || vp_w == 0 || vp_h == 0 {
        return;
    }

    let draw_w = f32::from(glyph.draw_w_px.max(1));
    let draw_h = f32::from(glyph.draw_h_px.max(1));
    let draw_x = center_x_px - (draw_w * 0.5);
    let draw_y = center_y_px - (draw_h * 0.5);
    let atlas_w = f32::from(glyph.atlas_w.max(1));
    let atlas_h = f32::from(glyph.atlas_h.max(1));
    let u0 = f32::from(glyph.src_x) / atlas_w;
    let v0 = f32::from(glyph.src_y) / atlas_h;
    let u1 = f32::from(glyph.src_x.saturating_add(glyph.src_w)) / atlas_w;
    let v1 = f32::from(glyph.src_y.saturating_add(glyph.src_h)) / atlas_h;

    trueos_gfx_core::push_tex_quad_px(
        out,
        trueos_gfx_core::ViewTransform::from_extent(vp_w, vp_h),
        draw_x,
        draw_y,
        draw_x + draw_w,
        draw_y + draw_h,
        [u0, v0, u1, v1],
        trueos_gfx_core::Rgba8::new(255, 255, 255, 255),
    );
}

#[inline]
fn collect_real_cursor_norm(out: &mut Vec<(u32, f32, f32)>, skip_slot_id: Option<u32>) {
    out.clear();

    let cursors = crate::r::cursor::ordered_cursor_snapshot_with_slots();
    for (slot_id, cx, cy) in cursors {
        if skip_slot_id == Some(slot_id) {
            continue;
        }
        let nx = if cx.is_finite() {
            cx.clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let ny = if cy.is_finite() {
            cy.clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        out.push((slot_id, nx, ny));
    }
}

fn append_kernel_cursor_overlay(
    rgb_blob: &mut Vec<u8>,
    tex_batches: &mut Vec<CursorOverlayTexBatch>,
    vp_w: u32,
    vp_h: u32,
    skip_slot_id: Option<u32>,
) {
    if vp_w == 0 || vp_h == 0 {
        return;
    }

    let mut real: Vec<(u32, f32, f32)> = Vec::new();
    collect_real_cursor_norm(&mut real, skip_slot_id);

    for &(slot_id, nx, ny) in real.iter() {
        if let Some(glyph) = crate::r::ui2::cursor_overlay_glyph_spec(slot_id, vp_h) {
            let (center_x_px, center_y_px) = cursor_overlay_center_px(nx, ny, vp_w, vp_h);
            let batch_idx = cursor_overlay_tex_batch_index(tex_batches, glyph.tex_id);
            append_cursor_glyph(
                &mut tex_batches[batch_idx].verts,
                center_x_px,
                center_y_px,
                vp_w,
                vp_h,
                glyph,
            );
            continue;
        }

        let ndc_x = nx * 2.0 - 1.0;
        let ndc_y = 1.0 - ny * 2.0;
        let color = crate::r::ui2::cursor_color_rgba8(slot_id);
        append_cursor_cross(rgb_blob, ndc_x, ndc_y, vp_w, vp_h, color);
    }
}

pub(super) fn append_kernel_cursor_overlay_draws(
    draws: &mut Vec<PendingDraw>,
    rgb_blob: &mut Vec<u8>,
    tex_blob: &mut Vec<u8>,
    vp_w: u32,
    vp_h: u32,
    skip_slot_id: Option<u32>,
) {
    if vp_w == 0 || vp_h == 0 {
        return;
    }

    let mut real: Vec<(u32, f32, f32)> = Vec::new();
    collect_real_cursor_norm(&mut real, skip_slot_id);

    for &(slot_id, nx, ny) in &real {
        if let Some(glyph) = crate::r::ui2::cursor_overlay_glyph_spec(slot_id, vp_h) {
            let idx = glyph.tex_id.saturating_sub(1) as usize;
            let Some((image, sample_kind, origin)) = GFX_CABI_STATE
                .lock()
                .tex_images
                .as_ref()
                .and_then(|images| images.get(idx))
                .and_then(|entry| entry.as_ref())
                .map(|entry| (entry.image, entry.sample_kind, entry.origin))
            else {
                let ndc_x = nx * 2.0 - 1.0;
                let ndc_y = 1.0 - ny * 2.0;
                let color = crate::r::ui2::cursor_color_rgba8(slot_id);
                append_cursor_cross(rgb_blob, ndc_x, ndc_y, vp_w, vp_h, color);
                continue;
            };

            let (center_x_px, center_y_px) = cursor_overlay_center_px(nx, ny, vp_w, vp_h);
            let mut verts = Vec::new();
            append_cursor_glyph(&mut verts, center_x_px, center_y_px, vp_w, vp_h, glyph);
            if verts.is_empty() {
                continue;
            }
            let blob_offset = tex_blob.len();
            append_tex_vertices_with_origin(tex_blob, verts.as_slice(), origin);
            draws.push(PendingDraw::Tex {
                tex_id: glyph.tex_id,
                image,
                sample_kind,
                sampler: SamplerDesc {
                    wrap_s: SamplerWrap::ClampToEdge,
                    wrap_t: SamplerWrap::ClampToEdge,
                    min_filter: SamplerFilter::Linear,
                    mag_filter: SamplerFilter::Linear,
                },
                blob_offset,
                blob_len: verts.len(),
                blend: BlendDesc::straight_alpha(),
            });
            continue;
        }
    }

    let blob_offset = rgb_blob.len();
    for &(slot_id, nx, ny) in &real {
        if crate::r::ui2::cursor_overlay_glyph_spec(slot_id, vp_h).is_some() {
            continue;
        }
        let ndc_x = nx * 2.0 - 1.0;
        let ndc_y = 1.0 - ny * 2.0;
        let color = crate::r::ui2::cursor_color_rgba8(slot_id);
        append_cursor_cross(rgb_blob, ndc_x, ndc_y, vp_w, vp_h, color);
    }

    let blob_len = rgb_blob.len().saturating_sub(blob_offset);
    if blob_len == 0 {
        return;
    }

    draws.push(PendingDraw::Rgb {
        blob_offset,
        blob_len,
        blend: BlendDesc::straight_alpha(),
    });
}

unsafe fn input_cursor_buttons(cursor_id: u32, out_buttons_down: *mut u32) -> i32 {
    if out_buttons_down.is_null() {
        return -1;
    }
    if cursor_id == 0 {
        return -1;
    }

    let Some(buttons_down) = crate::r::cursor::cursor_buttons(cursor_id) else {
        return 1;
    };
    *out_buttons_down = buttons_down;
    0
}

unsafe fn input_pop_cursor_event(out: *mut crate::usb2::hid::TrueosHidCursorEvent) -> i32 {
    if out.is_null() {
        return -1;
    }
    let Some(ev) = crate::usb2::hid::pop_cursor_event() else {
        return 0;
    };
    *out = ev;
    1
}

unsafe fn input_read_cursor_events_since(
    read_seq: u64,
    out: *mut crate::usb2::hid::TrueosHidCursorEvent,
    out_cap: u32,
    out_next_seq: *mut u64,
    out_dropped: *mut u32,
) -> u32 {
    if out_next_seq.is_null() || out_dropped.is_null() {
        return 0;
    }

    let cap = out_cap as usize;
    if cap == 0 || out.is_null() {
        let mut none: [crate::usb2::hid::TrueosHidCursorEvent; 0] = [];
        let (next_seq, dropped, _wrote) =
            crate::usb2::hid::read_cursor_events_since(read_seq, &mut none);
        *out_next_seq = next_seq;
        *out_dropped = dropped;
        return 0;
    }

    let out_slice = core::slice::from_raw_parts_mut(out, cap);
    let (next_seq, dropped, wrote) =
        crate::usb2::hid::read_cursor_events_since(read_seq, out_slice);
    *out_next_seq = next_seq;
    *out_dropped = dropped;
    wrote as u32
}

#[inline]
fn cursor_viewport_dimensions() -> (usize, usize) {
    crate::intel::active_scanout_dimensions()
        .map(|(w, h)| (w as usize, h as usize))
        .or_else(|| {
            crate::limine::framebuffer_response()
                .and_then(|resp| resp.framebuffers().first().copied())
                .map(|fb| (fb.width as usize, fb.height as usize))
        })
        .unwrap_or((320, 200))
}

fn input_write_cursor_event(
    slot_id: u32,
    x_px: i32,
    y_px: i32,
    buttons_down: u32,
    wheel: i32,
    flags: u32,
) -> i32 {
    if slot_id == 0 {
        return -1;
    }

    let (w, h) = cursor_viewport_dimensions();
    let max_x = w.saturating_sub(1) as i32;
    let max_y = h.saturating_sub(1) as i32;
    let clamped_x = x_px.clamp(0, max_x.max(0));
    let clamped_y = y_px.clamp(0, max_y.max(0));
    let w1 = (w.saturating_sub(1)).max(1) as f64;
    let h1 = (h.saturating_sub(1)).max(1) as f64;
    let nx = (clamped_x as f64) / w1;
    let ny = (clamped_y as f64) / h1;
    let wheel_i16 = wheel.clamp(i16::MIN as i32, i16::MAX as i32) as i16;

    crate::usb2::hid::inject_virtual_cursor_event(slot_id, nx, ny, buttons_down, wheel_i16, flags);
    0
}

pub fn kernel_cursor_overlay_tick() -> i32 {
    if end_frame_in_progress() {
        log_cursor_helper_skipped_end_frame_active();
        return 0;
    }

    crate::gfx::init(None);

    let now_ticks = embassy_time_driver::now();
    let suppress_ticks = ((embassy_time_driver::TICK_HZ as u64)
        .saturating_mul(CURSOR_TICK_SUPPRESS_AFTER_BASE_MS)
        .saturating_add(999))
        / 1000;

    let should_tick = {
        let st = GFX_CABI_STATE.lock();
        st.base_cache_valid
            && !st.frame_active
            && !st.cursor_frame_active
            && now_ticks.saturating_sub(st.base_cache_updated_at_ticks) >= suppress_ticks
    };
    if !should_tick {
        return 0;
    }

    let (vp_w, vp_h) = {
        let st = GFX_CABI_STATE.lock();
        (st.base_cache_screen_width, st.base_cache_screen_height)
    };
    if vp_w == 0 || vp_h == 0 {
        return 0;
    }

    let hw_cursor_slot = crate::intel::kernel_hw_cursor_slot();

    let mut rgb_blob: Vec<u8> = Vec::new();
    let mut tex_batches: Vec<CursorOverlayTexBatch> = Vec::new();
    append_kernel_cursor_overlay(&mut rgb_blob, &mut tex_batches, vp_w, vp_h, hw_cursor_slot);
    let have_tex = tex_batches.iter().any(|batch| !batch.verts.is_empty());
    if rgb_blob.is_empty() && !have_tex {
        return 0;
    }

    if end_frame_in_progress() {
        log_cursor_helper_skipped_end_frame_active();
        return 0;
    }

    let rc_begin = unsafe { trueos_cabi_gfx_cursor_begin_frame() };
    if rc_begin != 0 {
        return rc_begin;
    }

    let _ = unsafe { trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0) };

    if !rgb_blob.is_empty() {
        let rc_draw = unsafe {
            trueos_cabi_gfx_cursor_draw_rgb_triangles_no_present(rgb_blob.as_ptr(), rgb_blob.len())
        };
        if rc_draw != 0 {
            let _ = unsafe { trueos_cabi_gfx_cursor_end_frame() };
            return rc_draw;
        }
    }

    if have_tex {
        let _ = unsafe { trueos_cabi_gfx_set_sampler(0, 0, 0, 0) };
        for batch in &tex_batches {
            if batch.tex_id == 0 || batch.verts.is_empty() {
                continue;
            }
            let rc_draw = unsafe {
                trueos_cabi_gfx_cursor_draw_tex_triangles_no_present(
                    batch.tex_id,
                    batch.verts.as_ptr(),
                    batch.verts.len(),
                )
            };
            if rc_draw != 0 {
                let _ = unsafe { trueos_cabi_gfx_cursor_end_frame() };
                return rc_draw;
            }
        }
    }

    if end_frame_in_progress() {
        log_cursor_helper_skipped_end_frame_active();
        let mut st = GFX_CABI_STATE.lock();
        st.cursor_frame_active = false;
        st.cursor_rgb_draws = 0;
        st.cursor_tex_draws = 0;
        st.cursor_draw_bytes = 0;
        st.cursor_draws.clear();
        st.cursor_rgb_blob.clear();
        st.cursor_tex_blob.clear();
        return 0;
    }

    unsafe { trueos_cabi_gfx_cursor_end_frame() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_cursor_begin_frame() -> i32 {
    if end_frame_in_progress() {
        log_cursor_helper_skipped_end_frame_active();
        return 0;
    }

    crate::gfx::init(None);

    let mut st = GFX_CABI_STATE.lock();
    st.cursor_frame_seq = st.cursor_frame_seq.wrapping_add(1);
    st.cursor_frame_active = true;
    st.cursor_rgb_draws = 0;
    st.cursor_tex_draws = 0;
    st.cursor_draw_bytes = 0;
    st.cursor_draws.clear();
    st.cursor_rgb_blob.clear();
    st.cursor_tex_blob.clear();
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_cursor_draw_rgb_triangles_no_present(
    vtx_ptr: *const u8,
    vtx_len: usize,
) -> i32 {
    if vtx_ptr.is_null() {
        return if vtx_len == 0 { 0 } else { -1 };
    }
    if vtx_len == 0 {
        return 0;
    }
    const VTX_SIZE: usize = 12;
    let usable = vtx_len - (vtx_len % VTX_SIZE);
    if usable == 0 {
        return -2;
    }
    let vcount = (usable / VTX_SIZE) as u32;
    if vcount == 0 {
        return 0;
    }
    let bytes = core::slice::from_raw_parts(vtx_ptr, usable);
    let mut st = GFX_CABI_STATE.lock();
    if !st.cursor_frame_active {
        return -3;
    }
    st.cursor_rgb_draws = st.cursor_rgb_draws.saturating_add(1);
    st.cursor_draw_bytes = st.cursor_draw_bytes.saturating_add(usable);
    let blend = st.cur_blend;
    let mut off = 0usize;
    while off < usable {
        let rem = usable - off;
        let chunk = core::cmp::min(MAX_CMDSTREAM_DRAW_BYTES, rem);
        let chunk = chunk - (chunk % VTX_SIZE);
        if chunk == 0 {
            break;
        }
        let blob_offset = st.cursor_rgb_blob.len();
        st.cursor_rgb_blob
            .extend_from_slice(&bytes[off..off + chunk]);
        st.cursor_draws.push(PendingDraw::Rgb {
            blob_offset,
            blob_len: chunk,
            blend,
        });
        off += chunk;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_cursor_draw_tex_triangles_no_present(
    tex_id: u32,
    vtx_ptr: *const u8,
    vtx_len: usize,
) -> i32 {
    if tex_id == 0 {
        return -1;
    }
    if vtx_ptr.is_null() {
        return if vtx_len == 0 { 0 } else { -2 };
    }
    if vtx_len == 0 {
        return 0;
    }
    const VTX_SIZE: usize = 20;
    let usable = vtx_len - (vtx_len % VTX_SIZE);
    if usable == 0 {
        return -3;
    }
    let vcount = (usable / VTX_SIZE) as u32;
    if vcount == 0 {
        return 0;
    }
    let bytes = core::slice::from_raw_parts(vtx_ptr, usable);
    let mut st = GFX_CABI_STATE.lock();
    if !st.cursor_frame_active {
        return -4;
    }
    st.cursor_tex_draws = st.cursor_tex_draws.saturating_add(1);
    st.cursor_draw_bytes = st.cursor_draw_bytes.saturating_add(usable);
    let idx = tex_id.saturating_sub(1) as usize;
    let (image, sample_kind, origin) = st
        .tex_images
        .as_ref()
        .and_then(|images| images.get(idx))
        .and_then(|e| e.as_ref())
        .map(|e| (e.image, e.sample_kind, e.origin))
        .unwrap_or((
            ImageId::invalid(),
            TexSampleKind::Mask,
            TexCoordOrigin::TopLeft,
        ));
    let sampler = st.cur_sampler;
    let blend = st.cur_blend;
    let mut off = 0usize;
    while off < usable {
        let rem = usable - off;
        let chunk = core::cmp::min(MAX_CMDSTREAM_DRAW_BYTES, rem);
        let chunk = chunk - (chunk % VTX_SIZE);
        if chunk == 0 {
            break;
        }
        let blob_offset = st.cursor_tex_blob.len();
        append_tex_vertices_with_origin(&mut st.cursor_tex_blob, &bytes[off..off + chunk], origin);
        st.cursor_draws.push(PendingDraw::Tex {
            tex_id,
            image,
            sample_kind,
            sampler,
            blob_offset,
            blob_len: chunk,
            blend,
        });
        off += chunk;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_cursor_end_frame() -> i32 {
    if end_frame_in_progress() {
        log_cursor_helper_skipped_end_frame_active();
        let mut st = GFX_CABI_STATE.lock();
        st.cursor_frame_active = false;
        st.cursor_rgb_draws = 0;
        st.cursor_tex_draws = 0;
        st.cursor_draw_bytes = 0;
        st.cursor_draws.clear();
        st.cursor_rgb_blob.clear();
        st.cursor_tex_blob.clear();
        return 0;
    }

    crate::gfx::init(None);

    let (
        _seq,
        was_active,
        cursor_draws,
        cursor_rgb_src,
        cursor_tex_src,
        base_cache_valid,
        base_cache_clear_rgb,
        base_cache_draws,
        base_cache_rgb_blob,
        base_cache_tex_blob,
    ) = {
        let mut st = GFX_CABI_STATE.lock();
        let out = (
            st.cursor_frame_seq,
            st.cursor_frame_active,
            core::mem::take(&mut st.cursor_draws),
            core::mem::take(&mut st.cursor_rgb_blob),
            core::mem::take(&mut st.cursor_tex_blob),
            st.base_cache_valid,
            st.base_cache_clear_rgb,
            st.base_cache_draws.clone(),
            st.base_cache_rgb_blob.clone(),
            st.base_cache_tex_blob.clone(),
        );
        st.cursor_frame_active = false;
        out
    };
    if !was_active {
        return -3;
    }
    if !base_cache_valid {
        return -13;
    }

    let cursor_cache_draws = cursor_draws.clone();
    let cursor_cache_rgb_blob = cursor_rgb_src.clone();
    let cursor_cache_tex_blob = cursor_tex_src.clone();

    let mut draws = base_cache_draws;
    let mut rgb_src = base_cache_rgb_blob;
    let mut tex_src = base_cache_tex_blob;
    let rgb_off = rgb_src.len();
    let tex_off = tex_src.len();
    rgb_src.extend_from_slice(cursor_rgb_src.as_slice());
    tex_src.extend_from_slice(cursor_tex_src.as_slice());
    for d in cursor_draws {
        match d {
            PendingDraw::SetRenderTarget { tex_id } => {
                draws.push(PendingDraw::SetRenderTarget { tex_id });
            }
            PendingDraw::SetScissor { rect } => {
                draws.push(PendingDraw::SetScissor { rect });
            }
            PendingDraw::ClearRect {
                rgb,
                x,
                y,
                width,
                height,
            } => draws.push(PendingDraw::ClearRect {
                rgb,
                x,
                y,
                width,
                height,
            }),
            PendingDraw::Rgb {
                blob_offset,
                blob_len,
                blend,
            } => draws.push(PendingDraw::Rgb {
                blob_offset: blob_offset.saturating_add(rgb_off),
                blob_len,
                blend,
            }),
            PendingDraw::Tex {
                tex_id,
                image,
                sample_kind,
                sampler,
                blob_offset,
                blob_len,
                blend,
            } => draws.push(PendingDraw::Tex {
                tex_id,
                image,
                sample_kind,
                sampler,
                blob_offset: blob_offset.saturating_add(tex_off),
                blob_len,
                blend,
            }),
        }
    }

    let Some(ret) =
        crate::gfx::with_context_tag(crate::gfx::SystemLockOwner::CursorEndFrame, |ctx| {
            let (_p, _v, need_set_viewport) = match ensure_gfx_resources(ctx, 0) {
                Some(v) => v,
                None => return -1,
            };
            let swap = ctx.swapchain_desc();
            const MAX_PASS_VERTEX_BYTES: usize = 96 * 1024;

            enum Plan {
                SetRenderTarget {
                    image: Option<ImageId>,
                    vp_w: u32,
                    vp_h: u32,
                },
                SetScissor {
                    rect: Option<ScissorRect>,
                },
                ClearRect {
                    rgb: u32,
                    x: u32,
                    y: u32,
                    width: u32,
                    height: u32,
                },
                Rgb {
                    offset: u64,
                    vcount: u32,
                    blend: BlendDesc,
                },
                Tex {
                    tex_id: u32,
                    image: ImageId,
                    sample_kind: TexSampleKind,
                    sampler: SamplerDesc,
                    offset: u64,
                    vcount: u32,
                    blend: BlendDesc,
                },
            }

            let mut draw_idx = 0usize;
            let mut first_pass = true;
            let mut current_target_image: Option<ImageId> = None;
            let mut current_vp_w = swap.extent.width;
            let mut current_vp_h = swap.extent.height;

            while draw_idx < draws.len() {
                let start = draw_idx;
                let mut pass_bytes = 0usize;
                let mut pass_kind: u8 = 0;
                let mut pass_tex_kind: Option<TexSampleKind> = None;
                while draw_idx < draws.len() {
                    let (kind, add, tex_kind) = match &draws[draw_idx] {
                        PendingDraw::SetRenderTarget { .. } => {
                            if pass_kind == 0 {
                                draw_idx += 1;
                                continue;
                            }
                            break;
                        }
                        PendingDraw::SetScissor { .. } => {
                            if pass_kind == 0 {
                                draw_idx += 1;
                                continue;
                            }
                            break;
                        }
                        PendingDraw::ClearRect { .. } => {
                            if pass_kind == 0 {
                                draw_idx += 1;
                                continue;
                            }
                            break;
                        }
                        PendingDraw::Rgb { blob_len, .. } => {
                            (1u8, blob_len - (blob_len % 12), None)
                        }
                        PendingDraw::Tex {
                            blob_len,
                            sample_kind,
                            ..
                        } => (2u8, blob_len - (blob_len % 20), Some(*sample_kind)),
                    };
                    if add == 0 {
                        draw_idx += 1;
                        continue;
                    }
                    if pass_kind == 0 {
                        pass_kind = kind;
                        pass_tex_kind = tex_kind;
                    } else if kind != pass_kind {
                        break;
                    } else if kind == 2 && tex_kind != pass_tex_kind {
                        break;
                    }
                    if pass_bytes != 0 && pass_bytes.saturating_add(add) > MAX_PASS_VERTEX_BYTES {
                        break;
                    }
                    pass_bytes = pass_bytes.saturating_add(add);
                    draw_idx += 1;
                }

                let mut plans: Vec<Plan> = Vec::new();
                let mut rgb_blob: Vec<u8> = Vec::new();
                let mut tex_blob: Vec<u8> = Vec::new();

                for draw in draws[start..draw_idx].iter() {
                    match draw {
                        PendingDraw::SetRenderTarget { tex_id } => {
                            if *tex_id == 0 {
                                current_target_image = None;
                                current_vp_w = swap.extent.width;
                                current_vp_h = swap.extent.height;
                            } else {
                                let st = GFX_CABI_STATE.lock();
                                let idx = tex_id.saturating_sub(1) as usize;
                                let Some(img) = st
                                    .tex_images
                                    .as_ref()
                                    .and_then(|images| images.get(idx))
                                    .and_then(|entry| entry.as_ref())
                                else {
                                    return -12;
                                };
                                current_target_image = Some(img.image);
                                current_vp_w = img.width.max(1);
                                current_vp_h = img.height.max(1);
                            }
                            plans.push(Plan::SetRenderTarget {
                                image: current_target_image,
                                vp_w: current_vp_w,
                                vp_h: current_vp_h,
                            });
                        }
                        PendingDraw::SetScissor { rect } => {
                            plans.push(Plan::SetScissor { rect: *rect });
                        }
                        PendingDraw::ClearRect {
                            rgb,
                            x,
                            y,
                            width,
                            height,
                        } => {
                            plans.push(Plan::ClearRect {
                                rgb: *rgb,
                                x: *x,
                                y: *y,
                                width: *width,
                                height: *height,
                            });
                        }
                        PendingDraw::Rgb {
                            blob_offset,
                            blob_len,
                            blend,
                        } => {
                            const VTX_SIZE: usize = 12;
                            let usable = blob_len - (blob_len % VTX_SIZE);
                            if usable == 0 {
                                continue;
                            }
                            let start = *blob_offset;
                            let end = start.saturating_add(usable);
                            if end > rgb_src.len() {
                                continue;
                            }
                            let vcount = (usable / VTX_SIZE) as u32;
                            let off = rgb_blob.len() as u64;
                            rgb_blob.extend_from_slice(&rgb_src[start..end]);
                            plans.push(Plan::Rgb {
                                offset: off,
                                vcount,
                                blend: *blend,
                            });
                        }
                        PendingDraw::Tex {
                            tex_id,
                            image,
                            sample_kind,
                            sampler,
                            blob_offset,
                            blob_len,
                            blend,
                        } => {
                            const VTX_SIZE: usize = 20;
                            let usable = blob_len - (blob_len % VTX_SIZE);
                            if usable == 0 {
                                continue;
                            }
                            let start = *blob_offset;
                            let end = start.saturating_add(usable);
                            if end > tex_src.len() {
                                continue;
                            }
                            let vcount = (usable / VTX_SIZE) as u32;
                            let off = tex_blob.len() as u64;
                            tex_blob.extend_from_slice(&tex_src[start..end]);
                            plans.push(Plan::Tex {
                                tex_id: *tex_id,
                                image: *image,
                                sample_kind: *sample_kind,
                                sampler: *sampler,
                                offset: off,
                                vcount,
                                blend: *blend,
                            });
                        }
                    }
                }

                if plans.is_empty() {
                    continue;
                }

                let mut rgb_res: Option<(PipelineId, BufferId)> = None;
                if !rgb_blob.is_empty() {
                    let (pipeline, vbuf, _) = match ensure_gfx_resources(ctx, rgb_blob.len()) {
                        Some(v) => v,
                        None => return -4,
                    };
                    if ctx.write_buffer(vbuf, 0, rgb_blob.as_slice()).is_err() {
                        return -5;
                    }
                    rgb_res = Some((pipeline, vbuf));
                }

                let mut tex_res: Option<(PipelineId, BufferId)> = None;
                if !tex_blob.is_empty() {
                    let tex_kind = if plans.iter().filter_map(|plan| match plan {
                        Plan::Tex { sample_kind, .. } => Some(*sample_kind),
                        _ => None,
                    }).all(|sample_kind| sample_kind == TexSampleKind::Rgba) {
                        TexSampleKind::Rgba
                    } else {
                        TexSampleKind::Mask
                    };
                    let (pipeline, vbuf, _) = match ensure_gfx_resources_tex(
                        ctx,
                        tex_blob.len(),
                        match tex_kind {
                            TexSampleKind::Mask => TexPipelineKind::Mask,
                            TexSampleKind::Rgba => TexPipelineKind::Rgba,
                        },
                    ) {
                        Some(v) => v,
                        None => return -6,
                    };
                    if ctx.write_buffer(vbuf, 0, tex_blob.as_slice()).is_err() {
                        return -7;
                    }
                    tex_res = Some((pipeline, vbuf));
                }

                let is_last_pass = draw_idx >= draws.len();
                let mut cmds: Vec<Command> = Vec::new();
                if first_pass && need_set_viewport {
                    cmds.push(Command::SetViewport(Viewport {
                        x: 0,
                        y: 0,
                        width: current_vp_w as i32,
                        height: current_vp_h as i32,
                    }));
                }
                cmds.push(Command::SetRenderTarget(current_target_image));
                if first_pass {
                    cmds.push(Command::ClearColor {
                        rgb: base_cache_clear_rgb,
                    });
                }

                let mut last_blend: Option<BlendDesc> = None;

                for plan in plans.iter() {
                    match *plan {
                        Plan::SetRenderTarget { image, vp_w, vp_h } => {
                            cmds.push(Command::SetRenderTarget(image));
                            cmds.push(Command::SetViewport(Viewport {
                                x: 0,
                                y: 0,
                                    width: vp_w as i32,
                                    height: vp_h as i32,
                                }));
                        }
                        Plan::SetScissor { rect } => {
                            cmds.push(Command::SetScissor(rect.map(|scissor| GfxScissorRect {
                                x: scissor.x,
                                y: scissor.y,
                                width: scissor.width,
                                height: scissor.height,
                            })));
                        }
                        Plan::ClearRect {
                            rgb,
                            x,
                            y,
                            width,
                            height,
                        } => {
                            cmds.push(Command::ClearRect {
                                rgb,
                                x,
                                y,
                                width,
                                height,
                            });
                        }
                        Plan::Rgb {
                            offset,
                            vcount,
                            blend,
                        } => {
                            if last_blend != Some(blend) {
                                cmds.push(Command::SetBlend(blend));
                                last_blend = Some(blend);
                            }
                            let Some((pipeline, vbuf)) = rgb_res else {
                                return -8;
                            };
                            cmds.push(Command::BindPipeline(pipeline));
                            cmds.push(Command::BindVertexBuffer {
                                buffer: vbuf,
                                offset,
                            });
                            cmds.push(Command::Draw {
                                vertex_count: vcount,
                                first_vertex: 0,
                            });
                        }
                        Plan::Tex {
                            tex_id,
                            image,
                            sample_kind,
                            sampler,
                            offset,
                            vcount,
                            blend,
                        } => {
                            if last_blend != Some(blend) {
                                cmds.push(Command::SetBlend(blend));
                                last_blend = Some(blend);
                            }
                            let Some((pipeline, vbuf)) = tex_res else {
                                return -9;
                            };
                            let image_id = if image.is_valid() {
                                image
                            } else {
                                let mut st = GFX_CABI_STATE.lock();
                                let idx = tex_id.saturating_sub(1) as usize;
                                let desc = ImageDesc {
                                    width: 1,
                                    height: 1,
                                    format: ImageFormat::Rgba8888,
                                };
                                let Ok(img) = ctx.create_image(desc) else {
                                    return -10;
                                };
                                let white = [255u8, 255u8, 255u8, 255u8];
                                let _ = ctx.write_image(img, &white);
                                let images = st.tex_images.get_or_insert_with(Vec::new);
                                if idx >= images.len() {
                                    images.resize_with(idx + 1, || None);
                                }
                                images[idx] = Some(TexImage {
                                    image: img,
                                    width: 1,
                                    height: 1,
                                    sample_kind,
                                    origin: TexCoordOrigin::TopLeft,
                                    rgba: white.to_vec(),
                                });
                                img
                            };
                            cmds.push(Command::BindPipeline(pipeline));
                            cmds.push(Command::SetSampler(sampler));
                            cmds.push(Command::BindImage(image_id));
                            cmds.push(Command::BindVertexBuffer {
                                buffer: vbuf,
                                offset,
                            });
                            cmds.push(Command::Draw {
                                vertex_count: vcount,
                                first_vertex: 0,
                            });
                        }
                    }
                }

                if is_last_pass && current_target_image.is_none() {
                    cmds.push(Command::Present);
                }

                if !check_submit_budget(
                    rgb_blob.len().saturating_add(tex_blob.len()),
                    cmds.len(),
                    "cursor_end_frame_pass",
                ) {
                    return -11;
                }
                let submit_res = ctx.submit(CommandBuffer {
                    commands: cmds.as_slice(),
                });
                if submit_res.is_ok() {
                    let mut st = GFX_CABI_STATE.lock();
                    st.ring_idx = (st.ring_idx + 1) % GFX_CABI_VBUF_RING_LEN;
                } else {
                    return -11;
                }
                first_pass = false;
            }

            if first_pass {
                let mut cmds: Vec<Command> = Vec::new();
                if need_set_viewport {
                    cmds.push(Command::SetViewport(Viewport {
                        x: 0,
                        y: 0,
                        width: current_vp_w as i32,
                        height: current_vp_h as i32,
                    }));
                }
                cmds.push(Command::SetRenderTarget(current_target_image));
                cmds.push(Command::ClearColor {
                    rgb: base_cache_clear_rgb,
                });
                if current_target_image.is_none() {
                    cmds.push(Command::Present);
                }
                if !check_submit_budget(
                    rgb_src.len().saturating_add(tex_src.len()),
                    cmds.len(),
                    "cursor_end_frame_present_only",
                ) {
                    return -11;
                }
                let submit_res = ctx.submit(CommandBuffer {
                    commands: cmds.as_slice(),
                });
                if submit_res.is_ok() {
                    let mut st = GFX_CABI_STATE.lock();
                    st.ring_idx = (st.ring_idx + 1) % GFX_CABI_VBUF_RING_LEN;
                    return 0;
                }
                return -11;
            }

            0
        })
    else {
        return -12;
    };

    if ret == 0 {
        let mut st = GFX_CABI_STATE.lock();
        st.cursor_cache_valid = true;
        st.cursor_cache_draws = cursor_cache_draws;
        st.cursor_cache_rgb_blob = cursor_cache_rgb_blob;
        st.cursor_cache_tex_blob = cursor_cache_tex_blob;
    }

    ret
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_input_cursor_pos(
    cursor_id: u32,
    out_x: *mut i32,
    out_y: *mut i32,
) -> i32 {
    if out_x.is_null() || out_y.is_null() {
        return -1;
    }
    if cursor_id == 0 {
        return -1;
    }

    let Some((nx, ny)) = crate::r::cursor::cursor_pos(cursor_id) else {
        return 1;
    };

    let (w, h) = cursor_viewport_dimensions();
    let w1 = w.saturating_sub(1) as f64;
    let h1 = h.saturating_sub(1) as f64;

    *out_x = libm::round(nx * w1) as i32;
    *out_y = libm::round(ny * h1) as i32;
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_input_cursor_buttons(
    cursor_id: u32,
    out_buttons_down: *mut u32,
) -> i32 {
    input_cursor_buttons(cursor_id, out_buttons_down)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_input_pop_cursor_event(
    out: *mut crate::usb2::hid::TrueosHidCursorEvent,
) -> i32 {
    input_pop_cursor_event(out)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_input_read_cursor_events_since(
    read_seq: u64,
    out: *mut crate::usb2::hid::TrueosHidCursorEvent,
    out_cap: u32,
    out_next_seq: *mut u64,
    out_dropped: *mut u32,
) -> u32 {
    input_read_cursor_events_since(read_seq, out, out_cap, out_next_seq, out_dropped)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_input_write_cursor(
    slot_id: u32,
    x: i32,
    y: i32,
    buttons_down: u32,
    wheel: i32,
    flags: u32,
) -> i32 {
    input_write_cursor_event(slot_id, x, y, buttons_down, wheel, flags)
}
