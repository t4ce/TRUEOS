#!/usr/bin/env python3
"""Exercise both SDKs' production metadata decoder at the native ABI boundary."""
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SOURCES = (
    ROOT / "crates/trueos-v/src/vfs_async.rs",
    ROOT.parent / "TRUEOS-Buildins/vendor/trueos-v/src/vfs_async.rs",
)


def item(source, signature):
    start = source.index(signature)
    position = source.index("{", start) + 1
    depth = 1
    while depth:
        if source[position] == "{":
            depth += 1
        elif source[position] == "}":
            depth -= 1
        position += 1
    return source[start:position]


HARNESS = r'''
use core::future::{Future, poll_fn};
use core::task::{Context, Poll, Waker};
use std::cell::RefCell;
use std::collections::VecDeque;

struct Boundary {
    start: i32, statuses: VecDeque<i32>, len: isize, read: isize,
    bytes: [u8; 32], events: Vec<&'static str>, paths: Vec<Vec<u8>>, discards: Vec<u32>,
}
impl Default for Boundary {
    fn default() -> Self {
        let mut bytes = [0; 32];
        bytes[..4].copy_from_slice(&1u32.to_le_bytes());
        bytes[4..12].copy_from_slice(&0xfedc_ba98_7654_3210u64.to_le_bytes());
        bytes[12..20].copy_from_slice(&0x8765_4321_fedc_ba98u64.to_le_bytes());
        bytes[20..24].copy_from_slice(&0xfedc_ba98u32.to_le_bytes());
        bytes[24..32].copy_from_slice(&0x9876_5432_fedc_ba10u64.to_le_bytes());
        Self { start: 17, statuses: VecDeque::new(), len: 32, read: 32,
            bytes, events: Vec::new(), paths: Vec::new(), discards: Vec::new() }
    }
}
thread_local! { static BOUNDARY: RefCell<Boundary> = RefCell::new(Boundary::default()); }
fn boundary<T>(f: impl FnOnce(&mut Boundary) -> T) -> T {
    BOUNDARY.with(|state| f(&mut state.borrow_mut()))
}
fn reset() { boundary(|state| *state = Boundary::default()); }

mod vcabi {
    use super::boundary;
    pub unsafe fn trueos_cabi_async_fs_metadata_start_v2(path: *const u8, len: usize) -> i32 {
        boundary(|state| {
            state.events.push("start");
            state.paths.push(unsafe { core::slice::from_raw_parts(path, len) }.to_vec());
            state.start
        })
    }
    pub @SAFETY@fn trueos_cabi_async_fs_status(id: u32) -> i32 {
        assert_eq!(id, 17);
        boundary(|state| { state.events.push("status"); state.statuses.pop_front().unwrap_or(1) })
    }
    pub @SAFETY@fn trueos_cabi_async_fs_result_len(id: u32) -> isize {
        assert_eq!(id, 17);
        boundary(|state| { state.events.push("length"); state.len })
    }
    pub unsafe fn trueos_cabi_async_fs_result_read(id: u32, offset: usize, out: *mut u8, cap: usize) -> isize {
        assert_eq!((id, offset, cap), (17, 0, 32));
        boundary(|state| {
            state.events.push("read");
            let count = usize::try_from(state.read).unwrap_or(0).min(cap);
            unsafe { core::ptr::copy_nonoverlapping(state.bytes.as_ptr(), out, count); }
            state.read
        })
    }
    pub @SAFETY@fn trueos_cabi_async_fs_discard(id: u32) -> i32 {
        boundary(|state| { state.events.push("discard"); state.discards.push(id); });
        0
    }
}

// @PRODUCTION@

fn complete(future: impl Future<Output = Result<MetadataV2, i32>>) -> Result<MetadataV2, i32> {
    let mut future = core::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    for _ in 0..8 {
        if let Poll::Ready(result) = future.as_mut().poll(&mut context) { return result; }
    }
    panic!("metadata stayed pending");
}

#[test]
fn decodes_exact_record_fields_and_preserves_utf8_selector_path() {
    reset();
    let path = "trueosfs:disc7/dir/Größe.txt".as_bytes();
    let metadata = complete(metadata_v2(path)).unwrap();
    assert_eq!(metadata, MetadataV2 { kind: NodeKind::File, len: 0xfedc_ba98_7654_3210,
        file_id: 0x8765_4321_fedc_ba98, root_id: 0xfedc_ba98, revision: 0x9876_5432_fedc_ba10 });
    assert!(metadata.is_file());
    assert!(!metadata.is_dir());
    assert_eq!(metadata.len(), 0xfedc_ba98_7654_3210);
    boundary(|state| {
        assert_eq!(state.paths, [path.to_vec()]);
        assert_eq!(state.discards, [17]);
        assert_eq!(state.events, ["start", "status", "length", "read", "discard"]);
    });
    reset();
    boundary(|state| { state.bytes = [0; 32]; state.bytes[..4].copy_from_slice(&2u32.to_le_bytes()); });
    assert_eq!(complete(metadata_v2(b"/")), Ok(MetadataV2 {
        kind: NodeKind::Directory, len: 0, file_id: 0, root_id: 0, revision: 0,
    }));
    boundary(|state| assert_eq!(state.discards, [17]));
}

#[test]
fn malformed_payload_lengths_and_kinds_fail_and_release_operation() {
    for len in [0, 12, 16, 31, 33, isize::MAX, -17] {
        reset(); boundary(|state| state.len = len);
        assert_eq!(complete(metadata_v2(b"file")), Err(if len < 0 { len as i32 } else { ERR_IO }));
        boundary(|state| {
            assert_eq!(state.discards, [17]);
            assert!(!state.events.contains(&"read"));
        });
    }
    for kind in [0u32, 3, u32::MAX] {
        reset(); boundary(|state| state.bytes[..4].copy_from_slice(&kind.to_le_bytes()));
        assert_eq!(complete(metadata_v2(b"file")), Err(ERR_IO));
        boundary(|state| assert_eq!(state.discards, [17]));
    }
}

#[test]
fn rejects_partial_oversized_and_failed_result_reads() {
    for count in [0, 1, 31, 33, -8, -17] {
        reset(); boundary(|state| state.read = count);
        assert_eq!(complete(metadata_v2(b"file")), Err(if count < 0 { count as i32 } else { ERR_IO }));
        boundary(|state| assert_eq!(state.discards, [17]));
    }
}

#[test]
fn rejected_starts_do_not_discard_an_unowned_operation() {
    for start in [0, -8, -17] {
        reset(); boundary(|state| state.start = start);
        assert_eq!(complete(metadata_v2(b"file")), Err(if start == 0 { ERR_BAD_PARAM } else { start }));
        boundary(|state| {
            assert_eq!(state.events, ["start"]);
            assert!(state.discards.is_empty());
        });
    }
}

#[test]
fn ready_failures_release_once_and_pending_operations_can_finish() {
    for status in [-2, -8, -17] {
        reset(); boundary(|state| state.statuses.push_back(status));
        assert_eq!(complete(metadata_v2(b"file")), Err(status));
        boundary(|state| {
            assert_eq!(state.discards, [17]);
            assert!(!state.events.contains(&"length"));
        });
    }
    reset(); boundary(|state| state.statuses.extend([0, 0, 1]));
    assert!(complete(metadata_v2(b"file")).is_ok());
    boundary(|state| {
        assert_eq!(state.discards, [17]);
        assert_eq!(state.events.iter().filter(|event| **event == "status").count(), 3);
    });
}

#[test]
fn unpolled_and_cancelled_futures_respect_operation_ownership() {
    reset();
    drop(metadata_v2(b"file"));
    boundary(|state| assert!(state.events.is_empty()));
    boundary(|state| state.statuses.push_back(0));
    let mut future = Box::pin(metadata_v2(b"file"));
    assert!(future.as_mut().poll(&mut Context::from_waker(Waker::noop())).is_pending());
    boundary(|state| assert!(state.discards.is_empty()));
    drop(future);
    boundary(|state| {
        assert_eq!(state.events, ["start", "status", "discard"]);
        assert_eq!(state.discards, [17]);
    });
}
'''


class MetadataV2Boundary(unittest.TestCase):
    def test_both_sdk_boundaries(self):
        toolchain = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
        for source_path in SOURCES:
            with self.subTest(source=source_path):
                source = source_path.read_text()
                production = "\n".join((
                    "const ERR_IO: i32 = -2; const ERR_BAD_PARAM: i32 = -4;",
                    "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n" + item(source, "pub enum NodeKind"),
                    "#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n" + item(source, "pub struct MetadataV2"),
                    item(source, "impl MetadataV2"),
                    item(source, "struct Operation"),
                    item(source, "impl Operation"),
                    item(source, "impl Drop for Operation"),
                    item(source, "pub async fn metadata_v2("),
                ))
                # Root wrappers expose safe status access; Blueprint imports are unsafe.
                safety = "unsafe " if source_path == SOURCES[1] else ""
                harness = HARNESS.replace("// @PRODUCTION@", production).replace("@SAFETY@", safety)
                with tempfile.TemporaryDirectory(prefix="trueos-sdk-metadata-v2-") as temporary:
                    directory = Path(temporary)
                    rust = directory / "tests.rs"
                    binary = directory / "tests"
                    rust.write_text(harness)
                    subprocess.run(["rustc", "+" + toolchain, "--edition=2024", "--test",
                                    "-Dwarnings", "--target", "x86_64-unknown-linux-gnu",
                                    str(rust), "-o", str(binary)], check=True, timeout=60)
                    subprocess.run([str(binary)], check=True, timeout=60)


if __name__ == "__main__":
    unittest.main()
