#!/usr/bin/env python3
"""Render three isolated album views for every harvested archive asset over draw3d TCP."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_archive_gallery import ASSETS, BACKGROUND, load_asset, validate
from draw3d_celestial_garden import GardenMesh, add_box, add_octahedron, radial_frustum, torus
from draw3d_grid_world import add_cylinder
from draw3d_house_demo import Draw3dClient


ACCENTS = {
    "Floating Island": (71, 187, 202, 255),
    "Guy Sword": (212, 153, 68, 255),
    "Magnetic Stirrer": (76, 205, 186, 255),
    "Carved Sphere": (37, 198, 226, 255),
    "Remote Control": (229, 192, 77, 255),
}


def slugify(name):
    return name.lower().replace(" ", "-")


def model_dimensions(groups, scale):
    vertices = [vertex for _, _, group_vertices, _ in groups for vertex in group_vertices]
    minimum = [min(vertex[axis] for vertex in vertices) for axis in range(3)]
    maximum = [max(vertex[axis] for vertex in vertices) for axis in range(3)]
    return tuple((maximum[axis] - minimum[axis]) * scale for axis in range(3))


def build_stage(name, dimensions, floating):
    width, height, depth = dimensions
    footprint = max(width, depth)
    stage_radius = max(6.0, footprint * 0.72 + 2.3)
    accent = ACCENTS[name]

    floor = GardenMesh()
    floor.add_xyz(
        *radial_frustum(48, bottom_radius=0.94, top_radius=1.0, height=2.0),
        (0.0, -0.24, 0.0),
        (stage_radius * 1.45, 0.24, stage_radius),
    )

    floor_rings = GardenMesh()
    for factor in (0.46, 0.74, 1.02):
        floor_rings.add_xyz(*torus(stage_radius * factor, 0.035, 44, 5), (0.0, 0.035, 0.0))

    pedestal = GardenMesh()
    if not floating:
        add_cylinder(pedestal, (0.0, 0.43, 0.0), max(2.4, footprint * 0.57), 0.84, 24)
        add_cylinder(pedestal, (0.0, 0.82, 0.0), max(2.1, footprint * 0.51), 0.10, 24)
    else:
        add_cylinder(pedestal, (0.0, 0.16, 0.0), max(3.4, footprint * 0.44), 0.22, 28)

    trim = GardenMesh()
    trim_radius = max(2.1, footprint * (0.50 if not floating else 0.42))
    trim.add_xyz(*torus(trim_radius, 0.065, 40, 6), (0.0, 0.91 if not floating else 0.31, 0.0))
    trim.add_xyz(*torus(trim_radius * 0.78, 0.035, 36, 5), (0.0, 0.94 if not floating else 0.34, 0.0))

    arches = GardenMesh()
    arch_radius = max(4.0, height * 0.62, footprint * 0.46)
    arch_center_y = (1.0 if not floating else 2.3) + height * 0.52
    arch_z = -max(depth * 0.52, 2.2)
    for scale in (1.0, 1.18):
        arches.add_xyz(
            *torus(arch_radius * scale, 0.05, 44, 5),
            (0.0, arch_center_y, arch_z - (scale - 1.0) * 1.2),
            rotation=(math.pi / 2.0, 0.0, 0.0),
        )

    pylons = GardenMesh()
    pylon_x = min(stage_radius * 0.94, max(width * 0.72, 4.0))
    pylon_height = max(2.5, height * 0.42)
    for side in (-1.0, 1.0):
        add_box(pylons, (side * pylon_x, pylon_height * 0.50, -1.0), (0.25, pylon_height * 0.50, 0.25))
        add_octahedron(pylons, (side * pylon_x, pylon_height + 0.40, -1.0), (0.28, 0.48, 0.28))

    motes = GardenMesh()
    for index in range(18):
        angle = index * 2.399963
        radius = stage_radius * (0.55 + (index % 4) * 0.13)
        x = math.cos(angle) * radius
        z = math.sin(angle) * radius
        y = 1.4 + (index % 6) * max(0.55, height * 0.10)
        add_octahedron(motes, (x, y, z), (0.10, 0.18, 0.10), (0.0, angle, 0.0))

    return (
        (7_600, (14, 23, 36, 255), floor),
        (7_601, (42, 78, 97, 255), floor_rings),
        (7_602, (47, 55, 68, 255), pedestal),
        (7_603, accent, trim),
        (7_604, (49, 92, 121, 255), arches),
        (7_605, (91, 106, 124, 255), pylons),
        (7_606, accent, motes),
    )


def album_views(dimensions, base_y):
    width, height, depth = dimensions
    span = max(width, height, depth)
    distance = max(span * 1.85, 12.5)
    center_y = base_y + height * 0.50
    target = (0.0, center_y, 0.0)
    return {
        "01-three-quarter": ((distance * 0.48, center_y + height * 0.24, distance * 0.88), target, 44.0),
        "02-opposite-turn": ((-distance * 0.55, center_y + height * 0.14, distance * 0.82), target, 44.0),
        "03-elevated-detail": (
            (distance * 0.30, center_y + distance * 0.48, distance * 0.88),
            (0.0, center_y + height * 0.08, 0.0),
            43.0,
        ),
    }


def populate_asset(client, spec):
    source, groups, scale = load_asset(spec)
    dimensions = model_dimensions(groups, scale)
    floating = spec.name == "Floating Island"
    base_y = 2.25 if floating else 0.94
    views = album_views(dimensions, base_y)

    client.stop()
    client.clear()
    client.camera(*views["01-three-quarter"])

    next_instance = 130_000
    for mesh_id, color, mesh in build_stage(spec.name, dimensions, floating):
        validate(f"album stage {mesh_id}", mesh.vertices, mesh.faces)
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(next_instance, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
        next_instance += 1

    next_mesh = 7_700
    for group_name, color, vertices, faces in groups:
        validate(f"{spec.name}/{group_name}", vertices, faces)
        client.mesh(next_mesh, color, vertices, faces)
        client.instance(
            next_instance,
            next_mesh,
            (0.0, base_y, 0.0),
            (scale, scale, scale),
            (0.0, spec.yaw, 0.0),
        )
        next_mesh += 1
        next_instance += 1

    client.start(BACKGROUND)
    return source, groups, dimensions, views


def capture(client, spec, view_name, camera, output_dir, settle):
    client.camera(*camera)
    time.sleep(settle)
    path, image_format, width, height, image = client.render(
        output_dir / slugify(spec.name) / f"{slugify(spec.name)}-{view_name}.png"
    )
    if image_format != 2 or not image:
        raise RuntimeError(f"{spec.name}/{view_name} did not return a PNG")
    digest = hashlib.sha256(image).hexdigest()
    print(
        f"asset={spec.name!r} view={view_name} size={width}x{height} bytes={len(image)} "
        f"sha256={digest} path={path}"
    )
    return path, digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=3.0)
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=Path("bld/draw3d-captures/archive-album"),
    )
    parser.add_argument("--asset", action="append", help="render only this exact asset name")
    args = parser.parse_args()
    selected = [spec for spec in ASSETS if not args.asset or spec.name in args.asset]
    if not selected:
        raise SystemExit("no requested asset name matched")

    client = Draw3dClient(args.host)
    try:
        for spec in selected:
            source, groups, dimensions, views = populate_asset(client, spec)
            for view_name, camera in views.items():
                capture(client, spec, view_name, camera, args.output_dir, args.settle)
            stats = client.stats()
            print(
                f"album_asset={spec.name!r} source={source} groups={len(groups)} "
                f"dimensions={tuple(round(value, 3) for value in dimensions)} "
                f"scene_meshes={stats[0]} faces={stats[4]}"
            )

        # Leave the environment centerpiece live after the batch finishes.
        live_spec = next((spec for spec in ASSETS if spec.name == "Floating Island"), selected[0])
        _, _, _, live_views = populate_asset(client, live_spec)
        client.camera(*live_views["01-three-quarter"])
        time.sleep(args.settle)
        path, _, width, height, image = client.render(args.output_dir / "archive-album-final-live.png")
        print(
            f"final_live={path} size={width}x{height} bytes={len(image)} "
            f"sha256={hashlib.sha256(image).hexdigest()} asset={live_spec.name!r}"
        )
    finally:
        client.close()


if __name__ == "__main__":
    main()
