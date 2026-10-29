// Versioned byte-session ABI. Guest requests carry no caller-supplied owner identity.
// Geometry 0x0 selects captured command I/O; a valid nonzero geometry explicitly
// selects the interactive terminal. Both use the existing Shell3 AP worker pool.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_shell3_session_open_v1(script_ptr: *const u8, script_len: usize, cols: u32, rows: u32, stdin_open: u32) -> i64 {
    let cap = trueos_vm::vmcall::PAYLOAD_CAP;
    if script_len > cap - 12 || (script_len != 0 && script_ptr.is_null()) { return -1; }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let script = if script_len == 0 { &[] } else { unsafe { core::slice::from_raw_parts(script_ptr, script_len) } };
        let mut payload = alloc::vec![0; 12 + script_len];
        payload[..4].copy_from_slice(&cols.to_le_bytes());
        payload[4..8].copy_from_slice(&rows.to_le_bytes());
        payload[8..12].copy_from_slice(&stdin_open.to_le_bytes());
        payload[12..].copy_from_slice(script);
        let (status, rc) = trueos_vm::vmcall::call_with_payload(trueos_vm::vmcall::OP_BP_SHELL3_SESSION_OPEN_V1, 0, 0, &payload, &mut []);
        return if status == trueos_vm::vmcall::STATUS_OK { rc as i64 } else { -3 };
    }
    let Some(vm) = crate::hv::current_guest_execution_context_vm_id() else { return -3; };
    let Some(script) = crate::std_abi_shim::abi_read_bytes(script_ptr, script_len) else { return -1; };
    crate::shell3::sessions::open(vm, script, cols as usize, rows as usize, stdin_open != 0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_shell3_session_read_v1(session: u64, out_ptr: *mut u8, out_cap: usize) -> isize {
    let cap = out_cap.min(trueos_vm::vmcall::PAYLOAD_CAP);
    if cap < 8 || out_ptr.is_null() { return -1; }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let out = unsafe { core::slice::from_raw_parts_mut(out_ptr, cap) };
        let (status, rc) = trueos_vm::vmcall::call_with_payload(trueos_vm::vmcall::OP_BP_SHELL3_SESSION_READ_V1, session, cap as u64, &[], out);
        if status != trueos_vm::vmcall::STATUS_OK { return -3; }
        let rc = rc as isize;
        return if rc > cap as isize { -1 } else { rc };
    }
    let Some(vm) = crate::hv::current_guest_execution_context_vm_id() else { return -3; };
    let Some(out) = crate::std_abi_shim::abi_write_bytes(out_ptr, cap) else { return -1; };
    crate::shell3::sessions::read(vm, session, out)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_shell3_session_write_v1(session: u64, bytes_ptr: *const u8, bytes_len: usize) -> isize {
    // Partial acceptance is explicit, never an implicit vmcall truncation.
    let len = bytes_len.min(trueos_vm::vmcall::PAYLOAD_CAP);
    if len != 0 && bytes_ptr.is_null() { return -1; }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let bytes = if len == 0 { &[] } else { unsafe { core::slice::from_raw_parts(bytes_ptr, len) } };
        let (status, rc) = trueos_vm::vmcall::call_with_payload(trueos_vm::vmcall::OP_BP_SHELL3_SESSION_WRITE_V1, session, 0, bytes, &mut []);
        return if status == trueos_vm::vmcall::STATUS_OK { rc as isize } else { -3 };
    }
    let Some(vm) = crate::hv::current_guest_execution_context_vm_id() else { return -3; };
    let Some(bytes) = crate::std_abi_shim::abi_read_bytes(bytes_ptr, len) else { return -1; };
    crate::shell3::sessions::write(vm, session, bytes)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_shell3_session_control_v1(session: u64, operation: u32, cols: u32, rows: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut payload = [0u8; 8];
        payload[..4].copy_from_slice(&cols.to_le_bytes());
        payload[4..].copy_from_slice(&rows.to_le_bytes());
        let (status, rc) = trueos_vm::vmcall::call_with_payload(trueos_vm::vmcall::OP_BP_SHELL3_SESSION_CONTROL_V1, session, operation as u64, &payload, &mut []);
        return if status == trueos_vm::vmcall::STATUS_OK { rc as i32 } else { -3 };
    }
    let Some(vm) = crate::hv::current_guest_execution_context_vm_id() else { return -3; };
    crate::shell3::sessions::control(vm, session, operation, cols as usize, rows as usize)
}
