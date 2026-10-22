#!/usr/bin/env python3
"""Restore bundled native shader headers missing from older Blueprint revisions."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath

from package_blueprint import NATIVE_PROGRAMS, ROOT


def prepare(directory):
    manifest = json.loads((directory / "kernel.manifest.json").read_text())
    sources = json.loads((directory / "input.sources.json").read_text())
    source = manifest["source"]
    base = PurePosixPath(source["path"]).parent
    pending = []
    for entry in source["inputs"]:
        path = PurePosixPath(entry["path"])
        relative = path.relative_to(base)
        if path.is_absolute() or ".." in relative.parts:
            raise ValueError(f"unsafe bundled input: {path}")
        data = sources[entry["path"]].encode("utf-8")
        if len(data) != entry["size_bytes"] or hashlib.sha256(data).hexdigest() != entry["sha256"]:
            raise ValueError(f"bundled input differs from manifest: {path}")
        destination = directory / ("kernel.clcpp" if entry["path"] == source["path"] else relative)
        if destination.exists():
            if destination.read_bytes() != data:
                raise ValueError(f"existing input differs from bundle: {destination}")
        else:
            pending.append((destination, data))
    for destination, data in pending:
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(data)
    return len(pending)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--blueprints-root", type=Path, default=ROOT.parent / "TRUEOS-Blueprints")
    args = parser.parse_args()
    for name, _ in NATIVE_PROGRAMS.values():
        count = prepare(args.blueprints_root / "apps/shadertoy/assets" / name)
        print(f"{name}: restored {count} manifest-verified source inputs")


if __name__ == "__main__":
    main()
