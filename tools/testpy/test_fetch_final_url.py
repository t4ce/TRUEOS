#!/usr/bin/env python3
"""Exercise production final-URL metadata, its VM arm, and the vnetfs wrapper.

Compiles the actual Rust items on the host. Only the shared VM page, guest
context, and transport call are mocked; no network, kernel build, or rig reset
is involved. Metadata reads must leave the existing destructive body API intact.
"""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def item(source, signature):
    """Extract one complete production item without maintaining a test copy."""
    first = source.index(signature)
    opening = source.index("{", first)
    depth = 1
    for cursor in range(opening + 1, len(source)):
        depth += (source[cursor] == "{") - (source[cursor] == "}")
        if depth == 0:
            return source[first:cursor + 1]
    raise AssertionError(f"unterminated production item: {signature}")


def vm_arm(source, name):
    first = source.index(f"        {name} => {{")
    following = re.search(r"\n        OP_[A-Z0-9_]+(?:\s|\|)", source[first + 1:])
    if following is None:
        raise AssertionError(f"missing following VM arm: {name}")
    return source[first:first + 1 + following.start()]


def production():
    https = (ROOT / "src/r/net/https.rs").read_text()
    vmcall = (ROOT / "src/hv/vmcall.rs").read_text()
    wrapper = (ROOT / "crates/trueos-v/src/vnetfs.rs").read_text()
    codes = (ROOT / "src/r/cabi_codes.rs").read_text()
    constants = [
        "COMM_PAGE_BYTES", "PAYLOAD_CAP", "STATUS_OK", "STATUS_BAD_ARG",
        "OP_BP_FETCH_BYTES_RESULT_LEN", "OP_BP_FETCH_BYTES_READ",
        "OP_BP_FETCH_BYTES_FINAL_URL_V1",
    ]
    selected = [line for line in vmcall.splitlines()
                if any(line.startswith(f"pub const {name}:") for name in constants)]
    selected += [line for line in codes.splitlines()
                 if any(line.startswith(f"pub const {name}:") for name in
                        ["FS_ERR_BAD_PARAM", "FS_ERR_TOO_LARGE", "FS_ERR_NOT_FOUND", "FS_ERR_IO"])]
    selected.append("#[derive(Default)]\n" + item(https, "struct CabiNetFetchBytesResult {"))
    for signature in [
        "pub(crate) fn bounded_fetch_accessible_to_vm(",
        "fn pack_fetch_bytes_vm_read(",
        "pub(crate) fn decode_fetch_bytes_vm_read(",
        "pub(crate) fn cabi_net_fetch_bytes_result_len_host(",
        "pub(crate) fn cabi_net_fetch_bytes_read_chunk_host(",
        "pub(crate) fn cabi_net_fetch_bytes_final_url_read_chunk_host(",
        "fn cabi_net_fetch_bytes_result_len_guest(",
        "fn cabi_net_fetch_bytes_read_guest(",
        "fn cabi_net_fetch_bytes_final_url_guest(",
        'pub extern "C" fn trueos_cabi_net_fetch_bytes_result_len(',
        'pub extern "C" fn trueos_cabi_net_fetch_bytes_read(',
        'pub extern "C" fn trueos_cabi_net_fetch_bytes_final_url(',
    ]:
        selected.append(item(https, signature))
    selected.append("fn dispatch(op:u32,vm_id:u8,arg0:u64,arg1:u64)->DispatchOutcome {\n"
                    "let seq=7; match op {\n" + "\n".join(vm_arm(vmcall, name) for name in [
                        "OP_BP_FETCH_BYTES_RESULT_LEN", "OP_BP_FETCH_BYTES_READ",
                        "OP_BP_FETCH_BYTES_FINAL_URL_V1",
                    ]) + "\n _ => panic!(\"unexpected VM operation {op}\"),\n}}")
    selected.append("mod vnetfs {use super::vcabi;\n" + "\n".join(item(wrapper, signature) for signature in [
        "pub fn fetch_bytes_result_len(", "pub fn fetch_bytes_read(",
        "pub fn fetch_bytes_final_url(",
    ]) + "\n}")
    return "\n".join(selected)


STUBS = r'''
#![allow(dead_code, unused_unsafe)]
extern crate alloc;
use std::cell::{Cell, RefCell, UnsafeCell};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
static CABI_NET_FETCH_BYTES_RESULTS:Mutex<alloc::collections::BTreeMap<u32,CabiNetFetchBytesResult>>=
    Mutex::new(alloc::collections::BTreeMap::new());
thread_local! {
    static GUEST:Cell<bool>=const {Cell::new(false)};
    static PAGE_PRESENT:Cell<bool>=const {Cell::new(true)};
    static VM_HOST:UnsafeCell<HostPage>=UnsafeCell::new(HostPage{payload:[0;PAYLOAD_CAP]});
    static VM_RESPONSE:Cell<(u32,u64,u32)>=const {Cell::new((0,0,0))};
    static VM_CALLS:RefCell<Vec<(u32,u64,u64,usize)>>=const {RefCell::new(Vec::new())};
    static VM_OVERRIDE:RefCell<Option<(u32,u64)>>=const {RefCell::new(None)};
    static VM_READ_OVERRIDE:RefCell<Option<(u32,u64)>>=const {RefCell::new(None)};
    static WRAPPER_OVERRIDE:RefCell<Option<(isize,Vec<u8>,isize)>>=const {RefCell::new(None)};
    static WRAPPER_READS:Cell<usize>=const {Cell::new(0)};
}
struct HostPage {payload:[u8;PAYLOAD_CAP]}
enum DispatchOutcome {Resume}
fn host_ptr(_:u8)->Option<*mut HostPage> {
    PAGE_PRESENT.with(|present|present.get().then(||VM_HOST.with(|page|page.get())))
}
fn write_response(_:u8,_:u32,status:u32,value:u64,length:u32) {
    VM_RESPONSE.with(|response|response.set((status,value,length)));
}
mod hv {
    pub fn vm_run_generation(_:u8)->Option<u64> {Some(7)}
    pub fn current_hull_guest_context_vm_id()->Option<u8> {
        crate::GUEST.with(|guest|guest.get().then_some(1))
    }
}
mod r {pub mod net {pub mod https {
    pub(crate) use crate::{cabi_net_fetch_bytes_result_len_host,
        cabi_net_fetch_bytes_read_chunk_host,cabi_net_fetch_bytes_final_url_read_chunk_host,
        decode_fetch_bytes_vm_read,bounded_fetch_accessible_to_vm};
}}}
mod trueos_vm {pub mod vmcall {
    pub use crate::{PAYLOAD_CAP,STATUS_OK,OP_BP_FETCH_BYTES_RESULT_LEN,
        OP_BP_FETCH_BYTES_READ,OP_BP_FETCH_BYTES_FINAL_URL_V1};
    pub fn call(op:u32,a:u64,b:u64)->(u32,u64) {
        call_with_payload(op,a,b,&[],&mut[])
    }
    pub fn call_with_payload(op:u32,a:u64,b:u64,payload:&[u8],out:&mut[u8])->(u32,u64) {
        assert!(payload.is_empty());assert!(out.len()<=PAYLOAD_CAP);
        crate::VM_CALLS.with(|calls|calls.borrow_mut().push((op,a,b,out.len())));
        if let Some(response)=crate::VM_OVERRIDE.with(|value|*value.borrow()) {return response;}
        if op==OP_BP_FETCH_BYTES_FINAL_URL_V1&&b!=0 {
            if let Some(response)=crate::VM_READ_OVERRIDE.with(|value|*value.borrow()) {return response;}
        }
        crate::dispatch(op,1,a,b);
        let (status,value,length)=crate::VM_RESPONSE.with(|response|response.get());
        assert!(length as usize<=out.len(),"VM arm published beyond requested capacity");
        crate::VM_HOST.with(|page|unsafe {
            out[..length as usize].copy_from_slice(&(&(*page.get()).payload)[..length as usize]);
        });
        (status,value)
    }
}}
mod vcabi {
    pub use crate::{trueos_cabi_net_fetch_bytes_result_len,trueos_cabi_net_fetch_bytes_read};
    pub unsafe fn trueos_cabi_net_fetch_bytes_final_url(op:u32,out:*mut u8,cap:usize)->isize {
        // Invalid ABI responses can only be injected at the wrapper boundary:
        // production metadata stores String, so malformed UTF-8 cannot originate there.
        if let Some((length,bytes,result))=crate::WRAPPER_OVERRIDE.with(|value|value.borrow().clone()) {
            if cap==0 {return length;}
            crate::WRAPPER_READS.with(|reads|reads.set(reads.get()+1));
            let count=bytes.len().min(cap);
            unsafe {core::ptr::copy_nonoverlapping(bytes.as_ptr(),out,count);}
            return result;
        }
        crate::trueos_cabi_net_fetch_bytes_final_url(op,out,cap)
    }
}
'''


TESTS = r'''
#[cfg(test)]mod tests {
    use super::*;
    const ID:u32=77;
    const URL:&str="https://cdn.test:443/assets/final.png?signature=a%2Fb&v=7";
    const BODY:&[u8]=b"\x89PNG\r\n\x1a\nactual image bytes";
    fn setup(guest:bool,rc:Option<i32>,url:Option<&str>,body:&[u8]) {
        GUEST.with(|value|value.set(guest));PAGE_PRESENT.with(|value|value.set(true));
        VM_CALLS.with(|calls|calls.borrow_mut().clear());
        VM_OVERRIDE.with(|value|*value.borrow_mut()=None);
        VM_READ_OVERRIDE.with(|value|*value.borrow_mut()=None);
        WRAPPER_OVERRIDE.with(|value|*value.borrow_mut()=None);
        WRAPPER_READS.with(|value|value.set(0));
        CABI_NET_FETCH_BYTES_RESULTS.lock().clear();
        CABI_NET_FETCH_BYTES_RESULTS.lock().insert(ID,CabiNetFetchBytesResult {
            rc,body:body.to_vec(),final_url:url.map(String::from),
            ..Default::default()
        });
    }
    fn final_url(op:u32)->Result<Vec<u8>,isize> {
        let length=trueos_cabi_net_fetch_bytes_final_url(op,core::ptr::null_mut(),0);
        if length<0 {return Err(length);}
        let mut bytes=vec![0;length as usize];
        let got=trueos_cabi_net_fetch_bytes_final_url(op,bytes.as_mut_ptr(),bytes.len());
        if got<0 {Err(got)} else {bytes.truncate(got as usize);Ok(bytes)}
    }
    #[test]fn metadata_queries_and_repeated_reads_preserve_body_until_legacy_read() {
        for guest in [false,true] {
            setup(guest,Some(0),Some(URL),BODY);
            for _ in 0..3 {
                assert_eq!(final_url(ID).unwrap(),URL.as_bytes());
                assert_eq!(vnetfs::fetch_bytes_final_url(ID).unwrap(),URL);
                assert_eq!(trueos_cabi_net_fetch_bytes_result_len(ID),BODY.len() as isize);
            }
            assert_eq!(vnetfs::fetch_bytes_read(ID).unwrap(),BODY);
            assert_eq!(final_url(ID),Err(FS_ERR_NOT_FOUND as isize));
            assert_eq!(trueos_cabi_net_fetch_bytes_result_len(ID),FS_ERR_NOT_FOUND as isize);
        }
    }
    #[test]fn pending_failed_unknown_and_post_without_url_keep_their_body_state() {
        for guest in [false,true] {
            for (rc,url,expected) in [(None,Some(URL),-8),(Some(-19),Some(URL),-19),
                                     (Some(0),None,-4)] {
                setup(guest,rc,url,BODY);
                for _ in 0..2 {assert_eq!(final_url(ID),Err(expected));}
                assert!(CABI_NET_FETCH_BYTES_RESULTS.lock().contains_key(&ID));
                assert_eq!(trueos_cabi_net_fetch_bytes_result_len(ID),rc.map_or(-8,|rc|if rc==0 {BODY.len() as isize}else{rc as isize}));
            }
            assert_eq!(final_url(ID+1),Err(-8));
        }
    }
    #[test]fn exported_pointer_queries_and_capacity_errors_do_not_touch_output_or_body() {
        for guest in [false,true] {
            setup(guest,Some(0),Some(URL),BODY);
            let mut sentinel=[0xa5;8];
            assert_eq!(trueos_cabi_net_fetch_bytes_final_url(ID,core::ptr::null_mut(),0),URL.len() as isize);
            assert_eq!(trueos_cabi_net_fetch_bytes_final_url(ID,sentinel.as_mut_ptr(),0),URL.len() as isize);
            assert_eq!(trueos_cabi_net_fetch_bytes_final_url(ID,core::ptr::null_mut(),1),-4);
            assert_eq!(trueos_cabi_net_fetch_bytes_final_url(ID,sentinel.as_mut_ptr(),sentinel.len()),-7);
            assert_eq!(sentinel,[0xa5;8]);
            assert_eq!(vnetfs::fetch_bytes_read(ID).unwrap(),BODY);
        }
    }
    #[test]fn exported_large_capacity_writes_only_the_url_prefix() {
        for guest in [false,true] {
            setup(guest,Some(0),Some(URL),BODY);
            let mut output=vec![0xa5;URL.len()+19];
            assert_eq!(trueos_cabi_net_fetch_bytes_final_url(ID,output.as_mut_ptr(),output.len()),URL.len() as isize);
            assert_eq!(&output[..URL.len()],URL.as_bytes());
            assert_eq!(&output[URL.len()..],&[0xa5;19]);
            assert_eq!(vnetfs::fetch_bytes_read(ID).unwrap(),BODY);
        }
    }
    #[test]fn host_chunk_offsets_caps_and_end_reads_are_bounded_and_nonconsuming() {
        setup(false,Some(0),Some(URL),BODY);
        for offset in 0..=URL.len() {
            for cap in [0,1,7,URL.len()+9] {
                let mut output=vec![0xa5;cap+3];
                let expected=if cap==0 {if offset==0 {URL.len() as isize}else{-4}}
                    else {cap.min(URL.len()-offset) as isize};
                assert_eq!(cabi_net_fetch_bytes_final_url_read_chunk_host(ID,offset,&mut output[..cap]),expected);
                if cap!=0 {assert_eq!(&output[..expected as usize],&URL.as_bytes()[offset..offset+expected as usize]);}
                assert!(output[if cap==0 {0}else{expected as usize}..].iter().all(|byte|*byte==0xa5));
            }
        }
        for offset in [URL.len()+1,usize::MAX] {
            assert_eq!(cabi_net_fetch_bytes_final_url_read_chunk_host(ID,offset,&mut[0;4]),-4);
        }
        assert_eq!(vnetfs::fetch_bytes_read(ID).unwrap(),BODY);
    }
    #[test]fn guest_metadata_reads_cross_actual_payload_boundary_without_consuming_body() {
        let prefix="https://cdn.test/";
        let url=format!("{prefix}{}€{}?signature=keep", "x".repeat(PAYLOAD_CAP-prefix.len()-1),"y".repeat(PAYLOAD_CAP+37));
        setup(true,Some(0),Some(&url),BODY);
        let received=final_url(ID).unwrap();
        assert_eq!(received,url.as_bytes());
        assert_eq!(String::from_utf8(received).unwrap(),url);
        assert!(!url.is_char_boundary(PAYLOAD_CAP));
        let reads=VM_CALLS.with(|calls|calls.borrow().iter().copied().filter(|(op,_,arg,_)|
            *op==OP_BP_FETCH_BYTES_FINAL_URL_V1&&*arg!=0).collect::<Vec<_>>());
        assert_eq!(reads.len(),3);
        let mut copied=0;
        for (_,op_id,packed,capacity) in reads {
            assert_eq!(op_id,u64::from(ID));
            let (offset,want)=decode_fetch_bytes_vm_read(packed).unwrap();
            assert_eq!(offset,copied);assert_eq!(want,capacity);assert!(want<=PAYLOAD_CAP);
            copied+=want;
        }
        assert_eq!(copied,url.len());assert_eq!(vnetfs::fetch_bytes_read(ID).unwrap(),BODY);
    }
    #[test]fn package_sized_guest_body_crosses_old_eight_mib_limit_and_consumes_result() {
        // Current Codex archive size, rather than a tiny artificial payload.
        let mut body=vec![0x5a;80_561_558];
        for offset in (0..body.len()).step_by(PAYLOAD_CAP) {body[offset]=(offset/PAYLOAD_CAP) as u8;}
        setup(true,Some(0),Some(URL),&body);
        assert_eq!(vnetfs::fetch_bytes_read(ID).unwrap(),body);
        let reads=VM_CALLS.with(|calls|calls.borrow().iter().filter(|(op,_,_,_)|*op==OP_BP_FETCH_BYTES_READ).count());
        assert_eq!(reads,body.len().div_ceil(PAYLOAD_CAP));
        assert_eq!(reads,492);
        assert_eq!(trueos_cabi_net_fetch_bytes_result_len(ID),-8);
        // The bounded API's maximum also fits the unchanged u32 offset field.
        let packed=pack_fetch_bytes_vm_read(128*1024*1024-1,1).unwrap();
        assert_eq!(decode_fetch_bytes_vm_read(packed),Some((128*1024*1024-1,1)));
    }
    #[test]fn body_guest_reads_still_chunk_and_consume_on_the_final_chunk() {
        let body=(0..PAYLOAD_CAP+73).map(|index|(index%251) as u8).collect::<Vec<_>>();
        setup(true,Some(0),Some(URL),&body);
        assert_eq!(final_url(ID).unwrap(),URL.as_bytes());
        assert_eq!(vnetfs::fetch_bytes_read(ID).unwrap(),body);
        assert_eq!(final_url(ID),Err(-8));
        let reads=VM_CALLS.with(|calls|calls.borrow().iter().filter(|(op,_,_,_)|*op==OP_BP_FETCH_BYTES_READ).count());
        assert_eq!(reads,2);
    }
    #[test]fn vm_arm_rejects_untrusted_operation_ids_and_packed_capacities() {
        setup(true,Some(0),Some(URL),BODY);
        for (op_id,packed) in [(u64::from(u32::MAX)+1,0),(u64::from(ID),PAYLOAD_CAP as u64+1)] {
            dispatch(OP_BP_FETCH_BYTES_FINAL_URL_V1,1,op_id,packed);
            assert_eq!(VM_RESPONSE.with(|response|response.get()),(STATUS_BAD_ARG,0,0));
        }
        PAGE_PRESENT.with(|value|value.set(false));
        dispatch(OP_BP_FETCH_BYTES_FINAL_URL_V1,1,u64::from(ID),pack_fetch_bytes_vm_read(0,1).unwrap());
        assert_eq!(VM_RESPONSE.with(|response|response.get()),(STATUS_BAD_ARG,0,0));
        assert!(CABI_NET_FETCH_BYTES_RESULTS.lock().contains_key(&ID));
    }
    #[test]fn vm_chunk_response_lengths_and_error_payloads_match_actual_data() {
        setup(true,Some(0),Some(URL),BODY);
        let offset=URL.len()-3;
        dispatch(OP_BP_FETCH_BYTES_FINAL_URL_V1,1,u64::from(ID),pack_fetch_bytes_vm_read(offset,9).unwrap());
        assert_eq!(VM_RESPONSE.with(|response|response.get()),(STATUS_OK,3,3));
        VM_HOST.with(|page|unsafe{assert_eq!(&(&(*page.get()).payload)[..3],&URL.as_bytes()[offset..]);});
        dispatch(OP_BP_FETCH_BYTES_FINAL_URL_V1,1,u64::from(ID),pack_fetch_bytes_vm_read(URL.len()+1,9).unwrap());
        assert_eq!(VM_RESPONSE.with(|response|response.get()),(STATUS_OK,(-4i64) as u64,0));
    }
    #[test]fn guest_transport_status_failures_propagate_without_consuming_result() {
        setup(true,Some(0),Some(URL),BODY);
        VM_OVERRIDE.with(|value|*value.borrow_mut()=Some((STATUS_BAD_ARG,0)));
        assert_eq!(trueos_cabi_net_fetch_bytes_final_url(ID,core::ptr::null_mut(),0),-4);
        assert!(CABI_NET_FETCH_BYTES_RESULTS.lock().contains_key(&ID));
    }
    #[test]fn guest_rejects_short_oversized_or_failed_vm_chunk_responses() {
        for (status,count,expected) in [(STATUS_OK,0,FS_ERR_IO as isize),
            (STATUS_OK,(URL.len()-1) as u64,FS_ERR_IO as isize),
            (STATUS_OK,(URL.len()+1) as u64,FS_ERR_IO as isize),
            (STATUS_OK,(-19i64) as u64,-19),(STATUS_BAD_ARG,0,-4)] {
            setup(true,Some(0),Some(URL),BODY);
            VM_READ_OVERRIDE.with(|value|*value.borrow_mut()=Some((status,count)));
            let mut output=vec![0xa5;URL.len()+3];
            assert_eq!(trueos_cabi_net_fetch_bytes_final_url(ID,output.as_mut_ptr(),URL.len()),expected);
            assert_eq!(&output[URL.len()..],&[0xa5;3]);
            assert!(CABI_NET_FETCH_BYTES_RESULTS.lock().contains_key(&ID));
        }
    }
    #[test]fn wrapper_rejects_oversized_metadata_before_allocating_or_reading() {
        setup(false,Some(0),Some(URL),BODY);
        WRAPPER_OVERRIDE.with(|value|*value.borrow_mut()=Some((64*1024+1,Vec::new(),0)));
        assert_eq!(vnetfs::fetch_bytes_final_url(ID),Err(-7));
        assert_eq!(WRAPPER_READS.with(Cell::get),0);
        assert!(CABI_NET_FETCH_BYTES_RESULTS.lock().contains_key(&ID));
    }
    #[test]fn wrapper_rejects_invalid_utf8_and_short_metadata_reads() {
        setup(false,Some(0),Some(URL),BODY);
        WRAPPER_OVERRIDE.with(|value|*value.borrow_mut()=Some((2,vec![0xff,0xfe],2)));
        assert_eq!(vnetfs::fetch_bytes_final_url(ID),Err(-1));
        WRAPPER_OVERRIDE.with(|value|*value.borrow_mut()=Some((3,b"abc".to_vec(),2)));
        assert_eq!(vnetfs::fetch_bytes_final_url(ID),Err(-2));
        assert!(CABI_NET_FETCH_BYTES_RESULTS.lock().contains_key(&ID));
    }
    #[test]fn wrapper_rejects_empty_metadata_without_requesting_a_body_read() {
        setup(false,Some(0),Some(URL),BODY);
        WRAPPER_OVERRIDE.with(|value|*value.borrow_mut()=Some((0,Vec::new(),0)));
        assert_eq!(vnetfs::fetch_bytes_final_url(ID),Err(-4));
        assert_eq!(WRAPPER_READS.with(Cell::get),0);
        assert_eq!(vnetfs::fetch_bytes_read(ID).unwrap(),BODY);
    }
    #[test]fn wrapper_forwards_negative_query_and_read_errors() {
        setup(false,Some(0),Some(URL),BODY);
        for rc in [-8,-4,-7,-19] {
            WRAPPER_OVERRIDE.with(|value|*value.borrow_mut()=Some((rc,Vec::new(),0)));
            assert_eq!(vnetfs::fetch_bytes_final_url(ID),Err(rc as i32));
            WRAPPER_OVERRIDE.with(|value|*value.borrow_mut()=Some((3,Vec::new(),rc)));
            assert_eq!(vnetfs::fetch_bytes_final_url(ID),Err(rc as i32));
        }
    }
}
'''


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="fetch-final-url-") as directory:
        path = Path(directory)
        (path / "test.rs").write_text(STUBS + production() + TESTS)
        subprocess.run([
            "rustc", "--edition=2024", "--test", str(path / "test.rs"), "-o", str(path / "test"),
        ], check=True)
        subprocess.run([str(path / "test"), "--test-threads=1"], check=True)
