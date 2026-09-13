//! Single versioned transport for the owner-scoped video stream protocol.
use crate::r::services::video_service;
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_vmedia_video_command_v1(
    command: u32,
    a: u64,
    b: u64,
    input: *const u8,
    input_len: usize,
    output: *mut u8,
    output_len: usize,
) -> i32 {
    if (input_len != 0 && input.is_null())
        || (output_len != 0 && output.is_null())
        || input_len > 3072
        || output_len > 24
    {
        return -3;
    }
    let input = if input_len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(input, input_len) }
    };
    let output = if output_len == 0 {
        &mut []
    } else {
        unsafe { core::slice::from_raw_parts_mut(output, output_len) }
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut payload = [0u8; 3080];
        payload[..8].copy_from_slice(&b.to_le_bytes());
        payload[8..8 + input.len()].copy_from_slice(input);
        let (status, value) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_VMEDIA_VIDEO_COMMAND_V1,
            command as u64,
            a,
            &payload[..8 + input.len()],
            output,
        );
        if status == trueos_vm::vmcall::STATUS_OK {
            value as i64 as i32
        } else {
            -3
        }
    } else {
        video_service::command(crate::r::io::runtime_context_key(), command, a, b, input, output)
    }
}
