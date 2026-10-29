#!/usr/bin/env python3
"""Exercise production archive path checks and launch admission with host context."""

import json
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def function(source: str, signature: str) -> str:
    start = source.index(signature)
    opening = source.index("{", start)
    depth = 1
    end = opening + 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


HARNESS = r'''#![allow(dead_code)]
extern crate alloc;
mod r {
    #[path = @PATH_SOURCE@] pub mod path;
    pub mod io {
        pub mod kfs { pub fn exists(_: &str) -> Result<bool, ()> { Ok(false) } }
        pub mod env {
            use alloc::string::String;
            use std::cell::RefCell;
            thread_local! {
                static CONTEXT: RefCell<(bool, Option<String>)> =
                    RefCell::new((false, Some("apps/edit".into())));
            }
            pub fn set(scope: bool, root: Option<&str>) {
                CONTEXT.with(|context| *context.borrow_mut() = (scope, root.map(String::from)));
            }
            fn trueosfs_scope_granted() -> bool { CONTEXT.with(|context| context.borrow().0) }
            fn current_app_fs_root() -> Option<String> { CONTEXT.with(|context| context.borrow().1.clone()) }
            @RESOLVERS@
        }
        pub mod fs_cabi {
            use alloc::string::String;
            #[path = @LAUNCH_SOURCE@] mod launch_request;
            #[derive(Debug)]
            pub struct Admission {
                pub archive: String,
                pub path: Option<String>,
                pub new_shell: bool,
            }
            fn launch_archive_task(
                _: Option<u32>, archive: String, path: Option<String>,
                _: Option<String>, destination: launch_request::Destination,
            ) -> Result<Admission, ()> {
                Ok(Admission { archive, path,
                    new_shell: destination == launch_request::Destination::NewShell3 })
            }
            @LAUNCH_REQUEST@
            pub fn submit(path: &str) -> i32 {
                launch_script_request(0, format!("{path}\0\0sh3").as_bytes())
            }
        }
        pub mod async_fs_cabi {
            pub fn install_path(bytes: &[u8]) -> Option<String> {
                @INSTALL_PATH@
            }
        }
    }
}
mod hv { pub fn blueprint_console_target(_: u8) -> Option<u32> { Some(1) } }
mod shell3 { pub mod tui {
    pub fn frontend_for_target(target: &u32) -> Option<u32> { Some(*target) }
} }
mod workers {
    use std::cell::RefCell;
    use crate::r::io::fs_cabi::Admission;
    thread_local! { static QUEUED: RefCell<Vec<Admission>> = const { RefCell::new(Vec::new()) }; }
    pub struct Worker;
    pub fn pick_background_spawner() -> Option<Worker> { Some(Worker) }
    impl Worker { pub fn spawn(self, admission: Admission) {
        QUEUED.with(|queued| queued.borrow_mut().push(admission));
    } }
    pub fn take() -> Vec<Admission> { QUEUED.with(|queued| std::mem::take(&mut *queued.borrow_mut())) }
}

#[test]
fn confined_edit_can_install_and_launch_shared_builds() {
    use r::io::{async_fs_cabi, env, fs_cabi};
    env::set(false, Some("apps/edit/instances/editor"));
    let expected = "apps/common/ide/builds/42-7/1/demo.bp";
    for path in [
        "/apps/common/ide/builds/42-7/1/demo.bp",
        "apps/common/ide/builds/42-7/1/demo.bp",
        "common/ide/builds/42-7/1/demo.bp",
        "/common/ide/builds/42-7/1/demo.bp",
    ] {
        assert_eq!(async_fs_cabi::install_path(path.as_bytes()).as_deref(), Some(expected));
        assert_eq!(fs_cabi::submit(path), 0);
        let queued = workers::take();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].path.as_deref(), Some(expected));
        assert_eq!(queued[0].archive, "demo.bp");
        assert!(queued[0].new_shell);
    }
}

#[test]
fn confined_archive_requests_cannot_leave_shared_common() {
    use r::io::{async_fs_cabi, env, fs_cabi};
    env::set(false, Some("apps/edit"));
    for path in [
        "/apps/other/private.bp", "private/out.bp", "./private.bp",
        "/apps/common-other/out.bp", "common/../other/out.bp",
        "trueosfs:disc7/apps/other/out.bp", "trueosfs:disc7/apps/common/out.bp",
    ] {
        assert!(async_fs_cabi::install_path(path.as_bytes()).is_none(), "{path}");
        assert_ne!(fs_cabi::submit(path), 0, "{path}");
        assert!(workers::take().is_empty());
    }
    assert!(async_fs_cabi::install_path(b"private.bp").is_none());
    assert!(async_fs_cabi::install_path(b"common/invalid\xff.bp").is_none());
}

#[test]
fn broad_scope_retains_existing_root_and_mount_access() {
    use r::io::{async_fs_cabi, env, fs_cabi};
    env::set(true, Some("apps/edit"));
    for (path, expected) in [
        ("/apps/other/private.bp", "apps/other/private.bp"),
        ("trueosfs:disc7/packages/demo.bp", "trueosfs:disc7/packages/demo.bp"),
    ] {
        assert_eq!(async_fs_cabi::install_path(path.as_bytes()).as_deref(), Some(expected));
        assert_eq!(fs_cabi::submit(path), 0);
        assert_eq!(workers::take()[0].path.as_deref(), Some(expected));
    }
    assert!(async_fs_cabi::install_path(b"/apps/common/../secret.bp").is_none());
}

#[test]
fn absent_app_root_does_not_bypass_traversal_validation() {
    use r::io::{async_fs_cabi, env, fs_cabi};
    env::set(false, None);
    assert!(async_fs_cabi::install_path(b"apps/common/../secret.bp").is_none());
    assert_ne!(fs_cabi::submit("apps/common/../secret.bp"), 0);
    assert!(workers::take().is_empty());
}

#[test]
fn appdb_names_still_launch_without_filesystem_scope() {
    r::io::env::set(false, Some("apps/edit"));
    assert_eq!(r::io::fs_cabi::submit("demo.bp"), 0);
    let queued = workers::take();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].archive, "demo.bp");
    assert!(queued[0].path.is_none());
}
'''


def main() -> None:
    io = (ROOT / "src/r/io.rs").read_text()
    resolvers = "\n".join(function(io, signature) for signature in [
        "fn normalize_app_path(", "pub(crate) fn resolve_fs_path(",
        "pub(crate) fn resolve_blueprint_archive_path(",
    ])
    fs = (ROOT / "src/r/io/fs_cabi.rs").read_text()
    asynchronous = (ROOT / "src/r/io/async_fs_cabi.rs").read_text()
    install = asynchronous[asynchronous.index('if path == "vFile:appdb-install" {'):]
    closure = function(install, "|| {")[4:-1]
    source = HARNESS.replace("@PATH_SOURCE@", json.dumps(str(ROOT / "src/r/path.rs")))
    source = source.replace("@LAUNCH_SOURCE@", json.dumps(str(ROOT / "src/r/io/launch_request.rs")))
    source = source.replace("@RESOLVERS@", resolvers)
    source = source.replace("@LAUNCH_REQUEST@", function(fs, "fn launch_script_request("))
    source = source.replace("@INSTALL_PATH@", closure)
    channel = re.search(r'^channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel, "repository Rust toolchain channel missing"
    with tempfile.TemporaryDirectory(prefix="trueos-archive-scope-") as temporary:
        code, binary = Path(temporary) / "harness.rs", Path(temporary) / "tests"
        code.write_text(source)
        subprocess.run(["rustc", "+" + channel.group(1), "--edition=2024", "--test",
                        "--target", "x86_64-unknown-linux-gnu", str(code), "-o", str(binary)],
                       cwd=ROOT, check=True)
        subprocess.run([str(binary)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
