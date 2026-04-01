use libm::{ceilf, cosf, floorf, sinf};

use super::xelp_copy_ngin;

const XELP_RENDER_FIRST_DEFER_DISPLAY_DISCOVERY: bool = true;
const XELP_RENDER_FIRST_ISOLATE_RGB_TRIANGLE: bool = false;

const TRIANGLE_MIN_DIM: usize = 8;
const TRIANGLE_MAX_W: usize = 240;
const TRIANGLE_MAX_H: usize = 200;
const TRIANGLE_ROTATION_RAD: f32 = 0.62;
const TRIANGLE_ANGLE_STEP_RAD: f32 = 2.0943952;

#[inline]
pub(super) fn defer_display_discovery_for_render_first(intel_igpu770_present: bool) -> bool {
    XELP_RENDER_FIRST_DEFER_DISPLAY_DISCOVERY && intel_igpu770_present
}

#[inline]
pub(super) fn log_display_deferred_for_render_first() {
    crate::log!(
        "intel: display discovery deferred (render-first mode; run display-engine probe later)\n"
    );
}

#[inline]
pub(super) fn log_display_render_first_complete() {
    crate::log!(
        "intel: display discovery render-first prerequisite complete; continuing with display-engine probe\n"
    );
}

#[inline]
pub(super) fn isolate_rgb_triangle_proof() -> bool {
    XELP_RENDER_FIRST_ISOLATE_RGB_TRIANGLE
}

#[inline]
pub(super) fn log_rgb_triangle_isolation() {
    crate::log!(
        "intel/render-ngin: isolation mode active; skipping later framebuffer writers after RCS proof\n"
    );
}

pub(crate) mod xelp_3dstate {
    pub const OPCODE_GROUP_0: u8 = 0x0;
    pub const OPCODE_GROUP_1: u8 = 0x1;

    // Encodes only the documented opcode fields at bits 26:16.
    #[inline]
    pub const fn opcode_key(opcode_group: u8, sub_opcode: u8) -> u32 {
        (((opcode_group as u32) & 0x7) << 24) | ((sub_opcode as u32) << 16)
    }

    pub const DEPTH_STENCIL_STATE_POINTERS: u32 = opcode_key(OPCODE_GROUP_0, 0x25);
    pub const BINDING_TABLE_POINTERS_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x26);
    pub const BINDING_TABLE_POINTERS_HS: u32 = opcode_key(OPCODE_GROUP_0, 0x27);
    pub const BINDING_TABLE_POINTERS_DS: u32 = opcode_key(OPCODE_GROUP_0, 0x28);
    pub const BINDING_TABLE_POINTERS_GS: u32 = opcode_key(OPCODE_GROUP_0, 0x29);
    pub const BINDING_TABLE_POINTERS_PS: u32 = opcode_key(OPCODE_GROUP_0, 0x2A);
    pub const SAMPLER_STATE_POINTERS_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x2B);
    pub const SAMPLER_STATE_POINTERS_HS: u32 = opcode_key(OPCODE_GROUP_0, 0x2C);
    pub const SAMPLER_STATE_POINTERS_DS: u32 = opcode_key(OPCODE_GROUP_0, 0x2D);
    pub const SAMPLER_STATE_POINTERS_GS: u32 = opcode_key(OPCODE_GROUP_0, 0x2E);
    pub const SAMPLER_STATE_POINTERS_PS: u32 = opcode_key(OPCODE_GROUP_0, 0x2F);
    pub const URB_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x30);
    pub const URB_HS: u32 = opcode_key(OPCODE_GROUP_0, 0x31);
    pub const URB_DS: u32 = opcode_key(OPCODE_GROUP_0, 0x32);
    pub const URB_GS: u32 = opcode_key(OPCODE_GROUP_0, 0x33);
    pub const GATHER_CONSTANT_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x34);
    pub const GATHER_CONSTANT_GS: u32 = opcode_key(OPCODE_GROUP_0, 0x35);
    pub const GATHER_CONSTANT_HS: u32 = opcode_key(OPCODE_GROUP_0, 0x36);
    pub const GATHER_CONSTANT_DS: u32 = opcode_key(OPCODE_GROUP_0, 0x37);
    pub const GATHER_CONSTANT_PS: u32 = opcode_key(OPCODE_GROUP_0, 0x38);
    pub const DX9_CONSTANTF_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x39);
    pub const DX9_CONSTANTF_PS: u32 = opcode_key(OPCODE_GROUP_0, 0x3A);
    pub const DX9_CONSTANTI_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x3B);
    pub const DX9_CONSTANTI_PS: u32 = opcode_key(OPCODE_GROUP_0, 0x3C);
    pub const DX9_CONSTANTB_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x3D);
    pub const DX9_CONSTANTB_PS: u32 = opcode_key(OPCODE_GROUP_0, 0x3E);
    pub const DX9_LOCAL_VALID_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x3F);
    pub const DX9_LOCAL_VALID_PS: u32 = opcode_key(OPCODE_GROUP_0, 0x40);
    pub const DX9_GENERATE_ACTIVE_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x41);
    pub const DX9_GENERATE_ACTIVE_PS: u32 = opcode_key(OPCODE_GROUP_0, 0x42);
    pub const BINDING_TABLE_EDIT_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x43);
    pub const BINDING_TABLE_EDIT_GS: u32 = opcode_key(OPCODE_GROUP_0, 0x44);
    pub const BINDING_TABLE_EDIT_HS: u32 = opcode_key(OPCODE_GROUP_0, 0x45);
    pub const BINDING_TABLE_EDIT_DS: u32 = opcode_key(OPCODE_GROUP_0, 0x46);
    pub const BINDING_TABLE_EDIT_PS: u32 = opcode_key(OPCODE_GROUP_0, 0x47);
    pub const VF_HASHING: u32 = opcode_key(OPCODE_GROUP_0, 0x48);
    pub const VF_INSTANCING: u32 = opcode_key(OPCODE_GROUP_0, 0x49);
    pub const VF_SGVS: u32 = opcode_key(OPCODE_GROUP_0, 0x4A);
    pub const VF_TOPOLOGY: u32 = opcode_key(OPCODE_GROUP_0, 0x4B);
    pub const WM_CHROMA_KEY: u32 = opcode_key(OPCODE_GROUP_0, 0x4C);
    pub const PS_BLEND: u32 = opcode_key(OPCODE_GROUP_0, 0x4D);
    pub const WM_DEPTH_STENCIL: u32 = opcode_key(OPCODE_GROUP_0, 0x4E);
    pub const PS_EXTRA: u32 = opcode_key(OPCODE_GROUP_0, 0x4F);
    pub const RASTER: u32 = opcode_key(OPCODE_GROUP_0, 0x50);
    pub const SBE_SWIZ: u32 = opcode_key(OPCODE_GROUP_0, 0x51);
    pub const WM_HZ_OP: u32 = opcode_key(OPCODE_GROUP_0, 0x52);
    pub const INT: u32 = opcode_key(OPCODE_GROUP_0, 0x53);
    pub const RS_CONSTANT_POINTER: u32 = opcode_key(OPCODE_GROUP_0, 0x54);
    pub const VF_COMPONENT_PACKING: u32 = opcode_key(OPCODE_GROUP_0, 0x55);
    pub const VF_SGVS_2: u32 = opcode_key(OPCODE_GROUP_0, 0x56);
    pub const URB_ALLOC_VS: u32 = opcode_key(OPCODE_GROUP_0, 0x58);
    pub const URB_ALLOC_HS: u32 = opcode_key(OPCODE_GROUP_0, 0x59);
    pub const URB_ALLOC_DS: u32 = opcode_key(OPCODE_GROUP_0, 0x5A);
    pub const URB_ALLOC_GS: u32 = opcode_key(OPCODE_GROUP_0, 0x5B);
    pub const SO_BUFFER_INDEX_0: u32 = opcode_key(OPCODE_GROUP_0, 0x60);
    pub const SO_BUFFER_INDEX_1: u32 = opcode_key(OPCODE_GROUP_0, 0x61);
    pub const SO_BUFFER_INDEX_2: u32 = opcode_key(OPCODE_GROUP_0, 0x62);
    pub const SO_BUFFER_INDEX_3: u32 = opcode_key(OPCODE_GROUP_0, 0x63);
    pub const PTBR_MARKER: u32 = opcode_key(OPCODE_GROUP_0, 0x6A);
    pub const PTBR_TILE_SELECT: u32 = opcode_key(OPCODE_GROUP_0, 0x6B);
    pub const PRIMITIVE_REPLICATION: u32 = opcode_key(OPCODE_GROUP_0, 0x6C);
    pub const CONSTANT_ALL: u32 = opcode_key(OPCODE_GROUP_0, 0x6D);
    pub const AMFS: u32 = opcode_key(OPCODE_GROUP_0, 0x6F);
    pub const DEPTH_CNTL_BUFFER: u32 = opcode_key(OPCODE_GROUP_0, 0x70);
    pub const DEPTH_BOUNDS: u32 = opcode_key(OPCODE_GROUP_0, 0x71);
    pub const AMFS_TEXTURE_POINTERS: u32 = opcode_key(OPCODE_GROUP_0, 0x72);
    pub const CONSTANT_TS_POINTER: u32 = opcode_key(OPCODE_GROUP_0, 0x73);

    pub const DRAWING_RECTANGLE: u32 = opcode_key(OPCODE_GROUP_1, 0x00);
    pub const CHROMA_KEY: u32 = opcode_key(OPCODE_GROUP_1, 0x04);
    pub const POLY_STIPPLE_OFFSET: u32 = opcode_key(OPCODE_GROUP_1, 0x06);
    pub const POLY_STIPPLE_PATTERN: u32 = opcode_key(OPCODE_GROUP_1, 0x07);
    pub const LINE_STIPPLE: u32 = opcode_key(OPCODE_GROUP_1, 0x08);
    pub const AA_LINE_PARAMS: u32 = opcode_key(OPCODE_GROUP_1, 0x0A);
    pub const GS_SVB_INDEX: u32 = opcode_key(OPCODE_GROUP_1, 0x0B);
    pub const MULTISAMPLE: u32 = opcode_key(OPCODE_GROUP_1, 0x0D);
    pub const STENCIL_BUFFER: u32 = opcode_key(OPCODE_GROUP_1, 0x0E);
    pub const HIER_DEPTH_BUFFER: u32 = opcode_key(OPCODE_GROUP_1, 0x0F);
    pub const CLEAR_PARAMS: u32 = opcode_key(OPCODE_GROUP_1, 0x10);
    pub const MONOFILTER_SIZE: u32 = opcode_key(OPCODE_GROUP_1, 0x11);
    pub const PUSH_CONSTANT_ALLOC_VS: u32 = opcode_key(OPCODE_GROUP_1, 0x12);
    pub const PUSH_CONSTANT_ALLOC_HS: u32 = opcode_key(OPCODE_GROUP_1, 0x13);
    pub const PUSH_CONSTANT_ALLOC_DS: u32 = opcode_key(OPCODE_GROUP_1, 0x14);
    pub const PUSH_CONSTANT_ALLOC_GS: u32 = opcode_key(OPCODE_GROUP_1, 0x15);
    pub const PUSH_CONSTANT_ALLOC_PS: u32 = opcode_key(OPCODE_GROUP_1, 0x16);
    pub const SO_DECL_LIST: u32 = opcode_key(OPCODE_GROUP_1, 0x17);
    pub const SO_BUFFER: u32 = opcode_key(OPCODE_GROUP_1, 0x18);
    pub const BINDING_TABLE_POOL_ALLOC: u32 = opcode_key(OPCODE_GROUP_1, 0x19);
    pub const GATHER_POOL_ALLOC: u32 = opcode_key(OPCODE_GROUP_1, 0x1A);
    pub const DX9_CONSTANT_BUFFER_POOL_ALLOC: u32 = opcode_key(OPCODE_GROUP_1, 0x1B);
    pub const SAMPLE_PATTERN: u32 = opcode_key(OPCODE_GROUP_1, 0x1C);
    pub const URB_CLEAR: u32 = opcode_key(OPCODE_GROUP_1, 0x1D);
    pub const MODE_3D: u32 = opcode_key(OPCODE_GROUP_1, 0x1E);
}

#[inline]
fn edge_fn(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (px - ax) * (by - ay) - (py - ay) * (bx - ax)
}

#[inline]
fn clamp_color(v: f32) -> u32 {
    if v <= 0.0 {
        0
    } else if v >= 1.0 {
        255
    } else {
        (v * 255.0 + 0.5) as u32
    }
}

#[inline]
fn pack_xrgb8888(r: u32, g: u32, b: u32) -> u32 {
    (r << 16) | (g << 8) | b
}

pub(crate) fn submit_rgb_triangle_smoke_once() {
    crate::log!(
        "intel/render-ngin: api route=render.rgb-triangle.submit workload=rgb-triangle class=render transport=rcs-execlist summary=submit an RGB triangle proof through the Xe-LP render ngin\n"
    );
    super::intel_igpu770::ggtt_blt_smoke_test_once();
}

pub(crate) fn encode_rgb_triangle_store_batch(
    batch_dwords: &mut [u32],
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    result_gpu_addr: u64,
    done_value: u32,
) -> Result<usize, &'static str> {
    const RESERVED_END_DWORDS: usize = 2;
    const STORE_DWORDS: usize = 4;

    if batch_dwords.len() <= RESERVED_END_DWORDS + STORE_DWORDS {
        return Err("batch-too-small");
    }
    if rect_w < TRIANGLE_MIN_DIM || rect_h < TRIANGLE_MIN_DIM {
        return Err("triangle-too-small");
    }

    let tri_w = rect_w.min(TRIANGLE_MAX_W).max(TRIANGLE_MIN_DIM);
    let tri_h = rect_h.min(TRIANGLE_MAX_H).max(TRIANGLE_MIN_DIM);
    let center_x = rect_w as f32 * 0.5;
    let center_y = rect_h as f32 * 0.5;
    let radius = tri_w.min(tri_h) as f32 * 0.46;
    let a0 = TRIANGLE_ROTATION_RAD;
    let a1 = TRIANGLE_ROTATION_RAD + TRIANGLE_ANGLE_STEP_RAD;
    let a2 = TRIANGLE_ROTATION_RAD + (TRIANGLE_ANGLE_STEP_RAD * 2.0);

    let p0x = cosf(a0) * radius;
    let p0y = sinf(a0) * radius;
    let p1x = cosf(a1) * radius;
    let p1y = sinf(a1) * radius;
    let p2x = cosf(a2) * radius;
    let p2y = sinf(a2) * radius;

    let v0x = center_x + p0x;
    let v0y = center_y + p0y;
    let v1x = center_x + p1x;
    let v1y = center_y + p1y;
    let v2x = center_x + p2x;
    let v2y = center_y + p2y;
    let area = edge_fn(v0x, v0y, v1x, v1y, v2x, v2y);
    if area == 0.0 {
        return Err("triangle-degenerate");
    }

    let min_x = floorf(v0x.min(v1x).min(v2x)).max(0.0) as usize;
    let max_x = ceilf(v0x.max(v1x).max(v2x)).min(rect_w as f32) as usize;
    let min_y = floorf(v0y.min(v1y).min(v2y)).max(0.0) as usize;
    let max_y = ceilf(v0y.max(v1y).max(v2y)).min(rect_h as f32) as usize;

    batch_dwords.fill(0);
    let writable_limit = batch_dwords
        .len()
        .saturating_sub(RESERVED_END_DWORDS + STORE_DWORDS);
    let mut idx = 0usize;

    for y in min_y..max_y {
        for x in min_x..max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let w0 = edge_fn(v1x, v1y, v2x, v2y, px, py) / area;
            let w1 = edge_fn(v2x, v2y, v0x, v0y, px, py) / area;
            let w2 = edge_fn(v0x, v0y, v1x, v1y, px, py) / area;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            if idx + STORE_DWORDS > writable_limit {
                return Err("batch-exhausted");
            }

            let r = clamp_color(w0);
            let g = clamp_color(w1);
            let b = clamp_color(w2);
            let color = pack_xrgb8888(r, g, b);
            let dst = dst_gpu_addr
                .saturating_add((y as u64).saturating_mul(pitch as u64))
                .saturating_add((x as u64).saturating_mul(4));

            batch_dwords[idx] = xelp_copy_ngin::mi::STORE_DATA_IMM
                | xelp_copy_ngin::mi::SDI_GGTT
                | xelp_copy_ngin::mi::sdi_num_dw(1);
            batch_dwords[idx + 1] = dst as u32;
            batch_dwords[idx + 2] = (dst >> 32) as u32;
            batch_dwords[idx + 3] = color;
            idx += STORE_DWORDS;
        }
    }

    if idx == 0 {
        return Err("triangle-empty");
    }
    if idx + STORE_DWORDS > batch_dwords.len().saturating_sub(RESERVED_END_DWORDS) {
        return Err("batch-no-result-slot");
    }

    batch_dwords[idx] = xelp_copy_ngin::mi::STORE_DATA_IMM
        | xelp_copy_ngin::mi::SDI_GGTT
        | xelp_copy_ngin::mi::sdi_num_dw(1);
    batch_dwords[idx + 1] = result_gpu_addr as u32;
    batch_dwords[idx + 2] = (result_gpu_addr >> 32) as u32;
    batch_dwords[idx + 3] = done_value;
    idx += STORE_DWORDS;

    if idx + RESERVED_END_DWORDS > batch_dwords.len() {
        return Err("batch-no-end");
    }

    batch_dwords[idx] = xelp_copy_ngin::mi::BATCH_BUFFER_END;
    batch_dwords[idx + 1] = xelp_copy_ngin::mi::NOOP;
    idx += RESERVED_END_DWORDS;

    Ok(idx * core::mem::size_of::<u32>())
}
