#!/usr/bin/env python3
"""Check the production launch environment against production FS resolution.

Locale/network services and the selected launch context are mocked; unused
filesystem probes are disabled.
The VM path parser and app/common confinement code are compiled unchanged.
"""
from pathlib import Path
import re
import subprocess
import tempfile

from test_posix_renameat import function

ROOT = Path(__file__).resolve().parents[2]


def main():
    blueprint = ROOT / "src/hv/blueprint/blueprint.rs"
    builder = function(blueprint, "build_process_env")
    locale_functions = sorted(set(re.findall(r"crate::locale::(\w+)\(\)", builder)))
    locales = "\n".join(
        f'pub fn {name}() -> &\'static str {{ "test" }}' for name in locale_functions
    )
    harness = r'''
#![allow(dead_code)]
extern crate alloc;
use alloc::{collections::BTreeMap, string::String};
std::thread_local! {
    static ENV: std::cell::RefCell<BTreeMap<String, String>> = Default::default();
}
fn var(key: &str) -> Option<String> { ENV.with(|e| e.borrow().get(key).cloned()) }
fn current_app_fs_root() -> Option<String> { var("TRUEOS_APP_FS_ROOT") }
mod hv {
    pub struct BlueprintInstanceIdentity {
        pub instance: [u8; 16], pub lineage: [u8; 16], pub generation: u64,
        pub clone: bool, pub name: Option<String>, pub peer: Option<String>,
    }
    pub fn format_blueprint_uuid(_: &[u8; 16]) -> String { "uuid".into() }
}
mod net {
    pub mod adapter {
        pub fn get_hostname() -> String { "test".into() }
        pub fn ipv4_at(_: usize) -> Option<[u8; 4]> { None }
        pub fn primary_ipv4_router_snapshot() -> Option<[u8; 4]> { None }
    }
    pub fn primary_device_index() -> usize { 0 }
    pub fn device_count() -> usize { 1 }
}
mod r {
    pub use crate::path;
    pub mod io { pub mod kfs {
        pub fn exists(_: &str) -> Result<bool, ()> { panic!("unexpected storage probe") }
    } }
}

#[test]
fn userdata_stays_in_its_instance_even_with_broader_scope() {
    for root in ["apps/velosrv", "apps/velosrv/first--uuid", "apps/velosrv/second--uuid"] {
        for broad in [false, true] {
            let env = build_process_env("velosrv.bp", Some(root), None, None, broad);
            let data = env["VELOREN_USERDATA"].clone();
            assert_eq!(data, format!("/{root}/userdata"));
            assert_eq!(env.contains_key("TRUEOS_FS_SCOPE"), broad);
            ENV.with(|e| *e.borrow_mut() = env);
            assert_eq!(resolve_fs_path(&data, false), Some(format!("{root}/userdata")));
            assert_eq!(resolve_fs_path(&format!("{data}/server/saves"), false),
                       Some(format!("{root}/userdata/server/saves")));
        }
    }
}

#[test]
fn shared_userdata_override_uses_existing_common_scope() {
    let mut env = build_process_env("velosrv.bp", Some("apps/velosrv"), None, None, false);
    env.insert("VELOREN_USERDATA".into(), "common/veloren/userdata".into());
    let data = env["VELOREN_USERDATA"].clone();
    ENV.with(|e| *e.borrow_mut() = env);
    assert!(!trueosfs_scope_granted());
    assert_eq!(resolve_fs_path(&data, false), Some("apps/common/veloren/userdata".into()));
    assert_eq!(resolve_fs_path("../outside", false), None);
}

#[test]
fn bridge_is_specific_to_the_server_and_never_grants_global_fs() {
    for archive in ["velosrv", "VeLoSrV.bp"] {
        let env = build_process_env(archive, Some("apps/velosrv"), None, None, false);
        assert_eq!(env["VELOREN_USERDATA"], "/apps/velosrv/userdata");
        assert!(!env.contains_key("TRUEOS_FS_SCOPE"));
    }
    for archive in ["other.bp", "velosrv-helper.bp"] {
        assert!(!build_process_env(archive, Some("apps/other"), None, None, false)
            .contains_key("VELOREN_USERDATA"));
    }
}
'''
    production = "\n".join(function(blueprint, name) for name in (
        "build_process_env", "safe_archive_stem", "app_fs_common_root"
    ))
    production += "\n" + "\n".join(function(ROOT / "src/r/io.rs", name) for name in (
        "trueosfs_scope_granted", "normalize_app_path", "resolve_fs_path"
    ))
    harness += f'\nmod locale {{ {locales} }}\n'
    harness += f'#[path = "{ROOT / "src/r/path.rs"}"] pub mod path;\n'
    with tempfile.TemporaryDirectory(prefix="trueos-velosrv-env-") as directory:
        temp = Path(directory)
        source = temp / "test.rs"
        binary = temp / "test"
        source.write_text(harness + production)
        subprocess.run(["rustc", "--edition=2024", "--test", "--target",
                        "x86_64-unknown-linux-gnu", str(source), "-o", str(binary)],
                       check=True, cwd=ROOT)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
