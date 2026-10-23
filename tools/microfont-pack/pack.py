#!/usr/bin/env python3
"""Wrap a feature-selected portable MicroFont bundle for TRUEOS distribution."""

import argparse
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile


MEMBER_NAME = "microfont.bin"


def tar_payload(bundle: bytes) -> bytes:
    """The portable file is the only member; metadata does not depend on the host."""
    if not bundle:
        raise ValueError("the portable MicroFont bundle must not be empty")
    member = tarfile.TarInfo(MEMBER_NAME)
    member.size = len(bundle)
    member.mode = 0o644
    member.uid = member.gid = member.mtime = 0
    member.uname = member.gname = ""
    # Only the 512-byte member padding and two end markers are needed. Python's
    # default 10 KiB record padding would exceed TRUEOS's bound for tiny bundles.
    return member.tobuf(format=tarfile.USTAR_FORMAT) + bundle + bytes((-len(bundle)) % 512 + 1024)


def pack(bundle_path: Path, output_path: Path, lz4: str = "lz4") -> tuple[int, int]:
    """Preserve the selected file bytes inside a single standard tar/LZ4 frame."""
    bundle = bundle_path.read_bytes()
    payload = tar_payload(bundle)
    executable = shutil.which(lz4)
    if executable is None:
        raise ValueError(f"LZ4 compressor not found: {lz4}")
    output_path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".microfont-pack-", dir=output_path.parent) as tmp:
        temporary = Path(tmp)
        tar_path = temporary / "microfont.tar"
        packed_path = temporary / "microfont.tar.lz4"
        tar_path.write_bytes(payload)
        subprocess.run(
            [executable, "-q", "-12", "-BI", "--content-size", "-f", str(tar_path), str(packed_path)],
            check=True,
        )
        packed = packed_path.read_bytes()
        if not output_path.exists() or output_path.read_bytes() != packed:
            packed_path.replace(output_path)
    return len(bundle), len(packed)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path, help="uncompressed, feature-selected microfont.bin")
    parser.add_argument("--output", type=Path, required=True, help="TRUEOS microfont.tar.lz4")
    parser.add_argument("--lz4", default="lz4", help="LZ4 command (default: lz4)")
    args = parser.parse_args()
    try:
        source_bytes, packed_bytes = pack(args.input.resolve(), args.output.resolve(), args.lz4)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"microfont-pack: {error}\n")
    print(f"microfont-pack: {source_bytes} bytes -> {packed_bytes} bytes, one {MEMBER_NAME} member")


if __name__ == "__main__":
    main()
