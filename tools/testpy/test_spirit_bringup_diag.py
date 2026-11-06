#!/usr/bin/env python3
"""Exercise retained production Spirit diagnostics without GPU hardware."""
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    channel = re.search(r'^channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel
    source = ROOT / "src/spirit/bringup_diag.rs"
    harness = r'''
extern crate alloc;
use core::sync::atomic::{AtomicU64, Ordering};
const SPIRIT_FENCE_COUNT: usize = 4;
static COMPLETED: [AtomicU64; SPIRIT_FENCE_COUNT] = [const { AtomicU64::new(0) }; SPIRIT_FENCE_COUNT];
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
#[derive(Clone, Copy)]
struct SpiritFenceId(usize);
impl SpiritFenceId { fn index(self) -> usize { self.0 } }
struct Instant;
impl Instant {
    fn now() -> Self { Self }
    fn as_millis(&self) -> u64 { 1234 }
}
#[path = "@SOURCE@"] mod bringup_diag;

#[test]
fn producer_release_is_distinct_from_scanout_and_pending_polls_are_bounded() {
    let id = SpiritFenceId(0);
    bringup_diag::stage(id, "cursor-preparing");
    bringup_diag::submitted(id, 17);
    for polls in 1..=13 { bringup_diag::producer_pending(id, polls); }
    bringup_diag::released(id);
    let text = bringup_diag::snapshot_text();
    let line = text.lines().find(|line| line.starts_with("spirit fence=0 ")).unwrap();
    assert!(line.contains("sequence: 17"));
    assert!(line.contains("submitted: 1, released: 1, visible: 0"));
    assert!(line.contains("producer_polls: 8"));
    bringup_diag::cursor_armed(id, 17, 0x1000);
    bringup_diag::display(id, false, 1, 0x31, 0x1000, 0x2000);
    bringup_diag::display(id, true, 3, 0x31, 0x1000, 0x1000);
    COMPLETED[0].store(17, Ordering::Release);
    let text = bringup_diag::snapshot_text();
    let line = text.lines().find(|line| line.starts_with("spirit fence=0 ")).unwrap();
    assert!(line.contains("completed_sequence=17"));
    assert!(line.contains("visible: 1"));
    assert!(line.contains("cursor_sequence: 17, expected_cursor_gpu: 4096"));
    assert!(line.contains("base: 4096, live: 4096"));
}

#[test]
fn retained_failure_survives_a_last_frame_rearm() {
    let id = SpiritFenceId(1);
    bringup_diag::submitted(id, 9);
    bringup_diag::failed(id, "producer-completion-failed");
    bringup_diag::cursor_armed(id, 8, 0x3000);
    bringup_diag::display(id, true, 0, 1, 0x3000, 0x3000);
    let text = bringup_diag::snapshot_text();
    let line = text.lines().find(|line| line.starts_with("spirit fence=1 ")).unwrap();
    assert!(line.contains("producer-completion-failed"));
    assert!(line.contains("submitted: 1, released: 0, visible: 1"));
}
'''.replace("@SOURCE@", str(source))
    with tempfile.TemporaryDirectory(prefix="trueos-spirit-bringup-") as directory:
        work = Path(directory)
        (work / "test.rs").write_text(harness)
        binary = work / "tests"
        subprocess.run(["rustc", "+" + channel.group(1), "--edition=2024", "--test",
                        "--target", "x86_64-unknown-linux-gnu", str(work / "test.rs"),
                        "-o", str(binary)], cwd=work, check=True)
        subprocess.run([str(binary)], cwd=work, check=True)


if __name__ == "__main__":
    main()
