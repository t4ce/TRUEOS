#!/usr/bin/env python3
"""Verify published AArch64 CPU-kernel objects without compiler tools."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from artifact_contract import ContractError, verify_manifest


TOOL_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOL_DIR.parent.parent


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact-dir", type=Path, required=True)
    args = parser.parse_args()
    artifact_dir = args.artifact_dir.expanduser().resolve()
    manifests = sorted(artifact_dir.glob("*.manifest.json"))
    if not manifests:
        raise ContractError(f"{artifact_dir}: no AArch64 manifests found")
    for manifest_path in manifests:
        stem = manifest_path.name.removesuffix(".manifest.json")
        object_path = artifact_dir / f"{stem}.o"
        manifest = verify_manifest(
            manifest_path,
            object_path,
            repo_root=REPO_ROOT,
        )
        print(
            f"verified backend={manifest['backend']} artifact={stem} "
            f"sha256={manifest['artifact']['sha256']}"
        )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ContractError as error:
        print(f"aarch64-kernel-verify: {error}", file=sys.stderr)
        raise SystemExit(1)
