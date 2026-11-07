#!/usr/bin/env python3
"""Exercise native kind-checked leaf removal and operation lifetime in both SDKs."""
from pathlib import Path
import re
import subprocess
import tempfile
from test_sdk_metadata_v2 import HARNESS, SOURCES, item
ROOT=Path(__file__).resolve().parents[2]
TESTS=r'''
fn complete<F:Future>(f:F)->F::Output {
    let mut f=core::pin::pin!(f);
    for _ in 0..8 {if let Poll::Ready(v)=f.as_mut().poll(&mut Context::from_waker(Waker::noop())){return v;}}
    panic!("leaf removal stayed pending");
}
#[test]fn distinct_kind_and_utf8_path_survive_to_boundary_then_discard_once(){
    for directory in [false,true]{reset();let path="trueosfs:disc7/Größe/leaf".as_bytes();
        let result=if directory{complete(remove_empty_dir(path))}else{complete(remove_file(path))};assert_eq!(result,Ok(()));
        boundary(|s|{assert_eq!(s.paths,[path.to_vec()]);assert_eq!(s.events,[if directory{"empty-dir"}else{"file"},"status","discard"]);assert_eq!(s.discards,[17]);});}
}
#[test]fn start_rejection_never_discards_unowned_handles(){
    for start in [0,-6,-8,-9]{reset();boundary(|s|s.start=start);assert_eq!(complete(remove_file(b"leaf")),Err(if start==0{-4}else{start}));boundary(|s|{assert_eq!(s.events,["file"]);assert!(s.discards.is_empty());});}
}
#[test]fn kind_nonempty_storage_and_missing_failures_are_preserved(){
    for status in [-2,-3,-6,-8,-9]{reset();boundary(|s|s.statuses.push_back(status));assert_eq!(complete(remove_empty_dir(b"leaf")),Err(status));boundary(|s|assert_eq!(s.discards,[17]));}
}
#[test]fn pending_wait_and_cancellation_respect_owned_operation(){
    reset();boundary(|s|s.statuses.extend([0,0,1]));assert_eq!(complete(remove_file(b"leaf")),Ok(()));boundary(|s|{assert_eq!(s.events,["file","status","status","status","discard"]);assert_eq!(s.discards,[17]);});
    reset();drop(remove_empty_dir(b"leaf"));boundary(|s|assert!(s.events.is_empty()));boundary(|s|s.statuses.push_back(0));let mut f=Box::pin(remove_empty_dir(b"leaf"));assert!(f.as_mut().poll(&mut Context::from_waker(Waker::noop())).is_pending());drop(f);boundary(|s|{assert_eq!(s.events,["empty-dir","status","discard"]);assert_eq!(s.discards,[17]);});
}
'''
def main():
    toolchain=re.search(r'channel\s*=\s*"([^"]+)"',(ROOT/'rust-toolchain.toml').read_text())[1]
    for path in SOURCES:
        source=path.read_text()
        signatures=['struct Operation','impl Operation','impl Drop for Operation','pub async fn remove_file(','pub async fn remove_empty_dir(','async fn remove_leaf(']
        production='const ERR_BAD_PARAM:i32=-4;\n'+'\n'.join(item(source,sig) for sig in signatures)
        fixture='#![allow(dead_code)]\n'+HARNESS.split('fn complete(',1)[0]
        start='''pub unsafe fn trueos_cabi_async_fs_remove_leaf_start_v1(path:*const u8,len:usize,kind:u32)->i32 {
            boundary(|s|{s.events.push(match kind{1=>"file",2=>"empty-dir",_=>panic!("invalid leaf kind")});s.paths.push(unsafe{core::slice::from_raw_parts(path,len)}.to_vec());s.start})
        }
        '''
        fixture=fixture.replace('pub unsafe fn trueos_cabi_async_fs_metadata_start_v2',start+'pub unsafe fn trueos_cabi_async_fs_metadata_start_v2').replace('// @PRODUCTION@',production).replace('@SAFETY@','unsafe ' if path==SOURCES[1] else '')+TESTS
        with tempfile.TemporaryDirectory(prefix='trueos-sdk-remove-leaf-') as tmp:
            directory=Path(tmp);rust=directory/'tests.rs';binary=directory/'tests';rust.write_text(fixture)
            subprocess.run(['rustc','+'+toolchain,'--edition=2024','--test','-Dwarnings',str(rust),'-o',str(binary)],check=True,timeout=60)
            print('SDK:',path,flush=True);subprocess.run([str(binary)],check=True,timeout=60)
if __name__=='__main__':main()
