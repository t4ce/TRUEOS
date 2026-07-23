#!/usr/bin/env python3
"""Prove that a linked TRUEOS image contains only the selected copy Zebin."""

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


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Require a linked TRUEOS ELF to contain the selected Intel Zebin "
            "and no byte-identical copy of the forbidden fallback Zebin."
        )
    )
    parser.add_argument("--elf", required=True, type=_existing_file)
    parser.add_argument("--selected-bin", required=True, type=_existing_file)
    parser.add_argument("--forbidden-bin", required=True, type=_existing_file)
    args = parser.parse_args(argv)

    selected = args.selected_bin.read_bytes()
    forbidden = args.forbidden_bin.read_bytes()
    if not selected or not forbidden:
        raise LinkedArtifactError("selected and forbidden artifacts must be non-empty")
    if selected == forbidden:
        raise LinkedArtifactError("selected and forbidden artifacts are identical")

    with args.elf.open("rb") as image_file:
        with mmap.mmap(image_file.fileno(), 0, access=mmap.ACCESS_READ) as image:
            selected_count = _occurrences(image, selected)
            forbidden_count = _occurrences(image, forbidden)

    if selected_count == 0:
        raise LinkedArtifactError(
            f"{args.elf}: selected artifact is absent ({args.selected_bin})"
        )
    if forbidden_count != 0:
        raise LinkedArtifactError(
            f"{args.elf}: forbidden fallback occurs {forbidden_count} time(s) "
            f"({args.forbidden_bin})"
        )

    print(
        f"linked artifact verified: elf={args.elf} "
        f"selected_sha256={_sha256(selected)} selected_occurrences={selected_count} "
        f"forbidden_sha256={_sha256(forbidden)} forbidden_occurrences=0"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (LinkedArtifactError, OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2)
