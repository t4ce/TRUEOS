#!/usr/bin/env python3
"""Socket-only 16:9 scene proof for the TRUEOS Draw3D service."""

import argparse
import hashlib
import io
import math
import time
from pathlib import Path

from PIL import Image

from draw3d_house_demo import Draw3dClient


WHITE = (255, 255, 255, 255)
ACCENT = (40, 96, 220, 255)


def regular_polygon(sides, radius=1.0, phase=math.pi / 2.0):
    return tuple(
        (
            radius * math.cos(phase + index * math.tau / sides),
            radius * math.sin(phase + index * math.tau / sides),
            0.0,
        )
        for index in range(sides)
    )


def populate(client):
    client.stop()
    client.clear()
    client.camera((0.0, 0.0, 10.0), (0.0, 0.0, 0.0), 50.0)

    # A deliberately simple, large centered triangle makes target placement
    # and stale/cropped-frame failures obvious without depending on any
    # multi-draw shader-state experiment.
    vertices = regular_polygon(3, 2.8)
    client.mesh(9001, ACCENT, vertices, ((0, 1, 2),))
    client.instance(19_001, 9001, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start(WHITE)


def count_near_color(image, expected, tolerance=12):
    return sum(
        all(abs(pixel[channel] - expected[channel]) <= tolerance for channel in range(4))
        for pixel in image.get_flattened_data()
    )


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--width", type=int, default=2560)
    parser.add_argument("--height", type=int, default=1440)
    parser.add_argument("--settle", type=float, default=1.5)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("bld/draw3d-captures/fullscreen-socket-proof.png"),
    )
    args = parser.parse_args()

    client = Draw3dClient(args.host)
    try:
        populate(client)
        time.sleep(args.settle)
        path, image_format, width, height, encoded = client.render(args.output)
        if image_format != 2 or (width, height) != (args.width, args.height):
            raise RuntimeError(
                f"expected PNG {args.width}x{args.height}, got format={image_format} "
                f"{width}x{height}"
            )
        image = Image.open(io.BytesIO(encoded)).convert("RGBA")
        accent_pixels = count_near_color(image, ACCENT, tolerance=20)
        minimum = max(1_000, width * height // 500)
        if accent_pixels < minimum:
            raise RuntimeError(
                f"scene color proof failed: accent_pixels={accent_pixels} minimum={minimum}"
            )
        stats = client.stats()
        if stats[0:2] != (1, 1):
            raise RuntimeError(f"expected one mesh and one instance, got stats={stats}")
        print(
            f"socket_scene size={width}x{height} meshes={stats[0]} instances={stats[1]} "
            f"vertices={stats[2]} faces={stats[4]} accent_pixels={accent_pixels}"
        )
        print(
            f"capture bytes={len(encoded)} sha256={hashlib.sha256(encoded).hexdigest()} "
            f"path={path}"
        )
    finally:
        client.close()


if __name__ == "__main__":
    main()
