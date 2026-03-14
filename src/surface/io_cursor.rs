const CURSOR_TICK_SUPPRESS_AFTER_BASE_MS: u64 = 24;

#[inline]
fn push_rgb_quad(
    out: &mut Vec<u8>,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    color: (f32, f32, f32, f32),
) {
    let (r, g, b, a) = color;
    let v0 = RgbVtx {
        x: x0,
        y: y0,
        r,
        g,
        b,
        a,
    };
    let v1 = RgbVtx {
        x: x1,
        y: y0,
        r,
        g,
        b,
        a,
    };
    let v2 = RgbVtx {
        x: x1,
        y: y1,
        r,
        g,
        b,
        a,
    };
    let v3 = RgbVtx {
        x: x0,
        y: y1,
        r,
        g,
        b,
        a,
    };
    push_rgb_vtx(out, v0);
    push_rgb_vtx(out, v1);
    push_rgb_vtx(out, v2);
    push_rgb_vtx(out, v0);
    push_rgb_vtx(out, v2);
    push_rgb_vtx(out, v3);
}

#[inline]
fn append_cursor_cross(
    out: &mut Vec<u8>,
    ndc_x: f32,
    ndc_y: f32,
    vp_w: u32,
    vp_h: u32,
    color: (f32, f32, f32, f32),
) {
    let w = (vp_w as f32).max(1.0);
    let h = (vp_h as f32).max(1.0);
    let half_span_x = (5.0f32 * 2.0) / w;
    let half_span_y = (5.0f32 * 2.0) / h;
    let half_thickness_x = (1.0f32 * 2.0) / w;
    let half_thickness_y = (1.0f32 * 2.0) / h;

    push_rgb_quad(
        out,
        ndc_x - half_span_x,
        ndc_y - half_thickness_y,
        ndc_x + half_span_x,
        ndc_y + half_thickness_y,
        color,
    );
    push_rgb_quad(
        out,
        ndc_x - half_thickness_x,
        ndc_y - half_span_y,
        ndc_x + half_thickness_x,
        ndc_y + half_span_y,
        color,
    );
}

#[inline]
fn collect_real_cursor_norm(out: &mut Vec<(f32, f32)>) {
    out.clear();

    let cursors = crate::v::cursor::ordered_cursor_snapshot();
    for (cx, cy) in cursors {
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
        out.push((nx, ny));
    }
}

const CURSOR_COLORS: [(f32, f32, f32, f32); 4] = [
    (0.0, 1.0, 0.2, 1.0),
    (1.0, 0.9, 0.1, 1.0),
    (0.2, 0.8, 1.0, 1.0),
    (1.0, 0.4, 0.8, 1.0),
];

fn append_kernel_cursor_overlay_rgb(rgb_blob: &mut Vec<u8>, vp_w: u32, vp_h: u32) {
    if vp_w == 0 || vp_h == 0 {
        return;
    }

    let mut real: Vec<(f32, f32)> = Vec::new();
    collect_real_cursor_norm(&mut real);

    for (i, &(nx, ny)) in real.iter().enumerate() {
        let ndc_x = nx * 2.0 - 1.0;
        let ndc_y = 1.0 - ny * 2.0;
        let color = CURSOR_COLORS[i & 3];
        append_cursor_cross(rgb_blob, ndc_x, ndc_y, vp_w, vp_h, color);
    }
}

unsafe fn input_cursor_buttons(cursor_id: u32, out_buttons_down: *mut u32) -> i32 {
    if out_buttons_down.is_null() {
        return -1;
    }
    if cursor_id == 0 {
        return -1;
    }

    let Some(buttons_down) = crate::v::cursor::cursor_buttons(cursor_id) else {
        return 1;
    };
    *out_buttons_down = buttons_down;
    0
}

unsafe fn input_pop_cursor_event(out: *mut crate::usb::hid::TrueosHidCursorEvent) -> i32 {
    if out.is_null() {
        return -1;
    }
    let Some(ev) = crate::usb::hid::pop_cursor_event() else {
        return 0;
    };
    *out = ev;
    1
}

unsafe fn input_read_cursor_events_since(
    read_seq: u64,
    out: *mut crate::usb::hid::TrueosHidCursorEvent,
    out_cap: u32,
    out_next_seq: *mut u64,
    out_dropped: *mut u32,
) -> u32 {
    if out_next_seq.is_null() || out_dropped.is_null() {
        return 0;
    }

    let cap = out_cap as usize;
    if cap == 0 || out.is_null() {
        let mut none: [crate::usb::hid::TrueosHidCursorEvent; 0] = [];
        let (next_seq, dropped, _wrote) =
            crate::usb::hid::read_cursor_events_since(read_seq, &mut none);
        *out_next_seq = next_seq;
        *out_dropped = dropped;
        return 0;
    }

    let out_slice = core::slice::from_raw_parts_mut(out, cap);
    let (next_seq, dropped, wrote) = crate::usb::hid::read_cursor_events_since(read_seq, out_slice);
    *out_next_seq = next_seq;
    *out_dropped = dropped;
    wrote as u32
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

    let (w, h) = crate::gfx::cpu_backbuffer_dimensions().unwrap_or((320, 200));
    let max_x = w.saturating_sub(1) as i32;
    let max_y = h.saturating_sub(1) as i32;
    let clamped_x = x_px.clamp(0, max_x.max(0));
    let clamped_y = y_px.clamp(0, max_y.max(0));
    let w1 = (w.saturating_sub(1)).max(1) as f64;
    let h1 = (h.saturating_sub(1)).max(1) as f64;
    let nx = (clamped_x as f64) / w1;
    let ny = (clamped_y as f64) / h1;
    let wheel_i16 = wheel.clamp(i16::MIN as i32, i16::MAX as i32) as i16;

    crate::usb::hid::inject_virtual_cursor_event(slot_id, nx, ny, buttons_down, wheel_i16, flags);
    0
}

pub(super) fn append_kernel_cursor_overlay_draws(
    draws: &mut Vec<PendingDraw>,
    rgb_blob: &mut Vec<u8>,
    vp_w: u32,
    vp_h: u32,
) {
    let blob_offset = rgb_blob.len();
    append_kernel_cursor_overlay_rgb(rgb_blob, vp_w, vp_h);

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

pub fn kernel_cursor_overlay_tick() -> i32 {
    crate::gfx::init(crate::limine::framebuffer_response());

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

    let Some((vp_w, vp_h)) = crate::gfx::with_context(|ctx| {
        let e = ctx.swapchain_desc().extent;
        (e.width, e.height)
    }) else {
        return -12;
    };

    let mut draws: Vec<PendingDraw> = Vec::new();
    let mut rgb_blob: Vec<u8> = Vec::new();
    append_kernel_cursor_overlay_draws(&mut draws, &mut rgb_blob, vp_w, vp_h);
    if draws.is_empty() || rgb_blob.is_empty() {
        return 0;
    }

    let rc_begin = unsafe { trueos_cabi_gfx_cursor_begin_frame() };
    if rc_begin != 0 {
        return rc_begin;
    }

    let _ = unsafe { trueos_cabi_gfx_set_blend(1, 0x0302, 0x0303, 0x0302, 0x0303, 0, 0) };

    let rc_draw = unsafe {
        trueos_cabi_gfx_cursor_draw_rgb_triangles_no_present(rgb_blob.as_ptr(), rgb_blob.len())
    };
    if rc_draw != 0 {
        let _ = unsafe { trueos_cabi_gfx_cursor_end_frame() };
        return rc_draw;
    }

    unsafe { trueos_cabi_gfx_cursor_end_frame() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_gfx_cursor_begin_frame() -> i32 {
    crate::gfx::init(crate::limine::framebuffer_response());

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
        st.cursor_rgb_blob.extend_from_slice(&bytes[off..off + chunk]);
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
    let (image, sample_kind) = st
        .tex_images
        .as_ref()
        .and_then(|images| images.get(idx))
        .and_then(|e| e.as_ref())
        .map(|e| (e.image, e.sample_kind))
        .unwrap_or((ImageId::invalid(), TexSampleKind::Mask));
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
        st.cursor_tex_blob.extend_from_slice(&bytes[off..off + chunk]);
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
    crate::gfx::init(crate::limine::framebuffer_response());

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

    let Some(ret) = crate::gfx::with_context(|ctx| {
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
            while draw_idx < draws.len() {
                let (kind, add) = match &draws[draw_idx] {
                    PendingDraw::SetRenderTarget { .. } => {
                        if pass_kind == 0 {
                            draw_idx += 1;
                            continue;
                        }
                        break;
                    }
                    PendingDraw::Rgb { blob_len, .. } => (1u8, blob_len - (blob_len % 12)),
                    PendingDraw::Tex { blob_len, .. } => (2u8, blob_len - (blob_len % 20)),
                };
                if add == 0 {
                    draw_idx += 1;
                    continue;
                }
                if pass_kind == 0 {
                    pass_kind = kind;
                } else if kind != pass_kind {
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
                let tex_kind = if plans.iter().all(|plan| {
                    matches!(
                        plan,
                        Plan::Tex {
                            sample_kind: TexSampleKind::Rgba,
                            ..
                        }
                    )
                }) {
                    TexSampleKind::Rgba
                } else {
                    TexSampleKind::Mask
                };
                let (pipeline, vbuf, _) = match ensure_gfx_resources_tex(ctx, tex_blob.len(), tex_kind) {
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
    }) else {
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

    let Some((nx, ny)) = crate::v::cursor::cursor_pos(cursor_id) else {
        return 1;
    };

    let (w, h) = crate::gfx::cpu_backbuffer_dimensions().unwrap_or((320, 200));
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
    out: *mut crate::usb::hid::TrueosHidCursorEvent,
) -> i32 {
    input_pop_cursor_event(out)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_input_read_cursor_events_since(
    read_seq: u64,
    out: *mut crate::usb::hid::TrueosHidCursorEvent,
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
