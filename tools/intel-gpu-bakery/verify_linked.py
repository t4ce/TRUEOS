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


def verify_single_copy_artifacts(elf: Path, artifact_dir: Path) -> int:
    """Reject duplicate linked Zebins or SPIR-V outside their single Zebin.

    Optional artifacts may be unlinked. Existing selected/required checks decide
    which must be present; this checks every present artifact in the directory.
    """
    verified = 0
    with elf.open("rb") as image_file:
        with mmap.mmap(image_file.fileno(), 0, access=mmap.ACCESS_READ) as image:
            for binary_path in sorted(artifact_dir.glob("*.bin")):
                spirv_path = binary_path.with_suffix(".spv")
                if not spirv_path.is_file():
                    continue
                binary = binary_path.read_bytes()
                spirv = spirv_path.read_bytes()
                if not binary or not spirv:
                    raise LinkedArtifactError(f"empty shader artifact: {binary_path}")
                count = _occurrences(image, binary)
                if count == 0:
                    continue
                if count != 1:
                    raise LinkedArtifactError(
                        f"{elf}: duplicate shader binary ({binary_path}, copies={count})"
                    )
                if binary.count(spirv) != 1 or _occurrences(image, spirv) != 1:
                    raise LinkedArtifactError(
                        f"{elf}: SPIR-V must occur only inside its single Zebin ({spirv_path})"
                    )
                verified += 1
    # These two native stages are shared by independent graphics pipelines.
    # Count the committed byte sequences to catch regenerated private copies.
    repo_root = Path(__file__).resolve().parents[2]
    shared_stages = (
        "crates/trueos-shader/host_shader_validation/cache_extract/triangle_vs.bin",
        "picasso/picasso-retained-textured-forward/retained_textured_forward.ps.simd16.bin",
    )
    with elf.open("rb") as image_file:
        with mmap.mmap(image_file.fileno(), 0, access=mmap.ACCESS_READ) as image:
            for relative_path in shared_stages:
                stage = repo_root / relative_path
                count = _occurrences(image, stage.read_bytes())
                if count > 1:
                    raise LinkedArtifactError(
                        f"{elf}: duplicate shared graphics stage ({relative_path}, copies={count})"
                    )
    return verified


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
    single_copy_count = verify_single_copy_artifacts(args.elf, args.selected_bin.parent)
    required_text = ",".join(
        f"{path.name}:{digest}:{count}" for path, digest, count in required
    )

    print(
        f"linked artifact verified: elf={args.elf} "
        f"selected_sha256={selected_sha256} selected_occurrences={selected_count} "
        f"required={required_text or 'none'} single_copy_artifacts={single_copy_count}"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (LinkedArtifactError, OSError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2)
