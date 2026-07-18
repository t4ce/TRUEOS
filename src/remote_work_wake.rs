use core::arch::x86_64::__cpuid;
use core::sync::atomic::{AtomicU64, Ordering};

use x86_64::registers::model_specific::Msr;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

pub(crate) const REMOTE_WORK_WAKE_VECTOR: u8 = 0x41;
const AP_SPURIOUS_VECTOR: u8 = 0xFF;

static REMOTE_WORK_WAKE_INTERRUPTS: AtomicU64 = AtomicU64::new(0);
static REMOTE_WORK_WAKE_REQUESTS: AtomicU64 = AtomicU64::new(0);
static REMOTE_WORK_WAKE_SENT: AtomicU64 = AtomicU64::new(0);
static REMOTE_WORK_WAKE_FAILED: AtomicU64 = AtomicU64::new(0);
static AP_SPURIOUS_INTERRUPTS: AtomicU64 = AtomicU64::new(0);

const MSR_IA32_APIC_BASE: u32 = 0x0000_001B;
const MSR_IA32_X2APIC_EOI: u32 = 0x0000_080B;
const MSR_IA32_X2APIC_SIVR: u32 = 0x0000_080F;
const MSR_IA32_X2APIC_ICR: u32 = 0x0000_0830;
const APIC_BASE_ENABLE: u64 = 1 << 11;
const APIC_BASE_X2APIC_ENABLE: u64 = 1 << 10;
const X2APIC_SIVR_SOFTWARE_ENABLE: u64 = 1 << 8;

pub(crate) fn interrupt_install(idt: &mut InterruptDescriptorTable) {
    idt[REMOTE_WORK_WAKE_VECTOR].set_handler_fn(remote_work_wake_isr);
    idt[AP_SPURIOUS_VECTOR].set_handler_fn(ap_spurious_isr);
}

pub(crate) fn enable_local_apic_for_this_cpu() -> bool {
    if !cpu_supports_x2apic() {
        return false;
    }

    unsafe {
        let apic_base = Msr::new(MSR_IA32_APIC_BASE).read();
        Msr::new(MSR_IA32_APIC_BASE).write(apic_base | APIC_BASE_ENABLE | APIC_BASE_X2APIC_ENABLE);

        let sivr = Msr::new(MSR_IA32_X2APIC_SIVR).read();
        Msr::new(MSR_IA32_X2APIC_SIVR)
            .write((sivr & !0xFF) | AP_SPURIOUS_VECTOR as u64 | X2APIC_SIVR_SOFTWARE_ENABLE);
    }

    true
}

fn cpu_supports_x2apic() -> bool {
    let cpuid = __cpuid(1);
    let has_apic = (cpuid.edx & (1 << 9)) != 0;
    let has_x2apic = (cpuid.ecx & (1 << 21)) != 0;
    has_apic && has_x2apic
}

pub(crate) fn wake_cpu_for_remote_work(cpu_slot: u32) -> bool {
    REMOTE_WORK_WAKE_REQUESTS.fetch_add(1, Ordering::AcqRel);

    let cpu_ptr = crate::percpu::try_this_cpu_ptr();
    if !cpu_ptr.is_null() && unsafe { (*cpu_ptr).cpu_index() == cpu_slot } {
        return true;
    }

    let Some(lapic_id) = crate::percpu::cpu_slots()
        .iter()
        .find(|slot| slot.slot == cpu_slot)
        .map(|slot| slot.lapic_id)
    else {
        REMOTE_WORK_WAKE_FAILED.fetch_add(1, Ordering::AcqRel);
        return false;
    };

    if send_remote_work_x2apic_wake(lapic_id) {
        REMOTE_WORK_WAKE_SENT.fetch_add(1, Ordering::AcqRel);
        true
    } else {
        REMOTE_WORK_WAKE_FAILED.fetch_add(1, Ordering::AcqRel);
        false
    }
}

#[unsafe(export_name = "__trueos_embassy_pender")]
pub extern "Rust" fn trueos_embassy_pender(context: *mut ()) {
    let cpu_slot = context as usize;
    if cpu_slot == 0 {
        return;
    }

    let _ = wake_cpu_for_remote_work(cpu_slot as u32);
}

fn send_remote_work_x2apic_wake(lapic_id: u32) -> bool {
    if !local_x2apic_enabled() {
        return false;
    }

    let icr = ((lapic_id as u64) << 32) | REMOTE_WORK_WAKE_VECTOR as u64;
    unsafe {
        Msr::new(MSR_IA32_X2APIC_ICR).write(icr);
    }
    true
}

fn local_x2apic_enabled() -> bool {
    let apic_base = unsafe { Msr::new(MSR_IA32_APIC_BASE).read() };
    (apic_base & APIC_BASE_ENABLE) != 0 && (apic_base & APIC_BASE_X2APIC_ENABLE) != 0
}

#[inline]
fn local_eoi() {
    unsafe {
        if local_x2apic_enabled() {
            Msr::new(MSR_IA32_X2APIC_EOI).write(0);
        }
    }
}

#[allow(non_snake_case)]
extern "x86-interrupt" fn remote_work_wake_isr(_stack_frame: InterruptStackFrame) {
    REMOTE_WORK_WAKE_INTERRUPTS.fetch_add(1, Ordering::AcqRel);
    local_eoi();
}

#[allow(non_snake_case)]
extern "x86-interrupt" fn ap_spurious_isr(_stack_frame: InterruptStackFrame) {
    AP_SPURIOUS_INTERRUPTS.fetch_add(1, Ordering::AcqRel);
}
