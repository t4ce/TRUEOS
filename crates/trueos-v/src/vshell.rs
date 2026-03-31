extern crate alloc;

use alloc::{string::String, vec};

use crate::vcabi;

#[inline]
pub fn uart1_shell_write(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    unsafe { vcabi::trueos_cabi_uart1_shell_write(bytes.as_ptr(), bytes.len()) }
}

#[inline]
pub fn shell2_print_line(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    unsafe { vcabi::trueos_cabi_shell2_print_line(bytes.as_ptr(), bytes.len()) }
}

#[inline]
pub fn shell2_print_targeted_line(target_mask: u32, bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    unsafe {
        vcabi::trueos_cabi_shell2_print_targeted_line(target_mask, bytes.as_ptr(), bytes.len())
    }
}

#[inline]
pub fn shell1_submit_input(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    unsafe { vcabi::trueos_cabi_shell1_submit_input(bytes.as_ptr(), bytes.len()) }
}

#[inline]
pub fn shell_command_registry_json() -> Option<String> {
    let len = unsafe { vcabi::trueos_cabi_shell_command_registry_json(core::ptr::null_mut(), 0) };
    if len <= 0 {
        return None;
    }

    let mut bytes = vec![0u8; len as usize];
    let got =
        unsafe { vcabi::trueos_cabi_shell_command_registry_json(bytes.as_mut_ptr(), bytes.len()) };
    if got <= 0 {
        return None;
    }
    bytes.truncate(got as usize);
    String::from_utf8(bytes).ok()
}

#[inline]
pub fn shell1_history_total_lines() -> usize {
    unsafe { vcabi::trueos_cabi_shell_history_lines_all() }
}

#[inline]
pub fn shell1_history_text_since(start_line: usize, max_lines: usize) -> Option<String> {
    let len = unsafe {
        vcabi::trueos_cabi_shell_history_lines(start_line, max_lines, core::ptr::null_mut(), 0)
    };
    if len <= 0 {
        return None;
    }

    let mut bytes = vec![0u8; len as usize];
    let got = unsafe {
        vcabi::trueos_cabi_shell_history_lines(
            start_line,
            max_lines,
            bytes.as_mut_ptr(),
            bytes.len(),
        )
    };
    if got <= 0 {
        return None;
    }
    bytes.truncate(got as usize);
    String::from_utf8(bytes).ok()
}

#[inline]
pub fn shell_qjs_init() {
    unsafe { vcabi::trueos_cabi_shell_qjs_init() }
}

#[inline]
pub fn shell_qjs_write(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    unsafe { vcabi::trueos_cabi_shell_qjs_write(bytes.as_ptr(), bytes.len()) }
}

#[inline]
pub fn shell_qjs_write_byte(byte: u8) -> bool {
    unsafe { vcabi::trueos_cabi_shell_qjs_write_byte(byte) == 0 }
}

#[inline]
pub fn shell_qjs_read(out: &mut [u8]) -> usize {
    if out.is_empty() {
        return 0;
    }
    let got = unsafe { vcabi::trueos_cabi_shell_qjs_read(out.as_mut_ptr(), out.len()) };
    if got <= 0 { 0 } else { got as usize }
}

#[inline]
pub fn shell_qjs_read_byte() -> Option<u8> {
    let value = unsafe { vcabi::trueos_cabi_shell_qjs_read_byte() };
    if (0..=255).contains(&value) {
        Some(value as u8)
    } else {
        None
    }
}
