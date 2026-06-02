#!/usr/bin/env python3
"""Summarize known render packets from an oracle replay manifest.

This is intentionally narrow. It does not try to be a complete Intel batch
decoder; it finds the captured batch-start window from an oracle manifest and
prints the known 3DSTATE/3DPRIMITIVE packets that matter for the TRUEOS WM
frontier.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path


COMMANDS: dict[int, tuple[str, str]] = {
    0x7804: ("3DSTATE_CLEAR_PARAMS", "pipeline"),
    0x7805: ("3DSTATE_DEPTH_BUFFER", "pipeline"),
    0x7806: ("3DSTATE_STENCIL_BUFFER", "pipeline"),
    0x7807: ("3DSTATE_HIER_DEPTH_BUFFER", "pipeline"),
    0x7808: ("3DSTATE_VERTEX_BUFFERS", "VF"),
    0x7809: ("3DSTATE_VERTEX_ELEMENTS", "VF"),
    0x780A: ("3DSTATE_INDEX_BUFFER", "VF"),
    0x780B: ("3DSTATE_VF_STATISTICS", "VF"),
    0x780C: ("3DSTATE_VF", "VF"),
    0x780D: ("3DSTATE_VIEWPORT_STATE_POINTERS", "viewport"),
    0x780E: ("3DSTATE_CC_STATE_POINTERS", "CC"),
    0x7810: ("3DSTATE_VS", "VS"),
    0x7811: ("3DSTATE_GS", "GS"),
    0x7812: ("3DSTATE_CLIP", "CLIP"),
    0x7813: ("3DSTATE_SF", "SF"),
    0x7814: ("3DSTATE_WM", "WM"),
    0x7818: ("3DSTATE_SAMPLE_MASK", "WM"),
    0x781B: ("3DSTATE_HS", "HS"),
    0x781C: ("3DSTATE_TE", "TE"),
    0x781D: ("3DSTATE_DS", "DS"),
    0x781E: ("3DSTATE_STREAMOUT", "SOL"),
    0x781F: ("3DSTATE_SBE", "SF"),
    0x7820: ("3DSTATE_PS", "PS"),
    0x7821: ("3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP", "SF/CLIP"),
    0x7823: ("3DSTATE_VIEWPORT_STATE_POINTERS_CC", "CC"),
    0x7824: ("3DSTATE_BLEND_STATE_POINTERS", "blend"),
    0x7825: ("3DSTATE_DEPTH_STENCIL_STATE_POINTERS", "depth"),
    0x782A: ("3DSTATE_BINDING_TABLE_POINTERS_PS", "PS"),
    0x784B: ("3DSTATE_VF_TOPOLOGY", "VF"),
    0x784D: ("3DSTATE_PS_BLEND", "WM"),
    0x784E: ("3DSTATE_WM_DEPTH_STENCIL", "WM"),
    0x784F: ("3DSTATE_PS_EXTRA", "WM/PS"),
    0x7850: ("3DSTATE_RASTER", "SF/WM"),
    0x7851: ("3DSTATE_SBE_SWIZ", "SF"),
    0x7852: ("3DSTATE_WM_HZ_OP", "WM"),
    0x7858: ("3DSTATE_URB_ALLOC_VS", "URB"),
    0x7859: ("3DSTATE_URB_ALLOC_HS", "URB"),
    0x785A: ("3DSTATE_URB_ALLOC_DS", "URB"),
    0x785B: ("3DSTATE_URB_ALLOC_GS", "URB"),
    0x786C: ("3DSTATE_PRIMITIVE_REPLICATION", "pipeline"),
    0x7900: ("3DSTATE_DRAWING_RECTANGLE", "SF/WM"),
    0x790D: ("3DSTATE_MULTISAMPLE", "WM"),
    0x791C: ("3DSTATE_SAMPLE_PATTERN", "WM"),
    0x791D: ("3DSTATE_URB_CLEAR", "URB"),
    0x7A00: ("PIPE_CONTROL", "sync"),
    0x7B00: ("3DPRIMITIVE", "draw"),
}

FOCUS = {
    "3DSTATE_VIEWPORT_STATE_POINTERS_SF_CLIP",
    "3DSTATE_DRAWING_RECTANGLE",
    "3DSTATE_MULTISAMPLE",
    "3DSTATE_SAMPLE_PATTERN",
    "3DSTATE_SAMPLE_MASK",
    "3DSTATE_CLIP",
    "3DSTATE_SF",
    "3DSTATE_RASTER",
    "3DSTATE_SBE",
    "3DSTATE_SBE_SWIZ",
    "3DSTATE_WM",
    "3DSTATE_WM_HZ_OP",
    "3DSTATE_PS_BLEND",
    "3DSTATE_WM_DEPTH_STENCIL",
    "3DSTATE_PS_EXTRA",
    "3DSTATE_PS",
    "3DSTATE_PRIMITIVE_REPLICATION",
    "3DPRIMITIVE",
}


def dwords_from(path: Path) -> list[int]:
    data = path.read_bytes()
    return [int.from_bytes(data[i : i + 4], "little") for i in range(0, len(data) & ~3, 4)]


def command_key(word: int) -> int:
    return (word >> 16) & 0xFFFF


def packet_length(word: int) -> int:
    return (word & 0xFF) + 2


def choose_submit(manifest: dict) -> dict:
    submits = [s for s in manifest["submits"] if int(s.get("buffer_count") or 0) > 1]
    if not submits:
        raise SystemExit("manifest has no multi-object submits")
    return max(submits, key=lambda s: int(s.get("buffer_count") or 0))


def choose_batch_dump(manifest_path: Path, submit: dict) -> tuple[Path, int, dict]:
    trace_dir = Path(manifest_path.parent)
    batch_start = int(submit.get("batch_start") or 0)
    candidates: list[tuple[int, Path, int, dict]] = []
    for obj in submit.get("objects") or []:
        for dump in obj.get("dumps") or []:
            if not dump.get("exists"):
                continue
            dump_off = int(dump.get("offset") or 0)
            dump_len = int(dump.get("dump_len") or 0)
            if dump_off <= batch_start < dump_off + dump_len:
                score = 2 if dump_off == batch_start else 1
                candidates.append((score, trace_dir / dump["file"], batch_start - dump_off, obj))
    if not candidates:
        raise SystemExit(f"no dump covers batch_start=0x{batch_start:X}")
    candidates.sort(key=lambda item: (item[0], -int(item[3].get("index") or 0)), reverse=True)
    _score, path, byte_offset, obj = candidates[0]
    return path, byte_offset, obj


def print_packet(offset: int, words: list[int], index: int, focus_only: bool) -> None:
    word = words[index]
    name, stage = COMMANDS[command_key(word)]
    if focus_only and name not in FOCUS:
        return
    length = min(packet_length(word), len(words) - index)
    fields = " ".join(f"{value:08X}" for value in words[index : index + min(length, 12)])
    print(
        f"0x{offset + index * 4:05X} {name:<42} stage={stage:<7} "
        f"len_dw={length:<2} header=0x{word:08X} dwords={fields}"
    )


def scan(words: list[int], byte_offset: int, focus_only: bool) -> None:
    i = byte_offset // 4
    while i < len(words):
        key = command_key(words[i])
        if key in COMMANDS:
            print_packet(0, words, i, focus_only)
            i += max(1, packet_length(words[i]))
            continue
        i += 1


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "manifest",
        type=Path,
        nargs="?",
        default=Path(".codex_tmp/intel_userland_oracle/render-simple-triangle/replay_manifest.json"),
    )
    parser.add_argument("--all", action="store_true", help="include non-WM-frontier packets")
    args = parser.parse_args()

    manifest = json.loads(args.manifest.read_text())
    submit = choose_submit(manifest)
    dump_path, byte_offset, obj = choose_batch_dump(args.manifest, submit)
    print(
        "oracle-batch "
        f"submit_seq={submit.get('seq')} "
        f"buffers={submit.get('buffer_count')} "
        f"batch_start=0x{int(submit.get('batch_start') or 0):X} "
        f"batch_dump={dump_path} "
        f"dump_byte_offset=0x{byte_offset:X} "
        f"object_index={obj.get('index')} "
        f"handle={obj.get('handle')}"
    )
    scan(dwords_from(dump_path), byte_offset, focus_only=not args.all)


if __name__ == "__main__":
    main()
