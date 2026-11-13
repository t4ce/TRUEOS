#!/usr/bin/env python3
"""Embed the procedural shader and seven existing Lilly frames for file:// use."""
import argparse
import base64
import io
import json
from pathlib import Path
import subprocess
import tarfile


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
ASSET = "Lilly/Idle/Crossed-Arms/idle-1_frames"


def lilly_frames():
    archive = subprocess.run(
        ["lz4", "-dc", str(ROOT / "tools/Lilly.tar.lz4")],
        check=True, stdout=subprocess.PIPE,
    ).stdout
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        return [tar.extractfile(f"{ASSET}/frame_{i:02}.png").read() for i in range(1, 8)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "bld/spirit-knot-preview.html")
    args = parser.parse_args()
    shader = (HERE / "knot.frag").read_text()
    frames = ["data:image/png;base64," + base64.b64encode(png).decode() for png in lilly_frames()]
    html = (HERE / "preview.html").read_text()
    html = html.replace("__KNOT_SHADER__", shader).replace("__LILLY_FRAMES__", json.dumps(frames))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(html)
    print(args.output.resolve().as_uri())


if __name__ == "__main__":
    main()
