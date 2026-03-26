use core::sync::atomic::Ordering;

#[path = "marble2/apmarble_lane.rs"]
mod apmarble_lane;

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

#[inline]
fn poll_apmarble_lane() {
    let total = crate::smp::cpu_count();
    let slot = crate::percpu::this_cpu().cpu_index();
    apmarble_lane::poll_for_current_slot(slot, total);
}

pub fn run_ap_forever() -> ! {
    let mut counter: u64 = 0;
    loop {
        crate::time::poll();
        poll_local_executor();
        poll_apmarble_lane();
        log_ap_activity_once();

        if counter.is_multiple_of(100_000) {
            crate::smp::poll();
        }
        if counter.is_multiple_of(500_000) {
            let _slot = crate::percpu::this_cpu().cpu_index() as usize;
            let _total = crate::smp::cpu_count().max(1);
            let _outline = if crate::cpu::CpuProfile::current()
                .map(|profile| profile.is_perf())
                .unwrap_or(false)
            {
                0x00_FF_37_FF // 255,55,255
            } else {
                0x00_FF_FF_FF
            }; // actually we could color code turbo normal and marble
        }
        counter = counter.wrapping_add(1);
        crate::power::idle_hint();
    }
}
