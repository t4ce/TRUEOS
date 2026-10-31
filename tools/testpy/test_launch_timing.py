#!/usr/bin/env python3
"""Replay the production host launch-span helper against a shared test clock."""
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

HARNESS = r'''#![allow(dead_code)]
extern crate self as trueos_vm;
pub mod vmcall { pub const BP_LOG_LEVEL_IMPORTANT: u32 = 17; }
use std::sync::{Mutex, atomic::{AtomicU64, Ordering}};
static CLOCK_MS: AtomicU64 = AtomicU64::new(1000);
static CLOCK_READS: AtomicU64 = AtomicU64::new(0);
static RECORDS: Mutex<Vec<String>> = Mutex::new(Vec::new());
mod r {
    pub mod platform {
        pub fn trueos_platform_monotonic_nanos() -> u64 {
            crate::CLOCK_READS.fetch_add(1, crate::Ordering::Relaxed);
            crate::CLOCK_MS.load(crate::Ordering::Relaxed) * 1_000_000
        }
    }
    pub mod io { pub mod cabi {
        pub unsafe fn trueos_cabi_log(level: u32, target: *const u8, target_len: usize,
            message: *const u8, len: usize) -> i32 {
            assert_eq!(level, crate::vmcall::BP_LOG_LEVEL_IMPORTANT);
            assert!(len <= 256);
            assert_eq!(unsafe { std::slice::from_raw_parts(target, target_len) }, b"blueprint-loader");
            let message = unsafe { std::slice::from_raw_parts(message, len) };
            crate::RECORDS.lock().unwrap().push(std::str::from_utf8(message).unwrap().to_owned());
            0
        }
    }}
}
#[path = @SOURCE@] mod launch_timing;
use launch_timing::{Status, Trace};

fn reset() {
    RECORDS.lock().unwrap().clear();
    CLOCK_MS.store(1000, Ordering::Relaxed);
    CLOCK_READS.store(0, Ordering::Relaxed);
}
fn field<'a>(record: &'a str, key: &str) -> &'a str {
    record.split_whitespace().skip(1).find_map(|part| part.split_once('=').filter(|(name,_)| *name == key).map(|(_,value)|value)).unwrap()
}
fn rows() -> Vec<String> { RECORDS.lock().unwrap().clone() }

#[test]
fn ordinary_archives_do_not_read_the_clock_or_emit_records() {
    reset();
    let mut span = Trace::for_archive("other.bp").span("prebind");
    span.complete();
    drop(span);
    assert!(rows().is_empty());
    assert_eq!(CLOCK_READS.load(Ordering::Relaxed), 0);
    let mut unrelated = Trace::for_archive("sqlite-other.bp").span("prebind");
    unrelated.complete();
    assert!(rows().is_empty());
}
#[test]
fn the_cold_sqlite_dependency_gets_its_own_request() {
    reset();
    let codex = Trace::for_archive("codex.bp");
    let sqlite = Trace::for_archive("/common/sqlite.bp");
    let mut span = sqlite.span("prebind");
    span.complete();
    assert_ne!(codex.request, sqlite.request);
    assert!(sqlite.request > 0);
    assert_eq!(rows().len(), 2);
    assert_eq!(field(&rows()[0], "request"), sqlite.request.to_string());
}
#[test]
fn request_survives_vm_assignment_and_shared_carrier_clock() {
    reset();
    let trace = Trace::for_archive("/apps/CODEX.bp");
    let mut prebind = trace.span("prebind");
    CLOCK_MS.store(1007, Ordering::Relaxed);
    prebind.complete();
    drop(prebind);
    let bound = trace.bind(9, 17);
    std::thread::spawn(move || {
        let mut entry = bound.span("guest_entry");
        CLOCK_MS.store(1023, Ordering::Relaxed);
        entry.complete();
    }).join().unwrap();
    let records = rows();
    assert_eq!(records.len(), 4);
    assert_eq!(field(&records[0], "vm"), "255");
    assert_eq!(field(&records[0], "gen"), "0");
    assert_eq!(field(&records[1], "elapsed_ms"), "7");
    assert_eq!(field(&records[2], "vm"), "9");
    assert_eq!(field(&records[2], "gen"), "17");
    assert_eq!(field(&records[3], "mono_ms"), "1023");
    assert_eq!(field(&records[3], "elapsed_ms"), "16");
    for record in &records {
        assert!(record.starts_with("trueos-startup-v1 "));
        assert_eq!(field(record, "request"), trace.request.to_string());
        assert_eq!(record.split_whitespace().count(), 9);
    }
}
#[test]
fn failures_cancellation_and_unfinished_queues_are_distinct() {
    reset();
    let trace = Trace::for_archive("codex-check.bp");
    let result: Result<(), i32> = trace.measure("prebind", || Err(22));
    assert_eq!(result, Err(22));
    {
        let _cancelled = trace.span("readiness");
        CLOCK_MS.store(1011, Ordering::Relaxed);
    }
    let pending = trace.begin("ap_queue");
    let records = rows();
    assert_eq!(records.len(), 5);
    assert_eq!(field(&records[1], "status"), "error");
    assert_eq!(field(&records[3], "status"), "cancelled");
    assert_eq!(field(&records[3], "elapsed_ms"), "11");
    assert_eq!(field(&records[4], "event"), "begin");
    assert_eq!(field(&records[4], "status"), "pending");
    CLOCK_MS.store(1041, Ordering::Relaxed);
    pending.finish(Status::Ok);
    let records = rows();
    assert_eq!(records.len(), 6);
    assert_eq!(field(&records[5], "elapsed_ms"), "30");
}
#[test]
fn successful_and_error_finishes_are_emitted_once() {
    reset();
    let trace = Trace::for_archive("codex.bp");
    let mut span = trace.span("vm_boot");
    CLOCK_MS.store(1008, Ordering::Relaxed);
    span.error();
    span.complete();
    drop(span);
    let records = rows();
    assert_eq!(records.len(), 2);
    assert_eq!(field(&records[1], "status"), "error");
    assert_eq!(field(&records[1], "elapsed_ms"), "8");
    assert_ne!(trace.request, Trace::for_archive("codex.bp").request);
}
'''


def main():
    with tempfile.TemporaryDirectory(prefix="trueos-launch-timing-") as directory:
        directory = Path(directory)
        source = directory / "harness.rs"
        source.write_text(HARNESS.replace("@SOURCE@", json.dumps(str(ROOT / "src/hv/launch_timing.rs"))))
        binary = directory / "harness"
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)], check=True, timeout=60)
        subprocess.run([str(binary), "--test-threads=1"], check=True, timeout=30)


if __name__ == "__main__":
    main()
