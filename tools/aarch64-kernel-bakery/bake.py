#!/usr/bin/env python3
"""Bake freestanding C++ kernel entries into audited AArch64 ELF objects."""

from __future__ import annotations

import argparse
import json
import os
import shlex
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

from artifact_contract import (
    BACKEND,
    ContractError,
    ELF_MACHINE_AARCH64,
    SCHEMA_VERSION,
    analyze_object,
    atomic_write,
    input_records,
    repo_relative,
    sha256_file,
    stable_json,
)


TOOL_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOL_DIR.parent.parent
DEFAULT_PROFILE = TOOL_DIR / "profiles" / "aarch64-none-elf.json"


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Compile one freestanding C++ source into a reproducible, linkable "
            "AArch64 CPU-kernel object and emit an audited JSON manifest."
        )
    )
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--artifact-name")
    parser.add_argument("--profile", type=Path, default=DEFAULT_PROFILE)
    parser.add_argument("--expect-entry", action="append", default=[])
    parser.add_argument("--publish-dir", type=Path)
    parser.add_argument(
        "--build-root",
        type=Path,
        default=REPO_ROOT / "bld" / "aarch64-kernel-bakery",
    )
    parser.add_argument("--clang", type=Path)
    parser.add_argument(
        "--repro-check",
        action="store_true",
        help="Compile in two output roots and require byte-identical objects",
    )
    parser.add_argument("--toolchain-lock", type=Path)
    parser.add_argument("--write-toolchain-lock", type=Path)
    return parser


def _load_profile(path: Path) -> dict[str, Any]:
    try:
        profile = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"cannot load profile {path}: {error}") from error
    required = (
        "label",
        "target_triple",
        "architecture",
        "abi",
        "elf_machine",
        "clang_options",
    )
    missing = [key for key in required if key not in profile]
    if missing:
        raise ContractError(f"profile {path} misses keys: {', '.join(missing)}")
    if profile.get("schema_version") != 1:
        raise ContractError(f"profile {path}: unsupported schema")
    if profile["architecture"] != "aarch64" or profile["elf_machine"] != ELF_MACHINE_AARCH64:
        raise ContractError(f"profile {path}: expected an AArch64 target")
    if not isinstance(profile["clang_options"], list) or not all(
        isinstance(option, str) for option in profile["clang_options"]
    ):
        raise ContractError(f"profile {path}: clang_options must be strings")
    return profile


def _tool(requested: Path | None) -> Path:
    raw = str(requested) if requested else os.environ.get("ARM_CLANG")
    if raw:
        expanded = Path(raw).expanduser()
        if expanded.parent != Path("."):
            path = expanded.resolve()
        else:
            found = shutil.which(raw)
            path = Path(found).resolve() if found else Path()
    else:
        found = shutil.which("clang")
        path = Path(found).resolve() if found else Path()
    if not path or not path.is_file() or not os.access(path, os.X_OK):
        raise ContractError("missing Clang; set --clang or ARM_CLANG")
    return path


def _environment() -> dict[str, str]:
    return {
        "LANG": "C",
        "LC_ALL": "C",
        "PATH": os.defpath,
        "SOURCE_DATE_EPOCH": "0",
        "TZ": "UTC",
    }


def _run(
    command: list[str],
    *,
    cwd: Path,
    environment: dict[str, str],
    description: str,
) -> subprocess.CompletedProcess[str]:
    print(f"{description}: {shlex.join(command)}")
    process = subprocess.run(
        command,
        cwd=cwd,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    if process.stdout:
        print(process.stdout, end="" if process.stdout.endswith("\n") else "\n")
    if process.returncode != 0:
        raise ContractError(
            f"{description} failed with exit status {process.returncode}"
        )
    return process


def _version(clang: Path, environment: dict[str, str]) -> str:
    process = subprocess.run(
        [str(clang), "--version"],
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    text = process.stdout.strip()
    if process.returncode != 0 or not text:
        raise ContractError(f"cannot identify {clang}")
    return "\n".join(
        line for line in text.splitlines() if not line.startswith("InstalledDir:")
    )


def _fingerprint(
    clang: Path, profile: dict[str, Any], environment: dict[str, str]
) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "backend": BACKEND,
        "target_triple": profile["target_triple"],
        "profile_sha256": profile["_sha256"],
        "clang": {
            "executable_sha256": sha256_file(clang),
            "version": _version(clang, environment),
        },
    }


def _verify_lock(path: Path, fingerprint: dict[str, Any]) -> None:
    try:
        expected = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"cannot read toolchain lock {path}: {error}") from error
    if expected != fingerprint:
        raise ContractError(
            f"toolchain does not match {path}; regenerate deliberately with "
            "--write-toolchain-lock after reviewing the object"
        )


def _safe_clean(path: Path, build_root: Path) -> None:
    resolved = path.resolve()
    root = build_root.resolve()
    if resolved == root or root not in resolved.parents:
        raise ContractError(f"refusing to clean non-child build path {resolved}")
    if resolved.exists():
        shutil.rmtree(resolved)
    resolved.mkdir(parents=True)


def _parse_depfile(path: Path, cwd: Path) -> list[Path]:
    text = path.read_text(encoding="utf-8").replace("\\\n", " ")
    _target, separator, dependencies = text.partition(":")
    if not separator:
        raise ContractError(f"malformed Clang depfile {path}")
    try:
        words = shlex.split(dependencies, posix=True)
    except ValueError as error:
        raise ContractError(f"malformed Clang depfile {path}: {error}") from error
    return [
        candidate if candidate.is_absolute() else cwd / candidate
        for candidate in map(Path, words)
    ]


def _compile(
    *,
    clang: Path,
    source: Path,
    output_dir: Path,
    artifact_name: str,
    profile: dict[str, Any],
    environment: dict[str, str],
    description: str,
) -> tuple[Path, Path, list[str]]:
    object_path = output_dir / f"{artifact_name}.o"
    depfile = output_dir / f"{artifact_name}.d"
    command = [
        str(clang),
        f"--target={profile['target_triple']}",
        "-x",
        "c++",
        *profile["clang_options"],
        "-MMD",
        "-MF",
        str(depfile),
        "-MT",
        object_path.name,
        "-c",
        source.name,
        "-o",
        str(object_path),
    ]
    _run(command, cwd=source.parent, environment=environment, description=description)
    return object_path, depfile, command


def _normalize_command(
    command: list[str],
    *,
    clang: Path,
    source: Path,
    output_dir: Path,
) -> list[str]:
    replacements = {
        str(clang): "$CLANG",
        str(source.parent): "$SOURCE_DIR",
        str(output_dir): "$OUTPUT_DIR",
    }
    normalized: list[str] = []
    for item in command:
        result = item
        for original, replacement in sorted(
            replacements.items(), key=lambda pair: len(pair[0]), reverse=True
        ):
            result = result.replace(original, replacement)
        normalized.append(result)
    return normalized


def main() -> int:
    args = _parser().parse_args()
    source = args.source.expanduser().resolve()
    profile_path = args.profile.expanduser().resolve()
    build_root = args.build_root.expanduser().resolve()
    if not source.is_file():
        raise ContractError(f"source does not exist: {source}")
    if source.suffix not in (".cpp", ".cc", ".cxx"):
        raise ContractError("AArch64 CPU kernels must be ordinary freestanding C++")
    artifact_name = args.artifact_name or source.stem
    allowed_name_characters = (
        "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_-"
    )
    if not artifact_name or any(
        character not in allowed_name_characters for character in artifact_name
    ):
        raise ContractError(f"invalid artifact name: {artifact_name!r}")
    expected_entries = list(dict.fromkeys(args.expect_entry))
    if not expected_entries:
        raise ContractError("at least one --expect-entry is required")
    if args.publish_dir and not args.repro_check:
        raise ContractError("--publish-dir requires --repro-check")

    profile = _load_profile(profile_path)
    profile["_sha256"] = sha256_file(profile_path)
    clang = _tool(args.clang)
    environment = _environment()
    fingerprint = _fingerprint(clang, profile, environment)
    if args.toolchain_lock:
        _verify_lock(args.toolchain_lock.expanduser().resolve(), fingerprint)

    artifact_root = build_root / artifact_name
    primary = artifact_root / "primary"
    secondary = artifact_root / "secondary"
    _safe_clean(primary, build_root)
    if args.repro_check:
        _safe_clean(secondary, build_root)

    primary_object, primary_depfile, primary_command = _compile(
        clang=clang,
        source=source,
        output_dir=primary,
        artifact_name=artifact_name,
        profile=profile,
        environment=environment,
        description="AArch64 primary compile",
    )
    analysis = analyze_object(
        primary_object,
        expected_machine=profile["elf_machine"],
        expected_entries=expected_entries,
    )

    object_identical = False
    if args.repro_check:
        secondary_object, _secondary_depfile, _secondary_command = _compile(
            clang=clang,
            source=source,
            output_dir=secondary,
            artifact_name=artifact_name,
            profile=profile,
            environment=environment,
            description="AArch64 reproducibility compile",
        )
        object_identical = primary_object.read_bytes() == secondary_object.read_bytes()
        if not object_identical:
            raise ContractError("AArch64 object is not reproducible across output roots")
    if args.write_toolchain_lock:
        atomic_write(
            args.write_toolchain_lock.expanduser().resolve(),
            stable_json(fingerprint),
        )

    dependencies = _parse_depfile(primary_depfile, source.parent)
    if source not in [path.resolve() for path in dependencies]:
        dependencies.append(source)
    inputs = input_records(dependencies, REPO_ROOT)
    target = {
        "label": profile["label"],
        "target_triple": profile["target_triple"],
        "architecture": profile["architecture"],
        "abi": profile["abi"],
        "elf_machine": profile["elf_machine"],
        "endianness": "little",
    }
    manifest = {
        "schema_version": SCHEMA_VERSION,
        "backend": BACKEND,
        "artifact_name": artifact_name,
        "artifact": {
            "file": primary_object.name,
            "size_bytes": primary_object.stat().st_size,
            "sha256": sha256_file(primary_object),
        },
        "target": target,
        "expected_entries": expected_entries,
        "analysis": analysis,
        "inputs": inputs,
        "profile": {
            "path": repo_relative(profile_path, REPO_ROOT),
            "sha256": profile["_sha256"],
            "clang_options": profile["clang_options"],
        },
        "provenance": {
            "toolchain": fingerprint,
            "command": _normalize_command(
                primary_command,
                clang=clang,
                source=source,
                output_dir=primary,
            ),
        },
        "reproducibility": {
            "checked": bool(args.repro_check),
            "object_identical": object_identical,
        },
    }

    destination = (
        args.publish_dir.expanduser().resolve()
        if args.publish_dir
        else primary
    )
    destination.mkdir(parents=True, exist_ok=True)
    destination_object = destination / primary_object.name
    if destination_object != primary_object:
        shutil.copyfile(primary_object, destination_object)
    manifest_path = destination / f"{artifact_name}.manifest.json"
    atomic_write(manifest_path, stable_json(manifest))
    print(
        f"published backend={BACKEND} object={destination_object} "
        f"manifest={manifest_path} sha256={manifest['artifact']['sha256']}"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ContractError as error:
        print(f"aarch64-kernel-bakery: {error}", file=sys.stderr)
        raise SystemExit(1)
