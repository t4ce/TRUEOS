#!/usr/bin/env python3
"""Run both production SDK bounded-GET wrappers against their native ABI."""
from pathlib import Path
import re
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SOURCES = (
    ROOT / "crates/trueos-v/src/vnetfs.rs",
    ROOT.parent / "TRUEOS-Buildins/vendor/trueos-v/src/vnetfs.rs",
)


def item(source, signature):
    start = source.index(signature)
    cursor = source.index("{", start) + 1
    depth = 1
    while depth:
        depth += (source[cursor] == "{") - (source[cursor] == "}")
        cursor += 1
    return source[start:cursor]


HARNESS = r'''
use std::cell::RefCell;

struct Boundary {
    start: u32, len: isize, read: isize, payload: Vec<u8>,
    events: Vec<&'static str>, urls: Vec<Vec<u8>>, options: Vec<(u32, u64)>,
    capacities: Vec<usize>, discards: Vec<u32>,
}
impl Default for Boundary {
    fn default() -> Self {
        Self { start: 17, len: 4, read: 4, payload: vec![0, 255, 65, 128],
            events: Vec::new(), urls: Vec::new(), options: Vec::new(),
            capacities: Vec::new(), discards: Vec::new() }
    }
}
thread_local! { static BOUNDARY: RefCell<Boundary> = RefCell::new(Boundary::default()); }
fn boundary<T>(f: impl FnOnce(&mut Boundary) -> T) -> T {
    BOUNDARY.with(|state| f(&mut state.borrow_mut()))
}
fn reset() { boundary(|state| *state = Boundary::default()); }

mod vcabi {
    use super::boundary;
    pub unsafe fn trueos_cabi_net_fetch_bytes_start_with_options_v1(
        url: *const u8, len: usize, timeout: u32, limit: u64,
    ) -> u32 {
        boundary(|state| {
            state.events.push("start");
            state.urls.push(unsafe { core::slice::from_raw_parts(url, len) }.to_vec());
            state.options.push((timeout, limit));
            state.start
        })
    }
    pub unsafe fn trueos_cabi_net_fetch_bytes_result_len(id: u32) -> isize {
        assert_eq!(id, 17);
        boundary(|state| { state.events.push("length"); state.len })
    }
    pub unsafe fn trueos_cabi_net_fetch_bytes_read(id: u32, out: *mut u8, cap: usize) -> isize {
        assert_eq!(id, 17);
        boundary(|state| {
            state.events.push("read");
            state.capacities.push(cap);
            let count = usize::try_from(state.read).unwrap_or(0).min(cap).min(state.payload.len());
            if count != 0 {
                unsafe { core::ptr::copy_nonoverlapping(state.payload.as_ptr(), out, count); }
            }
            state.read
        })
    }
    pub @SAFETY@fn trueos_cabi_net_fetch_bytes_discard(id: u32) -> i32 {
        boundary(|state| { state.events.push("discard"); state.discards.push(id); });
        0
    }
}

// @PRODUCTION@

#[test]
fn valid_start_preserves_utf8_url_and_both_budget_edges() {
    for (timeout, limit) in [(1, 1), (90_000, 128 * 1024 * 1024), (25_000, 1024 * 1024)] {
        reset();
        let url = "https://trueos.eu/Größe.bp?q=1".as_bytes();
        assert_eq!(fetch_bytes_with_options(url, timeout, limit), Ok(17));
        boundary(|state| {
            assert_eq!(state.events, ["start"]);
            assert_eq!(state.urls, [url.to_vec()]);
            assert_eq!(state.options, [(timeout, limit as u64)]);
            assert!(state.discards.is_empty());
        });
    }
}

#[test]
fn invalid_start_budgets_are_rejected_before_the_abi_or_a_size_cast() {
    for (timeout, limit) in [(0, 1), (90_001, 1), (u32::MAX, 1),
        (1, 0), (1, 128 * 1024 * 1024 + 1), (1, usize::MAX)]
    {
        reset();
        assert_eq!(fetch_bytes_with_options(b"https://trueos.eu/apps", timeout, limit), Err(-4));
        boundary(|state| {
            assert!(state.events.is_empty());
            assert!(state.options.is_empty());
            assert!(state.discards.is_empty());
        });
    }
}

#[test]
fn rejected_native_start_does_not_claim_an_operation() {
    reset(); boundary(|state| state.start = 0);
    assert_eq!(fetch_bytes_with_options(b"https://trueos.eu/apps", 90_000, 1024 * 1024), Err(-4));
    boundary(|state| {
        assert_eq!(state.events, ["start"]);
        assert!(state.discards.is_empty());
    });
}

#[test]
fn exact_binary_body_is_copied_and_the_operation_is_released_once() {
    reset();
    assert_eq!(fetch_bytes_read_bounded(17, 4), Ok(vec![0, 255, 65, 128]));
    boundary(|state| {
        assert_eq!(state.events, ["length", "read", "discard"]);
        assert_eq!(state.capacities, [4]);
        assert_eq!(state.discards, [17]);
    });
}

#[test]
fn invalid_read_arguments_release_without_inspecting_or_allocating_a_body() {
    for (id, limit) in [(0, 4), (17, 0), (17, 128 * 1024 * 1024 + 1), (17, usize::MAX)] {
        reset();
        assert_eq!(fetch_bytes_read_bounded(id, limit), Err(-4));
        boundary(|state| {
            assert_eq!(state.events, ["discard"]);
            assert_eq!(state.discards, [id]);
        });
    }
}

#[test]
fn failed_or_oversized_lengths_are_released_before_the_body_read() {
    for len in [-2, -7, -8, -11, -115, 5, 128 * 1024 * 1024 + 1, isize::MAX] {
        reset(); boundary(|state| state.len = len);
        assert_eq!(fetch_bytes_read_bounded(17, 4), Err(if len < 0 { len as i32 } else { -7 }));
        boundary(|state| {
            assert_eq!(state.events, ["length", "discard"]);
            assert_eq!(state.discards, [17]);
        });
    }
}

#[test]
fn short_oversized_and_failed_reads_cannot_become_truncated_success() {
    for read in [0, 1, 3, 5, isize::MAX, -2, -7, -11, -115] {
        reset(); boundary(|state| state.read = read);
        assert_eq!(fetch_bytes_read_bounded(17, 4), Err(if read < 0 { read as i32 } else { -2 }));
        boundary(|state| {
            assert_eq!(state.events, ["length", "read", "discard"]);
            assert_eq!(state.capacities, [4]);
            assert_eq!(state.discards, [17]);
        });
    }
}

#[test]
fn an_empty_body_is_consumed_and_released_without_synthetic_bytes() {
    reset(); boundary(|state| { state.len = 0; state.read = 0; });
    assert_eq!(fetch_bytes_read_bounded(17, 4), Ok(Vec::new()));
    boundary(|state| {
        assert_eq!(state.events, ["length", "read", "discard"]);
        assert_eq!(state.capacities, [0]);
        assert_eq!(state.discards, [17]);
    });
}
'''


class BoundedFetchSdkBoundary(unittest.TestCase):
    def test_both_sdk_boundaries(self):
        toolchain = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
        for source_path in SOURCES:
            with self.subTest(source=source_path):
                source = source_path.read_text()
                production = "\n".join(item(source, signature) for signature in (
                    "pub fn fetch_bytes_with_options(",
                    "pub fn fetch_bytes_result_len(",
                    "pub fn fetch_bytes_read_bounded(",
                    "pub fn fetch_bytes_discard(",
                ))
                safety = "unsafe " if source_path == SOURCES[1] else ""
                harness = HARNESS.replace("// @PRODUCTION@", production).replace("@SAFETY@", safety)
                with tempfile.TemporaryDirectory(prefix="trueos-sdk-bounded-fetch-") as temporary:
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
