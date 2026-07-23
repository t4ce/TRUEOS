#!/usr/bin/env python3
"""Intel Zebin ELF/.ze_info inspection and TRUEOS contract generation.

This module intentionally uses only the Python standard library.  The TRUEOS
kernel consumes the generated Rust literal; it never parses JSON, ELF, or YAML.
"""

from __future__ import annotations

import hashlib
import json
import re
import struct
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


SCHEMA_VERSION = 1
ELF_MACHINE_INTEL_GT = 0xCD


class ContractError(RuntimeError):
    """An artifact cannot be represented by the TRUEOS direct-RCS contract."""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def atomic_write(path: Path, data: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    temporary.write_text(data, encoding="utf-8")
    temporary.replace(path)


def stable_json(value: Any) -> str:
    return json.dumps(value, indent=2, sort_keys=True) + "\n"


def _bounded_slice(data: bytes, offset: int, size: int, description: str) -> bytes:
    if offset < 0 or size < 0 or offset + size > len(data):
        raise ContractError(
            f"{description} is outside the file: offset={offset} size={size} "
            f"file_size={len(data)}"
        )
    return data[offset : offset + size]


def _cstring(table: bytes, offset: int, description: str) -> str:
    if offset < 0 or offset >= len(table):
        raise ContractError(f"{description} string offset {offset} is invalid")
    end = table.find(b"\0", offset)
    if end < 0:
        raise ContractError(f"{description} is not NUL terminated")
    return table[offset:end].decode("utf-8", errors="strict")


@dataclass(frozen=True)
class ElfSection:
    index: int
    name: str
    section_type: int
    flags: int
    offset: int
    size: int
    link: int
    info: int
    alignment: int
    entry_size: int
    data: bytes


@dataclass(frozen=True)
class ElfSymbol:
    name: str
    symbol_type: int
    binding: int
    section_index: int
    value: int
    size: int


@dataclass(frozen=True)
class Zebin:
    path: Path
    data: bytes
    elf_type: int
    machine: int
    sections: tuple[ElfSection, ...]
    symbols: tuple[ElfSymbol, ...]

    @classmethod
    def load(cls, path: Path) -> "Zebin":
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
            _flags,
            elf_header_size,
            _program_header_entry_size,
            _program_header_count,
            section_header_entry_size,
            section_count,
            section_name_index,
        ) = header
        if elf_header_size != 64 or elf_version != 1:
            raise ContractError(f"{path}: malformed ELF64 header")
        if elf_type != 1:
            raise ContractError(f"{path}: expected relocatable ELF (ET_REL), got {elf_type}")
        if machine != ELF_MACHINE_INTEL_GT:
            raise ContractError(
                f"{path}: expected Intel Graphics ELF machine 0x{ELF_MACHINE_INTEL_GT:x}, "
                f"got 0x{machine:x}"
            )
        if section_header_entry_size != 64 or section_count == 0:
            raise ContractError(f"{path}: unsupported or missing section table")
        if section_name_index >= section_count:
            raise ContractError(f"{path}: invalid section-name table index")

        raw_sections: list[tuple[int, ...]] = []
        for index in range(section_count):
            offset = section_header_offset + index * section_header_entry_size
            raw = _bounded_slice(data, offset, 64, f"section header {index}")
            raw_sections.append(struct.unpack("<IIQQQQIIQQ", raw))

        string_header = raw_sections[section_name_index]
        section_names = _bounded_slice(
            data, string_header[4], string_header[5], "section-name string table"
        )
        sections: list[ElfSection] = []
        for index, raw in enumerate(raw_sections):
            (
                name_offset,
                section_type,
                flags,
                _address,
                offset,
                size,
                link,
                info,
                alignment,
                entry_size,
            ) = raw
            name = _cstring(section_names, name_offset, f"section {index} name")
            payload = _bounded_slice(data, offset, size, f"section {name or index}")
            sections.append(
                ElfSection(
                    index=index,
                    name=name,
                    section_type=section_type,
                    flags=flags,
                    offset=offset,
                    size=size,
                    link=link,
                    info=info,
                    alignment=alignment,
                    entry_size=entry_size,
                    data=payload,
                )
            )

        symbols: list[ElfSymbol] = []
        for section in sections:
            if section.section_type != 2:  # SHT_SYMTAB
                continue
            if section.entry_size != 24 or section.size % section.entry_size != 0:
                raise ContractError(f"{path}: malformed symbol table {section.name}")
            if section.link >= len(sections):
                raise ContractError(f"{path}: symbol table has invalid string-table link")
            symbol_names = sections[section.link].data
            for offset in range(0, section.size, section.entry_size):
                name_offset, info, _other, section_index, value, size = struct.unpack_from(
                    "<IBBHQQ", section.data, offset
                )
                symbols.append(
                    ElfSymbol(
                        name=_cstring(symbol_names, name_offset, "symbol name"),
                        symbol_type=info & 0x0F,
                        binding=info >> 4,
                        section_index=section_index,
                        value=value,
                        size=size,
                    )
                )

        return cls(
            path=path,
            data=data,
            elf_type=elf_type,
            machine=machine,
            sections=tuple(sections),
            symbols=tuple(symbols),
        )

    def one_section(self, name: str) -> ElfSection:
        matches = [section for section in self.sections if section.name == name]
        if len(matches) != 1:
            raise ContractError(
                f"{self.path}: expected exactly one {name!r} section, got {len(matches)}"
            )
        return matches[0]


_INTEGER = re.compile(r"^-?[0-9]+$")
_HEX_INTEGER = re.compile(r"^0x[0-9a-fA-F]+$")


def _yaml_scalar(text: str) -> Any:
    text = text.strip()
    if not text:
        return None
    if len(text) >= 2 and text[0] == text[-1] and text[0] in "'\"":
        return text[1:-1]
    if text == "true":
        return True
    if text == "false":
        return False
    if text in ("null", "~"):
        return None
    if _INTEGER.fullmatch(text):
        return int(text, 10)
    if _HEX_INTEGER.fullmatch(text):
        return int(text, 16)
    if text.startswith("[") and text.endswith("]"):
        body = text[1:-1].strip()
        return [] if not body else [_yaml_scalar(part) for part in body.split(",")]
    return text


def _split_yaml_mapping(text: str) -> tuple[str, str]:
    if ":" not in text:
        raise ContractError(f"unsupported .ze_info YAML line: {text!r}")
    key, value = text.split(":", 1)
    key = key.strip()
    if not key:
        raise ContractError(f"empty .ze_info YAML key: {text!r}")
    return key, value.strip()


def parse_ze_info_yaml(text: str) -> dict[str, Any]:
    """Parse the mapping/list/scalar YAML subset emitted in Zebin .ze_info."""

    tokens: list[tuple[int, str]] = []
    for raw_line in text.replace("\0", "").splitlines():
        if not raw_line.strip() or raw_line.strip() in ("---", "..."):
            continue
        if "\t" in raw_line[: len(raw_line) - len(raw_line.lstrip())]:
            raise ContractError("tabs are unsupported in .ze_info indentation")
        tokens.append((len(raw_line) - len(raw_line.lstrip(" ")), raw_line.strip()))
    if not tokens:
        raise ContractError("empty .ze_info section")

    def parse_block(index: int, indent: int) -> tuple[Any, int]:
        if index >= len(tokens) or tokens[index][0] != indent:
            raise ContractError("invalid .ze_info indentation")
        is_list = tokens[index][1].startswith("-")
        value: Any = [] if is_list else {}

        while index < len(tokens):
            line_indent, content = tokens[index]
            if line_indent < indent:
                break
            if line_indent > indent:
                raise ContractError(
                    f"unexpected .ze_info indentation at {content!r}: "
                    f"wanted {indent}, got {line_indent}"
                )
            if is_list:
                if not content.startswith("-"):
                    break
                item_text = content[1:].strip()
                index += 1
                if not item_text:
                    if index >= len(tokens) or tokens[index][0] <= indent:
                        value.append(None)
                    else:
                        child, index = parse_block(index, tokens[index][0])
                        value.append(child)
                    continue
                if ":" not in item_text:
                    value.append(_yaml_scalar(item_text))
                    continue
                key, scalar_text = _split_yaml_mapping(item_text)
                item: dict[str, Any] = {key: _yaml_scalar(scalar_text)}
                if scalar_text == "":
                    if index >= len(tokens) or tokens[index][0] <= indent:
                        item[key] = None
                    else:
                        item[key], index = parse_block(index, tokens[index][0])
                if index < len(tokens) and tokens[index][0] > indent:
                    continuation_indent = tokens[index][0]
                    continuation, index = parse_block(index, continuation_indent)
                    if not isinstance(continuation, dict):
                        raise ContractError("list mapping continuation must be a mapping")
                    overlap = set(item).intersection(continuation)
                    if overlap:
                        raise ContractError(f"duplicate .ze_info keys: {sorted(overlap)}")
                    item.update(continuation)
                value.append(item)
            else:
                if content.startswith("-"):
                    break
                key, scalar_text = _split_yaml_mapping(content)
                if key in value:
                    raise ContractError(f"duplicate .ze_info key {key!r}")
                index += 1
                if scalar_text:
                    value[key] = _yaml_scalar(scalar_text)
                elif index < len(tokens) and tokens[index][0] > indent:
                    value[key], index = parse_block(index, tokens[index][0])
                else:
                    value[key] = None
        return value, index

    parsed, final_index = parse_block(0, tokens[0][0])
    if final_index != len(tokens) or not isinstance(parsed, dict):
        raise ContractError("could not parse complete .ze_info mapping")
    return parsed


def _integer(mapping: dict[str, Any], key: str, default: int | None = None) -> int:
    value = mapping.get(key, default)
    if not isinstance(value, int) or isinstance(value, bool):
        raise ContractError(f".ze_info {key!r} must be an integer, got {value!r}")
    return value


def _list(mapping: dict[str, Any], key: str) -> list[Any]:
    value = mapping.get(key, [])
    if value is None:
        return []
    if not isinstance(value, list):
        raise ContractError(f".ze_info {key!r} must be a list")
    return value


def _mapping(value: Any, description: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ContractError(f"{description} must be a mapping")
    return value


def _version_parts(value: Any) -> tuple[int, int]:
    if not isinstance(value, str) or not re.fullmatch(r"[0-9]+\.[0-9]+", value):
        raise ContractError(f"unsupported .ze_info version value {value!r}")
    major, minor = value.split(".", 1)
    return int(major), int(minor)


def _align_up(value: int, alignment: int) -> int:
    return (value + alignment - 1) // alignment * alignment


def _source_arg_info(root: dict[str, Any], kernel_name: str) -> list[dict[str, Any]]:
    misc_matches = []
    for item in _list(root, "kernels_misc_info"):
        mapping = _mapping(item, "kernels_misc_info item")
        if mapping.get("name") == kernel_name:
            misc_matches.append(mapping)
    if len(misc_matches) != 1:
        raise ContractError(
            f"kernel {kernel_name!r}: expected one kernels_misc_info record, "
            f"got {len(misc_matches)}; compile with -cl-kernel-arg-info and preserve "
            "OpenCL kernel argument metadata"
        )
    result = []
    seen: set[int] = set()
    for item in _list(misc_matches[0], "args_info"):
        arg = _mapping(item, "args_info item")
        index = _integer(arg, "index")
        if index in seen:
            raise ContractError(f"kernel {kernel_name!r}: duplicate args_info index {index}")
        seen.add(index)
        result.append(
            {
                "index": index,
                "name": str(arg.get("name", "")),
                "address_qualifier": str(arg.get("address_qualifier", "")),
                "access_qualifier": str(arg.get("access_qualifier", "")),
                "type_name": str(arg.get("type_name", "")),
                "type_qualifiers": str(arg.get("type_qualifiers", "")),
            }
        )
    result.sort(key=lambda arg: arg["index"])
    return result


def _scratch_bytes(kernel: dict[str, Any]) -> int:
    sizes = []
    for item in _list(kernel, "per_thread_memory_buffers"):
        buffer = _mapping(item, "per_thread_memory_buffers item")
        if str(buffer.get("type", "")).lower() == "scratch":
            sizes.append(_integer(buffer, "size"))
    # Zebin exposes scratch slots independently.  The programmed per-thread
    # scratch requirement is the largest slot, not their sum.
    return max(sizes, default=0)


def _kernel_contract(
    zebin: Zebin,
    root: dict[str, Any],
    kernel: dict[str, Any],
    ze_info_major: int,
    ze_info_minor: int,
) -> dict[str, Any]:
    name = kernel.get("name")
    if not isinstance(name, str) or not name:
        raise ContractError(".ze_info kernel has no valid name")
    text_section = zebin.one_section(f".text.{name}")
    symbol_matches = [
        symbol
        for symbol in zebin.symbols
        if symbol.name == name
        and symbol.symbol_type == 2  # STT_FUNC
        and symbol.section_index == text_section.index
    ]
    if len(symbol_matches) != 1:
        raise ContractError(
            f"kernel {name!r}: expected exactly one FUNC symbol in {text_section.name}, "
            f"got {len(symbol_matches)}"
        )
    symbol = symbol_matches[0]
    if symbol.size <= 0 or symbol.value + symbol.size > text_section.size:
        raise ContractError(
            f"kernel {name!r}: symbol range value={symbol.value} size={symbol.size} "
            f"does not fit section size={text_section.size}"
        )

    execution = _mapping(kernel.get("execution_env", {}), f"kernel {name} execution_env")
    simd_width = _integer(execution, "simd_size")
    grf_count = _integer(execution, "grf_count")
    slm_bytes = _integer(execution, "slm_size", 0)
    scratch_bytes = _scratch_bytes(kernel)

    raw_payload = [
        _mapping(item, f"kernel {name} payload argument")
        for item in _list(kernel, "payload_arguments")
    ]
    pointer_metadata: dict[int, list[dict[str, Any]]] = {}
    pointer_addresses: dict[int, dict[str, Any]] = {}
    payload_args: list[dict[str, Any]] = []
    cross_thread_end = 0
    for arg in raw_payload:
        offset = _integer(arg, "offset")
        size = _integer(arg, "size")
        if offset < 0 or size < 0:
            raise ContractError(f"kernel {name!r}: negative payload range")
        cross_thread_end = max(cross_thread_end, offset + size)
        arg_type = arg.get("arg_type")
        if arg_type == "arg_bypointer":
            index = _integer(arg, "arg_index")
            pointer_metadata.setdefault(index, []).append(arg)
        elif arg_type == "buffer_address":
            index = _integer(arg, "arg_index")
            if index in pointer_addresses:
                raise ContractError(f"kernel {name!r}: duplicate buffer address arg {index}")
            pointer_addresses[index] = arg
        elif arg_type == "arg_byvalue":
            payload_args.append(
                {
                    "arg_index": _integer(arg, "arg_index"),
                    "kind": "by_value",
                    "offset_bytes": offset,
                    "size_bytes": size,
                    "access": "none",
                    "address_mode": "none",
                    "address_space": str(arg.get("addrspace", "private")),
                }
            )

    valid_access = {"readonly", "writeonly", "readwrite"}
    valid_modes = {"stateful", "stateless"}
    for index in sorted(pointer_metadata):
        metadata_records = pointer_metadata[index]
        representations = []
        for metadata in metadata_records:
            access = str(metadata.get("access_type", "")).lower()
            mode = str(metadata.get("addrmode", "")).lower()
            address_space = str(metadata.get("addrspace", "")).lower()
            if access not in valid_access:
                raise ContractError(
                    f"kernel {name!r}: pointer arg {index} lacks a valid access_type "
                    f"(got {access!r}); metadata-preserving SPIR-V translation is required"
                )
            if mode not in valid_modes:
                raise ContractError(
                    f"kernel {name!r}: pointer arg {index} lacks a valid addrmode "
                    f"(got {mode!r})"
                )
            representations.append(
                {
                    "access": access,
                    "address_mode": mode,
                    "address_space": address_space,
                    "offset_bytes": _integer(metadata, "offset"),
                    "size_bytes": _integer(metadata, "size"),
                }
            )

        # Some IGC artifacts expose a hybrid pointer twice: a zero-sized
        # stateful representation (paired with a BTI) and an 8-byte stateless
        # representation carrying the actual cross-thread address.  Prefer the
        # explicit non-zero payload.  Pure stateful arguments instead obtain
        # their payload offset from buffer_address.
        explicit_payloads = [
            (metadata, representation)
            for metadata, representation in zip(
                metadata_records, representations, strict=True
            )
            if representation["size_bytes"] > 0
        ]
        if len(explicit_payloads) > 1:
            raise ContractError(
                f"kernel {name!r}: pointer arg {index} has ambiguous payloads"
            )
        if explicit_payloads:
            address, chosen = explicit_payloads[0]
        else:
            if index not in pointer_addresses:
                raise ContractError(
                    f"kernel {name!r}: pointer arg {index} has no payload address"
                )
            address = pointer_addresses[index]
            stateful = [
                representation
                for representation in representations
                if representation["address_mode"] == "stateful"
            ]
            if len(stateful) != 1:
                raise ContractError(
                    f"kernel {name!r}: buffer_address arg {index} lacks one "
                    "stateful pointer representation"
                )
            chosen = stateful[0]
        payload_args.append(
            {
                "arg_index": index,
                "kind": "by_pointer",
                "offset_bytes": _integer(address, "offset"),
                "size_bytes": _integer(address, "size"),
                "access": chosen["access"],
                "address_mode": chosen["address_mode"],
                "address_space": chosen["address_space"],
                "representations": representations,
            }
        )
    dangling_addresses = set(pointer_addresses).difference(pointer_metadata)
    if dangling_addresses:
        raise ContractError(
            f"kernel {name!r}: buffer addresses lack pointer metadata: "
            f"{sorted(dangling_addresses)}"
        )
    payload_args.sort(key=lambda arg: (arg["arg_index"], arg["kind"]))
    seen_args: set[int] = set()
    for arg in payload_args:
        index = arg["arg_index"]
        if index in seen_args:
            raise ContractError(f"kernel {name!r}: ambiguous payload arg index {index}")
        seen_args.add(index)

    bindings = []
    seen_bti: set[int] = set()
    seen_binding_arg: set[int] = set()
    for item in _list(kernel, "binding_table_indices"):
        binding = _mapping(item, f"kernel {name} binding")
        bti = _integer(binding, "bti_value")
        arg_index = _integer(binding, "arg_index")
        if bti in seen_bti or arg_index in seen_binding_arg:
            raise ContractError(f"kernel {name!r}: ambiguous binding table")
        seen_bti.add(bti)
        seen_binding_arg.add(arg_index)
        bindings.append({"arg_index": arg_index, "bti": bti})
    bindings.sort(key=lambda binding: binding["bti"])

    per_thread_end = 0
    per_thread_args = []
    for item in _list(kernel, "per_thread_payload_arguments"):
        arg = _mapping(item, f"kernel {name} per-thread payload")
        offset = _integer(arg, "offset")
        size = _integer(arg, "size")
        per_thread_end = max(per_thread_end, offset + size)
        per_thread_args.append(
            {"arg_type": str(arg.get("arg_type", "")), "offset": offset, "size": size}
        )

    return {
        "kernel_name": name,
        "ze_info_major": ze_info_major,
        "ze_info_minor": ze_info_minor,
        "text": {
            "section_name": text_section.name,
            "section_offset": text_section.offset,
            "section_size": text_section.size,
            "section_alignment": text_section.alignment,
            "symbol_value": symbol.value,
            "symbol_size": symbol.size,
            "entry_offset": text_section.offset + symbol.value,
            "entry_size": symbol.size,
            "sha256": hashlib.sha256(
                text_section.data[symbol.value : symbol.value + symbol.size]
            ).hexdigest(),
        },
        "simd_width": simd_width,
        "grf_count": grf_count,
        "scratch_bytes": scratch_bytes,
        "slm_bytes": slm_bytes,
        # Cross-thread data is delivered as whole 32-byte GRFs.
        "cross_thread_data_bytes": _align_up(cross_thread_end, 32),
        "per_thread_data_bytes": per_thread_end,
        "bindings": bindings,
        "payload_args": payload_args,
        "per_thread_payload_args": per_thread_args,
        "source_arg_info": _source_arg_info(root, name),
        "user_attributes": kernel.get("user_attributes", {}),
    }


def analyze_zebin(bin_path: Path, spv_path: Path | None = None) -> dict[str, Any]:
    zebin = Zebin.load(bin_path)
    ze_info = zebin.one_section(".ze_info")
    try:
        ze_text = ze_info.data.decode("utf-8", errors="strict").rstrip("\0")
    except UnicodeDecodeError as error:
        raise ContractError(f"{bin_path}: .ze_info is not UTF-8") from error
    root = parse_ze_info_yaml(ze_text)
    major, minor = _version_parts(root.get("version"))
    raw_kernels = _list(root, "kernels")
    if not raw_kernels:
        raise ContractError(f"{bin_path}: .ze_info contains no kernels")

    contracts = [
        _kernel_contract(
            zebin, root, _mapping(kernel, ".ze_info kernel"), major, minor
        )
        for kernel in raw_kernels
    ]
    names = [contract["kernel_name"] for contract in contracts]
    if len(set(names)) != len(names):
        raise ContractError(f"{bin_path}: duplicate kernel names in .ze_info")

    text_names = {
        section.name.removeprefix(".text.")
        for section in zebin.sections
        if section.name.startswith(".text.")
    }
    if text_names != set(names):
        raise ContractError(
            f"{bin_path}: .ze_info/text-section mismatch: "
            f"ze_info={sorted(names)} text={sorted(text_names)}"
        )

    return {
        "elf": {
            "class": 64,
            "endianness": "little",
            "type": "relocatable",
            "machine": zebin.machine,
            "size_bytes": len(zebin.data),
            "sha256": hashlib.sha256(zebin.data).hexdigest(),
            "ze_info_section_offset": ze_info.offset,
            "ze_info_section_size": ze_info.size,
        },
        "spirv": (
            {
                "size_bytes": spv_path.stat().st_size,
                "sha256": sha256_file(spv_path),
            }
            if spv_path is not None
            else None
        ),
        "kernels": contracts,
    }


def abi_projection(analysis: dict[str, Any]) -> dict[str, Any]:
    """Return facts that must remain stable across source frontends."""

    kernel_projections = []
    for kernel in analysis["kernels"]:
        kernel_projections.append(
            {
                "kernel_name": kernel["kernel_name"],
                "text_section_name": kernel["text"]["section_name"],
                "simd_width": kernel["simd_width"],
                "grf_count": kernel["grf_count"],
                "scratch_bytes": kernel["scratch_bytes"],
                "slm_bytes": kernel["slm_bytes"],
                "cross_thread_data_bytes": kernel["cross_thread_data_bytes"],
                "per_thread_data_bytes": kernel["per_thread_data_bytes"],
                "bindings": kernel["bindings"],
                "payload_args": kernel["payload_args"],
                "per_thread_payload_args": kernel["per_thread_payload_args"],
                "source_arg_info": kernel["source_arg_info"],
                "user_attributes": kernel["user_attributes"],
            }
        )
    return {"kernels": kernel_projections}


def validate_constraints(
    analysis: dict[str, Any],
    expected_simd_width: int,
    max_scratch_bytes: int,
    max_slm_bytes: int,
) -> None:
    for kernel in analysis["kernels"]:
        name = kernel["kernel_name"]
        if kernel["text"]["entry_offset"] % 64 != 0:
            raise ContractError(
                f"kernel {name!r}: entry offset {kernel['text']['entry_offset']} "
                "is not 64-byte aligned for the interface descriptor"
            )
        if kernel["simd_width"] != expected_simd_width:
            raise ContractError(
                f"kernel {name!r}: SIMD{kernel['simd_width']} violates "
                f"required SIMD{expected_simd_width}"
            )
        if kernel["scratch_bytes"] > max_scratch_bytes:
            raise ContractError(
                f"kernel {name!r}: scratch={kernel['scratch_bytes']} exceeds "
                f"supported {max_scratch_bytes}"
            )
        if kernel["slm_bytes"] > max_slm_bytes:
            raise ContractError(
                f"kernel {name!r}: SLM={kernel['slm_bytes']} exceeds "
                f"supported {max_slm_bytes}"
            )


def compare_abi(
    actual: dict[str, Any], reference: dict[str, Any], reference_path: Path
) -> None:
    actual_abi = abi_projection(actual)
    reference_abi = abi_projection(reference)
    if actual_abi == reference_abi:
        return
    actual_text = stable_json(actual_abi).splitlines()
    reference_text = stable_json(reference_abi).splitlines()
    first_difference = "<unknown>"
    for index, (expected, observed) in enumerate(
        zip(reference_text, actual_text, strict=False), start=1
    ):
        if expected != observed:
            first_difference = (
                f"line {index}: reference={expected!r}, generated={observed!r}"
            )
            break
    raise ContractError(
        f"generated ABI does not match reference {reference_path}: {first_difference}"
    )


def _rust_ident(value: str) -> str:
    result = re.sub(r"[^A-Za-z0-9]+", "_", value).strip("_").upper()
    if not result:
        raise ContractError(f"cannot derive a Rust identifier from {value!r}")
    if result[0].isdigit():
        result = f"KERNEL_{result}"
    return result


def default_rust_symbol(kernel_name: str, target: str, variant: str) -> str:
    return _rust_ident(f"{kernel_name}_{target}_{variant}_abi_contract")


def _rust_sha256(value: str) -> str:
    if not re.fullmatch(r"[0-9a-f]{64}", value):
        raise ContractError(f"invalid SHA-256 value {value!r}")
    parts = [f"0x{value[index:index + 2]}" for index in range(0, 64, 2)]
    rows = [", ".join(parts[index : index + 8]) for index in range(0, 32, 8)]
    return "[\n" + "".join(f"        {row},\n" for row in rows) + "    ]"


def _rust_string(value: str) -> str:
    return json.dumps(value)


def render_rust_contracts(manifest: dict[str, Any]) -> str:
    target = manifest["target"]
    artifact = manifest["artifact"]
    provenance = manifest["provenance"]
    requested_symbols = manifest.get("rust_symbols", {})
    output = [
        "// @generated by tools/intel-gpu-bakery; DO NOT EDIT.",
        f"// source: {manifest['source']['path']}",
        f"// frontend: {provenance['frontend']['description']}",
        f"// backend: {provenance['backend']['description']}",
        "// normalized commands: "
        + json.dumps(
            provenance["commands"], sort_keys=True, separators=(",", ":")
        ),
        "",
    ]
    for kernel in artifact["kernels"]:
        kernel_name = kernel["kernel_name"]
        symbol = requested_symbols.get(kernel_name)
        if not symbol:
            symbol = default_rust_symbol(
                kernel_name, target["label"], manifest["variant"]
            )
        payload_lines = []
        for arg in kernel["payload_args"]:
            kind = {
                "by_pointer": "GpgpuArtifactArgKind::ByPointer",
                "by_value": "GpgpuArtifactArgKind::ByValue",
            }[arg["kind"]]
            access = {
                "none": "GpgpuArtifactArgAccess::None",
                "readonly": "GpgpuArtifactArgAccess::ReadOnly",
                "writeonly": "GpgpuArtifactArgAccess::WriteOnly",
                "readwrite": "GpgpuArtifactArgAccess::ReadWrite",
            }[arg["access"]]
            address_mode = {
                "none": "GpgpuArtifactAddressMode::None",
                "stateful": "GpgpuArtifactAddressMode::Stateful",
                "stateless": "GpgpuArtifactAddressMode::Stateless",
            }[arg["address_mode"]]
            payload_lines.append(
                "        GpgpuArtifactPayloadArg { "
                f"arg_index: {arg['arg_index']}, kind: {kind}, "
                f"offset_bytes: {arg['offset_bytes']}, size_bytes: {arg['size_bytes']}, "
                f"access: {access}, address_mode: {address_mode} }},"
            )
        binding_lines = [
            (
                "        GpgpuArtifactBinding { "
                f"arg_index: {binding['arg_index']}, bti: {binding['bti']} }},"
            )
            for binding in kernel["bindings"]
        ]
        output.extend(
            [
                f"pub(crate) const {symbol}: GpgpuKernelAbiContract = "
                "GpgpuKernelAbiContract {",
                "    schema_version: GPGPU_KERNEL_ABI_SCHEMA_VERSION,",
                f"    kernel_name: {_rust_string(kernel_name)},",
                "    target: GpgpuKernelTarget {",
                f"        label: {_rust_string(target['label'])},",
                "        pci_device_ids: &["
                + ", ".join(f"0x{value:04x}" for value in target["pci_device_ids"])
                + "],",
                f"        revision_min: {target['revision_min']},",
                f"        revision_max: {target['revision_max']},",
                "    },",
                f"    ze_info_major: {kernel['ze_info_major']},",
                f"    ze_info_minor: {kernel['ze_info_minor']},",
                f"    zebin_sha256: {_rust_sha256(artifact['elf']['sha256'])},",
                f"    spv_sha256: {_rust_sha256(artifact['spirv']['sha256'])},",
                f"    text_section_name: {_rust_string(kernel['text']['section_name'])},",
                f"    text_section_offset: {kernel['text']['section_offset']},",
                f"    text_section_size: {kernel['text']['section_size']},",
                f"    entry_offset: {kernel['text']['entry_offset']},",
                f"    entry_size: {kernel['text']['entry_size']},",
                f"    simd_width: {kernel['simd_width']},",
                f"    grf_count: {kernel['grf_count']},",
                f"    scratch_bytes: {kernel['scratch_bytes']},",
                f"    slm_bytes: {kernel['slm_bytes']},",
                f"    cross_thread_data_bytes: {kernel['cross_thread_data_bytes']},",
                f"    per_thread_data_bytes: {kernel['per_thread_data_bytes']},",
                "    bindings: &[",
                *binding_lines,
                "    ],",
                "    payload_args: &[",
                *payload_lines,
                "    ],",
                "};",
                "",
            ]
        )
    return "\n".join(output)


def build_manifest(
    *,
    analysis: dict[str, Any],
    source: dict[str, Any],
    target: dict[str, Any],
    variant: str,
    provenance: dict[str, Any],
    abi_reference: dict[str, Any] | None,
    rust_symbols: dict[str, str] | None = None,
) -> dict[str, Any]:
    return {
        "schema_version": SCHEMA_VERSION,
        "source": source,
        "target": target,
        "variant": variant,
        "artifact": analysis,
        "abi_reference": abi_reference,
        "provenance": provenance,
        "rust_symbols": rust_symbols or {},
    }


def verify_manifest(
    manifest_path: Path,
    bin_path: Path,
    spv_path: Path,
    contract_path: Path,
    repo_root: Path,
) -> None:
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("schema_version") != SCHEMA_VERSION:
        raise ContractError(
            f"{manifest_path}: unsupported schema {manifest.get('schema_version')}"
        )
    actual = analyze_zebin(bin_path, spv_path)
    if actual != manifest.get("artifact"):
        raise ContractError(f"{manifest_path}: artifact facts/hashes are stale")
    for record in manifest.get("source", {}).get("inputs", []):
        raw_path = Path(record["path"])
        input_path = raw_path if raw_path.is_absolute() else repo_root / raw_path
        if not input_path.is_file():
            raise ContractError(f"{manifest_path}: recorded input is missing: {raw_path}")
        actual_record = {
            "path": record["path"],
            "size_bytes": input_path.stat().st_size,
            "sha256": sha256_file(input_path),
        }
        if actual_record != record:
            raise ContractError(f"{manifest_path}: recorded input is stale: {raw_path}")
    reference = manifest.get("abi_reference")
    if reference:
        raw_reference = Path(reference["path"])
        reference_path = (
            raw_reference if raw_reference.is_absolute() else repo_root / raw_reference
        )
        if not reference_path.is_file() or sha256_file(reference_path) != reference["sha256"]:
            raise ContractError(f"{manifest_path}: ABI reference is missing or changed")
        compare_abi(actual, analyze_zebin(reference_path), reference_path)
    profile = manifest.get("provenance", {}).get("profile")
    if profile:
        raw_profile = Path(profile["path"])
        profile_path = raw_profile if raw_profile.is_absolute() else repo_root / raw_profile
        if not profile_path.is_file() or sha256_file(profile_path) != profile["sha256"]:
            raise ContractError(f"{manifest_path}: bake profile is missing or changed")
    rendered = render_rust_contracts(manifest)
    existing = contract_path.read_text(encoding="utf-8")
    if existing != rendered:
        raise ContractError(f"{contract_path}: generated Rust contract is stale")


def repo_relative(path: Path, repo_root: Path) -> str:
    resolved = path.resolve()
    try:
        return resolved.relative_to(repo_root.resolve()).as_posix()
    except ValueError:
        return resolved.as_posix()


def input_records(paths: Iterable[Path], repo_root: Path) -> list[dict[str, Any]]:
    unique: dict[Path, dict[str, Any]] = {}
    for path in paths:
        resolved = path.resolve()
        if not resolved.is_file():
            raise ContractError(f"compiler dependency does not exist: {path}")
        unique[resolved] = {
            "path": repo_relative(resolved, repo_root),
            "size_bytes": resolved.stat().st_size,
            "sha256": sha256_file(resolved),
        }
    return sorted(unique.values(), key=lambda item: item["path"])
