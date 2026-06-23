#[inline]
fn local_cpu() -> Option<&'static crate::percpu::PerCpu> {
    let cpu_ptr = crate::percpu::this_cpu_ptr();
    if cpu_ptr.is_null() {
        return None;
    }
    Some(unsafe { &*cpu_ptr })
}

#[inline]
fn local_executor() -> Option<&'static embassy_executor::raw::Executor> {
    let cpu = local_cpu()?;
    let ex_ptr = cpu.executor_ptr();
    if ex_ptr.is_null() {
        return None;
    }
    Some(unsafe { &*ex_ptr })
}

/// Poll the current CPU's executor once (if initialized).
#[inline]
pub fn poll_local_executor() {
    let Some(cpu) = local_cpu() else { return };
    let Some(executor) = local_executor() else {
        return;
    };

    if !cpu.try_enter_executor_poll() {
        return;
    }
    crate::executor_cache::warm_bsp_executor(cpu, executor);
    unsafe { executor.poll() };
    cpu.leave_executor_poll();
}

#[inline]
fn wants_chill(sleep_ticks: u64) -> Option<u64> {
    let executor = local_executor()?;
    if executor.ready_task_count() != 0 {
        return None;
    }

    if executor.spawned_task_count() == 0 {
        return Some(u64::MAX);
    }

    Some(sleep_ticks)
}

pub fn run_ap_forever() -> ! {
    loop {
        crate::time::poll();
        poll_local_executor();
        let sleep_ticks = crate::time::ticks_until_next_wake().unwrap_or(u64::MAX);

        disable_interrupts();
        if let Some(sleep_ticks) = wants_chill(sleep_ticks) {
            if try_sti_hlt(sleep_ticks) {
                continue;
            }
        }
        core::hint::spin_loop();
    }
}

#[inline(always)]
fn try_sti_hlt(sleep_ticks: u64) -> bool {
    let armed_timer = crate::chronos::arm_local_tsc_deadline_after_ticks(sleep_ticks);
    if sleep_ticks != u64::MAX && !armed_timer {
        return false;
    }

    crate::smp::mark_current_hlt_state(true);
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("sti; hlt", options(nomem, nostack));
    }
    disable_interrupts();

    if armed_timer {
        crate::chronos::disarm_local_timer();
    }
    crate::smp::mark_current_hlt_state(false);
    true
}

#[inline(always)]
fn disable_interrupts() {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
    }
}
