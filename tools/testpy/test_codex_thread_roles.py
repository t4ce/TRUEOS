#!/usr/bin/env python3
"""Test the actual Codex parser and Tokio role/activity scopes with host rustc.

The production TRUEOS paths are also linked to a small thread-local CABI model.
This checks ABI glue and restoration; AP scheduling is tested separately by
test_thread_scheduler.py and still needs a booted-machine measurement.
"""

import json
import os
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
BUILDINS = Path(os.environ.get("TRUEOS_BUILDINS_ROOT", ROOT.parent / "TRUEOS-Buildins"))
CODEX = Path(os.environ.get("TRUEOS_CODEX_ROOT", ROOT.parent / "codex"))
TOKIO_ROLES = BUILDINS / "vendor/tokio-1.54.1/src/runtime/trueos.rs"
CODEX_ROLES = CODEX / "codex-rs/arg0/src/trueos_threads.rs"


HARNESS = r'''extern crate self as tracing;
#[macro_export]
macro_rules! info { ($($tokens:tt)*) => { () }; }
#[path = @TOKIO_ROLES@] mod pool_roles;
#[path = @CODEX_ROLES@] mod codex_roles;
use std::cell::Cell;
use std::sync::atomic::{AtomicU32, Ordering};
static MODE: AtomicU32 = AtomicU32::new(u32::MAX);
static CAP: AtomicU32 = AtomicU32::new(0);
std::thread_local! {
    static ROLE: Cell<u32> = const { Cell::new(0) };
    static ACTIVITY: Cell<u32> = const { Cell::new(0) };
}
mod startup_timing {
    pub fn mark(stage: &str) {
        assert!(stage.starts_with("thread_placement_configured mode=")
            || stage == "thread_placement_unavailable_default");
    }
}
#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_thread_configure_placement_v1(mode: u32, cap: u32) -> i32 {
    MODE.store(mode, Ordering::Relaxed);
    CAP.store(cap, Ordering::Relaxed);
    std::env::var("HARNESS_CONFIGURE_ERRNO").ok().and_then(|s| s.parse().ok()).unwrap_or(0)
}
#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_thread_swap_child_role_v1(role: u32) -> u32 {
    ROLE.with(|current| current.replace(role))
}
#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_thread_swap_activity_v1(activity: u32) -> u32 {
    ACTIVITY.with(|current| current.replace(activity))
}
fn main() {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let config = codex_roles::configure_from_env();
    if arguments[0] == "error" {
        assert!(config.unwrap_err().to_string().contains(&arguments[1]));
        return;
    }
    config.unwrap();
    assert_eq!(MODE.load(Ordering::Relaxed), arguments[0].parse::<u32>().unwrap());
    assert_eq!(CAP.load(Ordering::Relaxed), arguments[1].parse::<u32>().unwrap());
    {
        let _foreground = codex_roles::ChildRoleGuard::foreground();
        let _activity = codex_roles::ActivityGuard::foreground();
        assert_eq!(ROLE.with(Cell::get), 1);
        assert_eq!(ACTIVITY.with(Cell::get), 1);
        {
            let _pool = pool_roles::ChildRoleGuard::tokio_pool();
            assert_eq!(ROLE.with(Cell::get), 2);
            let worker = std::thread::spawn(|| {
                assert_eq!(ROLE.with(Cell::get), 0);
                assert_eq!(ACTIVITY.with(Cell::get), 0);
                let _scheduler = pool_roles::ActivityGuard::async_scheduler();
                assert_eq!(ACTIVITY.with(Cell::get), 2);
                {
                    let _blocking = pool_roles::ActivityGuard::blocking();
                    assert_eq!(ACTIVITY.with(Cell::get), 3);
                }
                assert_eq!(ACTIVITY.with(Cell::get), 2);
            });
            worker.join().unwrap();
            assert_eq!(ROLE.with(Cell::get), 2);
            assert_eq!(ACTIVITY.with(Cell::get), 1);
        }
        assert_eq!(ROLE.with(Cell::get), 1);
    }
    assert_eq!(ROLE.with(Cell::get), 0);
    assert_eq!(ACTIVITY.with(Cell::get), 0);
}
'''


def main() -> None:
    for source in [TOKIO_ROLES, CODEX_ROLES]:
        if not source.is_file():
            raise SystemExit(f"missing production source: {source}")
    with tempfile.TemporaryDirectory(prefix="trueos-codex-thread-roles-") as temporary:
        directory = Path(temporary)
        for index, source in enumerate([TOKIO_ROLES, CODEX_ROLES]):
            executable = directory / f"contracts-{index}"
            subprocess.run([
                "rustc", "--edition=2024", "--test", str(source), "-o", str(executable),
            ], check=True)
            subprocess.run([str(executable)], check=True)
        source = directory / "ffi.rs"
        source.write_text(HARNESS.replace("@TOKIO_ROLES@", json.dumps(str(TOKIO_ROLES)))
                          .replace("@CODEX_ROLES@", json.dumps(str(CODEX_ROLES))))
        executable = directory / "ffi"
        subprocess.run([
            "rustc", "--edition=2024", "--cfg", 'target_os="trueos"',
            "-Aexplicit_builtin_cfgs_in_flags", str(source), "-o", str(executable),
        ], check=True)
        env = dict(os.environ)
        for key in ["CODEX_TRUEOS_THREAD_PLACEMENT", "CODEX_TRUEOS_BACKGROUND_CARRIERS", "HARNESS_CONFIGURE_ERRNO"]:
            env.pop(key, None)
        cases = [
            ({}, ["0", "2"]),
            ({"CODEX_TRUEOS_THREAD_PLACEMENT": "isolated", "CODEX_TRUEOS_BACKGROUND_CARRIERS": "1"}, ["1", "1"]),
            ({"CODEX_TRUEOS_THREAD_PLACEMENT": "spread", "CODEX_TRUEOS_BACKGROUND_CARRIERS": "4"}, ["2", "4"]),
            ({"CODEX_TRUEOS_THREAD_PLACEMENT": "bad"}, ["error", "CODEX_TRUEOS_THREAD_PLACEMENT"]),
            ({"CODEX_TRUEOS_BACKGROUND_CARRIERS": "5"}, ["error", "CODEX_TRUEOS_BACKGROUND_CARRIERS"]),
            ({"HARNESS_CONFIGURE_ERRNO": "16"}, ["error", "configuration failed"]),
            ({"HARNESS_CONFIGURE_ERRNO": "95"}, ["0", "2"]),
            ({"CODEX_TRUEOS_THREAD_PLACEMENT": "single", "HARNESS_CONFIGURE_ERRNO": "95"}, ["error", "configuration failed"]),
            ({"CODEX_TRUEOS_BACKGROUND_CARRIERS": "2", "HARNESS_CONFIGURE_ERRNO": "95"}, ["error", "configuration failed"]),
        ]
        for settings, arguments in cases:
            subprocess.run([str(executable), *arguments], env=env | settings, check=True)
        print(f"TRUEOS role/activity FFI: {len(cases)} cases passed")


if __name__ == "__main__":
    main()
