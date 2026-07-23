#!/usr/bin/env python3
"""Generate a TRUEOS ABI contract from an existing Zebin without compilers."""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

from artifact_contract import (
    ContractError,
    analyze_zebin,
    atomic_write,
    build_manifest,
    compare_abi,
    input_records,
    render_rust_contracts,
    repo_relative,
    sha256_file,
    stable_json,
    validate_constraints,
)


TOOL_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOL_DIR.parent.parent
DEFAULT_PROFILE = TOOL_DIR / "profiles" / "adls.json"


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Inspect an existing .bin/.spv pair and generate manifest + no_std "
            "Rust contract without Clang/IGC. Intended for migration of legacy "
            "checked-in artifacts; it does not invent compiler provenance."
        )
    )
    parser.add_argument("--bin", type=Path, required=True)
    parser.add_argument("--spv", type=Path, required=True)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--profile", type=Path, default=DEFAULT_PROFILE)
    parser.add_argument("--variant", default="legacy")
    parser.add_argument("--output-dir", type=Path)
    parser.add_argument("--abi-reference-bin", type=Path)
    parser.add_argument("--expect-kernel", action="append", default=[])
    parser.add_argument("--rust-symbol", action="append", default=[])
    return parser


def _symbols(values: list[str]) -> dict[str, str]:
    result = {}
    for value in values:
        kernel, separator, symbol = value.partition("=")
        if (
            not separator
            or not kernel
            or not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", symbol)
        ):
            raise ContractError(f"invalid --rust-symbol {value!r}")
        result[kernel] = symbol
    return result


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    binary = args.bin.expanduser().resolve()
    spirv = args.spv.expanduser().resolve()
    source = args.source.expanduser().resolve()
    profile_path = args.profile.expanduser().resolve()
    for path in (binary, spirv, source, profile_path):
        if not path.is_file():
            raise ContractError(f"missing input: {path}")
    profile = json.loads(profile_path.read_text(encoding="utf-8"))
    analysis = analyze_zebin(binary, spirv)
    constraints = profile["constraints"]
    validate_constraints(
        analysis,
        int(constraints["simd_width"]),
        int(constraints["max_scratch_bytes"]),
        int(constraints["max_slm_bytes"]),
    )
    names = sorted(kernel["kernel_name"] for kernel in analysis["kernels"])
    if args.expect_kernel and names != sorted(args.expect_kernel):
        raise ContractError(
            f"kernel set mismatch: expected={sorted(args.expect_kernel)} actual={names}"
        )
    reference_record = None
    if args.abi_reference_bin:
        reference = args.abi_reference_bin.expanduser().resolve()
        compare_abi(analysis, analyze_zebin(reference), reference)
        reference_record = {
            "path": repo_relative(reference, REPO_ROOT),
            "sha256": sha256_file(reference),
            "result": "exact-match",
        }
    manifest = build_manifest(
        analysis=analysis,
        source={
            "path": repo_relative(source, REPO_ROOT),
            "language": "OpenCL C" if source.suffix == ".cl" else "unknown",
            # Compiler-free migration can prove the source itself. If a source
            # grows includes, rebake with Clang depfile capture instead.
            "inputs": input_records([source], REPO_ROOT),
        },
        target={
            "label": profile["label"],
            "ocloc_device": str(profile["device"]),
            "pci_device_ids": [
                int(value, 0) if isinstance(value, str) else int(value)
                for value in profile["pci_device_ids"]
            ],
            "revision_min": int(profile["revision_min"]),
            "revision_max": int(profile["revision_max"]),
        },
        variant=args.variant,
        provenance={
            "frontend": {
                "description": "pre-existing artifact; frontend provenance unavailable"
            },
            "backend": {
                "description": "pre-existing Intel Zebin; compiler provenance unavailable"
            },
            "commands": [],
            "toolchain": {"status": "unavailable-for-pre-existing-artifact"},
            "profile": {
                "path": repo_relative(profile_path, REPO_ROOT),
                "sha256": sha256_file(profile_path),
            },
            "reproducibility_check": "not-available-for-pre-existing-artifact",
        },
        abi_reference=reference_record,
        rust_symbols=_symbols(args.rust_symbol),
    )
    output_dir = (
        args.output_dir.expanduser().resolve() if args.output_dir else binary.parent
    )
    manifest_path = output_dir / f"{binary.stem}.manifest.json"
    contract_path = output_dir / f"{binary.stem}.contract.rs"
    atomic_write(manifest_path, stable_json(manifest))
    atomic_write(contract_path, render_rust_contracts(manifest))
    print(f"generated: {manifest_path}")
    print(f"generated: {contract_path}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ContractError, KeyError, json.JSONDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2)
