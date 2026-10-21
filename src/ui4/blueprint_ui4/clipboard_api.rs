//! Clipboard delivery is unavailable in the kernel baseline.
use super::*;

pub(crate) fn release_owner(owner: WindowOwner) {
}

pub(super) fn release_window(owner: WindowOwner, window: WindowId) {
}

pub(crate) fn trusted_paste(owner: WindowOwner, window: WindowId) {
}

pub unsafe extern "C" fn trueos_cabi_clipboard_command_v1(
    window_id: u32,
    command: u32,
    kind: u32,
    input: *const u8,
    input_len: usize,
    output: *mut u8,
    output_cap: usize,
) -> i32 {
    -38
}
