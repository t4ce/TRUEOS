#[inline]
fn local_cpu() -> Option<&'static crate::percpu::PerCpu> {
    let cpu_ptr = crate::percpu::this_cpu_ptr();
    if cpu_ptr.is_null() {
        return None;
    }
    Some(unsafe { &*cpu_ptr })
}

#[inline]
fn local_executor(
    cpu: &'static crate::percpu::PerCpu,
) -> Option<&'static trueos_executor::raw::Executor> {
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
    let Some(executor) = local_executor(cpu) else {
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
    let executor = local_executor(local_cpu()?)?;
    if executor.ready_task_count() != 0 {
        return None;
    }

    if executor.spawned_task_count() == 0 {
        return Some(u64::MAX);
    }

    Some(sleep_ticks)
}

/// Park a synchronous service-lane caller only while no other task is ready on
/// this AP's executor.
///
/// The caller must publish whatever wake routing it needs before calling this
/// function and use `still_waiting` for the final lost-wake check. Interrupts
/// are disabled before the executor/readiness checks, so a remote enqueue or
/// explicit wait-queue IPI racing after them remains pending across the atomic
/// `sti; hlt` window.
///
/// This is deliberately not an executor re-entry point. The currently polled
/// synchronous carrier remains on the stack; another ready task makes this
/// return `false` so callers can retain the existing non-reentrant fallback.
pub(crate) fn park_local_executor_blocking_if_idle(
    sleep_ticks: u64,
    still_waiting: impl FnOnce() -> bool,
) -> bool {
    if sleep_ticks == 0 {
        return false;
    }

    let interrupts_were_enabled = interrupts_enabled();
    disable_interrupts();
    let can_park = wants_chill(sleep_ticks).is_some() && still_waiting();
    let parked = can_park && try_sti_hlt(sleep_ticks);
    if interrupts_were_enabled {
        enable_interrupts();
    }
    parked
}

pub fn run_ap_forever() -> ! {
    loop {
        crate::live_update::poll_ap_transition_safe_point();
        crate::time::poll();
        poll_local_executor();
        crate::live_update::poll_ap_transition_safe_point();
        let sleep_ticks = crate::time::ticks_until_next_wake().unwrap_or(u64::MAX);
        crate::power::thermal::poll_current_core_passive(sleep_ticks);

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
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
    }
}

#[inline(always)]
fn enable_interrupts() {
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack, preserves_flags));
    }
}

#[inline(always)]
fn interrupts_enabled() -> bool {
    let flags: u64;
    unsafe {
        core::arch::asm!(
            "pushfq",
            "pop {}",
            out(reg) flags,
            options(preserves_flags)
        );
    }
    flags & (1 << 9) != 0
}
