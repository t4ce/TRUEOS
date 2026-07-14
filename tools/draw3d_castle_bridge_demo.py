#!/usr/bin/env python3
"""Show a castle and bridge on TRUEOS draw3d for fixed timed intervals."""

import argparse
import hashlib
import struct
import time
import zlib
from pathlib import Path

from draw3d_house_demo import CUBE_FACES, CUBE_VERTICES, Draw3dClient, MeshBuilder


WHITE = (255, 255, 255, 255)


def add_cube(mesh, location, scale, rotation_z=0.0):
    mesh.add(CUBE_VERTICES, CUBE_FACES, location, scale, rotation_z=rotation_z)


def castle_meshes():
    stone = MeshBuilder()
    add_cube(stone, (0.0, 2.2, 0.0), (2.8, 2.2, 1.7))

    towers = MeshBuilder()
    add_cube(towers, (-3.5, 2.2, 0.0), (1.1, 2.2, 1.1))
    add_cube(towers, (3.5, 2.2, 0.0), (1.1, 2.2, 1.1))

    roofs = MeshBuilder()
    add_cube(roofs, (0.0, 4.55, 0.0), (3.05, 0.24, 1.9))
    add_cube(roofs, (-3.5, 4.55, 0.0), (1.28, 0.24, 1.28))
    add_cube(roofs, (3.5, 4.55, 0.0), (1.28, 0.24, 1.28))

    accents = MeshBuilder()
    add_cube(accents, (0.0, 1.05, 1.74), (0.78, 1.05, 0.08))
    for x in (-1.45, 1.45):
        add_cube(accents, (x, 2.75, 1.74), (0.34, 0.48, 0.08))

    return (
        (701, (151, 158, 166, 255), stone),
        (702, (119, 128, 138, 255), towers),
        (703, (151, 47, 39, 255), roofs),
        (704, (76, 48, 36, 255), accents),
    )


def bridge_meshes():
    def component(mesh_id, color, location, scale, rotation_z=0.0):
        mesh = MeshBuilder()
        add_cube(mesh, location, scale, rotation_z=rotation_z)
        return mesh_id, color, mesh

    stone = (79, 87, 96, 255)
    roadway = (171, 74, 55, 255)
    cable = (226, 186, 86, 255)
    return (
        component(801, stone, (-4.2, 3.15, 0.0), (0.46, 2.15, 0.56)),
        # Keep the broad river behind the bridge in camera depth so the
        # painter-style renderer cannot cover the low deck.
        component(802, (58, 139, 190, 192), (0.0, -0.72, -3.5), (8.0, 0.14, 3.0)),
        component(803, stone, (0.0, 1.2, 0.0), (6.2, 0.28, 1.0)),
        component(804, stone, (4.2, 3.15, 0.0), (0.46, 2.15, 0.56)),
        component(805, roadway, (0.0, 1.52, 0.0), (6.1, 0.08, 0.72)),
        component(806, roadway, (0.0, 1.84, -0.92), (6.0, 0.12, 0.10)),
        component(807, roadway, (0.0, 1.84, 0.92), (6.0, 0.12, 0.10)),
        component(808, cable, (-2.1, 3.7, 0.0), (2.48, 0.065, 0.065), -0.55),
        component(809, cable, (2.1, 3.7, 0.0), (2.48, 0.065, 0.065), 0.55),
        component(810, cable, (0.0, 2.05, 0.0), (0.055, 0.45, 0.055)),
    )


def populate(client, camera_position, camera_target, meshes):
    client.stop()
    client.clear()
    client.camera(camera_position, camera_target, 50.0)
    for mesh_id, color, mesh in meshes:
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(10_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start(WHITE)


def capture(client, output):
    path, image_format, width, height, image = client.render(output)
    if image_format != 2 or (width, height) != (512, 512):
        raise RuntimeError("scene did not return a 512x512 PNG")
    changed = non_white_pixels(image)
    if changed == 0:
        raise RuntimeError("captured only the white clear frame")
    print(
        f"capture path={path} size={width}x{height} bytes={len(image)} "
        f"non_white_pixels={changed} sha256={hashlib.sha256(image).hexdigest()}"
    )


def non_white_pixels(png):
    offset = 8
    compressed = bytearray()
    while offset < len(png):
        length = struct.unpack(">I", png[offset : offset + 4])[0]
        kind = png[offset + 4 : offset + 8]
        data = png[offset + 8 : offset + 8 + length]
        if kind == b"IDAT":
            compressed.extend(data)
        offset += 12 + length
    raw = zlib.decompress(compressed)
    row_bytes = 512 * 4
    changed = 0
    for y in range(512):
        row = raw[y * (row_bytes + 1) : (y + 1) * (row_bytes + 1)]
        if row[0] != 0:
            raise RuntimeError("unexpected PNG row filter")
        changed += sum(row[x : x + 4] != b"\xff\xff\xff\xff" for x in range(1, len(row), 4))
    return changed


def show_for(client, name, duration, camera_position, camera_target, meshes, output):
    populate(client, camera_position, camera_target, meshes)
    started = time.monotonic()
    time.sleep(min(4.0, duration))
    capture(client, output)
    remaining = duration - (time.monotonic() - started)
    if remaining > 0.0:
        time.sleep(remaining)
    print(f"presented scene={name} seconds={time.monotonic() - started:.2f}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--seconds", type=float, default=20.0)
    parser.add_argument("--output-dir", type=Path, default=Path("bld/draw3d-captures"))
    args = parser.parse_args()

    client = Draw3dClient(args.host)
    try:
        show_for(
            client,
            "castle",
            args.seconds,
            (10.0, 7.0, 16.0),
            (0.0, 2.8, 0.0),
            castle_meshes(),
            args.output_dir / "castle-final.png",
        )
        show_for(
            client,
            "bridge",
            args.seconds,
            (10.0, 6.5, 16.0),
            (0.0, 1.2, 0.0),
            bridge_meshes(),
            args.output_dir / "bridge-final.png",
        )
        print("bridge remains running")
    finally:
        client.close()


if __name__ == "__main__":
    main()
