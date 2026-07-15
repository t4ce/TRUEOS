#!/usr/bin/env python3
"""Build and present a Counter-Strike-inspired tactical desert map.

The level is intentionally made from protocol-native meshes rather than an
asset import.  Large architectural groups are batched by visual role so the
kernel receives a detailed scene without turning every prop into a draw job.
"""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh, add_box, radial_frustum, triangular_prism
from draw3d_house_demo import Draw3dClient


BACKGROUND = (168, 198, 211, 255)
IDENTITY_LOCATION = (0.0, 0.0, 0.0)
IDENTITY_SCALE = (1.0, 1.0, 1.0)

# Hand-tuned viewpoints are useful both as screenshot checks and as design
# references for the final slow showcase orbit.
VIEWS = {
    "overview": ((29.5, 10.5, 25.0), (0.0, 1.6, 0.0), 54.0),
    "long-a": ((-16.0, 4.2, 13.2), (10.0, 1.5, 8.5), 58.0),
    "mid": ((-2.0, 4.7, 14.5), (1.5, 1.8, -3.0), 55.0),
    "site-b": ((-18.0, 5.1, -2.0), (-10.5, 1.4, -8.0), 57.0),
}


def add_cylinder(mesh, location, radius, height, segments=10):
    mesh.add_xyz(
        *radial_frustum(segments),
        location,
        (radius, height * 0.5, radius),
    )


def add_cone(mesh, location, radius, height, segments=8):
    mesh.add_xyz(
        *radial_frustum(segments, bottom_radius=1.0, top_radius=0.0, height=2.0),
        location,
        (radius, height * 0.5, radius),
    )


def add_stairs(mesh, start, step_size, count, axis="x"):
    """Add solid ascending steps; dimensions are half-extents."""
    x, y, z = start
    sx, sy, sz = step_size
    for index in range(count):
        rise = sy * (index + 1)
        if axis == "x":
            location = (x + index * sx * 2.0, y + rise, z)
            scale = (sx, rise, sz)
        else:
            location = (x, y + rise, z + index * sz * 2.0)
            scale = (sx, rise, sz)
        add_box(mesh, location, scale)


def add_crate(wood, bands, location, scale=(0.75, 0.75, 0.75)):
    x, y, z = location
    sx, sy, sz = scale
    add_box(wood, location, scale)
    band = 0.045
    # Contrasting straps make the cover readable from every orbit angle.
    add_box(bands, (x - sx * 0.58, y, z), (band, sy * 1.02, sz * 1.02))
    add_box(bands, (x + sx * 0.58, y, z), (band, sy * 1.02, sz * 1.02))
    add_box(bands, (x, y, z - sz * 0.58), (sx * 1.02, sy * 1.02, band))
    add_box(bands, (x, y, z + sz * 0.58), (sx * 1.02, sy * 1.02, band))


def add_door_panel(mesh, location, width=0.75, height=1.15, rotation_y=0.0):
    add_box(mesh, location, (width, height, 0.055), (0.0, rotation_y, 0.0))


def add_window_row(mesh, *, x0, y, z, count, spacing, rotation_y=0.0):
    for index in range(count):
        add_box(
            mesh,
            (x0 + index * spacing, y, z),
            (0.34, 0.42, 0.045),
            (0.0, rotation_y, 0.0),
        )


def add_letter_a(mesh, center, rotation_y=0.0):
    """Block-letter A, modeled as shallow geometry for an objective sign."""
    x, y, z = center
    for dx, rotation_z in ((-0.34, -0.31), (0.34, 0.31)):
        add_box(mesh, (x + dx, y, z), (0.11, 0.78, 0.055), (0.0, rotation_y, rotation_z))
    add_box(mesh, (x, y - 0.02, z), (0.42, 0.09, 0.060), (0.0, rotation_y, 0.0))


def add_letter_b(mesh, center, rotation_y=0.0):
    x, y, z = center
    add_box(mesh, (x - 0.30, y, z), (0.11, 0.78, 0.055), (0.0, rotation_y, 0.0))
    for dy in (-0.61, 0.0, 0.61):
        add_box(mesh, (x + 0.08, y + dy, z), (0.42, 0.10, 0.060), (0.0, rotation_y, 0.0))
    add_box(mesh, (x + 0.47, y + 0.31, z), (0.10, 0.30, 0.055), (0.0, rotation_y, 0.0))
    add_box(mesh, (x + 0.47, y - 0.31, z), (0.10, 0.30, 0.055), (0.0, rotation_y, 0.0))


def tactical_map_meshes():
    ground = GardenMesh()
    add_box(ground, (0.0, -0.42, 0.0), (20.0, 0.42, 15.0))
    # Raised site foundations and spawn aprons give the arena a readable relief.
    add_box(ground, (12.0, 0.12, 7.2), (5.0, 0.12, 4.4))
    add_box(ground, (-11.6, 0.12, -7.8), (5.2, 0.12, 4.4))
    add_box(ground, (-14.0, 0.08, 9.5), (4.2, 0.08, 3.4))
    add_box(ground, (14.3, 0.08, -10.0), (4.0, 0.08, 3.0))

    lanes = GardenMesh()
    # Mid, long A, B tunnels, and the two spawn connectors.
    add_box(lanes, (0.0, 0.035, -0.7), (2.75, 0.035, 11.4))
    add_box(lanes, (-1.5, 0.040, 10.2), (15.8, 0.040, 1.65))
    add_box(lanes, (-11.8, 0.045, -10.0), (5.4, 0.045, 1.45))
    add_box(lanes, (-13.9, 0.045, 2.5), (1.45, 0.045, 6.2))
    add_box(lanes, (13.8, 0.045, -2.0), (1.45, 0.045, 6.0))
    add_box(lanes, (7.8, 0.045, 5.2), (5.0, 0.045, 1.35), (0.0, -0.22, 0.0))

    perimeter = GardenMesh()
    # The enclosing silhouette makes the scene read as a playable arena.
    add_box(perimeter, (0.0, 2.0, -14.55), (20.0, 2.0, 0.45))
    add_box(perimeter, (0.0, 2.0, 14.55), (20.0, 2.0, 0.45))
    add_box(perimeter, (-19.55, 2.0, 0.0), (0.45, 2.0, 14.2))
    add_box(perimeter, (19.55, 2.0, 0.0), (0.45, 2.0, 14.2))
    # Irregular skyline blocks hide the simple outer rectangle.
    for location, scale in (
        ((-16.4, 3.4, -13.5), (2.0, 3.4, 1.0)),
        ((-7.0, 2.7, -13.7), (2.8, 2.7, 0.8)),
        ((7.2, 3.1, -13.7), (3.0, 3.1, 0.8)),
        ((16.2, 2.6, -13.5), (2.1, 2.6, 1.0)),
        ((-16.5, 2.8, 13.5), (2.2, 2.8, 1.0)),
        ((5.5, 2.4, 13.6), (3.3, 2.4, 0.9)),
        ((16.0, 3.2, 13.4), (2.2, 3.2, 1.1)),
    ):
        add_box(perimeter, location, scale)

    architecture = GardenMesh()
    # Terrorist spawn compound, CT headquarters, mid block, and site buildings.
    for location, scale in (
        ((-14.2, 2.0, 8.2), (4.0, 2.0, 2.1)),
        ((14.5, 2.1, -9.1), (3.8, 2.1, 2.5)),
        ((-10.7, 2.25, -8.2), (3.4, 2.25, 2.7)),
        ((11.8, 2.3, 7.5), (3.4, 2.3, 2.4)),
        ((5.2, 2.0, -4.6), (2.3, 2.0, 3.2)),
        ((-4.8, 1.8, 3.8), (2.4, 1.8, 2.7)),
        ((1.0, 2.5, -8.4), (2.1, 2.5, 2.1)),
    ):
        add_box(architecture, location, scale)
    # Mid control bridge and catwalk supports.
    add_box(architecture, (0.2, 3.05, -3.1), (4.2, 0.28, 1.0))
    add_box(architecture, (-3.45, 1.5, -3.1), (0.35, 1.5, 1.0))
    add_box(architecture, (3.85, 1.5, -3.1), (0.35, 1.5, 1.0))
    add_box(architecture, (7.6, 1.25, 2.5), (4.2, 0.22, 0.75), (0.0, -0.22, 0.0))

    lane_walls = GardenMesh()
    # Chicanes prevent the three main routes from becoming featureless tubes.
    for location, scale in (
        ((-8.0, 1.25, 6.6), (0.32, 1.25, 3.1)),
        ((-2.0, 1.20, 7.2), (0.32, 1.20, 2.4)),
        ((5.0, 1.15, 8.5), (0.32, 1.15, 2.0)),
        ((9.0, 1.20, -10.2), (0.32, 1.20, 3.0)),
        ((-5.7, 1.25, -9.5), (0.32, 1.25, 2.7)),
        ((-8.1, 1.05, -2.0), (3.1, 1.05, 0.30)),
        ((8.4, 1.05, 1.3), (3.1, 1.05, 0.30)),
        ((0.2, 1.15, 4.0), (2.2, 1.15, 0.30)),
        ((-0.8, 1.10, -6.1), (2.1, 1.10, 0.30)),
    ):
        add_box(lane_walls, location, scale)
    # Parapets expose elevated routes without making them visually solid.
    add_box(lane_walls, (0.2, 3.62, -2.25), (4.2, 0.30, 0.15))
    add_box(lane_walls, (0.2, 3.62, -3.95), (4.2, 0.30, 0.15))
    add_box(lane_walls, (7.6, 1.75, 3.25), (4.2, 0.28, 0.14), (0.0, -0.22, 0.0))

    trim = GardenMesh()
    # Roof caps, curbs, and stairs create strong highlights and route cues.
    for location, scale in (
        ((-14.2, 4.18, 8.2), (4.25, 0.18, 2.35)),
        ((14.5, 4.38, -9.1), (4.05, 0.18, 2.75)),
        ((-10.7, 4.68, -8.2), (3.65, 0.18, 2.95)),
        ((11.8, 4.78, 7.5), (3.65, 0.18, 2.65)),
        ((5.2, 4.18, -4.6), (2.55, 0.18, 3.45)),
        ((-4.8, 3.78, 3.8), (2.65, 0.18, 2.95)),
        ((1.0, 5.18, -8.4), (2.35, 0.18, 2.35)),
    ):
        add_box(trim, location, scale)
    add_stairs(trim, (-4.0, 0.0, -1.8), (0.42, 0.16, 1.0), 7, "x")
    add_stairs(trim, (5.0, 0.0, 4.0), (0.42, 0.14, 0.80), 6, "x")
    add_stairs(trim, (-15.4, 0.0, -3.4), (0.75, 0.13, 0.42), 7, "z")

    shadow = GardenMesh()
    # Door and window planes imply navigable interiors and deepen the facade.
    for location, width, height, rot_y in (
        ((-14.2, 1.2, 6.05), 0.85, 1.2, 0.0),
        ((14.5, 1.2, -6.55), 0.9, 1.2, 0.0),
        ((-10.7, 1.2, -5.45), 0.9, 1.2, 0.0),
        ((11.8, 1.2, 5.05), 0.9, 1.2, 0.0),
        ((2.9, 1.2, -4.6), 0.8, 1.2, math.pi / 2),
        ((-2.35, 1.2, 3.8), 0.8, 1.2, math.pi / 2),
        ((1.0, 1.2, -6.25), 0.75, 1.2, 0.0),
    ):
        add_door_panel(shadow, location, width, height, rot_y)
    add_window_row(shadow, x0=-16.3, y=2.75, z=6.02, count=4, spacing=1.4)
    add_window_row(shadow, x0=12.2, y=2.9, z=-6.52, count=4, spacing=1.35)
    add_window_row(shadow, x0=-12.7, y=3.0, z=-5.42, count=4, spacing=1.3)
    add_window_row(shadow, x0=9.7, y=3.0, z=5.02, count=4, spacing=1.35)

    wood = GardenMesh()
    crate_bands = GardenMesh()
    for position, scale in (
        ((9.6, 0.78, 8.8), (0.75, 0.75, 0.75)),
        ((11.1, 0.78, 8.8), (0.75, 0.75, 0.75)),
        ((10.35, 2.25, 8.8), (0.75, 0.75, 0.75)),
        ((14.0, 0.62, 5.2), (0.60, 0.60, 0.60)),
        ((-13.5, 0.78, -9.2), (0.75, 0.75, 0.75)),
        ((-12.0, 0.78, -9.2), (0.75, 0.75, 0.75)),
        ((-12.75, 2.25, -9.2), (0.75, 0.75, 0.75)),
        ((-8.0, 0.62, -6.0), (0.60, 0.60, 0.60)),
        ((1.2, 0.62, 2.5), (0.60, 0.60, 0.60)),
        ((-1.0, 0.48, -5.0), (0.46, 0.46, 0.46)),
        ((-11.0, 0.62, 10.3), (0.60, 0.60, 0.60)),
    ):
        add_crate(wood, crate_bands, position, scale)
    # Long-A awning and B-tunnel braces.
    add_box(wood, (5.8, 3.2, 11.8), (3.0, 0.12, 1.0), (0.0, 0.0, -0.07))
    for x in (3.0, 5.8, 8.6):
        add_box(wood, (x, 1.6, 11.8), (0.10, 1.6, 0.10))
    for z in (-11.2, -9.5, -7.8):
        add_box(wood, (-16.0, 1.55, z), (1.1, 0.10, 0.10))

    metal = GardenMesh()
    blue_metal = GardenMesh()
    # Containers are major cover landmarks; barrels form smaller decision points.
    add_box(blue_metal, (6.6, 1.25, -10.2), (2.7, 1.25, 1.15))
    add_box(blue_metal, (-15.8, 1.05, 1.8), (1.15, 1.05, 2.5))
    for x in (4.4, 5.3, 6.2, 7.1, 8.0, 8.8):
        add_box(metal, (x, 1.25, -9.02), (0.045, 1.05, 0.06))
    for z in (-0.2, 0.7, 1.6, 2.5, 3.4):
        add_box(metal, (-14.62, 1.05, z), (0.06, 0.90, 0.045))
    for position in (
        (13.6, 0.58, 9.0),
        (14.6, 0.58, 9.0),
        (-9.5, 0.58, -10.8),
        (-8.5, 0.58, -10.8),
        (3.3, 0.58, 1.1),
        (-4.0, 0.58, -5.8),
    ):
        add_cylinder(metal, position, 0.42, 1.15, 10)

    site_a = GardenMesh()
    site_b = GardenMesh()
    site_a.add_xyz(*radial_frustum(24), (12.0, 0.28, 7.2), (2.15, 0.045, 2.15))
    site_b.add_xyz(*radial_frustum(24), (-11.6, 0.28, -7.8), (2.15, 0.045, 2.15))
    add_letter_a(site_a, (14.8, 2.2, 5.02))
    add_letter_b(site_b, (-8.8, 2.2, -5.42))

    foliage = GardenMesh()
    trunks = GardenMesh()
    for x, z, height in (
        (-17.5, 11.5, 3.0),
        (17.3, 11.7, 3.4),
        (-17.2, -11.8, 3.2),
        (17.1, -12.0, 2.8),
        (8.6, 12.9, 2.7),
    ):
        add_cylinder(trunks, (x, height * 0.5, z), 0.16, height, 8)
        for angle in (0.0, math.pi * 0.5, math.pi, math.pi * 1.5):
            leaf_x = x + math.cos(angle) * 0.65
            leaf_z = z + math.sin(angle) * 0.65
            add_cone(
                foliage,
                (leaf_x, height + 0.15, leaf_z),
                0.75,
                1.5,
                6,
            )

    skyline = GardenMesh()
    # Water tower, radio mast, and distant roof shapes orient players at a glance.
    add_cylinder(skyline, (-16.2, 6.4, -11.8), 1.15, 1.65, 12)
    for x in (-16.8, -15.6):
        add_box(skyline, (x, 4.1, -11.8), (0.10, 1.7, 0.10), (0.0, 0.0, -0.06 if x < -16.2 else 0.06))
    add_box(skyline, (16.1, 6.2, 12.0), (0.11, 2.7, 0.11))
    add_box(skyline, (16.1, 8.8, 12.0), (1.1, 0.08, 0.08), (0.0, 0.0, 0.20))
    skyline.add_xyz(*triangular_prism(), (7.2, 5.7, -13.2), (3.0, 1.5, 0.8))
    skyline.add_xyz(*triangular_prism(), (-7.0, 5.2, 13.1), (2.7, 1.3, 0.8))

    return (
        (2001, (194, 159, 105, 255), ground),
        (2002, (91, 87, 76, 255), lanes),
        (2003, (137, 108, 73, 255), perimeter),
        (2004, (202, 181, 142, 255), architecture),
        (2005, (162, 132, 91, 255), lane_walls),
        (2006, (226, 207, 166, 255), trim),
        (2007, (36, 39, 40, 255), shadow),
        (2008, (117, 73, 38, 255), wood),
        (2009, (68, 55, 43, 255), crate_bands),
        (2010, (74, 83, 84, 255), metal),
        (2011, (46, 79, 105, 255), blue_metal),
        (2012, (178, 49, 40, 255), site_a),
        (2013, (221, 139, 46, 255), site_b),
        (2014, (57, 102, 57, 255), foliage),
        (2015, (105, 72, 42, 255), trunks),
        (2016, (93, 91, 82, 255), skyline),
    )


def validate_meshes(meshes):
    for mesh_id, _color, mesh in meshes:
        triangles = sum(len(face) - 2 for face in mesh.faces)
        if len(mesh.vertices) > 1_000 or triangles > 2_000:
            raise RuntimeError(
                f"mesh {mesh_id} exceeds service budget: "
                f"vertices={len(mesh.vertices)} triangles={triangles}"
            )


def set_static_view(client, name):
    position, target, fov = VIEWS[name]
    client.camera(position, target, fov)


def set_showcase_orbit(client, speed):
    # Phase zero starts near the proven south-east overview. The plane tilt
    # lifts the opening view above the skyline; the deliberately slow speed
    # keeps it a calm map showcase rather than a spinning model.
    look_at = (0.0, 3.0, 0.0)
    client.camera(
        (29.5, 9.0, 25.0),
        look_at,
        56.0,
        orbit_scale=(38.0, 30.0),
        orbit_rotation=(-0.040, -0.70, 0.20),
        orbit_speed=speed,
    )


def populate(client, speed=0.012):
    meshes = tactical_map_meshes()
    validate_meshes(meshes)
    client.stop()
    client.clear()
    set_static_view(client, "overview")
    for mesh_id, color, mesh in meshes:
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(50_000 + mesh_id, mesh_id, IDENTITY_LOCATION, IDENTITY_SCALE)
    client.start(BACKGROUND)
    set_showcase_orbit(client, speed)
    return meshes


def capture(client, output, settle):
    time.sleep(settle)
    path, image_format, width, height, image = client.render(output)
    if image_format != 2 or width <= 0 or height <= 0:
        raise RuntimeError("tactical map did not return a live PNG")
    digest = hashlib.sha256(image).hexdigest()
    print(
        f"capture size={width}x{height} bytes={len(image)} "
        f"sha256={digest} path={path}"
    )
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--speed", type=float, default=0.012, help="orbit radians per second")
    parser.add_argument("--settle", type=float, default=5.0)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("bld/draw3d-captures/tactical-strike/showcase.png"),
    )
    parser.add_argument(
        "--static-view",
        choices=tuple(VIEWS),
        help="capture this fixed design view before enabling the final orbit",
    )
    args = parser.parse_args()

    client = Draw3dClient(args.host)
    try:
        meshes = populate(client, args.speed)
        if args.static_view:
            set_static_view(client, args.static_view)
            capture(
                client,
                args.output.with_name(f"{args.output.stem}-{args.static_view}.png"),
                args.settle,
            )
            set_showcase_orbit(client, args.speed)
        capture(client, args.output, args.settle)
        stats = client.stats()
        print(
            f"scene meshes={stats[0]} instances={stats[1]} vertices={stats[2]} "
            f"faces={stats[4]} mesh_bytes={stats[5]} batches={len(meshes)} "
            f"orbit_speed={args.speed}"
        )
    finally:
        # Closing the TCP client does not stop the kernel-owned scene.
        client.close()


if __name__ == "__main__":
    main()
