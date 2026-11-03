#!/usr/bin/env python3
"""Host-check CABI parsing across safe-import syntax without changing the lock."""

from hashlib import sha256
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
ABI = ROOT / "crates/trueos-v/src/bp_abi.rs"
BUILD_RS = ROOT / "build.rs"
LOCK = ROOT / "abi/portal-cabi-v2.sha256"


def extract_function(source: str, name: str, next_name: str) -> str:
    start = source.index(f"fn {name}(")
    end = source.index(f"fn {next_name}(", start)
    return source[start:end]


def contract_digest(contracts: dict[str, str]) -> str:
    digest = sha256()
    for symbol, contract in sorted(contracts.items()):
        symbol_bytes = symbol.encode()
        contract_bytes = contract.encode()
        digest.update(len(symbol_bytes).to_bytes(8, "little"))
        digest.update(symbol_bytes)
        digest.update(len(contract_bytes).to_bytes(8, "little"))
        digest.update(contract_bytes)
    return digest.hexdigest()


def main() -> None:
    build = BUILD_RS.read_text()
    parser_parts = [
        extract_function(build, "parse_cabi_contracts", "matching_paren"),
        extract_function(build, "matching_paren", "split_top_level"),
        extract_function(build, "split_top_level", "top_level_colon"),
        extract_function(build, "top_level_colon", "normalize_tokens"),
        extract_function(build, "normalize_tokens", "aggregate_cabi_contract_digest"),
        extract_function(build, "render_portal_import_resolver", "parse_declared_cabi_contracts"),
    ]
    old_source = subprocess.run(
        ["git", "show", "HEAD:crates/trueos-v/src/bp_abi.rs"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    current_source = ABI.read_text()
    with tempfile.TemporaryDirectory(prefix="trueos-cabi-parser-") as temporary:
        directory = Path(temporary)
        old_path = directory / "head_bp_abi.rs"
        current_path = directory / "current_bp_abi.rs"
        harness_path = directory / "harness.rs"
        binary_path = directory / "cabi_parser_test"
        old_path.write_text(old_source)
        current_path.write_text(current_source)
        parser = "\n\n".join(parser_parts)
        harness_path.write_text(
            f'''use std::collections::BTreeMap;
use std::fs;

{parser}

fn main() {{
    let old_source = fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap();
    let current_source = fs::read_to_string(std::env::args().nth(2).unwrap()).unwrap();
    let old = parse_cabi_contracts(&old_source).unwrap();
    let current = parse_cabi_contracts(&current_source).unwrap();
    assert_eq!(old, current, "safe qualifiers must not alter ABI contracts");

    let mutated_source = current_source.replace(
        "pub safe fn trueos_cabi_dobby_ui4_focus(window_id: u64)",
        "pub safe fn trueos_cabi_dobby_ui4_focus(window_id: u32)",
    );
    assert_ne!(mutated_source, current_source, "test signature must exist");
    let mutated = parse_cabi_contracts(&mutated_source).unwrap();
    assert_ne!(current["trueos_cabi_dobby_ui4_focus"], mutated["trueos_cabi_dobby_ui4_focus"]);
    assert_ne!(current, mutated, "ABI type changes must change the contract map");

    let exports = current
        .keys()
        .map(|name| (name.clone(), format!("crate::mock::{name}")))
        .collect::<BTreeMap<_, _>>();
    let names = current.keys().cloned().collect::<Vec<_>>();
    let (resolver, unresolved) = render_portal_import_resolver(&names, &exports, true);
    assert!(unresolved.is_empty());
    for name in [
        "trueos_cabi_dobby_ui4_focus",
        "trueos_cabi_thread_current_id",
        "trueos_cabi_ui4_solara_frame_begin",
        "trueos_cabi_vmedia_image_decode_commit",
    ] {{
        assert!(resolver.contains(&format!("\\\"{name}\\\" => Some(")), "missing generated resolver arm: {{name}}");
    }}
    for (name, contract) in current {{
        println!("{{name}}\\t{{contract}}");
    }}
}}
'''
        )
        subprocess.run(
            ["rustc", "--edition=2024", str(harness_path), "-o", str(binary_path)],
            check=True,
        )
        output = subprocess.run(
            [str(binary_path), str(old_path), str(current_path)],
            check=True,
            capture_output=True,
            text=True,
        ).stdout

    contracts = dict(line.split("\t", 1) for line in output.splitlines())
    declared_names = set(
        re.findall(r"\bpub\s+(?:(?:safe|unsafe)\s+)?fn\s+(trueos_cabi_\w+)\s*\(", current_source)
    )
    assert set(contracts) == declared_names, "parser must retain every safe and unsafe ABI import"
    locked = next(line.strip() for line in LOCK.read_text().splitlines() if line.strip() and not line.lstrip().startswith("#"))
    assert contract_digest(contracts) == locked, "existing ABI digest must remain unchanged"
    print(f"verified {len(contracts)} ABI contracts; SHA-256 matches the existing lock")


if __name__ == "__main__":
    main()
