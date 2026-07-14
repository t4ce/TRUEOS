#!/usr/bin/env python3
"""Capture focused visual diagnostics from the live TRUEOS draw3d service."""

import argparse
import hashlib
import io
import math
import struct
import time
from collections import Counter
from pathlib import Path

from PIL import Image, ImageDraw

from draw3d_house_demo import CUBE_FACES, CUBE_VERTICES, Draw3dClient


WHITE = (255, 255, 255, 255)
RED = (224, 48, 48, 255)
GREEN = (40, 190, 72, 255)
BLUE = (45, 92, 220, 255)
YELLOW = (235, 184, 38, 255)
PURPLE = (151, 68, 210, 255)
CYAN = (35, 190, 205, 255)
QUAD = ((-1.0, -1.0, 0.0), (1.0, -1.0, 0.0), (1.0, 1.0, 0.0), (-1.0, 1.0, 0.0))
QUAD_FACE = ((0, 1, 2, 3),)
TRIANGLE = ((0.0, 1.0, 0.0), (-1.0, -1.0, 0.0), (1.0, -1.0, 0.0))
TRIANGLE_FACE = ((0, 1, 2),)


def rgba_image(encoded):
    return Image.open(io.BytesIO(encoded)).convert("RGBA")


def color_count(image, color):
    return sum(pixel == color for pixel in image.get_flattened_data())


def near_color_count(image, color, rgb_tolerance=25, alpha_tolerance=0):
    return sum(
        all(abs(pixel[index] - color[index]) <= rgb_tolerance for index in range(3))
        and abs(pixel[3] - color[3]) <= alpha_tolerance
        for pixel in image.get_flattened_data()
    )


def near_color(pixel, color, rgb_tolerance=25, alpha_tolerance=0):
    return all(abs(pixel[index] - color[index]) <= rgb_tolerance for index in range(3)) and abs(
        pixel[3] - color[3]
    ) <= alpha_tolerance


def non_white_bbox(image):
    alpha = Image.new("L", image.size)
    alpha.putdata([0 if pixel == WHITE else 255 for pixel in image.get_flattened_data()])
    return alpha.getbbox()


class DiagnosticClient(Draw3dClient):
    def camera_planes(
        self,
        position,
        direction,
        near=0.1,
        far=100.0,
        fov_degrees=50.0,
    ):
        self.call(
            0x22,
            struct.pack(
                "<12f",
                *position,
                *direction,
                0.0,
                1.0,
                0.0,
                near,
                far,
                math.radians(fov_degrees),
            ),
        )

    def set_color(self, mesh_id, color):
        self.call(0x07, struct.pack("<Q4B", mesh_id, *color))

    def set_location(self, instance_id, location):
        self.call(0x15, struct.pack("<Q3f", instance_id, *location))

    def delete_instance(self, instance_id):
        self.call(0x11, struct.pack("<Q", instance_id))


def reset(client, position=(0.0, 0.0, 10.0), target=(0.0, 0.0, 0.0)):
    client.stop()
    client.clear()
    client.camera(position, target, 50.0)


def add_mesh_instance(
    client,
    mesh_id,
    color,
    vertices,
    faces,
    location=(0.0, 0.0, 0.0),
    scale=(1.0, 1.0, 1.0),
    rotation=(0.0, 0.0, 0.0),
    instance_id=None,
):
    client.mesh(mesh_id, color, vertices, faces)
    client.instance(instance_id or (10_000 + mesh_id), mesh_id, location, scale, rotation)


def capture(client, output, signature, timeout=4.0):
    deadline = time.monotonic() + timeout
    last = None
    previous_sha = getattr(client, "_diagnostic_last_sha", None)
    while time.monotonic() < deadline:
        path, image_format, width, height, encoded = client.render(output)
        if image_format != 2 or (width, height) != (512, 512):
            raise RuntimeError(f"expected 512x512 PNG, got format={image_format} {width}x{height}")
        image = rgba_image(encoded)
        last = (path, encoded, image)
        current_sha = hashlib.sha256(encoded).hexdigest()
        if current_sha != previous_sha and signature(image):
            break
        time.sleep(0.1)
    else:
        raise RuntimeError(f"timed out waiting for render signature: {output.name}")

    path, encoded, image = last
    client._diagnostic_last_sha = hashlib.sha256(encoded).hexdigest()
    colors = Counter(image.get_flattened_data())
    return {
        "name": output.stem,
        "path": path,
        "sha256": hashlib.sha256(encoded).hexdigest(),
        "bytes": len(encoded),
        "bbox": non_white_bbox(image),
        "non_white": 512 * 512 - colors[WHITE],
        "top_colors": colors.most_common(8),
        "center": image.getpixel((256, 256)),
    }


def run_cases(client, output_dir):
    results = []

    reset(client)
    client.start(WHITE)
    results.append(capture(client, output_dir / "01_clear_white.png", lambda im: color_count(im, WHITE) == 512 * 512))

    reset(client)
    for mesh_id, color, x in ((101, RED, -2.5), (102, GREEN, 0.0), (103, BLUE, 2.5)):
        add_mesh_instance(client, mesh_id, color, QUAD, QUAD_FACE, location=(x, 0.0, 0.0))
    client.start(WHITE)
    results.append(
        capture(
            client,
            output_dir / "02_rgb_placement.png",
            lambda im: all(near_color_count(im, color) > 1_000 for color in (RED, GREEN, BLUE)),
        )
    )

    reset(client)
    add_mesh_instance(client, 201, RED, TRIANGLE, TRIANGLE_FACE, location=(-2.8, 0.0, 0.0))
    add_mesh_instance(client, 202, GREEN, QUAD, QUAD_FACE, location=(0.0, 0.0, 0.0))
    pentagon = tuple(
        (math.cos(math.pi / 2 + index * 2 * math.pi / 5), math.sin(math.pi / 2 + index * 2 * math.pi / 5), 0.0)
        for index in range(5)
    )
    add_mesh_instance(client, 203, BLUE, pentagon, (tuple(range(5)),), location=(2.8, 0.0, 0.0), scale=(1.25, 1.25, 1.25))
    client.start(WHITE)
    results.append(
        capture(
            client,
            output_dir / "03_polygon_shapes.png",
            lambda im: all(near_color_count(im, color) > 500 for color in (RED, GREEN, BLUE)),
        )
    )

    reset(client, position=(7.0, 5.0, 11.0))
    add_mesh_instance(client, 301, PURPLE, CUBE_VERTICES, CUBE_FACES, location=(-2.5, 0.0, 0.0), scale=(0.65, 1.4, 0.65))
    client.instance(10302, 301, (0.0, 0.0, 0.0), (1.15, 0.65, 0.65), (0.0, 0.55, 0.35))
    client.instance(10303, 301, (2.8, 0.0, 0.0), (0.8, 0.8, 1.7), (0.45, -0.5, 0.0))
    client.start(WHITE)
    results.append(capture(client, output_dir / "04_instance_transforms.png", lambda im: near_color_count(im, PURPLE) > 2_000))

    reset(client)
    add_mesh_instance(client, 401, BLUE, QUAD, QUAD_FACE, scale=(2.5, 2.5, 1.0))
    add_mesh_instance(client, 402, RED, QUAD, QUAD_FACE, location=(0.0, 0.0, 1.0), scale=(1.15, 1.15, 1.0))
    client.start(WHITE)
    results.append(
        capture(
            client,
            output_dir / "05_depth_occlusion.png",
            lambda im: near_color(im.getpixel((256, 256)), RED) and near_color_count(im, BLUE) > 1_000,
        )
    )

    reset(client)
    alphas = (255, 192, 128, 64)
    for index, (alpha, x) in enumerate(zip(alphas, (-3.3, -1.1, 1.1, 3.3))):
        color = (35, 130, 225, alpha)
        add_mesh_instance(client, 500 + index, color, QUAD, QUAD_FACE, location=(x, 0.0, 0.0), scale=(0.85, 1.7, 1.0))
    client.start(WHITE)
    results.append(
        capture(
            client,
            output_dir / "06_alpha_ladder.png",
            lambda im: all(
                near_color_count(im, (35, 130, 225, alpha), alpha_tolerance=1) > 500
                for alpha in alphas
            ),
        )
    )

    reset(client)
    add_mesh_instance(client, 601, BLUE, QUAD, QUAD_FACE, scale=(2.5, 2.5, 1.0))
    translucent_red = (224, 48, 48, 128)
    add_mesh_instance(client, 602, translucent_red, QUAD, QUAD_FACE, location=(0.0, 0.0, 1.0), scale=(1.2, 1.2, 1.0))
    client.start(WHITE)
    results.append(
        capture(
            client,
            output_dir / "07_alpha_overlap.png",
            lambda im: near_color(im.getpixel((256, 256)), translucent_red) and near_color_count(im, BLUE) > 1_000,
        )
    )

    client.stop()
    client.clear()
    client.camera_planes((0.0, 0.0, 0.0), (0.0, 0.0, -1.0), near=1.0, far=10.0)
    add_mesh_instance(client, 701, RED, QUAD, QUAD_FACE, location=(-1.8, 0.0, -0.5), scale=(0.65, 0.65, 1.0))
    add_mesh_instance(client, 702, GREEN, QUAD, QUAD_FACE, location=(0.0, 0.0, -3.0), scale=(0.65, 0.65, 1.0))
    add_mesh_instance(client, 703, BLUE, QUAD, QUAD_FACE, location=(1.8, 0.0, -12.0), scale=(0.65, 0.65, 1.0))
    client.start(WHITE)
    results.append(
        capture(
            client,
            output_dir / "08_near_far_clipping.png",
            lambda im: near_color_count(im, GREEN) > 500
            and near_color_count(im, RED) == 0
            and near_color_count(im, BLUE) == 0,
        )
    )

    reset(client)
    add_mesh_instance(client, 801, YELLOW, TRIANGLE, TRIANGLE_FACE, location=(-1.8, 0.0, 0.0), scale=(1.3, 1.3, 1.0))
    add_mesh_instance(client, 802, CYAN, TRIANGLE, ((0, 2, 1),), location=(1.8, 0.0, 0.0), scale=(1.3, 1.3, 1.0))
    client.start(WHITE)
    results.append(
        capture(
            client,
            output_dir / "09_winding.png",
            lambda im: near_color_count(im, YELLOW) > 500 and near_color_count(im, CYAN) > 500,
        )
    )

    reset(client)
    add_mesh_instance(client, 901, RED, QUAD, QUAD_FACE, location=(-2.0, 0.0, 0.0))
    client.start(WHITE)
    results.append(capture(client, output_dir / "10a_update_before.png", lambda im: near_color_count(im, RED) > 1_000))
    client.set_color(901, GREEN)
    client.set_location(10901, (2.0, 0.0, 0.0))
    results.append(
        capture(
            client,
            output_dir / "10b_update_after.png",
            lambda im: near_color_count(im, GREEN) > 1_000 and near_color_count(im, RED) == 0,
        )
    )
    client.delete_instance(10901)
    results.append(capture(client, output_dir / "10c_delete_after.png", lambda im: color_count(im, WHITE) == 512 * 512))

    # Leave a useful diagnostic card live on the display after the destructive tests.
    reset(client)
    for row, (shape, faces) in enumerate(((TRIANGLE, TRIANGLE_FACE), (QUAD, QUAD_FACE))):
        for column, color in enumerate((RED, GREEN, BLUE)):
            mesh_id = 1_000 + row * 10 + column
            add_mesh_instance(
                client,
                mesh_id,
                color,
                shape,
                faces,
                location=((column - 1) * 2.7, (0.9 - row) * 2.3, 0.0),
                scale=(0.8, 0.8, 1.0),
            )
    client.start(WHITE)
    results.append(
        capture(
            client,
            output_dir / "11_final_diagnostic_card.png",
            lambda im: all(near_color_count(im, color) > 500 for color in (RED, GREEN, BLUE)),
        )
    )
    return results


def contact_sheet(results, output):
    thumb_size = (256, 256)
    columns = 4
    rows = math.ceil(len(results) / columns)
    sheet = Image.new("RGB", (columns * 256, rows * 292), (28, 30, 34))
    draw = ImageDraw.Draw(sheet)
    for index, result in enumerate(results):
        image = Image.open(result["path"]).convert("RGB")
        image.thumbnail(thumb_size)
        x = index % columns * 256
        y = index // columns * 292
        sheet.paste(image, (x, y))
        draw.text((x + 6, y + 261), result["name"], fill=(235, 235, 235))
    sheet.save(output)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--output-dir", type=Path, default=Path("bld/draw3d-diagnostics"))
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)

    client = DiagnosticClient(args.host)
    try:
        results = run_cases(client, args.output_dir)
        stats = client.stats()
    finally:
        client.close()

    contact_sheet(results, args.output_dir / "contact-sheet.png")
    for result in results:
        print(
            f"{result['name']}: path={result['path']} sha256={result['sha256']} "
            f"bytes={result['bytes']} non_white={result['non_white']} bbox={result['bbox']} "
            f"center={result['center']} colors={result['top_colors']}"
        )
    print(f"final_stats={stats}")


if __name__ == "__main__":
    main()
