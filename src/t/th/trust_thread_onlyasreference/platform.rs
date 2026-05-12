//! Platform hooks for a portable no_std thread substrate.
//!
//! The TrustOS source directly names its kernel modules.  This trait is the
//! extraction boundary: TRUEOS, TrustOS, or another kernel can provide these
//! hooks without changing the scheduler core.

use super::types::ThreadId;

pub trait ThreadPlatform {
    const MAX_CPUS: usize;

    fn current_cpu() -> usize;
    fn ready_cpu_count() -> usize;
    fn now_nanos() -> u64;

    fn send_reschedule_ipi(cpu_id: usize);
    fn idle_hint();

    fn register_wakeup(thread_id: ThreadId, deadline_nanos: u64);
    fn log_thread_event(message: &str);
}
