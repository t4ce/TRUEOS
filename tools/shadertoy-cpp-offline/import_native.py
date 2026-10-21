#!/usr/bin/env python3
"""Import reviewed bake outputs into the Blueprint-owned native shader assets.

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
    parser.add_argument("--artifact-dir", type=Path, required=True, help="reviewed bakery output directory")
    parser.add_argument("--program", choices=[name for name, _ in NATIVE_PROGRAMS.values()])
    args = parser.parse_args()
    artifacts = args.artifact_dir
    for name, artifact in NATIVE_PROGRAMS.values():
        if args.program and args.program != name:
            continue
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
        trust = ROOT / f"crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/{artifact}.contract.rs"
        trust.write_bytes((dest / "kernel.contract.rs").read_bytes())
        print(f"{name}: imported {len(binary)} executable bytes and {len(sources)} raw source files")


if __name__ == "__main__":
    main()
