#!/usr/bin/env python3

from __future__ import annotations

import argparse
import hashlib
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Summarize dumped Intel shader blobs and optionally print Rust u32 words."
    )
    parser.add_argument(
        "dump_dir",
        type=Path,
        help="Directory containing dumped .bin shader blobs",
    )
    parser.add_argument(
        "--emit-rust",
        nargs="*",
        default=[],
        metavar="BLOB",
        help="Blob filenames to print as Rust u32 words",
    )
    return parser.parse_args()


def sha1_hex(data: bytes) -> str:
    return hashlib.sha1(data).hexdigest()


def words_from_bytes(data: bytes) -> list[int]:
    return [int.from_bytes(data[index : index + 4], "little") for index in range(0, len(data), 4)]


def print_summary(blob_path: Path) -> None:
    data = blob_path.read_bytes()
    size = len(data)
    aligned = "yes" if size % 4 == 0 else "no"
    print(f"{blob_path.name} size={size} bytes words={size // 4 if size % 4 == 0 else 'n/a'} sha1={sha1_hex(data)} aligned4={aligned}")


def print_rust(blob_path: Path) -> None:
    data = blob_path.read_bytes()
    if len(data) % 4 != 0:
        raise SystemExit(f"blob is not 4-byte aligned: {blob_path}")
    words = words_from_bytes(data)
    print()
    print(f"// {blob_path.name} size={len(data)} sha1={sha1_hex(data)}")
    for word in words:
        print(f"0x{word:08X},")


def main() -> None:
    args = parse_args()
    if not args.dump_dir.is_dir():
        raise SystemExit(f"dump directory not found: {args.dump_dir}")

    blobs = sorted(args.dump_dir.glob("*.bin"))
    if not blobs:
        raise SystemExit(f"no .bin blobs found in: {args.dump_dir}")

    for blob in blobs:
        print_summary(blob)

    if args.emit_rust:
        blob_names = {blob.name: blob for blob in blobs}
        for name in args.emit_rust:
            blob = blob_names.get(name)
            if blob is None:
                raise SystemExit(f"requested blob not found: {name}")
            print_rust(blob)


if __name__ == "__main__":
    main()