#!/usr/bin/env python3
"""Embed a local probe in a private kernel/ISO, without publishing or deploying."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bp", type=Path, required=True)
    parser.add_argument("--base-iso", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cache", type=Path, help="Reuse an existing PRIVATE kernel cache")
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    bundle = args.output / "blueprints"
    (bundle / "dist").mkdir(parents=True)
    package = bundle / "dist/depthchecker_factchecker.bp"
    shutil.copy2(args.bp, package)
    (bundle / "buildins.json").write_text('{"buildins":["depthchecker_factchecker"]}\n')
    base = args.output / "base.iso"
    shutil.copy2(args.base_iso, base)
    cache = args.cache.resolve() if args.cache else args.output / "kernel-cache"
    if not args.cache:
        # Independent files (reflinks where supported); cargo can mutate this
        # cache without racing another kernel build or changing its outputs.
        subprocess.run(["cp", "-a", "--reflink=auto", str(ROOT / "tgt"), str(cache)], check=True)
    environment = dict(os.environ, TRUEOS_BLUEPRINTS_DIR=str(bundle),
                       TRUEOS_ENABLE_BUILDINS="1", TRUEOS_REQUIRE_BUILDINS="1",
                       CARGO_BUILD_JOBS="2")
    diagnostic_sources = {}
    for name in ("src/hv/depthchecker_debug.rs", "src/hv/mod.rs", "src/std_abi_shim.rs",
                 "crates/trueos-vm/src/guest.rs"):
        source = ROOT / name
        destination = args.output / "kernel-sources" / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
        diagnostic_sources[name] = sha256(destination)
    command = ["cargo", "build", "--locked", "--offline", "--target-dir", str(cache),
               "-Z", "build-std=core,compiler_builtins,alloc,panic_abort",
               "-Z", "json-target-spec", "--target", ".cargo/x86_64-unknown-trueos.json"]
    with (args.output / "kernel-build.log").open("wb") as log:
        subprocess.run(command, cwd=ROOT, env=environment, stdout=log, stderr=subprocess.STDOUT, check=True)
    if any(sha256(ROOT / name) != expected for name, expected in diagnostic_sources.items()):
        raise RuntimeError("Diagnostic kernel sources changed during compilation; rerun into a fresh output directory")
    full = args.output / "TRUEOS.full.elf"
    shutil.copy2(cache / "x86_64-unknown-trueos/debug/TRUEOS", full)
    stripped = args.output / "TRUEOS.elf"
    shutil.copy2(full, stripped)
    subprocess.run(["strip", "-s", str(stripped)], check=True)
    iso = args.output / "trueos.iso"
    with (args.output / "iso-build.log").open("wb") as log:
        subprocess.run(["xorriso", "-indev", str(base), "-outdev", str(iso),
                        "-boot_image", "any", "replay", "-map", str(stripped), "/TRUEOS.elf"],
                       stdout=log, stderr=subprocess.STDOUT, check=True)
    patch = subprocess.check_output(["git", "diff", "HEAD", "--", "src", "crates/trueos-vm/src/guest.rs", "build.rs", "Cargo.toml", "Cargo.lock"], cwd=ROOT)
    (args.output / "kernel-source.patch").write_bytes(patch)
    record = {"command": command, "kernel_git": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip(),
              "bundle": ["depthchecker_factchecker"], "private_cache": str(cache),
              "diagnostic_source_sha256": diagnostic_sources,
              "files": {name: sha256(path) for name, path in (("probe.bp", package), ("base.iso", base),
                         ("TRUEOS.full.elf", full), ("TRUEOS.elf", stripped), ("trueos.iso", iso))}}
    (args.output / "build.json").write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps(record))


if __name__ == "__main__":
    main()
