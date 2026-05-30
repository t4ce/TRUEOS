#!/usr/bin/env python3
"""Extract a small i915 execbuffer replay manifest from an oracle trace.

This is intentionally a dumb bridge tool. It does not understand Mesa, Vulkan,
or i915 deeply; it only records the concrete objects Mesa handed to
DRM_IOCTL_I915_GEM_EXECBUFFER2 and the BO dumps captured beside them.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path


EXEC_RE = re.compile(r"execbuffer-pre .*buffer_count=(0x[0-9A-Fa-f]+|\d+) .*batch_start=(0x[0-9A-Fa-f]+|\d+) .*batch_len=(0x[0-9A-Fa-f]+|\d+) .*flags=(0x[0-9A-Fa-f]+|\d+) .*rsvd1=(0x[0-9A-Fa-f]+|\d+) .*rsvd2=(0x[0-9A-Fa-f]+|\d+)")
OBJ_RE = re.compile(r"execbuffer-pre object\[(\d+)\] (.*)")
DUMP_RE = re.compile(r"bo-dump phase=([A-Za-z0-9_]+) handle=(\d+) .*offset=(0x[0-9A-Fa-f]+|\d+) .*dump_len=(0x[0-9A-Fa-f]+|\d+) .*first_words=([^ ]+) file=\"([^\"]+)\"")
COVER_RE = re.compile(r"bo-dump-batch-start-covered handle=(\d+) batch_start=(0x[0-9A-Fa-f]+|\d+) map_bo_offset=(0x[0-9A-Fa-f]+|\d+) map_len=(0x[0-9A-Fa-f]+|\d+)")
EXIT_RE = re.compile(r"ioctl-exit .*name=DRM_IOCTL_I915_GEM_EXECBUFFER2 ret=(-?\d+) errno=(-?\d+)")
SEQ_RE = re.compile(r"trace seq=(\d+)")
KV_RE = re.compile(r"([A-Za-z0-9_]+)=(0x[0-9A-Fa-f]+|\d+)")


def parse_int(value: str) -> int:
    return int(value, 0)


def seq_of(line: str) -> int | None:
    match = SEQ_RE.search(line)
    return int(match.group(1)) if match else None


def sha256_file(path: Path) -> str | None:
    if not path.is_file():
        return None
    digest = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def finish_submit(submit: dict | None, submits: list[dict]) -> None:
    if submit is not None:
        submit["dumped_object_count"] = sum(1 for obj in submit["objects"] if obj.get("dumps"))
        submit["missing_dump_count"] = sum(1 for obj in submit["objects"] if not obj.get("dumps"))
        submits.append(submit)


def parse_log(log_path: Path, include_hashes: bool) -> dict:
    trace_dir = log_path.parent
    submits: list[dict] = []
    submit: dict | None = None
    object_by_handle: dict[int, dict] = {}

    with log_path.open("r", encoding="utf-8", errors="replace") as f:
        for line in f:
            if "execbuffer-pre buffers_ptr=" in line:
                finish_submit(submit, submits)
                match = EXEC_RE.search(line)
                if not match:
                    continue
                submit = {
                    "seq": seq_of(line),
                    "buffer_count": parse_int(match.group(1)),
                    "batch_start": parse_int(match.group(2)),
                    "batch_len": parse_int(match.group(3)),
                    "flags": parse_int(match.group(4)),
                    "rsvd1": parse_int(match.group(5)),
                    "rsvd2": parse_int(match.group(6)),
                    "objects": [],
                    "batch_start_cover": None,
                    "ret": None,
                    "errno": None,
                }
                object_by_handle = {}
                continue

            if submit is None:
                continue

            match = OBJ_RE.search(line)
            if match:
                index = int(match.group(1))
                fields = {key: parse_int(value) for key, value in KV_RE.findall(match.group(2))}
                obj = {
                    "index": index,
                    "handle": fields.get("handle"),
                    "size": fields.get("size"),
                    "offset": fields.get("offset"),
                    "alignment": fields.get("alignment"),
                    "flags": fields.get("flags"),
                    "reloc_count": fields.get("reloc_count"),
                    "mapped": fields.get("mapped"),
                    "map_len": fields.get("map_len"),
                    "dumps": [],
                }
                submit["objects"].append(obj)
                if obj["handle"] is not None:
                    object_by_handle[obj["handle"]] = obj
                continue

            match = DUMP_RE.search(line)
            if match:
                handle = int(match.group(2))
                rel_file = match.group(6)
                dump_path = trace_dir / rel_file
                dump = {
                    "phase": match.group(1),
                    "offset": parse_int(match.group(3)),
                    "dump_len": parse_int(match.group(4)),
                    "first_words": match.group(5).split(","),
                    "file": rel_file,
                    "exists": dump_path.is_file(),
                    "file_size": dump_path.stat().st_size if dump_path.is_file() else None,
                }
                if include_hashes:
                    dump["sha256"] = sha256_file(dump_path)
                object_by_handle.setdefault(handle, {"handle": handle, "dumps": []})["dumps"].append(dump)
                continue

            match = COVER_RE.search(line)
            if match:
                submit["batch_start_cover"] = {
                    "handle": int(match.group(1)),
                    "batch_start": parse_int(match.group(2)),
                    "map_bo_offset": parse_int(match.group(3)),
                    "map_len": parse_int(match.group(4)),
                }
                continue

            match = EXIT_RE.search(line)
            if match:
                submit["ret"] = int(match.group(1))
                submit["errno"] = int(match.group(2))
                continue

    finish_submit(submit, submits)
    return {
        "trace_dir": str(trace_dir),
        "log": str(log_path),
        "submit_count": len(submits),
        "submits": submits,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("log", type=Path)
    parser.add_argument("--hash", action="store_true", help="include sha256 for dump files")
    parser.add_argument("--main-only", action="store_true", help="print only the largest buffer_count submit")
    args = parser.parse_args()

    manifest = parse_log(args.log, args.hash)
    if args.main_only and manifest["submits"]:
        main_submit = max(manifest["submits"], key=lambda item: item["buffer_count"])
        manifest = {**manifest, "submit_count": 1, "submits": [main_submit]}
    print(json.dumps(manifest, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
