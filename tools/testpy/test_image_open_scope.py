#!/usr/bin/env python3
"""Exercise native image admission with production path scoping and launch bodies.

VM context, executor scheduling and AppDB are host adapters. The production
source validator, app path resolver, enqueue and launch task execute unchanged;
these tests do not claim filesystem reads, decoding or window presentation.
"""

from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def item(path: str, signature: str) -> str:
    source = (ROOT / path).read_text()
    start = source.index(signature)
    opening = source.index("{", start)
    depth, end = 1, opening + 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end] + "\n"


HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_vm;
extern crate self as trueos_crypto;
pub type AccountId = u64;
pub mod vmcall { __PAYLOAD_CONSTANTS__ }
use std::{cell::{Cell, RefCell}, collections::BTreeMap};

#[derive(Clone)]
struct Context { root: Option<String>, archive: String, scope: bool, account: Option<u64> }
thread_local! {
    static CURRENT: Cell<Option<u8>> = const { Cell::new(None) };
    static CONTEXTS: RefCell<BTreeMap<u8, Context>> = const { RefCell::new(BTreeMap::new()) };
    static LAUNCHES: RefCell<Vec<hv::launcher::LaunchRequest>> = const { RefCell::new(Vec::new()) };
}
fn configure(vm: u8, root: Option<&str>, archive: &str, scope: bool, account: Option<u64>) {
    CONTEXTS.with(|contexts| { contexts.borrow_mut().insert(vm, Context {
        root: root.map(String::from), archive: archive.into(), scope, account,
    }); });
}
fn current() -> Option<Context> {
    CURRENT.with(|vm| CONTEXTS.with(|contexts| contexts.borrow().get(&vm.get()?).cloned()))
}
fn reset() {
    CONTEXTS.with(|contexts| contexts.borrow_mut().clear());
    LAUNCHES.with(|launches| launches.borrow_mut().clear());
    CURRENT.with(|vm| vm.set(None));
    configure(0, Some("apps/codex"), "codex.bp", false, Some(17));
}
fn launched() -> Vec<hv::launcher::LaunchRequest> { LAUNCHES.with(|launches| launches.borrow().clone()) }
mod r {
    #[path = "__PATH_SOURCE__"] pub mod path;
    pub mod io {
      pub mod kfs {
        pub fn exists(_: &str) -> Result<bool, ()> { panic!("source admission must not inspect storage") }
      }
      pub mod env {
        use alloc::string::String;
        pub fn current_app_fs_root() -> Option<String> { crate::current()?.root }
        pub fn trueosfs_scope_granted() -> bool { crate::current().is_some_and(|context| context.scope) }
        fn var(key: &str) -> Option<String> {
            assert_eq!(key, "TRUEOS_APP_ARCHIVE");
            Some(crate::current()?.archive)
        }
        __RESOLVER__
    } }
}
mod hv {
    pub fn with_guest_broker_context<T>(vm: u8, body: impl FnOnce() -> T) -> T {
        let previous = crate::CURRENT.with(|current| current.replace(Some(vm)));
        let result = body();
        crate::CURRENT.with(|current| current.set(previous));
        result
    }
    pub fn blueprint_console_target(_: u8) -> Option<crate::shell3::MatrixTarget> { None }
    pub mod blueprint {
        use alloc::string::String;
        __APP_ROOT__
    }
    pub mod launcher {
        #[derive(Clone, Debug)]
        pub struct LaunchRequest {
            pub archive: String, pub args: Vec<String>, pub access_account: Option<u64>, pub script: Option<String>,
        }
        impl LaunchRequest {
            pub fn new(archive: String, _: Vec<u8>) -> Self {
                Self { archive, args: vec![], access_account: None, script: None }
            }
        }
        pub fn enqueue(request: LaunchRequest, _: Option<()>) -> Result<(), String> {
            crate::LAUNCHES.with(|launches| launches.borrow_mut().push(request)); Ok(())
        }
    }
}
mod access_key_db {
    pub fn principal_for_vm(vm: u8) -> Option<u64> {
        crate::CONTEXTS.with(|contexts| contexts.borrow().get(&vm).and_then(|context| context.account))
    }
}
mod shell3 {
    pub struct MatrixTarget;
    pub fn matrix_target_print_line(_: &MatrixTarget, _: &str) {}
}
mod app_db {
    pub fn get(archive: &str) -> Result<Option<Vec<u8>>, String> {
        assert_eq!(archive, "img.bp"); Ok(Some(vec![1]))
    }
}
mod log_os { pub fn blueprint_important_line(_: core::fmt::Arguments<'_>) {} }
mod workers {
    pub struct Worker;
    pub fn pick_background_spawner() -> Option<Worker> { Some(Worker) }
    impl Worker {
        pub fn spawn(self, future: impl core::future::Future<Output = ()>) {
            let mut future = std::pin::pin!(future);
            let mut context = core::task::Context::from_waker(core::task::Waker::noop());
            assert!(future.as_mut().poll(&mut context).is_ready());
        }
    }
}
mod service {
    use alloc::{string::String, vec::Vec};
    __SERVICE__
    fn launch_task(paths: Vec<String>, target: Option<crate::shell3::MatrixTarget>, account: Option<u64>)
        -> Result<impl core::future::Future<Output = ()>, ()>
    { Ok(launch_body(paths, target, account)) }
}

#[test] fn ordinary_codex_can_present_its_own_cached_image_without_global_scope() {
    reset();
    assert_eq!(service::enqueue(0, b"/apps/codex/.codex/cache/images/map.png"), 0);
    let requests = launched();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].args, ["apps/codex/.codex/cache/images/map.png"]);
    assert_eq!(requests[0].archive, "img.bp");
    assert_eq!(requests[0].access_account, Some(17));
    assert_eq!(requests[0].script.as_deref(), Some("fs-scope trueosfs\n"));
    hv::with_guest_broker_context(0, || assert!(!r::io::env::trueosfs_scope_granted()));
    assert!(CURRENT.with(Cell::get).is_none());
}
#[test] fn common_aliases_preserve_spaces_unicode_and_multiple_paths() {
    reset();
    assert_eq!(service::enqueue(0, "/apps/common/Manhattan map.png\0common/東京 bird.jpg".as_bytes()), 0);
    assert_eq!(launched()[0].args, ["apps/common/Manhattan map.png", "apps/common/東京 bird.jpg"]);
}
#[test] fn other_app_and_prefix_lookalike_paths_stay_inside_the_callers_view() {
    reset();
    assert_eq!(service::enqueue(0, b"/apps/hull/private.png\0/apps/codex-other/private.png"), 0);
    let requests = launched();
    assert_eq!(requests[0].args[0], "apps/codex/apps/hull/private.png");
    assert!(requests[0].args.iter().all(|path| path.starts_with("apps/codex/")));
    assert!(!requests[0].args.iter().any(|path| path.starts_with("apps/codex-other/")));
}
#[test] fn flat_container_paths_and_accounts_are_not_taken_from_another_caller() {
    reset();
    configure(1, Some("apps/codex_worker"), "codex.bp", false, Some(23));
    assert_eq!(service::enqueue(1, b".codex/cache/map.png"), 0);
    assert_eq!(service::enqueue(0, b".codex/cache/map.png"), 0);
    let requests = launched();
    assert_eq!(requests[0].args, ["apps/codex_worker/.codex/cache/map.png"]);
    assert_eq!(requests[0].access_account, Some(23));
    assert_eq!(requests[1].args, ["apps/codex/.codex/cache/map.png"]);
    assert_eq!(requests[1].access_account, Some(17));
}
#[test] fn malformed_or_escaping_payloads_never_reach_launch() {
    reset();
    for payload in [b"".as_slice(), b"\xff", b"\0", b"image.png\0", b"../private.png", b"common/../private.png", b"/", b"."] {
        assert_eq!(service::enqueue(0, payload), -1, "payload={payload:?}");
    }
    assert_eq!(service::enqueue(0, &vec![b'x'; vmcall::PAYLOAD_CAP + 1]), -1);
    let thirty_three = vec!["map.png"; 33].join("\0");
    assert_eq!(service::enqueue(0, thirty_three.as_bytes()), -1);
    assert!(launched().is_empty());
}
#[test] fn full_scope_still_normalizes_paths_and_unrooted_callers_fail_closed() {
    reset();
    configure(1, None, "other.bp", false, Some(23));
    assert_eq!(service::enqueue(1, b"/apps/codex/private.png"), -13);
    assert_eq!(service::enqueue(99, b"/apps/common/map.png"), -13);
    assert!(launched().is_empty());
    configure(1, None, "other.bp", true, Some(23));
    assert_eq!(service::enqueue(1, b"/maps/./Manhattan.png"), 0);
    assert_eq!(launched()[0].args, ["maps/Manhattan.png"]);
    assert_eq!(launched()[0].access_account, Some(23));
}
'''


def main() -> None:
    vmcall = (ROOT / "crates/trueos-vm/src/vmcall.rs").read_text()
    constants = "\n".join(
        re.search(rf"pub const {name}: usize = [^;]+;", vmcall)[0]
        for name in ("COMM_PAGE_BYTES", "PAYLOAD_CAP")
    )
    source = HARNESS.replace("__PAYLOAD_CONSTANTS__", constants)
    source = source.replace("__PATH_SOURCE__", str(ROOT / "src/r/path.rs"))
    source = source.replace("__RESOLVER__", "\n".join(
        item("src/r/io.rs", signature)
        for signature in ("fn normalize_app_path(", "pub(crate) fn resolve_fs_path(")
    ))
    source = source.replace("__APP_ROOT__", "\n".join(
        item("src/hv/blueprint/blueprint.rs", signature)
        for signature in ("fn safe_archive_stem(", "pub(crate) fn app_fs_root_for_archive(")
    ))
    source = source.replace("__SERVICE__", "\n".join(
        item("src/r/services/image_open_service.rs", signature)
        for signature in ("fn sources(", "pub(crate) fn enqueue(", "async fn launch_task(")
    ).replace("async fn launch_task(", "async fn launch_body(", 1))
    with tempfile.TemporaryDirectory(prefix="trueos-image-open-scope-") as temporary:
        directory = Path(temporary)
        harness = directory / "tests.rs"
        harness.write_text(source)
        for profile, flags in (("debug", []), ("optimized", ["-O"])):
            executable = directory / profile
            subprocess.run(["rustc", "--edition=2024", "--test", str(harness), *flags, "-o", str(executable)], check=True)
            print(f"image-open-scope profile={profile}", flush=True)
            subprocess.run([str(executable), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
