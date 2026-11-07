// Reference source for library/std/src/sys/exit/trueos.rs.
// Numeric exit codes are recorded as a reason; native child APIs expose
// lifecycle states rather than POSIX wait statuses.

unsafe extern "C" {
    fn trueos_cabi_blueprint_shutdown(reason: *const u8, len: usize) -> i32;
    fn sys_halt() -> !;
}

pub fn exit(code: i32) -> ! {
    let reason = format!("Rust process exit code={code}");
    unsafe {
        trueos_cabi_blueprint_shutdown(reason.as_ptr(), reason.len());
        // A successful native carrier/Hull shutdown never reaches this call.
        // Its terminal fallback still cannot return to user code.
        sys_halt()
    }
}
