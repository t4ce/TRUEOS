extern crate alloc;

use alloc::vec::Vec;

use crate::webgl_core::{
    WebGlDecodedVertex, WebGlDrawElementsCache, WebGlDrawElementsCacheKey, WebGlState,
    WebGlVertexAttrib,
};

pub(crate) struct DrawBuild {
    pub(crate) out: Vec<u8>,
    pub(crate) matrix_path: bool,
    pub(crate) count: usize,
}

fn ascii_lower(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for &b in bytes {
        out.push(if b'A' <= b && b <= b'Z' { b + 32 } else { b });
    }
    out
}

fn classify_attrib_name(name: &[u8]) -> Option<u32> {
    let lower = ascii_lower(name);
    if lower.windows(b"position".len()).any(|w| w == b"position") {
        Some(0)
    } else if lower.windows(b"color".len()).any(|w| w == b"color") {
        Some(1)
    } else if lower.windows(b"texcoord".len()).any(|w| w == b"texcoord")
        || lower.windows(b"texturecoord".len()).any(|w| w == b"texturecoord")
        || lower.windows(b"uv".len()).any(|w| w == b"uv")
    {
        Some(2)
    } else {
        None
    }
}

fn mat3_mul_vec3(m: &[f32; 9], x: f32, y: f32, z: f32) -> (f32, f32, f32) {
    let rx = m[0] * x + m[3] * y + m[6] * z;
    let ry = m[1] * x + m[4] * y + m[7] * z;
    let rz = m[2] * x + m[5] * y + m[8] * z;
    (rx, ry, rz)
}

fn ty_size_bytes(ty: u32) -> Option<usize> {
    match ty {
        0x1400 => Some(1), // BYTE
        0x1406 => Some(4), // FLOAT
        0x1401 => Some(1), // UNSIGNED_BYTE
        0x1402 => Some(2), // SHORT
        0x1403 => Some(2), // UNSIGNED_SHORT
        0x1405 => Some(4), // UNSIGNED_INT
        _ => None,
    }
}

fn read_u16_le(bytes: &[u8], off: usize) -> Option<u16> {
    let b0 = *bytes.get(off)?;
    let b1 = *bytes.get(off + 1)?;
    Some(u16::from_le_bytes([b0, b1]))
}

fn read_f32_le(bytes: &[u8], off: usize) -> Option<f32> {
    let b0 = *bytes.get(off)?;
    let b1 = *bytes.get(off + 1)?;
    let b2 = *bytes.get(off + 2)?;
    let b3 = *bytes.get(off + 3)?;
    Some(f32::from_le_bytes([b0, b1, b2, b3]))
}

fn read_i16_le(bytes: &[u8], off: usize) -> Option<i16> {
    let b0 = *bytes.get(off)?;
    let b1 = *bytes.get(off + 1)?;
    Some(i16::from_le_bytes([b0, b1]))
}

fn read_u32_le(bytes: &[u8], off: usize) -> Option<u32> {
    let b0 = *bytes.get(off)?;
    let b1 = *bytes.get(off + 1)?;
    let b2 = *bytes.get(off + 2)?;
    let b3 = *bytes.get(off + 3)?;
    Some(u32::from_le_bytes([b0, b1, b2, b3]))
}

fn read_attrib_component(bytes: &[u8], off: usize, ty: u32, normalized: bool) -> Option<f32> {
    match ty {
        0x1406 => read_f32_le(bytes, off), // FLOAT
        0x1401 => {
            let v = *bytes.get(off)? as f32; // UNSIGNED_BYTE
            Some(if normalized { v / 255.0 } else { v })
        }
        0x1400 => {
            let v = *bytes.get(off)? as i8 as f32; // BYTE
            Some(if normalized { (v / 127.0).max(-1.0) } else { v })
        }
        0x1403 => {
            let v = read_u16_le(bytes, off)? as f32; // UNSIGNED_SHORT
            Some(if normalized { v / 65535.0 } else { v })
        }
        0x1402 => {
            let v = read_i16_le(bytes, off)? as f32; // SHORT
            Some(if normalized { (v / 32767.0).max(-1.0) } else { v })
        }
        0x1405 => Some(read_u32_le(bytes, off)? as f32), // UNSIGNED_INT
        _ => None,
    }
}

fn clamp01(v: f32) -> f32 {
    if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}

fn attrib_color_component_to_u8(v: f32, ty: u32, normalized: bool) -> u8 {
    let x = if normalized || ty == 0x1406 {
        clamp01(v) * 255.0
    } else if v < 0.0 {
        0.0
    } else if v > 255.0 {
        255.0
    } else {
        v
    };
    (x + 0.5) as u8
}

fn blend_factor_rgb(factor: u32, src: [f32; 3], src_a: f32, dst: [f32; 3], dst_a: f32) -> [f32; 3] {
    match factor {
        0 => [0.0, 0.0, 0.0],                            // ZERO
        1 => [1.0, 1.0, 1.0],                            // ONE
        0x0302 => [src_a, src_a, src_a],                 // SRC_ALPHA
        0x0303 => [1.0 - src_a, 1.0 - src_a, 1.0 - src_a], // ONE_MINUS_SRC_ALPHA
        0x0304 => [dst_a, dst_a, dst_a],                 // DST_ALPHA
        0x0305 => [1.0 - dst_a, 1.0 - dst_a, 1.0 - dst_a], // ONE_MINUS_DST_ALPHA
        0x0300 => src,                                   // SRC_COLOR
        0x0301 => [1.0 - src[0], 1.0 - src[1], 1.0 - src[2]], // ONE_MINUS_SRC_COLOR
        0x0306 => dst,                                   // DST_COLOR
        0x0307 => [1.0 - dst[0], 1.0 - dst[1], 1.0 - dst[2]], // ONE_MINUS_DST_COLOR
        _ => [1.0, 1.0, 1.0],
    }
}

fn apply_blend_equation(eq: u32, s: f32, d: f32) -> f32 {
    match eq {
        0x800A => s - d, // FUNC_SUBTRACT
        0x800B => d - s, // FUNC_REVERSE_SUBTRACT
        _ => s + d,      // FUNC_ADD
    }
}

fn apply_simple_blend_rgb(
    enabled: bool,
    src_rgb_u8: [u8; 3],
    src_a_u8: u8,
    clear_rgb: u32,
    src_factor: u32,
    dst_factor: u32,
    eq: u32,
) -> [u8; 3] {
    if !enabled {
        return src_rgb_u8;
    }
    let src = [
        (src_rgb_u8[0] as f32) / 255.0,
        (src_rgb_u8[1] as f32) / 255.0,
        (src_rgb_u8[2] as f32) / 255.0,
    ];
    let src_a = (src_a_u8 as f32) / 255.0;
    let dst = [
        ((clear_rgb >> 16) as u8 as f32) / 255.0,
        (((clear_rgb >> 8) & 0xFF) as u8 as f32) / 255.0,
        ((clear_rgb & 0xFF) as u8 as f32) / 255.0,
    ];
    let dst_a = 1.0;
    let sf = blend_factor_rgb(src_factor, src, src_a, dst, dst_a);
    let df = blend_factor_rgb(dst_factor, src, src_a, dst, dst_a);
    let mut out = [0u8; 3];
    for i in 0..3 {
        let s = src[i] * sf[i];
        let d = dst[i] * df[i];
        let v = clamp01(apply_blend_equation(eq, s, d));
        out[i] = (v * 255.0 + 0.5) as u8;
    }
    out
}

fn find_pos_col_attribs(
    st: &WebGlState,
    element_array_buffer: Option<u32>,
) -> Option<(WebGlVertexAttrib, Option<WebGlVertexAttrib>)> {
    let mut pos_attr: Option<(u32, WebGlVertexAttrib)> = None;
    let mut col_attr: Option<(u32, WebGlVertexAttrib)> = None;
    for (idx, a) in st.attribs.iter() {
        let Some(name) = st.attrib_loc_to_name.get(idx) else {
            continue;
        };
        match classify_attrib_name(name.as_slice()) {
            Some(0) => {
                if pos_attr.is_none() && a.size >= 2 && ty_size_bytes(a.ty).is_some() {
                    pos_attr = Some((*idx, *a));
                }
            }
            Some(1) => {
                if col_attr.is_none() && a.size >= 3 && ty_size_bytes(a.ty).is_some() {
                    col_attr = Some((*idx, *a));
                }
            }
            _ => {}
        }
    }
    for (idx, a) in st.attribs.iter() {
        if !a.enabled {
            continue;
        }
        if pos_attr.is_none() && a.size >= 2 && a.ty == 0x1406 {
            pos_attr = Some((*idx, *a));
        }
        if col_attr.is_none() && a.size == 4 && a.ty == 0x1401 {
            col_attr = Some((*idx, *a));
        }
    }
    if pos_attr.is_none() {
        for (idx, a) in st.attribs.iter() {
            if a.enabled && a.size >= 2 && ty_size_bytes(a.ty).is_some() {
                pos_attr = Some((*idx, *a));
                break;
            }
        }
    }
    if pos_attr.is_none() {
        for (idx, a) in st.attribs.iter() {
            if a.size >= 2 && a.ty == 0x1406 {
                pos_attr = Some((*idx, *a));
                break;
            }
        }
    }
    if pos_attr.is_none() {
        for (idx, a) in st.attribs.iter() {
            if a.size >= 2 && ty_size_bytes(a.ty).is_some() {
                pos_attr = Some((*idx, *a));
                break;
            }
        }
    }
    if pos_attr.is_none() {
        for (buf_id, bytes) in st.buffers.iter() {
            if bytes.len() < 8 {
                continue;
            }
            if let Some(elem_buf) = element_array_buffer {
                if *buf_id == elem_buf {
                    continue;
                }
            }
            pos_attr = Some((
                0,
                WebGlVertexAttrib {
                    enabled: true,
                    size: 2,
                    ty: 0x1406,
                    normalized: false,
                    stride: 0,
                    offset: 0,
                    buffer: *buf_id,
                },
            ));
            break;
        }
    }
    let Some((_, pos)) = pos_attr else {
        return None;
    };
    Some((pos, col_attr.map(|(_, c)| c)))
}

fn emit_from_decoded(st: &WebGlState, decoded: &[WebGlDecodedVertex]) -> Vec<u8> {
    const VTX_SIZE: usize = 12;
    let viewport_w = st.viewport_w.max(1) as f32;
    let viewport_h = st.viewport_h.max(1) as f32;
    let matrix_path = st.has_translation_matrix && st.has_projection_matrix;
    let mut out = Vec::with_capacity(decoded.len().saturating_mul(VTX_SIZE));
    for dv in decoded.iter() {
        let (x, y) = if matrix_path {
            let (tx, ty, tz) = mat3_mul_vec3(&st.translation_matrix, dv.x_px, dv.y_px, 1.0);
            let (cx, cy, _cz) = mat3_mul_vec3(&st.projection_matrix, tx, ty, tz);
            (cx, cy)
        } else {
            let x = (2.0 * (dv.x_px / viewport_w)) - 1.0;
            let y = 1.0 - (2.0 * (dv.y_px / viewport_h));
            (x, y)
        };
        let rgb = apply_simple_blend_rgb(
            st.enabled_blend,
            [dv.r, dv.g, dv.b],
            dv.a,
            st.clear_rgb,
            st.blend_src_rgb,
            st.blend_dst_rgb,
            st.blend_eq_rgb,
        );
        out.extend_from_slice(&x.to_le_bytes());
        out.extend_from_slice(&y.to_le_bytes());
        out.push(rgb[0]);
        out.push(rgb[1]);
        out.push(rgb[2]);
        out.push(0);
    }
    out
}

pub(crate) fn build_draw_elements(
    st: &mut WebGlState,
    count: usize,
    index_off: usize,
) -> Result<DrawBuild, &'static str> {
    let elem_buf = st.element_array_buffer;
    let Some(elem_bytes) = st.buffers.get(&elem_buf).cloned() else {
        return Err("no-element-array-buffer");
    };
    let Some((pos, col)) = find_pos_col_attribs(st, Some(elem_buf)) else {
        return Err("no-pos-attrib");
    };

    let pos_ty_sz = ty_size_bytes(pos.ty).ok_or("bad-pos-type")?;
    let pos_stride = if pos.stride == 0 {
        (pos.size as usize).saturating_mul(pos_ty_sz)
    } else {
        pos.stride as usize
    };
    if pos_stride == 0 {
        return Err("pos-stride==0");
    }
    let Some(pos_bytes) = st.buffers.get(&pos.buffer).cloned() else {
        return Err("pos-buffer-missing");
    };

    let (col_bytes_opt, col_stride, col_ty_sz) = if let Some(c) = col {
        let col_ty_sz = ty_size_bytes(c.ty).unwrap_or(1);
        let col_stride = if c.stride == 0 {
            (c.size as usize).saturating_mul(col_ty_sz)
        } else {
            c.stride as usize
        };
        (st.buffers.get(&c.buffer).cloned(), col_stride, col_ty_sz)
    } else {
        (None, 0, 1)
    };

    let cache_key = WebGlDrawElementsCacheKey {
        count,
        index_off,
        element_array_buffer: elem_buf,
        element_array_version: st.buffer_versions.get(&elem_buf).copied().unwrap_or(0),
        pos,
        pos_buffer_version: st.buffer_versions.get(&pos.buffer).copied().unwrap_or(0),
        col,
        col_buffer_version: col
            .map(|c| st.buffer_versions.get(&c.buffer).copied().unwrap_or(0))
            .unwrap_or(0),
    };

    let mut decoded = st
        .draw_elements_cache
        .as_ref()
        .and_then(|cache| (cache.key == cache_key).then_some(cache.verts.clone()))
        .unwrap_or_default();
    if decoded.is_empty() {
        decoded = Vec::with_capacity(count);
        for i in 0..count {
            let idx_off = index_off.saturating_add(i.saturating_mul(2));
            let vtx_idx = read_u16_le(&elem_bytes, idx_off)
                .map(|v| v as usize)
                .unwrap_or(i);

            let base = vtx_idx.saturating_mul(pos_stride).saturating_add(pos.offset);
            let Some(x_px) = read_attrib_component(&pos_bytes, base, pos.ty, pos.normalized) else {
                continue;
            };
            let Some(y_px) = read_attrib_component(
                &pos_bytes,
                base.saturating_add(pos_ty_sz),
                pos.ty,
                pos.normalized,
            ) else {
                continue;
            };

            let (r, g, b, a) = if let (Some(c), Some(col_bytes)) = (col, col_bytes_opt.as_ref()) {
                let base = vtx_idx.saturating_mul(col_stride).saturating_add(c.offset);
                let r = read_attrib_component(col_bytes, base, c.ty, c.normalized)
                    .map(|v| attrib_color_component_to_u8(v, c.ty, c.normalized))
                    .unwrap_or(255);
                let g = read_attrib_component(
                    col_bytes,
                    base.saturating_add(col_ty_sz),
                    c.ty,
                    c.normalized,
                )
                .map(|v| attrib_color_component_to_u8(v, c.ty, c.normalized))
                .unwrap_or(255);
                let b = read_attrib_component(
                    col_bytes,
                    base.saturating_add(2usize.saturating_mul(col_ty_sz)),
                    c.ty,
                    c.normalized,
                )
                .map(|v| attrib_color_component_to_u8(v, c.ty, c.normalized))
                .unwrap_or(255);
                let a = read_attrib_component(
                    col_bytes,
                    base.saturating_add(3usize.saturating_mul(col_ty_sz)),
                    c.ty,
                    c.normalized,
                )
                .map(|v| attrib_color_component_to_u8(v, c.ty, c.normalized))
                .unwrap_or(255);
                (r, g, b, a)
            } else {
                (255, 255, 255, 255)
            };

            decoded.push(WebGlDecodedVertex {
                x_px,
                y_px,
                r,
                g,
                b,
                a,
            });
        }
        if !decoded.is_empty() {
            st.draw_elements_cache = Some(WebGlDrawElementsCache {
                key: cache_key,
                verts: decoded.clone(),
            });
        }
    }

    let mut out = emit_from_decoded(st, decoded.as_slice());
    if out.is_empty() {
        const VTX_SIZE: usize = 12;
        let viewport_w = st.viewport_w.max(1) as f32;
        let viewport_h = st.viewport_h.max(1) as f32;
        let matrix_path = st.has_translation_matrix && st.has_projection_matrix;
        let try_strides = [8usize, 12, 16, 20, 24, 28, 32];
        for (buf_id, bytes) in st.buffers.iter() {
            if *buf_id == elem_buf || bytes.len() < 8 {
                continue;
            }
            let mut recovered = Vec::new();
            for stride in try_strides.iter() {
                let mut tmp = Vec::with_capacity(count.saturating_mul(VTX_SIZE));
                for i in 0..count {
                    let base = i.saturating_mul(*stride);
                    let Some(x_px) = read_f32_le(bytes, base) else {
                        break;
                    };
                    let Some(y_px) = read_f32_le(bytes, base.saturating_add(4)) else {
                        break;
                    };
                    let (x, y) = if matrix_path {
                        let (tx, ty, tz) = mat3_mul_vec3(&st.translation_matrix, x_px, y_px, 1.0);
                        let (cx, cy, _cz) = mat3_mul_vec3(&st.projection_matrix, tx, ty, tz);
                        (cx, cy)
                    } else {
                        let x = (2.0 * (x_px / viewport_w)) - 1.0;
                        let y = 1.0 - (2.0 * (y_px / viewport_h));
                        (x, y)
                    };
                    tmp.extend_from_slice(&x.to_le_bytes());
                    tmp.extend_from_slice(&y.to_le_bytes());
                    tmp.push(255);
                    tmp.push(255);
                    tmp.push(255);
                    tmp.push(0);
                }
                if !tmp.is_empty() {
                    recovered = tmp;
                    break;
                }
            }
            if !recovered.is_empty() {
                out = recovered;
                break;
            }
        }
    }
    if out.is_empty() {
        return Err("out-empty");
    }
    Ok(DrawBuild {
        out,
        matrix_path: st.has_translation_matrix && st.has_projection_matrix,
        count,
    })
}

pub(crate) fn build_draw_arrays(
    st: &WebGlState,
    first: usize,
    count: usize,
) -> Result<DrawBuild, &'static str> {
    let Some((pos, col)) = find_pos_col_attribs(st, None) else {
        return Err("no-pos-attrib");
    };
    let pos_ty_sz = ty_size_bytes(pos.ty).ok_or("bad-pos-type")?;
    let pos_stride = if pos.stride == 0 {
        (pos.size as usize).saturating_mul(pos_ty_sz)
    } else {
        pos.stride as usize
    };
    if pos_stride == 0 {
        return Err("pos-stride==0");
    }
    let Some(pos_bytes) = st.buffers.get(&pos.buffer) else {
        return Err("pos-buffer-missing");
    };

    let (col_bytes_opt, col_stride, col_ty_sz) = if let Some(c) = col {
        let col_ty_sz = ty_size_bytes(c.ty).unwrap_or(1);
        let col_stride = if c.stride == 0 {
            (c.size as usize).saturating_mul(col_ty_sz)
        } else {
            c.stride as usize
        };
        (st.buffers.get(&c.buffer), col_stride, col_ty_sz)
    } else {
        (None, 0, 1)
    };

    let mut decoded = Vec::with_capacity(count);
    for i in 0..count {
        let vtx_idx = first.saturating_add(i);
        let base = vtx_idx.saturating_mul(pos_stride).saturating_add(pos.offset);
        let Some(x_px) = read_attrib_component(pos_bytes, base, pos.ty, pos.normalized) else {
            continue;
        };
        let Some(y_px) = read_attrib_component(
            pos_bytes,
            base.saturating_add(pos_ty_sz),
            pos.ty,
            pos.normalized,
        ) else {
            continue;
        };

        let (r, g, b, a) = if let (Some(c), Some(col_bytes)) = (col, col_bytes_opt) {
            let base = vtx_idx.saturating_mul(col_stride).saturating_add(c.offset);
            let r = read_attrib_component(col_bytes, base, c.ty, c.normalized)
                .map(|v| attrib_color_component_to_u8(v, c.ty, c.normalized))
                .unwrap_or(255);
            let g = read_attrib_component(
                col_bytes,
                base.saturating_add(col_ty_sz),
                c.ty,
                c.normalized,
            )
            .map(|v| attrib_color_component_to_u8(v, c.ty, c.normalized))
            .unwrap_or(255);
            let b = read_attrib_component(
                col_bytes,
                base.saturating_add(2usize.saturating_mul(col_ty_sz)),
                c.ty,
                c.normalized,
            )
            .map(|v| attrib_color_component_to_u8(v, c.ty, c.normalized))
            .unwrap_or(255);
            let a = read_attrib_component(
                col_bytes,
                base.saturating_add(3usize.saturating_mul(col_ty_sz)),
                c.ty,
                c.normalized,
            )
            .map(|v| attrib_color_component_to_u8(v, c.ty, c.normalized))
            .unwrap_or(255);
            (r, g, b, a)
        } else {
            (255, 255, 255, 255)
        };
        decoded.push(WebGlDecodedVertex {
            x_px,
            y_px,
            r,
            g,
            b,
            a,
        });
    }

    let out = emit_from_decoded(st, decoded.as_slice());
    if out.is_empty() {
        return Err("out-empty");
    }
    Ok(DrawBuild {
        out,
        matrix_path: st.has_translation_matrix && st.has_projection_matrix,
        count,
    })
}
