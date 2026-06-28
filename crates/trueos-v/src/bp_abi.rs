#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosCabiHeapStats {
    pub heap_start: usize,
    pub heap_end: usize,
    pub usable_total: usize,
    pub free_bytes: usize,
    pub largest_free_block: usize,
    pub free_blocks: usize,
    pub initialized: u32,
    pub source: u32,
}

unsafe extern "C" {
    pub fn trueos_cabi_poll_once();
    pub fn trueos_cabi_sleep_ms(ms: u64);
    pub fn trueos_cabi_thread_current_id() -> usize;
    pub fn trueos_time_monotonic_nanos() -> u64;
    pub fn trueos_time_unix_seconds() -> u64;
    pub fn trueos_time_unix_nanos() -> u64;
    pub fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    pub fn trueos_cabi_write_cstr(stream: u32, cstr: *const u8);
    pub fn trueos_cabi_alloc(size: usize) -> *mut u8;
    pub fn trueos_cabi_calloc(nmemb: usize, size: usize) -> *mut u8;
    pub fn trueos_cabi_free(ptr: *mut u8);
    pub fn trueos_cabi_realloc(ptr: *mut u8, size: usize) -> *mut u8;
    pub fn sys_alloc_aligned(size: usize, align: usize) -> *mut u8;
    pub fn sys_rand(recv_buf: *mut u32, words: usize);
    pub fn trueos_cabi_malloc_usable_size(ptr: *const u8) -> usize;
    pub fn trueos_cabi_heap_stats(out: *mut TrueosCabiHeapStats) -> i32;

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
    pub fn trueos_cabi_fs_create_dir_all(path_ptr: *const u8, path_len: usize) -> i32;
    pub fn trueos_cabi_fs_write_chunk(handle: u32, data_ptr: *const u8, data_len: usize) -> i32;
    pub fn trueos_cabi_fs_write_finish(handle: u32) -> i32;
    pub fn trueos_cabi_fs_write_abort(handle: u32) -> i32;
    pub fn trueos_cabi_fs_exists(path_ptr: *const u8, path_len: usize) -> i32;
    pub fn trueos_cabi_fs_stat(
        path_ptr: *const u8,
        path_len: usize,
        out_kind: *mut u32,
        out_len: *mut u64,
    ) -> i32;
    pub fn trueos_cabi_fs_list_dir(
        path_ptr: *const u8,
        path_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    pub fn trueos_cabi_trueosfs_primary_html_tree(
        max_entries: u32,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    pub fn trueos_cabi_trueosfs_json_all(
        max_entries: u32,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    pub fn trueos_cabi_fs_remove(path_ptr: *const u8, path_len: usize) -> i32;

    pub fn trueos_cabi_gfx_texture_dimensions(
        tex_id: u32,
        out_width: *mut u32,
        out_height: *mut u32,
    ) -> i32;
    pub fn trueos_cabi_gfx_texture_status(tex_id: u32) -> i32;
    pub fn trueos_cabi_gfx_upload_texture_rgba_image(
        tex_id: u32,
        width: u32,
        height: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;

    pub fn trueos_cabi_ui3_frame_create(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        tex_id: u32,
    ) -> u32;
    pub fn trueos_cabi_ui3_frame_close(frame_id: u32) -> i32;
    pub fn trueos_cabi_ui3_frame_request_repaint(frame_id: u32) -> i32;
    pub fn trueos_cabi_ui3_frame_set_position(frame_id: u32, x: i32, y: i32) -> i32;
    pub fn trueos_cabi_ui3_frame_set_size(frame_id: u32, width: u32, height: u32) -> i32;
    pub fn trueos_cabi_ui3_frame_begin(
        frame_id: u32,
        clear_rgb: u32,
        preserve_contents: u32,
        allow_present: u32,
    ) -> i32;
    pub fn trueos_cabi_ui3_frame_end(frame_id: u32) -> i32;
    pub fn trueos_cabi_ui3_frame_set_render_target(frame_id: u32, tex_id: u32) -> i32;
    pub fn trueos_cabi_ui3_frame_draw_solid_batch(
        frame_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;
    pub fn trueos_cabi_ui3_frame_draw_sprite_batch(
        frame_id: u32,
        tex_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;

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
    pub fn trueos_cabi_net_fetch_post_json_start_with_timeout(
        url_ptr: *const u8,
        url_len: usize,
        path_ptr: *const u8,
        path_len: usize,
        body_ptr: *const u8,
        body_len: usize,
        bearer_ptr: *const u8,
        bearer_len: usize,
        timeout_ms: u32,
    ) -> u32;
    pub fn trueos_cabi_net_fetch_post_json_bytes_start(
        url_ptr: *const u8,
        url_len: usize,
        body_ptr: *const u8,
        body_len: usize,
        bearer_ptr: *const u8,
        bearer_len: usize,
    ) -> u32;
    pub fn trueos_cabi_net_fetch_post_json_bytes_start_with_timeout(
        url_ptr: *const u8,
        url_len: usize,
        body_ptr: *const u8,
        body_len: usize,
        bearer_ptr: *const u8,
        bearer_len: usize,
        timeout_ms: u32,
    ) -> u32;
    pub fn trueos_cabi_net_fetch_result(op_id: u32) -> i32;
    pub fn trueos_cabi_net_fetch_bytes_result_len(op_id: u32) -> isize;
    pub fn trueos_cabi_net_fetch_bytes_read(op_id: u32, out_ptr: *mut u8, out_cap: usize) -> isize;
    pub fn trueos_cabi_net_fetch_discard(op_id: u32) -> i32;
    pub fn trueos_cabi_net_fetch_bytes_discard(op_id: u32) -> i32;
    pub fn trueos_cabi_net_fetch_wait(op_id: u32, timeout_ms: u64) -> i32;
    pub fn trueos_cabi_net_fetch_bytes_wait(op_id: u32, timeout_ms: u64) -> i32;

    pub fn trueos_cabi_socket_tcp_open(domain: i32, socket_type: i32, protocol: i32) -> i32;
    pub fn trueos_cabi_socket_tcp_close(socket_id: u32) -> i32;
    pub fn trueos_cabi_socket_tcp_set_nonblocking(socket_id: u32, nonblocking: u32) -> i32;
    pub fn trueos_cabi_socket_tcp_bind_v4(socket_id: u32, addr_be: u32, port_be: u16) -> i32;
    pub fn trueos_cabi_socket_tcp_bind_v6(socket_id: u32, addr_ptr: *const u8, port_be: u16)
    -> i32;
    pub fn trueos_cabi_socket_tcp_connect_v4(
        socket_id: u32,
        addr_be: u32,
        port_be: u16,
        nonblocking: u32,
    ) -> i32;
    pub fn trueos_cabi_socket_tcp_connect_v6(
        socket_id: u32,
        addr_ptr: *const u8,
        port_be: u16,
        nonblocking: u32,
    ) -> i32;
    pub fn trueos_cabi_socket_tcp_poll_connect(socket_id: u32, timeout_ms: u64) -> i32;
    pub fn trueos_cabi_socket_tcp_send(
        socket_id: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> isize;
    pub fn trueos_cabi_socket_tcp_recv(
        socket_id: u32,
        out_ptr: *mut u8,
        out_cap: usize,
        flags: i32,
        nonblocking: u32,
        timeout_ms: u64,
    ) -> isize;
    pub fn trueos_cabi_socket_tcp_shutdown(socket_id: u32, how: u32) -> i32;
    pub fn trueos_cabi_socket_tcp_take_error(socket_id: u32) -> i32;
    pub fn trueos_cabi_socket_tcp_peer_v4(
        socket_id: u32,
        out_addr_be: *mut u32,
        out_port_be: *mut u16,
    ) -> i32;
    pub fn trueos_cabi_socket_tcp_peer_v6(
        socket_id: u32,
        out_addr_ptr: *mut u8,
        out_port_be: *mut u16,
    ) -> i32;

    pub fn trueos_cabi_smtp_send_text_start(
        to_ptr: *const u8,
        to_len: usize,
        subject_ptr: *const u8,
        subject_len: usize,
        body_ptr: *const u8,
        body_len: usize,
        timeout_ms: u32,
    ) -> u32;
    pub fn trueos_cabi_smtp_result(op_id: u32) -> i32;
    pub fn trueos_cabi_smtp_wait(op_id: u32, timeout_ms: u64) -> i32;
    pub fn trueos_cabi_smtp_discard(op_id: u32) -> i32;
    pub fn trueos_cabi_smtp_configure_account(
        user_ptr: *const u8,
        user_len: usize,
        pass_ptr: *const u8,
        pass_len: usize,
        from_ptr: *const u8,
        from_len: usize,
    ) -> i32;
    pub fn trueos_cabi_smtp_password_configured() -> i32;

    pub fn trueos_cabi_audio_open_playback(
        format: u32,
        channels: u32,
        rate_hz: u32,
        out_handle: *mut u32,
    ) -> i32;
    pub fn trueos_cabi_audio_close(handle: u32) -> i32;
    pub fn trueos_cabi_audio_start(handle: u32) -> i32;
    pub fn trueos_cabi_audio_drop(handle: u32) -> i32;
    pub fn trueos_cabi_audio_drain(handle: u32, timeout_ms: u64) -> i32;
    pub fn trueos_cabi_audio_write_i16_interleaved(
        handle: u32,
        samples_ptr: *const i16,
        sample_count: usize,
    ) -> isize;
    pub fn trueos_cabi_audio_write_i16_stereo_48k(
        samples_ptr: *const i16,
        sample_count: usize,
    ) -> isize;
    pub fn trueos_cabi_audio_queued_frames(handle: u32) -> isize;
    pub fn trueos_cabi_audio_buffer_frames(handle: u32) -> isize;
    pub fn trueos_cabi_audio_state(handle: u32) -> i32;

    pub fn trueos_cabi_input_pop_mouse(
        out_buttons: *mut u8,
        out_dx: *mut i8,
        out_dy: *mut i8,
        out_wheel: *mut i8,
    ) -> i32;
    pub fn trueos_cabi_input_pop_tablet(out: *mut TrueosTabletEvent) -> i32;
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
    pub fn trueos_cabi_hid_mouse_read(
        controller_id: u32,
        slot_id: u32,
        ep_target: u32,
        out: *mut TrueosHidMouseSample,
        out_cap: u32,
        out_dropped: *mut u32,
    ) -> u32;
    pub fn trueos_cabi_hid_tablet_read(
        controller_id: u32,
        slot_id: u32,
        ep_target: u32,
        out: *mut TrueosHidTabletSample,
        out_cap: u32,
        out_dropped: *mut u32,
    ) -> u32;
    pub fn trueos_cabi_hid_hut_upsert_combo(
        combo_id: u32,
        source_kind: u8,
        source_tag_ptr: *const u8,
        source_tag_len: usize,
    ) -> i32;
    pub fn trueos_cabi_hid_hut_bind_combo_mouse(
        combo_id: u32,
        controller_id: u32,
        slot_id: u32,
        ep_target: u32,
    ) -> i32;
    pub fn trueos_cabi_hid_hut_bind_combo_keyboard(
        combo_id: u32,
        controller_id: u32,
        slot_id: u32,
        ep_target: u32,
    ) -> i32;
    pub fn trueos_cabi_hid_hut_bind_combo_tablet(
        combo_id: u32,
        controller_id: u32,
        slot_id: u32,
        ep_target: u32,
    ) -> i32;
    pub fn trueos_cabi_hid_hut_read_mice(out: *mut TrueosHidHutMouseState, out_cap: u32) -> u32;
    pub fn trueos_cabi_hid_hut_read_tablets(out: *mut TrueosHidHutTabletState, out_cap: u32)
    -> u32;
    pub fn trueos_cabi_hid_hut_read_keyboards(
        out: *mut TrueosHidHutKeyboardState,
        out_cap: u32,
    ) -> u32;
    pub fn trueos_cabi_hid_hut_read_combos(out: *mut TrueosHidHutCombo, out_cap: u32) -> u32;
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
    pub fn trueos_cabi_input_pop_keyboard_output(out: *mut TrueosKeyboardOutputEvent) -> i32;
    pub fn trueos_cabi_input_read_keyboard_output_since(
        read_seq: u64,
        out: *mut TrueosKeyboardOutputEvent,
        out_cap: u32,
        out_next_seq: *mut u64,
        out_dropped: *mut u32,
    ) -> u32;
    pub fn trueos_cabi_mouse_poll(out: *mut TrueosMouseState) -> i32;
    pub fn trueos_cabi_qjs_mouse_pop(out: *mut TrueosMouseState) -> i32;

    pub fn trueos_cabi_uart1_shell_write(data_ptr: *const u8, data_len: usize) -> usize;
    pub fn trueos_cabi_env_args_count() -> usize;
    pub fn trueos_cabi_env_arg(index: usize, out_ptr: *mut u8, out_cap: usize) -> isize;
    pub fn trueos_cabi_env_var(
        key_ptr: *const u8,
        key_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    pub fn trueos_cabi_shell2_print_line(data_ptr: *const u8, data_len: usize) -> usize;
    pub fn trueos_cabi_shell1_submit_input(data_ptr: *const u8, data_len: usize) -> usize;
    pub fn trueos_cabi_shell_attached_write(data_ptr: *const u8, data_len: usize) -> usize;
    pub fn trueos_cabi_shell_attached_read_byte() -> i32;
    pub fn trueos_cabi_shell_attached_retarget_slot(slot_ptr: *const u8, slot_len: usize) -> i32;
    pub fn trueos_cabi_shell_command_registry_json(out_ptr: *mut u8, out_cap: usize) -> isize;
    pub fn trueos_cabi_shell_history_lines_all() -> usize;
    pub fn trueos_cabi_shell_history_lines(
        start_line: usize,
        max_lines: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    pub fn trueos_cabi_ntp_current_unix_seconds() -> u64;
    pub fn trueos_cabi_ntp_kernel_date_day_month_year(out_ptr: *mut u8, out_cap: usize) -> usize;
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
pub struct TrueosTabletEvent {
    pub slot_id: u32,
    pub buttons: u8,
    pub report_id: u8,
    pub x_raw: u16,
    pub y_raw: u16,
    pub x_norm_q15: u16,
    pub y_norm_q15: u16,
    pub flags: u8,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosHidMouseSample {
    pub t_ms: u32,
    pub seq: u32,
    pub slot_id: u32,
    pub buttons: u8,
    pub dx: i8,
    pub dy: i8,
    pub wheel: i8,
    pub flags: u8,
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
pub struct TrueosHidTabletSample {
    pub t_ms: u32,
    pub seq: u32,
    pub slot_id: u32,
    pub buttons: u8,
    pub report_id: u8,
    pub flags: u8,
    pub reserved0: u8,
    pub x_raw: u16,
    pub y_raw: u16,
    pub x_norm_q15: u16,
    pub y_norm_q15: u16,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosKeyboardOutputEvent {
    pub t_ms: u32,
    pub seq: u32,
    pub device_seq: u32,
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub modifiers: u8,
    pub kind: u8,
    pub utf8_len: u8,
    pub reserved0: u8,
    pub key_code: u16,
    pub reserved1: u16,
    pub codepoint: u32,
    pub utf8: [u8; 4],
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
pub struct TrueosHidHutMouseState {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub buttons_down: u32,
    pub combo_id: u32,
    pub source_kind: u8,
    pub virtual_device: u8,
    pub source_tag_len: u8,
    pub reserved0: u8,
    pub source_tag: [u8; 32],
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosHidHutTabletState {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub x_raw: u16,
    pub y_raw: u16,
    pub buttons_down: u32,
    pub report_id: u8,
    pub source_kind: u8,
    pub virtual_device: u8,
    pub source_tag_len: u8,
    pub combo_id: u32,
    pub source_tag: [u8; 32],
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosHidHutKeyboardState {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub combo_id: u32,
    pub modifiers: u8,
    pub source_kind: u8,
    pub virtual_device: u8,
    pub source_tag_len: u8,
    pub keys: [u8; 6],
    pub ascii: [u8; 6],
    pub key_down_bits: [u32; 8],
    pub source_tag: [u8; 32],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosHidHutCombo {
    pub combo_id: u32,
    pub source_kind: u8,
    pub source_tag_len: u8,
    pub reserved0: u16,
    pub source_tag: [u8; 32],
    pub mouse_controller_id: u32,
    pub mouse_slot_id: u32,
    pub mouse_ep_target: u32,
    pub keyboard_controller_id: u32,
    pub keyboard_slot_id: u32,
    pub keyboard_ep_target: u32,
    pub tablet_controller_id: u32,
    pub tablet_slot_id: u32,
    pub tablet_ep_target: u32,
}
