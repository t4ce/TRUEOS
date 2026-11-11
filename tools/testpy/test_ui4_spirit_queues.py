#!/usr/bin/env python3
"""Verify production owner queues retain queue-time Spirit permits through regrant.

The grant registry and broker metadata are narrow adapters. Queueing, dequeue,
state coalescing and physical-key delivery are production input_broker items.
"""
from pathlib import Path
import subprocess
import tempfile
import re

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / "src/ui4/input_broker.rs").read_text()
    declarations = [
        "struct OwnerQueue", "struct QueuedOwnerEvent", "struct PhysicalOwnerQueue",
        "struct QueuedKeyboardEvent", "pub(crate) fn take_owner_input_events(",
        "fn enqueue_physical_owner_event(", "pub(super) fn take_physical_owner_events(",
        "fn owner_event_stamp(", "fn queued_owner_event_allowed(", "fn owner_event_allowed(",
        "fn enqueue_owner_event(", "fn owner_event_is_state_sample(", "fn coalesce_owner_state_sample(",
    ]
    production = "\n".join(
        ("#[derive(Clone,Copy)]\n" if name in ["struct QueuedOwnerEvent", "struct QueuedKeyboardEvent"] else "") + item(source, name)
        for name in declarations
    )
    fixture = (ROOT / "tools/testpy/fixtures/ui4_spirit_queues.rs").read_text().replace("@PRODUCTION@", production)
    fixture = fixture.replace("@SNAPSHOTS@", "\n".join(item(source, name) for name in [
        "fn focused_keyboard_state(", "fn window_input_routes(",
    ]))
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-spirit-queues-") as directory:
        directory = Path(directory)
        (directory / "src").mkdir()
        (directory / "src/lib.rs").write_text(fixture)
        (directory / "Cargo.toml").write_text(
            '[package]\nname="trueos-spirit-owner-queue-tests"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[dependencies]\nheapless="=0.8.0"\nspin="0.10"\n'
        )
        subprocess.run([
            "cargo", "+" + channel, "test", "--offline", "--manifest-path", str(directory / "Cargo.toml"),
            "--target", "x86_64-unknown-linux-gnu", "--target-dir", str(ROOT / "bld/test-cache/spirit-owner-queues"),
            "--", "--test-threads=1",
        ], cwd=directory, check=True)


if __name__ == "__main__": main()
