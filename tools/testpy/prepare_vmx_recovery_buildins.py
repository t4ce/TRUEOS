#!/usr/bin/env python3
"""Snapshot prepacked recovery probes into a local, rig-only ISO overlay.

This reads local packages and writes a new directory; it neither builds nor
publishes anything. Example:
  python3 tools/testpy/prepare_vmx_recovery_buildins.py --output bld/recovery/blueprints

The apps symlink supports Make's GPU asset checks. Packages are independent
copies: their hashes stay fixed when the source dist changes. The manifest
predates every copy, BUILDIN_COMMON_INPUTS= suppresses shared-source prerequisites,
and the emitted command uses -o for each snapshot so even later app-source edits
cannot invoke the ordinary built-in-app packer on these probe names. Use the
emitted make_command (also stored in metadata.json) for the exact snapshot.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex

ROOT = Path(__file__).resolve().parents[2]
PROBES = {"vm_recovery": "vm_recovery", "vmpeer": "vm_recovery_peer"}


def prepare(blueprints: Path, output: Path) -> dict:
    source = blueprints.expanduser().resolve(strict=True)
    output = output.expanduser().absolute()
    if output.exists() or output.is_symlink():
        raise ValueError(f"output already exists; choose a new directory: {output}")
    if not (source / "apps").is_dir():
        raise ValueError(f"Blueprint apps directory required for GPU asset checks: {source / 'apps'}")
    original_manifest = source / "buildins.json"
    original_manifest_bytes = original_manifest.read_bytes()
    document = json.loads(original_manifest_bytes)
    normal = document.get("buildins") if isinstance(document, dict) else None
    if not isinstance(normal, list) or not normal or any(
        not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", name) for name in normal
    ):
        raise ValueError(f"invalid buildins name list: {original_manifest}")
    if len(set(normal)) != len(normal) or set(normal).intersection(PROBES):
        raise ValueError("normal buildins contain duplicate or reserved recovery names")
    packages = {name: name for name in normal} | PROBES
    snapshots = []
    for name, package in packages.items():
        path = source / "dist" / f"{package}.bp"
        before = path.stat()
        if not path.is_file() or before.st_size == 0:
            raise ValueError(f"package must be a nonempty file: {path}")
        content = path.read_bytes()
        after = path.stat()
        if (before.st_ino, before.st_size, before.st_mtime_ns) != (
            after.st_ino, after.st_size, after.st_mtime_ns
        ):
            raise ValueError(f"package changed while being read: {path}")
        snapshots.append((content, {
            "name": name, "package": package, "source": str(path.resolve()),
            "bytes": len(content), "sha256": hashlib.sha256(content).hexdigest(),
            "source_mtime_ns": after.st_mtime_ns,
        }))
    # All inputs are validated before creating the exclusively owned output.
    output.mkdir(parents=True, exist_ok=False)
    (output / "apps").symlink_to(source / "apps", target_is_directory=True)
    dist = output / "dist"
    dist.mkdir()
    for content, row in snapshots:
        with (dist / f"{row['name']}.bp").open("xb") as handle:
            handle.write(content)
    manifest_bytes = (json.dumps({"buildins": list(packages)}, indent=2) + "\n").encode()
    manifest = output / "buildins.json"
    with manifest.open("xb") as handle:
        handle.write(manifest_bytes)
    oldest_package_ns = min(path.stat().st_mtime_ns for path in dist.iterdir())
    manifest_ns = oldest_package_ns - 1_000_000_000
    os.utime(manifest, ns=(manifest_ns, manifest_ns))
    make_command = ["make", "iso", f"BLUEPRINTS_DIR={output}", "BUILDIN_COMMON_INPUTS="]
    for name in packages:
        make_command.extend(["-o", str(dist / f"{name}.bp")])
    metadata = {
        "scope": "local approved-rig-only recovery ISO overlay; no publication",
        "blueprints": str(source), "output": str(output),
        "source_manifest_sha256": hashlib.sha256(original_manifest_bytes).hexdigest(),
        "manifest_sha256": hashlib.sha256(manifest_bytes).hexdigest(),
        "manifest_mtime_ns": manifest_ns,
        "packages": [row for _, row in snapshots],
        "make_argv": make_command,
        "make_command": shlex.join(make_command),
    }
    with (output / "metadata.json").open("x") as handle:
        json.dump(metadata, handle, indent=2)
        handle.write("\n")
    return metadata


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--blueprints", type=Path, default=ROOT.parent / "TRUEOS-Blueprints")
    parser.add_argument("--output", type=Path, required=True, help="new overlay directory; existing paths are refused")
    args = parser.parse_args()
    try:
        metadata = prepare(args.blueprints, args.output)
    except (OSError, ValueError) as error:
        parser.exit(1, f"error: {error}\n")
    print(json.dumps(metadata, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
