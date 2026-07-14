#!/usr/bin/env python3
"""Paint a receding brutalist echo chamber on the TRUEOS draw3d screen."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh, radial_frustum
from draw3d_house_demo import CUBE_FACES, CUBE_VERTICES, Draw3dClient, OCTAHEDRON_FACES, OCTAHEDRON_VERTICES
from draw3d_null_meridian import faceted_orb


BACKGROUND = (5, 7, 13, 255)


def add_box(mesh, location, scale, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(CUBE_VERTICES, CUBE_FACES, location, scale, rotation)


def add_diamond(mesh, location, scale, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(OCTAHEDRON_VERTICES, OCTAHEDRON_FACES, location, scale, rotation)


def frame(mesh, center, width, height, depth, thickness, skew=0.0):
    x, y, z = center
    half_w, half_h = width * 0.5, height * 0.5
    add_box(mesh, (x - half_w, y, z), (thickness, half_h, depth), (0.0, 0.0, skew))
    add_box(mesh, (x + half_w, y, z), (thickness, half_h, depth), (0.0, 0.0, -skew))
    add_box(mesh, (x, y - half_h, z), (half_w, thickness, depth))
    add_box(mesh, (x, y + half_h, z), (half_w, thickness, depth))


def build_scene():
    # The chamber is almost black; its geometry is defined by the frames and floor cuts.
    walls = GardenMesh()
    add_box(walls, (-6.4, 2.6, -2.9), (0.16, 4.9, 3.8), (0.0, 0.0, -0.03))
    add_box(walls, (6.4, 2.6, -2.9), (0.16, 4.9, 3.8), (0.0, 0.0, 0.03))
    add_box(walls, (0.0, 7.45, -3.0), (6.4, 0.16, 3.7))
    # Four receding ceiling ribs make the vanishing point explicit.
    for x in (-4.8, -1.6, 1.6, 4.8):
        add_box(walls, (x, 5.4, -3.0), (0.10, 2.0, 3.3), (0.0, 0.0, 0.04 * (1 if x < 0 else -1)))

    floor = GardenMesh()
    floor.add_xyz(*radial_frustum(8, 1.0, 0.88, 0.34), (0.0, -1.52, -1.0), (6.6, 1.0, 4.4))
    floor.add_xyz(*radial_frustum(8, 0.83, 0.68, 0.20), (0.0, -1.22, -1.15), (5.2, 1.0, 3.4))
    # Perspective seams on the floor, wide in front and narrow at the vanishing point.
    for x in (-4.8, -2.4, 0.0, 2.4, 4.8):
        end_x = x * 0.18
        add_box(floor, ((x + end_x) * 0.5, -1.12, -3.0), (abs(x - end_x) * 0.5 + 0.03, 0.035, 0.045), (0.0, 0.0, math.atan2(1.12, end_x - x)))
    for z in (-0.3, -1.4, -2.7, -4.0, -5.0):
        add_box(floor, (0.0, -1.10 + 0.02 * (z + 5.0), z), (3.5 + (z + 5.0) * 0.26, 0.035, 0.035))

    # The pale structural frames are the primary architectural rhythm.
    ivory = GardenMesh()
    frame(ivory, (0.0, 2.75, -0.15), 8.9, 7.7, 0.09, 0.085, 0.018)
    frame(ivory, (0.0, 2.95, -1.85), 6.9, 6.2, 0.08, 0.075, -0.014)
    frame(ivory, (-0.15, 3.10, -3.45), 5.0, 4.7, 0.07, 0.065, 0.012)
    frame(ivory, (0.10, 3.18, -4.75), 3.35, 3.35, 0.06, 0.055, -0.01)

    # Rust frames are deliberately offset: a second coordinate system is sliding through the first.
    rust = GardenMesh()
    frame(rust, (0.28, 2.70, -0.78), 7.65, 6.7, 0.065, 0.065, -0.025)
    frame(rust, (-0.20, 3.02, -2.65), 5.55, 5.0, 0.055, 0.055, 0.02)
    add_box(rust, (-3.2, 2.0, -0.35), (0.05, 2.4, 0.05), (0.0, 0.0, -0.26))
    add_box(rust, (3.05, 3.65, -0.40), (0.05, 2.1, 0.05), (0.0, 0.0, 0.23))

    # A muted teal axis breaks the warm architecture and points toward the far object.
    teal = GardenMesh()
    frame(teal, (0.0, 2.95, -4.15), 2.25, 2.55, 0.05, 0.06, 0.0)
    add_box(teal, (0.0, 3.05, -3.0), (0.045, 2.8, 0.045))
    add_box(teal, (-1.4, 2.95, -3.1), (0.85, 0.045, 0.045), (0.0, 0.0, -0.16))
    add_box(teal, (1.35, 3.18, -3.15), (0.78, 0.045, 0.045), (0.0, 0.0, 0.18))

    # Distant suspended artifact: one faceted core with an asymmetric crown.
    core = GardenMesh()
    core.add_xyz(*faceted_orb(20, 6, 1.0), (0.0, 3.32, -5.72), (0.74, 0.92, 0.52), (0.0, 0.22, 0.0))
    add_diamond(core, (0.0, 4.30, -5.72), (0.26, 0.45, 0.18), (0.0, 0.0, math.pi / 4))
    add_diamond(core, (-0.42, 3.90, -5.68), (0.14, 0.28, 0.10), (0.0, 0.0, -math.pi / 4))
    add_diamond(core, (0.42, 3.86, -5.66), (0.14, 0.30, 0.10), (0.0, 0.0, math.pi / 4))

    relic = GardenMesh()
    add_diamond(relic, (0.0, 3.32, -5.26), (0.25, 0.44, 0.09), (0.0, 0.0, math.pi / 4))

    signal = GardenMesh()
    add_box(signal, (0.0, 3.08, -5.12), (0.055, 0.055, 0.55))
    add_box(signal, (0.0, 3.08, -4.38), (0.055, 0.055, 0.25))
    add_box(signal, (-2.25, 1.10, -0.05), (1.45, 0.045, 0.045), (0.0, 0.0, -0.06))
    add_box(signal, (2.25, 1.20, -0.06), (1.45, 0.045, 0.045), (0.0, 0.0, 0.06))

    marks = GardenMesh()
    for x, y, z, scale, angle in (
        (-5.1, 4.8, -0.12, 0.22, 0.1),
        (5.0, 4.4, -0.10, 0.20, -0.1),
        (-4.15, 0.15, -0.25, 0.16, -0.2),
        (4.0, 0.4, -0.30, 0.16, 0.2),
        (0.0, 5.85, -4.8, 0.18, 0.0),
    ):
        add_diamond(marks, (x, y, z), (scale, scale * 1.8, scale * 0.6), (0.0, 0.0, angle))

    return (
        (1901, (14, 21, 35, 255), walls),
        (1902, (42, 47, 54, 255), floor),
        (1903, (220, 208, 179, 255), ivory),
        (1904, (174, 71, 54, 255), rust),
        (1905, (55, 166, 155, 255), teal),
        (1906, (28, 25, 31, 255), core),
        (1907, (222, 163, 73, 255), signal),
        (1908, (218, 217, 194, 255), marks),
        (1909, (55, 190, 172, 255), relic),
    )


def populate(client):
    client.stop()
    client.clear()
    client.camera((8.5, 5.5, 17.5), (0.0, 2.55, -1.0), 45.0)
    for mesh_id, color, mesh in build_scene():
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(100_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start(BACKGROUND)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=2.0)
    parser.add_argument("--output", type=Path, default=Path("bld/draw3d-captures/echo-chamber-live.png"))
    args = parser.parse_args()

    client = Draw3dClient(args.host)
    try:
        populate(client)
        time.sleep(args.settle)
        output, image_format, width, height, image = client.render(args.output)
        stats = client.stats()
        print(
            f"scene meshes={stats[0]} instances={stats[1]} vertices={stats[2]} "
            f"edges={stats[3]} faces={stats[4]} mesh_bytes={stats[5]}"
        )
        print(
            f"capture format={image_format} size={width}x{height} bytes={len(image)} "
            f"sha256={hashlib.sha256(image).hexdigest()} path={output}"
        )
        if image_format != 2 or (width, height) != (512, 512):
            raise RuntimeError("live scene did not return the expected 512x512 PNG")
    finally:
        client.close()


if __name__ == "__main__":
    main()
