#!/usr/bin/env python3
"""Verify app-owned shader publications and their kernel SHA-256 trust records."""
from pathlib import Path
import argparse
import hashlib
import json
import re
import struct

ROOT = Path(__file__).resolve().parents[2]
OWNERS = {"voxy": "voxy", "picasso": "TRUEOS-Picasso", "cubes": "Cubes"}


def decode(data):
    if data[:8] != b"APSHDR1\0" or len(data) < 20:
        raise ValueError("bad shader package header")
    identity, size, count = struct.unpack_from("<III", data, 8)
    if size + 16 != len(data) or not 0 < count <= 8 or len(data) > 4 * 1024 * 1024:
        raise ValueError("bad shader package size")
    cursor = 20
    stages = []
    for _ in range(count):
        kind, length = struct.unpack_from("<II", data, cursor)
        cursor += 8
        if kind > 1 or not length or (kind == 1 and length % 4):
            raise ValueError("bad shader stage layout")
        stage = data[cursor:cursor + length]
        if len(stage) != length:
            raise ValueError("truncated shader stage")
        stages.append((kind, stage))
        cursor += length
    if cursor != len(data):
        raise ValueError("trailing shader data")
    return identity, stages


def publications(repos):
    records = []
    trust = (ROOT / "src/gpu/app_shader_trust.rs").read_text()
    for owner, repo in OWNERS.items():
        directory = repos / repo / "shaders/trueos"
        bundle = bytearray()
        for record in json.loads((directory / "manifest.json").read_text()):
            data = (directory / (record["name"] + ".apshader")).read_bytes()
            identity, stages = decode(data)
            digest = hashlib.sha256(data).hexdigest()
            assert identity == record["id"] and len(data) == record["bytes"]
            assert digest == record["sha256"], record["name"]
            for (_, stage), provenance in zip(stages, record["stages"], strict=True):
                assert hashlib.sha256(stage).hexdigest() == provenance["sha256"]
            signature = ", ".join("0x" + digest[i:i+2] for i in range(0, 64, 2))
            assert f"({identity}, {len(data)}, [{signature}])" in trust, record["name"]
            records.append((record, data, stages))
            bundle.extend(data)
        assert bytes(bundle) == (directory / "app-shaders.bundle").read_bytes(), owner
    assert len(records) == 10 and len({r[0]["id"] for r in records}) == 10
    return records


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repos", type=Path, default=ROOT.parent)
    parser.add_argument("--kernel-elf", type=Path)
    args = parser.parse_args()
    records = publications(args.repos)
    if args.kernel_elf:
        image = args.kernel_elf.read_bytes()
        for record, package, stages in records:
            assert package not in image, f"app package linked in kernel: {record['name']}"
            for _, stage in stages:
                assert stage not in image, f"app shader linked in kernel: {record['name']}"
    print(f"Verified {len(records)} app packages: hashes, stage data, owner bundles" +
          (", no app shader payloads in kernel" if args.kernel_elf else ""))


if __name__ == "__main__":
    main()
