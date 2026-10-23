#!/usr/bin/env python3
"""Exercise the production SDK shutdown CABI's Hull/native routing."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / "src/r/io/fs_cabi.rs").read_text()
    function = re.search(
        r'^pub unsafe extern "C" fn trueos_cabi_blueprint_shutdown\(.*?^}',
        source, re.M | re.S,
    ).group()
    harness = r'''
use std::sync::{Mutex, atomic::{AtomicBool, AtomicU32, Ordering}};
static HULL: AtomicBool = AtomicBool::new(false);
static STATUS: AtomicU32 = AtomicU32::new(0);
static EVENTS: Mutex<Vec<String>> = Mutex::new(Vec::new());
#[macro_export] macro_rules! log {
    ($($args:tt)*) => { EVENTS.lock().unwrap().push(format!($($args)*)) };
}
mod hv {
    pub fn current_hull_guest_context_vm_id() -> Option<u8> {
        super::HULL.load(super::Ordering::SeqCst).then_some(1)
    }
}
mod r { pub mod threads {
    pub unsafe fn terminate_current_guest() -> bool {
        // The actual scheduler's nonreturning transfer has separate stack
        // tests. Here observe routing, including completed log/lock cleanup.
        super::super::EVENTS.try_lock().expect("logger guard crossed termination")
            .push("native-terminal".into());
        false
    }
} }
mod trueos_vm { pub mod vmcall {
    pub const OP_BP_SHUTDOWN: u32 = 1;
    pub const STATUS_OK: u32 = 0;
    pub fn call_with_payload(_: u32, _: u64, _: u64, data: &[u8], _: &mut [u8]) -> (u32, u64) {
        super::super::EVENTS.lock().unwrap().push(format!("hull:{}", String::from_utf8_lossy(data)));
        (super::super::STATUS.load(super::super::Ordering::SeqCst), 0)
    }
} }
@PRODUCTION@
fn reset(hull: bool) {
    HULL.store(hull, Ordering::SeqCst);
    STATUS.store(0, Ordering::SeqCst);
    EVENTS.lock().unwrap().clear();
}
#[test] fn sdk_native_abort_reaches_terminal_after_logging_returns() {
    reset(false);
    assert_eq!(unsafe { trueos_cabi_blueprint_shutdown(b"blueprint abort".as_ptr(), 15) }, 0);
    let events = EVENTS.lock().unwrap();
    assert!(events[0].contains("blueprint-shutdown: blueprint abort"));
    assert_eq!(events[1], "native-terminal");
}
#[test] fn hull_shutdown_keeps_vmcall_routing_and_reports_transport_failure() {
    reset(true);
    assert_eq!(unsafe { trueos_cabi_blueprint_shutdown(b"exit".as_ptr(), 4) }, 0);
    assert_eq!(*EVENTS.lock().unwrap(), ["hull:exit"]);
    STATUS.store(1, Ordering::SeqCst);
    assert_eq!(unsafe { trueos_cabi_blueprint_shutdown(std::ptr::null(), 0) }, -1);
    assert!(EVENTS.lock().unwrap().iter().all(|event| event != "native-terminal"));
}
#[test] fn native_shutdown_bounds_message_before_terminal_transfer() {
    reset(false);
    let data = [b'x'; 600];
    unsafe { trueos_cabi_blueprint_shutdown(data.as_ptr(), data.len()) };
    let events = EVENTS.lock().unwrap();
    assert_eq!(events[0].matches('x').count(), 512);
    assert_eq!(events[1], "native-terminal");
}
'''.replace("@PRODUCTION@", function)
    with tempfile.TemporaryDirectory(prefix="trueos-native-shutdown-") as directory:
        root = Path(directory)
        (root / "test.rs").write_text(harness)
        subprocess.run(["rustc", "--edition=2024", "--test", str(root / "test.rs"),
                        "-o", str(root / "tests")], cwd=ROOT, check=True)
        subprocess.run([str(root / "tests"), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
