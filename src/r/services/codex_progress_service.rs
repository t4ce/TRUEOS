//! Read-only observer on the BSP, outside Codex's LastAP executor.
use crate::r::{blocking::diagnostics, input_timing::now_ms, progress_probe};
use trueos_time::{Duration, Timer};

const PERIOD_MS: u64 = 5_000;
const JOB_LINES_PER_SAMPLE: usize = 4;

#[trueos_executor::task]
pub(crate) async fn codex_progress_task() {
    let mut previous = progress_probe::Snapshot::default();
    let mut previous_ms = now_ms();
    let mut sample = 0u64;
    loop {
        Timer::after(Duration::from_millis(PERIOD_MS)).await;
        let Some(slot) = crate::workers::codex_application_slot() else {
            continue;
        };
        let Some(spawner) = crate::workers::spawner_for_slot(slot) else {
            continue;
        };
        let now = now_ms();
        let counters = progress_probe::snapshot(slot as usize);
        let elapsed = now.saturating_sub(previous_ms);
        let spawned = spawner.spawned_task_count();
        if spawned != 0 {
            sample = sample.saturating_add(1);
            crate::log_info!(target: "blueprint";
                "codex-progress-ap slot={} sample={} dt_ms={} poll_b={} poll_e={} ready={} current={} ipi_tx_d={} ipi_rx_d={} ipi_fail_d={} arm_d={} timer_rx_d={}\n",
                slot, sample, elapsed, counters.poll_begin, counters.poll_end,
                spawner.ready_task_count(), usize::from(spawner.current_trueos_executor_task_name().is_some()),
                counters.ipi_sent.wrapping_sub(previous.ipi_sent),
                counters.ipi_received.wrapping_sub(previous.ipi_received),
                counters.ipi_failed.wrapping_sub(previous.ipi_failed),
                counters.timer_armed.wrapping_sub(previous.timer_armed),
                counters.timer_received.wrapping_sub(previous.timer_received));
            for line in crate::mio_compat::progress_lines(4) {
                crate::log_info!(target: "blueprint"; "{}\n", line);
            }
            let mut allocator_rows = 0usize;
            for vm_id in 0..crate::allcaps::hv::VM_ID_LIMIT {
                if allocator_rows == 4 {
                    break;
                }
                let Some(work) = crate::allocators::hv_guest_allocator_work_snapshot(vm_id as u8)
                else {
                    continue;
                };
                crate::log_info!(target: "blueprint";
                    "codex-allocator-work vm={} alloc_calls={} alloc_nodes={} alloc_max_nodes={} alloc_failures={}\n",
                    vm_id, work.alloc_calls, work.alloc_nodes, work.alloc_max_nodes, work.alloc_failures);
                allocator_rows += 1;
            }
            for line in crate::r::io::async_fs_cabi::progress_lines(4) {
                crate::log_info!(target: "blueprint"; "{}\n", line);
            }
            // Fixed global output bound, including when multiple VMs exist.
            // try_lock keeps suspect bookkeeping locks from blocking us.
            let mut remaining = JOB_LINES_PER_SAMPLE;
            for vm_id in 0..crate::allcaps::hv::VM_ID_LIMIT {
                if remaining == 0 {
                    break;
                }
                for line in diagnostics::progress_lines(vm_id as u8, remaining) {
                    crate::log_info!(target: "blueprint"; "{}\n", line);
                    remaining -= 1;
                }
            }
        }
        previous = counters;
        previous_ms = now;
    }
}
