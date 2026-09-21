//! WC3-private Blueprint ABI leaf functions and resolver.

use super::x86_runtime as runtime;
use v::bp_abi::{
    TrueosX86DebugRegistersV1,
    TrueosX86ExitV1,
    TrueosX86RegistersV1,
};

fn status(result: Result<(), i32>) -> i32 {
    result.map(|()| 0).unwrap_or_else(|error| error)
}
fn count(result: Result<usize, i32>) -> isize {
    result
        .map(|count| count as isize)
        .unwrap_or_else(|error| error as isize)
}

pub(super) unsafe extern "C" fn trueos_cabi_x86_address_space_create_v1(out: *mut u64) -> i32 {
    match out.as_mut() {
        Some(out) => status(runtime::address_space_create(out)),
        None => runtime::ERR_INVALID,
    }
}
pub(super) extern "C" fn trueos_cabi_x86_address_space_destroy_v1(handle: u64) -> i32 {
    status(runtime::address_space_destroy(handle))
}
pub(super) extern "C" fn trueos_cabi_x86_address_space_map_v1(
    handle: u64,
    start: u32,
    len: u32,
    permissions: u32,
) -> i32 {
    status(runtime::address_space_map(handle, start, len, permissions))
}
pub(super) extern "C" fn trueos_cabi_x86_address_space_unmap_v1(
    handle: u64,
    start: u32,
    len: u32,
) -> i32 {
    status(runtime::address_space_unmap(handle, start, len))
}
pub(super) unsafe extern "C" fn trueos_cabi_x86_address_space_read_v1(
    handle: u64,
    address: u32,
    out: *mut u8,
    len: usize,
) -> isize {
    if len != 0 && out.is_null() {
        return runtime::ERR_INVALID as isize;
    }
    count(runtime::address_space_read(handle, address, core::slice::from_raw_parts_mut(out, len)))
}
pub(super) unsafe extern "C" fn trueos_cabi_x86_address_space_write_v1(
    handle: u64,
    address: u32,
    data: *const u8,
    len: usize,
) -> isize {
    if len != 0 && data.is_null() {
        return runtime::ERR_INVALID as isize;
    }
    count(runtime::address_space_write(handle, address, core::slice::from_raw_parts(data, len)))
}
pub(super) unsafe extern "C" fn trueos_cabi_x86_context_create_v1(
    space: u64,
    registers: *const TrueosX86RegistersV1,
    out: *mut u64,
) -> i32 {
    match (registers.as_ref(), out.as_mut()) {
        (Some(registers), Some(out)) => status(runtime::context_create(space, *registers, out)),
        _ => runtime::ERR_INVALID,
    }
}
pub(super) extern "C" fn trueos_cabi_x86_context_destroy_v1(handle: u64) -> i32 {
    status(runtime::context_destroy(handle))
}
pub(super) unsafe extern "C" fn trueos_cabi_x86_context_registers_get_v1(
    handle: u64,
    out: *mut TrueosX86RegistersV1,
) -> i32 {
    match out.as_mut() {
        Some(out) => status(runtime::context_registers_get(handle, out)),
        None => runtime::ERR_INVALID,
    }
}
pub(super) unsafe extern "C" fn trueos_cabi_x86_context_registers_set_v1(
    handle: u64,
    registers: *const TrueosX86RegistersV1,
) -> i32 {
    match registers.as_ref() {
        Some(registers) => status(runtime::context_registers_set(handle, *registers)),
        None => runtime::ERR_INVALID,
    }
}
pub(super) unsafe extern "C" fn trueos_cabi_x86_context_debug_registers_get_v1(
    handle: u64,
    out: *mut TrueosX86DebugRegistersV1,
) -> i32 {
    match out.as_mut() {
        Some(out) => status(runtime::context_debug_registers_get(handle, out)),
        None => runtime::ERR_INVALID,
    }
}
pub(super) unsafe extern "C" fn trueos_cabi_x86_context_debug_registers_set_v1(
    handle: u64,
    registers: *const TrueosX86DebugRegistersV1,
) -> i32 {
    match registers.as_ref() {
        Some(registers) => status(runtime::context_debug_registers_set(handle, *registers)),
        None => runtime::ERR_INVALID,
    }
}
pub(super) unsafe extern "C" fn trueos_cabi_x86_context_run_v1(
    handle: u64,
    out: *mut TrueosX86ExitV1,
) -> i32 {
    match out.as_mut() {
        Some(out) => status(runtime::context_execute(handle, out, false)),
        None => runtime::ERR_INVALID,
    }
}
pub(super) unsafe extern "C" fn trueos_cabi_x86_context_resume_v1(
    handle: u64,
    out: *mut TrueosX86ExitV1,
) -> i32 {
    match out.as_mut() {
        Some(out) => status(runtime::context_execute(handle, out, true)),
        None => runtime::ERR_INVALID,
    }
}
pub(super) extern "C" fn trueos_cabi_x86_context_park_v1(handle: u64) -> i32 {
    status(runtime::context_park(handle))
}
pub(super) extern "C" fn trueos_cabi_x86_context_cancel_v1(handle: u64) -> i32 {
    status(runtime::context_cancel(handle))
}

pub(super) fn resolve(name: &str) -> Option<usize> {
    Some(match name {
        "trueos_cabi_x86_address_space_create_v1" => {
            trueos_cabi_x86_address_space_create_v1 as *const () as usize
        }
        "trueos_cabi_x86_address_space_destroy_v1" => {
            trueos_cabi_x86_address_space_destroy_v1 as *const () as usize
        }
        "trueos_cabi_x86_address_space_map_v1" => {
            trueos_cabi_x86_address_space_map_v1 as *const () as usize
        }
        "trueos_cabi_x86_address_space_unmap_v1" => {
            trueos_cabi_x86_address_space_unmap_v1 as *const () as usize
        }
        "trueos_cabi_x86_address_space_read_v1" => {
            trueos_cabi_x86_address_space_read_v1 as *const () as usize
        }
        "trueos_cabi_x86_address_space_write_v1" => {
            trueos_cabi_x86_address_space_write_v1 as *const () as usize
        }
        "trueos_cabi_x86_context_create_v1" => {
            trueos_cabi_x86_context_create_v1 as *const () as usize
        }
        "trueos_cabi_x86_context_destroy_v1" => {
            trueos_cabi_x86_context_destroy_v1 as *const () as usize
        }
        "trueos_cabi_x86_context_registers_get_v1" => {
            trueos_cabi_x86_context_registers_get_v1 as *const () as usize
        }
        "trueos_cabi_x86_context_registers_set_v1" => {
            trueos_cabi_x86_context_registers_set_v1 as *const () as usize
        }
        "trueos_cabi_x86_context_debug_registers_get_v1" => {
            trueos_cabi_x86_context_debug_registers_get_v1 as *const () as usize
        }
        "trueos_cabi_x86_context_debug_registers_set_v1" => {
            trueos_cabi_x86_context_debug_registers_set_v1 as *const () as usize
        }
        "trueos_cabi_x86_context_run_v1" => trueos_cabi_x86_context_run_v1 as *const () as usize,
        "trueos_cabi_x86_context_resume_v1" => {
            trueos_cabi_x86_context_resume_v1 as *const () as usize
        }
        "trueos_cabi_x86_context_park_v1" => trueos_cabi_x86_context_park_v1 as *const () as usize,
        "trueos_cabi_x86_context_cancel_v1" => {
            trueos_cabi_x86_context_cancel_v1 as *const () as usize
        }
        _ => return None,
    })
}
