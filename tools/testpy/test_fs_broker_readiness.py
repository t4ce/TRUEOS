#!/usr/bin/env python3
"""Exercise broker first-poll publication and the real host-launch prerequisite.

The production broker's startup prefix, readiness publisher, and prebind base
policy run unchanged. The long-lived request loop is replaced with Pending;
the watch sink records ordering without needing a disk, executor, GPU or VM.
"""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item

ROOT = Path(__file__).resolve().parents[2]
READINESS = "src/r/readiness.rs"


def harness() -> str:
    broker = (ROOT / "src/r/fs/request_broker.rs").read_text()
    service = broker.split("pub async fn service_task() {", 1)[1]
    prefix = service.split("    let mut active =", 1)[0]
    declarations = "\n".join(constant(READINESS, name) for name in (
        "BACKGROUND_AP_WORKER_READY", "TRUEOSFS_REQUEST_BROKER_READY",
        "TRUEOSFS_ROOT_MOUNTED", "TRUEOSFS_INDEX_READY", "NET_ANY_CONFIGURED",
        "APP_VM_READY", "APP_VM_READY_REQUIRED",
    ))
    declarations += "\n" + "\n".join(item(READINESS, name) for name in (
        "mask", "is_set", "set",
    ))
    return r'''
#![allow(dead_code)]
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::future::{Future, pending};
use std::task::{Context, Poll, Waker};
#[macro_export] macro_rules! log_info {($($t:tt)*) => {{}}}
mod r {
    pub mod readiness {
        use crate::{AtomicU32, Ordering};
        static READY: AtomicU32 = AtomicU32::new(0);
        struct Watch;
        static READINESS_WATCH: Watch = Watch;
        impl Watch {
            fn sender(&self) -> &Self { self }
            fn send(&self, value: u32) {
                if value & TRUEOSFS_REQUEST_BROKER_READY != 0 {
                    assert!(crate::broker::online(), "readiness preceded the actual broker");
                }
            }
        }
        @READINESS@
    }
}
mod broker {
    use crate::{AtomicBool, Ordering, pending};
    static BROKER_ONLINE: AtomicBool = AtomicBool::new(false);
    pub fn online() -> bool { BROKER_ONLINE.load(Ordering::Acquire) }
    pub async fn service_first_poll() {
        @PREFIX@
        pending::<()>().await;
    }
}
@BASE@

#[test]
fn app_launch_waits_for_first_broker_poll_without_waiting_for_disks() {
    use r::readiness::*;
    let required = prebind_base_readiness();
    assert_eq!(required, BACKGROUND_AP_WORKER_READY | TRUEOSFS_REQUEST_BROKER_READY);
    assert!(!is_set(required));

    // Disk/index/network completion is not a prerequisite for this host service.
    r::readiness::set(BACKGROUND_AP_WORKER_READY);
    let mut broker_task = Box::pin(broker::service_first_poll());
    assert_eq!(mask(), BACKGROUND_AP_WORKER_READY, "creating/queuing the task isn't online");
    assert!(!broker::online());
    assert!(!is_set(required), "launch preparation must still wait");

    assert!(matches!(broker_task.as_mut().poll(&mut Context::from_waker(Waker::noop())), Poll::Pending));
    assert!(broker::online());
    assert!(is_set(required), "ordinary admission may now prepare access policy");
    assert_eq!(mask(), required, "no unrelated readiness was invented");
    assert_eq!(mask() & (TRUEOSFS_ROOT_MOUNTED | TRUEOSFS_INDEX_READY | NET_ANY_CONFIGURED | APP_VM_READY), 0);

    // Re-polling the running service keeps the monotonic host prerequisite.
    assert!(matches!(broker_task.as_mut().poll(&mut Context::from_waker(Waker::noop())), Poll::Pending));
    assert_eq!(mask(), required);
}
'''.replace("@READINESS@", declarations).replace("@PREFIX@", prefix).replace(
        "@BASE@", item("src/hv/blueprint/prebind.rs", "prebind_base_readiness")
    )


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-fs-broker-readiness-") as directory:
        source = Path(directory) / "test.rs"
        binary = Path(directory) / "test"
        source.write_text(harness())
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)],
                       cwd=ROOT, check=True)
        subprocess.run([str(binary)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
