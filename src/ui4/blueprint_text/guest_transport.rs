//! Guest-side UI4 VM-call marshalling and synchronous retry policy.
//! Host surface ownership and GPU retirement stay in the parent module.

use alloc::vec::Vec;

use super::{
    BlueprintImageSourceInfo, CONTEXT_MENU_ENTRY_WIRE_HEADER_BYTES, CONTEXT_MENU_WIRE_HEADER_BYTES, ERROR_BUSY, ERROR_INVALID, ERROR_UI4, FONT_CANVAS_ROW_WIRE_HEADER_BYTES, FONT_CANVAS_WIRE_HEADER_BYTES, GUEST_TEXT_SCENE_BUSY_POLL_MS, IMAGE_SOURCE_READ_CHUNK_BYTES, MAX_CONTEXT_MENU_LABEL_BYTES, MAX_FONT_CANVAS_ROWS, MAX_INPUT_ROUTES, MAX_NATIVE_FONT_SIZES, MAX_TEXT_ROWS, MAX_TEXT_ROW_BYTES, SHELL2_FONT_SCALE_STEP_COUNT, TEXT_SCENE_ROW_WIRE_HEADER_BYTES, TEXT_SCENE_WIRE_HEADER_BYTES, TrueosUi4ContextMenuEntry, TrueosUi4ContextMenuEvent, TrueosUi4FontCanvasRow, TrueosUi4FontSpriteStatusV1, TrueosUi4InputRouteState, TrueosUi4KeyboardState, TrueosUi4PanEvent, TrueosUi4PointerEvent, TrueosUi4ResizeEvent, TrueosUi4Shell2FontScaleStep, TrueosUi4SolaraFontSize, TrueosUi4SolaraSceneTextRow, fn,
};

pub(super) unsafe fn guest_font_sizes(out: *mut TrueosUi4SolaraFontSize, out_cap: usize) -> isize {
    let response_cap = out_cap.min(MAX_NATIVE_FONT_SIZES);
    let response_bytes =
        response_cap.saturating_mul(core::mem::size_of::<TrueosUi4SolaraFontSize>());
    let mut response = alloc::vec![0u8; response_bytes];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SOLARA_FONT_SIZES,
        response_cap as u64,
        0,
        &[],
        response.as_mut_slice(),
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4 as isize;
    }
    let result = data as i64;
    if result < 0 {
        return result as isize;
    }
    let count = result as usize;
    let copied_entries = count.min(response_cap);
    let copied_bytes = copied_entries * core::mem::size_of::<TrueosUi4SolaraFontSize>();
    if copied_bytes != 0 {
        // SAFETY: the caller supplied capacity for response_cap entries and
        // call_with_payload initialized the copied response bytes.
        unsafe {
            core::ptr::copy_nonoverlapping(response.as_ptr(), out.cast::<u8>(), copied_bytes);
        }
    }
    count as isize
}

pub(super) unsafe fn guest_shell2_font_scale_steps(
    out: *mut TrueosUi4Shell2FontScaleStep,
    out_cap: usize,
) -> isize {
    let response_cap = out_cap.min(SHELL2_FONT_SCALE_STEP_COUNT);
    let entry_bytes = core::mem::size_of::<TrueosUi4Shell2FontScaleStep>();
    let mut response = alloc::vec![0u8; response_cap.saturating_mul(entry_bytes)];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SHELL2_FONT_SCALE_STEPS_V1,
        response_cap as u64,
        0,
        &[],
        response.as_mut_slice(),
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4 as isize;
    }
    let count = data as i64;
    if count < 0 {
        return count as isize;
    }
    let copied_entries = (count as usize).min(response_cap);
    let copied_bytes = copied_entries.saturating_mul(entry_bytes);
    if copied_bytes != 0 {
        // SAFETY: `out` has `response_cap` entries and the VM call filled the
        // response prefix represented by `copied_entries`.
        unsafe {
            core::ptr::copy_nonoverlapping(response.as_ptr(), out.cast::<u8>(), copied_bytes)
        };
    }
    count as isize
}

pub(super) unsafe fn guest_image_source_info(
    name_ptr: *const u8,
    name_len: usize,
    out: *mut BlueprintImageSourceInfo,
) -> i32 {
    let name = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };
    let mut response = [0u8; core::mem::size_of::<BlueprintImageSourceInfo>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_IMAGE_SOURCE_INFO,
        0,
        0,
        name,
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    unsafe { out.write(core::ptr::read_unaligned(response.as_ptr().cast())) };
    0
}

pub(super) unsafe fn guest_image_source_read(
    name_ptr: *const u8,
    name_len: usize,
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if out_cap > IMAGE_SOURCE_READ_CHUNK_BYTES {
        return ERROR_INVALID as isize;
    }
    let name = unsafe { core::slice::from_raw_parts(name_ptr, name_len) };
    let mut response = alloc::vec![0u8; out_cap];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_IMAGE_SOURCE_READ,
        offset as u64,
        out_cap as u64,
        name,
        response.as_mut_slice(),
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4 as isize;
    }
    let copied = data as usize;
    if copied > out_cap {
        return ERROR_UI4 as isize;
    }
    unsafe { core::ptr::copy_nonoverlapping(response.as_ptr(), out_ptr, copied) };
    copied as isize
}

pub(super) unsafe fn guest_context_menu_register(
    window_id: u32,
    entries: &[TrueosUi4ContextMenuEntry],
) -> i32 {
    let mut payload = Vec::with_capacity(
        CONTEXT_MENU_WIRE_HEADER_BYTES.saturating_add(
            entries
                .len()
                .saturating_mul(CONTEXT_MENU_ENTRY_WIRE_HEADER_BYTES + 16),
        ),
    );
    payload.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for entry in entries {
        if entry.label_ptr.is_null()
            || entry.label_len == 0
            || entry.label_len > MAX_CONTEXT_MENU_LABEL_BYTES
        {
            return ERROR_INVALID;
        }
        let Some(required) = payload
            .len()
            .checked_add(CONTEXT_MENU_ENTRY_WIRE_HEADER_BYTES)
            .and_then(|bytes| bytes.checked_add(entry.label_len))
        else {
            return ERROR_INVALID;
        };
        if required > trueos_vm::vmcall::PAYLOAD_CAP {
            return ERROR_INVALID;
        }
        // SAFETY: the C ABI requires `label_len` readable bytes per entry.
        let label = unsafe { core::slice::from_raw_parts(entry.label_ptr, entry.label_len) };
        payload.extend_from_slice(&entry.action_id.to_le_bytes());
        payload.extend_from_slice(&entry.enabled.to_le_bytes());
        payload.extend_from_slice(&(entry.label_len as u32).to_le_bytes());
        payload.extend_from_slice(label);
    }
    guest_status(
        trueos_vm::vmcall::OP_BP_UI4_CONTEXT_MENU_REGISTER,
        window_id as u64,
        0,
        payload.as_slice(),
    )
}

pub(super) unsafe fn guest_context_menu_event_take(
    window_id: u32,
    out: *mut TrueosUi4ContextMenuEvent,
) -> i32 {
    let mut response = [0u8; core::mem::size_of::<TrueosUi4ContextMenuEvent>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_CONTEXT_MENU_EVENT_TAKE,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let event = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(event) };
    0
}

pub(super) unsafe fn guest_pointer_event_take(window_id: u32, out: *mut TrueosUi4PointerEvent) -> i32 {
    let mut response = [0u8; core::mem::size_of::<TrueosUi4PointerEvent>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_POINTER_EVENT_TAKE,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let event = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(event) };
    0
}

pub(super) unsafe fn guest_keyboard_event_take(
    window_id: u32,
    out: *mut crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> i32 {
    let mut response = [0u8; core::mem::size_of::<crate::r::keyboard::TrueosKeyboardOutputEvent>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_KEYBOARD_EVENT_TAKE,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let event = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(event) };
    0
}

pub(super) unsafe fn guest_pan_event_take(window_id: u32, out: *mut TrueosUi4PanEvent) -> i32 {
    let mut response = [0u8; core::mem::size_of::<TrueosUi4PanEvent>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_PAN_EVENT_TAKE,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let event = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(event) };
    0
}

pub(super) unsafe fn guest_resize_event_take(window_id: u32, out: *mut TrueosUi4ResizeEvent) -> i32 {
    let mut response = [0u8; core::mem::size_of::<TrueosUi4ResizeEvent>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_RESIZE_EVENT_TAKE,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let event = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(event) };
    0
}

pub(super) unsafe fn guest_keyboard_state(window_id: u32, out: *mut TrueosUi4KeyboardState) -> i32 {
    let mut response = [0u8; core::mem::size_of::<TrueosUi4KeyboardState>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_KEYBOARD_STATE,
        window_id as u64,
        0,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64 as i32;
    if result != 0 {
        return result;
    }
    let state = unsafe { core::ptr::read_unaligned(response.as_ptr().cast()) };
    unsafe { out.write(state) };
    0
}

pub(super) unsafe fn guest_input_routes(
    window_id: u32,
    out: *mut TrueosUi4InputRouteState,
    out_cap: u32,
) -> isize {
    let response_cap = (out_cap as usize).min(MAX_INPUT_ROUTES);
    let mut response =
        alloc::vec![0u8; response_cap * core::mem::size_of::<TrueosUi4InputRouteState>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_INPUT_ROUTES,
        window_id as u64,
        response_cap as u64,
        &[],
        response.as_mut_slice(),
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4 as isize;
    }
    let result = data as i64;
    if result < 0 {
        return result as isize;
    }
    let copied = (result as usize).min(response_cap);
    if copied != 0 {
        // SAFETY: out_cap covers response_cap records and the vmcall copied
        // exactly the initialized response bytes for `copied` records.
        unsafe {
            core::ptr::copy_nonoverlapping(
                response.as_ptr(),
                out.cast::<u8>(),
                copied * core::mem::size_of::<TrueosUi4InputRouteState>(),
            );
        }
    }
    result as isize
}

pub(super) unsafe fn guest_text_scene(
    window_id: u32,
    font_id: u32,
    viewport_width: u32,
    viewport_height: u32,
    rgba: u32,
    rows: *const TrueosUi4SolaraSceneTextRow,
    row_count: usize,
) -> i32 {
    if rows.is_null() || row_count == 0 || row_count > MAX_TEXT_ROWS {
        return ERROR_INVALID;
    }
    // SAFETY: the Blueprint ABI promises row_count readable row descriptors.
    let input = unsafe { core::slice::from_raw_parts(rows, row_count) };
    let mut payload = Vec::with_capacity(
        TEXT_SCENE_WIRE_HEADER_BYTES
            .saturating_add(row_count.saturating_mul(TEXT_SCENE_ROW_WIRE_HEADER_BYTES + 32)),
    );
    payload.extend_from_slice(&viewport_width.to_le_bytes());
    payload.extend_from_slice(&viewport_height.to_le_bytes());
    payload.extend_from_slice(&rgba.to_le_bytes());
    payload.extend_from_slice(&(row_count as u32).to_le_bytes());
    for row in input {
        if row.text_ptr.is_null() || row.text_len == 0 || row.text_len > MAX_TEXT_ROW_BYTES {
            return ERROR_INVALID;
        }
        let Some(required) = payload
            .len()
            .checked_add(TEXT_SCENE_ROW_WIRE_HEADER_BYTES)
            .and_then(|bytes| bytes.checked_add(row.text_len))
        else {
            return ERROR_INVALID;
        };
        if required > trueos_vm::vmcall::PAYLOAD_CAP {
            return ERROR_INVALID;
        }
        // SAFETY: each ABI row promises text_len readable bytes.
        let text = unsafe { core::slice::from_raw_parts(row.text_ptr, row.text_len) };
        payload.extend_from_slice(&row.x.to_bits().to_le_bytes());
        payload.extend_from_slice(&row.y.to_bits().to_le_bytes());
        payload.extend_from_slice(&row.font_pixels.to_bits().to_le_bytes());
        payload.extend_from_slice(&(row.text_len as u32).to_le_bytes());
        payload.extend_from_slice(text);
    }
    loop {
        let result = guest_status(
            trueos_vm::vmcall::OP_BP_UI4_SOLARA_TEXT_SCENE,
            window_id as u64,
            font_id as u64,
            payload.as_slice(),
        );
        if result != ERROR_BUSY {
            return result;
        }
        // The host has transferred owned strings to the Embassy FontKernel
        // service. Pace the synchronous ABI poll so a pending GPU ticket
        // cannot turn into a VM-exit and serial-log storm.
        trueos_vm::vmcall::sleep_ms(GUEST_TEXT_SCENE_BUSY_POLL_MS);
    }
}

pub(super) unsafe fn guest_font_canvas(
    window_id: u32,
    font_id: u32,
    canvas_width: u32,
    canvas_height: u32,
    rows: *const TrueosUi4FontCanvasRow,
    row_count: usize,
) -> i32 {
    if rows.is_null() || row_count == 0 || row_count > MAX_FONT_CANVAS_ROWS {
        return ERROR_INVALID;
    }
    let input = unsafe { core::slice::from_raw_parts(rows, row_count) };
    let mut payload = Vec::with_capacity(
        FONT_CANVAS_WIRE_HEADER_BYTES
            .saturating_add(row_count.saturating_mul(FONT_CANVAS_ROW_WIRE_HEADER_BYTES + 32)),
    );
    payload.extend_from_slice(&canvas_width.to_le_bytes());
    payload.extend_from_slice(&canvas_height.to_le_bytes());
    payload.extend_from_slice(&(row_count as u32).to_le_bytes());
    for row in input {
        if row.text_ptr.is_null() || row.text_len == 0 || row.text_len > MAX_TEXT_ROW_BYTES {
            return ERROR_INVALID;
        }
        let Some(required) = payload
            .len()
            .checked_add(FONT_CANVAS_ROW_WIRE_HEADER_BYTES)
            .and_then(|bytes| bytes.checked_add(row.text_len))
        else {
            return ERROR_INVALID;
        };
        if required > trueos_vm::vmcall::PAYLOAD_CAP {
            return ERROR_INVALID;
        }
        let text = unsafe { core::slice::from_raw_parts(row.text_ptr, row.text_len) };
        payload.extend_from_slice(&row.x.to_bits().to_le_bytes());
        payload.extend_from_slice(&row.y.to_bits().to_le_bytes());
        payload.extend_from_slice(&row.font_pixels.to_bits().to_le_bytes());
        payload.extend_from_slice(&row.color_rgba.to_le_bytes());
        payload.extend_from_slice(&(row.text_len as u32).to_le_bytes());
        payload.extend_from_slice(text);
    }
    loop {
        let result = guest_status(
            trueos_vm::vmcall::OP_BP_UI4_FONT_CANVAS,
            window_id as u64,
            font_id as u64,
            payload.as_slice(),
        );
        if result != ERROR_BUSY {
            return result;
        }
        trueos_vm::vmcall::sleep_ms(GUEST_TEXT_SCENE_BUSY_POLL_MS);
    }
}

pub(super) fn guest_status(op: u32, arg0: u64, arg1: u64, payload: &[u8]) -> i32 {
    let (status, data) = trueos_vm::vmcall::call_with_payload(op, arg0, arg1, payload, &mut []);
    if status == trueos_vm::vmcall::STATUS_OK {
        data as i64 as i32
    } else {
        ERROR_UI4
    }
}

pub(super) unsafe fn guest_font_sprite_request(
    window_id: u32,
    font_id: u32,
    scalar: u32,
    font_pixels: f32,
    color_rgba: u32,
    out_ticket: *mut u64,
) -> i32 {
    let mut payload = [0u8; 12];
    payload[..4].copy_from_slice(&font_id.to_le_bytes());
    payload[4..8].copy_from_slice(&font_pixels.to_bits().to_le_bytes());
    payload[8..].copy_from_slice(&color_rgba.to_le_bytes());
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_FONT_SPRITE_REQUEST_V1,
        window_id as u64,
        scalar as u64,
        &payload,
        &mut [],
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let result = data as i64;
    if result < 0 {
        return result as i32;
    }
    unsafe { out_ticket.write(data) };
    0
}

pub(super) unsafe fn guest_font_sprite_status(
    window_id: u32,
    ticket: u64,
    out: *mut TrueosUi4FontSpriteStatusV1,
) -> i32 {
    let mut response = [0u8; core::mem::size_of::<TrueosUi4FontSpriteStatusV1>()];
    let (status, data) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_UI4_SCENE_FONT_SPRITE_STATUS_V1,
        window_id as u64,
        ticket,
        &[],
        &mut response,
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return ERROR_UI4;
    }
    let rc = data as i64 as i32;
    if rc == 0 {
        unsafe {
            core::ptr::copy_nonoverlapping(response.as_ptr(), out.cast::<u8>(), response.len());
        }
    }
    rc
}

