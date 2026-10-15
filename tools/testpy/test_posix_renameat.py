#!/usr/bin/env python3
"""Exercise production renameat validation with current async rename semantics.

Shares the production rename/open-file-world fixture with the binding test;
filesystem storage, pointer conversion and launch-root selection are mocked.
The function extractor remains shared by other focused ABI harnesses.
"""

from pathlib import Path
import re
import textwrap


ROOT = Path(__file__).resolve().parents[2]
SHIM = str(ROOT / "src/std_abi_shim.rs")


def function(path, name):
    source = Path(path).read_text()
    matches = list(re.finditer(
        rf'^(?P<indent> *)(?:pub(?:\([^)]*\))?\s+)?'
        rf'(?:unsafe\s+(?:extern\s+"C"\s+)?)?fn {re.escape(name)}\(',
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
    # Share the current async namespace + production file-world fixture. Keep
    # function() above available to the other production-extraction harnesses.
    from test_posix_rename_binding import harness as binding_harness
    return binding_harness() + TESTS


TESTS = r'''
fn invoke_at(a:i32,old:&str,b:i32,new:&str)->i32 {
 let old=CString::new(old).unwrap();let new=CString::new(new).unwrap();
 unsafe{renameat(a,old.as_ptr(),b,new.as_ptr())}
}
fn prepare(){reset();disk("source",b"payload");disk("target",b"old");}
#[test]fn cwd_rename_replaces_destination_with_one_namespace_operation(){
 prepare();assert_eq!(invoke_at(TRUEOS_AT_FDCWD,"source",TRUEOS_AT_FDCWD,"target"),0);
 assert_eq!(DISK.lock().files["apps/codex/target"],b"payload");assert!(!DISK.lock().files.contains_key("apps/codex/source"));
 assert_eq!(POLLS.load(Ordering::Relaxed),1);assert_eq!(DISCARDS.load(Ordering::Relaxed),1);
}
#[test]fn absolute_arguments_ignore_their_own_directory_descriptor_only(){
 for (a,old,b,new)in [(-912,"/apps/codex/source",-913,"/apps/codex/target"),(-912,"/apps/codex/source",TRUEOS_AT_FDCWD,"target"),(TRUEOS_AT_FDCWD,"source",-913,"/apps/codex/target")]{
 prepare();assert_eq!(invoke_at(a,old,b,new),0);}
 for(a,old,b,new)in [(-912,"source",-913,"/apps/codex/target"),(-912,"/apps/codex/source",-913,"target")]{
 prepare();assert_eq!(invoke_at(a,old,b,new),-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EBADF);assert_eq!(DISCARDS.load(Ordering::Relaxed),0);}
}
#[test]fn descriptors_are_not_treated_as_directory_paths(){
 for (fd,expected)in [(0,TRUEOS_ENOTDIR),(7,TRUEOS_ENOTDIR),(8,TRUEOS_ENOTDIR),(-912,TRUEOS_EBADF)]{
 prepare();open_fd(7,"target",false);assert!(SOCKET_FDS.lock().insert(8,()).is_ok());
 assert_eq!(invoke_at(fd,"source",TRUEOS_AT_FDCWD,"target"),-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),expected);assert_eq!(DISCARDS.load(Ordering::Relaxed),0);}
}
#[test]fn failed_namespace_operation_keeps_both_names(){
 prepare();DISK.lock().fail_rename=-9;assert_eq!(invoke_at(TRUEOS_AT_FDCWD,"source",TRUEOS_AT_FDCWD,"target"),-1);
 assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),17);assert_eq!(DISK.lock().files["apps/codex/source"],b"payload");assert_eq!(DISK.lock().files["apps/codex/target"],b"old");
}
#[test]fn empty_and_null_arguments_fail_before_namespace_work(){
 for (old,new)in [("","target"),("source","")]{prepare();assert_eq!(invoke_at(TRUEOS_AT_FDCWD,old,TRUEOS_AT_FDCWD,new),-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_ENOENT);}
 prepare();assert_eq!(unsafe{renameat(TRUEOS_AT_FDCWD,std::ptr::null(),TRUEOS_AT_FDCWD,c"target".as_ptr())},-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EINVAL);assert_eq!(DISCARDS.load(Ordering::Relaxed),0);
}
'''


def main():
    from test_posix_rename_binding import run
    run(harness())


if __name__ == "__main__":
    main()
