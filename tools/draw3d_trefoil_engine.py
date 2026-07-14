#!/usr/bin/env python3
"""Paint a dense procedural trefoil engine on the TRUEOS draw3d screen."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh, radial_frustum, torus
from draw3d_house_demo import CUBE_FACES, CUBE_VERTICES, Draw3dClient, OCTAHEDRON_FACES, OCTAHEDRON_VERTICES
from draw3d_null_meridian import faceted_orb


BACKGROUND = (5, 8, 15, 255)


def add_box(mesh, location, scale, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(CUBE_VERTICES, CUBE_FACES, location, scale, rotation)


def add_diamond(mesh, location, scale, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(OCTAHEDRON_VERTICES, OCTAHEDRON_FACES, location, scale, rotation)


def tube_trefoil(major_radius=2.2, center_radius=0.78, tube_radius=0.16, turns=2, segments=96, sides=6):
    """A faceted (2,3)-style knot in the camera-facing XY plane."""
    vertices = []
    for index in range(segments):
        t = math.tau * index / segments
        angle = turns * t
        orbit = 3 * t
        center = (
            (major_radius + center_radius * math.cos(orbit)) * math.cos(angle),
            (major_radius + center_radius * math.cos(orbit)) * math.sin(angle),
            center_radius * math.sin(orbit) * 0.68,
        )
        radial = (math.cos(angle), math.sin(angle), 0.0)
        binormal = (0.0, 0.0, 1.0)
        for side in range(sides):
            section = math.tau * side / sides
            offset = (
                tube_radius * (math.cos(section) * radial[0] + math.sin(section) * binormal[0]),
                tube_radius * (math.cos(section) * radial[1] + math.sin(section) * binormal[1]),
                tube_radius * (math.cos(section) * radial[2] + math.sin(section) * binormal[2]),
            )
            vertices.append(tuple(center[axis] + offset[axis] for axis in range(3)))
    faces = []
    for index in range(segments):
        next_index = (index + 1) % segments
        for side in range(sides):
            next_side = (side + 1) % sides
            faces.append(
                (
                    index * sides + side,
                    next_index * sides + side,
                    next_index * sides + next_side,
                    index * sides + next_side,
                )
            )
    return tuple(vertices), tuple(faces)


def add_strut(mesh, start, end, thickness=0.055):
    x1, y1, z1 = start
    x2, y2, z2 = end
    dx, dy = x2 - x1, y2 - y1
    length = math.hypot(dx, dy)
    if length < 0.001:
        return
    add_box(
        mesh,
        ((x1 + x2) * 0.5, (y1 + y2) * 0.5, (z1 + z2) * 0.5),
        (length * 0.5, thickness, thickness),
        (0.0, 0.0, math.atan2(dy, dx)),
    )


def build_scene():
    backdrop = GardenMesh()
    for x, y, height, lean in (
        (-5.9, 2.0, 5.8, -0.10),
        (-4.7, 0.8, 3.8, 0.11),
        (4.8, 1.5, 4.8, 0.09),
        (6.1, 3.6, 6.7, -0.07),
    ):
        add_box(backdrop, (x, y, -6.4), (0.17, height, 0.17), (0.0, 0.0, lean))

    base = GardenMesh()
    base.add_xyz(*radial_frustum(10, 1.0, 0.90, 0.38), (0.0, -1.45, 0.0), (5.55, 1.0, 3.1))
    base.add_xyz(*radial_frustum(10, 0.80, 0.66, 0.18), (0.0, -1.12, 0.0), (4.4, 1.0, 2.35))
    add_box(base, (0.0, -0.79, 0.0), (3.2, 0.045, 1.5))

    # A recessed echo knot makes the main object feel like a portal into a second scale.
    echo = GardenMesh()
    echo.add_xyz(*tube_trefoil(2.05, 0.68, 0.105, 2, 72, 5), (0.0, 3.20, -2.25), (1.0, 1.0, 1.0), (0.0, 0.0, 0.16))

    # The main continuous form is the visual signature. 576 vertices stays below the service limit.
    knot = GardenMesh()
    knot.add_xyz(*tube_trefoil(), (0.0, 3.12, 0.35), (1.0, 1.0, 1.0), (0.0, 0.0, -0.12))

    core = GardenMesh()
    core.add_xyz(*faceted_orb(24, 7, 1.0), (0.0, 3.16, 1.50), (0.95, 0.95, 0.78), (0.0, 0.20, 0.0))
    add_diamond(core, (0.0, 3.16, 2.22), (0.25, 0.54, 0.18), (0.0, 0.0, math.pi / 4))

    # Six sparse supports touch the knot without tracing it. They create mechanical tension and
    # give the sculpture an axis, while remaining separate from the dense procedural mesh.
    supports = GardenMesh()
    for angle, radius, y in (
        (-1.15, 2.35, 1.15),
        (-0.56, 2.55, 1.04),
        (0.0, 2.42, 0.98),
        (0.55, 2.52, 1.07),
        (1.12, 2.38, 1.16),
        (math.pi, 2.18, 1.10),
    ):
        x = math.sin(angle) * radius
        z = 0.85 + 0.16 * math.cos(angle)
        add_strut(supports, (x * 0.52, y + 1.1, 1.35), (x, y, z), 0.055)

    # Contrasting facets mark six knot crossings. Their positions are derived from the curve rather
    # than hand-placed, so the detail remains tied to the continuous form.
    facets = GardenMesh()
    for index in (4, 18, 34, 49, 67, 83):
        t = math.tau * index / 96.0
        angle = 2 * t
        orbit = 3 * t
        radius = 2.2 + 0.78 * math.cos(orbit)
        location = (radius * math.cos(angle), 3.12 + radius * math.sin(angle), 0.35 + 0.78 * math.sin(orbit) * 0.68)
        add_diamond(facets, location, (0.18, 0.28, 0.14), (0.0, 0.0, angle))

    # Small calibration arcs and top/bottom ticks keep the negative space intentional.
    accents = GardenMesh()
    accents.add_xyz(*torus(2.95, 0.045, 28, 5), (0.0, 3.12, 0.88), (1.0, 0.50, 0.72), (math.pi * 0.5, 0.0, 0.0))
    add_box(accents, (-2.8, -0.42, 1.55), (0.48, 0.035, 0.035), (0.0, 0.0, 0.08))
    add_box(accents, (2.85, -0.38, 1.55), (0.48, 0.035, 0.035), (0.0, 0.0, -0.08))
    add_box(accents, (0.0, 6.05, 0.78), (0.72, 0.035, 0.035))

    return (
        (1501, (17, 24, 38, 255), backdrop),
        (1502, (44, 49, 56, 255), base),
        (1503, (52, 83, 103, 255), echo),
        (1504, (213, 196, 159, 255), knot),
        (1505, (32, 28, 36, 255), core),
        (1506, (174, 75, 52, 255), supports),
        (1507, (54, 178, 164, 255), facets),
        (1508, (222, 163, 74, 255), accents),
    )


def populate(client):
    client.stop()
    client.clear()
    client.camera((8.0, 5.2, 16.5), (0.0, 2.78, 0.20), 43.0)
    for mesh_id, color, mesh in build_scene():
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(60_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start(BACKGROUND)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=2.0)
    parser.add_argument("--output", type=Path, default=Path("bld/draw3d-captures/trefoil-engine-live.png"))
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
