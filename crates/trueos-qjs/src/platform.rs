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
        unsafe { v::vcabi::trueos_cabi_gfx_upload_texture_png(tex_id, data_ptr, data_len) }
    }

    #[inline]
    pub unsafe fn upload_texture_png_async(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        unsafe { v::vcabi::trueos_cabi_gfx_upload_texture_png_async(tex_id, data_ptr, data_len) }
    }

    #[inline]
    pub unsafe fn upload_texture_jpeg(tex_id: u32, data_ptr: *const u8, data_len: usize) -> i32 {
        unsafe { v::vcabi::trueos_cabi_gfx_upload_texture_jpeg(tex_id, data_ptr, data_len) }
    }

    #[inline]
    pub unsafe fn upload_texture_jpeg_async(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        unsafe { v::vcabi::trueos_cabi_gfx_upload_texture_jpeg_async(tex_id, data_ptr, data_len) }
    }

    #[inline]
    pub unsafe fn upload_texture_svg(tex_id: u32, data_ptr: *const u8, data_len: usize) -> i32 {
        unsafe { v::vcabi::trueos_cabi_gfx_upload_texture_svg(tex_id, data_ptr, data_len) }
    }

    #[inline]
    pub unsafe fn upload_texture_svg_async(
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32 {
        unsafe { v::vcabi::trueos_cabi_gfx_upload_texture_svg_async(tex_id, data_ptr, data_len) }
    }

    #[inline]
    pub fn texture_status(tex_id: u32) -> i32 {
        unsafe { v::vcabi::trueos_cabi_gfx_texture_status(tex_id) }
    }

    #[inline]
    pub fn texture_dimensions(tex_id: u32) -> Option<(u32, u32)> {
        let mut width = 0;
        let mut height = 0;
        let rc = unsafe {
            v::vcabi::trueos_cabi_gfx_texture_dimensions(
                tex_id,
                &mut width as *mut u32,
                &mut height as *mut u32,
            )
        };
        (rc == 0).then_some((width, height))
    }
}

pub mod ui {
    unsafe extern "C" {
        fn trueos_cabi_ui2_signal_hosted_browser_dirty(content_id: u32, flags: u32);
        fn trueos_cabi_ui3_pixi_op(
            browser_id: u32,
            op_code: u32,
            node: u32,
            a: f32,
            b: f32,
            c: f32,
            d: f32,
            text_ptr: *const u8,
            text_len: usize,
        ) -> i32;
    }

    #[inline]
    pub fn signal_hosted_browser_dirty(content_id: u32, flags: u32) {
        unsafe { trueos_cabi_ui2_signal_hosted_browser_dirty(content_id, flags) };
    }

    #[inline]
    fn ui3_pixi_op(
        browser_id: u32,
        op_code: u32,
        node: u32,
        a: f32,
        b: f32,
        c: f32,
        d: f32,
        text: Option<&str>,
    ) -> bool {
        let (text_ptr, text_len) = text
            .map(|text| (text.as_ptr(), text.len()))
            .unwrap_or((core::ptr::null(), 0));
        unsafe {
            trueos_cabi_ui3_pixi_op(browser_id, op_code, node, a, b, c, d, text_ptr, text_len) >= 0
        }
    }

    #[inline]
    pub fn ui3_scene_begin(browser_id: u32, root_id: u32) -> bool {
        ui3_pixi_op(browser_id, 0, root_id, 0.0, 0.0, 0.0, 0.0, None)
    }

    #[inline]
    pub fn ui3_scene_node(browser_id: u32, node_id: u32, kind: u32) -> bool {
        ui3_pixi_op(browser_id, 1, node_id, kind as f32, 0.0, 0.0, 0.0, None)
    }

    #[inline]
    pub fn ui3_scene_add_child(browser_id: u32, parent: u32, child: u32) -> bool {
        ui3_pixi_op(browser_id, 2, parent, child as f32, 0.0, 0.0, 0.0, None)
    }

    #[inline]
    pub fn ui3_scene_add_child_at(browser_id: u32, parent: u32, child: u32, index: u32) -> bool {
        ui3_pixi_op(browser_id, 10, parent, child as f32, index as f32, 0.0, 0.0, None)
    }

    #[inline]
    pub fn ui3_scene_set_child_index(browser_id: u32, parent: u32, child: u32, index: u32) -> bool {
        ui3_pixi_op(browser_id, 11, parent, child as f32, index as f32, 0.0, 0.0, None)
    }

    #[inline]
    pub fn ui3_scene_remove_child(browser_id: u32, parent: u32, child: u32) -> bool {
        ui3_pixi_op(browser_id, 12, parent, child as f32, 0.0, 0.0, 0.0, None)
    }

    #[inline]
    pub fn ui3_scene_remove_from_parent(browser_id: u32, node_id: u32) -> bool {
        ui3_pixi_op(browser_id, 13, node_id, 0.0, 0.0, 0.0, 0.0, None)
    }

    #[inline]
    pub fn ui3_scene_remove_children(browser_id: u32, parent: u32) -> bool {
        ui3_pixi_op(browser_id, 14, parent, 0.0, 0.0, 0.0, 0.0, None)
    }

    #[inline]
    pub fn ui3_scene_visible(browser_id: u32, node_id: u32, visible: bool) -> bool {
        ui3_pixi_op(browser_id, 15, node_id, if visible { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0, None)
    }

    #[inline]
    pub fn ui3_scene_listen(browser_id: u32, node_id: u32, event: &str) -> bool {
        ui3_pixi_op(browser_id, 16, node_id, 0.0, 0.0, 0.0, 0.0, Some(event))
    }

    #[inline]
    pub fn ui3_scene_remove_all_listeners(browser_id: u32, node_id: u32) -> bool {
        ui3_pixi_op(browser_id, 17, node_id, 0.0, 0.0, 0.0, 0.0, None)
    }

    #[inline]
    pub fn ui3_scene_position(browser_id: u32, node_id: u32, x: f32, y: f32) -> bool {
        ui3_pixi_op(browser_id, 3, node_id, x, y, 0.0, 0.0, None)
    }

    #[inline]
    pub fn ui3_scene_graphics_clear(browser_id: u32, node_id: u32) -> bool {
        ui3_pixi_op(browser_id, 4, node_id, 0.0, 0.0, 0.0, 0.0, None)
    }

    #[inline]
    pub fn ui3_scene_graphics_rect(
        browser_id: u32,
        node_id: u32,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> bool {
        ui3_pixi_op(browser_id, 5, node_id, x, y, w, h, None)
    }

    #[inline]
    pub fn ui3_scene_graphics_fill(browser_id: u32, node_id: u32, rgb: u32, alpha: f32) -> bool {
        ui3_pixi_op(browser_id, 6, node_id, rgb as f32, alpha, 0.0, 0.0, None)
    }

    #[inline]
    pub fn ui3_scene_graphics_stroke(
        browser_id: u32,
        node_id: u32,
        rgb: u32,
        alpha: f32,
        width: f32,
    ) -> bool {
        ui3_pixi_op(browser_id, 7, node_id, rgb as f32, alpha, width, 0.0, None)
    }

    #[inline]
    pub fn ui3_scene_graphics_circle(
        browser_id: u32,
        node_id: u32,
        x: f32,
        y: f32,
        radius: f32,
    ) -> bool {
        ui3_pixi_op(browser_id, 18, node_id, x, y, radius, 0.0, None)
    }

    #[inline]
    pub fn ui3_scene_graphics_move_to(browser_id: u32, node_id: u32, x: f32, y: f32) -> bool {
        ui3_pixi_op(browser_id, 19, node_id, x, y, 0.0, 0.0, None)
    }

    #[inline]
    pub fn ui3_scene_graphics_line_to(browser_id: u32, node_id: u32, x: f32, y: f32) -> bool {
        ui3_pixi_op(browser_id, 20, node_id, x, y, 0.0, 0.0, None)
    }

    #[inline]
    pub fn ui3_scene_texture_rect(
        browser_id: u32,
        node_id: u32,
        tex_id: u32,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> bool {
        let h_text = alloc::format!("{}", h);
        ui3_pixi_op(browser_id, 22, node_id, tex_id as f32, x, y, w, Some(h_text.as_str()))
    }

    #[inline]
    pub fn ui3_scene_text(browser_id: u32, node_id: u32, text: &str) -> bool {
        ui3_pixi_op(browser_id, 8, node_id, 0.0, 0.0, 0.0, 0.0, Some(text))
    }

    #[inline]
    pub fn ui3_scene_text_fill(browser_id: u32, node_id: u32, rgb: u32, alpha: f32) -> bool {
        ui3_pixi_op(browser_id, 9, node_id, rgb as f32, alpha, 0.0, 0.0, None)
    }

    #[inline]
    pub fn ui3_scene_render(browser_id: u32, root_id: u32) -> bool {
        ui3_pixi_op(browser_id, 21, root_id, 0.0, 0.0, 0.0, 0.0, None)
    }
}
