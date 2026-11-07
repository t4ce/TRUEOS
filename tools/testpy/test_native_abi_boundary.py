#!/usr/bin/env python3
"""Exercise retired imports through production prebind and loader guards.

Only address providers and the unrelated unresolved-import fallback are modeled.
Retirement policy and the loader's decision order are production Rust bodies.
"""
from pathlib import Path
import json
import re
import subprocess
import tempfile

from test_thread_control_abi import function

ROOT = Path(__file__).resolve().parents[2]


def main():
    unix = (ROOT / "src/unix_compat.rs").read_text()
    prebind = (ROOT / "src/hv/blueprint/prebind.rs").read_text()
    loader = (ROOT / "src/hv/blueprint/blueprint.rs").read_text()
    classifier = unix.split("pub(crate) fn is_unix_import", 1)[1].split(
        "pub(crate) fn resolve_import", 1)[0]
    names = sorted(set(re.findall(r'"([^"\n]+)"', classifier)))
    resolver = unix.split("pub(crate) fn resolve_import", 1)[1]
    helpers = sorted(set(re.findall(r"crate::blueprint_shims::(\w+) as", resolver)))
    ctype = sorted(set(re.findall(r"crate::blueprint_shims::ctype::(\w+) as", resolver)))
    policy = "\n".join(function(prebind, name) for name in (
        "retired_import_error", "prebind_import_error"))
    source = f'''extern crate alloc;
extern crate self as trueos_math;
pub fn log() {{}}
mod blueprint_shims {{
    {" ".join(f"pub fn {name}() {{}}" for name in helpers)}
    pub mod ctype {{ {" ".join(f"pub fn {name}() {{}}" for name in ctype)} }}
}}
#[path = {json.dumps(str(ROOT / "src/unix_compat.rs"))}] mod unix_compat;
mod prebind {{ {policy} }}
use std::sync::atomic::{{AtomicUsize, Ordering}};
static KNOWN: AtomicUsize = AtomicUsize::new(0);
static JOKER: AtomicUsize = AtomicUsize::new(0);
fn resolve_known_import(_: &str) -> Option<usize> {{
    KNOWN.fetch_add(1, Ordering::Relaxed); None
}}
fn resolve_unresolved_import(_: &str) -> Option<usize> {{
    JOKER.fetch_add(1, Ordering::Relaxed); Some(123)
}}
{function(loader, "resolve_import")}
fn main() {{
    let mut retired = 0;
    let mut freestanding = 0;
    for name in {json.dumps(names)} {{
        if unix_compat::resolve_import(name).is_some() {{
            assert!(prebind::prebind_import_error(name).is_none(), "{{name}}");
            freestanding += 1;
        }} else {{
            assert!(prebind::prebind_import_error(name).is_some(), "{{name}}");
            assert!(resolve_import(name).is_none(), "{{name}} reached a fallback");
            retired += 1;
        }}
    }}
    for name in ["malloc_usable_size", "exit", "_exit", "trueos_cabi_alloc",
                 "trueos_cabi_calloc", "trueos_cabi_free", "trueos_cabi_realloc",
                 "trueos_cabi_malloc_usable_size"] {{
        assert!(prebind::prebind_import_error(name).is_some(), "{{name}}");
        assert!(resolve_import(name).is_none(), "{{name}} reached a fallback");
    }}
    assert_eq!(KNOWN.load(Ordering::Relaxed), 0);
    assert_eq!(JOKER.load(Ordering::Relaxed), 0);
    for name in ["sys_alloc_aligned", "sys_dealloc", "trueos_cabi_heap_stats",
                 "trueos_platform_thread_local_get", "trueos_cabi_thread_spawn",
                 "trueos_platform_dns_resolve_ipv4"] {{
        assert!(prebind::prebind_import_error(name).is_none(), "{{name}}");
    }}
    assert_eq!(resolve_import("unrelated_optional_import"), Some(123));
    println!("{{retired}} historical imports retired; {{freestanding}} freestanding helpers retained; removed aliases cannot reach loader fallbacks");
}}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-native-abi-") as temporary:
        directory = Path(temporary)
        path = directory / "boundary.rs"
        binary = directory / "boundary"
        path.write_text(source)
        subprocess.run(["rustc", "--edition=2024", str(path), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
