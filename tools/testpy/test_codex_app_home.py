#!/usr/bin/env python3
"""Exercise the production app-root functions for stable Codex home paths."""

from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
SOURCE_PATH = "src/hv/blueprint/blueprint.rs"


def item(name: str) -> str:
    source = (ROOT / SOURCE_PATH).read_text()
    declarations = list(
        re.finditer(
            rf"^(?:pub(?:\([^)]*\))?\s+)?fn\s+{re.escape(name)}\b",
            source,
            re.MULTILINE,
        )
    )
    if len(declarations) != 1:
        raise ValueError(f"{SOURCE_PATH}: expected one item {name}, got {len(declarations)}")
    start = declarations[0].start()
    ending = re.search(r"^}\s*\n", source[declarations[0].end():], re.MULTILINE)
    if not ending:
        raise ValueError(f"{SOURCE_PATH}: no top-level closing brace for {name}")
    end = declarations[0].end() + ending.end()
    return source[start:end]


def harness_source() -> str:
    functions = "\n".join(
        item(name)
        for name in (
            "safe_archive_stem",
            "uses_volatile_codex_files",
            "app_fs_root_for_archive",
            "safe_instance_component",
            "app_fs_root_for_named_instance",
        )
    )
    return r'''#![allow(dead_code)]
extern crate alloc;
use alloc::string::String;

''' + functions + r'''

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_named_codex_launches_share_the_app_home() {
        assert_eq!(app_fs_root_for_archive("codex.bp", &[]), "apps/codex");
        assert_eq!(
            app_fs_root_for_named_instance("codex.bp", "first", None, "guid-one"),
            "apps/codex",
        );
        assert_eq!(
            app_fs_root_for_named_instance("codex.bp", "second", None, "guid-two"),
            "apps/codex",
        );
        assert_eq!(
            app_fs_root_for_named_instance("codex.bp", "peer-name", Some("peer-a"), "guid"),
            "apps/codex",
        );
    }

    #[test]
    fn archive_normalization_and_codex_check_scope_match_production_rules() {
        assert_eq!(safe_archive_stem("  CODEX.bp  "), "codex");
        assert!(uses_volatile_codex_files("CODEX.bp"));
        assert_eq!(
            app_fs_root_for_named_instance("CODEX.bp", "named", None, "guid"),
            "apps/codex",
        );

        assert_eq!(safe_archive_stem("Codex-7A8f.bp"), "codex-7a8f");
        assert!(!uses_volatile_codex_files("Codex-7A8f.bp"));

        assert_eq!(
            app_fs_root_for_named_instance("CODEX-CHECK.bp", "named", None, "guid"),
            "apps/codex-check",
        );
        assert_ne!(
            app_fs_root_for_archive("codex.bp", &[]),
            app_fs_root_for_archive("codex-check.bp", &[]),
        );
    }

    #[test]
    fn non_codex_named_roots_remain_instance_scoped() {
        let first = app_fs_root_for_named_instance("other.bp", "worker", None, "guid-one");
        let second = app_fs_root_for_named_instance("other.bp", "worker", None, "guid-two");
        let peer = app_fs_root_for_named_instance("other.bp", "worker", Some("peer-a"), "guid-one");
        assert_eq!(first, "apps/other/worker--guid-one");
        assert_eq!(second, "apps/other/worker--guid-two");
        assert_eq!(peer, "apps/other/peer-peer-a--worker--guid-one");
        assert_ne!(first, second);
        assert_ne!(first, peer);
    }
}
'''


def run_contracts() -> None:
    source = harness_source()
    with tempfile.TemporaryDirectory(prefix="trueos-codex-app-home-") as temporary:
        directory = Path(temporary)
        rust_file = directory / "tests.rs"
        executable = directory / "tests"
        rust_file.write_text(source)
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(rust_file), "-o", str(executable)],
            check=True,
        )
        subprocess.run([str(executable)], check=True)


if __name__ == "__main__":
    run_contracts()
