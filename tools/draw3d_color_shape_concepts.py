#!/usr/bin/env python3
"""Cycle through ten small one-mesh color/shape concepts at a gentle cadence."""

import argparse
import hashlib
import io
import math
import time
from pathlib import Path

from PIL import Image

from draw3d_house_demo import Draw3dClient


WHITE = (255, 255, 255, 255)


def regular_polygon(sides, radius=2.8, phase=math.pi / 2.0):
    return tuple(
        (
            radius * math.cos(phase + index * math.tau / sides),
            radius * math.sin(phase + index * math.tau / sides),
            0.0,
        )
        for index in range(sides)
    )


def star(outer=2.9, inner=1.25, points=5):
    vertices = []
    for index in range(points * 2):
        radius = outer if index % 2 == 0 else inner
        angle = math.pi / 2.0 + index * math.tau / (points * 2)
        vertices.append((radius * math.cos(angle), radius * math.sin(angle), 0.0))
    return tuple(vertices)


CONCEPTS = (
    ("cobalt-triangle", (40, 96, 220, 255), regular_polygon(3), (0, 1, 2)),
    ("coral-square", (235, 92, 82, 255), regular_polygon(4, phase=math.pi / 4), tuple(range(4))),
    ("gold-diamond", (226, 166, 40, 255), regular_polygon(4), tuple(range(4))),
    ("violet-pentagon", (132, 76, 204, 255), regular_polygon(5), tuple(range(5))),
    ("teal-hexagon", (28, 164, 154, 255), regular_polygon(6), tuple(range(6))),
    ("magenta-star", (214, 54, 148, 255), star(), tuple(range(10))),
    ("lime-octagon", (116, 182, 56, 255), regular_polygon(8), tuple(range(8))),
    (
        "orange-arrow",
        (234, 116, 34, 255),
        ((-2.8, -1.2, 0.0), (0.7, -1.2, 0.0), (0.7, -2.3, 0.0), (2.8, 0.0, 0.0),
         (0.7, 2.3, 0.0), (0.7, 1.2, 0.0), (-2.8, 1.2, 0.0)),
        tuple(range(7)),
    ),
    (
        "aqua-house",
        (36, 174, 190, 255),
        ((-2.7, -2.2, 0.0), (2.7, -2.2, 0.0), (2.7, 0.1, 0.0), (0.0, 2.8, 0.0),
         (-2.7, 0.1, 0.0)),
        tuple(range(5)),
    ),
    ("ink-circle", (42, 48, 60, 255), regular_polygon(12), tuple(range(12))),
)


def count_near_color(image, expected, tolerance=18):
    return sum(
        all(abs(pixel[channel] - expected[channel]) <= tolerance for channel in range(4))
        for pixel in image.get_flattened_data()
    )


def show_concept(client, index, concept, output_dir, cadence, expected_width, expected_height):
    name, color, vertices, face = concept
    client.stop()
    client.clear()
    client.camera((0.0, 0.0, 10.0), (0.0, 0.0, 0.0), 50.0)
    client.mesh(30_000 + index, color, vertices, (face,))
    client.instance(40_000 + index, 30_000 + index, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start(WHITE)

    time.sleep(cadence)
    output_path = output_dir / f"{index:02d}-{name}.png"
    output, image_format, actual_width, actual_height, encoded = client.render(output_path)
    image = Image.open(io.BytesIO(encoded)).convert("RGBA")
    accent_pixels = count_near_color(image, color)
    stats = client.stats()
    print(
        f"{index:02d} {name} color={color[:3]} size={actual_width}x{actual_height} "
        f"accent_pixels={accent_pixels} meshes={stats[0]} instances={stats[1]} "
        f"sha256={hashlib.sha256(encoded).hexdigest()} path={output}"
    )
    if image_format != 2 or (actual_width, actual_height) != (expected_width, expected_height):
        raise RuntimeError(
            f"expected {expected_width}x{expected_height} PNG, got "
            f"format={image_format} {actual_width}x{actual_height}"
        )
    if accent_pixels < max(500, actual_width * actual_height // 500):
        raise RuntimeError(f"{name} produced too few colored pixels: {accent_pixels}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--cadence", type=float, default=0.5)
    parser.add_argument("--width", type=int, default=2560)
    parser.add_argument("--height", type=int, default=1440)
    parser.add_argument("--output-dir", type=Path, default=Path("bld/draw3d-captures/color-shape-concepts"))
    args = parser.parse_args()
    if args.cadence < 0.5:
        raise SystemExit("cadence must be at least 0.5 seconds for this gentle demo")

    client = Draw3dClient(args.host)
    try:
        for index, concept in enumerate(CONCEPTS, start=1):
            show_concept(
                client, index, concept, args.output_dir, args.cadence,
                args.width, args.height,
            )
    finally:
        client.close()


if __name__ == "__main__":
    main()
