#!/usr/bin/env python3
"""Test actual stop state and SDK drop order with a host CABI mock.

QEMU's tokio_stop probe covers the real ABI, workers, TLS and VM teardown.
This harness compiles the production SDK's TRUEOS branch, not its host stub.
"""
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SDK = ROOT.parent / "TRUEOS-Blueprints/api/src/shutdown.rs"


class CooperativeStopTests(unittest.TestCase):
    def test_protocol_races_worker_polling_and_cleanup_order(self):
        source = r'''
extern crate self as v;
#[path = "__PROTOCOL__"] mod stop;
#[path = "__SDK__"] mod shutdown;
static STATE: stop::CooperativeStop = stop::CooperativeStop::new();
static ORDER: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());
pub mod bp_abi {
    pub unsafe fn trueos_cabi_blueprint_stop_control_v1(op: u32) -> i32 {
        match op {
            0 => if super::STATE.register() { 0 } else { -1 },
            1 => super::STATE.requested() as i32,
            _ => -1,
        }
    }
    pub unsafe fn trueos_cabi_blueprint_shutdown(_: *const u8, _: usize) -> i32 {
        super::ORDER.lock().unwrap().push(9);
        0
    }
}
struct Resource(u8);
impl Drop for Resource {
    fn drop(&mut self) { ORDER.lock().unwrap().push(self.0); }
}
fn reset() { STATE.reset(); ORDER.lock().unwrap().clear(); }
#[test]
fn sdk_acknowledges_only_after_later_resources_drop() {
    reset();
    {
        let _shutdown = shutdown::ShutdownGuard::register().unwrap();
        let _runtime = Resource(1);
        let _persistence = Resource(2);
        assert!(!shutdown::requested().unwrap());
        assert!(STATE.request());
    }
    assert_eq!(*ORDER.lock().unwrap(), vec![2, 1, 9]);
}
#[test]
fn sdk_worker_observes_request_and_cannot_take_cleanup_ownership() {
    reset();
    let guard = shutdown::ShutdownGuard::register().unwrap();
    let worker = std::thread::spawn(|| {
        assert!(shutdown::ShutdownGuard::register().is_err());
        while !shutdown::requested().unwrap() { std::thread::yield_now(); }
    });
    assert!(STATE.request());
    worker.join().unwrap();
    assert!(guard.requested().unwrap());
    drop(guard);
    assert_eq!(*ORDER.lock().unwrap(), vec![9]);
}
#[test]
fn sdk_late_registration_does_not_acknowledge_an_immediate_stop() {
    reset();
    assert!(!STATE.request());
    assert!(shutdown::ShutdownGuard::register().is_err());
    assert!(ORDER.lock().unwrap().is_empty());
}
'''.replace("__PROTOCOL__", str(ROOT / "src/hv/cooperative_stop.rs")).replace("__SDK__", str(SDK))
        with tempfile.TemporaryDirectory(prefix="trueos-cooperative-stop-") as temp:
            src = Path(temp) / "stop.rs"
            binary = Path(temp) / "stop-tests"
            src.write_text(source)
            subprocess.run(["rustc", "--edition=2024", "--test", "--cfg", 'target_os="trueos"',
                            "-Aexplicit_builtin_cfgs_in_flags", str(src), "-o", str(binary)], check=True)
            result = subprocess.run([str(binary), "--test-threads=1"], check=True, text=True, capture_output=True)
            self.assertIn("9 passed; 0 failed", result.stdout)


if __name__ == "__main__":
    unittest.main()
