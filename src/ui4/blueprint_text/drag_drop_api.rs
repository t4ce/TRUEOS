//! Copied UI4 drag buffers. Window zero addresses the caller's leased Shell3.
use super::{
    ERROR_CONTEXT, ERROR_INVALID, ERROR_NOT_FOUND, ERROR_UI4, SURFACES, blueprint_owner,
    guest_status, surface_mut,
};
use crate::ui4::{CursorFrameKey, WindowOwner, drag_drop};

fn frame(owner: WindowOwner, window_id: u32) -> Result<CursorFrameKey, i32> {
    if window_id == 0 {
        let WindowOwner::Vm(vm) = owner else {
            return Err(ERROR_CONTEXT);
        };
        let window = crate::shell3::tui::window_for_vm(vm).ok_or(ERROR_NOT_FOUND)?;
        if crate::ui4::window_is_closed(WindowOwner::SHELL3_SERVICE, window) {
            return Err(ERROR_NOT_FOUND);
        }
        return Ok(CursorFrameKey::new(WindowOwner::SHELL3_SERVICE, window));
    }
    let mut surfaces = SURFACES.lock();
    let surface = surface_mut(&mut surfaces, owner, window_id).ok_or(ERROR_NOT_FOUND)?;
    if crate::ui4::window_is_closed(owner, surface.window) {
        return Err(ERROR_NOT_FOUND);
    }
    Ok(CursorFrameKey::new(owner, surface.window))
}

pub unsafe extern "C" fn trueos_cabi_ui4_drag_begin_v1(
    window_id: u32,
    kind: u32,
    label: *const u8,
    label_len: usize,
    payload: *const u8,
    payload_len: usize,
) -> i32 {
    if label.is_null()
        || label_len == 0
        || label_len > drag_drop::MAX_LABEL_BYTES
        || payload_len > drag_drop::MAX_PAYLOAD_BYTES
        || (payload_len != 0 && payload.is_null())
    {
        return ERROR_INVALID;
    }
    let label = unsafe { core::slice::from_raw_parts(label, label_len) };
    let data = if payload_len == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(payload, payload_len) }
    };
    let Ok(text) = core::str::from_utf8(label) else {
        return ERROR_INVALID;
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut request = alloc::vec::Vec::with_capacity(4 + label_len + payload_len);
        request.extend_from_slice(&(label_len as u32).to_le_bytes());
        request.extend_from_slice(label);
        request.extend_from_slice(data);
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_DRAG_BEGIN_V1,
            window_id as u64,
            kind as u64,
            &request,
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let origin = match frame(owner, window_id) {
        Ok(frame) => frame,
        Err(error) => return error,
    };
    let Some(source) = crate::ui4::input_broker::drag_source(origin) else {
        return -5;
    };
    drag_drop::begin(owner, origin, source, kind, text, data).unwrap_or_else(|error| error)
}

pub extern "C" fn trueos_cabi_ui4_drag_cancel_v1(token: i32) -> i32 {
    if token <= 0 {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(trueos_vm::vmcall::OP_BP_UI4_DRAG_CANCEL_V1, token as u64, 0, &[]);
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    i32::from(drag_drop::cancel(owner, token))
}

/// Returns zero when empty, otherwise kind/x/y/length (four LE words) + bytes.
/// A short output leaves the drop queued. Only the receiving owner can read it.
pub unsafe extern "C" fn trueos_cabi_ui4_drag_take_v1(
    window_id: u32,
    out: *mut u8,
    cap: usize,
) -> i32 {
    if out.is_null() || cap == 0 || cap > drag_drop::MAX_PAYLOAD_BYTES + 16 {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut response = [0u8; drag_drop::MAX_PAYLOAD_BYTES + 16];
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_UI4_DRAG_TAKE_V1,
            window_id as u64,
            cap as u64,
            &[],
            &mut response[..cap],
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return ERROR_UI4;
        }
        let rc = data as i64 as i32;
        if rc > 0 {
            if rc as usize > cap {
                return ERROR_INVALID;
            }
            unsafe {
                core::ptr::copy_nonoverlapping(response.as_ptr(), out, rc as usize);
            }
        }
        return rc;
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    let target = match frame(owner, window_id) {
        Ok(frame) => frame,
        Err(error) => return error,
    };
    let out = unsafe { core::slice::from_raw_parts_mut(out, cap) };
    match drag_drop::take(owner, target, out) {
        Ok(len) => {
            // Leased terminal apps consume character coordinates; scene frames
            // consume pixels. Use the same scale as Shell3's UI4 rasterizer.
            if window_id == 0 && len != 0 {
                let (width, height) = crate::shell3::service::ui4_cell_extent();
                let x = i32::from_le_bytes(out[4..8].try_into().unwrap()) / width;
                let y = i32::from_le_bytes(out[8..12].try_into().unwrap()) / height;
                out[4..8].copy_from_slice(&x.to_le_bytes());
                out[8..12].copy_from_slice(&y.to_le_bytes());
            }
            len as i32
        }
        Err(error) => error,
    }
}
