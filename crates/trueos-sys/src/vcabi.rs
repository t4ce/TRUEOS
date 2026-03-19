unsafe extern "C" {
    pub fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    pub fn trueos_cabi_poll_once();
    pub fn trueos_cabi_alloc(size: usize) -> *mut u8;
    pub fn trueos_cabi_calloc(nmemb: usize, size: usize) -> *mut u8;
    pub fn trueos_cabi_free(ptr: *mut u8);
    pub fn trueos_cabi_realloc(ptr: *mut u8, size: usize) -> *mut u8;
    pub fn trueos_cabi_malloc_usable_size(ptr: *const u8) -> usize;

    pub fn trueos_cabi_fs_read_file(
        path_ptr: *const u8,
        path_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    pub fn trueos_cabi_fs_write_begin(
        path_ptr: *const u8,
        path_len: usize,
        total_len: u64,
        out_handle: *mut u32,
    ) -> i32;
    pub fn trueos_cabi_fs_write_chunk(handle: u32, data_ptr: *const u8, data_len: usize) -> i32;
    pub fn trueos_cabi_fs_write_finish(handle: u32) -> i32;
    pub fn trueos_cabi_fs_write_abort(handle: u32) -> i32;
    pub fn trueos_cabi_trueosfs_primary_html_tree(
        max_entries: u32,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    pub fn trueos_cabi_fs_remove(path_ptr: *const u8, path_len: usize) -> i32;

    pub fn trueos_cabi_net_fetch_start(
        url_ptr: *const u8,
        url_len: usize,
        path_ptr: *const u8,
        path_len: usize,
    ) -> u32;
    pub fn trueos_cabi_net_prewarm_url_start(url_ptr: *const u8, url_len: usize) -> i32;
    pub fn trueos_cabi_net_fetch_bytes_start(url_ptr: *const u8, url_len: usize) -> u32;
    pub fn trueos_cabi_net_fetch_post_json_start(
        url_ptr: *const u8,
        url_len: usize,
        path_ptr: *const u8,
        path_len: usize,
        body_ptr: *const u8,
        body_len: usize,
        bearer_ptr: *const u8,
        bearer_len: usize,
    ) -> u32;
    pub fn trueos_cabi_net_fetch_result(op_id: u32) -> i32;
    pub fn trueos_cabi_net_fetch_bytes_result_len(op_id: u32) -> isize;
    pub fn trueos_cabi_net_fetch_bytes_read(op_id: u32, out_ptr: *mut u8, out_cap: usize)
        -> isize;
    pub fn trueos_cabi_net_fetch_discard(op_id: u32) -> i32;
    pub fn trueos_cabi_net_fetch_bytes_discard(op_id: u32) -> i32;
    pub fn trueos_cabi_net_fetch_wait(op_id: u32, timeout_ms: u64) -> i32;
    pub fn trueos_cabi_net_fetch_bytes_wait(op_id: u32, timeout_ms: u64) -> i32;

    pub fn trueos_cabi_input_pop_mouse(
        out_buttons: *mut u8,
        out_dx: *mut i8,
        out_dy: *mut i8,
        out_wheel: *mut i8,
    ) -> i32;
    pub fn trueos_cabi_input_cursor_pos(cursor_id: u32, out_x: *mut i32, out_y: *mut i32) -> i32;
    pub fn trueos_cabi_input_cursor_buttons(cursor_id: u32, out_buttons_down: *mut u32) -> i32;
    pub fn trueos_cabi_input_read_cursor_events_since(
        read_seq: u64,
        out: *mut TrueosHidCursorEvent,
        out_cap: u32,
        out_next_seq: *mut u64,
        out_dropped: *mut u32,
    ) -> u32;
    pub fn trueos_cabi_input_write_cursor(
        slot_id: u32,
        x: i32,
        y: i32,
        buttons_down: u32,
        wheel: i32,
        flags: u32,
    ) -> i32;
    pub fn trueos_cabi_hid_keyboard_read(
        controller_id: u32,
        slot_id: u32,
        ep_target: u32,
        out: *mut TrueosHidKeyboardSample,
        out_cap: u32,
        out_dropped: *mut u32,
    ) -> u32;
    pub fn trueos_cabi_input_write_keyboard_text(
        slot_id: u32,
        text_ptr: *const u8,
        text_len: usize,
        flags: u32,
    ) -> i32;
    pub fn trueos_cabi_input_write_keyboard_key(
        slot_id: u32,
        codepoint: u32,
        key_code: u32,
        modifiers: u32,
        flags: u32,
    ) -> i32;
    pub fn trueos_cabi_mouse_poll(out: *mut TrueosMouseState) -> i32;
    pub fn trueos_cabi_qjs_mouse_pop(out: *mut TrueosMouseState) -> i32;

    pub fn trueos_cabi_uart1_shell_write(data_ptr: *const u8, data_len: usize) -> usize;
    pub fn trueos_cabi_shell2_print_line(data_ptr: *const u8, data_len: usize) -> usize;
    pub fn trueos_cabi_shell2_print_targeted_line(
        target_mask: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> usize;
    pub fn trueos_cabi_shell1_submit_input(data_ptr: *const u8, data_len: usize) -> usize;
    pub fn trueos_cabi_shell_command_registry_json(out_ptr: *mut u8, out_cap: usize) -> isize;
    pub fn trueos_cabi_shell_history_lines_all() -> usize;
    pub fn trueos_cabi_shell_history_lines(
        start_line: usize,
        max_lines: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    pub fn trueos_cabi_shell_qjs_init();
    pub fn trueos_cabi_shell_qjs_write(data_ptr: *const u8, data_len: usize) -> usize;
    pub fn trueos_cabi_shell_qjs_write_byte(byte: u8) -> i32;
    pub fn trueos_cabi_shell_qjs_read(out_ptr: *mut u8, out_cap: usize) -> isize;
    pub fn trueos_cabi_shell_qjs_read_byte() -> i32;

    pub fn trueos_cabi_gfx_capture_screenshot_data_url(out_ptr: *mut u8, out_cap: usize) -> isize;

    pub fn trueos_cabi_ntp_current_unix_seconds() -> u64;
    pub fn trueos_cabi_ntp_kernel_date_day_month_year(out_ptr: *mut u8, out_cap: usize) -> usize;

    pub fn trueos_cabi_ui2_primary_browser_window_id() -> u32;
    pub fn trueos_cabi_ui2_window_create(
        title_ptr: *const u8,
        title_len: usize,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        z: i32,
        alpha: u32,
    ) -> u32;
    pub fn trueos_cabi_ui2_window_info(window_id: u32, out_info: *mut TrueosUi2WindowInfo) -> i32;
    pub fn trueos_cabi_ui2_window_set_title(
        window_id: u32,
        title_ptr: *const u8,
        title_len: usize,
    ) -> i32;
    pub fn trueos_cabi_ui2_window_set_icon(window_id: u32, icon_id: u32) -> i32;
    pub fn trueos_cabi_ui2_window_set_position(window_id: u32, x: i32, y: i32) -> i32;
    pub fn trueos_cabi_ui2_window_set_size(window_id: u32, width: u32, height: u32) -> i32;
    pub fn trueos_cabi_ui2_window_set_decorations(window_id: u32, mode: u32) -> i32;
    pub fn trueos_cabi_ui2_window_set_hit_test_visible(window_id: u32, visible: u32) -> i32;
    pub fn trueos_cabi_ui2_window_set_vertical_scrollbar_side(window_id: u32, side: u32) -> i32;
    pub fn trueos_cabi_ui2_window_set_horizontal_scrollbar_side(window_id: u32, side: u32)
        -> i32;
    pub fn trueos_cabi_ui2_window_minimize(window_id: u32) -> i32;
    pub fn trueos_cabi_ui2_window_maximize(window_id: u32) -> i32;
    pub fn trueos_cabi_ui2_window_restore(window_id: u32) -> i32;
    pub fn trueos_cabi_ui2_window_focus(window_id: u32) -> i32;
    pub fn trueos_cabi_ui2_window_close(window_id: u32) -> i32;
    pub fn trueos_cabi_ui2_window_begin_move(window_id: u32) -> i32;
    pub fn trueos_cabi_ui2_window_begin_resize(window_id: u32, edge_mask: u32) -> i32;
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosMouseState {
    pub x: i32,
    pub y: i32,
    pub dx: i32,
    pub dy: i32,
    pub wheel: i32,
    pub buttons: u32,
    pub seq: u32,
    pub slot_id: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosHidKeyboardSample {
    pub t_ms: u32,
    pub seq: u32,
    pub slot_id: u32,
    pub modifiers: u8,
    pub reserved0: u8,
    pub reserved1: u16,
    pub keys: [u8; 6],
    pub ascii: [u8; 6],
    pub flags: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosHidCursorEvent {
    pub t_ms: u32,
    pub seq: u32,
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub hid_kind: u8,
    pub reserved0: u8,
    pub reserved1: u16,
    pub buttons_down: u32,
    pub wheel: i16,
    pub reserved2: u16,
    pub x: f64,
    pub y: f64,
    pub flags: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi2WindowInfo {
    pub id: u32,
    pub kind: u32,
    pub state: u32,
    pub decoration_mode: u32,
    pub icon_id: u32,
    pub visible: u32,
    pub hit_test_visible: u32,
    pub selected: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub content_x: i32,
    pub content_y: i32,
    pub content_width: u32,
    pub content_height: u32,
    pub decoration_x: i32,
    pub decoration_y: i32,
    pub decoration_width: u32,
    pub decoration_height: u32,
}
