#!/usr/bin/env python3
"""Cloud source proof for the TRUEOS SIMD16 parallel-u32 incubator."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

TOOL_DIR = Path(__file__).resolve().parent
REPO_ROOT = TOOL_DIR.parent.parent
BUILD_ROOT = (REPO_ROOT / "bld").resolve()
CONTRACT_PATH = TOOL_DIR / "semantic-contract-v1.json"
HEADER_PATH = TOOL_DIR / "include" / "trueos_parallel_u32.hpp"
BANNED_DEVICE_TOKENS = (
    "__local",
    "barrier(",
    "work_group_barrier(",
    "sub_group_barrier(",
    "atomic_",
    "malloc(",
)


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser()
    result.add_argument(
        "--out-dir",
        type=Path,
        default=BUILD_ROOT / "intel-gpu-primitives-cloud",
    )
    result.add_argument("--clang", default=os.environ.get("CLANG", "clang"))
    result.add_argument("--llvm-spirv", default=os.environ.get("LLVM_SPIRV"))
    return result


def validate_output_root(path: Path) -> Path:
    output_root = path.expanduser().resolve()
    try:
        relative = output_root.relative_to(BUILD_ROOT)
    except ValueError as error:
        raise RuntimeError(
            f"output directory must be below the repository build root {BUILD_ROOT}"
        ) from error
    if output_root == BUILD_ROOT or not relative.parts:
        raise RuntimeError("refusing to use the build root itself as output")
    return output_root


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run(command: list[str], *, cwd: Path) -> subprocess.CompletedProcess[str]:
    process = subprocess.run(
        command,
        cwd=cwd,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
        env={
            "LANG": "C",
            "LC_ALL": "C",
            "PATH": os.environ.get("PATH", os.defpath),
            "SOURCE_DATE_EPOCH": "0",
            "TZ": "UTC",
        },
    )
    if process.returncode:
        rendered = " ".join(command)
        raise RuntimeError(
            f"command failed ({process.returncode}): {rendered}\n{process.stdout}"
        )
    return process


def load_contract() -> dict[str, object]:
    contract = json.loads(CONTRACT_PATH.read_text(encoding="utf-8"))
    if contract.get("schema_version") != 1:
        raise RuntimeError("unsupported semantic contract")
    return contract


def validate_sources(contract: dict[str, object]) -> list[tuple[Path, list[str]]]:
    artifacts = contract.get("artifacts")
    if not isinstance(artifacts, list) or not artifacts:
        raise RuntimeError("semantic contract has no artifacts")

    result: list[tuple[Path, list[str]]] = []
    seen_entries: set[str] = set()
    for record in artifacts:
        if not isinstance(record, dict):
            raise RuntimeError("malformed artifact record")
        source_name = record.get("source")
        entries = record.get("entries")
        if not isinstance(source_name, str) or not isinstance(entries, list):
            raise RuntimeError("malformed source/entry record")
        source = TOOL_DIR / source_name
        if not source.is_file():
            raise RuntimeError(f"missing source {source}")
        entry_names = []
        for entry in entries:
            if not isinstance(entry, str) or not re.fullmatch(
                r"[A-Za-z_][A-Za-z0-9_]*", entry
            ):
                raise RuntimeError(f"invalid kernel entry {entry!r}")
            if entry in seen_entries:
                raise RuntimeError(f"duplicate kernel entry {entry}")
            seen_entries.add(entry)
            entry_names.append(entry)

        text = source.read_text(encoding="utf-8")
        for token in BANNED_DEVICE_TOKENS:
            if token in text:
                raise RuntimeError(f"{source.name}: forbidden v1 token {token!r}")
        result.append((source, entry_names))

    header_text = HEADER_PATH.read_text(encoding="utf-8")
    for token in BANNED_DEVICE_TOKENS:
        if token in header_text:
            raise RuntimeError(f"{HEADER_PATH.name}: forbidden v1 token {token!r}")
    return result


def compile_source(
    *, clang: str, source: Path, output_root: Path, emit_text_ir: bool
) -> tuple[Path, Path | None, list[str]]:
    output_root.mkdir(parents=True, exist_ok=True)
    bitcode = output_root / f"{source.stem}.bc"
    text_ir = output_root / f"{source.stem}.ll" if emit_text_ir else None
    common = [
        clang,
        "--target=spir64",
        "-x",
        "clcpp",
        "-cl-std=CLC++",
        "-cl-kernel-arg-info",
        "-fno-discard-value-names",
        "-Wall",
        "-Wextra",
        "-Werror",
        "-Wdate-time",
        "-fno-exceptions",
        "-fno-rtti",
        "-emit-llvm",
    ]
    bitcode_command = [*common, "-c", source.name, "-o", str(bitcode)]
    run(bitcode_command, cwd=TOOL_DIR)
    normalized_common = ["$CLANG" if item == clang else item for item in common]
    commands = [
        " ".join(
            [
                *normalized_common,
                "-c",
                source.name,
                "-o",
                f"$OUT_DIR/{bitcode.name}",
            ]
        )
    ]
    if text_ir is not None:
        ir_command = [*common, "-S", source.name, "-o", str(text_ir)]
        run(ir_command, cwd=TOOL_DIR)
        commands.append(
            " ".join(
                [
                    *normalized_common,
                    "-S",
                    source.name,
                    "-o",
                    f"$OUT_DIR/{text_ir.name}",
                ]
            )
        )
    return bitcode, text_ir, commands


def verify_ir(text_ir: Path, entries: list[str]) -> None:
    text = text_ir.read_text(encoding="utf-8")
    definitions = set(
        re.findall(
            r"^define[^\n]*\bspir_kernel\b[^\n]*@([A-Za-z_][A-Za-z0-9_]*)\(",
            text,
            re.MULTILINE,
        )
    )
    expected = set(entries)
    if definitions != expected:
        raise RuntimeError(
            f"{text_ir.name}: kernel set mismatch "
            f"expected={sorted(expected)} observed={sorted(definitions)}"
        )

    subgroup_metadata = {
        node: int(value)
        for node, value in re.findall(
            r"^!(\d+)\s*=\s*!\{\s*i32\s+(\d+)\s*\}\s*$",
            text,
            re.MULTILINE,
        )
    }
    lines = text.splitlines()
    for entry in entries:
        line = next(
            (
                candidate
                for candidate in lines
                if "spir_kernel" in candidate and f"@{entry}(" in candidate
            ),
            "",
        )
        attachment = re.search(r"!intel_reqd_sub_group_size\s+!(\d+)", line)
        if attachment is None:
            raise RuntimeError(f"{text_ir.name}: {entry} lacks subgroup metadata")
        node = attachment.group(1)
        value = subgroup_metadata.get(node)
        if value != 16:
            rendered = "missing" if value is None else str(value)
            raise RuntimeError(
                f"{text_ir.name}: {entry} requires subgroup size {rendered}, expected 16"
            )


def translate_optional(
    translator: str | None,
    run_a: Path,
    run_b: Path,
    output_root: Path,
) -> tuple[str, str] | None:
    if not translator:
        return None
    executable = shutil.which(translator) if not Path(translator).is_file() else translator
    if not executable:
        raise RuntimeError(f"requested llvm-spirv is not executable: {translator}")
    output_a = output_root / "run-a" / f"{run_a.stem}.spv"
    output_b = output_root / "different-root" / "run-b" / f"{run_b.stem}.spv"
    options = ["--preserve-ocl-kernel-arg-type-metadata-through-string"]
    run(
        [str(executable), *options, str(run_a), "-o", str(output_a)],
        cwd=TOOL_DIR,
    )
    run(
        [str(executable), *options, str(run_b), "-o", str(output_b)],
        cwd=TOOL_DIR,
    )
    digest_a = sha256_file(output_a)
    digest_b = sha256_file(output_b)
    if digest_a != digest_b:
        raise RuntimeError(f"{run_a.stem}: two-root SPIR-V mismatch")
    return str(output_a.relative_to(output_root)), digest_a


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    clang = shutil.which(args.clang) if not Path(args.clang).is_file() else args.clang
    if not clang:
        raise RuntimeError(f"missing clang executable: {args.clang}")

    contract = load_contract()
    sources = validate_sources(contract)
    output_root = validate_output_root(args.out_dir)
    if output_root.exists():
        shutil.rmtree(output_root)
    run_a_root = output_root / "run-a"
    run_b_root = output_root / "different-root" / "run-b"

    compiler_version = run([str(clang), "--version"], cwd=TOOL_DIR).stdout.splitlines()[0]
    manifest: dict[str, object] = {
        "schema_version": 1,
        "proof": "trueos-intel-gpu-primitives-cloud-source-v1",
        "compiler": compiler_version,
        "semantic_contract": {
            "path": str(CONTRACT_PATH.relative_to(TOOL_DIR)),
            "sha256": sha256_file(CONTRACT_PATH),
        },
        "header": {
            "path": str(HEADER_PATH.relative_to(TOOL_DIR)),
            "sha256": sha256_file(HEADER_PATH),
        },
        "artifacts": [],
    }

    artifact_records: list[dict[str, object]] = []
    for source, entries in sources:
        bitcode_a, text_ir, commands = compile_source(
            clang=str(clang),
            source=source,
            output_root=run_a_root,
            emit_text_ir=True,
        )
        bitcode_b, _, _ = compile_source(
            clang=str(clang),
            source=source,
            output_root=run_b_root,
            emit_text_ir=False,
        )
        digest_a = sha256_file(bitcode_a)
        digest_b = sha256_file(bitcode_b)
        if digest_a != digest_b:
            raise RuntimeError(f"{source.name}: two-root LLVM bitcode mismatch")
        assert text_ir is not None
        verify_ir(text_ir, entries)
        spirv = translate_optional(args.llvm_spirv, bitcode_a, bitcode_b, output_root)
        artifact_records.append(
            {
                "source": source.name,
                "source_sha256": sha256_file(source),
                "entries": entries,
                "bitcode": str(bitcode_a.relative_to(output_root)),
                "bitcode_sha256": digest_a,
                "text_ir": str(text_ir.relative_to(output_root)),
                "commands": commands,
                "spirv": None
                if spirv is None
                else {"path": spirv[0], "sha256": spirv[1]},
                "two_root_reproducible": True,
            }
        )

    run(
        [
            sys.executable,
            "-B",
            "-m",
            "unittest",
            "discover",
            "-s",
            str(TOOL_DIR),
            "-p",
            "test_*.py",
        ],
        cwd=TOOL_DIR,
    )
    manifest["artifacts"] = artifact_records
    manifest["semantic_tests"] = "passed"
    output_root.mkdir(parents=True, exist_ok=True)
    manifest_path = output_root / "manifest.json"
    manifest_path.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    print(
        f"intel-gpu-primitives: verified {len(artifact_records)} sources, "
        f"{sum(len(record['entries']) for record in artifact_records)} entries, "
        f"manifest={manifest_path}"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)
