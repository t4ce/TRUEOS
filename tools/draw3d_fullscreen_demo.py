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
RED = (220, 52, 56, 255)
GREEN = (42, 176, 92, 255)
BLUE = (48, 96, 220, 255)


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

    shapes = (
        (9001, RED, regular_polygon(3), (-4.6, 0.0, 0.0), (1.7, 1.7, 1.0)),
        (9002, GREEN, regular_polygon(4, phase=math.pi / 4.0), (0.0, 0.0, 0.0), (1.6, 1.6, 1.0)),
        (9003, BLUE, regular_polygon(6), (4.6, 0.0, 0.0), (1.7, 1.7, 1.0)),
    )
    for mesh_id, color, vertices, location, scale in shapes:
        client.mesh(mesh_id, color, vertices, (tuple(range(len(vertices))),))
        client.instance(10_000 + mesh_id, mesh_id, location, scale)
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
        counts = {color: count_near_color(image, color) for color in (RED, GREEN, BLUE)}
        minimum = max(1_000, width * height // 500)
        if any(count < minimum for count in counts.values()):
            raise RuntimeError(f"scene color proof failed: counts={counts} minimum={minimum}")
        stats = client.stats()
        print(
            f"socket_scene size={width}x{height} meshes={stats[0]} instances={stats[1]} "
            f"vertices={stats[2]} faces={stats[4]} colors={list(counts.values())}"
        )
        print(
            f"capture bytes={len(encoded)} sha256={hashlib.sha256(encoded).hexdigest()} "
            f"path={path}"
        )
    finally:
        client.close()


if __name__ == "__main__":
    main()
