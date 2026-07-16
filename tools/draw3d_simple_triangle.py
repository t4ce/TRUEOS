#!/usr/bin/env python3
"""Present one simple triangle through the TRUEOS draw3d TCP service."""

import argparse
import hashlib
import time
from pathlib import Path

from draw3d_house_demo import Draw3dClient


TRIANGLE_MESH_ID = 31_001
TRIANGLE_INSTANCE_ID = 41_001
TRIANGLE_COLOR = (48, 112, 235, 255)
BACKGROUND_COLOR = (18, 24, 38, 255)
TRIANGLE_VERTICES = (
    (0.0, 2.7, 0.0),
    (-2.5, -1.7, 0.0),
    (2.5, -1.7, 0.0),
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=1.0)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("bld/draw3d-captures/guc-simple-triangle.png"),
    )
    args = parser.parse_args()

    client = Draw3dClient(args.host)
    try:
        client.stop()
        client.clear()
        client.camera((0.0, 0.0, 10.0), (0.0, 0.0, 0.0), 50.0)
        client.mesh(
            TRIANGLE_MESH_ID,
            TRIANGLE_COLOR,
            TRIANGLE_VERTICES,
            ((0, 1, 2),),
        )
        client.instance(
            TRIANGLE_INSTANCE_ID,
            TRIANGLE_MESH_ID,
            (0.0, 0.0, 0.0),
            (1.0, 1.0, 1.0),
        )
        client.start(BACKGROUND_COLOR)
        time.sleep(args.settle)

        output, image_format, width, height, image = client.render(args.output)
        stats = client.stats()
        if image_format != 2 or stats[:3] != (1, 1, 3) or stats[4] != 1:
            raise RuntimeError(
                "unexpected triangle response: "
                f"format={image_format} size={width}x{height} stats={stats}"
            )
        print(
            f"triangle presented size={width}x{height} bytes={len(image)} "
            f"sha256={hashlib.sha256(image).hexdigest()} stats={stats} path={output}"
        )
    finally:
        # Closing the TCP client intentionally leaves the retained scene live.
        client.close()


if __name__ == "__main__":
    main()
