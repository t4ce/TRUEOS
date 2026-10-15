#!/usr/bin/env python3
"""Run actual POSIX *at/open/stat implementations against a confined fake broker.

The production OpenFile enum/methods and C ABI functions are compiled unchanged;
only namespace storage, pointer translation and kernel bridge endpoints are host
fixtures. Broker boundaries assert that no FD/world registry lock is retained.
"""
from pathlib import Path
from test_posix_renameat import function
from test_posix_rename_binding import harness as rename_harness, run

ROOT = Path(__file__).resolve().parents[2]
SHIM = ROOT / 'src/std_abi_shim.rs'


def harness():
    source = SHIM.read_text()
    fixture = rename_harness()
    a = fixture.index('#[derive(Clone)] enum OpenFile {')
    b = fixture.index('\nstruct BytePipe ', a)
    enum_start = source.index('pub(crate) enum OpenFile {')
    enum_end = source.index('\npub(crate) struct FileLockHandle', enum_start)
    fixture = fixture[:a] + '#[derive(Clone)]\n' + source[enum_start:enum_end] + fixture[b:]
    fixture = fixture.replace('writable:true,flags:0}', 'writable:true,readable:true,lock:Arc::new(FileLockHandle{owner:r::fs::advisory_lock::Owner{realm:1,description:fd as u64},path:p.into()}),flags:0}')
    fixture = fixture.replace('pub mod kfs {pub fn exists', 'pub mod kfs {pub fn exists')
    fixture = fixture.replace('#[derive(Clone,Copy)]pub struct FsStat{pub revision:u64,pub file_id:u64,pub root_id:u32}', '#[derive(Clone,Copy,Debug)]pub struct FsStat{pub revision:u64,pub file_id:u64,pub root_id:u32,pub len:u64,pub kind:FsNodeKind} #[derive(Clone,Copy,Debug,PartialEq)]pub enum FsNodeKind{File,Directory}')
    start = fixture.index('fn regular_file_metadata(')
    end = fixture.index('\nmod r {', start)
    fixture = fixture[:start] + '''fn regular_file_metadata(p:&str)->Result<r::io::kfs::FsStat,i32>{
 boundary();let key=r::io::env::resolve_fs_path(p,false).ok_or(TRUEOS_EINVAL)?;let d=DISK.lock();
 let(kind,len)=if d.dirs.contains_key(&key){(r::io::kfs::FsNodeKind::Directory,0)}else if let Some(bytes)=d.files.get(&key){(r::io::kfs::FsNodeKind::File,bytes.len() as u64)}else{return Err(2)};
 Ok(r::io::kfs::FsStat{revision:23,file_id:17,root_id:2,len,kind})
}
''' + fixture[end:]
    fixture = fixture.replace('struct Disk {files:', 'struct Disk {dirs:BTreeMap<String,()>,files:')
    fixture = fixture.replace('Disk{files:BTreeMap::new(),', 'Disk{dirs:BTreeMap::new(),files:BTreeMap::new(),')
    fixture = fixture.replace(' pub mod net {', ' pub mod fs {pub mod advisory_lock {#[derive(Clone)]pub struct Owner{pub realm:u64,pub description:u64}}}\n pub mod net {')
    fixture = fixture.replace('  pub mod cabi {', '''  pub mod cabi {
   pub unsafe fn trueos_cabi_fs_create_dir_all(p:*const u8,n:usize)->i32{
    crate::boundary();let path=std::str::from_utf8(unsafe{std::slice::from_raw_parts(p,n)}).unwrap();
    let k=crate::key(path);crate::DISK.lock().dirs.insert(k,());crate::CREATES.fetch_add(1,std::sync::atomic::Ordering::Relaxed);0
   }
''')
    fixture += SUPPORT
    impl_start = source.index('impl OpenFile {')
    impl_end = source.index('\n/// Remove one descriptor', impl_start)
    fixture += source[impl_start:impl_end]
    struct_start = source.index('struct TrueosStat {')
    struct_end = source.index('\n#[repr(C)]', struct_start)
    fixture += '#[repr(C)]\n#[derive(Clone,Copy)]\n' + source[struct_start:struct_end]
    fixture += '\n'.join(function(SHIM, name) for name in (
        'posix_at_path','stat','lstat','fstat','fstatat','fstatat64','mkdirat',
        'mkdir','open','openat','futimens','utimes','pread64','pwrite64','open_file_poll_events',
    ))
    return fixture + TESTS


SUPPORT = r'''
use std::{slice,ptr};
const TRUEOS_EFAULT:i32=14;const TRUEOS_EEXIST:i32=17;const TRUEOS_ENOSYS:i32=38;
const TRUEOS_O_ACCMODE:i32=3;const TRUEOS_O_RDONLY:i32=0;const TRUEOS_O_WRONLY:i32=1;const TRUEOS_O_RDWR:i32=2;
const TRUEOS_O_CREAT:i32=0o100;const TRUEOS_O_TRUNC:i32=0o1000;const TRUEOS_O_DIRECTORY:i32=0o200000;
const TRUEOS_AT_SYMLINK_NOFOLLOW:i32=0x100;
const TRUEOS_DIR_MODE:u32=0o40755;const TRUEOS_FILE_MODE:u32=0o100644;
const POSIX_FILE_WORLD_PROBE_PATH:&str="trace-disabled";
struct FileLockHandle{owner:r::fs::advisory_lock::Owner,path:String}
fn file_lock_realm()->u64{1}
static NEXT:AtomicI32=AtomicI32::new(10);
fn next_file_fd()->i32{NEXT.fetch_add(1,Ordering::Relaxed)}
static CREATES:AtomicUsize=AtomicUsize::new(0);
fn copy_to_abi_out(p:*mut u8,b:&[u8])->bool{if p.is_null(){return false}unsafe{std::ptr::copy_nonoverlapping(b.as_ptr(),p,b.len())};true}
fn prepare_at(){reset();DISK.lock().dirs.clear();NEXT.store(10,Ordering::Relaxed);CREATES.store(0,Ordering::Relaxed);DISK.lock().dirs.insert("apps/codex".into(),());DISK.lock().dirs.insert("apps/codex/dir".into(),());disk("dir/file",b"payload");}
fn do_open(p:&str,flags:i32)->i32{let p=CString::new(p).unwrap();unsafe{open(p.as_ptr(),flags,0)}}
fn do_stat(fd:i32,p:&str,flags:i32)->Result<TrueosStat,i32>{
 let p=CString::new(p).unwrap();let mut out=std::mem::MaybeUninit::<TrueosStat>::uninit();unsafe{std::ptr::write_bytes(out.as_mut_ptr().cast::<u8>(),0xa5,std::mem::size_of::<TrueosStat>())};
 let rc=unsafe{fstatat(fd,p.as_ptr(),out.as_mut_ptr().cast(),flags)};
 if rc==0{Ok(unsafe{out.assume_init()})}else{assert!(unsafe{std::slice::from_raw_parts(out.as_ptr().cast::<u8>(),std::mem::size_of::<TrueosStat>())}.iter().all(|v|*v==0xa5));Err(TRUEOS_ERRNO.load(Ordering::Relaxed))}
}
fn do_mkdir(fd:i32,p:&str)->i32{let p=CString::new(p).unwrap();unsafe{mkdirat(fd,p.as_ptr(),0o700)}}
'''

TESTS = r'''
#[test]fn directory_open_relative_stat_and_close_are_real_descriptions(){
 prepare_at();let fd=do_open("dir",TRUEOS_O_DIRECTORY);assert!(fd>=3);
 let out=do_stat(fd,"file",0).unwrap();assert_eq!(out.st_mode,TRUEOS_FILE_MODE);assert_eq!(out.st_size,7);assert_eq!(out.__unused,[0;3]);assert_eq!(out.st_dev,2);assert_eq!(out.st_ino,17);
 let nofollow=do_stat(fd,"file",TRUEOS_AT_SYMLINK_NOFOLLOW).unwrap();assert_eq!(nofollow.st_size,7);
 let mut out=std::mem::MaybeUninit::<TrueosStat>::uninit();assert_eq!(unsafe{fstat(fd,out.as_mut_ptr().cast())},0);let out=unsafe{out.assume_init()};assert_eq!(out.st_mode,TRUEOS_DIR_MODE);assert_eq!(out.st_nlink,2);
 let p=c"file";let file=unsafe{openat(fd,p.as_ptr(),TRUEOS_O_RDONLY,0)};assert!(file>fd);
 assert_eq!(unsafe{close(fd)},0);assert_eq!(do_stat(fd,"file",0).err(),Some(TRUEOS_EBADF));assert_eq!(unsafe{close(file)},0);
}
#[test]fn absolute_ignores_bad_descriptor_and_relative_respects_root(){
 prepare_at();assert_eq!(do_stat(-990,"/apps/codex/dir/file",0).unwrap().st_size,7);
 assert_eq!(do_stat(TRUEOS_AT_FDCWD,"dir/file",0).unwrap().st_size,7);
 let fd=do_open("dir",0);assert!(fd>=3);assert_eq!(do_stat(fd,"../file",0).err(),Some(TRUEOS_EINVAL));
 assert_eq!(do_stat(-990,"dir/file",0).err(),Some(TRUEOS_EBADF));
 assert_eq!(do_stat(0,"dir/file",0).err(),Some(TRUEOS_ENOTDIR));
 let regular=do_open("dir/file",0);assert!(regular>=3);assert_eq!(do_stat(regular,"child",0).err(),Some(TRUEOS_ENOTDIR));
 assert!(SOCKET_FDS.lock().insert(77,()).is_ok());assert_eq!(do_stat(77,"child",0).err(),Some(TRUEOS_ENOTDIR));
}
#[test]fn invalid_flags_empty_null_and_missing_paths_do_not_write_output(){
 prepare_at();for flags in [1,0x1000,0x8000,-1]{assert_eq!(do_stat(TRUEOS_AT_FDCWD,"dir/file",flags).err(),Some(TRUEOS_EINVAL));}
 assert_eq!(do_stat(TRUEOS_AT_FDCWD,"",0).err(),Some(TRUEOS_ENOENT));assert_eq!(do_stat(TRUEOS_AT_FDCWD,"missing",0).err(),Some(TRUEOS_ENOENT));
 let mut out=std::mem::MaybeUninit::<TrueosStat>::uninit();assert_eq!(unsafe{fstatat(TRUEOS_AT_FDCWD,ptr::null(),out.as_mut_ptr().cast(),0)},-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EFAULT);
 assert_eq!(unsafe{fstatat(TRUEOS_AT_FDCWD,c"dir".as_ptr(),ptr::null_mut(),0)},-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EFAULT);
 assert_eq!(std::mem::size_of::<TrueosStat>(),144);assert_eq!(std::mem::offset_of!(TrueosStat,st_size),48);assert_eq!(std::mem::offset_of!(TrueosStat,st_ctime),104);
}
#[test]fn mkdirat_checks_parent_and_existing_target_and_uses_anchor(){
 prepare_at();let fd=do_open("dir",0);assert_eq!(do_mkdir(fd,"child"),0);assert!(DISK.lock().dirs.contains_key("apps/codex/dir/child"));
 assert_eq!(do_mkdir(fd,"child"),-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EEXIST);
 assert_eq!(do_mkdir(fd,"file"),-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EEXIST);
 assert_eq!(do_mkdir(fd,"missing/child"),-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_ENOENT);
 assert_eq!(do_mkdir(fd,"file/child"),-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_ENOTDIR);
 assert_eq!(do_mkdir(-900,"/apps/codex/absolute"),0);assert_eq!(do_mkdir(-900,"relative"),-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EBADF);
 assert_eq!(CREATES.load(Ordering::Relaxed),2);
}
#[test]fn directory_is_not_byte_storage_and_timestamp_mutation_is_honest(){
 prepare_at();let fd=do_open("dir",0);let mut b=0u8;assert_eq!(unsafe{write(fd,(&b as *const u8).cast(),1)},-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EISDIR);
 assert_eq!(unsafe{pread64(fd,(&mut b as *mut u8).cast(),1,0)},-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EISDIR);
 assert_eq!(do_open("dir/file",TRUEOS_O_DIRECTORY),-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_ENOTDIR);
 assert_eq!(do_open("dir",TRUEOS_O_RDWR),-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EISDIR);
 assert_eq!(unsafe{futimens(fd,ptr::null())},-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_ENOSYS);
 assert_eq!(unsafe{futimens(-999,ptr::null())},-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EBADF);
 assert_eq!(unsafe{utimes(c"dir/file".as_ptr(),ptr::null())},-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_ENOSYS);
}
'''

if __name__ == '__main__':
    run(harness())
