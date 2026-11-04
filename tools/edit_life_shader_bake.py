#!/usr/bin/env python3
"""Reproduce Edit's pinned, local-only C++ shader bakery setup.

The package pins come from existing workspace bakery installers, not current
package indexes. No apt install, system file writes, PATH edits or lock updates
occur. This reproduces the recovery used on 2026-11-04; the host already had
the exact libLLVM21 and libclang-cpp21 libraries required by the existing lock.
The bakery verifies those host libraries as part of its toolchain fingerprint.
"""

from __future__ import annotations

import argparse
import hashlib
import re
import shlex
import subprocess
import sys
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_BAKERY = ROOT.parent / "tools/intel-gpu-bakery"
CLANG_PACKAGES = {"clang-21", "libclang-common-21-dev", "libclang-rt-21-dev", "clang-tools-21"}
OTHER_PACKAGES = {"llvm-spirv-21", "libllvmspirvlib21.1", "intel-ocloc", "libigc2", "libigdfcl2"}


def package_pins(bakery: Path) -> list[tuple[str, str, str, str]]:
    pins = []
    installer = (bakery / "install_locked_clang21.sh").read_text()
    for name, digest, url in re.findall(r"^(\S+) ([0-9a-f]{128}) (https?://\S+)$", installer, re.M):
        if name in CLANG_PACKAGES:
            pins.append((name, "sha512", digest, url.replace("http:", "https:", 1)))
    bootstrap = (bakery.parent / "shadertoy-cpp-offline/bootstrap_toolchain.sh").read_text()
    for name, _version, digest, relative in re.findall(
        r"^download_and_extract (\S+) (\S+) ([0-9a-f]{64}) (\S+)$", bootstrap, re.M
    ):
        if name in OTHER_PACKAGES:
            pins.append((name, "sha256", digest, "https://archive.ubuntu.com/ubuntu/" + relative))
    if {pin[0] for pin in pins} != CLANG_PACKAGES | OTHER_PACKAGES:
        raise SystemExit("Existing installer package pins changed; review them before recovery")
    return pins


def recover_local(toolchain: Path, pins: list[tuple[str, str, str, str]]) -> None:
    toolchain.mkdir(parents=True, exist_ok=True)
    for name, algorithm, digest, url in pins:
        archive = toolchain / (name + ".deb")
        if not archive.is_file() or hashlib.new(algorithm, archive.read_bytes()).hexdigest() != digest:
            with urllib.request.urlopen(url, timeout=60) as response:
                data = response.read()
            if hashlib.new(algorithm, data).hexdigest() != digest:
                raise SystemExit(f"Pinned checksum mismatch: {name}")
            archive.write_bytes(data)
        subprocess.run(["dpkg-deb", "--extract", str(archive), str(toolchain / "root")], check=True)
        print(f"verified local package: {name}", flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bakery", type=Path, default=DEFAULT_BAKERY)
    parser.add_argument("--toolchain", type=Path, default=Path("/tmp/trueos-edit-life-toolchain"))
    parser.add_argument("--build-root", type=Path, default=Path("/tmp/trueos-edit-life-bake"))
    parser.add_argument("--recover", action="store_true", help="Download/extract the nine existing pinned packages locally")
    parser.add_argument("--bake", action="store_true", help="Reproducibly compile, validate and publish the Life artifact")
    parser.add_argument("--verify", action="store_true", help="Compiler-free verification of the existing Life artifact")
    parser.add_argument("--print-package-pins", action="store_true")
    args = parser.parse_args()
    bakery = args.bakery.expanduser().resolve()
    toolchain = args.toolchain.expanduser().resolve()
    pins = package_pins(bakery)
    if args.print_package_pins:
        for name, algorithm, digest, url in pins:
            print(f"{name}\t{algorithm}:{digest}\t{url}")
    if args.recover:
        recover_local(toolchain, pins)
    artifact_dir = ROOT / "crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp"
    if args.bake:
        command = [sys.executable, str(bakery / "bake.py"),
            "--source", str(ROOT / "crates/trueos-shader/gpgpu/kernels/edit_life_rgba8.clcpp"),
            "--artifact-name", "edit_life_rgba8", "--expect-kernel", "edit_life_rgba8",
            "--rust-symbol", "edit_life_rgba8=EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT",
            "--profile", str(bakery / "profiles/adls-4680-r0c-cpp.json"),
            "--toolchain-lock", str(bakery / "toolchains/adls-cpp-proof.lock.json"),
            "--repro-check", "--build-root", str(args.build_root.expanduser().resolve()),
            "--clang", str(toolchain / "root/usr/lib/llvm-21/bin/clang"),
            "--llvm-spirv", str(toolchain / "root/usr/bin/llvm-spirv-21"),
            "--ocloc", str(toolchain / "root/usr/bin/ocloc-26.05.1"),
            "--publish-dir", str(artifact_dir)]
        print(shlex.join(command), flush=True)
        subprocess.run(command, check=True)
    if args.verify:
        subprocess.run([sys.executable, str(bakery / "verify.py"),
            "--bin", str(artifact_dir / "edit_life_rgba8.bin")], check=True)
    if not any((args.recover, args.bake, args.verify, args.print_package_pins)):
        parser.print_help()


if __name__ == "__main__":
    main()
