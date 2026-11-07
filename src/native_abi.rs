//! Native Blueprint allocation, byte I/O, time, DNS and execution entry points.
//!
//! Buffers cross the VM boundary through the address translation helpers below.
//! Allocations use the owning heap directly; no libc pointer registry, errno,
//! POSIX descriptor namespace or pthread process state is installed.

extern crate alloc;
use alloc::{string::String, vec::Vec};
use core::{
    alloc::Layout,
    ffi::{c_char, c_int, c_void},
    ptr, slice,
};

const TRUEOS_EINVAL: c_int = 22;
const TRUEOS_EIO: c_int = 5;
const TRUEOS_EPIPE: c_int = 32;
const TRUEOS_ETIMEDOUT: c_int = 110;

#[repr(C)]
pub struct TrueosCabiHeapStats {
    pub heap_start: usize,
    pub heap_end: usize,
    pub usable_total: usize,
    pub free_bytes: usize,
    pub largest_free_block: usize,
    pub free_blocks: usize,
    pub initialized: u32,
    pub source: u32,
}

fn write_platform_fd(fd: u32, bytes: &[u8]) {
    match fd {
        1 => crate::r::io::cabi::write_console_bytes(crate::r::io::cabi::ConsoleStream::Out, bytes),
        2 => crate::r::io::cabi::write_console_bytes(crate::r::io::cabi::ConsoleStream::Err, bytes),
        _ => {}
    }
}

fn active_abi_guest_vm_id() -> Option<u8> {
    crate::hv::current_guest_execution_context_vm_id()
        .or_else(crate::hv::current_vm_id_by_lapic_low)
}

fn active_abi_alloc_guest_vm_id() -> Option<u8> {
    crate::hv::current_hull_guest_context_vm_id()
        .or_else(crate::r::kernel_task_domain::guest_owned_alloc_vm_id)
        .or_else(crate::hv::current_vm_id_by_lapic_low)
}

fn active_guest_stack_host_ptr_for_vm(vm_id: u8, ptr: *mut u8, len: usize) -> Option<*mut u8> {
    let guest_va = ptr as usize as u64;
    let offset = guest_va.checked_sub(crate::hv::memory::GUEST_STACK_VA_BASE)? as usize;
    if crate::hv::current_hull_guest_context_vm_id() == Some(vm_id) {
        let stack_bytes = crate::hv::memory::active_guest_stack_bytes_for_vm(vm_id);
        let end = offset.checked_add(len)?;
        if end <= stack_bytes {
            return Some(ptr);
        }
        return None;
    }
    let stack = crate::hv::memory::guest_stack_slice_for_vm(vm_id)?;
    let end = offset.checked_add(len)?;
    if end > stack.len() {
        return None;
    }
    let base = crate::hv::memory::guest_stack_mut_ptr_for_vm(vm_id)?;
    Some(unsafe { base.add(offset) })
}

fn active_guest_heap_host_ptr_for_vm(vm_id: u8, ptr: *mut u8, len: usize) -> Option<*mut u8> {
    let guest_va = ptr as usize;
    let (heap_start, heap_end) = crate::allocators::hv_guest_heap_bounds(vm_id)?;
    let end = guest_va.checked_add(len)?;
    if guest_va >= heap_start && end <= heap_end {
        Some(ptr)
    } else {
        None
    }
}

fn any_guest_host_ptr(ptr: *mut u8, len: usize) -> Option<*mut u8> {
    for vm_id in 0..crate::allcaps::hv::VM_ID_LIMIT {
        let vm_id = vm_id as u8;
        if let Some(host) = active_guest_stack_host_ptr_for_vm(vm_id, ptr, len)
            .or_else(|| active_guest_heap_host_ptr_for_vm(vm_id, ptr, len))
        {
            return Some(host);
        }
    }
    None
}

fn looks_like_low_guest_ptr(ptr: *const u8) -> bool {
    let guest_va = ptr as usize as u64;
    guest_va >= crate::hv::memory::GUEST_STACK_VA_BASE
        && guest_va < crate::hv::memory::GUEST_COMM_PAGE_VA
}

fn abi_host_ptr(ptr: *mut u8, len: usize) -> Option<*mut u8> {
    if ptr.is_null() {
        return None;
    }
    if len == 0 {
        return Some(ptr);
    }
    let Some(vm_id) = active_abi_guest_vm_id() else {
        return any_guest_host_ptr(ptr, len).or_else(|| {
            if looks_like_low_guest_ptr(ptr) {
                None
            } else {
                Some(ptr)
            }
        });
    };
    active_guest_stack_host_ptr_for_vm(vm_id, ptr, len)
        .or_else(|| active_guest_heap_host_ptr_for_vm(vm_id, ptr, len))
        .or_else(|| any_guest_host_ptr(ptr, len))
        .or_else(|| {
            if looks_like_low_guest_ptr(ptr) {
                None
            } else {
                Some(ptr)
            }
        })
}

pub(crate) fn abi_read_bytes<'a>(ptr: *const u8, len: usize) -> Option<&'a [u8]> {
    if len == 0 {
        return Some(&[]);
    }
    let host = abi_host_ptr(ptr.cast_mut(), len)?;
    Some(unsafe { slice::from_raw_parts(host.cast::<u8>(), len) })
}

pub(crate) fn abi_write_bytes<'a>(ptr: *mut u8, len: usize) -> Option<&'a mut [u8]> {
    if len == 0 {
        return Some(&mut []);
    }
    let host = abi_host_ptr(ptr, len)?;
    Some(unsafe { slice::from_raw_parts_mut(host, len) })
}

fn abi_cstr_to_string(ptr: *const c_char, max_len: usize) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let mut bytes = Vec::new();
    for offset in 0..max_len {
        let byte = *abi_read_bytes(unsafe { ptr.cast::<u8>().add(offset) }, 1)?.first()?;
        if byte == 0 {
            return String::from_utf8(bytes).ok();
        }
        bytes.push(byte);
    }
    None
}

pub(crate) fn copy_to_abi_out(ptr: *mut u8, bytes: &[u8]) -> bool {
    if ptr.is_null() {
        return false;
    }
    let Some(dst) = abi_host_ptr(ptr, bytes.len()) else {
        return false;
    };
    unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len()) };
    true
}

fn copy_usize_to_abi_out(ptr: *mut usize, value: usize) -> bool {
    copy_to_abi_out(ptr.cast::<u8>(), &value.to_ne_bytes())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_alloc_aligned(size: usize, align: usize) -> *mut u8 {
    if size == 0 {
        return ptr::null_mut();
    }

    let Ok(layout) = Layout::from_size_align(size, align.max(1)) else {
        return ptr::null_mut();
    };

    if let Some(vm_id) = active_abi_alloc_guest_vm_id() {
        unsafe { crate::allocators::alloc_raw_hv_guest(vm_id, layout) }
    } else {
        unsafe { crate::allocators::alloc_raw(layout) }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_dealloc(ptr: *mut u8) {
    unsafe { crate::allocators::dealloc_raw(ptr) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_rand(recv_buf: *mut u32, words: usize) {
    if recv_buf.is_null() || words == 0 {
        return;
    }

    let byte_len = words.saturating_mul(core::mem::size_of::<u32>());
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let bytes = unsafe { slice::from_raw_parts_mut(recv_buf.cast::<u8>(), byte_len) };
        let mut offset = 0usize;
        let mut chunk = [0u8; trueos_vm::vmcall::PAYLOAD_CAP];
        while offset < bytes.len() {
            let want = core::cmp::min(chunk.len(), bytes.len() - offset);
            let (status, got) = trueos_vm::vmcall::call_with_payload(
                trueos_vm::vmcall::OP_RAND_BYTES,
                want as u64,
                0,
                &[],
                &mut chunk[..want],
            );
            if status != trueos_vm::vmcall::STATUS_OK || got as usize != want {
                bytes[offset..].fill(0);
                return;
            }
            bytes[offset..offset + want].copy_from_slice(&chunk[..want]);
            offset += want;
        }
        return;
    }
    let Some(bytes) = abi_write_bytes(recv_buf.cast::<u8>(), byte_len) else {
        return;
    };
    if !crate::tyche::fill_bytes(bytes) {
        bytes.fill(0);
    }
}

fn child_vmx_stdio() -> bool {
    crate::r::io::env::var("TRUEOS_MCP_STDIO").as_deref() == Some("1")
}

fn write_child_vmx_stdout(bytes: &[u8]) -> Result<usize, c_int> {
    let len = bytes.len().min(trueos_vm::vmcall::PAYLOAD_CAP);
    loop {
        let result = unsafe {
            crate::r::io::fs_cabi::trueos_cabi_blueprint_child_send_v1(0, bytes.as_ptr(), len)
        };
        if result >= 0 {
            return Ok(result as usize);
        }
        if result != -3 {
            return Err(TRUEOS_EPIPE);
        }
        crate::r::io::fs_cabi::trueos_cabi_sleep_ms(10);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_write(fd: u32, write_buf: *const u8, nbytes: usize) {
    if write_buf.is_null() || nbytes == 0 {
        return;
    }
    let Some(bytes) = abi_read_bytes(write_buf, nbytes) else {
        return;
    };
    if fd == 1 && child_vmx_stdio() {
        let mut rest = bytes;
        while !rest.is_empty() {
            match write_child_vmx_stdout(rest) {
                Ok(n) if n > 0 => rest = &rest[n..],
                _ => break,
            }
        }
    } else {
        write_platform_fd(fd, bytes);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_read(_fd: u32, recv_buf: *mut u8, nrequested: usize) -> usize {
    if recv_buf.is_null() || nrequested == 0 {
        return 0;
    }

    let Some(out) = abi_write_bytes(recv_buf, nrequested) else {
        return 0;
    };
    if child_vmx_stdio() {
        let n = unsafe {
            crate::r::io::fs_cabi::trueos_cabi_blueprint_child_read_v1(
                0,
                out.as_mut_ptr(),
                out.len(),
            )
        };
        if n > 0 { n as usize } else { 0 }
    } else {
        crate::r::io::fs_cabi::read_attached_console_bytes(out)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_time_monotonic_nanos() -> u64 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return crate::hv::vmcall::guest_monotonic_nanos();
    }

    crate::chronos::monotonic_nanos()
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_time_unix_seconds() -> u64 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return crate::hv::vmcall::guest_unix_seconds();
    }

    crate::chronos::best_effort_unix_time_seconds().unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_time_unix_nanos() -> u64 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return crate::hv::vmcall::guest_unix_seconds().saturating_mul(1_000_000_000);
    }

    crate::chronos::best_effort_unix_time_seconds()
        .unwrap_or(0)
        .saturating_mul(1_000_000_000)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_boot_timestamp_secs() -> u64 {
    crate::limine::boot_timestamp_secs().unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_thread_current_id() -> usize {
    crate::r::native_thread::current_id()
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_ntp_current_unix_seconds() -> u64 {
    crate::r::net::ntp::current_unix_seconds()
        .or_else(crate::r::time::unix_time_seconds)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ntp_kernel_date_day_month_year(
    out_ptr: *mut u8,
    out_cap: usize,
) -> usize {
    let date = crate::r::net::ntp::kernel_date_day_month_year();
    let bytes = date.as_bytes();
    if out_ptr.is_null() || out_cap == 0 {
        return bytes.len();
    }
    let n = core::cmp::min(bytes.len(), out_cap);
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr, n);
    }
    n
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn sys_halt() -> ! {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        // Release the borrowed terminal before the final VMCALL stops the VM.
        let _ = trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_RETURN_TO_CLI, 0, 0);
        trueos_vm::vmcall::shutdown(b"blueprint process exit");
    }
    // A native application abort/exit is a terminal ABI boundary: there are
    // no host guards owned by this shim. Resume the saved carrier so it can
    // restore CR3 and realm state before requesting this VM's teardown.
    if unsafe { crate::r::threads::terminate_current_guest() } {
        unreachable!("terminal native continuation resumed");
    }
    loop {
        core::hint::spin_loop();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_heap_stats(out: *mut TrueosCabiHeapStats) -> i32 {
    if out.is_null() {
        return -1;
    }
    // Portal calls execute in the host, but allocations made for a blueprint are
    // charged to its VM-owned allocator. Report that same live domain here so a
    // guest can make bounded allocation decisions without knowing a static VM
    // RAM size. Non-VM callers continue to receive host allocator statistics.
    let stats = active_abi_alloc_guest_vm_id()
        .and_then(crate::allocators::hv_guest_heap_stats_if_configured)
        .unwrap_or_else(crate::allocators::heap_stats);
    let source = match stats.source {
        crate::allocators::HeapSourceKind::Unconfigured => 0,
        crate::allocators::HeapSourceKind::Arena => 1,
    };
    unsafe {
        *out = TrueosCabiHeapStats {
            heap_start: stats.heap_start,
            heap_end: stats.heap_end,
            usable_total: stats.usable_total,
            free_bytes: stats.free_bytes,
            largest_free_block: stats.largest_free_block,
            free_blocks: stats.free_blocks,
            initialized: u32::from(stats.initialized),
            source,
        };
    }
    0
}

fn dns_resolve_error_to_cabi_errno(err: crate::r::net::vlayer::DnsResolveError) -> c_int {
    match err {
        crate::r::net::vlayer::DnsResolveError::BadName
        | crate::r::net::vlayer::DnsResolveError::NoAnswer => TRUEOS_EIO,
        crate::r::net::vlayer::DnsResolveError::Runtime
        | crate::r::net::vlayer::DnsResolveError::NoNic
        | crate::r::net::vlayer::DnsResolveError::Timeout => TRUEOS_ETIMEDOUT,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_dns_resolve_ipv4(
    host: *const u8,
    host_len: usize,
    out_octets: *mut u8,
) -> c_int {
    if host.is_null() || out_octets.is_null() || host_len == 0 {
        return TRUEOS_EINVAL;
    }
    let Some(host_bytes) = abi_read_bytes(host, host_len) else {
        return TRUEOS_EINVAL;
    };
    let Ok(host_name) = core::str::from_utf8(host_bytes) else {
        return TRUEOS_EINVAL;
    };
    match crate::r::net::vlayer::resolve_ipv4_for_sync_abi(host_name) {
        Ok(ip) => {
            if copy_to_abi_out(out_octets, &ip) {
                0
            } else {
                TRUEOS_EINVAL
            }
        }
        Err(err) => {
            let errno = dns_resolve_error_to_cabi_errno(err);
            errno
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_thread_spawn(
    stack: usize,
    entry: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    arg: *mut c_void,
    thread: *mut usize,
) -> c_int {
    if thread.is_null() {
        return TRUEOS_EINVAL;
    }
    match crate::r::native_thread::spawn(stack, entry, arg, |id| copy_usize_to_abi_out(thread, id))
    {
        Ok(_) => 0,
        Err(error) => error,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_thread_join(thread: usize) -> c_int {
    crate::r::native_thread::join(thread).map_or_else(|error| error, |_| 0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_thread_detach(thread: usize) -> c_int {
    crate::r::native_thread::detach(thread).map_or_else(|error| error, |()| 0)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_thread_available_parallelism() -> usize {
    crate::r::platform::trueos_platform_cpu_count().max(1)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_thread_configure_placement_v1(mode: u32, background_cap: u32) -> i32 {
    crate::r::threads::configure_placement(mode, background_cap)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_thread_swap_child_role_v1(role: u32) -> u32 {
    crate::r::threads::swap_scope(false, role)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_thread_swap_activity_v1(activity: u32) -> u32 {
    crate::r::threads::swap_scope(true, activity)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_thread_set_name(name: *const c_char) -> c_int {
    let Some(name) = abi_cstr_to_string(name, 4096) else {
        return TRUEOS_EINVAL;
    };
    crate::r::blocking::set_current_service_lane_pthread_name(name.as_str());
    0
}
