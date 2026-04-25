// ARMTODO: Non-x86 builds still need full platform fault reporting. AArch64
// at least installs a minimal VBAR so early faults stop in a diagnosable place.

#[cfg(target_arch = "aarch64")]
use core::sync::atomic::{AtomicU64, Ordering};

#[derive(Copy, Clone, Debug)]
pub struct Frame {
    pub rbp: usize,
    pub rip: usize,
}

#[cfg(target_arch = "aarch64")]
static LAST_ESR_EL1: AtomicU64 = AtomicU64::new(0);
#[cfg(target_arch = "aarch64")]
static LAST_FAR_EL1: AtomicU64 = AtomicU64::new(0);
#[cfg(target_arch = "aarch64")]
static LAST_ELR_EL1: AtomicU64 = AtomicU64::new(0);
#[cfg(target_arch = "aarch64")]
static LAST_SPSR_EL1: AtomicU64 = AtomicU64::new(0);
#[cfg(target_arch = "aarch64")]
static LAST_LR: AtomicU64 = AtomicU64::new(0);
#[cfg(target_arch = "aarch64")]
static LAST_SP: AtomicU64 = AtomicU64::new(0);

pub(crate) fn init() {
    load_this_cpu();
}

#[cfg(target_arch = "aarch64")]
pub(crate) fn load_this_cpu() {
    unsafe {
        let base = aarch64_exception_vectors as *const () as usize;
        core::arch::asm!(
            "msr vbar_el1, {base}",
            "isb",
            base = in(reg) base,
            options(nostack, preserves_flags)
        );
    }
}

#[cfg(not(target_arch = "aarch64"))]
pub(crate) fn load_this_cpu() {}

#[cfg(target_arch = "aarch64")]
#[unsafe(no_mangle)]
#[unsafe(naked)]
unsafe extern "C" fn aarch64_exception_vectors() -> ! {
    core::arch::naked_asm!(
        ".balign 2048",
        ".rept 16",
        ".balign 128",
        "b {handler}",
        ".endr",
        handler = sym aarch64_exception_handler,
    );
}

#[cfg(target_arch = "aarch64")]
#[unsafe(no_mangle)]
extern "C" fn aarch64_exception_handler() -> ! {
    let esr: u64;
    let far: u64;
    let elr: u64;
    let spsr: u64;
    let lr: u64;
    let sp: u64;

    unsafe {
        core::arch::asm!(
            "mrs {esr}, esr_el1",
            "mrs {far}, far_el1",
            "mrs {elr}, elr_el1",
            "mrs {spsr}, spsr_el1",
            "mov {lr}, x30",
            "mov {sp}, sp",
            esr = out(reg) esr,
            far = out(reg) far,
            elr = out(reg) elr,
            spsr = out(reg) spsr,
            lr = out(reg) lr,
            sp = out(reg) sp,
            options(nomem, nostack, preserves_flags)
        );
    }

    LAST_ESR_EL1.store(esr, Ordering::Release);
    LAST_FAR_EL1.store(far, Ordering::Release);
    LAST_ELR_EL1.store(elr, Ordering::Release);
    LAST_SPSR_EL1.store(spsr, Ordering::Release);
    LAST_LR.store(lr, Ordering::Release);
    LAST_SP.store(sp, Ordering::Release);

    loop {
        core::hint::spin_loop();
    }
}

pub fn collect_backtrace(_max_frames: usize) -> heapless::Vec<Frame, 64> {
    heapless::Vec::new()
}

pub fn print_backtrace(_max_frames: usize) {}
