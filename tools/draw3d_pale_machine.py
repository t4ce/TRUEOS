#!/usr/bin/env python3
"""Paint a pale/clay/teal sculptural artifact on the TRUEOS draw3d screen."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh, radial_frustum
from draw3d_house_demo import (
    CUBE_FACES,
    CUBE_VERTICES,
    OCTAHEDRON_FACES,
    OCTAHEDRON_VERTICES,
    Draw3dClient,
)


BACKGROUND = (7, 10, 18, 255)


def add_xyz(mesh, vertices, faces, location, scale=(1.0, 1.0, 1.0), rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(vertices, faces, location, scale, rotation)


def add_box(mesh, location, scale, rotation=(0.0, 0.0, 0.0)):
    add_xyz(mesh, CUBE_VERTICES, CUBE_FACES, location, scale, rotation)


def add_diamond(mesh, location, scale, rotation=(0.0, 0.0, 0.0)):
    add_xyz(mesh, OCTAHEDRON_VERTICES, OCTAHEDRON_FACES, location, scale, rotation)


def blade(width=1.0, height=3.0, depth=0.45, shoulder=0.25):
    """A tapered, slightly shouldered blade for the central fan."""
    w, d = width * 0.5, depth * 0.5
    vertices = (
        (-w, -height * 0.5, -d),
        (w, -height * 0.5, -d),
        (w * shoulder, height * 0.5, -d),
        (-w * shoulder, height * 0.5, -d),
        (-w * 0.78, -height * 0.14, d),
        (w * 0.78, -height * 0.14, d),
        (w * shoulder * 0.75, height * 0.5, d),
        (-w * shoulder * 0.75, height * 0.5, d),
    )
    faces = (
        (0, 1, 2, 3),
        (4, 7, 6, 5),
        (0, 4, 5, 1),
        (3, 2, 6, 7),
        (0, 3, 7, 4),
        (1, 5, 6, 2),
    )
    return vertices, faces


def broken_arc(major_radius, tube_radius, start, end, major_steps=24, minor_steps=6):
    """Tube arc in the XY plane, with real gaps so it reads as an artifact, not a halo."""
    vertices = []
    for major in range(major_steps + 1):
        u = start + (end - start) * major / major_steps
        for minor in range(minor_steps):
            v = math.tau * minor / minor_steps
            radius = major_radius + tube_radius * math.cos(v)
            vertices.append((radius * math.cos(u), radius * math.sin(u), tube_radius * math.sin(v)))
    faces = []
    for major in range(major_steps):
        for minor in range(minor_steps):
            nxt_minor = (minor + 1) % minor_steps
            row = major * minor_steps
            next_row = (major + 1) * minor_steps
            faces.append((row + minor, next_row + minor, next_row + nxt_minor, row + nxt_minor))
    return tuple(vertices), tuple(faces)


def build_scene():
    # Distant architectural ribs provide a quiet scale reference for the sculpture.
    backdrop = GardenMesh()
    for x, height, lean in (
        (-6.0, 6.8, -0.10),
        (-4.8, 4.3, 0.10),
        (4.7, 5.2, -0.08),
        (6.2, 7.4, 0.08),
    ):
        add_box(backdrop, (x, height * 0.5 - 0.85, -6.6), (0.30, height, 0.24), (0.0, 0.0, lean))
    add_box(backdrop, (0.0, -0.50, -6.3), (7.8, 0.08, 0.16))
    # Broken rings are offset from the core, like a museum object mounted inside a diagram.
    add_xyz(backdrop, *broken_arc(3.85, 0.075, math.radians(-38), math.radians(132)), (0.0, 3.15, -5.8), (1.0, 1.0, 1.0), (0.0, 0.0, 0.0))
    add_xyz(backdrop, *broken_arc(3.25, 0.050, math.radians(150), math.radians(304)), (0.0, 3.15, -5.55), (1.0, 1.0, 1.0), (0.0, 0.0, 0.0))

    frame = GardenMesh()
    frame.add_xyz(*broken_arc(4.15, 0.085, math.radians(-12), math.radians(118)), (0.0, 3.05, -5.15), (1.0, 1.0, 1.0), (0.0, 0.0, 0.0))

    base = GardenMesh()
    base.add_xyz(*radial_frustum(10, 1.0, 0.92, 0.42), (0.0, -1.48, 0.0), (5.5, 1.0, 3.05))
    base.add_xyz(*radial_frustum(10, 0.82, 0.90, 0.22), (0.0, -1.12, 0.0), (4.65, 1.0, 2.55))
    base.add_xyz(*radial_frustum(8, 0.72, 0.58, 0.18), (0.0, -0.91, 0.0), (3.55, 1.0, 1.95))
    for x in (-3.9, 3.9):
        add_box(base, (x, -0.72, -0.25), (0.34, 0.34, 1.65), (0.0, 0.0, 0.12 if x < 0 else -0.12))

    # A blunt, stepped spine anchors the fan. It is intentionally heavier and less symmetrical
    # than a tower: the “artifact” should look assembled, not procedurally mirrored.
    spine = GardenMesh()
    spine.add_xyz(*radial_frustum(7, 1.0, 0.72, 4.9), (0.0, 1.75, -0.15), (0.96, 1.0, 0.75), (0.0, 0.14, 0.0))
    spine.add_xyz(*radial_frustum(7, 0.72, 0.40, 1.9), (0.0, 5.06, -0.12), (0.95, 1.0, 0.72), (0.0, -0.09, 0.0))
    add_box(spine, (0.0, 1.20, 1.03), (0.72, 1.65, 0.05))
    add_box(spine, (0.0, 4.58, 0.48), (0.24, 0.52, 0.04))

    # Bone-colored fan: six blades all meet the same invisible hinge, but their offsets and lean
    # are deliberately irregular. This is the scene's main authored silhouette.
    bone = GardenMesh()
    blade_mesh = blade(0.92, 3.45, 0.40, 0.22)
    for angle, radius, z, scale in (
        (math.radians(-72), 1.42, 0.55, (1.00, 1.00, 1.0)),
        (math.radians(-43), 1.44, 0.46, (0.92, 1.04, 1.0)),
        (math.radians(-14), 1.40, 0.38, (1.04, 1.08, 1.0)),
        (math.radians(15), 1.42, 0.43, (0.88, 1.02, 1.0)),
        (math.radians(44), 1.47, 0.50, (0.94, 1.00, 1.0)),
        (math.radians(73), 1.38, 0.54, (0.82, 0.96, 1.0)),
    ):
        x = math.sin(angle) * radius
        y = 3.05 + math.cos(angle) * radius
        add_xyz(bone, *blade_mesh, (x, y, z), scale, (0.0, 0.0, angle))

    # Clay blades sit between the pale vanes, breaking the perfect fan with a second material rhythm.
    clay = GardenMesh()
    clay_mesh = blade(0.56, 2.55, 0.32, 0.18)
    for angle, radius, z, scale in (
        (math.radians(-58), 1.25, 0.70, (1.0, 1.08, 1.0)),
        (math.radians(-27), 1.30, 0.62, (0.88, 1.0, 1.0)),
        (math.radians(9), 1.25, 0.66, (0.92, 1.06, 1.0)),
        (math.radians(43), 1.28, 0.72, (0.80, 0.98, 1.0)),
    ):
        x = math.sin(angle) * radius
        y = 3.10 + math.cos(angle) * radius
        add_xyz(clay, *clay_mesh, (x, y, z), scale, (0.0, 0.0, angle))

    # Teal inner seed and small suspended counterweight.
    teal = GardenMesh()
    add_diamond(teal, (0.0, 3.22, 1.66), (0.66, 0.88, 0.38), (0.0, 0.0, math.pi / 4))
    add_diamond(teal, (0.0, 3.22, 1.92), (0.22, 0.52, 0.12), (0.0, 0.0, math.pi / 4))
    add_box(teal, (0.0, 5.40, 0.16), (0.045, 0.62, 0.045))
    add_diamond(teal, (0.0, 5.83, 0.16), (0.30, 0.54, 0.16), (0.0, 0.0, math.pi / 4))

    # Oxide struts and mustard measurement tiles keep the lower third active without adding noise.
    oxide = GardenMesh()
    for x, y, z, length, angle in (
        (-3.2, 0.65, 0.92, 1.65, -0.58),
        (3.1, 0.72, 0.82, 1.55, 0.50),
        (-2.55, 1.90, 0.56, 1.20, 0.35),
        (2.55, 2.05, 0.62, 1.25, -0.32),
    ):
        add_box(oxide, (x, y, z), (length, 0.07, 0.07), (0.0, 0.0, angle))
    mustard = GardenMesh()
    for x, y, z, width, angle in (
        (-3.4, 0.02, 1.55, 0.72, 0.12),
        (-2.35, 0.20, 1.42, 0.44, -0.28),
        (2.45, 0.12, 1.43, 0.52, 0.28),
        (3.40, 0.00, 1.55, 0.68, -0.12),
        (0.0, 0.30, 1.58, 1.90, 0.05),
    ):
        add_box(mustard, (x, y, z), (width, 0.035, 0.035), (0.0, 0.0, angle))

    # Small dark/ivory markers make the object feel catalogued rather than decorative.
    markers = GardenMesh()
    for x, y, z, sx, sy in (
        (-5.2, 5.2, -5.6, 0.10, 0.48),
        (-4.35, 6.1, -5.7, 0.08, 0.30),
        (4.9, 5.6, -5.7, 0.10, 0.44),
        (5.65, 4.0, -5.8, 0.07, 0.28),
        (-5.65, 1.65, -5.4, 0.08, 0.36),
        (5.3, 1.0, -5.3, 0.09, 0.34),
    ):
        add_box(markers, (x, y, z), (sx, sy, 0.025))

    return (
        (1301, (26, 35, 49, 255), backdrop),
        (1302, (48, 53, 59, 255), base),
        (1303, (18, 22, 28, 255), spine),
        (1304, (207, 196, 165, 255), bone),
        (1305, (173, 72, 47, 255), clay),
        (1306, (57, 166, 153, 255), teal),
        (1307, (177, 74, 50, 255), oxide),
        (1308, (224, 165, 76, 255), mustard),
        (1309, (129, 158, 139, 255), frame),
        (1310, (220, 215, 193, 255), markers),
    )


def populate(client):
    client.stop()
    client.clear()
    client.camera((5.5, 4.2, 19.0), (0.0, 2.75, 0.15), 43.5)
    for mesh_id, color, mesh in build_scene():
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(40_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start(BACKGROUND)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=2.0)
    parser.add_argument("--output", type=Path, default=Path("bld/draw3d-captures/pale-machine-live.png"))
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
