#!/usr/bin/env python3
"""Import raw v1 line strips into MicroFont's mask-first glyph source files."""

import argparse
import math
import os
from pathlib import Path
import re
import stat
import struct
import subprocess
import tempfile
from typing import NamedTuple


DEFAULT_DATA_DIR = Path(__file__).resolve().parents[2] / "vendor/microfont/data/line-v1"
HEADER_BYTES = 20


class ImportPlan(NamedTuple):
    target: Path
    data: bytes
    strips: int
    raster_bytes: int


def validate_lines(data: bytes) -> int:
    """Validate every complete strip and return its count without changing bytes."""
    if not data:
        raise ValueError("raw line data is empty")
    cursor = 0
    strips = 0
    while cursor < len(data):
        header_at = cursor
        count = (data[cursor] & 63) + 2
        cursor += 1
        end = cursor + count
        if end > len(data):
            raise ValueError(f"truncated strip at byte {header_at}: needs {count} points")
        for offset in range(cursor, end):
            point = data[offset]
            if point & 128 or point & 7 >= 6 or point >> 3 >= 11:
                raise ValueError(f"invalid 6x11 line point at byte {offset}: 0x{point:02X}")
        cursor = end
        strips += 1
    return strips


def source_header(data: bytes) -> bytes:
    """Validate bitmap metadata and return the existing 20-byte source header."""
    if len(data) < HEADER_BYTES:
        raise ValueError("mask-first source header is shorter than 20 bytes")
    mask = int.from_bytes(data[:16], "little")
    flags, kind = data[16:18]
    if mask >> 66:
        raise ValueError("bitmap mask exceeds the 6x11 cell")
    if flags & ~1:
        raise ValueError("unknown bitmap-presence flags")
    if not flags & 1 and mask:
        raise ValueError("bitmap value has no presence flag")
    if kind > 6:
        raise ValueError("unknown raster kind")
    if data[18:20] != bytes(2):
        raise ValueError("reserved header bytes must be zero")
    if kind == 0 and len(data) != HEADER_BYTES:
        raise ValueError("raster kind 0 cannot have a payload")
    return data[:HEADER_BYTES]


def is_mask_first_source(data: bytes) -> bool:
    """Recognize complete canonical records before treating input as raw strips."""
    try:
        header = source_header(data)
        kind = header[17]
        payload = data[HEADER_BYTES:]
        if kind == 0:
            return True
        if kind == 1:
            validate_lines(payload)
            return True
        if kind == 2:
            if len(payload) % 32:
                return False
            for index, (value,) in enumerate(struct.iter_unpack("<f", payload)):
                if not math.isfinite(value) or not 0 <= value <= (6 if index % 2 == 0 else 11):
                    return False
            return True
        if kind == 3:
            return len(payload) == 5 and 1 <= payload[4] <= 5
        if kind == 4:
            return len(payload) == 9 and payload[8] <= 1
        if kind == 5:
            return bool(payload) and len(payload) % 4 == 0
        if len(payload) != 17 or payload[0] > 3:
            return False
        x, y, radius, width = struct.unpack("<ffff", payload[1:])
        sx = 1 if payload[0] in (0, 3) else -1
        sy = 1 if payload[0] < 2 else -1
        half = width * 0.5
        return (all(math.isfinite(v) for v in (x, y, radius, width))
                and radius > half > 0 and half <= x <= 6 - half and half <= y <= 11 - half
                and 0 < x + sx * radius < 6 and 0 < y + sy * radius < 11)
    except ValueError:
        return False


def parse_glyph(value: str) -> int:
    match = re.fullmatch(r"U\+([0-9A-Fa-f]{1,6})", value, re.IGNORECASE)
    if not match:
        raise ValueError("--glyph must be a Unicode codepoint such as U+2192")
    codepoint = int(match[1], 16)
    if codepoint > 0x10FFFF or 0xD800 <= codepoint <= 0xDFFF:
        raise ValueError(f"{value} is not a Unicode scalar")
    return codepoint


def glyph_from_filename(path: Path) -> int:
    match = re.fullmatch(r"u([0-9A-Fa-f]{4,6})\.bin", path.name)
    if not match:
        raise ValueError(f"{path}: use a uXXXX.bin source name or specify --glyph U+XXXX")
    return parse_glyph(f"U+{match[1]}")


def _git_header(target: Path) -> bytes:
    parent = target.parent
    while not parent.exists():
        parent = parent.parent
    try:
        root = subprocess.run(
            ["git", "-C", str(parent), "rev-parse", "--show-toplevel"],
            check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        ).stdout.decode().strip()
    except FileNotFoundError as error:
        raise ValueError("Git is required by --recover-header-from-git but is unavailable") from error
    except subprocess.CalledProcessError as error:
        raise ValueError(f"{target}: cannot recover a header outside a Git repository") from error
    root_path = Path(root).resolve()
    try:
        relative = target.relative_to(root_path).as_posix()
    except ValueError as error:
        raise ValueError(f"{target}: target is outside its Git repository") from error
    head = subprocess.run(
        ["git", "-C", str(root_path), "rev-parse", "--verify", "HEAD"],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    if head.returncode:
        raise ValueError(f"{target}: Git repository has no readable HEAD for header recovery")
    revision = head.stdout.decode("ascii").strip()
    try:
        entry = subprocess.run(
            ["git", "-C", str(root_path), "ls-tree", "-z", revision, "--", relative],
            check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        ).stdout
    except subprocess.CalledProcessError as error:
        raise ValueError(f"{target}: cannot inspect Git HEAD for header recovery") from error
    if not entry:
        # A newly added glyph has no previous bitmap metadata to recover.
        return bytes(HEADER_BYTES)
    previous = subprocess.run(
        ["git", "-C", str(root_path), "show", f"{revision}:{relative}"],
        check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    ).stdout
    try:
        return source_header(previous)
    except ValueError as error:
        raise ValueError(f"{target}: Git HEAD does not contain valid bitmap metadata: {error}") from error


def plan_import(source: Path, data_dir: Path = DEFAULT_DATA_DIR, *,
                glyph: int | None = None, recover_header_from_git: bool = False) -> ImportPlan:
    if glyph is None and source.name == "homebutton.bin":
        target = (data_dir / "homebutton.bin").resolve()
    else:
        codepoint = glyph_from_filename(source) if glyph is None else glyph
        if not 0 <= codepoint <= 0x10FFFF or 0xD800 <= codepoint <= 0xDFFF:
            raise ValueError(f"U+{codepoint:X} is not a Unicode scalar")
        target = (data_dir / f"u{codepoint:04X}.bin").resolve()
    raw = source.read_bytes()
    if is_mask_first_source(raw):
        raise ValueError(f"{source}: input is already a mask-first glyph source; provide raw line strips")
    try:
        strips = validate_lines(raw)
    except ValueError as error:
        raise ValueError(f"{source}: {error}") from error
    if target.exists():
        try:
            header = source_header(target.read_bytes())
        except ValueError as error:
            if not recover_header_from_git:
                raise ValueError(
                    f"{target}: invalid existing bitmap metadata ({error}); "
                    "use --recover-header-from-git to recover its Git HEAD header"
                ) from error
            header = _git_header(target)
    else:
        header = _git_header(target) if recover_header_from_git else bytes(HEADER_BYTES)
    result = header[:17] + bytes([1, 0, 0]) + raw
    return ImportPlan(target, result, strips, len(raw))


def _write_atomic(plan: ImportPlan) -> None:
    if plan.target.exists() and plan.target.read_bytes() == plan.data:
        return
    mode = stat.S_IMODE(plan.target.stat().st_mode) if plan.target.exists() else 0o644
    descriptor, temporary = tempfile.mkstemp(prefix=f".{plan.target.name}.", dir=plan.target.parent)
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(plan.data)
        os.chmod(temporary, mode)
        os.replace(temporary, plan.target)
    finally:
        Path(temporary).unlink(missing_ok=True)


def import_lines(sources: list[Path], data_dir: Path = DEFAULT_DATA_DIR, *,
                 glyph: int | None = None, recover_header_from_git: bool = False) -> list[ImportPlan]:
    if not sources:
        raise ValueError("provide at least one raw line source")
    if glyph is not None and len(sources) != 1:
        raise ValueError("--glyph can only be used with one source file")
    plans = []
    targets = set()
    for source in sources:
        plan = plan_import(source, data_dir, glyph=glyph,
                           recover_header_from_git=recover_header_from_git)
        if plan.target in targets:
            raise ValueError(f"duplicate glyph target: {plan.target}")
        targets.add(plan.target)
        plans.append(plan)
    # No target or directory is touched until every input and header is valid.
    for plan in plans:
        plan.target.parent.mkdir(parents=True, exist_ok=True)
        _write_atomic(plan)
    return plans


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("sources", nargs="+", type=Path, metavar="SOURCE.bin")
    parser.add_argument("--glyph", metavar="U+XXXX", help="target codepoint for one custom-named input")
    parser.add_argument("--data-dir", type=Path, default=DEFAULT_DATA_DIR,
                        help="canonical glyph directory (default: vendored MicroFont data/line-v1)")
    parser.add_argument("--recover-header-from-git", action="store_true",
                        help="explicitly recover missing/overwritten bitmap metadata from Git HEAD")
    args = parser.parse_args()
    try:
        glyph = parse_glyph(args.glyph) if args.glyph else None
        plans = import_lines(args.sources, args.data_dir, glyph=glyph,
                             recover_header_from_git=args.recover_header_from_git)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"microfont-import-lines: {error}\n")
    for plan in plans:
        print(f"{plan.target}: {plan.strips} strips, {plan.raster_bytes} raster bytes")
    print(f"Imported {len(plans)} glyphs.")


if __name__ == "__main__":
    main()
