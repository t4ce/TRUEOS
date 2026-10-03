#!/usr/bin/env python3
"""Execute the production spin-progress dispatcher in all execution realms."""
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


def spin_step_source():
    source = (ROOT / "src/wait.rs").read_text()
    start = source.index("pub fn spin_step() {")
    brace = source.index("{", start)
    depth = 1
    end = brace + 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


class SpinProgressRoutingTests(unittest.TestCase):
    def test_production_dispatch_preserves_execution_realm(self):
        source = r'''
use std::sync::{Mutex, atomic::{AtomicUsize, Ordering}};

const HOST: usize = 0;
const CONTINUATION: usize = 1;
const HULL: usize = 2;
static REALM: AtomicUsize = AtomicUsize::new(HOST);
static CALLS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

fn record(call: &'static str) { CALLS.lock().unwrap().push(call); }
fn realm() -> usize { REALM.load(Ordering::SeqCst) }

mod r {
    pub mod threads {
        pub fn yield_now() -> bool {
            super::super::record("continuation-yield");
            super::super::realm() == super::super::CONTINUATION
        }
    }
}

mod hv {
    pub fn current_hull_guest_context_vm_id() -> Option<u8> {
        assert_ne!(super::realm(), super::CONTINUATION,
            "native continuation reached fallback realm discovery");
        super::record("hull-check");
        (super::realm() == super::HULL).then_some(0)
    }
    pub mod vmcall {
        pub fn guest_yield() {
            assert_eq!(super::super::realm(), super::super::HULL,
                "host or continuation attempted guest VMCALL");
            super::super::record("guest-yield");
        }
    }
}

mod time {
    pub fn poll() {
        assert_eq!(super::realm(), super::HOST,
            "Hull or continuation touched host time state");
        super::record("host-time");
    }
}
mod runtime {
    pub fn poll_local_executor() {
        assert_eq!(super::realm(), super::HOST,
            "Hull or continuation touched the GS-backed host executor");
        super::record("host-executor");
    }
}

mod wait {
    PRODUCTION_DISPATCH
}

fn check(context: usize, expected: &[&'static str]) {
    REALM.store(context, Ordering::SeqCst);
    CALLS.lock().unwrap().clear();
    wait::spin_step();
    assert_eq!(*CALLS.lock().unwrap(), expected);
}

#[test]
fn continuation_suspends_and_does_not_enter_host_polling() {
    check(CONTINUATION, &["continuation-yield"]);
}

#[test]
fn hull_yields_through_vmcall_without_host_time_executor_or_gs() {
    check(HULL, &["continuation-yield", "hull-check", "guest-yield"]);
}

#[test]
fn ordinary_host_polls_time_then_executor() {
    check(HOST, &["continuation-yield", "hull-check", "host-time", "host-executor"]);
}
'''.replace("PRODUCTION_DISPATCH", spin_step_source())
        with tempfile.TemporaryDirectory(prefix="trueos-spin-routing-") as directory:
            path = Path(directory) / "tests.rs"
            binary = Path(directory) / "tests"
            path.write_text(source)
            subprocess.run(["rustc", "--edition=2024", "--test", str(path), "-o", str(binary)], check=True)
            subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    unittest.main()
