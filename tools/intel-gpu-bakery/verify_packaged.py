#!/usr/bin/env python3
"""Verify selected and required Intel Zebins in the ELF packaged by an ISO."""

from __future__ import annotations

import argparse
import hashlib
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from verify_linked import (
    LinkedArtifactError,
    verify_linked_image,
    verify_required_artifacts,
)


class PackagedArtifactError(RuntimeError):
    pass


def _existing_file(value: str) -> Path:
    path = Path(value).expanduser().resolve()
    if not path.is_file():
        raise argparse.ArgumentTypeError(f"file does not exist: {path}")
    return path


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _require_identical(reference: Path, candidate: Path, label: str) -> str:
    reference_size = reference.stat().st_size
    candidate_size = candidate.stat().st_size
    reference_sha256 = _sha256(reference)
    candidate_sha256 = _sha256(candidate)
    if reference_size != candidate_size or reference_sha256 != candidate_sha256:
        raise PackagedArtifactError(
            f"{label} differs from runtime ELF: "
            f"runtime_size={reference_size} candidate_size={candidate_size} "
            f"runtime_sha256={reference_sha256} candidate_sha256={candidate_sha256}"
        )
    return reference_sha256


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Extract /TRUEOS.elf from an ISO, prove byte identity with the "
            "stripped runtime/staging ELF, then prove the requested artifact "
            "is selected and the alternate artifact is absent."
        )
    )
    parser.add_argument("--runtime-elf", required=True, type=_existing_file)
    parser.add_argument("--staged-elf", required=True, type=_existing_file)
    parser.add_argument("--iso", required=True, type=_existing_file)
    parser.add_argument("--selected-bin", required=True, type=_existing_file)
    parser.add_argument("--forbidden-bin", required=True, type=_existing_file)
    parser.add_argument(
        "--required-bin",
        action="append",
        default=[],
        type=_existing_file,
        help="additional artifact that must occur in the packaged ELF; repeatable",
    )
    parser.add_argument("--xorriso", default="xorriso")
    args = parser.parse_args(argv)

    xorriso = shutil.which(args.xorriso)
    if xorriso is None:
        raise PackagedArtifactError(f"xorriso executable not found: {args.xorriso}")

    staged_sha256 = _require_identical(
        args.runtime_elf, args.staged_elf, "ISO staging ELF"
    )
    with tempfile.TemporaryDirectory(prefix="trueos-artifact-verify-") as directory:
        extracted = Path(directory) / "TRUEOS.elf"
        process = subprocess.run(
            [
                xorriso,
                "-osirrox",
                "on",
                "-indev",
                str(args.iso),
                "-extract",
                "/TRUEOS.elf",
                str(extracted),
            ],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            check=False,
        )
        if process.returncode != 0 or not extracted.is_file():
            raise PackagedArtifactError(
                f"cannot extract /TRUEOS.elf from {args.iso}: "
                f"exit={process.returncode} output={process.stdout.strip()!r}"
            )
        iso_sha256 = _require_identical(
            args.runtime_elf, extracted, "ISO-extracted ELF"
        )
        selected_sha256, selected_count, forbidden_sha256 = verify_linked_image(
            extracted, args.selected_bin, args.forbidden_bin
        )
        required = verify_required_artifacts(extracted, args.required_bin)
        required_text = ",".join(
            f"{path.name}:{digest}:{count}" for path, digest, count in required
        )

    print(
        f"packaged artifact verified: iso={args.iso} member=/TRUEOS.elf "
        f"runtime_sha256={staged_sha256} iso_member_sha256={iso_sha256} "
        f"selected_sha256={selected_sha256} selected_occurrences={selected_count} "
        f"forbidden_sha256={forbidden_sha256} forbidden_occurrences=0 "
        f"required={required_text or 'none'}"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (
        LinkedArtifactError,
        PackagedArtifactError,
        OSError,
        ValueError,
    ) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2)
