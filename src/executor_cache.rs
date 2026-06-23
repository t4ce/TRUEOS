use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use embassy_executor::raw::Executor;

const BSP_CPU_SLOT: u32 = 0;
const LOG_SAMPLE_1K: u64 = 1_024;
const LOG_SAMPLE_SLOW: u64 = 1 << 20;

static BSP_WARM_SAMPLES: AtomicU64 = AtomicU64::new(0);
static BSP_WARM_EXECUTOR: AtomicUsize = AtomicUsize::new(0);
static BSP_WARM_READY: AtomicUsize = AtomicUsize::new(0);
static BSP_WARM_SPAWNED: AtomicUsize = AtomicUsize::new(0);
static BSP_WARM_SLACK: AtomicU64 = AtomicU64::new(0);
static BSP_WARM_POLL_LIMIT: AtomicUsize = AtomicUsize::new(0);
static BSP_WARM_FINGERPRINT: AtomicU64 = AtomicU64::new(0);

#[inline(always)]
pub fn warm_bsp_executor(cpu: &crate::percpu::PerCpu, executor: &'static Executor) {
    if cpu.cpu_index() != BSP_CPU_SLOT {
        return;
    }

    let executor_addr = executor as *const Executor as usize;
    let ready = executor.ready_task_count();
    let spawned = executor.spawned_task_count();
    let slack = executor.timer_slack_ticks();
    let poll_limit = executor.poll_limit_tasks();
    let sample = BSP_WARM_SAMPLES
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1);

    BSP_WARM_EXECUTOR.store(executor_addr, Ordering::Relaxed);
    BSP_WARM_READY.store(ready, Ordering::Relaxed);
    BSP_WARM_SPAWNED.store(spawned, Ordering::Relaxed);
    BSP_WARM_SLACK.store(slack, Ordering::Relaxed);
    BSP_WARM_POLL_LIMIT.store(poll_limit, Ordering::Relaxed);

    let fingerprint = (executor_addr as u64)
        ^ sample.rotate_left(7)
        ^ (ready as u64).rotate_left(17)
        ^ (spawned as u64).rotate_left(29)
        ^ slack.rotate_left(41)
        ^ (poll_limit as u64).rotate_left(53);
    BSP_WARM_FINGERPRINT.store(fingerprint, Ordering::Relaxed);
    core::hint::black_box(fingerprint);

    if should_log_sample(sample) {
        crate::log_info!(
            target: "executor-cache";
            "executor-cache: bsp-warm sample={} exec={:#x} ready={} spawned={} slack={} poll_limit={}\n",
            sample,
            executor_addr,
            ready,
            spawned,
            slack,
            poll_limit
        );
    }
}

#[inline(always)]
fn should_log_sample(sample: u64) -> bool {
    sample == 1 || sample == LOG_SAMPLE_1K || sample % LOG_SAMPLE_SLOW == 0
}
