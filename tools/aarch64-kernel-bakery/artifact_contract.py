#!/usr/bin/env python3
"""Standard-library-only contracts for freestanding AArch64 kernel objects."""

from __future__ import annotations

import hashlib
import json
import struct
from pathlib import Path
from typing import Any, Iterable


SCHEMA_VERSION = 1
BACKEND = "aarch64-cpu-aot"
ELF_MACHINE_AARCH64 = 183
SHN_UNDEF = 0
SHT_SYMTAB = 2
SHT_NOBITS = 8
STB_GLOBAL = 1
STB_WEAK = 2
STT_FUNC = 2
SHF_EXECINSTR = 0x4


class ContractError(RuntimeError):
    """An AArch64 object does not satisfy the freestanding artifact contract."""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def stable_json(value: Any) -> str:
    return json.dumps(value, indent=2, sort_keys=True) + "\n"


def atomic_write(path: Path, data: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.write_text(data, encoding="utf-8")
    temporary.replace(path)


def repo_relative(path: Path, repo_root: Path) -> str:
    resolved = path.resolve()
    try:
        return resolved.relative_to(repo_root.resolve()).as_posix()
    except ValueError:
        return resolved.as_posix()


def input_records(paths: Iterable[Path], repo_root: Path) -> list[dict[str, Any]]:
    records: dict[Path, dict[str, Any]] = {}
    for path in paths:
        resolved = path.resolve()
        if not resolved.is_file():
            raise ContractError(f"compiler input does not exist: {path}")
        records[resolved] = {
            "path": repo_relative(resolved, repo_root),
            "sha256": sha256_file(resolved),
            "size_bytes": resolved.stat().st_size,
        }
    return sorted(records.values(), key=lambda item: item["path"])


def _bounded_slice(data: bytes, offset: int, size: int, description: str) -> bytes:
    if offset < 0 or size < 0 or offset + size > len(data):
        raise ContractError(
            f"{description} is outside the file: offset={offset} size={size} "
            f"file_size={len(data)}"
        )
    return data[offset : offset + size]


def _cstring(table: bytes, offset: int, description: str) -> str:
    if offset < 0 or offset >= len(table):
        raise ContractError(f"{description} has invalid string offset {offset}")
    end = table.find(b"\0", offset)
    if end < 0:
        raise ContractError(f"{description} is not NUL terminated")
    try:
        return table[offset:end].decode("utf-8")
    except UnicodeDecodeError as error:
        raise ContractError(f"{description} is not UTF-8") from error


def analyze_object(
    path: Path,
    *,
    expected_machine: int,
    expected_entries: Iterable[str],
) -> dict[str, Any]:
    data = path.read_bytes()
    if len(data) < 64 or data[:4] != b"\x7fELF":
        raise ContractError(f"{path}: not an ELF file")
    if data[4] != 2:
        raise ContractError(f"{path}: expected ELFCLASS64, got {data[4]}")
    if data[5] != 1:
        raise ContractError(f"{path}: expected little-endian ELF, got EI_DATA={data[5]}")
    if data[6] != 1:
        raise ContractError(f"{path}: unsupported ELF version {data[6]}")

    header = struct.unpack_from("<HHIQQQIHHHHHH", data, 16)
    (
        elf_type,
        machine,
        elf_version,
        _entry,
        _program_header_offset,
        section_header_offset,
        flags,
        elf_header_size,
        _program_header_entry_size,
        _program_header_count,
        section_header_entry_size,
        section_count,
        section_name_index,
    ) = header
    if elf_type != 1:
        raise ContractError(f"{path}: expected relocatable ELF (ET_REL), got {elf_type}")
    if machine != expected_machine:
        raise ContractError(
            f"{path}: expected ELF machine {expected_machine}, got {machine}"
        )
    if elf_version != 1 or elf_header_size != 64:
        raise ContractError(f"{path}: malformed ELF64 header")
    if section_header_entry_size != 64 or section_count == 0:
        raise ContractError(f"{path}: missing or unsupported section table")
    if section_name_index >= section_count:
        raise ContractError(f"{path}: invalid section-name table index")

    raw_sections: list[tuple[int, ...]] = []
    for index in range(section_count):
        offset = section_header_offset + index * section_header_entry_size
        _bounded_slice(data, offset, 64, f"section header {index}")
        raw_sections.append(struct.unpack_from("<IIQQQQIIQQ", data, offset))

    name_section = raw_sections[section_name_index]
    section_names = _bounded_slice(
        data, name_section[4], name_section[5], "section-name table"
    )
    sections: list[dict[str, Any]] = []
    for index, section in enumerate(raw_sections):
        (
            name_offset,
            section_type,
            section_flags,
            address,
            offset,
            size,
            link,
            info,
            alignment,
            entry_size,
        ) = section
        name = _cstring(section_names, name_offset, f"section {index} name")
        if section_type != SHT_NOBITS:
            _bounded_slice(data, offset, size, f"section {name or index}")
        sections.append(
            {
                "index": index,
                "name": name,
                "type": section_type,
                "flags": section_flags,
                "address": address,
                "offset": offset,
                "size": size,
                "link": link,
                "info": info,
                "alignment": alignment,
                "entry_size": entry_size,
            }
        )

    forbidden_sections = sorted(
        section["name"]
        for section in sections
        if section["size"] > 0
        and section["name"]
        in (
            ".ctors",
            ".dtors",
            ".fini_array",
            ".init_array",
            ".tbss",
            ".tdata",
        )
    )
    if forbidden_sections:
        raise ContractError(
            f"{path}: freestanding object has forbidden runtime sections: "
            f"{', '.join(forbidden_sections)}"
        )

    symbol_sections = [section for section in sections if section["type"] == SHT_SYMTAB]
    if len(symbol_sections) != 1:
        raise ContractError(
            f"{path}: expected exactly one symbol table, got {len(symbol_sections)}"
        )
    symbol_section = symbol_sections[0]
    if symbol_section["entry_size"] != 24 or symbol_section["size"] % 24:
        raise ContractError(f"{path}: unsupported ELF64 symbol table")
    if symbol_section["link"] >= len(sections):
        raise ContractError(f"{path}: symbol string table index is invalid")
    string_section = sections[symbol_section["link"]]
    string_table = _bounded_slice(
        data, string_section["offset"], string_section["size"], "symbol string table"
    )

    symbols: list[dict[str, Any]] = []
    for index in range(symbol_section["size"] // 24):
        offset = symbol_section["offset"] + index * 24
        name_offset, info, other, section_index, value, size = struct.unpack_from(
            "<IBBHQQ", data, offset
        )
        name = _cstring(string_table, name_offset, f"symbol {index} name")
        symbols.append(
            {
                "index": index,
                "name": name,
                "binding": info >> 4,
                "type": info & 0x0F,
                "visibility": other & 0x03,
                "section_index": section_index,
                "value": value,
                "size": size,
            }
        )

    undefined = sorted(
        {
            symbol["name"]
            for symbol in symbols
            if symbol["name"]
            and symbol["section_index"] == SHN_UNDEF
            and symbol["binding"] in (STB_GLOBAL, STB_WEAK)
        }
    )
    if undefined:
        raise ContractError(
            f"{path}: freestanding object has undefined symbols: {', '.join(undefined)}"
        )

    entries: list[dict[str, Any]] = []
    requested = list(dict.fromkeys(expected_entries))
    if not requested:
        raise ContractError(f"{path}: at least one expected entry is required")
    exported = sorted(
        {
            symbol["name"]
            for symbol in symbols
            if symbol["name"]
            and symbol["section_index"] != SHN_UNDEF
            and symbol["binding"] in (STB_GLOBAL, STB_WEAK)
        }
    )
    unexpected_exports = sorted(set(exported) - set(requested))
    if unexpected_exports:
        raise ContractError(
            f"{path}: object has unexpected exported symbols: "
            f"{', '.join(unexpected_exports)}"
        )
    for name in requested:
        matches = [
            symbol
            for symbol in symbols
            if symbol["name"] == name
            and symbol["binding"] == STB_GLOBAL
            and symbol["type"] == STT_FUNC
            and symbol["section_index"] != SHN_UNDEF
        ]
        if len(matches) != 1:
            raise ContractError(
                f"{path}: expected exactly one global function {name}, got {len(matches)}"
            )
        symbol = matches[0]
        if symbol["visibility"] != 0:
            raise ContractError(f"{path}: entry {name} must have default visibility")
        if symbol["section_index"] >= len(sections):
            raise ContractError(f"{path}: entry {name} has invalid section index")
        section = sections[symbol["section_index"]]
        if not section["flags"] & SHF_EXECINSTR:
            raise ContractError(f"{path}: entry {name} is not in executable code")
        if symbol["size"] <= 0 or symbol["value"] + symbol["size"] > section["size"]:
            raise ContractError(f"{path}: entry {name} has an invalid code range")
        code = _bounded_slice(
            data,
            section["offset"] + symbol["value"],
            symbol["size"],
            f"entry {name}",
        )
        entries.append(
            {
                "name": name,
                "section": section["name"],
                "section_offset": section["offset"],
                "entry_offset": section["offset"] + symbol["value"],
                "entry_size": symbol["size"],
                "entry_sha256": hashlib.sha256(code).hexdigest(),
            }
        )

    return {
        "elf_class": 64,
        "endianness": "little",
        "elf_type": elf_type,
        "elf_machine": machine,
        "elf_flags": flags,
        "entries": entries,
        "exported_symbols": exported,
        "undefined_symbols": undefined,
    }


def verify_manifest(
    manifest_path: Path,
    object_path: Path,
    *,
    repo_root: Path,
) -> dict[str, Any]:
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"cannot read manifest {manifest_path}: {error}") from error
    if manifest.get("schema_version") != SCHEMA_VERSION:
        raise ContractError(f"{manifest_path}: unsupported schema")
    if manifest.get("backend") != BACKEND:
        raise ContractError(f"{manifest_path}: unexpected backend")

    artifact = manifest.get("artifact")
    if not isinstance(artifact, dict):
        raise ContractError(f"{manifest_path}: artifact record is missing")
    if artifact.get("file") != object_path.name:
        raise ContractError(f"{manifest_path}: object filename is stale")
    if not object_path.is_file():
        raise ContractError(f"{object_path}: object is missing")
    if artifact.get("size_bytes") != object_path.stat().st_size:
        raise ContractError(f"{object_path}: object size changed")
    if artifact.get("sha256") != sha256_file(object_path):
        raise ContractError(f"{object_path}: object hash changed")

    target = manifest.get("target")
    if (
        not isinstance(target, dict)
        or target.get("elf_machine") != ELF_MACHINE_AARCH64
        or target.get("architecture") != "aarch64"
        or target.get("endianness") != "little"
        or target.get("abi") != "AAPCS64"
        or not str(target.get("target_triple", "")).startswith("aarch64")
    ):
        raise ContractError(f"{manifest_path}: AArch64 target record is missing")
    expected_entries = manifest.get("expected_entries")
    if not isinstance(expected_entries, list) or not expected_entries:
        raise ContractError(f"{manifest_path}: expected entry set is missing")
    analysis = analyze_object(
        object_path,
        expected_machine=target["elf_machine"],
        expected_entries=expected_entries,
    )
    if manifest.get("analysis") != analysis:
        raise ContractError(f"{manifest_path}: recorded ELF analysis is stale")

    inputs = manifest.get("inputs")
    if not isinstance(inputs, list) or not inputs:
        raise ContractError(f"{manifest_path}: compiler input records are missing")
    for record in inputs:
        if not isinstance(record, dict) or not isinstance(record.get("path"), str):
            raise ContractError(f"{manifest_path}: malformed compiler input record")
        path = Path(record["path"])
        if not path.is_absolute():
            path = repo_root / path
        if not path.is_file():
            raise ContractError(f"{manifest_path}: compiler input is missing: {path}")
        if record.get("size_bytes") != path.stat().st_size:
            raise ContractError(f"{manifest_path}: compiler input size changed: {path}")
        if record.get("sha256") != sha256_file(path):
            raise ContractError(f"{manifest_path}: compiler input hash changed: {path}")

    profile_record = manifest.get("profile")
    if (
        not isinstance(profile_record, dict)
        or not isinstance(profile_record.get("path"), str)
        or not isinstance(profile_record.get("sha256"), str)
    ):
        raise ContractError(f"{manifest_path}: target profile record is missing")
    profile_path = Path(profile_record["path"])
    if not profile_path.is_absolute():
        profile_path = repo_root / profile_path
    if not profile_path.is_file():
        raise ContractError(f"{manifest_path}: target profile is missing")
    if profile_record["sha256"] != sha256_file(profile_path):
        raise ContractError(f"{manifest_path}: target profile changed")
    try:
        profile = json.loads(profile_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"{manifest_path}: cannot read target profile") from error
    expected_target = {
        "label": profile.get("label"),
        "target_triple": profile.get("target_triple"),
        "architecture": profile.get("architecture"),
        "abi": profile.get("abi"),
        "elf_machine": profile.get("elf_machine"),
        "endianness": "little",
    }
    if target != expected_target:
        raise ContractError(f"{manifest_path}: target does not match profile")
    if profile_record.get("clang_options") != profile.get("clang_options"):
        raise ContractError(f"{manifest_path}: recorded Clang options are stale")

    provenance = manifest.get("provenance")
    toolchain = provenance.get("toolchain") if isinstance(provenance, dict) else None
    if (
        not isinstance(toolchain, dict)
        or toolchain.get("backend") != BACKEND
        or toolchain.get("target_triple") != target["target_triple"]
        or toolchain.get("profile_sha256") != profile_record["sha256"]
        or not isinstance(toolchain.get("clang"), dict)
        or not isinstance(toolchain["clang"].get("executable_sha256"), str)
        or not isinstance(toolchain["clang"].get("version"), str)
    ):
        raise ContractError(f"{manifest_path}: toolchain provenance is missing")
    command = provenance.get("command")
    if (
        not isinstance(command, list)
        or not command
        or command[0] != "$CLANG"
        or f"--target={target['target_triple']}" not in command
    ):
        raise ContractError(f"{manifest_path}: normalized compile command is missing")

    reproducibility = manifest.get("reproducibility")
    if (
        not isinstance(reproducibility, dict)
        or reproducibility.get("checked") is not True
        or reproducibility.get("object_identical") is not True
    ):
        raise ContractError(f"{manifest_path}: reproducibility proof is missing")
    return manifest
