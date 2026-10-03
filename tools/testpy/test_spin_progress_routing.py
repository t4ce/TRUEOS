#!/usr/bin/env python3
"""Execute the production spin-progress dispatcher in all execution realms."""
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


def progress_source(name):
    source = (ROOT / "src/wait.rs").read_text()
    start = source.index(f"pub fn {name}() {{")
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
use std::sync::{Mutex, atomic::{AtomicBool, AtomicUsize, Ordering}};

const HOST: usize = 0;
const CONTINUATION: usize = 1;
const HULL: usize = 2;
static REALM: AtomicUsize = AtomicUsize::new(HOST);
static CRITICAL: AtomicBool = AtomicBool::new(false);
static CALLS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

fn record(call: &'static str) { CALLS.lock().unwrap().push(call); }
fn realm() -> usize { REALM.load(Ordering::SeqCst) }

mod r {
    pub mod threads {
        pub fn yield_now() -> bool {
            assert!(!super::super::CRITICAL.load(super::super::Ordering::SeqCst),
                "critical section attempted continuation suspension");
            super::super::record("continuation-yield");
            super::super::realm() == super::super::CONTINUATION
        }
    }
}

mod hv {
    pub fn current_hull_guest_context_vm_id() -> Option<u8> {
        if !super::CRITICAL.load(super::Ordering::SeqCst) {
            assert_ne!(super::realm(), super::CONTINUATION,
                "native continuation reached fallback realm discovery");
        }
        super::record("hull-check");
        (super::realm() == super::HULL).then_some(0)
    }
    pub mod vmcall {
        pub fn guest_yield() {
            assert!(!super::super::CRITICAL.load(super::super::Ordering::SeqCst),
                "critical section attempted guest VMCALL yielding");
            assert_eq!(super::super::realm(), super::super::HULL,
                "host or continuation attempted guest VMCALL");
            super::super::record("guest-yield");
        }
    }
}

mod time {
    pub fn poll() {
        assert_ne!(super::realm(), super::HULL,
            "Hull touched host time state");
        super::record("host-time");
    }
}
mod runtime {
    pub fn poll_local_executor() {
        assert!(!super::CRITICAL.load(super::Ordering::SeqCst),
            "critical section reentered the host executor");
        assert_eq!(super::realm(), super::HOST,
            "Hull or continuation touched the GS-backed host executor");
        super::record("host-executor");
    }
}

mod wait {
    PRODUCTION_DISPATCH
    PRODUCTION_NO_EXEC_DISPATCH
}

fn check(context: usize, expected: &[&'static str]) {
    CRITICAL.store(false, Ordering::SeqCst);
    REALM.store(context, Ordering::SeqCst);
    CALLS.lock().unwrap().clear();
    wait::spin_step();
    assert_eq!(*CALLS.lock().unwrap(), expected);
}

fn check_no_exec(context: usize, expected: &[&'static str]) {
    CRITICAL.store(true, Ordering::SeqCst);
    REALM.store(context, Ordering::SeqCst);
    CALLS.lock().unwrap().clear();
    wait::spin_step_no_exec();
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

#[test]
fn hull_critical_section_neither_polls_host_time_nor_yields_vmcall() {
    check_no_exec(HULL, &["hull-check"]);
}

#[test]
fn ordinary_host_critical_section_polls_time_without_executor_reentry() {
    check_no_exec(HOST, &["hull-check", "host-time"]);
}

#[test]
fn continuation_critical_section_polls_time_without_suspending() {
    check_no_exec(CONTINUATION, &["hull-check", "host-time"]);
}
'''.replace("PRODUCTION_DISPATCH", progress_source("spin_step"))
        source = source.replace("PRODUCTION_NO_EXEC_DISPATCH", progress_source("spin_step_no_exec"))
        with tempfile.TemporaryDirectory(prefix="trueos-spin-routing-") as directory:
            path = Path(directory) / "tests.rs"
            binary = Path(directory) / "tests"
            path.write_text(source)
            subprocess.run(["rustc", "--edition=2024", "--test", str(path), "-o", str(binary)], check=True)
            subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    unittest.main()
