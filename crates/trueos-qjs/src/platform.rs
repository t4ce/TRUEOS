#![cfg(feature = "trueos")]

pub mod sys {
    #[inline]
    pub fn write_stream(stream: u32, bytes: &[u8]) {
        v::vsys::write_stream(stream, bytes);
    }

    #[inline]
    pub fn write_stdout(bytes: &[u8]) {
        write_stream(1, bytes);
    }

    #[inline]
    pub fn write_stderr(bytes: &[u8]) {
        write_stream(2, bytes);
    }

    #[inline]
    pub fn poll_once() {
        v::vsys::poll_once();
    }
}

pub mod gfx {
    unsafe extern "C" {
        fn trueos_cabi_gfx_upload_texture_rgba(
            tex_id: u32,
            width: u32,
            height: u32,
            data_ptr: *const u8,
            data_len: usize,
        ) -> i32;
        fn trueos_cabi_gfx_upload_texture_png(
            tex_id: u32,
            data_ptr: *const u8,
            data_len: usize,
        ) -> i32;
        fn trueos_cabi_gfx_upload_texture_png_async(
            tex_id: u32,
            data_ptr: *const u8,
            data_len: usize,
        ) -> i32;
        fn trueos_cabi_gfx_upload_texture_jpeg(
            tex_id: u32,
            data_ptr: *const u8,
            data_len: usize,
        ) -> i32;
        fn trueos_cabi_gfx_upload_texture_jpeg_async(
            tex_id: u32,
            data_ptr: *const u8,
            data_len: usize,
        ) -> i32;
        fn trueos_cabi_gfx_upload_texture_svg(
            tex_id: u32,
            data_ptr: *const u8,
            data_len: usize,
        ) -> i32;
        fn trueos_cabi_gfx_upload_texture_svg_async(
            tex_id: u32,
            data_ptr: *const u8,
            data_len: usize,
        ) -> i32;
    }

    #[inline]
    pub unsafe fn upload_texture_rgba(
        tex_id: u32,
        width: u32,
        height: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        unsafe { trueos_cabi_gfx_upload_texture_rgba(tex_id, width, height, data_ptr, data_len) }
    }

    #[inline]
    pub unsafe fn upload_texture_png(tex_id: u32, data_ptr: *const u8, data_len: usize) -> i32 {
        unsafe { trueos_cabi_gfx_upload_texture_png(tex_id, data_ptr, data_len) }
    }

    #[inline]
    pub unsafe fn upload_texture_png_async(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        unsafe { trueos_cabi_gfx_upload_texture_png_async(tex_id, data_ptr, data_len) }
    }

    #[inline]
    pub unsafe fn upload_texture_jpeg(tex_id: u32, data_ptr: *const u8, data_len: usize) -> i32 {
        unsafe { trueos_cabi_gfx_upload_texture_jpeg(tex_id, data_ptr, data_len) }
    }

    #[inline]
    pub unsafe fn upload_texture_jpeg_async(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        unsafe { trueos_cabi_gfx_upload_texture_jpeg_async(tex_id, data_ptr, data_len) }
    }

    #[inline]
    pub unsafe fn upload_texture_svg(tex_id: u32, data_ptr: *const u8, data_len: usize) -> i32 {
        unsafe { trueos_cabi_gfx_upload_texture_svg(tex_id, data_ptr, data_len) }
    }

    #[inline]
    pub unsafe fn upload_texture_svg_async(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        unsafe { trueos_cabi_gfx_upload_texture_svg_async(tex_id, data_ptr, data_len) }
    }

    #[inline]
    pub fn texture_status(tex_id: u32) -> i32 {
        unsafe { v::qjs_abi::trueos_cabi_gfx_texture_status(tex_id) }
    }

    #[inline]
    pub fn texture_dimensions(tex_id: u32) -> Option<(u32, u32)> {
        let mut width = 0;
        let mut height = 0;
        let rc = unsafe {
            v::qjs_abi::trueos_cabi_gfx_texture_dimensions(
                tex_id,
                &mut width as *mut u32,
                &mut height as *mut u32,
            )
        };
        (rc >= 0 && width > 0 && height > 0).then_some((width, height))
    }
}

pub mod ui {
    #[inline]
    pub fn signal_hosted_browser_dirty(content_id: u32, flags: u32) {
        let _ = (content_id, flags);
    }
}
