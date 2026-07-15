#!/usr/bin/env python3
"""Present a moonlit medieval castle from several draw3d camera perspectives."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import (
    GardenMesh,
    add_box,
    add_octahedron,
    crescent,
    radial_frustum,
    torus,
    triangular_prism,
)
from draw3d_house_demo import CUBE_FACES, CUBE_VERTICES, Draw3dClient


BACKGROUND = (5, 8, 24, 255)
IDENTITY_LOCATION = (0.0, 0.0, 0.0)
IDENTITY_SCALE = (1.0, 1.0, 1.0)


VIEWS = {
    "gate": ((0.0, 5.8, 20.0), (0.0, 3.0, 0.0), 43.0),
    "aerial": ((-12.5, 13.5, 14.5), (0.0, 2.2, 0.0), 50.0),
    "hero": ((12.0, 8.4, 18.0), (0.0, 3.0, 0.0), 47.0),
}


def add_cylinder(mesh, location, radius, height, segments=12):
    mesh.add_xyz(
        *radial_frustum(segments),
        location,
        (radius, height * 0.5, radius),
    )


def add_cone(mesh, location, radius, height, segments=12):
    mesh.add_xyz(
        *radial_frustum(segments, bottom_radius=1.0, top_radius=0.0, height=2.0),
        location,
        (radius, height * 0.5, radius),
    )


def add_disc(mesh, location, radius, thickness=0.08, segments=24):
    mesh.add_xyz(
        *radial_frustum(segments, 1.0, 1.0, 2.0),
        location,
        (radius, thickness * 0.5, radius),
    )


def add_crenellation_line(mesh, start, end, y, count, block=(0.23, 0.24, 0.23)):
    for index in range(count):
        progress = index / max(1, count - 1)
        x = start[0] + (end[0] - start[0]) * progress
        z = start[1] + (end[1] - start[1]) * progress
        add_box(mesh, (x, y, z), block)


def add_tower_crenellations(mesh, x, z, y, radius):
    for step in range(8):
        angle = math.tau * step / 8
        add_box(
            mesh,
            (x + math.cos(angle) * radius, y, z + math.sin(angle) * radius),
            (0.24, 0.25, 0.24),
            (0.0, -angle, 0.0),
        )


def add_window(mesh, location, rotation_y=0.0, scale=(0.17, 0.30, 0.055)):
    add_box(mesh, location, scale, (0.0, rotation_y, 0.0))


def add_flag(banners, poles, origin, direction=1.0):
    x, y, z = origin
    add_box(poles, (x, y + 0.7, z), (0.045, 0.72, 0.045))
    vertices = (
        (0.0, 0.55, 0.0),
        (direction * 0.85, 0.37, 0.0),
        (direction * 0.62, 0.05, 0.0),
        (0.0, 0.12, 0.0),
    )
    banners.add_xyz(vertices, ((0, 1, 2, 3),), (x, y + 0.7, z))


def add_spotlight_beam(mesh, source, target, top_width):
    sx, sy, sz = source
    tx, ty, tz = target
    vertices = (
        (sx - 0.11, sy, sz),
        (sx + 0.11, sy, sz),
        (tx + top_width, ty, tz),
        (tx - top_width, ty, tz),
    )
    mesh.add_xyz(vertices, ((0, 1, 2), (0, 2, 3)), IDENTITY_LOCATION)


def castle_meshes():
    moon = GardenMesh()
    moon.add_xyz(*crescent(24), (-5.1, 8.1, -8.0), (2.25, 2.25, 2.25))
    for x, y, size in ((-7.0, 6.4, 0.10), (-1.8, 9.0, 0.08), (2.0, 7.9, 0.10), (6.5, 8.8, 0.09)):
        add_octahedron(moon, (x, y, -7.6), (size, size * 1.8, size))

    mountains = GardenMesh()
    mountain = triangular_prism()
    for location, scale in (
        ((-6.7, -0.8, -6.6), (3.6, 3.4, 1.0)),
        ((-2.8, -0.9, -6.9), (2.7, 2.5, 1.0)),
        ((1.0, -0.8, -6.8), (3.8, 3.7, 1.0)),
        ((5.6, -0.8, -6.4), (3.4, 2.8, 1.0)),
    ):
        mountains.add_xyz(*mountain, location, scale)

    water = GardenMesh()
    add_disc(water, (0.0, -0.72, 0.0), 7.2, 0.20, 32)
    water.add_xyz(*torus(1.0, 0.045, 32, 6), (0.0, -0.56, 0.0), (6.3, 1.0, 5.7))

    island = GardenMesh()
    island.add_xyz(
        *radial_frustum(24, bottom_radius=0.82, top_radius=1.0, height=2.0),
        (0.0, -0.45, 0.0),
        (5.9, 0.45, 5.1),
    )
    for x, z, scale in (
        ((-4.3, 0.5, (0.45, 0.95, 0.48))),
        ((-2.1, 2.7, (0.36, 0.72, 0.40))),
        ((2.6, 2.4, (0.42, 0.88, 0.44))),
        ((4.4, -0.4, (0.34, 0.75, 0.38))),
    ):
        add_octahedron(island, (x, -1.25, z), scale)

    dark_stone = GardenMesh()
    # Rear towers and side curtain walls establish the castle's depth.
    for x, z in ((-3.35, -2.35), (3.35, -2.35)):
        add_cylinder(dark_stone, (x, 2.45, z), 1.05, 4.9, 12)
    add_box(dark_stone, (0.0, 2.0, -2.25), (3.35, 1.6, 0.42))
    add_box(dark_stone, (-3.15, 1.75, 0.0), (0.42, 1.45, 2.3))
    add_box(dark_stone, (3.15, 1.75, 0.0), (0.42, 1.45, 2.3))

    stone = GardenMesh()
    # Front towers, gate wall, central keep, and two stair-stepped keep shoulders.
    for x, z in ((-3.35, 2.35), (3.35, 2.35)):
        add_cylinder(stone, (x, 2.45, z), 1.05, 4.9, 12)
    add_box(stone, (0.0, 1.95, 2.25), (3.35, 1.55, 0.42))
    add_box(stone, (0.0, 4.05, -0.25), (2.05, 3.0, 1.55))
    add_box(stone, (-2.25, 3.15, -0.35), (0.55, 1.85, 1.15))
    add_box(stone, (2.25, 3.15, -0.35), (0.55, 1.85, 1.15))

    highlights = GardenMesh()
    # Warm planes are painted onto surfaces facing the theatrical lamps.
    add_box(highlights, (0.0, 3.9, 1.315), (1.82, 2.55, 0.035))
    add_box(highlights, (-3.35, 2.35, 3.31), (0.54, 1.70, 0.035))
    add_box(highlights, (3.35, 2.35, 3.31), (0.54, 1.70, 0.035))
    add_box(highlights, (-1.75, 2.15, 2.705), (0.72, 1.15, 0.035))
    add_box(highlights, (1.75, 2.15, 2.705), (0.72, 1.15, 0.035))

    battlements = GardenMesh()
    add_crenellation_line(battlements, (-2.8, 2.72), (2.8, 2.72), 3.68, 9)
    add_crenellation_line(battlements, (-2.8, -2.72), (2.8, -2.72), 3.68, 9)
    add_crenellation_line(battlements, (-3.58, -1.65), (-3.58, 1.65), 3.48, 6)
    add_crenellation_line(battlements, (3.58, -1.65), (3.58, 1.65), 3.48, 6)
    for x in (-1.72, -1.15, -0.58, 0.0, 0.58, 1.15, 1.72):
        add_box(battlements, (x, 7.24, 1.33), (0.22, 0.27, 0.24))
        add_box(battlements, (x, 7.24, -1.83), (0.22, 0.27, 0.24))
    for z in (-1.3, -0.78, -0.25, 0.28, 0.80):
        add_box(battlements, (-2.28, 7.24, z), (0.24, 0.27, 0.21))
        add_box(battlements, (2.28, 7.24, z), (0.24, 0.27, 0.21))
    for x, z in ((-3.35, -2.35), (3.35, -2.35), (-3.35, 2.35), (3.35, 2.35)):
        add_tower_crenellations(battlements, x, z, 5.05, 0.82)

    roofs = GardenMesh()
    for x, z in ((-3.35, -2.35), (3.35, -2.35), (-3.35, 2.35), (3.35, 2.35)):
        add_cone(roofs, (x, 5.85, z), 1.30, 1.9, 12)
        add_box(roofs, (x, 6.86, z), (0.055, 0.55, 0.055))

    openings = GardenMesh()
    # Rounded gate opening: a rectangle plus a shallow circular cap.
    add_box(openings, (0.0, 1.20, 2.705), (0.72, 1.20, 0.045))
    openings.add_xyz(
        *radial_frustum(16, 1.0, 1.0, 0.10),
        (0.0, 2.35, 2.73),
        (0.72, 0.72, 0.72),
        (math.pi / 2, 0.0, 0.0),
    )
    for x in (-1.20, 0.0, 1.20):
        add_window(openings, (x, 4.75, 1.335))
        add_window(openings, (x, 6.05, 1.335), scale=(0.14, 0.24, 0.055))
    for x in (-3.35, 3.35):
        add_window(openings, (x, 2.25, 3.37))
        add_window(openings, (x, 3.35, 3.26), scale=(0.14, 0.24, 0.055))

    windows = GardenMesh()
    for x in (-1.20, 0.0, 1.20):
        add_window(windows, (x, 4.75, 1.38), scale=(0.12, 0.23, 0.040))
    for x in (-3.35, 3.35):
        add_window(windows, (x, 2.25, 3.42), scale=(0.12, 0.23, 0.040))
    for x in (-0.75, 0.75):
        add_window(windows, (x, 6.05, 1.38), scale=(0.10, 0.18, 0.040))

    wood = GardenMesh()
    add_box(wood, (0.0, 0.18, 4.85), (0.82, 0.10, 2.15), (0.08, 0.0, 0.0))
    for z in (3.25, 3.85, 4.45, 5.05, 5.65, 6.25):
        add_box(wood, (0.0, 0.38 - (z - 3.25) * 0.075, z), (0.92, 0.045, 0.035))
    add_box(wood, (-0.86, 1.10, 3.00), (0.055, 1.05, 0.055), (0.0, 0.0, -0.40))
    add_box(wood, (0.86, 1.10, 3.00), (0.055, 1.05, 0.055), (0.0, 0.0, 0.40))

    banners = GardenMesh()
    poles = GardenMesh()
    add_flag(banners, poles, (-1.55, 7.18, 0.80), 1.0)
    add_flag(banners, poles, (1.55, 7.18, 0.80), -1.0)
    add_flag(banners, poles, (-3.35, 6.86, 2.35), 1.0)
    add_flag(banners, poles, (3.35, 6.86, 2.35), -1.0)

    lamps = GardenMesh()
    for x in (-3.9, 3.9):
        add_box(lamps, (x, 0.16, 5.05), (0.34, 0.16, 0.42), (0.0, 0.0, -0.08 if x < 0 else 0.08))
        add_octahedron(lamps, (x, 0.44, 4.88), (0.25, 0.22, 0.25))
    beams = GardenMesh()
    add_spotlight_beam(beams, (-3.9, 0.45, 4.88), (-0.9, 6.7, 0.85), 0.72)
    add_spotlight_beam(beams, (3.9, 0.45, 4.88), (0.9, 6.7, 0.85), 0.72)

    return (
        (1001, (246, 211, 121, 255), moon),
        (1002, (26, 26, 67, 255), mountains),
        (1003, (22, 67, 91, 255), water),
        (1004, (39, 58, 70, 255), island),
        (1005, (66, 70, 86, 255), dark_stone),
        (1006, (116, 119, 125, 255), stone),
        (1007, (170, 162, 139, 255), highlights),
        (1008, (139, 140, 142, 255), battlements),
        (1009, (101, 32, 43, 255), roofs),
        (1010, (20, 23, 35, 255), openings),
        (1011, (255, 184, 74, 255), windows),
        (1012, (93, 55, 35, 255), wood),
        (1013, (164, 35, 55, 255), banners),
        (1014, (191, 151, 70, 255), poles),
        (1015, (248, 198, 88, 255), lamps),
        (1016, (255, 221, 154, 48), beams),
    )


def populate(client):
    client.stop()
    client.clear()
    position, target, fov = VIEWS["hero"]
    client.camera(position, target, fov)
    for mesh_id, color, mesh in castle_meshes():
        triangles = sum(len(face) - 2 for face in mesh.faces)
        if len(mesh.vertices) > 1_000 or triangles > 2_000:
            raise RuntimeError(
                f"mesh {mesh_id} exceeds budget: vertices={len(mesh.vertices)} triangles={triangles}"
            )
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(30_000 + mesh_id, mesh_id, IDENTITY_LOCATION, IDENTITY_SCALE)
    client.start(BACKGROUND)


def capture_view(client, name, output_dir, settle):
    position, target, fov = VIEWS[name]
    client.camera(position, target, fov)
    time.sleep(settle)
    path, image_format, width, height, image = client.render(output_dir / f"moonlit-castle-{name}.png")
    if image_format != 2 or width <= 0 or height <= 0:
        raise RuntimeError(f"{name} view did not return a non-empty PNG target")
    digest = hashlib.sha256(image).hexdigest()
    print(f"view={name} size={width}x{height} bytes={len(image)} sha256={digest} path={path}")
    return path, digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=4.0)
    parser.add_argument("--output-dir", type=Path, default=Path("bld/draw3d-captures"))
    args = parser.parse_args()

    client = Draw3dClient(args.host)
    try:
        populate(client)
        for view_name in ("gate", "hero", "aerial"):
            capture_view(client, view_name, args.output_dir, args.settle)
        stats = client.stats()
        print(
            f"scene meshes={stats[0]} instances={stats[1]} vertices={stats[2]} "
            f"edges={stats[3]} faces={stats[4]} mesh_bytes={stats[5]} final_view=aerial"
        )
    finally:
        client.close()


if __name__ == "__main__":
    main()
