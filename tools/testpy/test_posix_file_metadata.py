#!/usr/bin/env python3
"""Check actual POSIX metadata routing across a Hull's private filesystem globals."""
from pathlib import Path
import subprocess
import tempfile

from test_posix_renameat import function

ROOT = Path(__file__).resolve().parents[2]
SHIM = ROOT / "src/std_abi_shim.rs"
definitions = function(SHIM, "regular_file_metadata")
try:
    definitions += function(SHIM, "regular_file_metadata_from_cabi")
except ValueError:
    pass
definitions += function(SHIM, "fs_rc_to_errno")

source = r'''#![allow(dead_code)]
use core::ffi::c_int;
use std::cell::RefCell;
const TRUEOS_ENOENT:i32=2; const TRUEOS_EINVAL:i32=22;
const TRUEOS_EIO:i32=5; const TRUEOS_ENAMETOOLONG:i32=36;
struct Fixture { hull:bool, start:i32, statuses:Vec<i32>, response:Vec<u8>, read_error:Option<isize>, calls:Vec<String> }
std::thread_local! { static FIXTURE:RefCell<Fixture>=RefCell::new(Fixture {hull:false,start:7,statuses:vec![1],response:vec![],read_error:None,calls:vec![]}); }
mod hv { pub fn current_hull_guest_context_vm_id()->Option<u8> { super::FIXTURE.with(|f| f.borrow().hull.then_some(0)) } }
mod r {pub mod io {
    pub mod env {
        pub fn resolve_fs_path(path:&str,_:bool)->Option<String> {
            super::super::super::FIXTURE.with(|f| f.borrow_mut().calls.push(format!("resolve:{path}")));
            Some(format!("apps/probe/{}",path.trim_start_matches('/')))
        }
    }
    pub mod kfs {
        #[derive(Clone,Copy,Debug,Eq,PartialEq)] pub enum FsNodeKind { File,Directory }
        #[derive(Clone,Copy,Debug,Eq,PartialEq)] pub struct FsStat { pub kind:FsNodeKind,pub len:u64,pub file_id:u64,pub root_id:u32,pub revision:u64 }
        pub enum FsError { NoRoot }
        pub fn stat(path:&str)->Result<FsStat,FsError> {
            super::super::super::FIXTURE.with(|f| {
                let mut f=f.borrow_mut(); f.calls.push(format!("kfs:{path}"));
                // The Hull's private BSS contains no host mounts, even though
                // CABI writes have committed successfully on host disk 2.
                if f.hull {Err(FsError::NoRoot)} else {Ok(FsStat {kind:FsNodeKind::File,len:35,file_id:77,root_id:2,revision:99})}
            })
        }
    }
    pub mod fs_cabi { pub fn fs_error_to_code(_:super::kfs::FsError)->i32 {super::cabi::FS_ERR_NOT_FOUND} }
    pub mod cabi {
        include!("@ROOT@/src/r/cabi_codes.rs");
        pub fn trueos_cabi_poll_once() { super::super::super::FIXTURE.with(|f| f.borrow_mut().calls.push("poll".into())); }
    }
    pub mod async_fs_cabi {
        pub unsafe fn trueos_cabi_async_fs_metadata_start_v2(path:*const u8,len:usize)->i32 {
            let path=std::str::from_utf8(unsafe {std::slice::from_raw_parts(path,len)}).unwrap();
            super::super::super::FIXTURE.with(|f| {let mut f=f.borrow_mut();f.calls.push(format!("start:{path}"));f.start})
        }
        pub fn trueos_cabi_async_fs_status(id:u32)->i32 {
            super::super::super::FIXTURE.with(|f| {let mut f=f.borrow_mut();f.calls.push(format!("status:{id}"));if f.statuses.len()>1 {f.statuses.remove(0)} else {f.statuses[0]}})
        }
        pub fn trueos_cabi_async_fs_result_len(id:u32)->isize {
            super::super::super::FIXTURE.with(|f| {let mut f=f.borrow_mut();f.calls.push(format!("len:{id}"));f.response.len() as isize})
        }
        pub unsafe fn trueos_cabi_async_fs_result_read(id:u32,offset:usize,out:*mut u8,cap:usize)->isize {
            super::super::super::FIXTURE.with(|f| {
                let mut f=f.borrow_mut();f.calls.push(format!("read:{id}"));assert_eq!(offset,0);
                if let Some(error)=f.read_error {return error;}
                let count=cap.min(f.response.len());unsafe {std::ptr::copy_nonoverlapping(f.response.as_ptr(),out,count)};count as isize
            })
        }
        pub fn trueos_cabi_async_fs_discard(id:u32)->i32 {
            super::super::super::FIXTURE.with(|f| f.borrow_mut().calls.push(format!("discard:{id}")));0
        }
    }
}}
@DEFINITIONS@
fn reset(hull:bool) {
    let mut bytes=Vec::new();bytes.extend_from_slice(&1u32.to_le_bytes());bytes.extend_from_slice(&35u64.to_le_bytes());
    bytes.extend_from_slice(&77u64.to_le_bytes());bytes.extend_from_slice(&2u32.to_le_bytes());bytes.extend_from_slice(&99u64.to_le_bytes());
    FIXTURE.with(|f| *f.borrow_mut()=Fixture {hull,start:7,statuses:vec![0,0,1],response:bytes,read_error:None,calls:vec![]});
}
#[test] fn hull_reads_host_commit_metadata_without_consulting_private_mounts() {
    reset(true);let metadata=regular_file_metadata("/depthchecker_factchecker.txt").unwrap();
    assert_eq!(metadata,r::io::kfs::FsStat {kind:r::io::kfs::FsNodeKind::File,len:35,file_id:77,root_id:2,revision:99});
    FIXTURE.with(|f| {let f=f.borrow();assert_eq!(f.calls.first().unwrap(),"start:/depthchecker_factchecker.txt");
        assert!(!f.calls.iter().any(|x| x.starts_with("kfs:")||x.starts_with("resolve:")));
        assert_eq!(f.calls.iter().filter(|x|*x=="poll").count(),2);assert_eq!(f.calls.last().unwrap(),"discard:7");});
}
#[test] fn native_metadata_keeps_host_path_resolution_and_synchronous_stat() {
    reset(false);assert_eq!(regular_file_metadata("/file.txt").unwrap().file_id,77);
    FIXTURE.with(|f| assert_eq!(f.borrow().calls,vec!["resolve:/file.txt","kfs:apps/probe/file.txt"]));
}
#[test] fn missing_and_failed_requests_preserve_errors_and_release_operations() {
    for status in [r::io::cabi::FS_ERR_NOT_FOUND,r::io::cabi::FS_ERR_IO] {
        reset(true);FIXTURE.with(|f| f.borrow_mut().statuses=vec![status]);
        assert_eq!(regular_file_metadata("missing").unwrap_err(),fs_rc_to_errno(status));
        FIXTURE.with(|f| assert_eq!(f.borrow().calls.last().unwrap(),"discard:7"));
    }
    reset(true);FIXTURE.with(|f| f.borrow_mut().start=r::io::cabi::FS_ERR_BAD_PATH);
    assert_eq!(regular_file_metadata("bad").unwrap_err(),TRUEOS_EINVAL);
    FIXTURE.with(|f| assert_eq!(f.borrow().calls,vec!["start:bad"]));
    reset(true);FIXTURE.with(|f| f.borrow_mut().read_error=Some(r::io::cabi::FS_ERR_IO as isize));
    assert_eq!(regular_file_metadata("file").unwrap_err(),TRUEOS_EIO);
    FIXTURE.with(|f| assert_eq!(f.borrow().calls.last().unwrap(),"discard:7"));
}
#[test] fn full_v2_record_is_required_and_directory_kind_is_preserved() {
    for size in [0,24,31,33] {
        reset(true);FIXTURE.with(|f| f.borrow_mut().response.resize(size,0));
        assert_eq!(regular_file_metadata("file").unwrap_err(),TRUEOS_EIO);
        FIXTURE.with(|f| assert_eq!(f.borrow().calls.last().unwrap(),"discard:7"));
    }
    reset(true);FIXTURE.with(|f| f.borrow_mut().response[..4].copy_from_slice(&2u32.to_le_bytes()));
    assert_eq!(regular_file_metadata("directory").unwrap().kind,r::io::kfs::FsNodeKind::Directory);
    reset(true);FIXTURE.with(|f| f.borrow_mut().response[..4].copy_from_slice(&3u32.to_le_bytes()));
    assert_eq!(regular_file_metadata("badkind").unwrap_err(),TRUEOS_EIO);
}
'''
source = source.replace("@ROOT@", str(ROOT)).replace("@DEFINITIONS@", definitions)
with tempfile.TemporaryDirectory(prefix="posix-file-metadata-") as folder:
    folder = Path(folder)
    fixture = folder / "tests.rs"
    binary = folder / "tests"
    fixture.write_text(source)
    subprocess.run(["rustc", "--edition=2024", "--test", str(fixture), "-o", str(binary)], check=True)
    subprocess.run([str(binary)], check=True)
