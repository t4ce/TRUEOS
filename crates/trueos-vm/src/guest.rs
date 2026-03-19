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
    jz .Ldo_vmcall
    mov dx, 0xE9
    out dx, al
    jmp .Lwrite_preserve

.Ldo_vmcall:
    mov eax, 1
    vmcall
    cli
.Lhalt:
    hlt
    jmp .Lhalt

.Lvmcr3:
    .asciz "VMCR3\n"
.Lpreserve:
    .asciz "VMPRESERVE\n"
"#
);

unsafe extern "C" {
    #[link_name = "trueos_vmx_guest_entry"]
    pub fn entry() -> !;
}
