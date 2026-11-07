#!/usr/bin/env python3
"""Check both SDKs' reset boundary, commit, cancellation, and kernel shape admission."""
from pathlib import Path
import re
import subprocess
import tempfile
from test_sdk_metadata_v2 import SOURCES, item

ROOT = Path(__file__).resolve().parents[2]
HARNESS = r'''
#![allow(dead_code)]
use core::future::{Future, poll_fn};
use core::task::{Context, Poll, Waker};
use std::cell::RefCell;
use std::collections::VecDeque;
struct State { start:i32, commit:i32, statuses:VecDeque<i32>, events:Vec<&'static str> }
thread_local! { static STATE:RefCell<State> = const { RefCell::new(State{start:17,commit:0,statuses:VecDeque::new(),events:Vec::new()}) }; }
fn state<T>(f:impl FnOnce(&mut State)->T)->T{STATE.with(|s|f(&mut s.borrow_mut()))}
fn reset(){state(|s|{s.start=17;s.commit=0;s.statuses.clear();s.events.clear();});}
const ERR_BAD_PARAM:i32=-4;
#[derive(Clone,Copy,PartialEq)] struct ContentTypeId(u32);
impl ContentTypeId { const BLOB:Self=Self(1);fn raw(self)->u32{self.0} }
mod vcabi {
    pub unsafe fn trueos_cabi_async_fs_checked_write_begin_v1(path:*const u8,len:usize,total:usize,kind:u32,flags:u32,hash:*const u8)->i32{
        assert_eq!(unsafe{core::slice::from_raw_parts(path,len)},"trueosfs:disc7/Größe.txt".as_bytes());
        assert_eq!((total,kind,flags),(0,super::ContentTypeId::BLOB.raw(),10));
        assert_eq!(unsafe{core::slice::from_raw_parts(hash,32)},&[7;32]);
        super::state(|s|{s.events.push("start");s.start})
    }
    pub unsafe fn trueos_cabi_async_fs_write_chunk(_:u32,_:usize,_:*const u8,_:usize)->i32{panic!("empty reset cannot send chunks")}
    pub unsafe fn trueos_cabi_async_fs_write_commit(id:u32)->i32{assert_eq!(id,17);super::state(|s|{s.events.push("commit");s.commit})}
    pub @SAFETY@fn trueos_cabi_async_fs_status(id:u32)->i32{assert_eq!(id,17);super::state(|s|{s.events.push("status");s.statuses.pop_front().unwrap_or(1)})}
    pub @SAFETY@fn trueos_cabi_async_fs_discard(id:u32)->i32{assert_eq!(id,17);super::state(|s|s.events.push("discard"));0}
}
// @PRODUCTION@
fn complete<F:Future>(f:F)->F::Output{let mut f=std::pin::pin!(f);for _ in 0..8 {if let Poll::Ready(v)=f.as_mut().poll(&mut Context::from_waker(Waker::noop())){return v}}panic!("pending")}
const PATH:&[u8]="trueosfs:disc7/Größe.txt".as_bytes();
#[test]fn empty_reset_waits_for_commit_then_discards_exactly_once(){
    reset();state(|s|s.statuses.extend([0,1]));assert_eq!(complete(reset_blob_checked(PATH,[7;32])),Ok(()));
    state(|s|assert_eq!(s.events,["start","commit","status","status","discard"]));
}
#[test]fn failures_and_cancellation_preserve_ownership(){
    for code in [-4,-17] {reset();state(|s|s.start=code);assert_eq!(complete(reset_blob_checked(PATH,[7;32])),Err(code));state(|s|assert_eq!(s.events,["start"]));}
    reset();state(|s|s.commit=-2);assert_eq!(complete(reset_blob_checked(PATH,[7;32])),Err(-2));state(|s|assert_eq!(s.events,["start","commit","discard"]));
    reset();state(|s|s.statuses.push_back(-17));assert_eq!(complete(reset_blob_checked(PATH,[7;32])),Err(-17));state(|s|assert_eq!(s.events,["start","commit","status","discard"]));
    reset();drop(reset_blob_checked(PATH,[7;32]));state(|s|assert!(s.events.is_empty()));
    state(|s|s.statuses.push_back(0));let mut f=Box::pin(reset_blob_checked(PATH,[7;32]));assert!(f.as_mut().poll(&mut Context::from_waker(Waker::noop())).is_pending());drop(f);
    state(|s|assert_eq!(s.events,["start","commit","status","discard"]));
}
'''
ADMISSION = r'''
#![allow(dead_code)]
use std::string::String;
const FS_ERR_BAD_PARAM:i32=-4; const FS_ERR_TYPE_REQUIRED:i32=-15;
#[derive(Clone,Copy,PartialEq)]pub struct ContentTypeId(u32);
impl ContentTypeId{const NONE:Self=Self(0);const BLOB:Self=Self(1);fn is_registered(self)->bool{(1..=2).contains(&self.0)}}
mod r{pub mod fs{pub mod trueosfs{pub use crate::ContentTypeId;}}}
// @PRODUCTION@
fn start_write_inner(_:u32,_:String,total:usize,kind:ContentTypeId,legacy:bool,options:Option<CheckedWriteOptions>)->i32{
    assert!(!legacy);let o=options.unwrap();assert!(o.reset_blob);assert_eq!(total,0);assert!(kind==ContentTypeId::BLOB);17
}
#[test]fn reset_requires_empty_blob_digest_and_exclusive_mode(){
    for create in [false,true]{for infer in [false,true]{for digest in [None,Some([7;32])]{for len in [0,1]{for kind in [0,1,2,99]{
        let options=CheckedWriteOptions{create_new:create,infer_blob:infer,reset_blob:true,expected_sha256:digest};
        let rc=start_write_checked(7,"a.txt".into(),len,ContentTypeId(kind),options);
        assert_eq!(rc,if kind==0{-15}else if !create&&!infer&&digest.is_some()&&len==0&&kind==1{17}else{-4});
    }}}}}
    assert_eq!(start_write_checked(7,"vFile:startup".into(),0,ContentTypeId::BLOB,CheckedWriteOptions{create_new:false,infer_blob:false,reset_blob:true,expected_sha256:Some([7;32])}),-4);
}
'''


def run(source, toolchain):
    with tempfile.TemporaryDirectory(prefix='trueos-sdk-blob-reset-') as temporary:
        path = Path(temporary)
        (path / 'test.rs').write_text(source)
        subprocess.run(['rustc', '+' + toolchain, '--edition=2024', '--test', '-Dwarnings',
                        str(path / 'test.rs'), '-o', str(path / 'test')], check=True, timeout=60)
        subprocess.run([str(path / 'test')], check=True, timeout=60)


def main():
    toolchain = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / 'rust-toolchain.toml').read_text())[1]
    for path in SOURCES:
        source = path.read_text()
        production = '\n'.join(item(source, signature) for signature in [
            'struct Operation', 'pub struct TypedWrite', 'impl Operation',
            'impl Drop for Operation', 'impl TypedWrite', 'pub async fn reset_blob_checked('])
        print('SDK:', path, flush=True)
        run(HARNESS.replace('// @PRODUCTION@', production).replace('@SAFETY@', 'unsafe ' if path == SOURCES[1] else ''), toolchain)
    source = (ROOT / 'src/r/io/async_fs_cabi.rs').read_text()
    production = item(source, 'pub(crate) struct CheckedWriteOptions') + item(source, 'pub(crate) fn start_write_checked(')
    run(ADMISSION.replace('// @PRODUCTION@', production), toolchain)


if __name__ == '__main__':
    main()
