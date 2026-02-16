extern crate alloc;

extern "C" {
    fn trueos_cabi_gfx_draw_rgb_triangles(clear_rgb: u32, vtx_ptr: *const u8, vtx_len: usize) -> i32;
}

pub(crate) fn submit_rgb_triangles(clear_rgb: u32, vertices: Option<&[u8]>) {
    match vertices {
        Some(vtx) => unsafe {
            let _ = trueos_cabi_gfx_draw_rgb_triangles(clear_rgb, vtx.as_ptr(), vtx.len());
        },
        None => unsafe {
            let _ = trueos_cabi_gfx_draw_rgb_triangles(clear_rgb, core::ptr::null(), 0);
        },
    }
}
