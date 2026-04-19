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
    if unsafe { trueos_hv_guest_blueprint_run() } {
        crate::vmcall::preserve();
        loop {
            core::hint::spin_loop();
        }
    }

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
    #[link_name = "__text_start"]
    fn image_text_start() -> !;
    #[link_name = "__text_end"]
    fn image_text_end() -> !;
    #[link_name = "__rodata_start"]
    fn image_rodata_start() -> !;
    #[link_name = "__rodata_end"]
    fn image_rodata_end() -> !;
    #[link_name = "__data_start"]
    fn image_data_start() -> !;
    #[link_name = "__data_end"]
    fn image_data_end() -> !;
    #[link_name = "__bss_start"]
    fn image_bss_start() -> !;
    #[link_name = "__bss_end"]
    fn image_bss_end() -> !;
    #[link_name = "trueos_vmx_hull_text_start"]
    fn hull_text_start() -> !;
    #[link_name = "trueos_vmx_hull_text_end"]
    fn hull_text_end() -> !;
    #[link_name = "trueos_vmx_guest_entry"]
    pub fn entry() -> !;
    /// Defined in `src/hv/guest_run.rs`; executes a staged blueprint launch inside
    /// the hull and returns `true` when the hull should preserve/stop afterwards.
    fn trueos_hv_guest_blueprint_run() -> bool;
    /// Defined in `src/hv/guest_run.rs`; starts a real shell2 instance over
    /// the vmcall I/O bridge using the already-live host heap and time driver.
    fn trueos_hv_guest_shell_run() -> !;
}

#[derive(Copy, Clone, Debug)]
pub struct HullImageLayout {
    pub text_start: u64,
    pub text_end: u64,
    pub rodata_start: u64,
    pub rodata_end: u64,
    pub data_start: u64,
    pub data_end: u64,
    pub vmcall_bss_start: u64,
    pub vmcall_bss_end: u64,
    pub vpanic_bss_start: u64,
    pub vpanic_bss_end: u64,
    pub demo_bss_start: u64,
    pub demo_bss_end: u64,
    pub bss_start: u64,
    pub bss_end: u64,
}

pub fn hull_image_layout() -> HullImageLayout {
    let text_start = image_text_start as *const () as u64;
    let text_end = image_text_end as *const () as u64;
    let rodata_start = image_rodata_start as *const () as u64;
    let rodata_end = image_rodata_end as *const () as u64;
    let data_start = image_data_start as *const () as u64;
    let data_end = image_data_end as *const () as u64;

    let vmcall_bss_start = crate::vmcall::hull_bss_anchor();
    let vmcall_bss_end = vmcall_bss_start.saturating_add(64);
    let vpanic_bss_start = crate::vpanic::hull_bss_anchor();
    let vpanic_bss_end = vpanic_bss_start.saturating_add(64);
    let demo_bss_start = crate::demo::hull_bss_anchor();
    let demo_bss_end = demo_bss_start.saturating_add(64);
    let bss_start = image_bss_start as *const () as u64;
    let bss_end = image_bss_end as *const () as u64;

    HullImageLayout {
        text_start,
        text_end,
        rodata_start,
        rodata_end,
        data_start,
        data_end,
        vmcall_bss_start,
        vmcall_bss_end,
        vpanic_bss_start,
        vpanic_bss_end,
        demo_bss_start,
        demo_bss_end,
        bss_start,
        bss_end,
    }
}

pub fn hull_image_bounds() -> (u64, u64) {
    let layout = hull_image_layout();
    let start = layout
        .text_start
        .min(layout.rodata_start)
        .min(layout.data_start)
        .min(layout.bss_start);
    let end = layout
        .text_end
        .max(layout.rodata_end)
        .max(layout.data_end)
        .max(layout.bss_end);

    (start, end)
}
