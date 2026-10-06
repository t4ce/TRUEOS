#!/usr/bin/env python3
"""Exercise production broker SURFLIVE receipt functions on the host."""
from pathlib import Path
import subprocess
import tempfile

source = (Path(__file__).resolve().parents[2] / "src/ui4/window_broker.rs").read_text()


def function(signature: str) -> str:
    start = source.index(signature)
    end = source.index("\n}", start) + 2
    return source[start:end]


note = source[source.index("    fn note_background_surflive(") : source.index("\n    fn new(")]
harness = r'''
#![allow(dead_code)]
use std::sync::{Mutex as StdMutex, MutexGuard};

struct Mutex<T>(StdMutex<T>);
impl<T> Mutex<T> {
    const fn new(value: T) -> Self { Self(StdMutex::new(value)) }
    fn lock(&self) -> MutexGuard<'_, T> { self.0.lock().unwrap() }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WindowOwner(u8);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WindowId(u32);
impl WindowId { fn raw(self) -> u32 { self.0 } }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WindowState { Pending, Ready, Closing }
#[derive(Clone, Copy)]
struct WindowBackground { frame: u64, publish_serial: u64 }
struct WindowRecord {
    generation: u16,
    owner: WindowOwner,
    state: WindowState,
    revision: u64,
    background: Option<WindowBackground>,
    background_presented_serials: [u64; 8],
    background_presented_serial_cursor: u8,
    presented_serials: [u64; 8],
}
impl WindowRecord {
NOTE
}
#[derive(Clone, Copy)]
struct WindowSnapshot {
    layer: u8,
    id: WindowId,
    owner: WindowOwner,
    revision: u64,
    frame: u64,
    publish_serial: u64,
}
struct WindowBroker { windows: Vec<WindowRecord> }
static WINDOW_BROKER: Mutex<WindowBroker> = Mutex::new(WindowBroker { windows: Vec::new() });
#[derive(Debug)]
enum WindowBrokerError { InvalidHandle }
fn unpack_handle(raw: u32) -> Result<(usize, u16), WindowBrokerError> {
    let low = raw as u16;
    let generation = (raw >> 16) as u16;
    if low == 0 || generation == 0 { return Err(WindowBrokerError::InvalidHandle); }
    Ok((usize::from(low - 1), generation))
}
fn acknowledge_window_frame_revision(_: WindowId, _: u64, _: u64) -> bool {
    panic!("foreground path is outside this isolated background test")
}
QUERY_FOREGROUND
QUERY_BACKGROUND
ACK

#[test]
fn background_ack_is_exact_post_surflive_and_layer_local() {
    let owner = WindowOwner(3);
    let foreign = WindowOwner(4);
    let id = WindowId((1 << 16) | 1);
    let mut broker = WINDOW_BROKER.lock();
    broker.windows.clear();
    broker.windows.push(WindowRecord {
        generation: 1,
        owner,
        state: WindowState::Ready,
        revision: 2,
        background: Some(WindowBackground { frame: 50, publish_serial: 2 }),
        background_presented_serials: [0; 8],
        background_presented_serial_cursor: 0,
        presented_serials: [0; 8],
    });
    drop(broker);
    let first = WindowSnapshot {
        layer: 1, id, owner, revision: 2, frame: 50, publish_serial: 2,
    };
    assert!(!window_background_frame_was_presented(owner, id, 2));
    assert!(!acknowledge_window_surface(WindowSnapshot { owner: foreign, ..first }));
    assert!(!window_background_frame_was_presented(owner, id, 2));
    assert!(!acknowledge_window_surface(WindowSnapshot {
        id: WindowId((2 << 16) | 1), ..first
    }));
    assert!(!window_background_frame_was_presented(foreign, id, 2));

    // A newer broker revision does not erase an older physically displayed
    // frame, but it leaves newer damage unacknowledged.
    {
        let mut broker = WINDOW_BROKER.lock();
        broker.windows[0].revision = 3;
        broker.windows[0].background.as_mut().unwrap().publish_serial = 3;
    }
    assert!(!acknowledge_window_surface(first));
    assert!(window_background_frame_was_presented(owner, id, 2));
    assert!(!window_background_frame_was_presented(owner, id, 3));
    assert!(!window_frame_was_presented(owner, id, 2));
    assert!(acknowledge_window_surface(WindowSnapshot {
        revision: 3, publish_serial: 3, ..first
    }));
    assert!(window_background_frame_was_presented(owner, id, 3));

    // Frame replacement advances the background serial and rejects stale
    // damage acknowledgement.
    {
        let mut broker = WINDOW_BROKER.lock();
        broker.windows[0].revision = 4;
        broker.windows[0].background = Some(WindowBackground { frame: 51, publish_serial: 4 });
    }
    assert!(!acknowledge_window_surface(WindowSnapshot {
        revision: 3, publish_serial: 3, ..first
    }));
    assert!(acknowledge_window_surface(WindowSnapshot {
        revision: 4, frame: 51, publish_serial: 4, ..first
    }));
    assert!(window_background_frame_was_presented(owner, id, 4));
    assert!(!window_frame_was_presented(owner, id, 4));
    assert!(!window_background_frame_was_presented(owner, WindowId((2 << 16) | 1), 4));
}

#[test]
fn background_history_is_bounded() {
    let mut record = WindowRecord {
        generation: 1,
        owner: WindowOwner(3),
        state: WindowState::Ready,
        revision: 1,
        background: None,
        background_presented_serials: [0; 8],
        background_presented_serial_cursor: 0,
        presented_serials: [0; 8],
    };
    record.presented_serials[0] = 77;
    for serial in 1..=9 { record.note_background_surflive(serial); }
    assert!(!record.background_presented_serials.contains(&1));
    assert!(record.background_presented_serials.contains(&2));
    assert!(record.background_presented_serials.contains(&9));
    assert!(!record.background_presented_serials.contains(&77));
    assert_eq!(record.presented_serials[0], 77);
}
'''
harness = (
    harness.replace("NOTE", note)
    .replace("QUERY_FOREGROUND", function("pub(crate) fn window_frame_was_presented("))
    .replace("QUERY_BACKGROUND", function("pub(crate) fn window_background_frame_was_presented("))
    .replace("ACK", function("pub(super) fn acknowledge_window_surface(").replace("pub(super)", "pub(crate)", 1))
)

with tempfile.TemporaryDirectory() as directory:
    path = Path(directory)
    (path / "receipt.rs").write_text(harness)
    subprocess.run(
        ["rustc", "--edition=2024", "--test", str(path / "receipt.rs"), "-o", str(path / "receipt")],
        check=True,
    )
    subprocess.run([str(path / "receipt"), "--test-threads=1"], check=True)
