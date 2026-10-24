#!/usr/bin/env python3
"""Verify app-owned shader publications and their kernel SHA-256 trust records."""
from pathlib import Path
import argparse
import hashlib
import json
import re
import struct
import subprocess
import tarfile
import io

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
    loader = (ROOT / "src/hv/mod.rs").read_text()
    launch = loader[loader.index("fn prepare_blueprint_launch_on_lane("):loader.index("pub fn stage_blueprint_launch(")]
    assert launch.index("enter_host_alloc_domain_current_cpu") < launch.index("register_blueprint_elf(") < launch.index("drop(host_alloc_guard)"), "app shader intake must run in the Blueprint host-allocation loader boundary"
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


def blueprint_packages(path):
    image = path.read_bytes()
    if image[:4] == b"TRBP":
        flags = struct.unpack_from("<H", image, 6)[0] & 255
        length = struct.unpack_from("<I", image, 16)[0]
        image = image[24:24+length]
        if flags == 3:
            decoded = subprocess.run(["lz4", "-d", "-c"], input=image, capture_output=True, check=True).stdout
            with tarfile.open(fileobj=io.BytesIO(decoded)) as archive:
                image = archive.extractfile("module.elf").read()
        elif flags != 1:
            raise ValueError("unsupported Blueprint payload")
    header = struct.unpack_from("<16sHHIQQQIHHHHHH", image)
    assert header[0][:6] == b"\x7fELF\x02\x01"
    table, stride, count, names_index = header[6], header[11], header[12], header[13]
    assert stride == 64
    names_header = struct.unpack_from("<IIQQQQIIQQ", image, table + stride * names_index)
    names = image[names_header[4]:names_header[4]+names_header[5]]
    packages = []
    for index in range(count):
        section = struct.unpack_from("<IIQQQQIIQQ", image, table + stride * index)
        name = names[section[0]:].split(b"\0", 1)[0]
        if name != b".trueos.shaders":
            continue
        assert section[2] & 2 == 0, "shader package must be non-ALLOC guest metadata"
        payload = image[section[4]:section[4]+section[5]]
        while payload:
            length = 16 + struct.unpack_from("<I", payload, 12)[0]
            package = payload[:length]
            identity, _ = decode(package)
            packages.append((identity, package))
            payload = payload[length:]
    return packages


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repos", type=Path, default=ROOT.parent)
    parser.add_argument("--kernel-elf", type=Path)
    parser.add_argument("--blueprint", type=Path, action="append", default=[])
    args = parser.parse_args()
    records = publications(args.repos)
    if args.kernel_elf:
        image = args.kernel_elf.read_bytes()
        for record, package, stages in records:
            assert package not in image, f"app package linked in kernel: {record['name']}"
            for _, stage in stages:
                assert stage not in image, f"app shader linked in kernel: {record['name']}"
    trusted = {r[0]["id"]: r[1] for r in records}
    for path in args.blueprint:
        carried = blueprint_packages(path)
        assert carried, f"no app shader packages in {path}"
        for identity, package in carried:
            assert package == trusted[identity], f"shader package differs: {path} id={identity}"
        print(f"{path.name}: verified non-ALLOC app shader IDs {[identity for identity, _ in carried]}")
    print(f"Verified {len(records)} app packages: hashes, stage data, owner bundles" +
          (", no app shader payloads in kernel" if args.kernel_elf else ""))


if __name__ == "__main__":
    main()
