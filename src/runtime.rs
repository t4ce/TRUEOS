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
        crate::time::poll();
        poll_local_executor();
        crate::lumen::burn_baby::poll_compute_lane();

        if counter.is_multiple_of(100_000) {
            crate::smp::poll();
        }
        if counter.is_multiple_of(5_000) {
            let slot = crate::percpu::this_cpu().cpu_index() as usize;
            if slot > 0 {
                let _ = crate::tst_ui2_coreticks_demo::ui2_coreticks_tick_tile_index(slot);
            }
        }
        counter = counter.wrapping_add(1);
        crate::power::idle_hint();
    }
}
