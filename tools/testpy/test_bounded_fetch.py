#!/usr/bin/env python3
"""Exercise production bounded GET admission, deadlines, framing and ownership.

TLS I/O, the clock, task scheduler and VM shared page are deterministic mocks.
Actual HTTP parsing, redirect policy, result maps, cancellation and VM start arm
are extracted from production. No sockets, kernel image build or rig operation.
"""
from pathlib import Path
import re
import subprocess
import tempfile

import test_media_https_redirects as media
from test_fetch_final_url import item, vm_arm

ROOT = Path(__file__).resolve().parents[2]
HTTPS = (ROOT / "src/r/net/https.rs").read_text()
VM = (ROOT / "src/hv/vmcall.rs").read_text()


def production():
    selected = [media.PRODUCTION]
    for name in ["BOUNDED_FETCH_TIMEOUT_MAX_MS", "BOUNDED_FETCH_BODY_MAX_BYTES", "HTTPS_RESPONSE_OVERHEAD_MAX"]:
        selected.append(re.search(r"(?m)^const " + name + r":.*;$", HTTPS).group(0))
    selected.append("#[derive(Clone, Copy, Debug, Eq, PartialEq)]\nstruct JsonPostCancelled;")
    for signature in [
        "pub(crate) fn bounded_fetch_options(", "pub(crate) fn bounded_fetch_request(",
        "fn bounded_fetch_error_code(", "fn fetch_error_to_code(",
        "async fn await_bounded_fetch", "async fn fetch_bounded_response(",
        "pub(crate) fn cabi_net_fetch_bytes_start_bounded_host(",
        'pub unsafe extern "C" fn trueos_cabi_net_fetch_bytes_start_with_options_v1(',
        "async fn await_json_post_or_cancel", "fn register_json_post_cancellation(",
        "fn cancel_json_post_operation(", "fn finish_json_post_operation(",
        "pub(crate) fn bounded_fetch_accessible_to_vm(",
        "pub(crate) fn discard_bounded_fetches_for_vm(",
        "pub(crate) fn cabi_net_fetch_bytes_discard_host(",
        "pub(crate) fn cabi_net_fetch_bytes_result_len_host(",
        "fn response_transport_limit(", "fn decode_chunked(",
    ]:
        selected.append(item(HTTPS, signature))
    codes = (ROOT / "src/r/cabi_codes.rs").read_text()
    for name in ["NET_ERR_TIMEOUT", "NET_ERR_RESPONSE_FRAMING", "NET_ERR_BAD_URL", "NET_ERR_HTTP",
                 "FS_ERR_TOO_LARGE", "FS_ERR_BAD_PARAM", "FS_ERR_NOT_FOUND"]:
        selected.append(re.search(r"(?m)^pub const " + name + r":.*;$", codes).group(0))
    selected.append(media.VM_CONSTANTS)
    constants = (ROOT / "crates/trueos-vm/src/vmcall.rs").read_text()
    selected.append(re.search(r"(?m)^pub const OP_BP_FETCH_BYTES_START_WITH_OPTIONS_V1:.*;$", constants).group(0)
                    .replace("pub const OP_BP_FETCH_BYTES_START_WITH_OPTIONS_V1", "const SDK_BOUNDED_OPCODE"))
    selected.append(re.search(r"(?m)^pub const OP_BP_FETCH_BYTES_START_WITH_OPTIONS_V1:.*;$", VM).group(0))
    selected.append(media.VM_DISPATCH)
    selected.append("fn dispatch_bounded_start(vm_id:u8,arg0:u64,arg1:u64,req_len:u32)->DispatchOutcome {\n"
                    "let seq=8;match OP_BP_FETCH_BYTES_START_WITH_OPTIONS_V1 {\n" +
                    vm_arm(VM, "OP_BP_FETCH_BYTES_START_WITH_OPTIONS_V1") + "\n _=>unreachable!(),}}")
    return "\n".join(selected)


def stubs():
    source = media.STUBS.replace("extern crate alloc;", "extern crate alloc;\nuse alloc::sync::Arc;")
    source = source.replace(
        "mod hv {pub fn current_hull_guest_context_vm_id()",
        "mod hv {pub fn vm_run_generation(vm:u8)->Option<u64> {if vm==1 {crate::GENERATION.with(|v|v.get())}else{Some(22)}} pub fn current_hull_guest_context_vm_id()")
    source = source.replace("fetch_bytes_request_context,cabi_net_fetch_bytes_start_with_context_host};",
                            "fetch_bytes_request_context,cabi_net_fetch_bytes_start_with_context_host,bounded_fetch_request,cabi_net_fetch_bytes_start_bounded_host};")
    source = source.replace("pub use crate::{PAYLOAD_CAP,OP_BP_FETCH_BYTES_START,STATUS_OK};",
                            "pub use crate::{PAYLOAD_CAP,OP_BP_FETCH_BYTES_START,STATUS_OK};pub const OP_BP_FETCH_BYTES_START_WITH_OPTIONS_V1:u32=crate::SDK_BOUNDED_OPCODE;")
    source = source.replace("assert_eq!(op,OP_BP_FETCH_BYTES_START);", "assert!([OP_BP_FETCH_BYTES_START,OP_BP_FETCH_BYTES_START_WITH_OPTIONS_V1].contains(&op));")
    source = source.replace("crate::dispatch_start(1,a,payload.len() as u32);", "if op==OP_BP_FETCH_BYTES_START {crate::dispatch_start(1,a,payload.len() as u32);} else {crate::dispatch_bounded_start(1,a,b,payload.len() as u32);}")
    return source + r'''
static CABI_JSON_POST_CANCELLATIONS:Mutex<alloc::collections::BTreeMap<u32,Arc<JsonPostCancellation>>>=
    Mutex::new(alloc::collections::BTreeMap::new());
thread_local! {
    static GENERATION:Cell<Option<u64>>=const {Cell::new(Some(7))};
    static TASKS:RefCell<Vec<core::pin::Pin<Box<dyn Future<Output=()>>>>>=const {RefCell::new(Vec::new())};
}
mod wait {
    pub fn spawn_local_detached(future:impl core::future::Future<Output=()>+'static) {
        crate::TASKS.with(|tasks|tasks.borrow_mut().push(Box::pin(future)));
    }
}
mod surfer {pub mod html_shack {
    pub struct BytesResponse {pub url:String,pub bytes:Vec<u8>}
    pub async fn fetch_bytes_via_pool(_:String,_:u64,_:usize)->Result<BytesResponse,String> {Err("host fixture does not run HTTP pool".into())}
}}
'''


TESTS = r'''
#[cfg(test)]mod bounded_tests {
    use super::*;
    const URL:&str="https://catalog.test/p/actual.bp?signature=a%2Fb";
    fn reply(status:u16,location:Option<&str>,elapsed:u64)->Step {
        Step::Reply{status,location:location.map(String::from),body:vec![1,2,3],elapsed,cancel:false}
    }
    fn setup(steps:impl IntoIterator<Item=Step>) {
        assert!(TASKS.with(|tasks|tasks.borrow().is_empty()),"fixture leaked an active task/fence");
        CLOCK.with(|clock|clock.set(0));GENERATION.with(|g|g.set(Some(7)));GUEST.with(|g|g.set(false));
        TRANSPORT.with(|t|*t.borrow_mut()=Transport{steps:steps.into_iter().collect(),..Default::default()});
        CABI_NET_FETCH_BYTES_RESULTS.lock().clear();CABI_JSON_POST_CANCELLATIONS.lock().clear();
        SPAWNED.with(|s|s.borrow_mut().clear());VM_CALLS.with(|s|s.borrow_mut().clear());
    }
    fn poll_tasks() {
        let tasks=TASKS.with(|t|core::mem::take(&mut *t.borrow_mut()));
        let mut cx=Context::from_waker(core::task::Waker::noop());
        for mut task in tasks {if task.as_mut().poll(&mut cx).is_pending() {TASKS.with(|t|t.borrow_mut().push(task));}}
    }
    fn settle() {
        for _ in 0..100 {
            poll_tasks();if TASKS.with(|t|t.borrow().is_empty()) {return;}
            CLOCK.with(|clock|clock.set(clock.get()+1));
        }
        panic!("bounded task failed to retire under finite deadline");
    }
    fn start(timeout:u32,max:u64,guest:bool)->u32 {
        GUEST.with(|g|g.set(guest));
        unsafe {trueos_cabi_net_fetch_bytes_start_with_options_v1(URL.as_ptr(),URL.len(),timeout,max)}
    }
    #[test]fn options_validate_wide_values_before_integer_narrowing() {
        setup([]);
        assert_eq!(bounded_fetch_options(90_000,128*1024*1024),Some((90_000,128*1024*1024)));
        for (timeout,max) in [(0,1),(90_001,1),(u64::MAX,1),(1,0),(1,128*1024*1024+1),(1,u64::MAX)] {
            assert_eq!(bounded_fetch_options(timeout,max),None);
        }
        for bytes in [&b""[..],&b"\xff"[..],&b"https://x/\r\nInjected:x"[..],&b"https://x/\0"[..],&b"file:///a"[..]] {
            assert!(bounded_fetch_request(bytes,1,1).is_none());
        }
        let huge=vec![b'a';PAYLOAD_CAP+1];
        assert!(bounded_fetch_request(&huge,1,1).is_none());
        unsafe {
            assert_eq!(trueos_cabi_net_fetch_bytes_start_with_options_v1(core::ptr::null(),1,1,1),0);
            // Oversized length is rejected before even borrowing this deliberately invalid pointer.
            assert_eq!(trueos_cabi_net_fetch_bytes_start_with_options_v1(core::ptr::dangling(),PAYLOAD_CAP+1,1,1),0);
        }
        assert!(CABI_NET_FETCH_BYTES_RESULTS.lock().is_empty());
    }
    #[test]fn host_and_actual_vm_start_preserve_budgets_url_and_owner() {
        for guest in [false,true] {
            setup([reply(200,None,0)]);
            let id=start(90_000,128*1024*1024,guest);assert_ne!(id,0);
            assert_eq!(CABI_NET_FETCH_BYTES_RESULTS.lock()[&id].owner,guest.then_some((1,7)));
            settle();assert_eq!(cabi_net_fetch_bytes_result_len_host(id),3);
            assert!(CABI_JSON_POST_CANCELLATIONS.lock().is_empty());
            TRANSPORT.with(|t|{let t=t.borrow();assert_eq!(t.requests.len(),1);let r=&t.requests[0];
                assert_eq!(r.timeout,90_000);assert_eq!(r.max_bytes,128*1024*1024);assert_eq!(r.url,"https://catalog.test:443/p/actual.bp?signature=a%2Fb");});
            cabi_net_fetch_bytes_discard_host(id);
        }
    }
    #[test]fn legacy_defaults_remain_45seconds_and_eight_mib() {
        setup([]);
        unsafe {assert_ne!(trueos_cabi_net_fetch_bytes_start_with_referrer(URL.as_ptr(),URL.len(),URL.as_ptr(),URL.len()),0);}
        SPAWNED.with(|s|{let s=s.borrow();assert_eq!(s.len(),1);assert_eq!(s[0].timeout,45_000);assert_eq!(s[0].max_bytes,8*1024*1024);});
    }
    #[test]fn actual_vm_arm_rejects_wide_budgets_malformed_url_and_missing_generation() {
        setup([]);
        VM_HOST.with(|page|unsafe{(&mut (*page.get()).payload)[..URL.len()].copy_from_slice(URL.as_bytes());});
        for (timeout,max,len) in [(u64::MAX,1,URL.len()as u32),(1,u64::MAX,URL.len()as u32),(1,1,0),(1,1,PAYLOAD_CAP as u32+1)] {
            dispatch_bounded_start(1,timeout,max,len);assert_eq!(VM_RESPONSE.with(|r|r.get()).0,STATUS_BAD_ARG);
        }
        GENERATION.with(|g|g.set(None));dispatch_bounded_start(1,1,1,URL.len()as u32);
        assert_eq!(VM_RESPONSE.with(|r|r.get()).0,STATUS_BAD_ARG);
        assert!(CABI_NET_FETCH_BYTES_RESULTS.lock().is_empty());assert!(TASKS.with(|t|t.borrow().is_empty()));
    }
    #[test]fn redirects_share_absolute_deadline_including_final_ready_body() {
        setup([reply(302,Some("/next"),4),reply(200,None,7)]);
        let id=start(10,1024,false);settle();assert_eq!(cabi_net_fetch_bytes_result_len_host(id),NET_ERR_TIMEOUT as isize);
        TRANSPORT.with(|t|{let t=t.borrow();assert_eq!(t.requests.iter().map(|r|r.timeout).collect::<Vec<_>>(),vec![10,6]);});
        assert!(CABI_JSON_POST_CANCELLATIONS.lock().is_empty());
    }
    #[test]fn deadline_cancels_request_and_waits_until_tls_owner_fence_retires() {
        setup([Step::WaitForCancellation]);
        let id=start(4,1024,true);poll_tasks();assert_eq!(cabi_net_fetch_bytes_result_len_host(id),-8);
        CLOCK.with(|c|c.set(4));poll_tasks();assert_eq!(cabi_net_fetch_bytes_result_len_host(id),-8);
        settle();assert_eq!(cabi_net_fetch_bytes_result_len_host(id),NET_ERR_TIMEOUT as isize);
        TRANSPORT.with(|t|{let t=t.borrow();assert!(t.fence_retired);assert!(!t.dropped_before_fence);});
        assert!(CABI_JSON_POST_CANCELLATIONS.lock().is_empty());
    }
    #[test]fn discard_pending_and_vm_teardown_never_resurrect_after_late_completion() {
        for teardown in [false,true] {
            setup([Step::WaitForCancellation]);let id=start(90,1024,true);poll_tasks();
            let token=CABI_JSON_POST_CANCELLATIONS.lock()[&id].clone();
            if teardown {assert_eq!(discard_bounded_fetches_for_vm(1,7),1);} else {assert_eq!(cabi_net_fetch_bytes_discard_host(id),0);}
            assert!(token.is_cancelled());assert_eq!(cabi_net_fetch_bytes_result_len_host(id),-8);
            GENERATION.with(|g|g.set(Some(8)));settle();
            assert!(!CABI_NET_FETCH_BYTES_RESULTS.lock().contains_key(&id));
            TRANSPORT.with(|t|{let t=t.borrow();assert!(t.fence_retired);assert!(!t.dropped_before_fence);});
        }
    }
    #[test]fn vm_teardown_reclaims_ready_body_preserving_other_runs_and_legacy_ops() {
        setup([reply(200,None,0)]);let id=start(90,1024,true);settle();
        assert!(bounded_fetch_accessible_to_vm(id,1));assert!(!bounded_fetch_accessible_to_vm(id,2));
        GENERATION.with(|g|g.set(Some(8)));assert!(!bounded_fetch_accessible_to_vm(id,1));
        CABI_NET_FETCH_BYTES_RESULTS.lock().insert(999,CabiNetFetchBytesResult{owner:Some((1,8)),body:vec![4;10],rc:Some(0),..Default::default()});
        CABI_NET_FETCH_BYTES_RESULTS.lock().insert(1000,CabiNetFetchBytesResult{body:vec![5;10],rc:Some(0),..Default::default()});
        assert_eq!(discard_bounded_fetches_for_vm(1,7),1);assert_eq!(cabi_net_fetch_bytes_result_len_host(id),-8);
        assert_eq!(cabi_net_fetch_bytes_result_len_host(999),10);assert_eq!(cabi_net_fetch_bytes_result_len_host(1000),10);
        assert!(bounded_fetch_accessible_to_vm(1000,2));
    }
    #[test]fn stale_run_completion_is_dropped_without_teardown_race() {
        setup([reply(200,None,0)]);let id=start(90,1024,true);
        GENERATION.with(|g|g.set(Some(8)));settle();
        assert_eq!(cabi_net_fetch_bytes_result_len_host(id),-8);assert!(CABI_JSON_POST_CANCELLATIONS.lock().is_empty());
    }
    #[test]fn body_caps_allocation_and_framing_have_distinct_error_codes() {
        setup([]);
        for (message,expected) in [("timeout during readiness",NET_ERR_TIMEOUT),("too large chunked body",FS_ERR_TOO_LARGE),
            ("too large response framing received=9",NET_ERR_RESPONSE_FRAMING),("response allocation failed",-5),("http status 404",NET_ERR_HTTP)] {
            assert_eq!(bounded_fetch_error_code(message),expected);
        }
    }
    #[test]fn realistic_chunk_framing_fits_separate_budget_without_raising_decoded_cap() {
        // A near-cap >8MiB body in 16KiB chunks previously exceeded max+4096.
        let cap=9*1024*1024;let mut encoded=Vec::new();let mut left=cap;
        while left>0 {let size=left.min(16*1024);encoded.extend_from_slice(format!("{size:x}\r\n").as_bytes());encoded.resize(encoded.len()+size,b'x');encoded.extend_from_slice(b"\r\n");left-=size;}
        encoded.extend_from_slice(b"0\r\n\r\n");
        assert!(encoded.len()>cap+4096);assert!(encoded.len()+64<=response_transport_limit(cap));
        assert_eq!(decode_chunked(&encoded,cap).unwrap().unwrap().len(),cap);
        assert!(decode_chunked(&encoded,cap-1).unwrap_err().starts_with("too large"));
        assert_eq!(decode_chunked(&encoded[..encoded.len()-3],cap).unwrap(),None);
        assert_eq!(response_transport_limit(128*1024*1024),128*1024*1024+1024*1024+4096);
        // Very fragmented legal bodies retain a finite, separately stated transport limit.
        assert!(2*128*1024*1024>response_transport_limit(128*1024*1024));
    }
}
'''


def main():
    with tempfile.TemporaryDirectory(prefix="bounded-fetch-") as temporary:
        source = Path(temporary) / "bounded.rs"
        source.write_text(stubs() + production() + TESTS)
        binary = Path(temporary) / "bounded"
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)], cwd=ROOT, check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
