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

#[inline]
fn wants_chill() -> bool {
    false
}

pub fn run_ap_forever() -> ! {
    loop {
        crate::time::poll();
        poll_local_executor();
        if wants_chill() {
            hlt();
        } else {
            core::hint::spin_loop();
        }
    }
}

#[inline(always)]
fn hlt() {
    // todo signal to BSP that we need a wakeup at get woken up
    // todo first: dont use the param actually but just make clear
    // that its a forever hlt because our TODO firstofall is
    // to wantsChill() return the real number of tasks for the executor
    // and if therefore we need a simple cnt++ to each ap, we can
    #[cfg(target_arch = "x86_64")]
    unsafe {
        core::arch::asm!("sti; hlt", options(nomem, nostack));
    }
}
