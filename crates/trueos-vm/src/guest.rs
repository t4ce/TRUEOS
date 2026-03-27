core::arch::global_asm!(
    r#"
    .global trueos_vmx_guest_entry
    .type trueos_vmx_guest_entry,@function
trueos_vmx_guest_entry:
    lea rsi, [rip + .Lvmcr3]
.Lwrite_vmcr3:
    lodsb
    test al, al
    jz .Lpreserve_start
    mov dx, 0xE9
    out dx, al
    jmp .Lwrite_vmcr3

.Lpreserve_start:
    lea rsi, [rip + .Lpreserve]
.Lwrite_preserve:
    lodsb
    test al, al
    jz .Ldo_run
    mov dx, 0xE9
    out dx, al
    jmp .Lwrite_preserve

.Ldo_run:
    lea rsi, [rip + .Lrun]
.Lwrite_run:
    lodsb
    test al, al
    jz .Lcall_run
    mov dx, 0xE9
    out dx, al
    jmp .Lwrite_run

.Lcall_run:
    call trueos_vm_guest_run

    lea rsi, [rip + .Lidle]
.Lwrite_idle:
    lodsb
    test al, al
    jz .Lcall_idle
    mov dx, 0xE9
    out dx, al
    jmp .Lwrite_idle

.Lcall_idle:
    call trueos_vm_guest_idle
    ud2

.Lvmcr3:
    .asciz "VMCR3\n"
.Lpreserve:
    .asciz "VMPRESERVE\n"
.Lrun:
    .asciz "VMRUN\n"
.Lidle:
    .asciz "VMIDLE\n"
"#
);

#[unsafe(no_mangle)]
pub extern "C" fn trueos_vm_guest_run() {
    crate::demo::start();
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_vm_guest_idle() -> ! {
    crate::demo::idle();
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_vm_preserve() {
    crate::vmcall::preserve();
}

unsafe extern "C" {
    #[link_name = "trueos_vmx_guest_entry"]
    pub fn entry() -> !;
}
