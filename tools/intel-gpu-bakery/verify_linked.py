#!/usr/bin/env python3
"""Prove the selected and required Intel Zebins in a linked TRUEOS image."""

from __future__ import annotations

import argparse
import hashlib
import mmap
import sys
from pathlib import Path


class LinkedArtifactError(RuntimeError):
    pass


def _existing_file(value: str) -> Path:
    path = Path(value).expanduser().resolve()
    if not path.is_file():
        raise argparse.ArgumentTypeError(f"file does not exist: {path}")
    return path


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _occurrences(image: mmap.mmap, needle: bytes) -> int:
    count = 0
    offset = 0
    while True:
        found = image.find(needle, offset)
        if found < 0:
            return count
        count += 1
        offset = found + 1


def verify_linked_image(elf: Path, selected_bin: Path) -> tuple[str, int]:
    selected = selected_bin.read_bytes()
    if not selected:
        raise LinkedArtifactError("selected artifact must be non-empty")

    with elf.open("rb") as image_file:
        with mmap.mmap(image_file.fileno(), 0, access=mmap.ACCESS_READ) as image:
            selected_count = _occurrences(image, selected)

    if selected_count == 0:
        raise LinkedArtifactError(f"{elf}: selected artifact is absent ({selected_bin})")
    return _sha256(selected), selected_count


def verify_required_artifacts(
    elf: Path, required_bins: list[Path]
) -> list[tuple[Path, str, int]]:
    records: list[tuple[Path, str, int]] = []
    with elf.open("rb") as image_file:
        with mmap.mmap(image_file.fileno(), 0, access=mmap.ACCESS_READ) as image:
            for required_bin in required_bins:
                required = required_bin.read_bytes()
                if not required:
                    raise LinkedArtifactError(
                        f"required artifact must be non-empty: {required_bin}"
                    )
                count = _occurrences(image, required)
                if count == 0:
                    raise LinkedArtifactError(
                        f"{elf}: required artifact is absent ({required_bin})"
                    )
                records.append((required_bin, _sha256(required), count))
    return records


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Require a linked TRUEOS ELF to contain the selected and required "
            "Intel Zebins."
        )
    )
    parser.add_argument("--elf", required=True, type=_existing_file)
    parser.add_argument("--selected-bin", required=True, type=_existing_file)
    parser.add_argument(
        "--required-bin",
        action="append",
        default=[],
        type=_existing_file,
        help="additional artifact that must occur in the linked image; repeatable",
    )
    args = parser.parse_args(argv)

    selected_sha256, selected_count = verify_linked_image(args.elf, args.selected_bin)
    required = verify_required_artifacts(args.elf, args.required_bin)
    required_text = ",".join(
        f"{path.name}:{digest}:{count}" for path, digest, count in required
    )

    print(
        f"linked artifact verified: elf={args.elf} "
        f"selected_sha256={selected_sha256} selected_occurrences={selected_count} "
        f"required={required_text or 'none'}"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (LinkedArtifactError, OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2)
