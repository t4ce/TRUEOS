#!/usr/bin/env python3
"""Exercise Draw3D opaque depth and the alpha/depth boundary on real hardware."""

import argparse
import hashlib
import io
import time
from pathlib import Path

from PIL import Image

from draw3d_house_demo import Draw3dClient


WHITE = (255, 255, 255, 255)
RED = (224, 48, 48, 255)
GREEN = (40, 190, 72, 255)
BLUE = (45, 92, 220, 255)
TRANSLUCENT_RED = (224, 48, 48, 128)
TRANSLUCENT_BLUE = (45, 92, 220, 128)
ZERO_ALPHA_MAGENTA = (255, 0, 255, 0)
QUAD = ((-1.0, -1.0, 0.0), (1.0, -1.0, 0.0), (1.0, 1.0, 0.0), (-1.0, 1.0, 0.0))
QUAD_FACE = ((0, 1, 2, 3),)


def reset(client):
    client.stop()
    client.clear()
    client.camera((0.0, 0.0, 10.0), (0.0, 0.0, 0.0), 50.0)


def add_quad(client, mesh_id, color, z, scale):
    client.mesh(mesh_id, color, QUAD, QUAD_FACE)
    client.instance(10_000 + mesh_id, mesh_id, (0.0, 0.0, z), (scale, scale, 1.0))


def close_rgb(actual, expected, tolerance=12):
    return all(abs(actual[index] - expected[index]) <= tolerance for index in range(3))


def source_over(source, destination):
    alpha = source[3] / 255.0
    return tuple(round(source[index] * alpha + destination[index] * (1.0 - alpha)) for index in range(3)) + (255,)


def capture_case(client, output, expected_center, tolerance, settle):
    client.start(WHITE)
    time.sleep(settle)
    path, image_format, width, height, encoded = client.render(output)
    if image_format != 2 or width <= 0 or height <= 0:
        raise RuntimeError(f"expected a live PNG, got format={image_format} size={width}x{height}")
    image = Image.open(io.BytesIO(encoded)).convert("RGBA")
    center = image.getpixel((width // 2, height // 2))
    if center[3] != expected_center[3] or not close_rgb(center, expected_center, tolerance):
        raise RuntimeError(
            f"{output.stem}: center={center} expected={expected_center} tolerance={tolerance}"
        )
    print(
        f"PASS {output.stem}: center={center} expected={expected_center} "
        f"size={width}x{height} sha256={hashlib.sha256(encoded).hexdigest()} path={path}"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=0.2)
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path("bld/draw3d-captures/depth-alpha"),
    )
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)

    client = Draw3dClient(args.host)
    try:
        # Opaque objects are submitted front-to-back. The farther blue quad is
        # deliberately submitted second and must lose the fixed-function test.
        reset(client)
        add_quad(client, 101, BLUE, 0.0, 2.5)
        add_quad(client, 102, RED, 1.0, 1.2)
        capture_case(client, args.output_dir / "01_opaque_depth.png", RED, 4, args.settle)

        # Blended geometry is drawn after opaque geometry, but a translucent
        # object behind the opaque result must still fail read-only depth.
        reset(client)
        add_quad(client, 201, TRANSLUCENT_BLUE, 0.0, 2.5)
        add_quad(client, 202, RED, 1.0, 1.2)
        capture_case(client, args.output_dir / "02_alpha_behind_opaque.png", RED, 4, args.settle)

        # A translucent object in front must pass and blend over opaque color.
        reset(client)
        add_quad(client, 301, BLUE, 0.0, 2.5)
        add_quad(client, 302, TRANSLUCENT_RED, 1.0, 1.2)
        capture_case(
            client,
            args.output_dir / "03_alpha_in_front.png",
            source_over(TRANSLUCENT_RED, BLUE),
            12,
            args.settle,
        )

        # Transparent layers remain painter-ordered and never occlude each
        # other through depth writes: far blue first, then near red.
        reset(client)
        add_quad(client, 401, TRANSLUCENT_BLUE, 0.0, 2.5)
        add_quad(client, 402, TRANSLUCENT_RED, 1.0, 1.2)
        capture_case(
            client,
            args.output_dir / "04_transparent_order.png",
            source_over(TRANSLUCENT_RED, source_over(TRANSLUCENT_BLUE, WHITE)),
            12,
            args.settle,
        )

        # Alpha zero is rejected before residency/submission and cannot write
        # either color or depth in front of the green control quad.
        reset(client)
        add_quad(client, 501, GREEN, 0.0, 2.5)
        add_quad(client, 502, ZERO_ALPHA_MAGENTA, 1.0, 1.2)
        capture_case(client, args.output_dir / "05_zero_alpha.png", GREEN, 4, args.settle)
    finally:
        # Retain the last scene on the display, matching the other Draw3D tools.
        client.close()


if __name__ == "__main__":
    main()
