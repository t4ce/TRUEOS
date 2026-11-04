#!/usr/bin/env python3
"""Exercise production container-0 and explicit flat app-root selection."""

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
            "app_fs_root_for_container_id",
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
    fn default_codex_stays_container_zero_and_named_launches_are_additional() {
        assert_eq!(app_fs_root_for_archive("codex.bp", &[]), "apps/codex");
        assert_eq!(
            app_fs_root_for_named_instance("codex.bp", "first", None, "guid-one"),
            "apps/codex_first--guid-one",
        );
        assert_eq!(
            app_fs_root_for_named_instance("codex.bp", "second", None, "guid-two"),
            "apps/codex_second--guid-two",
        );
        assert_eq!(
            app_fs_root_for_named_instance("codex.bp", "peer-name", Some("peer-a"), "guid"),
            "apps/codex_peer-peer-a--peer-name--guid",
        );
    }

    #[test]
    fn archive_normalization_and_codex_check_scope_match_production_rules() {
        assert_eq!(safe_archive_stem("  CODEX.bp  "), "codex");
        assert!(uses_volatile_codex_files("CODEX.bp"));
        assert_eq!(
            app_fs_root_for_named_instance("CODEX.bp", "named", None, "guid"),
            "apps/codex_named--guid",
        );

        assert_eq!(safe_archive_stem("Codex-7A8f.bp"), "codex-7a8f");
        assert!(!uses_volatile_codex_files("Codex-7A8f.bp"));

        assert_eq!(
            app_fs_root_for_named_instance("CODEX-CHECK.bp", "named", None, "guid"),
            "apps/codex-check_named--guid",
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
        assert_eq!(first, "apps/other_worker--guid-one");
        assert_eq!(second, "apps/other_worker--guid-two");
        assert_eq!(peer, "apps/other_peer-peer-a--worker--guid-one");
        assert_ne!(first, second);
        assert_ne!(first, peer);
    }

    #[test]
    fn explicit_container_selection_is_stable_and_zero_keeps_existing_paths() {
        for app in ["codex", "codex-check", "other"] {
            let archive = format!("{app}.bp");
            let default = app_fs_root_for_archive(&archive, &[]);
            assert_eq!(default, format!("apps/{app}"));
            assert_eq!(app_fs_root_for_container_id(&archive, "0").unwrap(), default);
            let first = app_fs_root_for_container_id(&archive, "1").unwrap();
            let reopened = app_fs_root_for_container_id(&archive, "1").unwrap();
            assert_eq!(first, format!("apps/{app}_1"));
            assert_eq!(first, reopened);
            assert_ne!(first, default);
            assert_ne!(first, app_fs_root_for_container_id(&archive, "2").unwrap());
        }
    }

    #[test]
    fn explicit_ids_cannot_escape_the_flat_container_component() {
        for id in ["", ".", "..", "a/b", "/1", "../1", "1/../2", "a b", "a\\b", "ß"] {
            assert!(app_fs_root_for_container_id("codex.bp", id).is_err(), "{id}");
        }
        assert!(app_fs_root_for_container_id("codex.bp", &"a".repeat(193)).is_err());
    }

    #[test]
    fn generated_named_and_peer_container_ids_can_be_selected_again() {
        let guid = "11111111-2222-4333-8444-555555555555";
        for (name, peer) in [
            ("-worker".to_owned(), None),
            ("n".repeat(48), Some("p".repeat(48))),
        ] {
            let root = app_fs_root_for_named_instance("codex.bp", &name, peer.as_deref(), guid);
            let id = root.strip_prefix("apps/codex_").unwrap();
            assert_eq!(app_fs_root_for_container_id("codex.bp", id).unwrap(), root);
        }
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
