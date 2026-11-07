//! Read-only classification of the launch's existing native byte streams.

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_stdio_is_terminal_v1(stream: u32) -> i32 {
    if stream > 2 {
        return 0;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        // The launch's console state can change while parked; query the live
        // host owner rather than the Hull's private launch-context snapshot.
        let (status, result) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_STDIO_IS_TERMINAL_V1,
            u64::from(stream),
            0,
        );
        return (status == trueos_vm::vmcall::STATUS_OK && result == 1) as i32;
    }
    crate::r::native_process::current_vm()
        .is_some_and(|vm| crate::hv::blueprint_stdio_is_terminal(vm, stream)) as i32
}
