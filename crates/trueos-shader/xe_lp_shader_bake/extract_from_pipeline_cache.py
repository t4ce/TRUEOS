#!/usr/bin/env python3

from __future__ import annotations

import argparse
import re
from dataclasses import dataclass
from pathlib import Path


OFFSET_RE = re.compile(r"^0x([0-9a-fA-F]+):\s+(.*)$")


@dataclass
class ExecutableSlice:
    name: str
    stage: str
    start: int
    size: int


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Carve raw Intel shader bytes out of an ANV pipeline cache blob by "
            "matching stage spans derived from pipeline executable GEN assembly."
        )
    )
    parser.add_argument("cache_blob", type=Path)
    parser.add_argument("exec_dir", type=Path)
    parser.add_argument("--out-dir", type=Path, required=True)
    return parser.parse_args()


def last_instruction_size(line: str) -> int:
    return 8 if "compacted" in line else 16


def parse_assembly(path: Path) -> ExecutableSlice:
    offsets: list[tuple[int, str]] = []
    for line in path.read_text(errors="replace").splitlines():
        match = OFFSET_RE.match(line)
        if match:
            offsets.append((int(match.group(1), 16), match.group(2)))
    if not offsets:
        raise SystemExit(f"no instruction offsets found in {path}")

    start = offsets[0][0]
    last_off, last_text = offsets[-1]
    end = last_off + last_instruction_size(last_text)

    parts = path.name.split("_")
    if len(parts) < 4:
        raise SystemExit(f"unexpected executable dump filename: {path.name}")
    stage = parts[1]
    name = path.stem
    return ExecutableSlice(name=name, stage=stage, start=start, size=end - start)


def stage_enum(stage: str) -> int:
    mapping = {
        "vertex": 0,
        "fragment": 4,
    }
    try:
        return mapping[stage]
    except KeyError as exc:
        raise SystemExit(f"unsupported stage for extractor: {stage}") from exc


def find_stage_blob(blob: bytes, wanted_stage: int, wanted_size: int) -> tuple[int, bytes]:
    candidates: list[tuple[int, bytes]] = []
    for off in range(0, len(blob) - 8):
        stage = int.from_bytes(blob[off : off + 4], "little")
        size = int.from_bytes(blob[off + 4 : off + 8], "little")
        if stage != wanted_stage or size != wanted_size:
            continue
        data_off = off + 8
        data_end = data_off + size
        if data_end > len(blob):
            continue
        data = blob[data_off:data_end]
        if sum(b != 0 for b in data) <= size // 4:
            continue
        candidates.append((off, data))
    if not candidates:
        raise SystemExit(
            f"no candidate stage blob found for stage={wanted_stage} size={wanted_size}"
        )
    return candidates[0]


def write_bytes(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def main() -> None:
    args = parse_args()
    blob = args.cache_blob.read_bytes()
    args.out_dir.mkdir(parents=True, exist_ok=True)

    assembly_files = sorted(args.exec_dir.glob("*_GEN_Assembly.txt"))
    if not assembly_files:
        raise SystemExit(f"no GEN assembly files found in {args.exec_dir}")

    executables = [parse_assembly(path) for path in assembly_files]

    vertex_execs = [exe for exe in executables if exe.stage == "vertex"]
    fragment_execs = [exe for exe in executables if exe.stage == "fragment"]
    if len(vertex_execs) != 1:
        raise SystemExit(f"expected exactly one vertex executable, found {len(vertex_execs)}")
    if not fragment_execs:
        raise SystemExit("expected at least one fragment executable")

    vertex = vertex_execs[0]
    vs_off, vs_blob = find_stage_blob(blob, stage_enum("vertex"), vertex.size)
    write_bytes(args.out_dir / "triangle_vs.bin", vs_blob)
    print(f"vertex blob: off=0x{vs_off:X} size={len(vs_blob)} -> triangle_vs.bin")

    frag_base = min(exe.start for exe in fragment_execs)
    frag_end = max(exe.start + exe.size for exe in fragment_execs)
    frag_span = frag_end - frag_base
    fs_off, fs_blob = find_stage_blob(blob, stage_enum("fragment"), frag_span)
    write_bytes(args.out_dir / "triangle_ps_combined.bin", fs_blob)
    print(
        f"fragment combined blob: off=0x{fs_off:X} size={len(fs_blob)} -> triangle_ps_combined.bin"
    )

    for exe in fragment_execs:
        rel_off = exe.start - frag_base
        rel_end = rel_off + exe.size
        if rel_end > len(fs_blob):
            raise SystemExit(
                f"fragment executable slice out of range: {exe.name} rel_off={rel_off} size={exe.size}"
            )
        variant_name = "triangle_ps.bin"
        if "SIMD16" in exe.name or "fragment_01_GEN_Assembly" not in exe.name:
            if exe.start != frag_base:
                variant_name = "triangle_ps_simd8.bin"
            else:
                variant_name = "triangle_ps_simd16.bin"
        if exe.start == frag_base:
            variant_name = "triangle_ps_simd16.bin"
        elif rel_off == 64:
            variant_name = "triangle_ps.bin"
        write_bytes(args.out_dir / variant_name, fs_blob[rel_off:rel_end])
        print(
            f"fragment slice: {exe.name} rel_off=0x{rel_off:X} size={exe.size} -> {variant_name}"
        )


if __name__ == "__main__":
    main()
