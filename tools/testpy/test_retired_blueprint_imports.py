#!/usr/bin/env python3
"""Exercise production prebind/loader rejection and compiler runtime aliases."""
from pathlib import Path
import json
import subprocess
import tempfile

from test_posix_renameat import function

ROOT = Path(__file__).resolve().parents[2]


def main():
    evidence = json.loads((ROOT / 'tools/docs/unused-export-removal-2026-10-22.json').read_text())
    loader = ROOT / 'src/hv/blueprint/blueprint.rs'
    prebind = ROOT / 'src/hv/blueprint/prebind.rs'
    retired = ','.join(json.dumps(n) for n in evidence['removed'])
    aliases = ','.join(json.dumps(n) for n in evidence['redundant_aliases'])
    # The literal resolver's dependencies are mocked; the production prefix
    # still decides how hash-dependent compiler imports bind to runtime hooks.
    prefix = function(loader, 'resolve_known_import').split('\n    match name {', 1)[0]
    prefix = prefix.replace('fn resolve_known_import', 'pub(super) fn resolve_known_import', 1)
    runtime = prefix + '\n    None\n}\n'
    hooks = ['portal_core_panic_bounds_check', 'portal_rust_alloc_error_handler',
             'portal_no_alloc_shim_is_unstable_v2', 'portal_rust_alloc',
             'portal_rust_dealloc', 'portal_rust_realloc', 'portal_rust_alloc_zeroed']
    runtime += '\n'.join('pub(super) fn ' + hook + '() {}' for hook in hooks)
    runtime += '\nfn is_core_panic_bounds_check_import(_: &str) -> bool { false }\n'
    production = ('mod prebind {\n' + function(prebind, 'retired_import_error')
                  + function(prebind, 'prebind_import_error') + '}\n'
                  + 'mod aliases {\n' + runtime + '}\n'
                  + function(loader, 'resolve_import'))
    harness = r'''
#![allow(dead_code)]
mod unix_compat {
    pub fn unsupported_unix_import_reason(name: &str) -> Option<&'static str> {
        if name == "pthread_create" { Some("unsupported") } else { None }
    }
}
std::thread_local! { static FALLBACKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
fn resolve_known_import(name: &str) -> Option<usize> {
    if name == "memcpy" { Some(123) } else { aliases::resolve_known_import(name) }
}
fn resolve_unresolved_import(_: &str) -> Option<usize> {
    FALLBACKS.with(|n| n.set(n.get() + 1));
    Some(999)
}
PRODUCTION
const RETIRED: &[&str] = &[RETIRED_NAMES];
const OLD_ALIASES: &[&str] = &[ALIAS_NAMES];
#[test]
fn retired_imports_fail_prebind_and_never_reach_the_return_zero_fallback() {
    assert_eq!(RETIRED.len(), 25);
    FALLBACKS.with(|n| n.set(0));
    for name in RETIRED {
        let reason = prebind::prebind_import_error(name).expect(name);
        assert!(reason.contains("retired"), "{name}: {reason}");
        assert_eq!(resolve_import(name), None, "{name}");
    }
    assert_eq!(FALLBACKS.with(|n| n.get()), 0);
}
#[test]
fn live_imports_and_existing_unsupported_unix_policy_keep_their_behavior() {
    FALLBACKS.with(|n| n.set(0));
    assert_eq!(resolve_import("memcpy"), Some(123));
    assert_eq!(resolve_import("pthread_create"), None);
    for name in ["trueos_cabi_async_fs_metadata_start_v2", "trueos_service_lane_submit_job",
                 "trueos_tokio_platform_wait_after", "trueos_cabi_vgpu_scalar_f32_v1",
                 "__memmove_chk", "__memset_chk", "fcntl64"] {
        assert!(prebind::prebind_import_error(name).is_none(), "{name}");
    }
    assert_eq!(resolve_import("unknown_test_import"), Some(999));
    assert_eq!(FALLBACKS.with(|n| n.get()), 1);
}
#[test]
fn removed_literal_aliases_still_resolve_through_the_compiler_independent_rules() {
    assert_eq!(OLD_ALIASES.len(), 11);
    for name in OLD_ALIASES {
        assert!(aliases::resolve_known_import(name).is_some(), "{name}");
    }
    for (suffix, target) in [
        ("___rustc12___rust_alloc", aliases::portal_rust_alloc as *const () as usize),
        ("___rustc14___rust_dealloc", aliases::portal_rust_dealloc as *const () as usize),
        ("___rustc14___rust_realloc", aliases::portal_rust_realloc as *const () as usize),
        ("___rustc19___rust_alloc_zeroed", aliases::portal_rust_alloc_zeroed as *const () as usize),
        ("___rustc26___rust_alloc_error_handler", aliases::portal_rust_alloc_error_handler as *const () as usize),
        ("___rustc35___rust_no_alloc_shim_is_unstable_v2", aliases::portal_no_alloc_shim_is_unstable_v2 as *const () as usize),
    ] {
        assert_eq!(aliases::resolve_known_import(&format!("_RNvNewToolchainHash_7{suffix}")), Some(target));
    }
}
'''
    harness = harness.replace('PRODUCTION', production).replace('RETIRED_NAMES', retired).replace('ALIAS_NAMES', aliases)
    with tempfile.TemporaryDirectory(prefix='trueos-retired-imports-') as directory:
        source = Path(directory) / 'test.rs'
        binary = Path(directory) / 'test'
        source.write_text(harness)
        subprocess.run(['rustc', '--edition=2024', '--test', str(source), '-o', str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    main()
