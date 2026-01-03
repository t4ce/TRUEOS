use core::arch::asm;

use alloc::boxed::Box;
use x86_64::registers::model_specific::Msr;

const MSR_IA32_GS_BASE: u32 = 0xC000_0101;

#[repr(C)]
pub struct PerCpu {
    self_ptr: *mut PerCpu,
    lapic_id: u32,
    cpu_index: u32,
}

impl PerCpu {
    #[inline(always)]
    pub fn lapic_id(&self) -> u32 {
        self.lapic_id
    }

    #[inline(always)]
    pub fn cpu_index(&self) -> u32 {
        self.cpu_index
    }
}

pub fn init_bsp() {
    let lapic_id = read_lapic_id_via_cpuid();
    init_with(lapic_id, 0, "bsp")
}

pub fn init_ap(lapic_id: u32, cpu_index: u32) {
    init_with(lapic_id, cpu_index, "ap")
}

#[inline(always)]
fn init_with(lapic_id: u32, cpu_index: u32, tag: &str) {
    let mut percpu = Box::new(PerCpu {
        self_ptr: core::ptr::null_mut(),
        lapic_id,
        cpu_index,
    });

    let ptr: *mut PerCpu = &mut *percpu;
    percpu.self_ptr = ptr;

    let _leaked: &'static mut PerCpu = Box::leak(percpu);

    let mut gs_base = Msr::new(MSR_IA32_GS_BASE);
    unsafe { gs_base.write(ptr as u64) };

    crate::log!(
        "0x{:016X} lapic={} cpu={}\n",
        ptr as u64,
        lapic_id,
        cpu_index
    );
}

#[inline(always)]
pub fn this_cpu() -> &'static PerCpu {
    unsafe { &*this_cpu_ptr() }
}

#[inline(always)]
pub fn this_cpu_ptr() -> *mut PerCpu {
    let ptr: *mut PerCpu;
    unsafe {
        asm!(
            "mov {0}, gs:0",
            out(reg) ptr,
            options(nostack, preserves_flags)
        );
    }
    ptr
}

#[inline(always)]
fn read_lapic_id_via_cpuid() -> u32 {
    let cpuid = raw_cpuid::CpuId::new();
    cpuid
        .get_feature_info()
        .map(|f| f.initial_local_apic_id() as u32)
        .unwrap_or(0)
}
