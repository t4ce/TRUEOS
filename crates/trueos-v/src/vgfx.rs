extern crate alloc;

use alloc::vec::Vec;
use alloc::{string::String, vec};

use crate::vcabi;
pub use crate::vcabi::TrueosGfxTraceEntry;

pub const GFX_TRACE_OP_BEGIN_FRAME: u32 = 1;
pub const GFX_TRACE_OP_END_FRAME: u32 = 2;
pub const GFX_TRACE_OP_SET_BLEND: u32 = 3;
pub const GFX_TRACE_OP_SET_SAMPLER: u32 = 4;
pub const GFX_TRACE_OP_SET_SCISSOR: u32 = 5;
pub const GFX_TRACE_OP_CLEAR_SCISSOR: u32 = 6;
pub const GFX_TRACE_OP_SET_RENDER_TARGET: u32 = 7;
pub const GFX_TRACE_OP_CLEAR_RENDER_TARGET: u32 = 8;
pub const GFX_TRACE_OP_UPLOAD_TEXTURE_RGBA: u32 = 9;
pub const GFX_TRACE_OP_UPLOAD_TEXTURE_PNG: u32 = 10;
pub const GFX_TRACE_OP_UPLOAD_TEXTURE_JPEG: u32 = 11;
pub const GFX_TRACE_OP_UPLOAD_TEXTURE_SVG: u32 = 12;
pub const GFX_TRACE_OP_DRAW_RGB_TRIANGLES: u32 = 13;
pub const GFX_TRACE_OP_DRAW_TEX_TRIANGLES: u32 = 14;

#[inline]
pub fn gfx_trace_set_enabled(enabled: bool) -> bool {
    unsafe { vcabi::trueos_cabi_gfx_trace_set_enabled(enabled as u32) != 0 }
}

#[inline]
pub fn gfx_trace_clear() {
    unsafe { vcabi::trueos_cabi_gfx_trace_clear() }
}

#[inline]
pub fn gfx_trace_snapshot(max_entries: u32) -> Vec<TrueosGfxTraceEntry> {
    if max_entries == 0 {
        return Vec::new();
    }
    let mut out = vec![TrueosGfxTraceEntry::default(); max_entries as usize];
    let got = unsafe { vcabi::trueos_cabi_gfx_trace_snapshot(out.as_mut_ptr(), max_entries) };
    out.truncate(got as usize);
    out
}

#[inline]
pub fn upload_svg_to_texture(tex_id: u32, svg: &[u8]) -> i32 {
    if svg.is_empty() {
        return -1;
    }
    unsafe { vcabi::trueos_cabi_gfx_upload_texture_svg(tex_id, svg.as_ptr(), svg.len()) }
}

#[inline]
pub fn upload_svg_to_texture_async(tex_id: u32, svg: &[u8]) -> i32 {
    if svg.is_empty() {
        return -1;
    }
    unsafe { vcabi::trueos_cabi_gfx_upload_texture_svg_async(tex_id, svg.as_ptr(), svg.len()) }
}

#[inline]
pub fn probe_upload_svg_to_texture_async(tex_id: u32) -> i32 {
    unsafe { vcabi::trueos_cabi_gfx_upload_texture_svg_async(tex_id, core::ptr::null(), 0) }
}

#[inline]
pub fn texture_status(tex_id: u32) -> i32 {
    unsafe { vcabi::trueos_cabi_gfx_texture_status(tex_id) }
}

#[inline]
pub fn capture_screenshot_data_url() -> Option<String> {
    let len =
        unsafe { vcabi::trueos_cabi_gfx_capture_screenshot_data_url(core::ptr::null_mut(), 0) };
    if len <= 0 {
        return None;
    }

    let mut bytes = vec![0u8; len as usize];
    let got = unsafe {
        vcabi::trueos_cabi_gfx_capture_screenshot_data_url(bytes.as_mut_ptr(), bytes.len())
    };
    if got <= 0 {
        return None;
    }
    bytes.truncate(got as usize);
    String::from_utf8(bytes).ok()
}
