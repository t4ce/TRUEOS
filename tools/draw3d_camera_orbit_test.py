#!/usr/bin/env python3
"""Exercise the optional draw3d camera orbit and pull two live PNG frames."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_house_demo import Draw3dClient, populate


def capture(client, path):
    output, image_format, width, height, image = client.render(path)
    if image_format != 2 or width <= 0 or height <= 0:
        raise RuntimeError("camera orbit test did not receive a live PNG target")
    digest = hashlib.sha256(image).hexdigest()
    print(
        f"capture format={image_format} size={width}x{height} bytes={len(image)} "
        f"sha256={digest} path={output}"
    )
    return output, digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=0.5)
    parser.add_argument("--interval", type=float, default=0.8)
    parser.add_argument("--speed", type=float, default=0.8, help="orbit radians per second")
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path("bld/draw3d-captures/camera-orbit"),
    )
    args = parser.parse_args()

    client = Draw3dClient(args.host)
    try:
        populate(client)
        client.camera(
            (14.0, 2.0, 0.0),
            (0.0, 2.0, 0.0),
            48.0,
            orbit_scale=(14.0, 9.0),
            orbit_rotation=(math.radians(-8.0), 0.0, math.radians(3.0)),
            orbit_speed=args.speed,
        )
        time.sleep(args.settle)
        first_path, first_hash = capture(client, args.output_dir / "orbit-frame-a.png")
        time.sleep(args.interval)
        second_path, second_hash = capture(client, args.output_dir / "orbit-frame-b.png")
        if args.speed != 0.0 and first_hash == second_hash:
            raise RuntimeError("nonzero camera orbit returned two byte-identical live frames")
        print(
            f"orbit speed={args.speed} distinct={int(first_hash != second_hash)} "
            f"frames={first_path},{second_path}"
        )
    finally:
        client.close()


if __name__ == "__main__":
    main()
