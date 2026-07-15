#!/usr/bin/env python3
"""Add the kernel's embedded font to the celestial garden through draw3d TCP."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

from draw3d_celestial_garden import BACKGROUND, GardenMesh, build_scene
from draw3d_house_demo import CUBE_FACES, CUBE_VERTICES, Draw3dClient


KERNEL_FONT_PATH = Path(__file__).with_name("L_10646.TTF")
TITLE_TEXT = "TRUE OS"
TITLE_MESH_ID = 912
TITLE_INSTANCE_ID = 20_912


def merged_mask_rectangles(mask, threshold=96):
    """Merge equal horizontal mask runs into vertically extended rectangles."""
    pixels = mask.load()
    active = {}
    rectangles = []
    for y in range(mask.height):
        runs = []
        x = 0
        while x < mask.width:
            while x < mask.width and pixels[x, y] < threshold:
                x += 1
            if x == mask.width:
                break
            x0 = x
            while x < mask.width and pixels[x, y] >= threshold:
                x += 1
            runs.append((x0, x))

        continuing = {}
        for run in runs:
            if run in active:
                x0, x1, y0, _ = active.pop(run)
                continuing[run] = (x0, x1, y0, y + 1)
            else:
                continuing[run] = (run[0], run[1], y, y + 1)
        rectangles.extend(active.values())
        active = continuing
    rectangles.extend(active.values())
    return rectangles


def embedded_font_mesh(text=TITLE_TEXT, font_px=40, world_width=6.4, depth=0.12):
    """Voxel-extrude a glyph mask from the exact TTF embedded by the kernel."""
    font = ImageFont.truetype(str(KERNEL_FONT_PATH), font_px)
    left, top, right, bottom = font.getbbox(text)
    mask = Image.new("L", (right - left + 4, bottom - top + 4), 0)
    ImageDraw.Draw(mask).text((2 - left, 2 - top), text, font=font, fill=255)
    rectangles = merged_mask_rectangles(mask)
    pixel_scale = world_width / mask.width

    mesh = GardenMesh()
    for x0, x1, y0, y1 in rectangles:
        width = (x1 - x0) * pixel_scale
        height = (y1 - y0) * pixel_scale
        center_x = ((x0 + x1) * 0.5 - mask.width * 0.5) * pixel_scale
        center_y = (mask.height * 0.5 - (y0 + y1) * 0.5) * pixel_scale
        mesh.add_xyz(
            CUBE_VERTICES,
            CUBE_FACES,
            (center_x, center_y, 0.0),
            (width * 0.5, height * 0.5, depth * 0.5),
        )
    return mesh, mask.size, len(rectangles)


def title_rotation(location, camera_position):
    """Rotate a local XY title plane to face the scene camera."""
    dx = camera_position[0] - location[0]
    dy = camera_position[1] - location[1]
    dz = camera_position[2] - location[2]
    distance = math.sqrt(dx * dx + dy * dy + dz * dz)
    pitch = -math.asin(dy / distance)
    yaw = math.atan2(dx, dz)
    return pitch, yaw, 0.0


def populate(client):
    camera_position = (9.8, 6.6, 16.5)
    title_location = (0.35, 7.15, -1.8)

    client.stop()
    client.clear()
    client.camera(camera_position, (0.0, 1.35, 0.0), 47.0)
    for mesh_id, color, mesh in build_scene():
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(20_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))

    title, mask_size, rectangle_count = embedded_font_mesh()
    if len(title.vertices) > 1_000:
        raise RuntimeError(f"embedded font mesh exceeds vertex budget: {len(title.vertices)}")
    triangle_count = sum(len(face) - 2 for face in title.faces)
    if triangle_count > 2_000:
        raise RuntimeError(f"embedded font mesh exceeds triangle budget: {triangle_count}")
    client.mesh(TITLE_MESH_ID, (235, 225, 198, 255), title.vertices, title.faces)
    client.instance(
        TITLE_INSTANCE_ID,
        TITLE_MESH_ID,
        title_location,
        (1.0, 1.0, 1.0),
        title_rotation(title_location, camera_position),
    )
    client.start(BACKGROUND)
    return mask_size, rectangle_count, len(title.vertices), len(title.faces), triangle_count


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=5.0)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("bld/draw3d-captures/celestial-garden-embedded-font.png"),
    )
    args = parser.parse_args()

    client = Draw3dClient(args.host)
    try:
        mask_size, rectangles, vertices, faces, triangles = populate(client)
        time.sleep(args.settle)
        output, image_format, width, height, image = client.render(args.output)
        mesh_count, instance_count, scene_vertices, edges, scene_faces, mesh_bytes = client.stats()
        print(
            f'font source={KERNEL_FONT_PATH} text="{TITLE_TEXT}" mask={mask_size[0]}x{mask_size[1]} '
            f"rectangles={rectangles} vertices={vertices} faces={faces} triangles={triangles}"
        )
        print(
            f"scene meshes={mesh_count} instances={instance_count} vertices={scene_vertices} "
            f"edges={edges} faces={scene_faces} mesh_bytes={mesh_bytes}"
        )
        print(
            f"capture format={image_format} size={width}x{height} bytes={len(image)} "
            f"sha256={hashlib.sha256(image).hexdigest()} path={output}"
        )
        if image_format != 2 or width <= 0 or height <= 0:
            raise RuntimeError("live scene did not return a non-empty PNG target")
    finally:
        client.close()


if __name__ == "__main__":
    main()
