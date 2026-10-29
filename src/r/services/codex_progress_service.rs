//! Read-only observer on the BSP, outside every Codex application carrier.
//! One sample emits at most 25 records regardless of the number of VMs/APs.
use crate::r::{blocking::diagnostics, input_timing::now_ms, progress_probe};
use trueos_time::{Duration, Timer};

const PERIOD_MS: u64 = 5_000;
const LINES_PER_CATEGORY: usize = 4;

#[trueos_executor::task]
pub(crate) async fn codex_progress_task() {
    let mut previous = [progress_probe::Snapshot::default(); crate::allcaps::hv::VM_CPU_SLOT_LIMIT];
    let mut previous_ms = [now_ms(); crate::allcaps::hv::VM_CPU_SLOT_LIMIT];
    let mut carrier_cursor = 0;
    let mut timing_cursor = 0;
    let mut idle_timing_sweep = diagnostics::TimingSweep::new();
    let mut job_cursor = 0;
    let mut allocator_cursor = 0;
    let mut had_active = false;
    let mut previous_retirements = 0;
    let mut sample = 0u64;
    loop {
        Timer::after(Duration::from_millis(PERIOD_MS)).await;
        let carriers = diagnostics::progress_carriers();
        let active = carriers.active.iter().filter(|active| **active).count();
        let retirements = diagnostics::timing_retirements();
        // Drain every aggregate bucket after retirement, even when more than
        // four short-lived jobs finish before the observer's first sample.
        if retirements != previous_retirements || (active == 0 && had_active) {
            idle_timing_sweep.restart();
        }
        previous_retirements = retirements;
        let timing_lines = if active == 0 {
            idle_timing_sweep.lines(&mut timing_cursor, LINES_PER_CATEGORY)
        } else {
            diagnostics::timing_lines(&mut timing_cursor, LINES_PER_CATEGORY)
        };
        // TCP owners such as App Store can run without a Codex carrier. Keep
        // their bounded transport counters observable through the idle gate.
        for line in crate::mio_compat::progress_lines(LINES_PER_CATEGORY) {
            crate::log_info!(target: "blueprint"; "{}\n", line);
        }
        // An exhausted/empty sweep is silent; a busy roster remains evidence.
        if active == 0 && !had_active && carriers.busy_rosters == 0 && timing_lines.is_empty() { continue; }
        had_active = active != 0;
        sample = sample.saturating_add(1);
        let now = now_ms();
        crate::log_info!(target: "blueprint";
            "codex-progress sample={} active_carriers={} roster_busy={} period_ms={}\n",
            sample, active, carriers.busy_rosters, PERIOD_MS);
        for slot in progress_probe::sample_carriers(&carriers.active, carriers.longest_running,
            &mut carrier_cursor, LINES_PER_CATEGORY)
        {
            let Some(spawner) = crate::workers::spawner_for_slot(slot as u32) else { continue; };
            let counters = progress_probe::snapshot(slot);
            let old = previous[slot];
            crate::log_info!(target: "blueprint";
                "codex-progress-ap slot={} sample={} dt_ms={} poll_b={} poll_e={} ready={} current={} ipi_tx_d={} ipi_rx_d={} ipi_fail_d={} arm_d={} timer_rx_d={}\n",
                slot, sample, now.saturating_sub(previous_ms[slot]), counters.poll_begin, counters.poll_end,
                spawner.ready_task_count(), usize::from(spawner.current_trueos_executor_task_name().is_some()),
                counters.ipi_sent.wrapping_sub(old.ipi_sent),
                counters.ipi_received.wrapping_sub(old.ipi_received),
                counters.ipi_failed.wrapping_sub(old.ipi_failed),
                counters.timer_armed.wrapping_sub(old.timer_armed),
                counters.timer_received.wrapping_sub(old.timer_received));
            previous[slot] = counters;
            previous_ms[slot] = now;
        }
        for line in timing_lines {
            crate::log_info!(target: "blueprint"; "{}\n", line);
        }
        for line in diagnostics::sample_progress_lines(&mut job_cursor, LINES_PER_CATEGORY) {
            crate::log_info!(target: "blueprint"; "{}\n", line);
        }
        let mut allocator_rows = 0usize;
        for offset in 0..crate::allcaps::hv::VM_ID_LIMIT {
            if allocator_rows == LINES_PER_CATEGORY { break; }
            let vm_id = (allocator_cursor + offset) % crate::allcaps::hv::VM_ID_LIMIT;
            let Some(work) = crate::allocators::hv_guest_allocator_work_snapshot(vm_id as u8) else { continue; };
            crate::log_info!(target: "blueprint";
                "codex-allocator-work vm={} alloc_calls={} alloc_nodes={} alloc_max_nodes={} alloc_failures={}\n",
                vm_id, work.alloc_calls, work.alloc_nodes, work.alloc_max_nodes, work.alloc_failures);
            allocator_rows += 1;
            // Advance after the loop; changing its base would skip owners.
            if allocator_rows == LINES_PER_CATEGORY { allocator_cursor = (vm_id + 1) % crate::allcaps::hv::VM_ID_LIMIT; }
        }
        for line in crate::r::io::async_fs_cabi::progress_lines(LINES_PER_CATEGORY) {
            crate::log_info!(target: "blueprint"; "{}\n", line);
        }
    }
}
