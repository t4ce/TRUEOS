#!/usr/bin/env python3
"""Run real SDK A4 composition, wire upload and kernel admission together.

Only the native ABI entry context, clock/logging and blocking font filesystem
are host adapters. No printer connection or physical output is performed.
"""
from pathlib import Path
import subprocess
import tempfile
import runpy

ROOT = Path(__file__).resolve().parents[2]
SDK = ROOT.parent / "TRUEOS-Blueprints/crates/trueos-v/src/vprint2d.rs"

ADAPTERS = r'''
mod vfs_async {
    pub async fn read_file(_: &[u8]) -> Result<Vec<u8>,i32> { Err(-2) }
    pub fn block_on<F:std::future::Future>(future:F)->F::Output {
        let mut future=std::pin::pin!(future);
        match future.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())) {
            std::task::Poll::Ready(value)=>value,
            std::task::Poll::Pending=>panic!("unused font adapter must not block"),
        }
    }
}
#[path="__SDK__"] mod vprint2d;
thread_local! {
    static FAIL_APPEND:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};
    static CALLS:std::cell::RefCell<Vec<u32>>=const{std::cell::RefCell::new(Vec::new())};
}
#[unsafe(no_mangle)]
unsafe extern "C" fn trueos_vlayer_print2d_submit(kind:u32,subject:u64,raw:*const u8,len:usize)->i64 {
    CALLS.with(|calls|calls.borrow_mut().push(kind));
    if kind==4 && FAIL_APPEND.with(|fail|fail.replace(false)) { return -5; }
    let bytes=if len==0 {&[]}else{unsafe{std::slice::from_raw_parts(raw,len)}};
    r::print2d::submit_for_owner(240,kind,subject,bytes)
}
#[unsafe(no_mangle)]
extern "C" fn trueos_vlayer_print2d_status(id:u32)->i32 { r::print2d::status_for_owner(240,id) }

#[test]
fn sdk_chunks_commit_complete_native_document_and_abort_failures_without_printing() {
    use vprint2d::{a4,JobState};
    let mut page=a4::Page::white().unwrap();
    // High-entropy horizontal rows force several ABI chunks, not just a tiny blank fixture.
    for y in 0..200usize { for x in 0..a4::WIDTH as usize {
        let at=(y*a4::WIDTH as usize+x)*3;
        page.rgb_mut()[at..at+3].copy_from_slice(&[(x%251)as u8,((x+y)%253)as u8,((x*3+y)%255)as u8]);
    }}
    let mut document=a4::Document::new();document.push(&page).unwrap();document.push(&page).unwrap();
    assert!(document.bytes().len()>64*1024);
    assert_eq!(r::net::pwg_raster::validate_a4(document.bytes()).unwrap(),2);
    let id=document.submit().unwrap();
    assert_eq!(id.status().unwrap(),JobState::Queued);
    let job=r::print2d::take_next_job().expect("complete job admitted once");
    assert_eq!(job.id,id.get());assert_eq!(job.document,document.bytes());
    assert!(r::print2d::take_next_job().is_none());
    let calls=CALLS.with(|calls|calls.borrow().clone());
    assert_eq!(calls[0],3);assert_eq!(calls[calls.len()-1],5);assert!(calls.iter().filter(|kind|**kind==4).count()>1);
    r::print2d::transition(id.get(),r::print2d::PrintJobState::Completed,"host-fixture-complete");
    assert_eq!(id.status().unwrap(),JobState::Completed);
    FAIL_APPEND.with(|fail|fail.set(true));
    assert_eq!(document.submit().unwrap_err(),vprint2d::Error::Transport);
    assert_eq!(CALLS.with(|calls|*calls.borrow().last().unwrap()),6);
    assert!(r::print2d::take_next_job().is_none());
    assert!(vprint2d::submit_a4_pwg(b"broken").is_err());
    assert!(r::print2d::take_next_job().is_none());
}
'''

def main():
    harness = runpy.run_path(str(ROOT / "tools/testpy/test_a4_print_queue.py"))["HARNESS"]
    harness = harness.replace("__PWG__", str(ROOT / "src/r/net/pwg_raster.rs"))
    harness = harness.replace("__QUEUE__", str(ROOT / "src/r/print2d.rs"))
    harness = harness.replace("__IPP__", str(ROOT / "src/r/net/ipp_print.rs"))
    harness += ADAPTERS.replace("__SDK__", str(SDK))
    with tempfile.TemporaryDirectory(prefix="trueos-a4-producers-") as folder:
        folder = Path(folder)
        source = folder / "actual.rs"
        source.write_text(harness)
        for profile, optimization in [("debug", "0"), ("optimized", "3")]:
            binary = folder / profile
            subprocess.run(["rustc", "+nightly-2026-07-10", "--edition=2024", "--test",
                            "-C", "opt-level="+optimization, str(source), "-o", str(binary)],
                           check=True, cwd="/var/tmp")
            print("a4-producers profile="+profile, flush=True)
            subprocess.run([str(binary)], check=True)

if __name__ == "__main__":
    main()
