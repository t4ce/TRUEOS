core::arch::global_asm!(
    r#"
    .global trueos_vmx_hull_text_start
trueos_vmx_hull_text_start:
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

    .global trueos_vmx_hull_text_end
trueos_vmx_hull_text_end:
"#
);

#[used]
#[unsafe(link_section = ".rodata.trueos_vm_hull")]
static HULL_RODATA_ANCHOR: [u8; 16] = *b"TRUEOS_VM_HULL\0\0";

#[unsafe(no_mangle)]
pub extern "C" fn trueos_vm_guest_run() {
    crate::demo::start();
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_vm_guest_idle() -> ! {
    unsafe { trueos_hv_guest_shell_run() }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_vm_preserve() {
    crate::vmcall::preserve();
}

unsafe extern "C" {
    #[link_name = "trueos_vmx_hull_text_start"]
    fn hull_text_start() -> !;
    #[link_name = "trueos_vmx_hull_text_end"]
    fn hull_text_end() -> !;
    #[link_name = "trueos_vmx_guest_entry"]
    pub fn entry() -> !;
    /// Defined in `src/hv/guest_run.rs`; starts a real shell2 instance over
    /// the vmcall I/O bridge using the already-live host heap and time driver.
    fn trueos_hv_guest_shell_run() -> !;
}

pub fn hull_image_bounds() -> (u64, u64) {
    let text_start = hull_text_start as *const () as u64;
    let text_end = hull_text_end as *const () as u64;
    let rodata = core::ptr::addr_of!(HULL_RODATA_ANCHOR) as u64;
    let bss_seq = crate::vmcall::hull_bss_anchor();
    let bss_stage = crate::vpanic::hull_bss_anchor();
    let bss_demo = crate::demo::hull_bss_anchor();

    let start = text_start.min(rodata);
    let end = text_end
        .max(rodata.saturating_add(HULL_RODATA_ANCHOR.len() as u64))
        .max(bss_seq.saturating_add(64))
        .max(bss_stage.saturating_add(64))
        .max(bss_demo.saturating_add(64));

    (start, end)
}
