#!/usr/bin/env python3
"""Paint a folded Möbius archive on the TRUEOS draw3d screen."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh, radial_frustum
from draw3d_house_demo import CUBE_FACES, CUBE_VERTICES, Draw3dClient, OCTAHEDRON_FACES, OCTAHEDRON_VERTICES
from draw3d_null_meridian import faceted_orb


BACKGROUND = (5, 7, 14, 255)


def add_box(mesh, location, scale, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(CUBE_VERTICES, CUBE_FACES, location, scale, rotation)


def add_diamond(mesh, location, scale, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(OCTAHEDRON_VERTICES, OCTAHEDRON_FACES, location, scale, rotation)


def mobius_strip(radius=2.55, half_width=0.52, segments=96, across=6):
    """A faceted Möbius band with a genuine half-twist around its loop."""
    vertices = []
    for index in range(segments):
        u = math.tau * index / segments
        radial = (math.cos(u), math.sin(u), 0.0)
        twist = u * 0.5
        for side in range(across):
            width = -half_width + 2.0 * half_width * side / (across - 1)
            offset = (
                width * math.cos(twist) * radial[0],
                width * math.cos(twist) * radial[1],
                width * math.sin(twist),
            )
            vertices.append((radius * radial[0] + offset[0], radius * radial[1] + offset[1], offset[2]))
    faces = []
    for index in range(segments):
        next_index = (index + 1) % segments
        for side in range(across - 1):
            faces.append(
                (
                    index * across + side,
                    next_index * across + side,
                    next_index * across + side + 1,
                    index * across + side + 1,
                )
            )
    return tuple(vertices), tuple(faces)


def add_strut(mesh, start, end, thickness=0.06):
    x1, y1, z1 = start
    x2, y2, z2 = end
    dx, dy = x2 - x1, y2 - y1
    length = math.hypot(dx, dy)
    if length < 0.001:
        return
    add_box(mesh, ((x1 + x2) * 0.5, (y1 + y2) * 0.5, (z1 + z2) * 0.5), (length * 0.5, thickness, thickness), (0.0, 0.0, math.atan2(dy, dx)))


def build_scene():
    backdrop = GardenMesh()
    # A sparse museum wall: mostly negative space, with two vertical scale references.
    for x, y, height, lean in (
        (-5.6, 2.0, 6.4, -0.08),
        (-4.5, 0.7, 3.6, 0.10),
        (4.9, 1.6, 4.5, 0.08),
        (6.0, 3.4, 6.8, -0.06),
    ):
        add_box(backdrop, (x, y, -6.2), (0.16, height, 0.16), (0.0, 0.0, lean))

    base = GardenMesh()
    base.add_xyz(*radial_frustum(10, 1.0, 0.88, 0.36), (0.0, -1.48, 0.0), (5.4, 1.0, 3.0))
    base.add_xyz(*radial_frustum(10, 0.80, 0.65, 0.18), (0.0, -1.15, 0.0), (4.35, 1.0, 2.35))
    add_box(base, (0.0, -0.79, 0.0), (3.5, 0.04, 1.5))

    # The main form is a wide surface, not a tube. The half-twist creates changing face orientation
    # and a silhouette that cannot be mistaken for a stock torus or ring.
    ribbon = GardenMesh()
    ribbon.add_xyz(*mobius_strip(2.55, 0.64), (0.0, 3.20, 0.15), (1.0, 1.0, 1.0), (0.30, 0.22, -0.10))

    # A narrow inner echo sits behind the wide band and gives the fold a second scale.
    echo = GardenMesh()
    echo.add_xyz(*mobius_strip(1.88, 0.18, 72, 4), (0.32, 3.22, -1.50), (1.0, 1.0, 1.0), (0.22, -0.18, 0.18))

    core = GardenMesh()
    core.add_xyz(*faceted_orb(24, 7, 1.0), (0.0, 3.18, 1.15), (0.84, 0.84, 0.64), (0.0, 0.15, 0.0))
    add_diamond(core, (0.0, 3.18, 1.82), (0.22, 0.48, 0.16), (0.0, 0.0, math.pi / 4))

    # Suspend the archive from a few deliberately visible struts, but keep them shallow and local.
    supports = GardenMesh()
    add_strut(supports, (-2.2, 1.10, 1.15), (-1.0, 2.15, 0.70), 0.07)
    add_strut(supports, (2.2, 1.15, 1.05), (1.0, 2.18, 0.72), 0.07)
    add_strut(supports, (-2.0, 5.10, 0.88), (-0.90, 4.24, 0.48), 0.055)
    add_strut(supports, (2.0, 5.00, 0.90), (0.90, 4.22, 0.50), 0.055)

    # Terracotta tabs sit at the twist extrema; teal facets sit at the opposite side of the fold.
    tabs = GardenMesh()
    for angle in (0.0, math.pi, math.pi * 0.5, math.pi * 1.5):
        x = 2.55 * math.cos(angle)
        y = 3.20 + 2.55 * math.sin(angle)
        z = 0.15 + 0.52 * math.sin(angle * 0.5)
        add_diamond(tabs, (x, y, z + 0.30), (0.20, 0.38, 0.16), (0.0, 0.0, angle))
    for angle in (math.pi / 4, 3 * math.pi / 4, 5 * math.pi / 4, 7 * math.pi / 4):
        x = 2.55 * math.cos(angle)
        y = 3.20 + 2.55 * math.sin(angle)
        z = 0.15 + 0.52 * math.sin(angle * 0.5)
        add_diamond(tabs, (x, y, z - 0.26), (0.14, 0.28, 0.12), (0.0, 0.0, angle))

    # Registration marks make the sculpture feel observed and measured, not merely ornamental.
    marks = GardenMesh()
    for x, y, z, width, angle in (
        (-3.8, -0.64, 1.45, 0.52, 0.08),
        (-2.9, -0.57, 1.55, 0.34, -0.18),
        (2.6, -0.62, 1.52, 0.44, 0.16),
        (3.55, -0.66, 1.45, 0.60, -0.08),
        (0.0, -0.54, 1.58, 1.15, 0.0),
        (0.0, 6.15, 0.80, 0.72, 0.0),
    ):
        add_box(marks, (x, y, z), (width, 0.035, 0.035), (0.0, 0.0, angle))

    return (
        (1601, (17, 24, 38, 255), backdrop),
        (1602, (44, 49, 56, 255), base),
        (1603, (216, 196, 159, 255), ribbon),
        (1604, (59, 103, 118, 255), echo),
        (1605, (28, 25, 32, 255), core),
        (1606, (178, 76, 55, 255), supports),
        (1607, (54, 179, 164, 255), tabs),
        (1608, (222, 163, 74, 255), marks),
    )


def populate(client):
    client.stop()
    client.clear()
    client.camera((10.8, 6.0, 15.8), (0.0, 2.72, 0.16), 44.0)
    for mesh_id, color, mesh in build_scene():
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(70_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start(BACKGROUND)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=2.0)
    parser.add_argument("--output", type=Path, default=Path("bld/draw3d-captures/mobius-archive-live.png"))
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
