#!/usr/bin/env python3
"""Run production SSH owner/router scheduling with deterministic host boundaries.

Connection/worker/task code, bounded queues, wait registration, and SSH readiness
predicates come from production. Only encryption, AP placement, and time are
controlled fixtures; no sockets, account state, or hardware are touched.
"""
from pathlib import Path
import re
import subprocess
import tempfile

from test_net_queue_wake import production_wait_queue
from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]


def main():
    adapter = (ROOT / "src/net/adapter.rs").read_text()
    ssh = (ROOT / "src/shell3/ssh.rs").read_text()
    transport = (ROOT / "src/shell3/net.rs").read_text()
    transport = re.sub(r"#\[trueos_executor::task(?:\([^\n]*\))?\]\n", "", transport)
    transport = transport.replace("//!", "//")
    fixture = (ROOT / "tools/testpy/fixtures/ssh_scheduling.rs").read_text()
    fields = {
        "WAIT": production_wait_queue(),
        "QUEUE": item(adapter, "pub struct NetQueue<T>") + "\n" + item(adapter, "impl<T> NetQueue<T>"),
        "SSH_READINESS": item(ssh, "pub(super) fn needs_progress(&self)") + "\n"
        + item(ssh, "pub(super) fn next_deadline(&self)"),
        "TRANSPORT": transport,
    }
    for key, value in fields.items():
        fixture = fixture.replace("@" + key + "@", value)
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-ssh-scheduling-") as directory:
        work = Path(directory)
        (work / "lib.rs").write_text(fixture)
        (work / "Cargo.toml").write_text('''[package]
name="ssh-scheduling-tests"
version="0.0.0"
edition="2024"
[workspace]
[lib]
path="lib.rs"
[dependencies]
spin="0.10"
''')
        subprocess.run([
            "cargo", "+" + channel, "test", "--offline", "--manifest-path", str(work / "Cargo.toml"),
            "--target", "x86_64-unknown-linux-gnu", "--target-dir", str(ROOT / "bld/test-cache/ssh-scheduling"),
            "--", "--test-threads=1",
        ], cwd=work, check=True)


if __name__ == "__main__":
    main()
