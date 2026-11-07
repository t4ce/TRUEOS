#!/usr/bin/env python3
"""Exercise both SDK single-directory helpers with native operation ownership."""
from pathlib import Path
import re
import subprocess
import tempfile
from test_sdk_metadata_v2 import HARNESS, SOURCES, item
ROOT=Path(__file__).resolve().parents[2]
TESTS=r'''
fn complete<F:Future>(f:F)->F::Output {
    let mut f=core::pin::pin!(f);
    for _ in 0..8 { if let Poll::Ready(v)=f.as_mut().poll(&mut Context::from_waker(Waker::noop())){return v;} }
    panic!("directory operation stayed pending");
}
#[test]fn native_create_preserves_selector_and_utf8_and_discards_after_commit(){
    reset();let path="trueosfs:disc7/Größe/new".as_bytes();assert_eq!(complete(create_dir(path)),Ok(()));
    boundary(|s|{assert_eq!(s.paths,[path.to_vec()]);assert_eq!(s.events,["create","status","discard"]);assert_eq!(s.discards,[17]);});
}
#[test]fn native_create_rejected_start_does_not_discard_an_unowned_handle(){
    for start in [0,-9,-8,-6,-4]{reset();boundary(|s|s.start=start);assert_eq!(complete(create_dir(b"p/new")),Err(if start==0{-4}else{start}));boundary(|s|{assert_eq!(s.events,["create"]);assert!(s.discards.is_empty());});}
}
#[test]fn native_create_failure_codes_are_not_rewritten_and_release_once(){
    for status in [-2,-3,-6,-8,-9]{reset();boundary(|s|s.statuses.push_back(status));assert_eq!(complete(create_dir(b"p/new")),Err(status));boundary(|s|assert_eq!(s.discards,[17]));}
}
#[test]fn native_create_pending_cannot_report_success_before_commit(){
    reset();boundary(|s|s.statuses.extend([0,0,1]));assert_eq!(complete(create_dir(b"p/new")),Ok(()));boundary(|s|{assert_eq!(s.events,["create","status","status","status","discard"]);assert_eq!(s.discards,[17]);});
}
#[test]fn native_create_cancellation_and_unpolled_future_own_only_started_operation(){
    reset();drop(create_dir(b"p/new"));boundary(|s|assert!(s.events.is_empty()));boundary(|s|s.statuses.push_back(0));
    let mut f=Box::pin(create_dir(b"p/new"));assert!(f.as_mut().poll(&mut Context::from_waker(Waker::noop())).is_pending());boundary(|s|assert!(s.discards.is_empty()));drop(f);
    boundary(|s|{assert_eq!(s.events,["create","status","discard"]);assert_eq!(s.discards,[17]);});
}
'''
def main():
    toolchain=re.search(r'channel\s*=\s*"([^"]+)"',(ROOT/'rust-toolchain.toml').read_text())[1]
    for path in SOURCES:
        source=path.read_text()
        production='\n'.join(['const ERR_BAD_PARAM:i32=-4;',item(source,'struct Operation'),item(source,'impl Operation'),item(source,'impl Drop for Operation'),item(source,'pub async fn create_dir(')])
        fixture='#![allow(dead_code)]\n'+HARNESS.split('fn complete(',1)[0]
        start='''pub unsafe fn trueos_cabi_async_fs_create_dir_start_v1(path:*const u8,len:usize)->i32 {
            boundary(|s|{s.events.push("create");s.paths.push(unsafe{core::slice::from_raw_parts(path,len)}.to_vec());s.start})
        }
        '''
        fixture=fixture.replace('pub unsafe fn trueos_cabi_async_fs_metadata_start_v2',start+'pub unsafe fn trueos_cabi_async_fs_metadata_start_v2').replace('// @PRODUCTION@',production).replace('@SAFETY@','unsafe ' if path==SOURCES[1] else '')+TESTS
        with tempfile.TemporaryDirectory(prefix='trueos-sdk-create-dir-') as tmp:
            directory=Path(tmp);rust=directory/'tests.rs';binary=directory/'tests';rust.write_text(fixture)
            subprocess.run(['rustc','+'+toolchain,'--edition=2024','--test','-Dwarnings',str(rust),'-o',str(binary)],check=True,timeout=60)
            print('SDK:',path,flush=True);subprocess.run([str(binary)],check=True,timeout=60)
if __name__=='__main__':main()
