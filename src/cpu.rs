use crate::{exceptions, globalog, percpu, runtime};
use ::limine::mp::Cpu as LimineCpu;
use alloc::vec::Vec;
use core::arch::x86_64::__cpuid;
use core::ptr::null_mut;
use core::sync::atomic::{AtomicPtr, AtomicU8, AtomicU32, AtomicUsize, Ordering};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};

const AP_HEARTBEAT_TASK_POOL: usize = 256;
static ATOMIC_BOMB_RESTARTS: AtomicU32 = AtomicU32::new(0);

#[repr(C, align(64))]
struct CpuProfileRecord {
    registered: AtomicU8,
    core_kind: AtomicU8,
    lapic_id: AtomicU32,
}

impl CpuProfileRecord {
    const fn new() -> Self {
        Self {
            registered: AtomicU8::new(0),
            core_kind: AtomicU8::new(trueos_qjs::workers::CORE_KIND_UNKNOWN),
            lapic_id: AtomicU32::new(0),
        }
    }
}

static CPU_PROFILE_PTR: AtomicPtr<CpuProfileRecord> = AtomicPtr::new(null_mut());
static CPU_PROFILE_LEN: AtomicUsize = AtomicUsize::new(0);
static CPU_PROFILE_INIT: spin::Once<()> = spin::Once::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuProfile {
    slot: u32,
    lapic_id: u32,
    core_kind: u8,
}

#[allow(dead_code)]
impl CpuProfile {
    pub const fn new(slot: u32, lapic_id: u32, core_kind: u8) -> Self {
        Self {
            slot,
            lapic_id,
            core_kind,
        }
    }

    pub fn current() -> Option<Self> {
        let cpu_ptr = percpu::this_cpu_ptr();
        if cpu_ptr.is_null() {
            return None;
        }

        let cpu = unsafe { &*cpu_ptr };
        Self::for_slot(cpu.cpu_index())
            .or_else(|| Some(Self::new(cpu.cpu_index(), cpu.lapic_id(), intel_core_kind_hint())))
    }

    pub fn for_slot(slot: u32) -> Option<Self> {
        let rec = profile_record(slot as usize)?;
        if rec.registered.load(Ordering::Acquire) == 0 {
            return None;
        }

        Some(Self {
            slot,
            lapic_id: rec.lapic_id.load(Ordering::Acquire),
            core_kind: rec.core_kind.load(Ordering::Acquire),
        })
    }

    pub fn for_lapic_id(lapic_id: u32) -> Option<Self> {
        let slot = percpu::slot_for_lapic_id(lapic_id) as u32;
        let profile = Self::for_slot(slot)?;
        if profile.lapic_id == lapic_id {
            Some(profile)
        } else {
            None
        }
    }

    pub const fn slot(self) -> u32 {
        self.slot
    }

    pub const fn lapic_id(self) -> u32 {
        self.lapic_id
    }

    pub const fn core_kind(self) -> u8 {
        self.core_kind
    }

    pub fn core_kind_name(self) -> &'static str {
        match self.core_kind {
            trueos_qjs::workers::CORE_KIND_PERF => "perf",
            trueos_qjs::workers::CORE_KIND_EFF => "eff",
            _ => "unknown",
        }
    }

    pub const fn is_bsp(self) -> bool {
        self.slot == 0
    }

    pub const fn is_perf(self) -> bool {
        self.core_kind == trueos_qjs::workers::CORE_KIND_PERF
    }

    pub const fn is_eff(self) -> bool {
        self.core_kind == trueos_qjs::workers::CORE_KIND_EFF
    }

    pub fn register_worker_spawner(self, spawner: Spawner) {
        trueos_qjs::workers::register_core_spawner(self.slot, self.core_kind, spawner);
    }
}

pub fn init_profiles(total_slots: usize) {
    CPU_PROFILE_INIT.call_once(|| {
        if total_slots == 0 {
            return;
        }

        let mut records: Vec<CpuProfileRecord> = Vec::with_capacity(total_slots);
        for _ in 0..total_slots {
            records.push(CpuProfileRecord::new());
        }

        let mut boxed = records.into_boxed_slice();
        let ptr = boxed.as_mut_ptr();
        let len = boxed.len();
        core::mem::forget(boxed);

        CPU_PROFILE_PTR.store(ptr, Ordering::Release);
        CPU_PROFILE_LEN.store(len, Ordering::Release);
    });
}

pub fn register_current_profile() -> Option<CpuProfile> {
    let cpu_ptr = percpu::this_cpu_ptr();
    if cpu_ptr.is_null() {
        return None;
    }

    let cpu = unsafe { &*cpu_ptr };
    let profile = CpuProfile::new(cpu.cpu_index(), cpu.lapic_id(), intel_core_kind_hint());
    store_profile(profile);
    Some(profile)
}

pub fn register_current_worker_spawner(spawner: Spawner) -> Option<CpuProfile> {
    let profile = register_current_profile()?;
    profile.register_worker_spawner(spawner);
    Some(profile)
}

fn enter_ap_runtime(spawner: Spawner) -> ! {
    let profile = register_current_worker_spawner(spawner)
        .unwrap_or_else(|| CpuProfile::current().unwrap_or(CpuProfile::new(0, 0, 0)));

    if let Err(e) = spawner.spawn(ap_heartbeat_task()) {
        crate::log!("ap: heartbeat task spawn failed: {:?}\n", e);
    }
    crate::smp::mark_online();
    exceptions::load_this_cpu();
    runtime::run_ap_forever()
}

#[embassy_executor::task]
async fn atomic_bomb_after_restart_task(restart_count: u32) {
    Timer::after(EmbassyDuration::from_secs(2)).await;

    if let Some(profile) = CpuProfile::current() {
        crate::log!(
            "PANIC PANIC PANIC: atomic_bomb post-restart work slot={} lapic={} restart_count={}\n",
            profile.slot(),
            profile.lapic_id(),
            restart_count
        );
    } else {
        crate::log!(
            "PANIC PANIC PANIC: atomic_bomb post-restart work on unknown cpu restart_count={}\n",
            restart_count
        );
    }

    if restart_count == 1 {
        Timer::after(EmbassyDuration::from_secs(1)).await;
        panic!("PANIC PANIC PANIC: atomic_bomb second strike after restart");
    }

    crate::log!(
        "PANIC PANIC PANIC: atomic_bomb stabilized after restart_count={}\n",
        restart_count
    );
}

pub fn can_restart_current_worker_ap_from_panic() -> bool {
    CpuProfile::current()
        .map(|profile| profile.slot() >= 2)
        .unwrap_or(false)
}

pub fn restart_current_worker_ap_from_panic() -> ! {
    unsafe { enable_sse() };

    let cpu = percpu::this_cpu();
    let slot = cpu.cpu_index();
    let lapic_id = cpu.lapic_id();

    crate::log!("PANIC PANIC PANIC: restarting worker ap slot={} lapic={}\n", slot, lapic_id);

    percpu::init_ap(lapic_id, slot);
    let ex = percpu::init_executor();
    let spawner = ex.spawner();
    let restart_count = ATOMIC_BOMB_RESTARTS.fetch_add(1, Ordering::AcqRel) + 1;
    if let Err(e) = spawner.spawn(atomic_bomb_after_restart_task(restart_count)) {
        crate::log!("PANIC PANIC PANIC: failed to spawn atomic_bomb post-restart task: {:?}\n", e);
    }
    enter_ap_runtime(spawner)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn ap_start(cpu: &LimineCpu) -> ! {
    enable_sse();
    let slot = percpu::slot_for_lapic_id(cpu.lapic_id);
    percpu::init_ap(cpu.lapic_id, slot as u32);
    let ex = percpu::init_executor();
    let spawner = ex.spawner();
    enter_ap_runtime(spawner)
}

pub(crate) fn intel_core_kind_hint() -> u8 {
    detect_current_core_kind()
}

fn detect_current_core_kind() -> u8 {
    let r0 = __cpuid(0);
    let max = r0.eax;
    if max < 0x1A {
        return trueos_qjs::workers::CORE_KIND_UNKNOWN;
    }
    let r = __cpuid(0x1A);
    let core_type = (r.eax >> 24) as u8;
    match core_type {
        0x40 => trueos_qjs::workers::CORE_KIND_PERF,
        0x20 => trueos_qjs::workers::CORE_KIND_EFF,
        _ => trueos_qjs::workers::CORE_KIND_UNKNOWN,
    }
}

fn profile_record(slot: usize) -> Option<&'static CpuProfileRecord> {
    let ptr = CPU_PROFILE_PTR.load(Ordering::Acquire);
    let len = CPU_PROFILE_LEN.load(Ordering::Acquire);
    if ptr.is_null() || slot >= len {
        return None;
    }
    Some(unsafe { &*ptr.add(slot) })
}

fn store_profile(profile: CpuProfile) {
    let Some(rec) = profile_record(profile.slot as usize) else {
        return;
    };

    rec.lapic_id.store(profile.lapic_id, Ordering::Release);
    rec.core_kind.store(profile.core_kind, Ordering::Release);
    rec.registered.store(1, Ordering::Release);
}

#[embassy_executor::task(pool_size = AP_HEARTBEAT_TASK_POOL)]
async fn ap_heartbeat_task() {
    loop {
        Timer::after(EmbassyDuration::from_secs(5)).await;
        let slot = percpu::this_cpu().cpu_index() as u8;
        let mark = if slot < 10 {
            b'0' + slot
        } else {
            b'A' + ((slot - 10) % 26)
        };
        globalog::debugcon_write_byte_raw(mark);
    }
}

pub unsafe fn enable_sse() {
    let mut cr0 = Cr0::read();
    cr0.remove(Cr0Flags::EMULATE_COPROCESSOR);
    cr0.remove(Cr0Flags::TASK_SWITCHED);
    cr0.insert(Cr0Flags::MONITOR_COPROCESSOR | Cr0Flags::NUMERIC_ERROR);
    Cr0::write(cr0);

    let mut cr4 = Cr4::read();
    cr4.insert(Cr4Flags::OSFXSR | Cr4Flags::OSXMMEXCPT_ENABLE);
    Cr4::write(cr4);

    // Reset the x87 and SSE control state so newly started cores begin from a
    // known-good environment before any C/Rust/QuickJS float code runs.
    core::arch::asm!("fninit", options(nostack, preserves_flags));
    let mxcsr: u32 = 0x1F80;
    core::arch::asm!(
        "ldmxcsr [{mxcsr_ptr}]",
        mxcsr_ptr = in(reg) &mxcsr,
        options(nostack, preserves_flags, readonly),
    );
}

#[inline(always)]
pub(crate) fn long_mode_active() -> bool {
    use x86_64::registers::model_specific::Msr;
    const IA32_EFER: u32 = 0xC000_0080;
    const EFER_LMA_BIT: u64 = 1 << 10;
    let efer = unsafe { Msr::new(IA32_EFER).read() };
    (efer & EFER_LMA_BIT) != 0
}
