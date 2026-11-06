#!/usr/bin/env python3
"""Test the actual guest wait transport's finite timeout wire boundary."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
const STATUS_OK: u64 = 0;
const OP_BP_PLATFORM_WAIT_AFTER: u64 = 79;
static STATUS: AtomicU64 = AtomicU64::new(STATUS_OK);
static NOTIFIED: AtomicU64 = AtomicU64::new(0);
static PACKED: AtomicU64 = AtomicU64::new(0);
fn guest_call(op: u64, key: u64, packed: u64) -> (u64, u64) {
    assert_eq!(op, OP_BP_PLATFORM_WAIT_AFTER);
    assert_eq!(key, 1234);
    PACKED.store(packed, Relaxed);
    // Simulate the host completing even the largest finite wire timeout
    // immediately, so the >49-day regression has no wall-clock wait.
    (STATUS.load(Relaxed), NOTIFIED.load(Relaxed))
}
// @PRODUCTION_FUNCTION@
#[test]
fn shortened_finite_expiration_is_spurious_instead_of_a_false_timeout() {
    let cap = u64::from(u32::MAX - 1);
    for timeout in [0, 1, cap] {
        assert!(!guest_platform_wait_after(1234, 0x8765_4321, timeout));
        assert_eq!(PACKED.load(Relaxed), (0x8765_4321u64 << 32) | timeout);
    }
    for timeout in [cap + 1, cap * 2, u64::MAX - 1] {
        assert!(guest_platform_wait_after(1234, 0x8765_4321, timeout),
            "transport reported an early known timeout");
        assert_eq!(PACKED.load(Relaxed), (0x8765_4321u64 << 32) | cap);
    }
    assert!(!guest_platform_wait_after(1234, 0x8765_4321, u64::MAX));
    assert_eq!(PACKED.load(Relaxed), (0x8765_4321u64 << 32) | u64::from(u32::MAX));
    NOTIFIED.store(1, Relaxed);
    assert!(guest_platform_wait_after(1234, 0x8765_4321, 1));
    assert!(guest_platform_wait_after(1234, 0x8765_4321, u64::MAX));
    STATUS.store(1, Relaxed);
    assert!(!guest_platform_wait_after(1234, 0x8765_4321, cap + 1));
}
'''


class PlatformWaitTimeout(unittest.TestCase):
    def test_production_guest_transport(self):
        if not shutil.which("rustup"):
            self.skipTest("pinned rustc is needed")
        installed = Path(subprocess.check_output(
            ["rustup", "run", "nightly-2026-07-10", "rustc", "--print", "sysroot"], text=True).strip())
        source = (ROOT / "src/hv/vmcall.rs").read_text()
        function = "pub fn guest_platform_wait_after(" + source.split(
            "pub fn guest_platform_wait_after(", 1)[1].split("\npub fn guest_cpu_count", 1)[0]
        with tempfile.TemporaryDirectory(prefix="trueos-platform-timeout-") as directory:
            source = Path(directory) / "transport_tests.rs"
            binary = Path(directory) / "transport_tests"
            source.write_text(HARNESS.replace("// @PRODUCTION_FUNCTION@", function))
            env = dict(os.environ)
            env["LD_LIBRARY_PATH"] = str(installed / "lib") + ":" + env.get("LD_LIBRARY_PATH", "")
            subprocess.run([installed / "bin/rustc", "--edition=2024", "--test", source, "-o", binary],
                           env=env, check=True, timeout=60)
            subprocess.run([binary], env=env, check=True, timeout=60)


if __name__ == "__main__":
    unittest.main()
