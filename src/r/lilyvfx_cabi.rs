//! Compatibility entry points for the disabled Spirit background export.

pub(crate) const CONTROL_BYTES: usize = 33 * 4;
const FRAME_PITCH: u32 = 256 * 4;
const FRAME_BYTES: usize = FRAME_PITCH as usize * 256;
pub(crate) const CHUNK_BYTES: usize = FRAME_BYTES / 2;
const ERR_NOT_SUPPORTED: i32 = -95;

pub(crate) fn discard_guest(_vm_id: u8) {}

pub(crate) fn capture_guest(vm_id: u8, controls: &[u8], first: &mut [u8]) -> Result<u64, i32> {
    if vm_id as usize >= crate::hv::TRUEOS_VM_ID_LIMIT || first.len() != CHUNK_BYTES {
        return Err(-22);
    }
    Err(capture_status(controls))
}

pub(crate) fn finish_guest(vm_id: u8, _ticket: u64, tail: &mut [u8]) -> Result<(), i32> {
    if vm_id as usize >= crate::hv::TRUEOS_VM_ID_LIMIT || tail.len() != CHUNK_BYTES {
        return Err(-22);
    }
    Err(ERR_NOT_SUPPORTED)
}

fn capture_status(controls: &[u8]) -> i32 {
    let Ok(page) = <&[u8; CONTROL_BYTES]>::try_from(controls) else {
        return -22;
    };
    let word = |index: usize| {
        let offset = index * 4;
        u32::from_le_bytes(page[offset..offset + 4].try_into().unwrap())
    };
    if word(0) != 0x5356_4658
        || word(1) != 1
        || !matches!(word(3), 2..=13)
        || word(27) != FRAME_PITCH
    {
        return -22;
    }
    ERR_NOT_SUPPORTED
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lilyvfx_background_v1(
    controls: *const u8,
    controls_len: usize,
    output: *mut u8,
    output_len: usize,
) -> i32 {
    if controls.is_null()
        || output.is_null()
        || controls_len != CONTROL_BYTES
        || output_len != FRAME_BYTES
    {
        return -22;
    }

    capture_status(unsafe { core::slice::from_raw_parts(controls, controls_len) })
}
