#!/usr/bin/env python3
"""Paint an asymmetrical nested lattice reliquary on the TRUEOS draw3d screen."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh, radial_frustum
from draw3d_house_demo import CUBE_FACES, CUBE_VERTICES, Draw3dClient, OCTAHEDRON_FACES, OCTAHEDRON_VERTICES
from draw3d_null_meridian import faceted_orb


BACKGROUND = (6, 8, 15, 255)


def add_box(mesh, location, scale, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(CUBE_VERTICES, CUBE_FACES, location, scale, rotation)


def add_diamond(mesh, location, scale, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(OCTAHEDRON_VERTICES, OCTAHEDRON_FACES, location, scale, rotation)


def bar(mesh, location, length, thickness, axis, rotation=(0.0, 0.0, 0.0)):
    half = (length * 0.5, thickness, thickness)
    if axis == "y":
        half = (thickness, length * 0.5, thickness)
    elif axis == "z":
        half = (thickness, thickness, length * 0.5)
    add_box(mesh, location, half, rotation)


def wire_cube(mesh, center, dimensions, thickness, rotation=(0.0, 0.0, 0.0)):
    """Build a structural cube from bars, with separate endpoints at every corner."""
    sx, sy, sz = (dimension * 0.5 for dimension in dimensions)
    # The frames stay axis-aligned locally; the camera supplies the oblique perspective.
    for y in (-sy, sy):
        for z in (-sz, sz):
            bar(mesh, (center[0], center[1] + y, center[2] + z), dimensions[0], thickness, "x", rotation)
    for x in (-sx, sx):
        for z in (-sz, sz):
            bar(mesh, (center[0] + x, center[1], center[2] + z), dimensions[1], thickness, "y", rotation)
    for x in (-sx, sx):
        for y in (-sy, sy):
            bar(mesh, (center[0] + x, center[1] + y, center[2]), dimensions[2], thickness, "z", rotation)


def build_scene():
    # The backdrop is a single deep architectural plane, interrupted by a few vertical cuts.
    backdrop = GardenMesh()
    add_box(backdrop, (0.0, 2.0, -6.8), (7.7, 4.2, 0.16))
    for x, y, height, angle in (
        (-5.7, 2.3, 5.2, -0.08),
        (-4.5, 0.8, 3.2, 0.13),
        (4.8, 2.0, 4.7, 0.08),
        (6.0, 3.9, 6.6, -0.06),
    ):
        bar(backdrop, (x, y, -6.45), height, 0.18, "y", (0.0, 0.0, angle))

    base = GardenMesh()
    base.add_xyz(*radial_frustum(8, 1.0, 0.87, 0.42), (0.0, -1.44, 0.0), (5.4, 1.0, 3.1))
    base.add_xyz(*radial_frustum(8, 0.80, 0.65, 0.20), (0.0, -1.08, 0.0), (4.2, 1.0, 2.35))
    # A diagonal floor inlay gives the base a readable direction.
    bar(base, (0.0, -0.78, 0.80), 6.2, 0.045, "x", (0.0, 0.0, -0.08))
    bar(base, (0.0, -0.70, -0.60), 4.2, 0.035, "x", (0.0, 0.0, 0.18))

    # Three cages occupy different depth bands. Their material separation is intentional: on the
    # current renderer, a single multi-depth mesh would be visually unstable.
    outer = GardenMesh()
    wire_cube(outer, (0.0, 2.45, -0.15), (6.5, 6.0, 4.2), 0.055)
    # Remove the lower rear impression by adding two broken diagonal cuts at the shoulders.
    bar(outer, (-2.3, 4.55, 0.2), 2.7, 0.05, "y", (0.0, 0.0, -0.24))
    bar(outer, (2.4, 4.25, 0.1), 2.2, 0.05, "y", (0.0, 0.0, 0.28))

    middle = GardenMesh()
    wire_cube(middle, (-0.38, 2.58, 0.48), (4.55, 4.45, 3.1), 0.085)
    # A second, offset rectangle makes the middle cage feel folded rather than nested.
    bar(middle, (-0.38, 4.70, 1.05), 3.7, 0.07, "x", (0.0, 0.0, 0.0))
    bar(middle, (-2.24, 2.55, 0.48), 3.4, 0.07, "y", (0.0, 0.0, 0.0))

    inner = GardenMesh()
    wire_cube(inner, (0.42, 2.92, 0.92), (2.5, 3.6, 2.0), 0.10)
    for y in (1.45, 3.15, 4.80):
        bar(inner, (0.42, y, 1.95), 1.8, 0.07, "x", (0.0, 0.0, -0.05))

    # The reliquary core is a faceted, slightly off-axis object suspended inside the cages.
    core = GardenMesh()
    core.add_xyz(*faceted_orb(20, 6, 1.0), (0.35, 3.12, 1.45), (0.86, 1.12, 0.70), (0.0, 0.24, 0.0))
    add_diamond(core, (0.35, 3.12, 2.10), (0.25, 0.48, 0.18), (0.0, 0.0, math.pi / 4))

    # Material interventions: a rust-colored diagonal and a pair of teal counterweights.
    rust = GardenMesh()
    for location, length, angle in (
        ((-2.55, 1.15, 1.95), 3.0, -0.58),
        ((2.55, 3.95, 1.74), 2.7, 0.50),
        ((-2.10, 4.85, 1.72), 2.0, 0.38),
    ):
        bar(rust, location, length, 0.075, "x", (0.0, 0.0, angle))
    teal = GardenMesh()
    add_diamond(teal, (-2.35, 1.22, 1.55), (0.26, 0.75, 0.18), (0.0, 0.0, -0.24))
    add_diamond(teal, (2.55, 4.15, 1.38), (0.24, 0.62, 0.16), (0.0, 0.0, 0.34))
    add_diamond(teal, (1.95, 1.00, -0.55), (0.22, 0.52, 0.15), (0.0, 0.0, -0.22))

    # A few small gold registration marks establish a visual grammar without turning into stars.
    marks = GardenMesh()
    for x, y, z, width, angle in (
        (-3.8, -0.62, 1.65, 0.58, 0.08),
        (-2.9, -0.52, 1.72, 0.34, -0.18),
        (2.55, -0.58, 1.70, 0.48, 0.18),
        (3.55, -0.65, 1.62, 0.65, -0.10),
        (0.0, -0.50, 1.78, 1.25, 0.0),
    ):
        bar(marks, (x, y, z), width, 0.035, "x", (0.0, 0.0, angle))

    return (
        (1401, (18, 25, 40, 255), backdrop),
        (1402, (47, 52, 59, 255), base),
        (1403, (218, 204, 171, 255), outer),
        (1404, (187, 82, 57, 255), middle),
        (1405, (60, 135, 125, 255), inner),
        (1406, (39, 30, 36, 255), core),
        (1407, (181, 75, 56, 255), rust),
        (1408, (55, 186, 172, 255), teal),
        (1409, (220, 163, 75, 255), marks),
    )


def populate(client):
    client.stop()
    client.clear()
    client.camera((9.8, 5.8, 18.5), (0.0, 2.20, 0.20), 46.0)
    for mesh_id, color, mesh in build_scene():
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(50_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start(BACKGROUND)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=2.0)
    parser.add_argument("--output", type=Path, default=Path("bld/draw3d-captures/lattice-reliquary-live.png"))
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
