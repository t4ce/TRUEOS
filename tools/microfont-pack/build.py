#!/usr/bin/env python3
"""Build one selected MicroFont file, optionally wrapped for TRUEOS."""

import argparse
import os
from pathlib import Path
import subprocess
import tempfile


CRATE = Path(__file__).resolve().parents[2] / "vendor/microfont"


def features(value: str) -> tuple[str, ...]:
    selected = {part.strip() for part in value.split(",")}
    if not selected or not selected <= {"hex", "raster"}:
        raise argparse.ArgumentTypeError("select hex, raster, or hex,raster")
    return tuple(name for name in ("hex", "raster") if name in selected)


def build(selected: tuple[str, ...], output: Path, trueos: bool = False) -> int:
    with tempfile.TemporaryDirectory(prefix="microfont-build-") as tmp:
        work = Path(tmp)
        packer = work / "build-script"
        subprocess.run(["rustc", "--edition=2024", "-A", "dead_code", str(CRATE / "build.rs"),
                        "-o", str(packer)], check=True, capture_output=True, text=True)
        env = {key: value for key, value in os.environ.items()
               if not key.startswith("CARGO_FEATURE_") and key != "MICROFONT_SOURCE_DIR"}
        env.update(CARGO_MANIFEST_DIR=str(CRATE), OUT_DIR=str(work),
                   CARGO_CFG_TARGET_OS="trueos" if trueos else "none")
        env.update({"CARGO_FEATURE_" + name.upper(): "1" for name in selected})
        subprocess.run([str(packer)], env=env, check=True, capture_output=True, text=True)
        artifact = work / ("microfont.tar.lz4" if trueos else "microfont.bin")
        data = artifact.read_bytes()
        output.parent.mkdir(parents=True, exist_ok=True)
        if not output.exists() or output.read_bytes() != data:
            output.write_bytes(data)
        return len(data)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--features", type=features, default=features("hex,raster"),
                        help="hex, raster, or hex,raster (default)")
    parser.add_argument("--trueos", action="store_true", help="emit one TRUEOS tar/LZ4 archive")
    parser.add_argument("--output", type=Path, required=True, help="final single-file artifact")
    args = parser.parse_args()
    try:
        size = build(args.features, args.output.resolve(), args.trueos)
    except subprocess.CalledProcessError as error:
        parser.exit(1, f"microfont-build: {error.stderr or error}\n")
    except OSError as error:
        parser.exit(1, f"microfont-build: {error}\n")
    print(f"microfont-build: {','.join(args.features)} -> {args.output} ({size} bytes)")


if __name__ == "__main__":
    main()
