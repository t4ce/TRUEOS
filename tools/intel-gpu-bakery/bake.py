#!/usr/bin/env python3
"""Opt-in TRUEOS OpenCL/C++ for OpenCL -> Intel Zebin bakery."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shlex
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

from artifact_contract import (
    ContractError,
    analyze_zebin,
    atomic_write,
    build_manifest,
    compare_abi,
    input_records,
    render_rust_contracts,
    repo_relative,
    sha256_file,
    stable_json,
    validate_constraints,
)


TOOL_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOL_DIR.parent.parent
DEFAULT_PROFILE = TOOL_DIR / "profiles" / "adls.json"


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Bake one source into SPIR-V + Intel Zebin and generate an audited "
            "TRUEOS no_std ABI contract. With no --publish-dir, output remains "
            "under the ignored build root."
        )
    )
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--artifact-name")
    parser.add_argument("--profile", type=Path, default=DEFAULT_PROFILE)
    parser.add_argument("--variant")
    parser.add_argument("--frontend", choices=("auto", "clang-clcpp", "ocloc-cl"), default="auto")
    parser.add_argument("--publish-dir", type=Path)
    parser.add_argument(
        "--build-root",
        type=Path,
        default=REPO_ROOT / "bld" / "intel-gpu-bakery",
    )
    parser.add_argument("--clang", type=Path)
    parser.add_argument("--llvm-spirv", type=Path)
    parser.add_argument("--ocloc", type=Path)
    parser.add_argument(
        "--ocloc-library-path",
        action="append",
        type=Path,
        default=[],
        help="Repeat for directories containing libocloc/libigc",
    )
    parser.add_argument("--abi-reference-bin", type=Path)
    parser.add_argument("--expect-kernel", action="append", default=[])
    parser.add_argument(
        "--rust-symbol",
        action="append",
        default=[],
        metavar="KERNEL=SYMBOL",
    )
    parser.add_argument(
        "--repro-check",
        action="store_true",
        help="Compile twice in distinct build roots and require identical BC/SPIR-V/Zebin",
    )
    parser.add_argument(
        "--toolchain-lock",
        type=Path,
        help="Require the current toolchain fingerprint to match this JSON lock",
    )
    parser.add_argument(
        "--write-toolchain-lock",
        type=Path,
        help="Write the current toolchain fingerprint after a successful bake",
    )
    return parser


def _load_profile(path: Path) -> dict[str, Any]:
    try:
        profile = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"cannot load profile {path}: {error}") from error
    required = ("label", "device", "pci_device_ids", "constraints", "cpp")
    missing = [key for key in required if key not in profile]
    if missing:
        raise ContractError(f"profile {path} misses keys: {', '.join(missing)}")
    if profile.get("schema_version") != 1:
        raise ContractError(f"profile {path}: unsupported schema")
    return profile


def _tool(
    requested: Path | None,
    environment_name: str,
    command_name: str,
    candidates: list[Path],
) -> Path:
    raw: str | None = str(requested) if requested else os.environ.get(environment_name)
    if raw:
        path = Path(raw).expanduser().resolve()
    else:
        found = shutil.which(command_name)
        if found:
            path = Path(found).resolve()
        else:
            path = next((candidate.resolve() for candidate in candidates if candidate.is_file()), Path())
    if not path or not path.is_file() or not os.access(path, os.X_OK):
        raise ContractError(
            f"missing executable {command_name}; set --{environment_name.lower().replace('_', '-')} "
            f"or {environment_name}"
        )
    return path


def _ocloc_library_paths(ocloc: Path, requested: list[Path]) -> list[Path]:
    paths = [path.expanduser().resolve() for path in requested]
    environment = os.environ.get("OCLOC_LD_LIBRARY_PATH", "")
    paths.extend(Path(item).resolve() for item in environment.split(":") if item)

    # oneAPI/DPC++ layout: .../lib/ocloc/bin/ocloc with sibling igc/lib.
    if ocloc.parent.name == "bin" and ocloc.parent.parent.name == "ocloc":
        component_root = ocloc.parent.parent
        igc_library_root = component_root.parent / "igc" / "lib"
        paths.extend(
            [
                component_root / "lib",
                igc_library_root,
                igc_library_root / "igc2",
            ]
        )
    # Extracted Debian packages: .../usr/bin/ocloc + .../usr/lib/x86_64-linux-gnu.
    if ocloc.parent.name == "bin" and ocloc.parent.parent.name == "usr":
        paths.append(ocloc.parent.parent / "lib" / "x86_64-linux-gnu")

    unique = []
    seen = set()
    for path in paths:
        if path.is_dir() and path not in seen:
            unique.append(path)
            seen.add(path)
    return unique


def _environment(library_paths: list[Path]) -> dict[str, str]:
    environment = os.environ.copy()
    existing = environment.get("LD_LIBRARY_PATH", "")
    joined = ":".join(str(path) for path in library_paths)
    environment["LD_LIBRARY_PATH"] = f"{joined}:{existing}".strip(":")
    environment["LC_ALL"] = "C"
    environment["LANG"] = "C"
    return environment


def _display_command(command: list[str], replacements: dict[str, str]) -> list[str]:
    normalized = []
    for item in command:
        result = item
        for source, replacement in sorted(
            replacements.items(), key=lambda pair: len(pair[0]), reverse=True
        ):
            result = result.replace(source, replacement)
        normalized.append(result)
    return normalized


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


def _version(
    tool: Path,
    arguments: list[str],
    *,
    cwd: Path,
    environment: dict[str, str],
) -> str:
    process = subprocess.run(
        [str(tool), *arguments],
        cwd=cwd,
        env=environment,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    text = process.stdout.strip().replace("\0", "")
    if process.returncode != 0 or not text:
        raise ContractError(
            f"cannot identify {tool.name}: exit={process.returncode} output={text!r}"
        )
    return text


def _compiler_libraries(paths: list[Path]) -> list[dict[str, Any]]:
    patterns = (
        "libocloc.so*",
        "libigc.so*",
        "libigdfcl.so*",
        "libiga64.so*",
        "libopencl-clang.so*",
    )
    aliases: dict[Path, set[str]] = {}
    for directory in paths:
        for pattern in patterns:
            for path in sorted(directory.glob(pattern)):
                resolved = path.resolve()
                if not resolved.is_file():
                    continue
                aliases.setdefault(resolved, set()).add(path.name)
        if directory.name == "igc2":
            # Some IGC packages place compiler resources rather than shared
            # objects in this sibling directory. They are compiler inputs too.
            for path in sorted(directory.rglob("*")):
                resolved = path.resolve()
                if resolved.is_file():
                    relative = path.relative_to(directory).as_posix()
                    aliases.setdefault(resolved, set()).add(f"igc2/{relative}")
    records = []
    for resolved, names in aliases.items():
        sorted_names = sorted(names, key=lambda name: (len(name), name))
        records.append(
            {
                "name": sorted_names[0],
                "aliases": sorted(names),
                "resolved_name": resolved.name,
                "sha256": sha256_file(resolved),
                "size_bytes": resolved.stat().st_size,
            }
        )
    return sorted(records, key=lambda item: (item["name"], item["resolved_name"]))


def _tool_record(path: Path, version: str) -> dict[str, Any]:
    resolved = path.resolve()
    return {
        "executable_sha256": sha256_file(resolved),
        "version": version,
    }


def _toolchain_fingerprint(
    *,
    frontend: str,
    clang: Path | None,
    llvm_spirv: Path | None,
    ocloc: Path,
    query_dir: Path,
    environment: dict[str, str],
    library_paths: list[Path],
    profile_sha256: str,
) -> dict[str, Any]:
    query_dir.mkdir(parents=True, exist_ok=True)
    tools: dict[str, Any] = {}
    if clang is not None:
        tools["clang"] = _tool_record(
            clang, _version(clang, ["--version"], cwd=query_dir, environment=environment)
        )
    if llvm_spirv is not None:
        tools["llvm_spirv"] = _tool_record(
            llvm_spirv,
            _version(llvm_spirv, ["--version"], cwd=query_dir, environment=environment),
        )
    driver_version = _version(
        ocloc,
        ["query", "OCL_DRIVER_VERSION"],
        cwd=query_dir,
        environment=environment,
    )
    tools["ocloc"] = _tool_record(ocloc, driver_version)
    return {
        "schema_version": 1,
        "frontend": frontend,
        "profile_sha256": profile_sha256,
        "tools": tools,
        "compiler_libraries": _compiler_libraries(library_paths),
    }


def _lock_projection(fingerprint: dict[str, Any]) -> dict[str, Any]:
    # Tool identity is content/version based and does not constrain the host's
    # installation path. Published manifests therefore contain the same
    # path-independent representation as the reviewed lock.
    tools = {
        name: {
            "executable_sha256": value["executable_sha256"],
            "version": value["version"],
        }
        for name, value in fingerprint["tools"].items()
    }
    return {
        "schema_version": fingerprint["schema_version"],
        "frontend": fingerprint["frontend"],
        "profile_sha256": fingerprint["profile_sha256"],
        "tools": tools,
        "compiler_libraries": fingerprint["compiler_libraries"],
    }


def _verify_lock(path: Path, fingerprint: dict[str, Any]) -> None:
    try:
        expected = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"cannot read toolchain lock {path}: {error}") from error
    actual = _lock_projection(fingerprint)
    if expected != actual:
        raise ContractError(
            f"toolchain does not match {path}; regenerate deliberately with "
            "--write-toolchain-lock after reviewing compiler changes"
        )


def _parse_depfile(path: Path, cwd: Path) -> list[Path]:
    text = path.read_text(encoding="utf-8").replace("\\\n", " ")
    _target, separator, dependencies = text.partition(":")
    if not separator:
        raise ContractError(f"malformed Clang depfile {path}")
    try:
        words = shlex.split(dependencies, posix=True)
    except ValueError as error:
        raise ContractError(f"malformed Clang depfile {path}: {error}") from error
    return [candidate if candidate.is_absolute() else cwd / candidate for candidate in map(Path, words)]


def _safe_clean(path: Path, build_root: Path) -> None:
    resolved = path.resolve()
    root = build_root.resolve()
    if resolved == root or root not in resolved.parents:
        raise ContractError(f"refusing to clean non-child build path {resolved}")
    if resolved.exists():
        shutil.rmtree(resolved)
    resolved.mkdir(parents=True)


def _cpp_commands(
    *,
    source: Path,
    out_dir: Path,
    artifact_name: str,
    clang: Path,
    llvm_spirv: Path,
    ocloc: Path,
    profile: dict[str, Any],
) -> tuple[list[str], list[str], list[str], Path, Path, Path, Path]:
    bitcode = out_dir / f"{artifact_name}.bc"
    spirv = out_dir / f"{artifact_name}.spv"
    binary = out_dir / f"{artifact_name}.bin"
    depfile = out_dir / f"{artifact_name}.d"
    cpp = profile["cpp"]
    clang_command = [
        str(clang),
        f"--target={cpp['clang_target']}",
        "-x",
        "clcpp",
        f"-cl-std={cpp['standard']}",
        *cpp["clang_options"],
        "-MMD",
        "-MF",
        str(depfile),
        "-MT",
        bitcode.name,
        "-emit-llvm",
        "-c",
        source.name,
        "-o",
        str(bitcode),
    ]
    translator_command = [
        str(llvm_spirv),
        *cpp["llvm_spirv_options"],
        str(bitcode),
        "-o",
        str(spirv),
    ]
    ocloc_command = [
        str(ocloc),
        "compile",
        "-file",
        str(spirv),
        "-spirv_input",
        "-device",
        str(profile["device"]),
        "-64",
        "-output",
        artifact_name,
        "-out_dir",
        str(out_dir),
        "-output_no_suffix",
        "-gen_file",
    ]
    return (
        clang_command,
        translator_command,
        ocloc_command,
        bitcode,
        spirv,
        binary,
        depfile,
    )


def _build_once(
    *,
    frontend: str,
    source: Path,
    artifact_name: str,
    out_dir: Path,
    build_root: Path,
    profile: dict[str, Any],
    clang: Path | None,
    llvm_spirv: Path | None,
    ocloc: Path,
    environment: dict[str, str],
) -> dict[str, Any]:
    _safe_clean(out_dir, build_root)
    commands: list[list[str]] = []
    inputs = [source]
    bitcode: Path | None = None
    if frontend == "clang-clcpp":
        assert clang is not None and llvm_spirv is not None
        (
            clang_command,
            translator_command,
            ocloc_command,
            bitcode,
            spirv,
            binary,
            depfile,
        ) = _cpp_commands(
            source=source,
            out_dir=out_dir,
            artifact_name=artifact_name,
            clang=clang,
            llvm_spirv=llvm_spirv,
            ocloc=ocloc,
            profile=profile,
        )
        # Source basename + source-directory cwd is a reproducibility invariant:
        # Clang embeds the spelling of this filename in LLVM/SPIR-V.
        _run(
            clang_command,
            cwd=source.parent,
            environment=environment,
            description="C++ for OpenCL -> LLVM bitcode",
        )
        inputs = _parse_depfile(depfile, source.parent)
        _run(
            translator_command,
            cwd=out_dir,
            environment=environment,
            description="LLVM bitcode -> OpenCL SPIR-V",
        )
        _run(
            ocloc_command,
            cwd=out_dir,
            environment=environment,
            description="OpenCL SPIR-V -> Intel Zebin",
        )
        commands.extend([clang_command, translator_command, ocloc_command])
    else:
        spirv = out_dir / f"{artifact_name}.spv"
        binary = out_dir / f"{artifact_name}.bin"
        ocloc_command = [
            str(ocloc),
            "compile",
            "-file",
            str(source),
            "-device",
            str(profile["device"]),
            "-64",
            "-output",
            artifact_name,
            "-out_dir",
            str(out_dir),
            "-output_no_suffix",
            "-gen_file",
        ]
        _run(
            ocloc_command,
            cwd=out_dir,
            environment=environment,
            description="OpenCL C -> SPIR-V + Intel Zebin",
        )
        commands.append(ocloc_command)

    validate_command = [str(ocloc), "validate", "-file", str(binary)]
    _run(
        validate_command,
        cwd=out_dir,
        environment=environment,
        description="ocloc validate",
    )
    commands.append(validate_command)
    for path in (spirv, binary):
        if not path.is_file() or path.stat().st_size == 0:
            raise ContractError(f"compiler did not produce {path}")
    return {
        "bitcode": bitcode,
        "spirv": spirv,
        "binary": binary,
        "inputs": inputs,
        "commands": commands,
    }


def _rust_symbol_map(values: list[str]) -> dict[str, str]:
    result = {}
    identifier = __import__("re").compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
    for value in values:
        kernel, separator, symbol = value.partition("=")
        if not separator or not kernel or not identifier.fullmatch(symbol):
            raise ContractError(f"invalid --rust-symbol {value!r}; use KERNEL=RUST_IDENT")
        result[kernel] = symbol
    return result


def _publish(source: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_name(f".{destination.name}.tmp")
    shutil.copyfile(source, temporary)
    temporary.replace(destination)


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    source = args.source.expanduser().resolve()
    if not source.is_file():
        raise ContractError(f"source does not exist: {source}")
    profile_path = args.profile.expanduser().resolve()
    profile = _load_profile(profile_path)
    artifact_name = args.artifact_name or source.stem
    if not artifact_name or "/" in artifact_name:
        raise ContractError(f"invalid artifact name {artifact_name!r}")

    frontend = args.frontend
    if frontend == "auto":
        frontend = "clang-clcpp" if source.suffix == ".clcpp" else "ocloc-cl"
    if frontend == "clang-clcpp" and source.suffix != ".clcpp":
        raise ContractError("clang-clcpp frontend requires a .clcpp source")
    if frontend == "ocloc-cl" and source.suffix != ".cl":
        raise ContractError("ocloc-cl frontend requires a .cl source")
    variant = args.variant or ("cpp" if frontend == "clang-clcpp" else "legacy")

    ocloc_candidates = [
        REPO_ROOT / "bld/intel-tools/root/usr/bin/ocloc-26.05.1",
    ]
    ocloc = _tool(args.ocloc, "OCLOC", "ocloc", ocloc_candidates)
    clang: Path | None = None
    llvm_spirv: Path | None = None
    if frontend == "clang-clcpp":
        clang = _tool(args.clang, "CLANG", "clang", [])
        llvm_spirv = _tool(args.llvm_spirv, "LLVM_SPIRV", "llvm-spirv", [])

    library_paths = _ocloc_library_paths(ocloc, args.ocloc_library_path)
    environment = _environment(library_paths)
    build_root = args.build_root.expanduser().resolve()
    run_root = build_root / profile["label"] / variant / artifact_name
    run_root.mkdir(parents=True, exist_ok=True)
    fingerprint = _toolchain_fingerprint(
        frontend=frontend,
        clang=clang,
        llvm_spirv=llvm_spirv,
        ocloc=ocloc,
        query_dir=run_root / "tool-query",
        environment=environment,
        library_paths=library_paths,
        profile_sha256=sha256_file(profile_path),
    )
    if args.toolchain_lock:
        _verify_lock(args.toolchain_lock.expanduser().resolve(), fingerprint)

    run_a = _build_once(
        frontend=frontend,
        source=source,
        artifact_name=artifact_name,
        out_dir=run_root / "run-a",
        build_root=build_root,
        profile=profile,
        clang=clang,
        llvm_spirv=llvm_spirv,
        ocloc=ocloc,
        environment=environment,
    )
    if args.repro_check:
        run_b = _build_once(
            frontend=frontend,
            source=source,
            artifact_name=artifact_name,
            out_dir=run_root / "different-root" / "run-b",
            build_root=build_root,
            profile=profile,
            clang=clang,
            llvm_spirv=llvm_spirv,
            ocloc=ocloc,
            environment=environment,
        )
        compare_names = ("spirv", "binary")
        if frontend == "clang-clcpp":
            compare_names = ("bitcode", *compare_names)
        for name in compare_names:
            first = run_a[name]
            second = run_b[name]
            assert first is not None and second is not None
            if sha256_file(first) != sha256_file(second):
                raise ContractError(
                    f"reproducibility check failed: {name} differs across build roots"
                )
        print("reproducibility check: BC/SPIR-V/Zebin are byte-identical")

    analysis = analyze_zebin(run_a["binary"], run_a["spirv"])
    constraints = profile["constraints"]
    validate_constraints(
        analysis,
        expected_simd_width=int(constraints["simd_width"]),
        max_scratch_bytes=int(constraints["max_scratch_bytes"]),
        max_slm_bytes=int(constraints["max_slm_bytes"]),
    )
    actual_names = sorted(kernel["kernel_name"] for kernel in analysis["kernels"])
    if args.expect_kernel and actual_names != sorted(args.expect_kernel):
        raise ContractError(
            f"kernel set mismatch: expected={sorted(args.expect_kernel)} actual={actual_names}"
        )

    abi_reference = None
    if args.abi_reference_bin:
        reference_path = args.abi_reference_bin.expanduser().resolve()
        reference = analyze_zebin(reference_path)
        compare_abi(analysis, reference, reference_path)
        abi_reference = {
            "path": repo_relative(reference_path, REPO_ROOT),
            "sha256": sha256_file(reference_path),
            "result": "exact-match",
        }
        print(f"ABI reference: exact match with {reference_path}")

    source_inputs = input_records(run_a["inputs"], REPO_ROOT)
    replacements = {
        str(clang): "$CLANG" if clang else "",
        str(llvm_spirv): "$LLVM_SPIRV" if llvm_spirv else "",
        str(ocloc): "$OCLOC",
        str(source.parent): "$SOURCE_DIR",
        str(source): f"$SOURCE_DIR/{source.name}",
        str(run_root / "run-a"): "$OUT_DIR",
    }
    replacements = {key: value for key, value in replacements.items() if key}
    normalized_commands = [
        _display_command(command, replacements) for command in run_a["commands"]
    ]
    manifest = build_manifest(
        analysis=analysis,
        source={
            "path": repo_relative(source, REPO_ROOT),
            "language": "C++ for OpenCL" if frontend == "clang-clcpp" else "OpenCL C",
            "inputs": source_inputs,
        },
        target={
            "label": profile["label"],
            "ocloc_device": str(profile["device"]),
            "pci_device_ids": [
                int(value, 0) if isinstance(value, str) else int(value)
                for value in profile["pci_device_ids"]
            ],
            "revision_min": int(profile["revision_min"]),
            "revision_max": int(profile["revision_max"]),
        },
        variant=variant,
        provenance={
            "frontend": {
                "description": (
                    "Clang C++ for OpenCL -> LLVM bitcode -> llvm-spirv"
                    if frontend == "clang-clcpp"
                    else "ocloc OpenCL C frontend"
                )
            },
            "backend": {"description": "Intel IGC through ocloc -spirv_input" if frontend == "clang-clcpp" else "Intel IGC through ocloc"},
            "commands": normalized_commands,
            "toolchain": fingerprint,
            "profile": {
                "path": repo_relative(profile_path, REPO_ROOT),
                "sha256": sha256_file(profile_path),
            },
            "reproducibility_check": "passed" if args.repro_check else "not-requested",
        },
        abi_reference=abi_reference,
        rust_symbols=_rust_symbol_map(args.rust_symbol),
    )
    manifest_path = run_a["binary"].with_suffix(".manifest.json")
    contract_path = run_a["binary"].with_suffix(".contract.rs")
    atomic_write(manifest_path, stable_json(manifest))
    atomic_write(contract_path, render_rust_contracts(manifest))

    if args.write_toolchain_lock:
        atomic_write(
            args.write_toolchain_lock.expanduser().resolve(),
            stable_json(_lock_projection(fingerprint)),
        )

    if args.publish_dir:
        publish_dir = args.publish_dir.expanduser().resolve()
        publish_dir.mkdir(parents=True, exist_ok=True)
        for source_path in (
            run_a["binary"],
            run_a["spirv"],
            manifest_path,
            contract_path,
        ):
            _publish(source_path, publish_dir / source_path.name)
        print(f"published: {publish_dir}")
    else:
        print(f"no-publish output: {run_root / 'run-a'}")
    print(
        f"artifact SHA-256: bin={analysis['elf']['sha256']} "
        f"spv={analysis['spirv']['sha256']}"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ContractError as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2)
