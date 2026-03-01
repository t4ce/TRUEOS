use embassy_executor::{SendSpawner, Spawner};
use spin::Mutex;

static FIRST_AP_SPAWNER: Mutex<Option<SendSpawner>> = Mutex::new(None);

/// Register a spawner for the first AP (CPU slot 1).
#[inline]
pub fn register_first_ap_spawner(spawner: Spawner) {
    let mut guard = FIRST_AP_SPAWNER.lock();
    if guard.is_none() {
        *guard = Some(spawner.make_send());
    }
}

/// Return the first AP spawner if that AP is online and initialized.
#[inline]
pub fn first_ap_spawner() -> Option<SendSpawner> {
    *FIRST_AP_SPAWNER.lock()
}

#[inline]
fn local_cpu_ptr() -> *mut crate::percpu::PerCpu {
    let cpu_ptr = crate::percpu::this_cpu_ptr();
    if cpu_ptr.is_null() {
        return core::ptr::null_mut();
    }
    cpu_ptr
}

/// Poll the current CPU's executor once (if initialized).
#[inline]
pub fn poll_local_executor() {
    let cpu_ptr = local_cpu_ptr();
    if cpu_ptr.is_null() {
        return;
    }

    let cpu = unsafe { &*cpu_ptr };
    let ex_ptr = cpu.executor_ptr();
    if ex_ptr.is_null() {
        return;
    }

    if !cpu.try_enter_executor_poll() {
        return;
    }
    unsafe { (&*ex_ptr).poll() };
    cpu.leave_executor_poll();
}

pub fn run_ap_forever() -> ! {
    let mut counter: u64 = 0;
    loop {
        if counter.is_multiple_of(100_000) {
            crate::wait::spin_step();
            crate::smp::poll();
        }
        if counter.is_multiple_of(500_000) {
            let slot = crate::percpu::this_cpu().cpu_index() as usize;
            let total = crate::smp::cpu_count().max(1);
            let outline = match crate::cpu::intel_core_kind_hint() {
                trueos_qjs::workers::CORE_KIND_PERF => 0x00_FF_37_FF, // 255,55,255
                _ => 0x00_FF_FF_FF,
            };
            crate::vga::draw_header_square(
                total,
                slot,
                crate::vga::DEFAULT_SHADOW_COLOR,
                outline,
                (counter % 360) as u32,
            );
        }
        counter = counter.wrapping_add(1);
    }
}
