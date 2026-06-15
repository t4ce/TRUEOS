#[cfg(target_arch = "x86_64")]
use core::sync::atomic::{AtomicU64, Ordering};

#[cfg(target_arch = "x86_64")]
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

#[cfg(target_arch = "x86_64")]
pub(crate) const AP_WAKE_VECTOR: u8 = 0x41;

#[cfg(target_arch = "x86_64")]
static AP_WAKE_INTERRUPTS: AtomicU64 = AtomicU64::new(0);

#[cfg(target_arch = "x86_64")]
pub(crate) fn interrupt_install(idt: &mut InterruptDescriptorTable) {
    idt[AP_WAKE_VECTOR].set_handler_fn(ap_wake_isr);
}

#[allow(non_snake_case)]
#[cfg(target_arch = "x86_64")]
extern "x86-interrupt" fn ap_wake_isr(_stack_frame: InterruptStackFrame) {
    AP_WAKE_INTERRUPTS.fetch_add(1, Ordering::AcqRel);
}
