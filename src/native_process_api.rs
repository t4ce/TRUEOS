//! Read-only identity of the existing Blueprint execution owner and parent.

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_process_identity_v1(which: u32) -> u32 {
    if which > 1 {
        return 0;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, result) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_PROCESS_IDENTITY_V1,
            u64::from(which),
            0,
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            u32::try_from(result).unwrap_or(0)
        } else {
            0
        };
    }
    crate::r::native_process::current_vm()
        .map(|vm| crate::hv::blueprint_native_process_identity(vm, which))
        .unwrap_or(0)
}
