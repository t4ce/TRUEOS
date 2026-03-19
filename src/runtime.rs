use core::sync::atomic::Ordering;

#[inline]
fn local_cpu_ptr() -> *mut crate::percpu::PerCpu {
    let cpu_ptr = crate::percpu::this_cpu_ptr();
    if cpu_ptr.is_null() {
        return core::ptr::null_mut();
    }
    cpu_ptr
}

#[inline]
fn ap_slot_mark(slot: u32) -> u8 {
    if slot < 10 {
        b'0' + slot as u8
    } else {
        b'A' + ((slot as u8 - 10) % 26)
    }
}

#[inline]
fn log_ap_activity_once() {
    let slot = crate::percpu::this_cpu().cpu_index();
    if slot >= 64 {
        return;
    }

    let mask = 1u64 << slot;
    if crate::logflag::AP_ACTIVITY_LOGGED.fetch_or(mask, Ordering::AcqRel) & mask == 0 {
        crate::log!(
            "ap: juiced slot={} mark={}\n",
            slot,
            ap_slot_mark(slot) as char
        );
    }
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
        log_ap_activity_once();

        if counter.is_multiple_of(100_000) {
            crate::smp::poll();
        }
        if counter.is_multiple_of(500_000) {
            let slot = crate::percpu::this_cpu().cpu_index() as usize;
            let total = crate::smp::cpu_count().max(1);
            let outline = if crate::cpu::CpuProfile::current()
                .map(|profile| profile.is_perf())
                .unwrap_or(false)
            {
                0x00_FF_37_FF // 255,55,255
            } else {
                0x00_FF_FF_FF
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
        crate::power::idle_hint();
    }
}
