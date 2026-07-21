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

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi4SolaraFontSize {
    pub native_scale: u32,
    pub target_pixels: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TrueosUi4SolaraTextRow {
    pub text_ptr: *const u8,
    pub text_len: usize,
    pub x: f32,
    pub y: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct TrueosUi4SolaraSceneTextRow {
    pub text_ptr: *const u8,
    pub text_len: usize,
    pub x: f32,
    pub y: f32,
    pub font_pixels: f32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi4PanEvent {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub hid_kind: u32,
    pub phase: u32,
    pub x: u32,
    pub y: u32,
    pub local_x: i32,
    pub local_y: i32,
    pub dx: i32,
    pub dy: i32,
    pub combo_id: u32,
    pub vcursor: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi4KeyboardState {
    pub controller_id: u32,
    pub slot_id: u32,
    pub ep_target: u32,
    pub combo_id: u32,
    pub modifiers: u8,
    pub source_kind: u8,
    pub virtual_keyboard: u8,
    pub reserved0: u8,
    pub keys: [u8; 6],
    pub ascii: [u8; 6],
    pub key_down_bits: [u32; 8],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi4SkyboxRenderParams {
    pub right_x: f32,
    pub right_y: f32,
    pub right_z: f32,
    pub up_x: f32,
    pub up_y: f32,
    pub up_z: f32,
    pub forward_x: f32,
    pub forward_y: f32,
    pub forward_z: f32,
    pub aspect_tan_half_fov_y: f32,
    pub tan_half_fov_y: f32,
    pub rect_x: u32,
    pub rect_y: u32,
    pub rect_width: u32,
    pub rect_height: u32,
}

unsafe extern "C" {
    pub fn trueos_cabi_gridpaper_snapshot_submit(
        generation: u64,
        scale_percent: u32,
        raw_ptr: *const u8,
        raw_len: usize,
    ) -> i32;
    pub fn trueos_cabi_gridpaper_snapshot_submit_instance(
        instance_id: u32,
        generation: u64,
        scale_percent: u32,
        raw_ptr: *const u8,
        raw_len: usize,
    ) -> i32;
    pub fn trueos_cabi_gridpaper_text_animations_submit(raw_ptr: *const u8, raw_len: usize) -> i32;
    pub fn trueos_cabi_gridpaper_text_animations_submit_instance(
        instance_id: u32,
        raw_ptr: *const u8,
        raw_len: usize,
    ) -> i32;
    pub fn trueos_cabi_gridpaper_close() -> i32;
    pub fn trueos_cabi_gridpaper_close_instance(instance_id: u32) -> i32;
    pub fn trueos_cabi_gridpaper_print_request_take() -> u64;
    pub fn trueos_cabi_gridpaper_print_request_take_instance(instance_id: u32) -> u64;

    pub fn trueos_cabi_ui4_solara_font_sizes(
        out: *mut TrueosUi4SolaraFontSize,
        out_cap: usize,
    ) -> isize;
    pub fn trueos_cabi_ui4_solara_frame_open(x: i32, y: i32, width: u32, height: u32) -> u32;
    pub fn trueos_cabi_ui4_scene_frame_open_immutable(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> u32;
    pub fn trueos_cabi_ui4_scene_frame_open_streaming(
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> u32;
    pub fn trueos_cabi_ui4_solara_frame_begin(window_id: u32, clear_rgba: u32) -> i32;
    pub fn trueos_cabi_ui4_scene_pan_event_take(window_id: u32, out: *mut TrueosUi4PanEvent)
    -> i32;
    pub fn trueos_cabi_ui4_scene_keyboard_state(
        window_id: u32,
        out: *mut TrueosUi4KeyboardState,
    ) -> i32;
    pub fn trueos_cabi_ui4_solara_text_rows(
        window_id: u32,
        font_id: u32,
        native_scale: u32,
        dst_x: i32,
        dst_y: i32,
        rgba: u32,
        rows: *const TrueosUi4SolaraTextRow,
        row_count: usize,
    ) -> i32;
    pub fn trueos_cabi_ui4_solara_text_scene(
        window_id: u32,
        font_id: u32,
        viewport_width: u32,
        viewport_height: u32,
        rgba: u32,
        rows: *const TrueosUi4SolaraSceneTextRow,
        row_count: usize,
    ) -> i32;
    pub fn trueos_cabi_ui4_solara_frame_publish(
        window_id: u32,
        damage_x: u32,
        damage_y: u32,
        damage_width: u32,
        damage_height: u32,
    ) -> i32;
    pub fn trueos_cabi_ui4_solara_frame_close(window_id: u32) -> i32;
    pub fn trueos_cabi_ui4_solara_frame_close_requested(window_id: u32, flags: u32) -> i32;
    pub fn trueos_cabi_ui4_scene_frame_set_position(window_id: u32, x: i32, y: i32) -> i32;
    pub fn trueos_cabi_ui4_scene_frame_resize(window_id: u32, width: u32, height: u32) -> i32;
    pub fn trueos_cabi_ui4_scene_frame_write_opaque_rgba8(
        window_id: u32,
        rgba_ptr: *const u8,
        rgba_len: usize,
    ) -> i32;
    pub fn trueos_cabi_ui4_scene_skybox_upload_rgb565(
        window_id: u32,
        width: u32,
        height: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;
    pub fn trueos_cabi_ui4_scene_skybox_render_rgb565(
        window_id: u32,
        params: *const TrueosUi4SkyboxRenderParams,
    ) -> i32;

    pub fn trueos_cabi_poll_once();
    pub fn trueos_cabi_sleep_ms(ms: u64);
    pub fn trueos_cabi_thread_current_id() -> usize;
    pub fn trueos_cabi_wls_current_slot() -> u32;
    pub fn trueos_time_monotonic_nanos() -> u64;
    pub fn trueos_time_unix_seconds() -> u64;
    pub fn trueos_time_unix_nanos() -> u64;
    pub fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    pub fn trueos_cabi_write_cstr(stream: u32, cstr: *const u8);
    pub fn trueos_cabi_log(
        level: u32,
        target_ptr: *const u8,
        target_len: usize,
        message_ptr: *const u8,
        message_len: usize,
    ) -> i32;
    pub fn trueos_cabi_alloc(size: usize) -> *mut u8;
    pub fn trueos_cabi_calloc(nmemb: usize, size: usize) -> *mut u8;
    pub fn trueos_cabi_free(ptr: *mut u8);
    pub fn trueos_cabi_realloc(ptr: *mut u8, size: usize) -> *mut u8;
    pub fn sys_alloc_aligned(size: usize, align: usize) -> *mut u8;
    pub fn sys_rand(recv_buf: *mut u32, words: usize);
    pub fn trueos_cabi_malloc_usable_size(ptr: *const u8) -> usize;
    pub fn trueos_cabi_heap_stats(out: *mut TrueosCabiHeapStats) -> i32;
    pub fn trueos_cabi_calculator_evaluate(
        operation: u32,
        arguments: *const f64,
        argument_count: usize,
        out_value: *mut f64,
    ) -> i32;

    pub fn trueos_cabi_async_fs_read_start(path_ptr: *const u8, path_len: usize) -> i32;
    pub fn trueos_cabi_async_fs_write_begin(
        path_ptr: *const u8,
        path_len: usize,
        total_len: usize,
    ) -> i32;
    pub fn trueos_cabi_async_fs_write_chunk(
        id: u32,
        offset: usize,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;
    pub fn trueos_cabi_async_fs_write_commit(id: u32) -> i32;
    pub fn trueos_cabi_async_fs_create_dir_all_start(path_ptr: *const u8, path_len: usize) -> i32;
    pub fn trueos_cabi_async_fs_stat_start(path_ptr: *const u8, path_len: usize) -> i32;
    pub fn trueos_cabi_async_fs_list_dir_start(path_ptr: *const u8, path_len: usize) -> i32;
    pub fn trueos_cabi_async_fs_remove_start(path_ptr: *const u8, path_len: usize) -> i32;
    pub fn trueos_cabi_async_fs_status(id: u32) -> i32;
    pub fn trueos_cabi_async_fs_result_len(id: u32) -> isize;
    pub fn trueos_cabi_async_fs_result_read(
        id: u32,
        offset: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    pub fn trueos_cabi_async_fs_discard(id: u32) -> i32;

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
    pub fn trueos_cabi_gfx_upload_skybox_rgb565(
        skybox_id: u32,
        width: u32,
        height: u32,
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
    pub fn trueos_cabi_dns_resolve_ipv4(
        host: *const u8,
        host_len: usize,
        out_octets: *mut u8,
    ) -> i32;

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
    pub fn trueos_cabi_tun_open(
        ipv4_be: u32,
        ipv4_prefix_len: u32,
        ipv6_ptr: *const u8,
        ipv6_prefix_len: u32,
        mtu: u32,
    ) -> i32;
    pub fn trueos_cabi_tun_close(tun_id: u32) -> i32;
    pub fn trueos_cabi_tun_send(tun_id: u32, data_ptr: *const u8, data_len: usize) -> isize;
    pub fn trueos_cabi_tun_recv(tun_id: u32, out_ptr: *mut u8, out_cap: usize) -> isize;
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
    pub fn trueos_cabi_audio_set_paused(handle: u32, paused: u32) -> i32;
    pub fn trueos_cabi_audio_paused(handle: u32) -> i32;
    pub fn trueos_cabi_audio_set_volume_percent(handle: u32, percent: u32) -> i32;
    pub fn trueos_cabi_audio_volume_percent(handle: u32) -> i32;
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
    pub fn trueos_cabi_audio_monitor_start_cursor(preroll_samples: usize) -> u64;
    pub fn trueos_cabi_audio_monitor_read_i16_since(
        cursor: u64,
        out_ptr: *mut i16,
        out_cap: usize,
        out_next_cursor: *mut u64,
    ) -> isize;

    pub fn trueos_cabi_vgpu_open(requested_caps: u64, out_device: *mut u64) -> i32;
    pub fn trueos_cabi_vgpu_close(device: u64) -> i32;
    pub fn trueos_cabi_vgpu_device_info(device: u64, out_info: *mut crate::vgpu::DeviceInfo)
    -> i32;
    pub fn trueos_cabi_vgpu_buffer_create(
        device: u64,
        bytes: usize,
        usage: u32,
        out_buffer: *mut u64,
    ) -> i32;
    pub fn trueos_cabi_vgpu_buffer_destroy(device: u64, buffer: u64) -> i32;
    pub fn trueos_cabi_vgpu_buffer_write(
        device: u64,
        buffer: u64,
        offset: usize,
        data: *const u8,
        data_len: usize,
    ) -> isize;
    pub fn trueos_cabi_vgpu_buffer_read(
        device: u64,
        buffer: u64,
        offset: usize,
        out: *mut u8,
        out_len: usize,
    ) -> isize;
    pub fn trueos_cabi_vgpu_buffer_info(
        device: u64,
        buffer: u64,
        out_info: *mut crate::vgpu::BufferInfo,
    ) -> i32;
    pub fn trueos_cabi_vgpu_queue_create(device: u64, class: u32, out_queue: *mut u64) -> i32;
    pub fn trueos_cabi_vgpu_queue_destroy(device: u64, queue: u64) -> i32;
    pub fn trueos_cabi_vgpu_submit_control_nop(
        device: u64,
        queue: u64,
        out_point: *mut crate::vgpu::TimelinePoint,
    ) -> i32;
    pub fn trueos_cabi_vgpu_timeline(
        device: u64,
        queue: u64,
        out_status: *mut crate::vgpu::TimelineStatus,
    ) -> i32;
    pub fn trueos_cabi_vgpu_wait(device: u64, queue: u64, value: u64) -> i32;

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
    pub fn trueos_cabi_mouse_motion_cursor_request(
        label_ptr: *const u8,
        label_len: usize,
        out_cursor: *mut MouseMotionCursorInfo,
    ) -> i32;
    pub fn trueos_cabi_mouse_motion_cursor_release(handle: u64) -> i32;
    pub fn trueos_cabi_mouse_motion_submit(handle: u64, command: *const MouseMotionCommand) -> i32;
    pub fn trueos_cabi_mouse_motion_submit_json(
        handle: u64,
        json_ptr: *const u8,
        json_len: usize,
    ) -> i32;
    pub fn trueos_cabi_mouse_motion_cursor_idle(handle: u64) -> i32;
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
    pub fn trueos_cabi_shell2_raw_write(data_ptr: *const u8, data_len: usize) -> usize;
    pub fn trueos_cabi_qjs_workbench_eval_v1(
        source_ptr: *const u8,
        source_len: usize,
        mode: u32,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    pub fn trueos_cabi_qjs_workbench_poll_v1(out_ptr: *mut u8, out_cap: usize) -> isize;
    pub fn trueos_cabi_qjs_workbench_close_v1() -> i32;
    pub fn trueos_cabi_blueprint_exit_reason(data_ptr: *const u8, data_len: usize) -> i32;
    pub fn trueos_cabi_blueprint_shutdown(data_ptr: *const u8, data_len: usize) -> i32;
    pub fn trueos_cabi_blueprint_return_to_cli() -> i32;
    pub fn trueos_cabi_konsole_size(out_cols: *mut u32, out_rows: *mut u32) -> i32;
    pub fn trueos_cabi_konsole_begin_frame(cols: u32, rows: u32, reserved_top_rows: u32) -> i32;
    pub fn trueos_cabi_konsole_write_row(
        row: u32,
        col: u32,
        data_ptr: *const u8,
        data_len: usize,
    ) -> i32;
    pub fn trueos_cabi_konsole_set_cursor(row: u32, col: u32, visible: u32) -> i32;
    pub fn trueos_cabi_konsole_end_frame() -> i32;
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
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct MouseMotionCursorInfo {
    pub handle: u64,
    pub slot_id: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct MouseMotionCommand {
    pub opcode: u8,
    pub path: u8,
    pub easing: u8,
    pub flags: u8,
    pub duration_ms: u32,
    pub x: i32,
    pub y: i32,
    pub control1_x: i32,
    pub control1_y: i32,
    pub control2_x: i32,
    pub control2_y: i32,
    pub buttons_set: u32,
    pub buttons_clear: u32,
    pub wheel: i16,
    pub reserved: u16,
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
