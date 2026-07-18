#!/usr/bin/env python3
"""Compose the grid world, Hero of Time relics, and Great Deku Tree over draw3d TCP."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh, add_box, add_octahedron
from draw3d_grid_world import (
    add_cone,
    add_cylinder,
    build_heightfield_chunks,
    build_world_details,
    terrain_height,
)
from draw3d_hero_of_time import ENVIRONMENT_MESH_IDS, build_scene as build_hero_scene
from draw3d_house_demo import Draw3dClient
from draw3d_poly_trees import build_great_deku


CELLS = 80
CHUNK_CELLS = 27
WORLD_SIZE = 64.0
HERO_LOCATION = (-14.0, 5.42, -10.0)
HERO_SCALE = (1.08, 1.08, 1.08)
HERO_YAW = -0.18
DEKU_LOCATION = (14.0, 7.04, 11.0)
DEKU_SCALE = (1.18, 1.18, 1.18)
DEKU_YAW = -0.16

VIEWS = {
    "overview": ((40.0, 27.0, 43.0), (0.0, 4.2, 0.0), 51.0),
    "journey": ((-37.0, 18.0, 35.0), (0.0, 4.0, 1.0), 50.0),
    "crossing": ((-10.0, 10.0, 14.0), (3.0, 1.5, 1.0), 50.0),
    "hero": ((-2.0, 13.5, 8.5), (-14.0, 8.7, -10.0), 45.0),
    "deku": ((5.0, 16.5, 29.0), (14.0, 11.6, 11.0), 44.0),
}


def add_tree_to_layers(trunks, crowns_dark, crowns_light, x, z, scale, yaw=0.0):
    y = terrain_height(x, z)
    add_cylinder(trunks, (x, y + scale * 0.78, z), scale * 0.18, scale * 1.56, 7, (0.0, yaw, 0.0))
    add_cone(
        crowns_dark,
        (x, y + scale * 1.72, z - 0.08),
        scale * 0.82,
        scale * 1.72,
        8,
        (0.0, yaw, 0.0),
    )
    add_cone(
        crowns_light,
        (x + 0.10, y + scale * 2.18, z + 0.14),
        scale * 0.57,
        scale * 1.20,
        7,
        (0.0, yaw - 0.10, 0.0),
    )


def build_forest_details():
    details = list(build_world_details(WORLD_SIZE))
    by_id = {mesh_id: mesh for mesh_id, _, mesh in details}
    trunks = by_id[5_266]
    crowns_dark = by_id[5_267]
    crowns_light = by_id[5_268]

    # Dense perimeter clusters frame the landmarks while keeping the river corridor open.
    forest_sites = (
        (-29.0, -27.0, 2.45, 0.12), (-25.0, -29.0, 1.85, -0.14), (-20.0, -26.0, 2.20, 0.26),
        (-10.0, -28.0, 1.80, -0.20), (-2.0, -27.0, 2.05, 0.18), (14.0, -27.0, 2.30, -0.12),
        (22.0, -28.0, 1.88, 0.24), (29.0, -24.0, 2.45, -0.16), (29.0, -13.0, 1.95, 0.20),
        (28.0, 1.0, 2.25, -0.24), (29.0, 12.0, 2.05, 0.10), (27.0, 27.0, 2.50, -0.12),
        (18.0, 28.0, 1.90, 0.18), (8.0, 28.0, 2.25, -0.18), (-4.0, 28.0, 1.88, 0.24),
        (-15.0, 27.0, 2.30, -0.10), (-27.0, 27.0, 2.48, 0.12), (-29.0, 16.0, 1.95, -0.22),
        (-29.0, 4.0, 2.20, 0.16), (-28.0, -8.0, 1.82, -0.10), (-22.0, 5.0, 1.60, 0.18),
        (-19.0, 14.0, 1.72, -0.18), (20.0, 2.0, 1.68, 0.12), (21.0, 18.0, 1.80, -0.20),
    )
    for site in forest_sites:
        add_tree_to_layers(trunks, crowns_dark, crowns_light, *site)

    roots_and_stumps = GardenMesh()
    for x, z, scale, yaw in (
        (-20.0, -4.0, 0.92, 0.18), (-7.0, 17.0, 0.72, -0.24),
        (8.0, 21.0, 0.82, 0.12), (23.0, -9.0, 0.76, -0.18),
    ):
        y = terrain_height(x, z)
        add_cylinder(roots_and_stumps, (x, y + scale * 0.42, z), scale * 0.48, scale * 0.84, 8)
        for angle in (0.0, math.tau / 3.0, math.tau * 2.0 / 3.0):
            add_box(
                roots_and_stumps,
                (x + math.cos(angle) * scale * 0.52, y + 0.12, z + math.sin(angle) * scale * 0.52),
                (scale * 0.56, 0.11, scale * 0.13),
                (0.0, -angle, 0.0),
            )

    fern_dark = GardenMesh()
    fern_light = GardenMesh()
    fern_sites = (
        (-24.0, -15.0), (-21.0, -12.0), (-18.5, -18.0), (-10.0, -19.0), (-5.0, -15.0),
        (5.0, -21.0), (12.0, -20.0), (21.0, -20.0), (24.0, -3.0), (24.0, 8.0),
        (23.0, 15.0), (16.0, 20.0), (9.0, 23.0), (0.0, 20.0), (-10.0, 22.0),
        (-18.0, 20.0), (-23.0, 11.0), (-24.0, 0.0), (-18.0, 2.0), (17.5, 3.5),
    )
    for index, (x, z) in enumerate(fern_sites):
        y = terrain_height(x, z)
        for blade in range(5):
            angle = blade * math.tau / 5.0 + index * 0.17
            dx, dz = math.cos(angle) * 0.26, math.sin(angle) * 0.26
            target = fern_light if (index + blade) % 3 == 0 else fern_dark
            add_cone(target, (x + dx, y + 0.38, z + dz), 0.12, 0.75, 5, (0.0, angle, math.sin(angle) * 0.38))

    mushrooms = GardenMesh()
    for x, z, size in (
        (-21.0, -3.0, 0.22), (-20.5, -2.5, 0.16), (-8.0, 14.0, 0.20),
        (7.5, 18.0, 0.18), (18.0, 5.0, 0.24), (22.0, -10.0, 0.17),
    ):
        y = terrain_height(x, z)
        add_cylinder(mushrooms, (x, y + size * 0.62, z), size * 0.20, size * 0.75, 6)
        add_octahedron(mushrooms, (x, y + size * 1.10, z), (size, size * 0.45, size))

    shrine_stones = GardenMesh()
    for x, z, yaw in ((-7.8, -10.0, 0.0), (8.8, 11.0, 0.0), (16.0, 4.0, 0.32), (-5.0, 7.0, -0.28)):
        y = terrain_height(x, z)
        add_box(shrine_stones, (x, y + 0.72, z), (0.42, 0.72, 0.32), (0.0, yaw, 0.0))
        add_box(shrine_stones, (x, y + 1.52, z), (0.62, 0.10, 0.46), (0.0, yaw, 0.0))

    details.extend(
        (
            (5_270, (86, 52, 31, 255), roots_and_stumps),
            (5_271, (21, 75, 47, 255), fern_dark),
            (5_272, (62, 137, 70, 255), fern_light),
            (5_273, (185, 62, 48, 255), mushrooms),
            (5_274, (109, 116, 112, 255), shrine_stones),
        )
    )
    return tuple(details)


def validate_mesh(label, mesh):
    triangles = sum(len(face) - 2 for face in mesh.faces)
    if len(mesh.vertices) > 1_000 or triangles > 2_000:
        raise RuntimeError(f"{label} exceeds budget: vertices={len(mesh.vertices)} triangles={triangles}")


def upload_mesh_instance(client, mesh_id, instance_id, color, mesh, location=(0.0, 0.0, 0.0), scale=(1.0, 1.0, 1.0), yaw=0.0):
    validate_mesh(f"mesh {mesh_id}", mesh)
    client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
    client.instance(instance_id, mesh_id, location, scale, (0.0, yaw, 0.0))


def populate(client):
    client.stop()
    client.clear()
    client.camera(*VIEWS["overview"])

    terrain = build_heightfield_chunks(CELLS, CHUNK_CELLS, WORLD_SIZE)
    details = build_forest_details()
    hero_parts = tuple(
        part for part in build_hero_scene()
        if part[0] not in ENVIRONMENT_MESH_IDS or part[0] in (1_101, 1_128, 1_132, 1_133)
    )
    deku_parts = build_great_deku()
    total = len(terrain) + len(details) + len(hero_parts) + len(deku_parts)
    if total > 100:
        raise RuntimeError(f"integrated forest requires {total} meshes/instances, above draw3d's limit of 100")

    for mesh_id, color, vertices, faces in terrain:
        client.mesh(mesh_id, color, vertices, faces)
        client.instance(80_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))

    for mesh_id, color, mesh in details:
        upload_mesh_instance(client, mesh_id, 80_000 + mesh_id, color, mesh)

    for mesh_id, color, mesh in hero_parts:
        upload_mesh_instance(
            client,
            mesh_id,
            90_000 + mesh_id,
            color,
            mesh,
            HERO_LOCATION,
            HERO_SCALE,
            HERO_YAW,
        )

    for index, (_, color, mesh) in enumerate(deku_parts):
        mesh_id = 6_200 + index
        upload_mesh_instance(
            client,
            mesh_id,
            96_200 + index,
            color,
            mesh,
            DEKU_LOCATION,
            DEKU_SCALE,
            DEKU_YAW,
        )

    # No explicit clear color: let UI4 compose the scene over its surroundings.
    client.start()
    return len(terrain), len(details), len(hero_parts), len(deku_parts)


def capture_view(client, name, output_dir, settle):
    client.camera(*VIEWS[name])
    time.sleep(settle)
    path, image_format, width, height, image = client.render(output_dir / f"forest-world-{name}.png")
    if image_format != 2 or width <= 0 or height <= 0:
        raise RuntimeError(f"{name} did not return a non-empty PNG target")
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
        counts = populate(client)
        for view_name in ("crossing", "hero", "deku", "overview", "journey"):
            capture_view(client, view_name, args.output_dir, args.settle)
        stats = client.stats()
        print(
            f"terrain={counts[0]} details={counts[1]} hero={counts[2]} deku={counts[3]} "
            f"scene_meshes={stats[0]} instances={stats[1]} vertices={stats[2]} "
            f"faces={stats[4]} mesh_bytes={stats[5]} final_view=journey"
        )
    finally:
        client.close()


if __name__ == "__main__":
    main()
