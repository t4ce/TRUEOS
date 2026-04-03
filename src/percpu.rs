use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};

use alloc::boxed::Box;
use alloc::vec::Vec;
use embassy_executor::raw::Executor as RawExecutor;
use x86_64::registers::model_specific::Msr;

const MSR_IA32_GS_BASE: u32 = 0xC000_0101;
static TOTAL_SLOTS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static CPU_SLOT_TABLE: core::sync::atomic::AtomicPtr<CpuSlot> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());
static CPU_SLOT_LEN: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

#[repr(C)]
#[derive(Copy, Clone)]
pub struct CpuSlot {
    pub lapic_id: u32,
    pub slot: u32,
}

#[repr(C)]
pub struct PerCpu {
    self_ptr: *mut PerCpu,
    lapic_id: u32,
    cpu_index: u32,
    executor: *mut RawExecutor,
    executor_polling: AtomicBool,
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

    #[inline(always)]
    pub fn set_executor_ptr(&mut self, ptr: *mut RawExecutor) {
        self.executor = ptr;
    }

    #[inline(always)]
    pub fn executor_ptr(&self) -> *mut RawExecutor {
        self.executor
    }

    #[inline(always)]
    pub fn try_enter_executor_poll(&self) -> bool {
        !self.executor_polling.swap(true, Ordering::AcqRel)
    }

    #[inline(always)]
    pub fn leave_executor_poll(&self) {
        self.executor_polling.store(false, Ordering::Release);
    }
}

pub fn init_bsp() {
    let lapic_id = read_lapic_id_via_cpuid();
    init_with(lapic_id, 0, "bsp")
}

pub fn init_ap(lapic_id: u32, cpu_index: u32) {
    init_with(lapic_id, cpu_index, "ap")
}

pub fn init_executor() -> &'static mut RawExecutor {
    let executor = Box::leak(Box::new(RawExecutor::new(core::ptr::null_mut())));
    unsafe {
        (&mut *this_cpu_ptr()).set_executor_ptr(executor as *mut RawExecutor);
    }
    executor
}

#[inline]
pub fn total_slots() -> usize {
    TOTAL_SLOTS.load(Ordering::Acquire)
}

#[inline]
pub fn cpu_slots() -> &'static [CpuSlot] {
    cpu_slot_table()
}

#[inline]
fn cpu_slot_table() -> &'static [CpuSlot] {
    let len = CPU_SLOT_LEN.load(Ordering::Acquire);
    let ptr = CPU_SLOT_TABLE.load(Ordering::Acquire);
    if ptr.is_null() || len == 0 {
        return &[];
    }
    unsafe { core::slice::from_raw_parts(ptr, len) }
}

#[inline]
pub fn slot_for_lapic_id(lapic_id: u32) -> usize {
    let slots = cpu_slot_table();
    if !slots.is_empty() {
        for entry in slots {
            if entry.lapic_id == lapic_id {
                return entry.slot as usize;
            }
        }
    }
    let total = total_slots();
    if total == 0 {
        0
    } else {
        (lapic_id as usize) % total
    }
}

pub fn install_cpu_slot_lapic_order_owned(lapic_ids: Vec<u32>) {
    let mut slots: Vec<CpuSlot> = Vec::with_capacity(lapic_ids.len());
    for lapic_id in lapic_ids {
        if slots.iter().any(|s| s.lapic_id == lapic_id) {
            continue;
        }
        let slot = slots.len() as u32;
        slots.push(CpuSlot { lapic_id, slot });
    }

    let len = slots.len();
    let mut boxed = slots.into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    core::mem::forget(boxed);
    CPU_SLOT_TABLE.store(ptr, Ordering::Release);
    CPU_SLOT_LEN.store(len, Ordering::Release);
    TOTAL_SLOTS.store(len, Ordering::Release);
}

#[inline(always)]
fn init_with(lapic_id: u32, cpu_index: u32, _tag: &str) {
    let mut percpu = Box::new(PerCpu {
        self_ptr: core::ptr::null_mut(),
        lapic_id,
        cpu_index,
        executor: core::ptr::null_mut(),
        executor_polling: AtomicBool::new(false),
    });

    let ptr: *mut PerCpu = &mut *percpu;
    percpu.self_ptr = ptr;

    let _leaked: &'static mut PerCpu = Box::leak(percpu);

    let mut gs_base = Msr::new(MSR_IA32_GS_BASE);
    unsafe { gs_base.write(ptr as u64) };

    if crate::logflag::BOOT_INFO_LOGS {
        crate::log!("0x{:016X} lapic={} cpu={}\n", ptr as u64, lapic_id, cpu_index);
    }
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
