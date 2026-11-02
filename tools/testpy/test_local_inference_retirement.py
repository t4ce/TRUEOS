#!/usr/bin/env python3
"""Exercise retired inference and retained Spirit ABIs using production modules."""

from pathlib import Path
import json
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    abi = (ROOT / "crates/trueos-v/src/bp_abi.rs").read_text()
    status = re.search(r"pub struct TrueosLumenStatus \{[^}]+\}", abi).group()
    phase = re.search(r"pub const LUMEN_PHASE_ERROR: u32 = [^;]+;", abi).group()
    harness = r'''
#![allow(dead_code)]
extern crate self as v;
extern crate self as trueos_vm;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
static GUEST: AtomicBool = AtomicBool::new(false);
static AVAILABLE: AtomicBool = AtomicBool::new(true);
static PRESENTATIONS: AtomicU32 = AtomicU32::new(0);
static LAST_OP: AtomicU32 = AtomicU32::new(0);
#[macro_export] macro_rules! log_info { ($($tokens:tt)*) => {}; }
pub mod bp_abi {
    #[repr(C)] #[derive(Clone, Copy, Debug, Default)]
    @STATUS@
    @PHASE@
}
mod hv {
    pub fn current_hull_guest_context_vm_id() -> Option<u8> {
        super::GUEST.load(super::Ordering::Relaxed).then_some(1)
    }
}
pub mod vmcall {
    pub const STATUS_OK: u32 = 0;
    pub const OP_BP_SPIRIT_EMOTION_PLAY: u32 = 1;
    pub const OP_BP_SPIRIT_TEXT_PRESENT_SILENT: u32 = 2;
    pub const OP_BP_SPIRIT_MOVE: u32 = 3;
    pub fn call_with_payload(op: u32, _: u64, _: u64, _: &[u8], _: &mut [u8]) -> (u32,u64) {
        super::LAST_OP.store(op, super::Ordering::Relaxed);
        (STATUS_OK, (-19i64) as u64)
    }
    pub fn call(op: u32, a: u64, b: u64) -> (u32,u64) {
        call_with_payload(op,a,b,&[],&mut [])
    }
}
mod spirit {
    pub fn enqueue_reasoning_response(_: u64, _: &str) -> bool {
        super::PRESENTATIONS.fetch_add(1, super::Ordering::Relaxed);
        super::AVAILABLE.load(super::Ordering::Relaxed)
    }
    pub fn enqueue_emotion_words(_: &[&str]) -> Result<(), ()> {
        super::AVAILABLE.load(super::Ordering::Relaxed).then_some(()).ok_or(())
    }
    pub fn move_spirit_to(_: f64, _: f64) -> Result<(), ()> {
        super::AVAILABLE.load(super::Ordering::Relaxed).then_some(()).ok_or(())
    }
}
#[path = @LUMEN_PATH@] mod retired;
#[path = @SPIRIT_PATH@] mod presentation;

#[test]
fn retired_requests_fail_without_presentation_or_output_writes() {
    let before = PRESENTATIONS.load(Ordering::Relaxed);
    assert_eq!(retired::template_open(1,b"system"),-5);
    assert_eq!(retired::prompt_submit(1,1,&[],b"prompt"),-5);
    assert_eq!(retired::prompt_submit_with_no_argument_tool(1,1,&[],b"prompt",b"tool"),-5);
    assert_eq!(retired::tool_result_submit(1,1,&[],b"result"),-5);
    assert_eq!(retired::checkpoint_request(1),-5);
    assert_eq!(retired::restore_begin(1,usize::MAX),-5);
    assert_eq!(retired::restore_write(1,0,b"state"),-5);
    assert_eq!(retired::restore_commit(1),-5);
    assert_eq!(retired::spirit_response_present(1,1,b"text"),-5);
    let mut output = [0xa5;16];
    assert_eq!(retired::reply_read(1,&mut output),-5);
    assert_eq!(retired::checkpoint_read(1,0,&mut output),-5);
    assert_eq!(output,[0xa5;16]);
    let status=retired::status(1).unwrap();
    assert_eq!(status.phase,bp_abi::LUMEN_PHASE_ERROR);
    assert_eq!(status.error,-5);
    assert_eq!((status.reply_len,status.checkpoint_len,status.reply_tail_len),(0,0,0));
    assert_eq!(retired::close(1),0);
    assert_eq!(PRESENTATIONS.load(Ordering::Relaxed),before);
}

#[test]
fn legacy_c_imports_remain_safe_unavailable_endpoints() {
    unsafe {
        assert_eq!(retired::trueos_cabi_lumen_template_open(std::ptr::null(),usize::MAX),-5);
        assert_eq!(retired::trueos_cabi_lumen_prompt_submit(1,std::ptr::null(),0,std::ptr::null(),0),-5);
        assert_eq!(retired::trueos_cabi_lumen_prompt_submit_with_no_argument_tool_v1(1,std::ptr::null(),0,std::ptr::null(),0,std::ptr::null(),0),-5);
        assert_eq!(retired::trueos_cabi_lumen_tool_result_submit(1,std::ptr::null(),0,std::ptr::null(),0),-5);
        assert_eq!(retired::trueos_cabi_lumen_status(std::ptr::null_mut()),-5);
        assert_eq!(retired::trueos_cabi_lumen_reply_read(std::ptr::null_mut(),usize::MAX),-5);
        assert_eq!(retired::trueos_cabi_lumen_checkpoint_read(0,std::ptr::null_mut(),usize::MAX),-5);
        assert_eq!(retired::trueos_cabi_lumen_restore_write(0,std::ptr::null(),usize::MAX),-5);
        assert_eq!(retired::trueos_cabi_spirit_response_present(1,std::ptr::null(),0),-5);
    }
    assert_eq!(retired::trueos_cabi_lumen_checkpoint_request(),-5);
    assert_eq!(retired::trueos_cabi_lumen_restore_begin(usize::MAX),-5);
    assert_eq!(retired::trueos_cabi_lumen_restore_commit(),-5);
    assert_eq!(retired::trueos_cabi_lumen_close(),0);
}

#[test]
fn spirit_text_remains_bounded_and_independent_of_inference() {
    GUEST.store(false,Ordering::Relaxed);
    AVAILABLE.store(true,Ordering::Relaxed);
    for (turn,text) in [(0,b"text".as_slice()),(1,b""),(1,b" \n\t"),(1,&[0xff]),(1,&[b'x';4097])] {
        assert_eq!(presentation::spirit_text_present_silent(turn,text),-3);
    }
    assert_eq!(presentation::spirit_text_present_silent(1,b"Codex text"),0);
    AVAILABLE.store(false,Ordering::Relaxed);
    assert_eq!(presentation::spirit_text_present_silent(2,b"busy"),-5);
    AVAILABLE.store(true,Ordering::Relaxed);
    assert_eq!(presentation::spirit_move(f32::NAN,0.5),-3);
    assert_eq!(presentation::spirit_move(-0.1,0.5),-3);
    assert_eq!(presentation::spirit_move(0.5,1.1),-3);
    assert_eq!(presentation::spirit_move(0.5,0.5),0);
    unsafe { assert_eq!(presentation::trueos_cabi_spirit_emotion_play(b"happy".as_ptr(),5),0); }
}

#[test]
fn guest_spirit_exports_still_forward_through_vmcall() {
    GUEST.store(true,Ordering::Relaxed);
    unsafe {
        assert_eq!(presentation::trueos_cabi_spirit_text_present_silent(1,b"text".as_ptr(),4),-19);
        assert_eq!(LAST_OP.load(Ordering::Relaxed),vmcall::OP_BP_SPIRIT_TEXT_PRESENT_SILENT);
        assert_eq!(presentation::trueos_cabi_spirit_emotion_play(b"happy".as_ptr(),5),-19);
        assert_eq!(LAST_OP.load(Ordering::Relaxed),vmcall::OP_BP_SPIRIT_EMOTION_PLAY);
    }
    assert_eq!(presentation::trueos_cabi_spirit_move(0.5,0.5),-19);
    assert_eq!(LAST_OP.load(Ordering::Relaxed),vmcall::OP_BP_SPIRIT_MOVE);
    GUEST.store(false,Ordering::Relaxed);
}
'''
    harness = harness.replace("@STATUS@", status).replace("@PHASE@", phase)
    harness = harness.replace("@LUMEN_PATH@", json.dumps(str(ROOT / "src/ai/lumen_service.rs")))
    harness = harness.replace("@SPIRIT_PATH@", json.dumps(str(ROOT / "src/ai/spirit_abi.rs")))
    with tempfile.TemporaryDirectory(prefix="trueos-inference-retirement-") as temp:
        work = Path(temp)
        source, binary = work / "test.rs", work / "test"
        source.write_text(harness)
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
