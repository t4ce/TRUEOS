#!/usr/bin/env python3
"""Exercise the production AVC teardown future against delayed GuC events."""

from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def main() -> None:
    path = "src/intel/media/pic_backend.rs"
    source = (ROOT / path).read_text()
    match = re.search(r"^async fn destroy_avc_context\(.*?^}\n", source, re.M | re.S)
    if match is None:
        raise ValueError("production AVC teardown future missing")
    harness = "#![allow(dead_code)]\n" + constant(path, "AVC_CONTEXT_TEARDOWN_TIMEOUT_US")
    harness += "\n" + item(path, "media_backend_now_ticks")
    harness += "\n" + item(path, "media_backend_elapsed_us")
    harness += "\n" + match.group() + r'''
use std::{cell::RefCell, collections::VecDeque, future::Future, task::{Context, Poll, Waker}};
use intel::guc_submission::GucSubmissionError::*;
type Outcome = Result<(), intel::guc_submission::GucSubmissionError>;
#[derive(Default)]
struct Rig {
    now: u64,
    outcomes: VecDeque<Outcome>,
    calls: Vec<u64>,
    yields: usize,
}
thread_local! { static RIG: RefCell<Rig> = RefCell::new(Rig::default()); }
mod embassy_time_driver {
    pub const TICK_HZ: u64 = 1_000;
    pub fn now() -> u64 { super::RIG.with(|rig| rig.borrow().now) }
}
mod intel {
    #[derive(Clone, Copy)]
    pub struct Dev;
    pub mod guc_submission {
        #[derive(Clone, Copy)]
        pub struct GucContextToken(pub u64);
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum GucSubmissionError {
            DisablePending, DeregisterPending, DeviceFaulted, InvalidContext,
            DisableRejected, DeregisterRejected, TransportNotReady,
        }
        pub struct Scheduler;
        pub static INTEL_GUC_SCHEDULER: Scheduler = Scheduler;
        impl Scheduler {
            pub fn destroy(&self, _: super::Dev, token: GucContextToken) -> super::super::Outcome {
                super::super::RIG.with(|rig| {
                    let mut rig = rig.borrow_mut();
                    rig.calls.push(token.0);
                    rig.outcomes.pop_front().expect("unexpected teardown attempt")
                })
            }
        }
    }
}
mod trueos_time {
    pub struct Timer;
    impl Timer {
        pub async fn after_millis(ms: u64) {
            let mut yielded = false;
            std::future::poll_fn(|cx| {
                if yielded {
                    super::RIG.with(|rig| rig.borrow_mut().now += ms);
                    std::task::Poll::Ready(())
                } else {
                    yielded = true;
                    super::RIG.with(|rig| rig.borrow_mut().yields += 1);
                    cx.waker().wake_by_ref();
                    std::task::Poll::Pending
                }
            }).await
        }
    }
}
fn run(outcomes: impl IntoIterator<Item = Outcome>) -> (Outcome, Rig) {
    RIG.with(|rig| *rig.borrow_mut() = Rig {
        outcomes: outcomes.into_iter().collect(), ..Rig::default()
    });
    let mut future = std::pin::pin!(destroy_avc_context(
        intel::Dev, intel::guc_submission::GucContextToken(0x1234)
    ));
    let mut context = Context::from_waker(Waker::noop());
    // This tests a yielding future, not a busy loop with a mocked clock jump.
    for _ in 0..1000 {
        if let Poll::Ready(result) = future.as_mut().poll(&mut context) {
            let rig = RIG.with(|rig| std::mem::take(&mut *rig.borrow_mut()));
            assert!(rig.calls.iter().all(|token| *token == 0x1234));
            return (result, rig);
        }
    }
    panic!("teardown did not finish within its bounded wait");
}
#[test]
fn immediate_completion_has_no_delay() {
    let (result, rig) = run([Ok(())]);
    assert_eq!(result, Ok(()));
    assert_eq!(rig.calls.len(), 1);
    assert_eq!(rig.yields, 0);
}
#[test]
fn delayed_disable_and_deregister_keep_the_same_token_until_done() {
    let (result, rig) = run([
        Err(DisablePending), Err(DisablePending), Err(DeregisterPending), Ok(())
    ]);
    assert_eq!(result, Ok(()));
    assert_eq!(rig.calls.len(), 4);
    assert_eq!(rig.yields, 3);
    assert_eq!(rig.now, 3);
}
#[test]
fn permanent_errors_do_not_retry_or_report_safe_release() {
    for error in [DeviceFaulted, InvalidContext, DisableRejected,
                  DeregisterRejected, TransportNotReady] {
        let (result, rig) = run([Err(error)]);
        assert_eq!(result, Err(error));
        assert_eq!(rig.calls.len(), 1);
        assert_eq!(rig.yields, 0);
    }
}
#[test]
fn pending_timeout_preserves_failure_and_never_consumes_late_success() {
    for error in [DisablePending, DeregisterPending] {
        let (result, rig) = run(std::iter::repeat_n(Err(error), 101).chain([Ok(())]));
        assert_eq!(result, Err(error));
        assert_eq!(rig.now, 100);
        assert_eq!(rig.yields, 100);
        assert_eq!(rig.outcomes, [Ok(())]);
    }
}
#[test]
fn fault_arriving_while_teardown_is_pending_stops_retries() {
    let (result, rig) = run([Err(DisablePending), Err(DeviceFaulted), Ok(())]);
    assert_eq!(result, Err(DeviceFaulted));
    assert_eq!(rig.calls.len(), 2);
    assert_eq!(rig.outcomes, [Ok(())]);
}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-avc-teardown-") as temporary:
        directory = Path(temporary)
        rust = directory / "teardown.rs"
        binary = directory / "teardown-tests"
        rust.write_text(harness)
        subprocess.run(["rustc", "--edition=2024", "--test", str(rust), "-o", str(binary)],
                       cwd=ROOT, check=True)
        subprocess.run([str(binary)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
