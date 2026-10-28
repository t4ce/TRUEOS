#!/usr/bin/env python3
"""Run production continuations/placement and bounded Tokio blocking experiments.

Only the platform boundary is replaced: Linux threads stand in for AP carriers,
Linux guarded mappings back the real continuation stacks, and host timers supply
time. The scheduler, stack swaps, wait queues, lifetime, WLS identities, timing,
diagnostics and placement policies are read from current production sources.
"""

import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
FIXTURES = Path(__file__).with_name("thread_scheduler")


def main() -> None:
    channel = re.search(r'^channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel
    threads = (ROOT / "src/r/threads.rs").read_text()
    # Executor task macros and platform admission below this boundary require
    # the booted kernel. Keep the complete Future implementation unchanged.
    threads = threads[:threads.index("#[trueos_executor::task(")]
    for module in ["context", "stack", "timing", "exit_trace", "placement"]:
        threads = threads.replace(f"mod {module};", f'#[path = "{ROOT}/src/r/threads/{module}.rs"] mod {module};')
    tests = (ROOT / "src/r/threads/tests.rs").read_text() + (FIXTURES / "experiments.rs").read_text()
    threads = threads.replace("mod tests;", "mod tests {\n" + tests + "\n}")
    wait = (ROOT / "src/wait.rs").read_text()
    wait = wait[wait.index("/// Register a waker"):wait.index("type JobFuture =")]
    # The adapters before this range depend on Embassy and the kernel executor;
    # WaitQueue and CompletionCell themselves are retained byte for byte.
    header = "use alloc::vec::Vec; use core::future::Future; use core::sync::atomic::{AtomicU32, Ordering}; use core::task::{Context, Poll, Waker}; use spin::Mutex; use crate::{TICK_HZ, now};\n"
    harness = (FIXTURES / "harness.rs").read_text().replace("@TRUEOS_ROOT@", str(ROOT))
    harness = harness.replace("@THREADS@", threads).replace("@WAIT@", header + wait)
    allcaps = (ROOT / "src/allcaps.rs").read_text()
    for name in ["VM_ID_LIMIT", "VM_CPU_SLOT_LIMIT", "WORKER_SLOT_COUNT", "CPU_TRACK_COUNT", "TLS_SLOT_COUNT"]:
        value = re.search(rf"pub const {name}: usize = ([0-9_* +]+);", allcaps)
        assert value, f"production topology constant not found: {name}"
        harness = harness.replace(f"@{name}@", value.group(1))
    buildins = Path(os.environ.get("TRUEOS_BUILDINS_ROOT", ROOT.parent / "TRUEOS-Buildins"))
    tokio = buildins / "vendor/tokio-1.54.1"
    assert (tokio / "Cargo.toml").is_file(), "Set TRUEOS_BUILDINS_ROOT to the buildins checkout containing the production Tokio port"
    with tempfile.TemporaryDirectory(prefix="trueos-thread-scheduler-") as directory:
        work = Path(directory)
        manifest = '''[package]
name = "trueos-thread-scheduler-tests"
version = "0.1.0"
edition = "2024"
[workspace]
[dependencies]
spin = "=0.10.1"
heapless = "=0.9.1"
tokio = { path = @TOKIO_PATH@, features = ["rt", "rt-multi-thread", "sync", "time"] }
[lib]
path = "lib.rs"
[lints.rust]
unexpected_cfgs = { level = "allow" }
'''
        (work / "Cargo.toml").write_text(manifest.replace("@TOKIO_PATH@", json.dumps(str(tokio))))
        (work / "Cargo.lock").write_text((FIXTURES / "Cargo.lock").read_text())
        (work / "lib.rs").write_text(harness)
        env = dict(os.environ)
        env["CARGO_TARGET_DIR"] = str(ROOT / "bld/thread-scheduler-host-tests")
        env["RUSTFLAGS"] = "--cfg thread_scheduler_harness -C force-frame-pointers=yes"
        result = subprocess.run([
            "cargo", "+" + channel.group(1), "test", "--offline", "--locked", "--lib",
            "--target", "x86_64-unknown-linux-gnu", "--", "--test-threads=1", "--nocapture",
        ], cwd=work, env=env)
        raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()
