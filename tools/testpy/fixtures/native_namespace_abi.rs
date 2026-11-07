#![allow(dead_code)]
use std::cell::RefCell;
use std::string::String;
const FS_ERR_BAD_PARAM: i32 = -4;
const FS_ERR_BAD_UTF8: i32 = -1;
const FS_ERR_TOO_LARGE: i32 = -7;
const FS_ERR_BAD_PATH: i32 = -6;
const BLUEPRINT_ASYNC_FS_MAX_PATH: usize = 1024;
#[derive(Debug, PartialEq)]
enum Call {
    Create(u32, String),
    Remove(u32, String, NodeKind),
    Guest(u32, u64, u64, Vec<u8>),
    Reply(i32),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NodeKind {
    File,
    Directory,
}
struct Boundary {
    guest: bool,
    status: u32,
    value: i32,
    calls: Vec<Call>,
}
thread_local! {static BOUNDARY:RefCell<Boundary>=RefCell::new(Boundary{guest:false,status:7,value:31,calls:Vec::new()});}
fn boundary<T>(f: impl FnOnce(&mut Boundary) -> T) -> T {
    BOUNDARY.with(|s| f(&mut s.borrow_mut()))
}
fn reset(guest: bool) {
    boundary(|s| {
        *s = Boundary {
            guest,
            status: 7,
            value: 31,
            calls: Vec::new(),
        }
    });
}
fn runtime_context_key() -> u32 {
    0x8123_0045
}
mod env {
    pub fn resolve_fs_path(path: &str, _: bool) -> Option<String> {
        Some(path.trim_start_matches('/').into())
    }
}
mod hv {
    pub fn current_hull_guest_context_vm_id() -> Option<u8> {
        crate::boundary(|s| s.guest.then_some(3))
    }
}
mod r {
    pub mod fs {
        pub mod trueosfs {
            pub use crate::NodeKind;
        }
    }
    pub mod cabi_codes {
        pub const BLUEPRINT_ASYNC_FS_MAX_PATH: usize = 1024;
    }
    pub mod io {
        pub mod cabi {
            pub const FS_ERR_BAD_PARAM: i32 = -4;
        }
    }
}
mod trueos_vm {
    pub mod vmcall {
        pub const OP_BP_ASYNC_FS_CREATE_DIR_START_V1: u32 = 0x24e;
        pub const OP_BP_ASYNC_FS_REMOVE_LEAF_START_V1: u32 = 0x24f;
        pub const STATUS_OK: u32 = 7;
        pub fn call_with_payload(
            op: u32,
            a: u64,
            b: u64,
            bytes: &[u8],
            _: &mut [u8],
        ) -> (u32, u64) {
            crate::boundary(|s| {
                s.calls.push(crate::Call::Guest(op, a, b, bytes.to_vec()));
                (s.status, (s.value as i64) as u64)
            })
        }
    }
}
mod adapter {
    use super::*;
    fn is_virtual_write(_: &str) -> bool {
        false
    }
    fn start_create_dir(owner: u32, path: String) -> i32 {
        boundary(|s| {
            s.calls.push(Call::Create(owner, path));
            s.value
        })
    }
    fn start_remove_leaf(owner: u32, path: String, kind: NodeKind) -> i32 {
        boundary(|s| {
            s.calls.push(Call::Remove(owner, path, kind));
            s.value
        })
    }
    // @ADAPTER@
}
#[derive(Debug, PartialEq)]
enum DispatchOutcome {
    Resume,
    Continue,
}
const STATUS_OK: u32 = 7;
fn write_response(_: u8, _: u64, status: u32, value: u64, _: u64) {
    assert_eq!(status, STATUS_OK);
    boundary(|s| s.calls.push(Call::Reply((value as i64) as i32)));
}
fn admit_create(arg0: u64, arg1: u64, req_len: u32) -> DispatchOutcome {
    let op = trueos_vm::vmcall::OP_BP_ASYNC_FS_CREATE_DIR_START_V1;
    let vm_id = 3;
    let seq = 7;
    // @CREATE_GUARD@
    DispatchOutcome::Continue
}
fn admit_remove(arg0: u64, arg1: u64, req_len: u32) -> DispatchOutcome {
    let op = trueos_vm::vmcall::OP_BP_ASYNC_FS_REMOVE_LEAF_START_V1;
    let vm_id = 3;
    let seq = 7;
    // @REMOVE_GUARD@
    DispatchOutcome::Continue
}
#[test]
fn host_calls_preserve_owner_kind_and_normalized_utf8_path() {
    reset(false);
    let p = "/Größe/leaf".as_bytes();
    unsafe {
        assert_eq!(
            adapter::trueos_cabi_async_fs_create_dir_start_v1(p.as_ptr(), p.len()),
            31
        );
        assert_eq!(
            adapter::trueos_cabi_async_fs_remove_leaf_start_v1(p.as_ptr(), p.len(), 2),
            31
        );
    }
    boundary(|s| {
        assert_eq!(
            s.calls,
            [
                Call::Create(0x8123_0045, "Größe/leaf".into()),
                Call::Remove(0x8123_0045, "Größe/leaf".into(), NodeKind::Directory)
            ]
        )
    });
}
#[test]
fn guest_calls_use_distinct_opcode_kind_and_signed_reply() {
    reset(true);
    boundary(|s| s.value = -9);
    let p = b"dir/file";
    unsafe {
        assert_eq!(
            adapter::trueos_cabi_async_fs_create_dir_start_v1(p.as_ptr(), p.len()),
            -9
        );
        assert_eq!(
            adapter::trueos_cabi_async_fs_remove_leaf_start_v1(p.as_ptr(), p.len(), 1),
            -9
        );
    }
    boundary(|s| {
        assert_eq!(
            s.calls,
            [
                Call::Guest(0x24e, 0, 0, p.to_vec()),
                Call::Guest(0x24f, 1, 0, p.to_vec())
            ]
        )
    });
    reset(true);
    boundary(|s| s.status = 99);
    unsafe {
        assert_eq!(
            adapter::trueos_cabi_async_fs_remove_leaf_start_v1(p.as_ptr(), p.len(), 1),
            -4
        );
        assert_eq!(
            adapter::trueos_cabi_async_fs_create_dir_start_v1(p.as_ptr(), p.len()),
            -4
        );
    }
}
#[test]
fn malformed_parameters_fail_before_borrow_or_submission() {
    reset(false);
    unsafe {
        assert_eq!(
            adapter::trueos_cabi_async_fs_remove_leaf_start_v1(1usize as *const u8, 3, 0),
            -4
        );
        assert_eq!(
            adapter::trueos_cabi_async_fs_remove_leaf_start_v1(std::ptr::null(), 1, 1),
            -4
        );
        assert_eq!(
            adapter::trueos_cabi_async_fs_remove_leaf_start_v1(std::ptr::null(), 0, 2),
            -4
        );
        assert_eq!(
            adapter::trueos_cabi_async_fs_create_dir_start_v1(1usize as *const u8, 1025),
            -7
        );
        let p = [0xff];
        assert_eq!(
            adapter::trueos_cabi_async_fs_create_dir_start_v1(p.as_ptr(), 1),
            -1
        );
    }
    boundary(|s| assert!(s.calls.is_empty()));
}
#[test]
fn wide_kind_is_not_narrowed_into_valid_kind() {
    for value in [0, 3, u64::MAX, (1u64 << 32) | 1, (1u64 << 32) | 2] {
        reset(false);
        assert_eq!(admit_remove(value, 0, 8), DispatchOutcome::Resume);
        boundary(|s| assert_eq!(s.calls, [Call::Reply(-4)]));
    }
}
#[test]
fn native_vm_guards_bound_paths_and_reserved_arguments() {
    for (a, b, n) in [(1, 1, 8), (2, 0, 0), (1, 0, 1025)] {
        reset(false);
        assert_eq!(admit_remove(a, b, n), DispatchOutcome::Resume);
    }
    for (a, b, n) in [(1, 0, 8), (0, 1, 8), (0, 0, 1025)] {
        reset(false);
        assert_eq!(admit_create(a, b, n), DispatchOutcome::Resume);
    }
    reset(false);
    assert_eq!(admit_remove(1, 0, 1024), DispatchOutcome::Continue);
    assert_eq!(admit_remove(2, 0, 1), DispatchOutcome::Continue);
    assert_eq!(admit_create(0, 0, 0), DispatchOutcome::Continue);
    boundary(|s| assert!(s.calls.is_empty()));
}
