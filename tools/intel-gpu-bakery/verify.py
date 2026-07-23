#!/usr/bin/env python3
"""Compiler-free verification of committed TRUEOS Intel GPU artifacts."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from artifact_contract import ContractError, verify_manifest


TOOL_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOL_DIR.parent.parent


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Reparse Zebin ELF/.ze_info and verify artifact/SPIR-V hashes, "
            "generated Rust, source/header hashes, ABI reference, and profile. "
            "No Clang, llvm-spirv, or IGC installation is required."
        )
    )
    inputs = parser.add_mutually_exclusive_group(required=True)
    inputs.add_argument("--artifact-dir", type=Path)
    inputs.add_argument("--bin", type=Path)
    parser.add_argument("--spv", type=Path)
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--contract", type=Path)
    return parser


def _verify_one(
    binary: Path,
    spirv: Path | None,
    manifest: Path | None,
    contract: Path | None,
) -> None:
    spirv = spirv or binary.with_suffix(".spv")
    manifest = manifest or binary.with_suffix(".manifest.json")
    contract = contract or binary.with_suffix(".contract.rs")
    for path in (binary, spirv, manifest, contract):
        if not path.is_file():
            raise ContractError(f"missing verification input: {path}")
    verify_manifest(manifest, binary, spirv, contract, REPO_ROOT)
    print(f"verified: {binary}")


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    if args.artifact_dir:
        if args.spv or args.manifest or args.contract:
            raise ContractError(
                "--spv/--manifest/--contract are only valid with a single --bin"
            )
        directory = args.artifact_dir.expanduser().resolve()
        binaries = sorted(directory.rglob("*.bin")) if directory.is_dir() else []
        if not binaries:
            raise ContractError(f"no .bin artifacts found under {directory}")
        for binary in binaries:
            _verify_one(binary, None, None, None)
        print(f"verified {len(binaries)} artifact(s)")
    else:
        assert args.bin is not None
        _verify_one(
            args.bin.expanduser().resolve(),
            args.spv.expanduser().resolve() if args.spv else None,
            args.manifest.expanduser().resolve() if args.manifest else None,
            args.contract.expanduser().resolve() if args.contract else None,
        )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ContractError as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2)
