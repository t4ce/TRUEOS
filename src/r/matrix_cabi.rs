//! Headless, bounded Odin-native matrix multiplication. Wire data is little endian.

use alloc::vec::Vec;

const MAX_N: usize = 128;
const MAX_CODE: usize = 2048;

pub(crate) fn capacity() -> u32 {
    crate::workers::background_worker_slots().len().min(64) as u32
}

pub(crate) fn compute_wire(payload: &[u8]) -> Result<Vec<u8>, i32> {
    if payload.len() < 16 { return Err(-22); }
    let word = |offset| u32::from_le_bytes(payload[offset..offset+4].try_into().unwrap());
    let n = word(0) as usize;
    let saturate = word(4);
    let rounds = word(8);
    let code_len = word(12) as usize;
    if !(1..=MAX_N).contains(&n) || saturate > 1 || !(1..=4).contains(&rounds)
        || !(112..=MAX_CODE).contains(&code_len) || payload.len() != 16+code_len+n*n*4 {
        return Err(-22);
    }
    crate::intel::gpgpu::dispatch_native_matrix(&payload[16..16+code_len], &payload[16+code_len..], n, saturate, rounds)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_matrix_compute_capacity_v1() -> u32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, value) = trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_MATRIX_COMPUTE_V1, 1, 0);
        if status != trueos_vm::vmcall::STATUS_OK { return 0; }
        return value.min(64) as u32;
    }
    capacity()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_matrix_compute_v1(
    code: *const u8, code_len: usize, input: *const u32, n: u32,
    saturate: u32, rounds: u32, output: *mut u32,
) -> i32 {
    if !(1..=MAX_N as u32).contains(&n) || saturate > 1 || !(1..=4).contains(&rounds)
        || !(112..=MAX_CODE).contains(&code_len) || code.is_null() || input.is_null()
        || output.is_null() || input.addr()%4 != 0 || output.addr()%4 != 0 { return -22; }
    let size = n as usize*n as usize*4;
    let mut payload = Vec::new();
    if payload.try_reserve_exact(16+code_len+size).is_err() { return -12; }
    for word in [n, saturate, rounds, code_len as u32] { payload.extend_from_slice(&word.to_le_bytes()); }
    unsafe {
        payload.extend_from_slice(core::slice::from_raw_parts(code, code_len));
        payload.extend_from_slice(core::slice::from_raw_parts(input.cast::<u8>(), size));
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let target = unsafe { core::slice::from_raw_parts_mut(output.cast::<u8>(), size) };
        let (status, rc, count) = trueos_vm::vmcall::call_with_payload_reported_len(
            trueos_vm::vmcall::OP_BP_MATRIX_COMPUTE_V1, 0, 0, &payload, target);
        if status != trueos_vm::vmcall::STATUS_OK { return -5; }
        if rc as i64 != 0 { return rc as i64 as i32; }
        if count != size { return -5; }
        return 0;
    }
    match compute_wire(&payload) {
        Ok(result) => { unsafe { core::ptr::copy_nonoverlapping(result.as_ptr(), output.cast::<u8>(), size); } 0 }
        Err(rc) => rc,
    }
}
