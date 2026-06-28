pub use crate::ui3::ui3_img::{
    trueos_cabi_gfx_texture_dimensions, trueos_cabi_gfx_texture_status,
    trueos_cabi_gfx_upload_texture_jpeg, trueos_cabi_gfx_upload_texture_jpeg_async,
    trueos_cabi_gfx_upload_texture_png, trueos_cabi_gfx_upload_texture_png_async,
    trueos_cabi_gfx_upload_texture_rgba, trueos_cabi_gfx_upload_texture_rgba_image,
    trueos_cabi_gfx_upload_texture_rgba_image_async, trueos_cabi_gfx_upload_texture_svg,
    trueos_cabi_gfx_upload_texture_svg_async,
};

#[inline]
fn vmcall_i32(op: u32, arg0: u64, arg1: u64) -> i32 {
    let (status, data) = crate::hv::vmcall::guest_call(op, arg0, arg1);
    if status == crate::hv::vmcall::STATUS_OK {
        data as i64 as i32
    } else {
        -1
    }
}

#[inline]
fn vmcall_payload_i32(op: u32, arg0: u64, arg1: u64, payload: &[u8]) -> i32 {
    let (status, data) = trueos_vm::vmcall::call_with_payload(op, arg0, arg1, payload, &mut []);
    if status == trueos_vm::vmcall::STATUS_OK {
        data as i64 as i32
    } else {
        -1
    }
}

fn vmcall_chunked_triangles(op: u32, arg0: u64, arg1: u64, bytes: &[u8], tri_bytes: usize) -> i32 {
    if tri_bytes == 0 || bytes.len() % tri_bytes != 0 {
        return -3;
    }
    if bytes.is_empty() {
        return vmcall_payload_i32(op, arg0, arg1, &[]);
    }
    let max_chunk = (trueos_vm::vmcall::PAYLOAD_CAP / tri_bytes) * tri_bytes;
    if max_chunk == 0 {
        return -3;
    }
    let mut offset = 0usize;
    while offset < bytes.len() {
        let end = core::cmp::min(offset.saturating_add(max_chunk), bytes.len());
        let rc = vmcall_payload_i32(op, arg0, arg1, &bytes[offset..end]);
        if rc != 0 {
            return rc;
        }
        offset = end;
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui3_frame_create(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    tex_id: u32,
) -> u32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let tex_id_bytes = tex_id.to_le_bytes();
        let (status, frame_id) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_UI3_FRAME_CREATE,
            crate::hv::vmcall::pack_i32_pair(x, y),
            crate::hv::vmcall::pack_u32_pair(width, height),
            &tex_id_bytes,
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            frame_id as u32
        } else {
            0
        };
    }

    crate::ui3::ui3_frame::create_frame(x, y, width, height, tex_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui3_frame_close(frame_id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return vmcall_i32(crate::hv::vmcall::OP_BP_UI3_FRAME_CLOSE, frame_id as u64, 0);
    }

    if crate::ui3::ui3_frame::close_frame(frame_id) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui3_frame_request_repaint(frame_id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return vmcall_i32(crate::hv::vmcall::OP_BP_UI3_FRAME_REQUEST_REPAINT, frame_id as u64, 0);
    }

    if crate::ui3::ui3_frame::request_repaint(frame_id) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui3_frame_set_position(frame_id: u32, x: i32, y: i32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return vmcall_i32(
            crate::hv::vmcall::OP_BP_UI3_FRAME_SET_POSITION,
            frame_id as u64,
            crate::hv::vmcall::pack_i32_pair(x, y),
        );
    }

    if crate::ui3::ui3_frame::set_position(frame_id, x, y) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui3_frame_set_size(frame_id: u32, width: u32, height: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return vmcall_i32(
            crate::hv::vmcall::OP_BP_UI3_FRAME_SET_SIZE,
            frame_id as u64,
            crate::hv::vmcall::pack_u32_pair(width, height),
        );
    }

    if crate::ui3::ui3_frame::set_size(frame_id, width, height) {
        0
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui3_frame_begin(
    frame_id: u32,
    clear_rgb: u32,
    preserve_contents: u32,
    allow_present: u32,
) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let flags = (preserve_contents != 0) as u64 | (((allow_present != 0) as u64) << 1);
        return vmcall_i32(
            crate::hv::vmcall::OP_BP_UI3_FRAME_BEGIN,
            frame_id as u64,
            (clear_rgb as u64) | (flags << 32),
        );
    }

    crate::ui3::ui3_frame::begin_frame(
        frame_id,
        clear_rgb,
        preserve_contents != 0,
        allow_present != 0,
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui3_frame_end(frame_id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return vmcall_i32(crate::hv::vmcall::OP_BP_UI3_FRAME_END, frame_id as u64, 0);
    }

    crate::ui3::ui3_frame::end_frame(frame_id)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ui3_frame_set_render_target(frame_id: u32, tex_id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return vmcall_i32(
            crate::hv::vmcall::OP_BP_UI3_FRAME_SET_RENDER_TARGET,
            frame_id as u64,
            tex_id as u64,
        );
    }

    crate::ui3::ui3_frame::set_render_target(frame_id, tex_id)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui3_frame_draw_rgb_triangles(
    frame_id: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        if data_ptr.is_null() && data_len != 0 {
            return -1;
        }
        let bytes = if data_len == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(data_ptr, data_len) }
        };
        return vmcall_chunked_triangles(
            trueos_vm::vmcall::OP_BP_UI3_FRAME_DRAW_RGB_TRIANGLES,
            frame_id as u64,
            0,
            bytes,
            crate::intel::types::RGB_VERTEX_SIZE * 3,
        );
    }

    if data_ptr.is_null() && data_len != 0 {
        return -1;
    }
    let bytes = if data_len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(data_ptr, data_len) }
    };
    crate::ui3::ui3_frame::draw_rgb_triangles(frame_id, bytes)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui3_frame_draw_tex_triangles(
    frame_id: u32,
    tex_id: u32,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        if data_ptr.is_null() && data_len != 0 {
            return -1;
        }
        let bytes = if data_len == 0 {
            &[]
        } else {
            unsafe { core::slice::from_raw_parts(data_ptr, data_len) }
        };
        return vmcall_chunked_triangles(
            trueos_vm::vmcall::OP_BP_UI3_FRAME_DRAW_TEX_TRIANGLES,
            frame_id as u64,
            tex_id as u64,
            bytes,
            crate::intel::types::TEX_VERTEX_SIZE * 3,
        );
    }

    if data_ptr.is_null() && data_len != 0 {
        return -1;
    }
    let bytes = if data_len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(data_ptr, data_len) }
    };
    crate::ui3::ui3_frame::draw_tex_triangles(frame_id, tex_id, bytes)
}
