#!/usr/bin/env python3
"""Compile the pinned Misaki US dictionaries into canonical KLEX v1.

The kernel never parses JSON. This host-only tool verifies exact upstream
source bytes, merges silver then gold (gold DEFAULT wins), preserves every
non-null non-DEFAULT pronunciation in a sorted variant table, and emits one
hash-sealed, zero-copy artifact.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import struct
import tempfile
from pathlib import Path
from typing import Any

MAGIC = b"TRKLEX1\0"
VERSION = 1
HEADER_BYTES = 256
ENTRY_RECORD_BYTES = 12
VARIANT_RECORD_BYTES = 16
FLAG_POS_VARIANTS = 1

DIGEST_OFFSET = 72
DIGEST_END = 104
SILVER_DIGEST_OFFSET = 104
GOLD_DIGEST_OFFSET = 136
LICENSE_DIGEST_OFFSET = 168
SOURCE_COMMIT_OFFSET = 200

MAX_WORD_BYTES = 256
MAX_PRONUNCIATION_BYTES = 512
MAX_TAG_BYTES = 32
MAX_ENTRIES = 500_000
MAX_VARIANTS = 4_096

SILVER_SHA256 = bytes.fromhex(
    "57cae2a1a9d73ce219ad9142b0d904914a0228cb1babce20e5bfd4e1b1307ee4"
)
GOLD_SHA256 = bytes.fromhex(
    "bb83c899d8dbfa160fa05661bea052bacfeece9b639851662334e85002ee8ad9"
)
LICENSE_SHA256 = bytes.fromhex(
    "1bea4b79e660b7477ea5919bed5944d970c86531b508bd1d538309c0d12e8858"
)
SOURCE_COMMIT = bytes.fromhex("7bbe06cacd9102d8a0d9e338a3711ae7208de0ad")

PINNED_SILVER_ENTRIES = 299_704
PINNED_GOLD_ENTRIES = 90_201
PINNED_OVERLAP = 1
PINNED_OUTPUT_ENTRIES = 389_904
PINNED_VARIANTS = 41


class CompileError(ValueError):
    """The source or requested artifact violates the sealed profile."""


def sha256(data: bytes) -> bytes:
    return hashlib.sha256(data).digest()


def _dict_without_duplicates(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    output: dict[str, Any] = {}
    for key, value in pairs:
        if key in output:
            raise CompileError(f"duplicate JSON key: {key!r}")
        output[key] = value
    return output


def decode_dictionary(data: bytes, label: str) -> dict[str, Any]:
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError as error:
        raise CompileError(f"{label}: source is not UTF-8: {error}") from error
    try:
        value = json.loads(text, object_pairs_hook=_dict_without_duplicates)
    except (json.JSONDecodeError, CompileError) as error:
        raise CompileError(f"{label}: invalid JSON: {error}") from error
    if not isinstance(value, dict):
        raise CompileError(f"{label}: root must be an object")
    return value


def _checked_text(value: Any, label: str, maximum: int) -> str:
    if not isinstance(value, str):
        raise CompileError(f"{label}: expected a string")
    encoded = value.encode("utf-8")
    if not encoded:
        raise CompileError(f"{label}: must not be empty")
    if len(encoded) > maximum:
        raise CompileError(f"{label}: {len(encoded)} bytes exceeds {maximum}")
    return value


def merge_dictionaries(
    silver: dict[str, Any], gold: dict[str, Any]
) -> tuple[list[tuple[str, str]], list[tuple[str, str, str]], int]:
    defaults: dict[str, str] = {}
    for word, pronunciation in silver.items():
        checked_word = _checked_text(word, "silver word", MAX_WORD_BYTES)
        defaults[checked_word] = _checked_text(
            pronunciation,
            f"silver pronunciation for {word!r}",
            MAX_PRONUNCIATION_BYTES,
        )

    overlap = sum(word in defaults for word in gold)
    variants: list[tuple[str, str, str]] = []
    for word, value in gold.items():
        checked_word = _checked_text(word, "gold word", MAX_WORD_BYTES)
        if isinstance(value, str):
            default = _checked_text(
                value,
                f"gold pronunciation for {word!r}",
                MAX_PRONUNCIATION_BYTES,
            )
        elif isinstance(value, dict):
            if "DEFAULT" not in value:
                raise CompileError(f"gold {word!r}: missing DEFAULT")
            default = _checked_text(
                value["DEFAULT"],
                f"gold DEFAULT for {word!r}",
                MAX_PRONUNCIATION_BYTES,
            )
            for tag, pronunciation in value.items():
                if tag == "DEFAULT" or pronunciation is None:
                    continue
                checked_tag = _checked_text(
                    tag, f"gold variant tag for {word!r}", MAX_TAG_BYTES
                )
                checked_pronunciation = _checked_text(
                    pronunciation,
                    f"gold {tag!r} pronunciation for {word!r}",
                    MAX_PRONUNCIATION_BYTES,
                )
                variants.append((checked_word, checked_tag, checked_pronunciation))
        else:
            raise CompileError(f"gold {word!r}: expected string or object")
        defaults[checked_word] = default

    entries = sorted(defaults.items(), key=lambda item: item[0].encode("utf-8"))
    variants.sort(key=lambda item: (item[0].encode("utf-8"), item[1].encode("utf-8")))
    if len({word for word, _ in entries}) != len(entries):
        raise CompileError("merged dictionary contains duplicate words")
    if len({(word, tag) for word, tag, _ in variants}) != len(variants):
        raise CompileError("variant dictionary contains duplicate word/tag keys")
    return entries, variants, overlap


def build_artifact(
    entries: list[tuple[str, str]],
    variants: list[tuple[str, str, str]],
    *,
    silver_digest: bytes = SILVER_SHA256,
    gold_digest: bytes = GOLD_SHA256,
    license_digest: bytes = LICENSE_SHA256,
    source_commit: bytes = SOURCE_COMMIT,
) -> bytes:
    if len(entries) > MAX_ENTRIES:
        raise CompileError(f"{len(entries)} entries exceeds {MAX_ENTRIES}")
    if len(variants) > MAX_VARIANTS:
        raise CompileError(f"{len(variants)} variants exceeds {MAX_VARIANTS}")
    if any(len(value) != length for value, length in (
        (silver_digest, 32),
        (gold_digest, 32),
        (license_digest, 32),
        (source_commit, 20),
    )):
        raise CompileError("invalid provenance digest length")

    pool = bytearray()
    entry_records = bytearray()
    entry_indexes: dict[str, int] = {}
    previous_word: bytes | None = None
    for index, (word, pronunciation) in enumerate(entries):
        word_bytes = _checked_text(word, "entry word", MAX_WORD_BYTES).encode("utf-8")
        pronunciation_bytes = _checked_text(
            pronunciation,
            f"entry pronunciation for {word!r}",
            MAX_PRONUNCIATION_BYTES,
        ).encode("utf-8")
        if previous_word is not None and previous_word >= word_bytes:
            raise CompileError("entries must be strictly UTF-8 sorted")
        previous_word = word_bytes
        word_offset = len(pool)
        pool.extend(word_bytes)
        pronunciation_offset = len(pool)
        pool.extend(pronunciation_bytes)
        if pronunciation_offset > 0xFFFF_FFFF or len(pool) > 0xFFFF_FFFF:
            raise CompileError("string pool exceeds KLEX v1 offsets")
        entry_records.extend(
            struct.pack(
                "<IHHI",
                word_offset,
                len(word_bytes),
                len(pronunciation_bytes),
                pronunciation_offset,
            )
        )
        entry_indexes[word] = index

    variant_records = bytearray()
    previous_variant: tuple[int, bytes] | None = None
    for word, tag, pronunciation in variants:
        if word not in entry_indexes:
            raise CompileError(f"variant word has no default entry: {word!r}")
        entry_index = entry_indexes[word]
        tag_bytes = _checked_text(tag, "variant tag", MAX_TAG_BYTES).encode("utf-8")
        pronunciation_bytes = _checked_text(
            pronunciation,
            f"variant pronunciation for {word!r}/{tag!r}",
            MAX_PRONUNCIATION_BYTES,
        ).encode("utf-8")
        key = (entry_index, tag_bytes)
        if previous_variant is not None and previous_variant >= key:
            raise CompileError("variants must be strictly word/tag sorted")
        previous_variant = key
        tag_offset = len(pool)
        pool.extend(tag_bytes)
        pronunciation_offset = len(pool)
        pool.extend(pronunciation_bytes)
        if pronunciation_offset > 0xFFFF_FFFF or len(pool) > 0xFFFF_FFFF:
            raise CompileError("string pool exceeds KLEX v1 offsets")
        variant_records.extend(
            struct.pack(
                "<IIIHH",
                entry_index,
                tag_offset,
                pronunciation_offset,
                len(tag_bytes),
                len(pronunciation_bytes),
            )
        )

    variants_offset = HEADER_BYTES + len(entry_records)
    strings_offset = variants_offset + len(variant_records)
    file_bytes = strings_offset + len(pool)
    header = bytearray(HEADER_BYTES)
    header[:8] = MAGIC
    struct.pack_into("<HHI", header, 8, VERSION, HEADER_BYTES, FLAG_POS_VARIANTS if variants else 0)
    struct.pack_into("<IIHH", header, 16, len(entries), len(variants), ENTRY_RECORD_BYTES, VARIANT_RECORD_BYTES)
    struct.pack_into(
        "<QQQQQ",
        header,
        32,
        HEADER_BYTES,
        variants_offset,
        strings_offset,
        file_bytes,
        len(pool),
    )
    header[SILVER_DIGEST_OFFSET:GOLD_DIGEST_OFFSET] = silver_digest
    header[GOLD_DIGEST_OFFSET:LICENSE_DIGEST_OFFSET] = gold_digest
    header[LICENSE_DIGEST_OFFSET:SOURCE_COMMIT_OFFSET] = license_digest
    header[SOURCE_COMMIT_OFFSET:SOURCE_COMMIT_OFFSET + 20] = source_commit

    output = header + entry_records + variant_records + pool
    if len(output) != file_bytes:
        raise CompileError("internal file-size mismatch")
    output[DIGEST_OFFSET:DIGEST_END] = sha256(output)
    return bytes(output)


def read_pinned(path: Path, expected_digest: bytes, label: str) -> bytes:
    data = path.read_bytes()
    actual = sha256(data)
    if actual != expected_digest:
        raise CompileError(
            f"{label}: SHA-256 {actual.hex()} != pinned {expected_digest.hex()}"
        )
    return data


def write_atomic(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary: Path | None = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="wb", dir=path.parent, prefix=f".{path.name}.", delete=False
        ) as output:
            temporary = Path(output.name)
            output.write(data)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    finally:
        if temporary is not None and temporary.exists():
            temporary.unlink()


def compile_pinned(silver_path: Path, gold_path: Path, license_path: Path) -> tuple[bytes, int]:
    silver_bytes = read_pinned(silver_path, SILVER_SHA256, "US silver")
    gold_bytes = read_pinned(gold_path, GOLD_SHA256, "US gold")
    read_pinned(license_path, LICENSE_SHA256, "Misaki license")
    silver = decode_dictionary(silver_bytes, "US silver")
    gold = decode_dictionary(gold_bytes, "US gold")
    entries, variants, overlap = merge_dictionaries(silver, gold)
    observed = (len(silver), len(gold), overlap, len(entries), len(variants))
    expected = (
        PINNED_SILVER_ENTRIES,
        PINNED_GOLD_ENTRIES,
        PINNED_OVERLAP,
        PINNED_OUTPUT_ENTRIES,
        PINNED_VARIANTS,
    )
    if observed != expected:
        raise CompileError(f"pinned source profile {observed} != expected {expected}")
    return build_artifact(entries, variants), len(variants)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--silver", required=True, type=Path, help="pinned us_silver.json")
    parser.add_argument("--gold", required=True, type=Path, help="pinned us_gold.json")
    parser.add_argument("--license", required=True, type=Path, help="pinned misaki-rs LICENSE")
    parser.add_argument("--output", required=True, type=Path, help="destination misaki-us.klex")
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify that OUTPUT already equals a fresh deterministic compile",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    artifact, variant_count = compile_pinned(args.silver, args.gold, args.license)
    digest = sha256(artifact).hex()
    if args.check:
        existing = args.output.read_bytes()
        if existing != artifact:
            raise CompileError(
                f"{args.output}: stale artifact; got {sha256(existing).hex()}, expected {digest}"
            )
        action = "verified"
    else:
        write_atomic(args.output, artifact)
        action = "wrote"
    print(
        f"{action} path={args.output} entries={PINNED_OUTPUT_ENTRIES} "
        f"variants={variant_count} bytes={len(artifact)} sha256={digest}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
