#!/usr/bin/env python3
"""Compile the production rename/renameat shims with an in-memory FS boundary.

The real C-string conversion, filesystem errno mapping, read/write transaction
helpers, launch-root path resolution, FsPath and fixed FD registry are used.
Only guest-pointer translation, launch context and filesystem storage are mocked.
Registry guards also reject overlapping file/socket locks.
"""

from pathlib import Path
import re
import subprocess
import tempfile
import textwrap

from test_clip_position3_uv_texture import constant, item

ROOT = Path(__file__).resolve().parents[2]
SHIM = str(ROOT / "src/std_abi_shim.rs")


def function(path, name):
    source = Path(path).read_text()
    matches = list(re.finditer(
        rf'^(?P<indent> *)(?:pub(?:\([^)]*\))?\s+)?'
        rf'(?:unsafe\s+extern\s+"C"\s+)?fn {re.escape(name)}\(',
        source, re.MULTILINE))
    if len(matches) != 1:
        raise ValueError(f"{path}: expected one function {name}, got {len(matches)}")
    declaration = matches[0]
    ending = re.search(rf'^{declaration["indent"]}\}}\s*\n',
                       source[declaration.end():], re.MULTILINE)
    if ending is None:
        raise ValueError(f"{path}: missing function boundary {name}")
    # Exclude no_mangle: these functions must not replace the host libc symbols.
    return textwrap.dedent(source[declaration.start():declaration.end() + ending.end()])


def harness():
    definitions = [constant(SHIM, name) for name in (
        "TRUEOS_ENOENT", "TRUEOS_EINVAL", "TRUEOS_ENAMETOOLONG", "TRUEOS_EIO",
        "TRUEOS_EBADF", "TRUEOS_ENOTDIR", "TRUEOS_AT_FDCWD",
    )]
    definitions += [item(SHIM, name) for name in (
        "abi_cstr_to_string", "fs_rc_to_errno", "read_file_from_cabi",
        "write_file_to_cabi", "renameat_dirfd_check",
    )]
    definitions += [function(SHIM, name) for name in ("rename", "renameat")]
    env = "\n".join(function(ROOT / "src/r/io.rs", name) for name in (
        "normalize_app_path", "resolve_fs_path"))
    support = r'''#![allow(dead_code)]
extern crate alloc;
use alloc::{string::String, vec::Vec};
use core::{ffi::{c_char, c_int}, ptr, sync::atomic::{AtomicBool, AtomicI32, Ordering}};
#[path="@ROOT@/src/r/static_map.rs"] mod static_map;
use static_map::FixedKeyMap;
static TRUEOS_ERRNO: AtomicI32 = AtomicI32::new(0);
static FILE_LOCKED: AtomicBool = AtomicBool::new(false);
static SOCKET_LOCKED: AtomicBool = AtomicBool::new(false);
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value:T)->Self { Self(std::sync::Mutex::new(value)) }
    fn lock(&self)->std::sync::MutexGuard<'_,T> { self.0.lock().unwrap() }
}
struct Registry { table:Mutex<FixedKeyMap<c_int, (), 8>>, file:bool }
struct RegistryGuard<'a> { guard:std::sync::MutexGuard<'a,FixedKeyMap<c_int,(),8>>, file:bool }
impl Registry {
    const fn new(file:bool)->Self { Self { table:Mutex::new(FixedKeyMap::new()), file } }
    fn lock(&self)->RegistryGuard<'_> {
        let (own,other)=if self.file { (&FILE_LOCKED,&SOCKET_LOCKED) } else { (&SOCKET_LOCKED,&FILE_LOCKED) };
        assert!(!other.load(Ordering::SeqCst),"overlapping file/socket registry locks");
        assert!(!own.swap(true,Ordering::SeqCst),"recursive registry lock");
        RegistryGuard { guard:self.table.lock(), file:self.file }
    }
}
impl core::ops::Deref for RegistryGuard<'_> {
    type Target=FixedKeyMap<c_int,(),8>;
    fn deref(&self)->&Self::Target { &self.guard }
}
impl core::ops::DerefMut for RegistryGuard<'_> {
    fn deref_mut(&mut self)->&mut Self::Target { &mut self.guard }
}
impl Drop for RegistryGuard<'_> {
    fn drop(&mut self) {
        (if self.file { &FILE_LOCKED } else { &SOCKET_LOCKED }).store(false,Ordering::SeqCst);
    }
}
static OPEN_FILES:Registry=Registry::new(true);
static SOCKET_FDS:Registry=Registry::new(false);
fn abi_read_bytes<'a>(pointer:*const u8,len:usize)->Option<&'a [u8]> {
    if pointer.is_null() { None } else { Some(unsafe { core::slice::from_raw_parts(pointer,len) }) }
}
mod r {
    #[path="@ROOT@/src/r/path.rs"] pub mod path;
    pub mod io {
        pub mod kfs { pub fn exists(_path:&str)->Result<bool,()> { unreachable!() } }
        pub mod env {
            use alloc::string::String;
            pub static ROOT:crate::Mutex<Option<String>>=crate::Mutex::new(None);
            fn trueosfs_scope_granted()->bool { false }
            fn current_app_fs_root()->Option<String> { ROOT.lock().clone() }
            @ENV@
        }
        pub mod cabi {
            include!("@ROOT@/src/r/cabi_codes.rs");
            use alloc::{collections::BTreeMap,string::String,vec::Vec};
            #[derive(Clone,Copy,Debug,PartialEq,Eq)]
            pub enum Stage { Query,Read,Begin,Chunk,Finish,Remove }
            pub struct State {
                pub files:BTreeMap<String,Vec<u8>>,
                pub pending:Option<(String,Vec<u8>)>,
                pub operations:Vec<(Stage,String)>,
                pub aborts:usize,
                pub failure:Option<(Stage,i32)>,
            }
            impl State {
                pub const fn new()->Self {
                    Self { files:BTreeMap::new(),pending:None,operations:Vec::new(),aborts:0,failure:None }
                }
                fn fail(&self,stage:Stage)->Option<i32> {
                    self.failure.filter(|(at,_)| *at==stage).map(|(_,rc)| rc)
                }
            }
            pub static STATE:crate::Mutex<State>=crate::Mutex::new(State::new());
            unsafe fn resolved(pointer:*const u8,len:usize)->Result<String,i32> {
                let raw=unsafe { core::slice::from_raw_parts(pointer,len) };
                let raw=core::str::from_utf8(raw).map_err(|_| FS_ERR_BAD_UTF8)?;
                let resolved=super::env::resolve_fs_path(raw,false).ok_or(FS_ERR_BAD_PATH)?;
                crate::r::path::FsPath::parse(&resolved,false)
                    .map(|path| path.to_relative_string()).map_err(|_| FS_ERR_BAD_PATH)
            }
            pub unsafe fn trueos_cabi_fs_read_file(pointer:*const u8,len:usize,out:*mut u8,cap:usize)->isize {
                let path=match unsafe { resolved(pointer,len) } { Ok(path)=>path,Err(rc)=>return rc as isize };
                let stage=if out.is_null() { Stage::Query } else { Stage::Read };
                let mut state=STATE.lock();
                state.operations.push((stage,path.clone()));
                if let Some(rc)=state.fail(stage) { return rc as isize; }
                let Some(bytes)=state.files.get(&path) else { return FS_ERR_NOT_FOUND as isize; };
                if !out.is_null() {
                    assert!(cap>=bytes.len());
                    unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(),out,bytes.len()); }
                }
                bytes.len() as isize
            }
            pub unsafe fn trueos_cabi_fs_write_begin(pointer:*const u8,len:usize,_total:u64,out:*mut u32)->i32 {
                let path=match unsafe { resolved(pointer,len) } { Ok(path)=>path,Err(rc)=>return rc };
                let mut state=STATE.lock();
                state.operations.push((Stage::Begin,path.clone()));
                if let Some(rc)=state.fail(Stage::Begin) { return rc; }
                state.pending=Some((path,Vec::new()));
                unsafe { *out=17; }
                0
            }
            pub unsafe fn trueos_cabi_fs_write_chunk(handle:u32,pointer:*const u8,len:usize)->i32 {
                assert_eq!(handle,17);
                let mut state=STATE.lock();
                let path=state.pending.as_ref().unwrap().0.clone();
                state.operations.push((Stage::Chunk,path));
                if let Some(rc)=state.fail(Stage::Chunk) { return rc; }
                state.pending.as_mut().unwrap().1.extend_from_slice(unsafe { core::slice::from_raw_parts(pointer,len) });
                0
            }
            pub unsafe fn trueos_cabi_fs_write_abort(handle:u32)->i32 {
                assert_eq!(handle,17);
                let mut state=STATE.lock();state.aborts+=1;state.pending=None;0
            }
            pub unsafe fn trueos_cabi_fs_write_finish(handle:u32)->i32 {
                assert_eq!(handle,17);
                let mut state=STATE.lock();
                let path=state.pending.as_ref().unwrap().0.clone();
                state.operations.push((Stage::Finish,path));
                if let Some(rc)=state.fail(Stage::Finish) { return rc; }
                let (path,bytes)=state.pending.take().unwrap();state.files.insert(path,bytes);0
            }
            pub unsafe fn trueos_cabi_fs_remove(pointer:*const u8,len:usize)->i32 {
                let path=match unsafe { resolved(pointer,len) } { Ok(path)=>path,Err(rc)=>return rc };
                let mut state=STATE.lock();state.operations.push((Stage::Remove,path.clone()));
                if let Some(rc)=state.fail(Stage::Remove) { return rc; }
                if state.files.remove(&path).is_some() { 0 } else { FS_ERR_NOT_FOUND }
            }
        }
    }
}
#[cfg(test)] mod tests {
    use super::*;
    use r::io::cabi::{STATE,Stage};
    fn prepare() {
        *OPEN_FILES.lock()=FixedKeyMap::new();*SOCKET_FDS.lock()=FixedKeyMap::new();
        OPEN_FILES.lock().insert(7,()).unwrap();SOCKET_FDS.lock().insert(8,()).unwrap();
        *r::io::env::ROOT.lock()=Some(String::from("apps/velosrv"));
        *STATE.lock()=r::io::cabi::State::new();
        STATE.lock().files.insert(String::from("apps/velosrv/source"),b"payload".to_vec());
        TRUEOS_ERRNO.store(123,Ordering::Relaxed);
    }
    fn invoke(oldfd:c_int,old:&str,newfd:c_int,new:&str)->c_int {
        let old=std::ffi::CString::new(old).unwrap();let new=std::ffi::CString::new(new).unwrap();
        unsafe { renameat(oldfd,old.as_ptr(),newfd,new.as_ptr()) }
    }
    fn errno()->c_int { TRUEOS_ERRNO.load(Ordering::Relaxed) }
    fn assert_preserved() {
        let state=STATE.lock();
        assert_eq!(state.files.get("apps/velosrv/source").unwrap(),b"payload");
        assert!(!state.files.contains_key("apps/velosrv/destination"));
        assert!(state.operations.iter().all(|(stage,_)| *stage!=Stage::Remove));
    }
    #[test] fn containers_read_and_update_the_same_shared_userdata_files() {
        prepare();
        for path in ["/apps/voxy/userdata/voxygen/settings.ron",
                     "/apps/voxy/userdata/voxygen/profile.ron",
                     "/apps/voxy/userdata/voxygen/logs/today.log"] {
            *r::io::env::ROOT.lock()=Some("apps/voxy/container_1--first".into());
            write_file_to_cabi(path,b"first launch").unwrap();
            *r::io::env::ROOT.lock()=Some("apps/voxy/container_1--second".into());
            assert_eq!(read_file_from_cabi(path).unwrap(),b"first launch");
            write_file_to_cabi(path,b"second launch").unwrap();
            *r::io::env::ROOT.lock()=Some("apps/voxy".into());
            assert_eq!(read_file_from_cabi(path).unwrap(),b"second launch");
        }
        let state=STATE.lock();
        assert!(state.operations.iter().all(|(_,path)| path.starts_with("apps/voxy/userdata/")));
    }
    #[test] fn cwd_rename_commits_destination_before_deleting_source() {
        prepare();assert_eq!(invoke(TRUEOS_AT_FDCWD,"source",TRUEOS_AT_FDCWD,"destination"),0);
        assert_eq!(errno(),0);
        let state=STATE.lock();
        assert!(!state.files.contains_key("apps/velosrv/source"));
        assert_eq!(state.files.get("apps/velosrv/destination").unwrap(),b"payload");
        assert_eq!(state.operations.iter().map(|(stage,_)| *stage).collect::<Vec<_>>(),
            [Stage::Query,Stage::Read,Stage::Begin,Stage::Chunk,Stage::Finish,Stage::Remove]);
    }
    #[test] fn absolute_arguments_ignore_their_own_directory_descriptor_only() {
        for (oldfd,old,newfd,new) in [
            (-912,"/source",-913,"/destination"),
            (-912,"/source",TRUEOS_AT_FDCWD,"destination"),
            (TRUEOS_AT_FDCWD,"source",-913,"/destination"),
        ] {
            prepare();assert_eq!(invoke(oldfd,old,newfd,new),0);assert_eq!(errno(),0);
        }
        prepare();assert_eq!(invoke(-912,"/source",-913,"destination"),-1);assert_eq!(errno(),TRUEOS_EBADF);
        assert_preserved();assert!(STATE.lock().operations.is_empty());
        prepare();assert_eq!(invoke(-912,"source",-913,"/destination"),-1);assert_eq!(errno(),TRUEOS_EBADF);
        assert_preserved();assert!(STATE.lock().operations.is_empty());
    }
    #[test] fn both_relative_arguments_validate_known_nondirectory_and_unknown_fds() {
        for fd in [0,1,2,7,8,99,-1] {
            let expected=if [0,1,2,7,8].contains(&fd) { TRUEOS_ENOTDIR } else { TRUEOS_EBADF };
            for source_invalid in [true,false] {
                prepare();
                let (oldfd,newfd)=if source_invalid { (fd,TRUEOS_AT_FDCWD) } else { (TRUEOS_AT_FDCWD,fd) };
                assert_eq!(invoke(oldfd,"source",newfd,"destination"),-1);assert_eq!(errno(),expected);
                assert_preserved();assert!(STATE.lock().operations.is_empty());
                assert!(!FILE_LOCKED.load(Ordering::SeqCst));assert!(!SOCKET_LOCKED.load(Ordering::SeqCst));
            }
        }
    }
    #[test] fn empty_and_invalid_c_strings_fail_before_filesystem_mutation() {
        for (old,new) in [("","destination"),("source","")] {
            prepare();assert_eq!(invoke(TRUEOS_AT_FDCWD,old,TRUEOS_AT_FDCWD,new),-1);assert_eq!(errno(),TRUEOS_ENOENT);
            assert!(STATE.lock().operations.is_empty());assert_preserved();
        }
        prepare();let valid=std::ffi::CString::new("source").unwrap();
        assert_eq!(unsafe { renameat(TRUEOS_AT_FDCWD,ptr::null(),TRUEOS_AT_FDCWD,valid.as_ptr()) },-1);
        assert_eq!(errno(),TRUEOS_EINVAL);
        let invalid=[0xffu8,0];
        assert_eq!(unsafe { renameat(TRUEOS_AT_FDCWD,valid.as_ptr(),TRUEOS_AT_FDCWD,invalid.as_ptr().cast()) },-1);
        assert_eq!(errno(),TRUEOS_EINVAL);assert!(STATE.lock().operations.is_empty());
    }
    #[test] fn read_and_write_failures_preserve_source_and_propagate_errno() {
        for (stage,rc,expected) in [
            (Stage::Query,r::io::cabi::FS_ERR_NOT_FOUND,TRUEOS_ENOENT),
            (Stage::Read,r::io::cabi::FS_ERR_IO,TRUEOS_EIO),
            (Stage::Begin,r::io::cabi::FS_ERR_BAD_PATH,TRUEOS_EINVAL),
            (Stage::Chunk,r::io::cabi::FS_ERR_NO_SPACE,TRUEOS_EIO),
            (Stage::Finish,r::io::cabi::FS_ERR_IO,TRUEOS_EIO),
        ] {
            prepare();STATE.lock().failure=Some((stage,rc));
            assert_eq!(invoke(TRUEOS_AT_FDCWD,"source",TRUEOS_AT_FDCWD,"destination"),-1);
            assert_eq!(errno(),expected);assert_preserved();
            assert_eq!(STATE.lock().aborts,usize::from(stage==Stage::Chunk));
        }
    }
    #[test] fn same_resolved_path_checks_existence_without_mutation() {
        prepare();assert_eq!(invoke(TRUEOS_AT_FDCWD,"./source",-73,"/apps/velosrv//source"),0);
        assert_eq!(errno(),0);assert_preserved();
        assert_eq!(STATE.lock().operations.iter().map(|(stage,_)| *stage).collect::<Vec<_>>(),[Stage::Query,Stage::Read]);
        prepare();assert_eq!(invoke(TRUEOS_AT_FDCWD,"missing",TRUEOS_AT_FDCWD,"./missing"),-1);
        assert_eq!(errno(),TRUEOS_ENOENT);assert_preserved();
    }
    #[test] fn confinement_rejects_parent_escape_and_preserves_source() {
        prepare();assert_eq!(invoke(TRUEOS_AT_FDCWD,"source",TRUEOS_AT_FDCWD,"../destination"),-1);
        assert_eq!(errno(),TRUEOS_EINVAL);assert_preserved();assert!(STATE.lock().operations.is_empty());
    }
    #[test] fn failed_cleanup_retains_committed_destination_and_reports_success() {
        prepare();STATE.lock().failure=Some((Stage::Remove,r::io::cabi::FS_ERR_IO));
        assert_eq!(invoke(TRUEOS_AT_FDCWD,"source",TRUEOS_AT_FDCWD,"destination"),0);assert_eq!(errno(),0);
        let state=STATE.lock();
        assert_eq!(state.files.get("apps/velosrv/source").unwrap(),b"payload");
        assert_eq!(state.files.get("apps/velosrv/destination").unwrap(),b"payload");
    }
}
'''
    return support.replace("@ROOT@", str(ROOT)).replace("@ENV@", env) + "\n".join(definitions)


def main():
    with tempfile.TemporaryDirectory(prefix="trueos-posix-renameat-") as directory:
        folder = Path(directory)
        rust = folder / "tests.rs"
        binary = folder / "tests"
        rust.write_text(harness())
        subprocess.run(["rustc", "--edition=2024", "--test", str(rust), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
