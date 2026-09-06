#!/usr/bin/env python3
"""Copy reviewed native gallery artifacts and every baked source into the Blueprint.

Run package_blueprint.py --update-trust after reviewing these imports. This does
not rebake or alter executable code; stale source provenance fails closed.
"""
import argparse
import hashlib
import json
from pathlib import Path

from package_blueprint import ROOT, NATIVE_PROGRAMS


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--blueprints-root", type=Path, default=ROOT.parent / "TRUEOS-Blueprints")
    args = parser.parse_args()
    artifacts = ROOT / "crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp"
    for name, artifact in NATIVE_PROGRAMS.values():
        manifest = json.loads((artifacts / f"{artifact}.manifest.json").read_bytes())
        sources = {}
        for entry in manifest["source"]["inputs"]:
            data = (ROOT / entry["path"]).read_bytes()
            if len(data) != entry["size_bytes"] or hashlib.sha256(data).hexdigest() != entry["sha256"]:
                raise SystemExit(f"{artifact}: stale baked input: {entry['path']}")
            sources[entry["path"]] = data.decode()
        binary = (artifacts / f"{artifact}.bin").read_bytes()
        if hashlib.sha256(binary).hexdigest() != manifest["artifact"]["elf"]["sha256"]:
            raise SystemExit(f"{artifact}: binary differs from baked provenance")
        dest = args.blueprints_root / "apps/shadertoy/assets" / name
        dest.mkdir(parents=True, exist_ok=True)
        for suffix in ("bin", "spv", "manifest.json", "contract.rs"):
            (dest / f"kernel.{suffix}").write_bytes((artifacts / f"{artifact}.{suffix}").read_bytes())
        (dest / "kernel.clcpp").write_text(sources[manifest["source"]["path"]])
        (dest / "input.sources.json").write_text(json.dumps(sources, sort_keys=True, indent=2) + "\n")
        print(f"{name}: imported {len(binary)} executable bytes and {len(sources)} raw source files")


if __name__ == "__main__":
    main()
