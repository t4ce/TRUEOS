#!/usr/bin/env python3
"""Adapt one ShaderToy Image pass and bake it with the TRUEOS pipeline."""

from __future__ import annotations

import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys

from adapter import AdapterError, adapt


TOOL_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOL_DIR.parent.parent


def _tool(explicit: str | None, environment: str, names: tuple[str, ...], candidates: tuple[Path, ...]) -> Path | None:
    raw = explicit or os.environ.get(environment)
    if raw:
        return Path(raw).expanduser().resolve()
    for name in names:
        found = shutil.which(name)
        if found:
            return Path(found).resolve()
    for candidate in candidates:
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return candidate.resolve()
    return None


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("--clang")
    parser.add_argument("--llvm-spirv")
    parser.add_argument("--ocloc")
    args = parser.parse_args(argv)

    try:
        source = args.source.read_text(encoding="utf-8")
        generated = adapt(source)
    except (OSError, UnicodeError, AdapterError) as error:
        print(f"adapter: {error}", file=sys.stderr)
        return 2

    session = REPO_ROOT / "bld" / "shadertoy-cpp-offline" / "session"
    session.mkdir(parents=True, exist_ok=True)
    generated_path = session / "shadertoy_image.clcpp"
    generated_path.write_text(generated, encoding="utf-8")

    local_root = REPO_ROOT / "bld" / "shadertoy-cpp-toolchain" / "root"
    clang = _tool(
        args.clang,
        "CLANG",
        ("clang-21", "clang"),
        (local_root / "usr/lib/llvm-21/bin/clang",),
    )
    llvm_spirv = _tool(
        args.llvm_spirv,
        "LLVM_SPIRV",
        ("llvm-spirv-21", "llvm-spirv"),
        (local_root / "usr/bin/llvm-spirv-21", local_root / "usr/bin/llvm-spirv"),
    )
    ocloc = _tool(
        args.ocloc,
        "OCLOC",
        ("ocloc-26.05.1", "ocloc"),
        (
            REPO_ROOT / "bld/intel-tools/root/usr/bin/ocloc-26.05.1",
            local_root / "usr/bin/ocloc-26.05.1",
            local_root / "usr/bin/ocloc",
        ),
    )
    missing = [name for name, path in (("clang-21", clang), ("llvm-spirv", llvm_spirv), ("ocloc", ocloc)) if path is None]
    if missing:
        print(
            "toolchain: missing " + ", ".join(missing)
            + "; run `make -C tools/shadertoy-cpp-offline toolchain` or set CLANG, LLVM_SPIRV, and OCLOC",
            file=sys.stderr,
        )
        return 3

    command = [
        sys.executable,
        "-B",
        str(REPO_ROOT / "tools/intel-gpu-bakery/bake.py"),
        "--source", str(generated_path),
        "--artifact-name", "shadertoy_image",
        "--profile", str(REPO_ROOT / "tools/intel-gpu-bakery/profiles/adls-4680-r0c-cpp.json"),
        "--variant", "cpp-native",
        "--build-root", str(REPO_ROOT / "bld/shadertoy-cpp-offline/bakery"),
        "--expect-kernel", "shadertoy_image",
        "--clang", str(clang),
        "--llvm-spirv", str(llvm_spirv),
        "--ocloc", str(ocloc),
    ]
    result = subprocess.run(command, cwd=REPO_ROOT)
    if result.returncode:
        return result.returncode

    spirv = (
        REPO_ROOT
        / "bld/shadertoy-cpp-offline/bakery/adls/cpp-native/shadertoy_image/run-a/shadertoy_image.spv"
    )
    if not spirv.is_file():
        print(f"bakery: expected output is missing: {spirv}", file=sys.stderr)
        return 4
    print(f"SHADERTOY_SPV={spirv}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

