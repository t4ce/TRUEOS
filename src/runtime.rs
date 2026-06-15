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
            hlt(sleep_ticks);
        } else {
            enable_interrupts();
            core::hint::spin_loop();
        }
    }
}

#[inline(always)]
fn hlt(_sleep_ticks: u64) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("sti; hlt", options(nomem, nostack));
    }
}

#[inline(always)]
fn disable_interrupts() {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
    }
}

#[inline(always)]
fn enable_interrupts() {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
    }
}
