#!/usr/bin/env python3
"""Exercise production filesystem scoping for flat app container roots."""

from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def item(path, signature):
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
mod hv { pub mod blueprint {
    use alloc::string::String;
    __APP_FUNCTIONS__
} }
mod r {
    #[path = "__PATH_SOURCE__"] pub mod path;
    pub mod io {
      pub mod kfs {
        pub fn exists(_: &str) -> Result<bool, ()> { panic!("path normalization must not inspect storage") }
      }
      pub mod env {
        use alloc::string::String;
        static ROOT: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
        static ARCHIVE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
        static SCOPE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        fn current_app_fs_root() -> Option<String> { ROOT.lock().unwrap().clone() }
        fn var(key: &str) -> Option<String> {
            assert_eq!(key, "TRUEOS_APP_ARCHIVE");
            ARCHIVE.lock().unwrap().clone()
        }
        fn trueosfs_scope_granted() -> bool { SCOPE.load(std::sync::atomic::Ordering::Relaxed) }
        pub fn context(root: &str, archive: Option<&str>, scope: bool) {
            *ROOT.lock().unwrap() = Some(root.into());
            *ARCHIVE.lock().unwrap() = archive.map(String::from);
            SCOPE.store(scope, std::sync::atomic::Ordering::Relaxed);
        }
        __PATH_FUNCTIONS__
    } }
}
use r::io::env::{context, resolve_fs_path};

#[test] fn default_and_flat_containers_share_only_the_owning_apps_export() {
    for root in ["apps/voxy", "apps/voxy_1", "apps/voxy_worker--guid"] {
        context(root, Some("voxy.bp"), false);
        assert_eq!(resolve_fs_path("/apps/voxy/userdata/maps", false).as_deref(), Some("apps/voxy/userdata/maps"));
        assert_eq!(resolve_fs_path("userdata/maps", false), Some(format!("{root}/userdata/maps")));
    }
}

#[test] fn underscore_app_names_come_from_archive_identity() {
    context("apps/my_app_id_two", Some("My_App.bp"), false);
    assert_eq!(resolve_fs_path("/apps/my_app/userdata", false).as_deref(), Some("apps/my_app/userdata"));
    assert_eq!(resolve_fs_path("/apps/my/userdata", false).as_deref(), Some("apps/my_app_id_two/apps/my/userdata"));
    assert_eq!(resolve_fs_path("/apps/my_app/userdata-backup", false).as_deref(), Some("apps/my_app_id_two/apps/my_app/userdata-backup"));
}

#[test] fn missing_archive_never_guesses_an_app_from_its_flat_directory() {
    context("apps/voxy_1", None, false);
    assert_eq!(resolve_fs_path("/apps/voxy/userdata", false).as_deref(), Some("apps/voxy_1/apps/voxy/userdata"));
    assert_eq!(resolve_fs_path("/apps/voxy_1/userdata", false).as_deref(), Some("apps/voxy_1/userdata"));
}

#[test] fn common_screenshots_and_parent_rejection_keep_existing_behavior() {
    context("apps/voxy_1", Some("voxy.bp"), false);
    assert_eq!(resolve_fs_path("common/model.bin", false).as_deref(), Some("apps/common/model.bin"));
    assert_eq!(resolve_fs_path("/apps/common/model.bin", false).as_deref(), Some("apps/common/model.bin"));
    assert_eq!(resolve_fs_path("/screenshots/final.png", false).as_deref(), Some("screenshots/final.png"));
    assert_eq!(resolve_fs_path("/apps/voxy/userdata/../secret", false), None);
    assert_eq!(resolve_fs_path("/apps/other/userdata", false).as_deref(), Some("apps/voxy_1/apps/other/userdata"));
}

#[test] fn explicit_volume_scope_retains_its_path_behavior() {
    context("apps/voxy_1", Some("voxy.bp"), true);
    assert_eq!(resolve_fs_path("/apps/other/./file", false).as_deref(), Some("apps/other/file"));
    assert_eq!(resolve_fs_path("../file", false), None);
}

#[test] fn delegated_sqlite_view_reuses_only_the_private_service_root() {
    let root = "apps/codex/.codex/sqlite-service/codex";
    context(root, Some("sqlite.bp"), false);
    assert_eq!(resolve_fs_path("/apps/codex/.codex/sqlite-service/codex/db-123.sqlite", false), Some(format!("{root}/db-123.sqlite")));
    assert_eq!(resolve_fs_path("db-123.sqlite", false), Some(format!("{root}/db-123.sqlite")));
    assert_eq!(resolve_fs_path("/common/sqlite-service/codex", false).as_deref(), Some("apps/common/sqlite-service/codex"));
    assert_eq!(resolve_fs_path("/apps/common/sqlite-service/codex", false).as_deref(), Some("apps/common/sqlite-service/codex"));
    assert_eq!(resolve_fs_path("/apps/codex/.codex/auth.json", false), Some(format!("{root}/apps/codex/.codex/auth.json")));
    assert_eq!(resolve_fs_path("../auth.json", false), None);
    assert_eq!(resolve_fs_path("/apps/codex/.codex/sqlite-service/codex/../auth.json", false), None);
}
'''


def main():
    source = HARNESS.replace("__APP_FUNCTIONS__", "\n".join(
        item("src/hv/blueprint/blueprint.rs", signature)
        for signature in ("fn safe_archive_stem(", "pub(crate) fn app_fs_root_for_archive(")
    )).replace("__PATH_FUNCTIONS__", "\n".join(
        item("src/r/io.rs", signature)
        for signature in ("fn normalize_app_path(", "pub(crate) fn resolve_fs_path(")
    )).replace("__PATH_SOURCE__", str(ROOT / "src/r/path.rs"))
    with tempfile.TemporaryDirectory(prefix="trueos-flat-container-paths-") as temporary:
        directory = Path(temporary)
        harness, executable = directory / "tests.rs", directory / "tests"
        harness.write_text(source)
        subprocess.run(["rustc", "--edition=2024", "--test", str(harness), "-o", str(executable)], check=True)
        subprocess.run([str(executable), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
