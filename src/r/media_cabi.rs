use crate::r::media_service::{self, ImageInfo};

#[inline]
fn direct_owner() -> u32 {
    crate::r::io::runtime_context_key()
}

fn guest_result(call_status: u32, value: u64) -> i32 {
    if call_status == trueos_vm::vmcall::STATUS_OK {
        (value as i64) as i32
    } else {
        media_service::ERR_INVALID
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vmedia_image_decode_begin(format: u32, total_len: usize) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, value) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_VMEDIA_IMAGE_DECODE_BEGIN,
            format as u64,
            total_len as u64,
        );
        return guest_result(status, value);
    }
    media_service::begin(direct_owner(), format, total_len)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vmedia_image_decode_write(
    id: u32,
    offset: usize,
    bytes_ptr: *const u8,
    bytes_len: usize,
) -> i32 {
    if bytes_ptr.is_null() || bytes_len == 0 {
        return media_service::ERR_INVALID;
    }
    let bytes = unsafe { core::slice::from_raw_parts(bytes_ptr, bytes_len) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut copied = 0usize;
        while copied < bytes.len() {
            let end = copied
                .saturating_add(trueos_vm::vmcall::PAYLOAD_CAP)
                .min(bytes.len());
            let Some(chunk_offset) = offset.checked_add(copied) else {
                return media_service::ERR_INVALID;
            };
            let (status, value) = trueos_vm::vmcall::call_with_payload(
                trueos_vm::vmcall::OP_BP_VMEDIA_IMAGE_DECODE_WRITE,
                id as u64,
                chunk_offset as u64,
                &bytes[copied..end],
                &mut [],
            );
            let rc = guest_result(status, value);
            if rc != 0 {
                return rc;
            }
            copied = end;
        }
        return 0;
    }
    media_service::write(direct_owner(), id, offset, bytes)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vmedia_image_decode_commit(id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, value) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_VMEDIA_IMAGE_DECODE_COMMIT,
            id as u64,
            0,
        );
        return guest_result(status, value);
    }
    media_service::commit(direct_owner(), id)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vmedia_image_decode_status(id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, value) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_VMEDIA_IMAGE_DECODE_STATUS,
            id as u64,
            0,
        );
        return guest_result(status, value);
    }
    media_service::status(direct_owner(), id)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vmedia_image_decode_info(id: u32, out: *mut ImageInfo) -> i32 {
    if out.is_null() {
        return media_service::ERR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut bytes = [0u8; core::mem::size_of::<ImageInfo>()];
        let (status, value) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_VMEDIA_IMAGE_DECODE_INFO,
            id as u64,
            0,
            &[],
            &mut bytes,
        );
        let rc = guest_result(status, value);
        if rc != 0 {
            return rc;
        }
        unsafe {
            *out = ImageInfo {
                width: u32::from_le_bytes(bytes[0..4].try_into().unwrap_or_default()),
                height: u32::from_le_bytes(bytes[4..8].try_into().unwrap_or_default()),
                stride_bytes: u32::from_le_bytes(bytes[8..12].try_into().unwrap_or_default()),
                byte_len: u32::from_le_bytes(bytes[12..16].try_into().unwrap_or_default()),
                source_format: u32::from_le_bytes(bytes[16..20].try_into().unwrap_or_default()),
                pixel_format: u32::from_le_bytes(bytes[20..24].try_into().unwrap_or_default()),
                backend: u32::from_le_bytes(bytes[24..28].try_into().unwrap_or_default()),
                revision: u32::from_le_bytes(bytes[28..32].try_into().unwrap_or_default()),
            };
        }
        return 0;
    }
    match media_service::info(direct_owner(), id) {
        Ok(info) => {
            unsafe { *out = info };
            0
        }
        Err(error) => error,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vmedia_image_decode_read(
    id: u32,
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if out_ptr.is_null() || out_cap == 0 {
        return media_service::ERR_INVALID as isize;
    }
    let out = unsafe { core::slice::from_raw_parts_mut(out_ptr, out_cap) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut copied = 0usize;
        while copied < out.len() {
            let cap = (out.len() - copied).min(trueos_vm::vmcall::PAYLOAD_CAP);
            let Some(chunk_offset) = offset.checked_add(copied) else {
                return media_service::ERR_INVALID as isize;
            };
            let packed = ((chunk_offset as u64) << 32) | cap as u64;
            let (status, value) = trueos_vm::vmcall::call_with_payload(
                trueos_vm::vmcall::OP_BP_VMEDIA_IMAGE_DECODE_READ,
                id as u64,
                packed,
                &[],
                &mut out[copied..copied + cap],
            );
            let got = guest_result(status, value);
            if got < 0 {
                return got as isize;
            }
            let got = got as usize;
            if got > cap {
                return media_service::ERR_FAILED as isize;
            }
            copied += got;
            if got < cap {
                break;
            }
        }
        return copied as isize;
    }
    match media_service::read(direct_owner(), id, offset, out) {
        Ok(copied) => copied as isize,
        Err(error) => error as isize,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_vmedia_image_decode_discard(id: u32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, value) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_VMEDIA_IMAGE_DECODE_DISCARD,
            id as u64,
            0,
        );
        return guest_result(status, value);
    }
    media_service::discard(direct_owner(), id)
}
