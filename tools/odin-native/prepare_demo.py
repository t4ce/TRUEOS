#!/usr/bin/env python3
"""Prepare a private native-Odin kernel and ISO; never deploy or reset hardware."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
KERNEL_SOURCES = (
    "Cargo.toml", "Cargo.lock", "build.rs", "abi/portal-cabi-v2.sha256",
    "src/gpu/native_xelp.rs", "src/intel/gpgpu/gpgpu.rs",
    "src/intel/gpgpu/operations/generated_pixel.rs",
    "src/ui4/blueprint_ui4.rs", "src/ui4/blueprint_ui4/native_compute_api.rs",
    "src/hv/blueprint/prebind.rs", "src/hv/vmcall.rs",
    "src/r/io/async_fs_cabi.rs", "crates/trueos-vm/src/vmcall.rs",
)
ODIN_SOURCES = (
    "trueos/main.odin", "trueos/mandelbrot.odin", "trueos/embedded.odin",
    "trueos/build.sh", "trueos/build-mandelbrot.sh", "trueos/prepare.py",
    "trueos/package.py", "typecheck/gpu_native.odin",
    "bac/intel_xelp/target.odin", "bac/intel_xelp/encode.odin",
)


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def git_head(path):
    return subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=path).decode().strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bp", type=Path,
                        default=ROOT.parent / "odin-son/trueos/build/odin-mandelbrot.bp")
    parser.add_argument("--blueprints-root", type=Path, default=ROOT.parent / "TRUEOS-Blueprints")
    parser.add_argument("--odin-root", type=Path, default=ROOT.parent / "odin-son")
    parser.add_argument("--base-iso", type=Path, default=ROOT / "bld/trueos.iso")
    parser.add_argument("--output", type=Path, required=True, help="New artifact directory")
    parser.add_argument("--cache", type=Path,
                        help="Reuse a Cargo cache, including tgt; otherwise clone tgt privately")
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.blueprints_root = args.blueprints_root.resolve()
    args.odin_root = args.odin_root.resolve()
    manifest = args.blueprints_root / "buildins.json"
    names = json.loads(manifest.read_text())["buildins"]
    if (not isinstance(names, list) or not names
            or any(not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", name)
                   for name in names) or len(set(names)) != len(names)):
        parser.error("buildins.json must contain unique valid app names")
    names = [name for name in names if name != "odin-mandelbrot"] + ["odin-mandelbrot"]
    packages = {name: args.blueprints_root / "dist" / f"{name}.bp" for name in names}
    packages["odin-mandelbrot"] = args.bp.resolve()
    snapshots = [("kernel", ROOT, name) for name in KERNEL_SOURCES]
    snapshots += [("odin-son", args.odin_root, name) for name in ODIN_SOURCES]
    for path in [args.base_iso, *packages.values(), *(base / name for _, base, name in snapshots)]:
        if not path.is_file():
            parser.error(f"required input is missing: {path}")
    cache = args.cache.resolve() if args.cache else args.output / "kernel-cache"
    if args.cache and not cache.is_dir():
        parser.error(f"Cargo cache is missing: {cache}")
    args.output.mkdir(parents=True, exist_ok=False)
    bundle = args.output / "blueprints"
    (bundle / "dist").mkdir(parents=True)
    for name, path in packages.items():
        shutil.copy2(path, bundle / "dist" / f"{name}.bp")
    (bundle / "buildins.json").write_text(json.dumps({"buildins": names}, indent=2) + "\n")
    base = args.output / "base.iso"
    shutil.copy2(args.base_iso, base)
    if not args.cache:
        subprocess.run(["cp", "-a", "--reflink=auto", str(ROOT / "tgt"), str(cache)], check=True)
    source_hashes = {}
    for repository, source_root, name in snapshots:
        destination = args.output / "sources" / repository / name
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source_root / name, destination)
        source_hashes[f"{repository}/{name}"] = sha256(destination)
    # Preserve uncommitted changes as review material; snapshots above also
    # include newly created integration files that git diff might not show.
    for repository, source_root in (("kernel", ROOT), ("odin-son", args.odin_root)):
        patch = subprocess.check_output(["git", "diff", "HEAD", "--"], cwd=source_root)
        (args.output / f"{repository}-source.patch").write_bytes(patch)
    environment = dict(os.environ, TRUEOS_BLUEPRINTS_DIR=str(bundle),
                       TRUEOS_ENABLE_BUILDINS="1", TRUEOS_REQUIRE_BUILDINS="1",
                       CARGO_BUILD_JOBS=os.environ.get("CARGO_BUILD_JOBS", "2"))
    command = ["cargo", "build", "--locked", "--offline", "--target-dir", str(cache),
               "--features", "odin-native-gpgpu",
               "-Z", "build-std=core,compiler_builtins,alloc,panic_abort",
               "-Z", "json-target-spec", "--target", ".cargo/x86_64-unknown-trueos.json"]
    print(f"Building native-Odin demo; log: {args.output / 'kernel-build.log'}", flush=True)
    with (args.output / "kernel-build.log").open("wb") as log:
        subprocess.run(command, cwd=ROOT, env=environment,
                       stdout=log, stderr=subprocess.STDOUT, check=True)
    for repository, source_root, name in snapshots:
        if sha256(source_root / name) != source_hashes[f"{repository}/{name}"]:
            raise RuntimeError(f"Source changed during build: {repository}/{name}; use a fresh output")
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
    files = {name: sha256(args.output / name) for name in
             ("base.iso", "TRUEOS.full.elf", "TRUEOS.elf", "trueos.iso",
              "blueprints/buildins.json", "kernel-source.patch", "odin-son-source.patch")}
    files.update({f"blueprints/dist/{name}.bp": sha256(bundle / "dist" / f"{name}.bp")
                  for name in names})
    record = {"command": command, "kernel_git": git_head(ROOT),
              "odin_git": git_head(args.odin_root), "bundle": names,
              "cargo_cache": str(cache), "reused_cache": bool(args.cache),
              "source_sha256": source_hashes, "files": files,
              "launch": "Shell3 CMD/AppDB: odin-mandelbrot",
              "deployed": False}
    (args.output / "build.json").write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps(record))


if __name__ == "__main__":
    main()
