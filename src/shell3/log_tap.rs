//! Independent live reader of the accepted log_os TCP stream.
//! Logging never takes a Matrix lock; the worker publishes only after releasing
//! the ring lock. The attachment belongs to one exact §log slot lifetime.

use super::{MatrixSlotLease, MatrixSlots, matrix_target};
use alloc::{sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;
use trueos_time::{Duration, Timer};

struct Tap {
    lease: MatrixSlotLease,
    active: AtomicBool,
}
impl matrix_target::MatrixSlotAttachment for Tap {
    fn on_matrix_slot_freed(&self, lease: &MatrixSlotLease) {
        if lease == &self.lease {
            self.active.store(false, Ordering::Release);
        }
    }
}
static ACTIVE: Mutex<Option<Arc<Tap>>> = Mutex::new(None);

pub(super) fn start(lease: MatrixSlotLease) -> Result<(), &'static str> {
    let mut current = ACTIVE.lock();
    if current
        .as_ref()
        .is_some_and(|tap| tap.lease == lease && tap.active.load(Ordering::Acquire))
    {
        return Ok(());
    }
    let worker = crate::workers::pick_background_spawner().ok_or("log: no background worker")?;
    let tap = Arc::new(Tap {
        lease: lease.clone(),
        active: AtomicBool::new(true),
    });
    let task = drain(tap.clone(), crate::log_os::logtotcp::live_cursor())
        .map_err(|_| "log: tap worker busy; try again")?;
    if matrix_target::attach_matrix_slot_resource(&lease, tap.clone()).is_err() {
        // Poll the admitted task once so its task-pool slot is released.
        tap.active.store(false, Ordering::Release);
        worker.spawn(task);
        return Err("log: slot was freed");
    }
    if let Some(previous) = current.replace(tap) {
        previous.active.store(false, Ordering::Release);
    }
    worker.spawn(task);
    Ok(())
}

#[trueos_executor::task(pool_size = 4)]
async fn drain(tap: Arc<Tap>, mut cursor: u64) {
    let mut bytes = [0u8; 4096];
    let mut pending = Vec::with_capacity(4096);
    while tap.active.load(Ordering::Acquire) {
        if let Some((count, lost)) =
            crate::log_os::logtotcp::copy_for_screen(&mut cursor, &mut bytes)
        {
            if lost != 0 {
                pending.clear();
                MatrixSlots::echo_output(
                    &tap.lease,
                    alloc::format!("log: {lost} bytes overwritten before display"),
                );
            }
            for byte in &bytes[..count] {
                if *byte == b'\n' {
                    publish(&tap.lease, &mut pending);
                } else {
                    pending.push(*byte);
                    if pending.len() == 4096 {
                        publish(&tap.lease, &mut pending);
                    }
                }
            }
            // Flush a partial line once the stream catches up, without retaining
            // an unbounded record or waiting forever for a missing newline.
            if count == 0 && !pending.is_empty() {
                publish(&tap.lease, &mut pending);
            }
        }
        Timer::after(Duration::from_millis(25)).await;
    }
    let mut current = ACTIVE.lock();
    if current
        .as_ref()
        .is_some_and(|active| Arc::ptr_eq(active, &tap))
    {
        *current = None;
    }
}

fn publish(lease: &MatrixSlotLease, pending: &mut Vec<u8>) {
    if pending.last() == Some(&b'\r') {
        pending.pop();
    }
    MatrixSlots::echo_output(lease, alloc::string::String::from_utf8_lossy(pending).into_owned());
    pending.clear();
}
