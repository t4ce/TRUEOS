#!/usr/bin/env python3
"""Exercise both archive client copies against an owner-operation C ABI mock."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
HARNESS = r'''
extern crate alloc;
mod bp_abi {
    #[derive(Default)]
    pub struct TrueosArchiveReport {
        pub input_bytes: u64, pub output_bytes: u64, pub file_count: u32, pub reserved: u32,
    }
}
mod vsys { pub fn log_record(_: u32, _: &str, _: &str) -> i32 { 0 } }
mod vcabi {
    use std::cell::Cell;
    thread_local! {
        pub static STEP: Cell<i32> = const { Cell::new(0) };
        pub static DISCARDS: Cell<u32> = const { Cell::new(0) };
        pub static READ_FAILURE: Cell<bool> = const { Cell::new(false) };
        pub static LENGTH: Cell<usize> = const { Cell::new(150000) };
    }
    pub unsafe fn trueos_cabi_archive_unpack_start(_: *const u8, _: usize, _: *const u8, _: usize) -> i32 { 7 }
    pub unsafe fn trueos_cabi_archive_pack_start(_: *const u8, _: usize, _: *const u8, _: usize) -> i32 { 7 }
    pub unsafe fn trueos_cabi_archive_pack_many_start(_: *const u8, _: usize, _: *const u8, _: usize) -> i32 { 7 }
    pub unsafe fn trueos_cabi_archive_lz4_decode_start_v1(_: *const u8, _: usize) -> i32 { 7 }
    pub unsafe fn trueos_cabi_archive_result_len_v1(id: u32) -> isize { assert_eq!(id,7); LENGTH.get() as isize }
    pub unsafe fn trueos_cabi_archive_result_read_v1(id: u32, offset: usize, out: *mut u8, cap: usize) -> isize {
        assert_eq!(id,7);
        assert_eq!(DISCARDS.get(),0, "result was discarded before the copy");
        if READ_FAILURE.get() { return -2; }
        let count = cap.min(1234).min(LENGTH.get()-offset);
        for i in 0..count { unsafe { *out.add(i) = ((offset+i)%251) as u8; } }
        count as isize
    }
    pub unsafe fn trueos_cabi_archive_status(id: u32) -> i32 { assert_eq!(id,7); STEP.get() }
    pub unsafe fn trueos_cabi_archive_report(id: u32, out: *mut crate::bp_abi::TrueosArchiveReport) -> i32 {
        assert_eq!(id,7);
        unsafe { *out = crate::bp_abi::TrueosArchiveReport {
            input_bytes: 100, output_bytes: 200, file_count: 2,
            reserved: if STEP.get() == 0 { 42 } else { 100 },
        }; }
        0
    }
    pub unsafe fn trueos_cabi_archive_discard(id: u32) -> i32 {
        assert_eq!(id,7); DISCARDS.set(DISCARDS.get()+1); 0
    }
}
#[path="CLIENT_PATH"] mod archive;
fn poll<F: std::future::Future>(future: std::pin::Pin<&mut F>) -> std::task::Poll<F::Output> {
    future.poll(&mut std::task::Context::from_waker(std::task::Waker::noop()))
}
#[test] fn progress_reaches_100_only_after_success_and_discards_once() {
    let seen = std::cell::RefCell::new(Vec::new());
    {
        let mut future = std::pin::pin!(archive::unpack_with_progress(b"archive", b"out", |p| seen.borrow_mut().push(p)));
        assert!(poll(future.as_mut()).is_pending());
        assert_eq!(*seen.borrow(), vec![42]);
        vcabi::STEP.set(1);
        let std::task::Poll::Ready(Ok(report)) = poll(future.as_mut()) else { panic!() };
        assert_eq!(report.file_count,2);
        assert_eq!(*seen.borrow(),vec![42,100]);
    }
    assert_eq!(vcabi::DISCARDS.get(),1);
}
#[test] fn failure_and_cancel_release_handle_without_100() {
    let seen = std::cell::RefCell::new(Vec::new());
    {
        let mut future = std::pin::pin!(archive::unpack_with_progress(b"archive", b"out", |p| seen.borrow_mut().push(p)));
        assert!(poll(future.as_mut()).is_pending());
        vcabi::STEP.set(-7);
        assert_eq!(poll(future.as_mut()),std::task::Poll::Ready(Err(-7)));
    }
    assert_eq!(*seen.borrow(),vec![42]);
    assert_eq!(vcabi::DISCARDS.get(),1);
    vcabi::STEP.set(0);
    {
        let mut future = std::pin::pin!(archive::unpack(b"archive", b"out"));
        assert!(poll(future.as_mut()).is_pending());
    }
    assert_eq!(vcabi::DISCARDS.get(),2);
}
#[test] fn ram_decode_copies_partial_reads_before_discard() {
    vcabi::STEP.set(1);
    {
        let mut future = std::pin::pin!(archive::decode_lz4_to_memory(b"archive", 150000));
        let std::task::Poll::Ready(Ok(bytes)) = poll(future.as_mut()) else { panic!() };
        assert_eq!(bytes.len(),150000);
        assert!(bytes.iter().enumerate().all(|(i,b)| *b==(i%251) as u8));
    }
    assert_eq!(vcabi::DISCARDS.get(),1);
}
#[test] fn ram_decode_limit_read_error_and_cancel_release_results() {
    vcabi::STEP.set(1);
    {
        let mut future = std::pin::pin!(archive::decode_lz4_to_memory(b"archive", 16));
        assert_eq!(poll(future.as_mut()),std::task::Poll::Ready(Err(-7)));
    }
    assert_eq!(vcabi::DISCARDS.get(),1);
    vcabi::DISCARDS.set(0);
    vcabi::READ_FAILURE.set(true);
    {
        let mut future = std::pin::pin!(archive::decode_lz4_to_memory(b"archive", 150000));
        assert_eq!(poll(future.as_mut()),std::task::Poll::Ready(Err(-2)));
    }
    assert_eq!(vcabi::DISCARDS.get(),1);
    vcabi::STEP.set(0);
    {
        let mut future = std::pin::pin!(archive::decode_lz4_to_memory(b"archive", 150000));
        assert!(poll(future.as_mut()).is_pending());
    }
    assert_eq!(vcabi::DISCARDS.get(),2);
}
#[test] fn ram_progress_distinguishes_decode_and_copy_and_yields_for_cancel() {
    use archive::MemoryProgress::{Decoding, Copying};
    let total = 3 * 1024 * 1024;
    vcabi::LENGTH.set(total);
    let seen = std::cell::RefCell::new(Vec::new());
    {
        let mut future = std::pin::pin!(archive::decode_lz4_to_memory_with_progress(b"archive", total, |p| seen.borrow_mut().push(p)));
        assert!(poll(future.as_mut()).is_pending());
        assert_eq!(*seen.borrow(), vec![Decoding { percent: 42 }]);
        vcabi::STEP.set(1);
        assert!(poll(future.as_mut()).is_pending());
        let reports = seen.borrow();
        assert_eq!(reports[1], Decoding { percent: 100 });
        assert_eq!(reports[2], Copying { copied: 0, total });
        assert!(matches!(reports.last(), Some(Copying { copied, .. }) if *copied > 0 && *copied < total));
        assert_eq!(vcabi::DISCARDS.get(), 0);
    }
    assert_eq!(vcabi::DISCARDS.get(), 1);
    vcabi::DISCARDS.set(0);
    seen.borrow_mut().clear();
    {
        let mut future = std::pin::pin!(archive::decode_lz4_to_memory_with_progress(b"archive", total, |p| seen.borrow_mut().push(p)));
        let bytes = loop {
            match poll(future.as_mut()) {
                std::task::Poll::Ready(Ok(bytes)) => break bytes,
                std::task::Poll::Pending => {},
                other => panic!("unexpected {other:?}"),
            }
        };
        assert_eq!(bytes.len(), total);
        assert!(bytes.iter().enumerate().all(|(i,b)| *b == (i % 251) as u8));
    }
    assert_eq!(seen.borrow().last(), Some(&Copying { copied: total, total }));
    let copies: Vec<_> = seen.borrow().iter().filter_map(|p| match p { Copying { copied, .. } => Some(*copied), _ => None }).collect();
    assert!(copies.windows(2).all(|w| w[0] < w[1]));
    assert_eq!(vcabi::DISCARDS.get(), 1);
}
'''

def main():
    with tempfile.TemporaryDirectory(prefix='trueos-archive-progress-') as td:
        folder = Path(td)
        for repo in [ROOT, ROOT.parent / 'TRUEOS-Blueprints']:
            source = folder / 'tests.rs'
            source.write_text(HARNESS.replace('CLIENT_PATH', str(repo / 'crates/trueos-v/src/varchive.rs')))
            binary = folder / 'tests'
            subprocess.run(['rustc', '+nightly-2026-07-10', '--edition=2024', '--test', str(source), '-o', str(binary)], check=True)
            subprocess.run([str(binary)], check=True)

if __name__ == '__main__':
    main()
