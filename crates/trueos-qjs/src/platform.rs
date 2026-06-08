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
        fn trueos_cabi_ui3_scene_begin(browser_id: u32, root_id: u32) -> i32;
        fn trueos_cabi_ui3_scene_node(browser_id: u32, node_id: u32, kind: u32) -> i32;
        fn trueos_cabi_ui3_scene_add_child(browser_id: u32, parent: u32, child: u32) -> i32;
        fn trueos_cabi_ui3_scene_add_child_at(
            browser_id: u32,
            parent: u32,
            child: u32,
            index: u32,
        ) -> i32;
        fn trueos_cabi_ui3_scene_set_child_index(
            browser_id: u32,
            parent: u32,
            child: u32,
            index: u32,
        ) -> i32;
        fn trueos_cabi_ui3_scene_remove_child(browser_id: u32, parent: u32, child: u32) -> i32;
        fn trueos_cabi_ui3_scene_remove_from_parent(browser_id: u32, node_id: u32) -> i32;
        fn trueos_cabi_ui3_scene_remove_children(browser_id: u32, parent: u32) -> i32;
        fn trueos_cabi_ui3_scene_visible(browser_id: u32, node_id: u32, visible: bool) -> i32;
        fn trueos_cabi_ui3_scene_listen(
            browser_id: u32,
            node_id: u32,
            event_ptr: *const u8,
            event_len: usize,
        ) -> i32;
        fn trueos_cabi_ui3_scene_remove_all_listeners(browser_id: u32, node_id: u32) -> i32;
        fn trueos_cabi_ui3_scene_position(browser_id: u32, node_id: u32, x: f32, y: f32) -> i32;
        fn trueos_cabi_ui3_scene_graphics_clear(browser_id: u32, node_id: u32) -> i32;
        fn trueos_cabi_ui3_scene_graphics_rect(
            browser_id: u32,
            node_id: u32,
            x: f32,
            y: f32,
            w: f32,
            h: f32,
        ) -> i32;
        fn trueos_cabi_ui3_scene_graphics_fill(
            browser_id: u32,
            node_id: u32,
            rgb: u32,
            alpha: f32,
        ) -> i32;
        fn trueos_cabi_ui3_scene_graphics_stroke(
            browser_id: u32,
            node_id: u32,
            rgb: u32,
            alpha: f32,
            width: f32,
        ) -> i32;
        fn trueos_cabi_ui3_scene_graphics_circle(
            browser_id: u32,
            node_id: u32,
            x: f32,
            y: f32,
            radius: f32,
        ) -> i32;
        fn trueos_cabi_ui3_scene_graphics_move_to(
            browser_id: u32,
            node_id: u32,
            x: f32,
            y: f32,
        ) -> i32;
        fn trueos_cabi_ui3_scene_graphics_line_to(
            browser_id: u32,
            node_id: u32,
            x: f32,
            y: f32,
        ) -> i32;
        fn trueos_cabi_ui3_scene_text(
            browser_id: u32,
            node_id: u32,
            text_ptr: *const u8,
            text_len: usize,
        ) -> i32;
        fn trueos_cabi_ui3_scene_text_fill(
            browser_id: u32,
            node_id: u32,
            rgb: u32,
            alpha: f32,
        ) -> i32;
        fn trueos_cabi_ui3_scene_render(browser_id: u32, root_id: u32) -> i32;
        fn trueos_cabi_ui3_native_hello_scene(
            browser_id: u32,
            html_ptr: *const u8,
            html_len: usize,
        ) -> i32;
    }

    #[inline]
    pub fn signal_hosted_browser_dirty(content_id: u32, flags: u32) {
        unsafe { trueos_cabi_ui2_signal_hosted_browser_dirty(content_id, flags) };
    }

    #[inline]
    pub fn ui3_scene_begin(browser_id: u32, root_id: u32) -> bool {
        unsafe { trueos_cabi_ui3_scene_begin(browser_id, root_id) >= 0 }
    }

    #[inline]
    pub fn ui3_scene_node(browser_id: u32, node_id: u32, kind: u32) -> bool {
        unsafe { trueos_cabi_ui3_scene_node(browser_id, node_id, kind) >= 0 }
    }

    #[inline]
    pub fn ui3_scene_add_child(browser_id: u32, parent: u32, child: u32) -> bool {
        unsafe { trueos_cabi_ui3_scene_add_child(browser_id, parent, child) >= 0 }
    }

    #[inline]
    pub fn ui3_scene_add_child_at(browser_id: u32, parent: u32, child: u32, index: u32) -> bool {
        unsafe { trueos_cabi_ui3_scene_add_child_at(browser_id, parent, child, index) >= 0 }
    }

    #[inline]
    pub fn ui3_scene_set_child_index(browser_id: u32, parent: u32, child: u32, index: u32) -> bool {
        unsafe { trueos_cabi_ui3_scene_set_child_index(browser_id, parent, child, index) >= 0 }
    }

    #[inline]
    pub fn ui3_scene_remove_child(browser_id: u32, parent: u32, child: u32) -> bool {
        unsafe { trueos_cabi_ui3_scene_remove_child(browser_id, parent, child) >= 0 }
    }

    #[inline]
    pub fn ui3_scene_remove_from_parent(browser_id: u32, node_id: u32) -> bool {
        unsafe { trueos_cabi_ui3_scene_remove_from_parent(browser_id, node_id) >= 0 }
    }

    #[inline]
    pub fn ui3_scene_remove_children(browser_id: u32, parent: u32) -> bool {
        unsafe { trueos_cabi_ui3_scene_remove_children(browser_id, parent) >= 0 }
    }

    #[inline]
    pub fn ui3_scene_visible(browser_id: u32, node_id: u32, visible: bool) -> bool {
        unsafe { trueos_cabi_ui3_scene_visible(browser_id, node_id, visible) >= 0 }
    }

    #[inline]
    pub fn ui3_scene_listen(browser_id: u32, node_id: u32, event: &str) -> bool {
        unsafe {
            trueos_cabi_ui3_scene_listen(browser_id, node_id, event.as_ptr(), event.len()) >= 0
        }
    }

    #[inline]
    pub fn ui3_scene_remove_all_listeners(browser_id: u32, node_id: u32) -> bool {
        unsafe { trueos_cabi_ui3_scene_remove_all_listeners(browser_id, node_id) >= 0 }
    }

    #[inline]
    pub fn ui3_scene_position(browser_id: u32, node_id: u32, x: f32, y: f32) -> bool {
        unsafe { trueos_cabi_ui3_scene_position(browser_id, node_id, x, y) >= 0 }
    }

    #[inline]
    pub fn ui3_scene_graphics_clear(browser_id: u32, node_id: u32) -> bool {
        unsafe { trueos_cabi_ui3_scene_graphics_clear(browser_id, node_id) >= 0 }
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
        unsafe { trueos_cabi_ui3_scene_graphics_rect(browser_id, node_id, x, y, w, h) >= 0 }
    }

    #[inline]
    pub fn ui3_scene_graphics_fill(browser_id: u32, node_id: u32, rgb: u32, alpha: f32) -> bool {
        unsafe { trueos_cabi_ui3_scene_graphics_fill(browser_id, node_id, rgb, alpha) >= 0 }
    }

    #[inline]
    pub fn ui3_scene_graphics_stroke(
        browser_id: u32,
        node_id: u32,
        rgb: u32,
        alpha: f32,
        width: f32,
    ) -> bool {
        unsafe {
            trueos_cabi_ui3_scene_graphics_stroke(browser_id, node_id, rgb, alpha, width) >= 0
        }
    }

    #[inline]
    pub fn ui3_scene_graphics_circle(
        browser_id: u32,
        node_id: u32,
        x: f32,
        y: f32,
        radius: f32,
    ) -> bool {
        unsafe { trueos_cabi_ui3_scene_graphics_circle(browser_id, node_id, x, y, radius) >= 0 }
    }

    #[inline]
    pub fn ui3_scene_graphics_move_to(browser_id: u32, node_id: u32, x: f32, y: f32) -> bool {
        unsafe { trueos_cabi_ui3_scene_graphics_move_to(browser_id, node_id, x, y) >= 0 }
    }

    #[inline]
    pub fn ui3_scene_graphics_line_to(browser_id: u32, node_id: u32, x: f32, y: f32) -> bool {
        unsafe { trueos_cabi_ui3_scene_graphics_line_to(browser_id, node_id, x, y) >= 0 }
    }

    #[inline]
    pub fn ui3_scene_text(browser_id: u32, node_id: u32, text: &str) -> bool {
        unsafe { trueos_cabi_ui3_scene_text(browser_id, node_id, text.as_ptr(), text.len()) >= 0 }
    }

    #[inline]
    pub fn ui3_scene_text_fill(browser_id: u32, node_id: u32, rgb: u32, alpha: f32) -> bool {
        unsafe { trueos_cabi_ui3_scene_text_fill(browser_id, node_id, rgb, alpha) >= 0 }
    }

    #[inline]
    pub fn ui3_scene_render(browser_id: u32, root_id: u32) -> bool {
        unsafe { trueos_cabi_ui3_scene_render(browser_id, root_id) >= 0 }
    }

    #[inline]
    pub fn ui3_native_hello_scene(browser_id: u32, html: &str) -> i32 {
        unsafe { trueos_cabi_ui3_native_hello_scene(browser_id, html.as_ptr(), html.len()) }
    }
}
