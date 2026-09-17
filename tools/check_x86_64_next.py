#!/usr/bin/env python3
"""Validate the pinned paging experiment without deploying or touching a VM.

Python 3.11+ is required. Default: isolated host tests + TRUEOS-target compile
probe. --static-only needs no Rust. --kernel-check additionally resolves and
checks the real kernel in a fully provisioned checkout; it never boots it.
"""
from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REV = "78b81025d65a3e899f48a6fded8b27889a09d70b"
GIT = "https://github.com/rust-osdev/x86_64"
FEATURES = {"instructions", "nightly", "memory_encryption"}


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def read_contract(root: Path) -> tuple[dict, dict, str]:
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    dep = manifest["dependencies"]["x86_64"]
    patch = manifest["patch"]["crates-io"]["x86_64"]
    require(dep["version"] == "=0.15.5", "x86_64 must have an exact version requirement")
    require(dep.get("default-features") is False, "upstream defaults must be disabled")
    require(set(dep["features"]) == FEATURES, "unexpected x86_64 feature policy")
    require(patch == {"git": GIT, "rev": REV}, "unexpected or floating upstream patch")
    source = (root / "src/pci/mmio.rs").read_text()
    require("OffsetPageTable::new(" not in source, "old MMIO mapper constructor remains")
    require("OffsetPageTable::from_phys_offset(" in source, "MMIO mapper migration missing")
    channel = tomllib.loads((root / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    require(channel.startswith("nightly-"), "the existing kernel toolchain must be explicit")
    return dep, patch, channel


def check_resolution(metadata: dict) -> None:
    packages = [p for p in metadata["packages"] if p["name"] == "x86_64"]
    require(len(packages) == 1, "expected one x86_64 package; inspect duplicate versions/sources")
    package = packages[0]
    require(package["version"] == "0.15.5", "unexpected resolved x86_64 version")
    require(package.get("source") == f"git+{GIT}?rev={REV}#{REV}",
            "Cargo did not resolve the reviewed Git revision (check local patch overrides/lockfile)")
    node = next(n for n in metadata["resolve"]["nodes"] if n["id"] == package["id"])
    require(FEATURES <= set(node["features"]), "required effective features are missing")


def probe_manifest(dep: dict, patch: dict) -> str:
    features = ", ".join(json.dumps(f) for f in dep["features"])
    return f'''[package]
name = "trueos-x86-64-next-probe"
version = "0.0.0"
edition = "2024"
publish = false
[workspace]
[dependencies]
x86_64 = {{ version = "{dep['version']}", default-features = false, features = [{features}] }}
[patch.crates-io]
x86_64 = {{ git = "{patch['git']}", rev = "{patch['rev']}" }}
'''


def run(command: list[str], cwd: Path, log: Path) -> str:
    print("+ " + " ".join(command), flush=True)
    result = subprocess.run(command, cwd=cwd, text=True, capture_output=True, check=False)
    log.write_text(result.stdout + result.stderr)
    if result.stdout and log.suffix != ".json":
        print(result.stdout, end="", flush=True)
    if result.stderr:
        print(result.stderr, end="", file=sys.stderr, flush=True)
    require(result.returncode == 0, f"command failed ({result.returncode}); see {log}")
    return result.stdout


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--static-only", action="store_true")
    mode.add_argument("--kernel-check", action="store_true")
    parser.add_argument("--evidence-dir", type=Path, default=ROOT / "tgt/x86_64-next-validation")
    args = parser.parse_args()
    dep, patch, channel = read_contract(ROOT)
    print(f"Static contract OK: {REV}; toolchain unchanged: {channel}")
    if args.static_only:
        print("Rust compilation, paging execution, and hardware validation NOT run.")
        return 0
    require(shutil.which("cargo") is not None and shutil.which("rustc") is not None,
            f"Rust is required: install {channel} with rust-src, then rerun (no checks were skipped)")
    evidence = args.evidence_dir.resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    cargo = ["cargo", f"+{channel}"]
    # Outside the repository: do not inherit its custom-target/build-std/linker
    # settings when running software-only host tests.
    with tempfile.TemporaryDirectory(prefix="trueos-paging-") as directory:
        work = Path(directory)
        (work / "src").mkdir()
        manifest = probe_manifest(dep, patch)
        (work / "Cargo.toml").write_text(manifest)
        (evidence / "probe-Cargo.toml").write_text(manifest)
        shutil.copyfile(ROOT / "tools/x86_64_paging_probe.rs", work / "src/lib.rs")
        version = run(["rustc", f"+{channel}", "-Vv"], work, evidence / "rustc.txt")
        release = re.search(r"^release: (\d+)\.(\d+)\.(\d+)", version, re.M)
        require(release is not None, "could not determine rustc release")
        require(tuple(map(int, release.groups())) >= (1, 98, 0), "upstream requires Rust >= 1.98")
        host = re.search(r"^host: (\S+)", version, re.M)
        require(host is not None and host[1].startswith("x86_64-"), "probe requires an x86_64 host")
        raw = run(cargo + ["metadata", "--format-version", "1"], work, evidence / "probe-metadata.json")
        check_resolution(json.loads(raw))
        (evidence / "probe-metadata.json").write_text(raw)
        run(cargo + ["test", "--locked", "--lib", "--target", host[1]], work, evidence / "host-tests.txt")
        target = ROOT / ".cargo/x86_64-unknown-trueos.json"
        require(target.is_file(), "TRUEOS custom target specification is missing")
        run(cargo + ["check", "--lib", "--target", str(target), "-Zjson-target-spec",
                     "-Zbuild-std=core,compiler_builtins", "-Zbuild-std-features=compiler-builtins-mem"],
            work, evidence / "custom-target-check.txt")
        shutil.copyfile(work / "Cargo.lock", evidence / "probe-Cargo.lock")
    if args.kernel_check:
        raw = run(cargo + ["metadata", "--format-version", "1"], ROOT, evidence / "kernel-metadata.json")
        check_resolution(json.loads(raw))
        (evidence / "kernel-metadata.json").write_text(raw)
        run(cargo + ["check", "--bin", "TRUEOS"], ROOT, evidence / "kernel-check.txt")
        shutil.copyfile(ROOT / "Cargo.lock", evidence / "kernel-Cargo.lock")
    print(f"Requested software checks passed. Evidence: {evidence}")
    print("NOT hardware validation: no boot, CR3 load, TLB shootdown, EPT/VPID, or DMA retirement exercised.")
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (RuntimeError, OSError, KeyError, StopIteration, ValueError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        sys.exit(1)
