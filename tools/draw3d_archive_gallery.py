#!/usr/bin/env python3
"""Stage selected authored archive meshes as a draw3d retrospective over TCP."""

import argparse
import gzip
import hashlib
import json
import math
import time
from dataclasses import dataclass
from pathlib import Path

from draw3d_celestial_garden import GardenMesh, add_box, add_octahedron, radial_frustum, torus
from draw3d_grid_world import add_cylinder
from draw3d_house_demo import Draw3dClient


BACKGROUND = (3, 6, 16, 255)
EXPORT_DIR = Path("bld/model-archive-review/exports")

VIEWS = {
    "collection": ((27.0, 18.0, 31.0), (0.0, 5.2, 0.0), 49.0),
    "island": ((15.0, 12.5, 19.0), (0.0, 9.2, 0.0), 44.0),
    "character": ((-18.5, 7.8, 15.5), (-10.5, 4.5, 3.5), 42.0),
    "mechanical": ((20.0, 10.0, 16.0), (8.5, 4.0, 0.0), 47.0),
    "studies": ((24.0, 11.0, -22.0), (0.0, 3.5, -8.0), 47.0),
}


@dataclass(frozen=True)
class AssetSpec:
    name: str
    filename: str
    location: tuple
    target_size: float
    yaw: float
    skip_groups: tuple = ()


ASSETS = (
    AssetSpec("Floating Island", "floating-island.json.gz", (0.0, 5.3, 0.0), 13.8, -0.12),
    AssetSpec("Guy Sword", "guy-sword.json.gz", (-10.5, 1.08, 3.5), 8.2, 0.30),
    AssetSpec("Magnetic Stirrer", "mag-stirr.json.gz", (10.5, 1.08, 3.5), 7.6, -0.42),
    AssetSpec("Carved Sphere", "carved-sphere.json.gz", (-8.5, 1.08, -10.0), 5.0, 0.18, ("Material.002",)),
    AssetSpec("Remote Control", "remote-control.json.gz", (8.5, 1.08, -10.0), 8.2, -0.20),
)


CHARACTER_COLORS = {
    "Unassigned/Cylinder": (53, 113, 104, 255),
    "Unassigned/Cylinder.005": (169, 103, 57, 255),
    "Unassigned/Cylinder.006": (207, 164, 91, 255),
    "Unassigned/Cylinder.007": (72, 91, 116, 255),
    "Unassigned/Cylinder.008": (183, 198, 204, 255),
    "Unassigned/Cylinder.009": (89, 67, 129, 255),
    "Unassigned/Icosphere.001": (221, 181, 124, 255),
    "Unassigned/Sphere.002": (66, 132, 112, 255),
    "Unassigned/Sphere.003": (86, 151, 123, 255),
}

ISLAND_STRATA = {
    "snow": (211, 222, 217, 255),
    "sunlit": (181, 145, 79, 255),
    "ochre": (119, 78, 37, 255),
    "underside": (48, 40, 36, 255),
}


def compact_faces(vertices, faces):
    used = sorted({index for face in faces for index in face})
    remap = {old: new for new, old in enumerate(used)}
    return tuple(vertices[index] for index in used), tuple(tuple(remap[index] for index in face) for face in faces)


def split_island_land(vertices, faces, height):
    partitions = {name: [] for name in ISLAND_STRATA}
    for face in faces:
        a, b, c = (vertices[index] for index in face)
        ab = tuple(b[axis] - a[axis] for axis in range(3))
        ac = tuple(c[axis] - a[axis] for axis in range(3))
        normal_y = ab[2] * ac[0] - ab[0] * ac[2]
        length = math.sqrt(
            (ab[1] * ac[2] - ab[2] * ac[1]) ** 2
            + normal_y**2
            + (ab[0] * ac[1] - ab[1] * ac[0]) ** 2
        ) or 1.0
        normal_y /= length
        relative_height = sum(point[1] for point in (a, b, c)) / 3.0 / max(height, 0.001)
        if normal_y > 0.48 and relative_height > 0.58:
            partition = "snow"
        elif relative_height > 0.62:
            partition = "sunlit"
        elif relative_height > 0.34:
            partition = "ochre"
        else:
            partition = "underside"
        partitions[partition].append(face)
    return tuple(
        (name, ISLAND_STRATA[name], *compact_faces(vertices, partition_faces))
        for name, partition_faces in partitions.items()
        if partition_faces
    )


def load_asset(spec):
    path = EXPORT_DIR / spec.filename
    if not path.exists():
        raise FileNotFoundError(
            f"missing converted archive mesh {path}; run tools/blender_archive_export.py with Blender first"
        )
    with gzip.open(path, "rt", encoding="utf-8") as handle:
        payload = json.load(handle)
    groups = [group for group in payload["groups"] if group["name"] not in spec.skip_groups]
    all_vertices = [vertex for group in groups for vertex in group["vertices"]]
    minimum = [min(vertex[axis] for vertex in all_vertices) for axis in range(3)]
    maximum = [max(vertex[axis] for vertex in all_vertices) for axis in range(3)]
    center_x = (minimum[0] + maximum[0]) * 0.5
    center_z = (minimum[2] + maximum[2]) * 0.5
    extent = max(maximum[axis] - minimum[axis] for axis in range(3)) or 1.0
    scale = spec.target_size / extent

    normalized = []
    for group in groups:
        vertices = tuple(
            (vertex[0] - center_x, vertex[1] - minimum[1], vertex[2] - center_z)
            for vertex in group["vertices"]
        )
        color = tuple(group["color"])
        faces = tuple(tuple(face) for face in group["faces"])
        if spec.name == "Floating Island" and group["name"] == "Material.002":
            normalized.extend(
                (f"{group['name']}/{name}", layer_color, layer_vertices, layer_faces)
                for name, layer_color, layer_vertices, layer_faces in split_island_land(
                    vertices,
                    faces,
                    maximum[1] - minimum[1],
                )
            )
            continue
        if spec.name == "Guy Sword":
            color = CHARACTER_COLORS.get(group["name"], color)
        normalized.append((group["name"], color, vertices, faces))
    return payload["source"], normalized, scale


def build_gallery_environment():
    floor = GardenMesh()
    floor.add_xyz(
        *radial_frustum(48, bottom_radius=0.94, top_radius=1.0, height=2.0),
        (0.0, -0.28, 0.0),
        (22.5, 0.28, 18.5),
    )

    floor_inlay = GardenMesh()
    for radius in (6.2, 12.5, 17.5):
        floor_inlay.add_xyz(*torus(radius, 0.045, 48, 5), (0.0, 0.035, 0.0))

    paths = GardenMesh()
    for x, z, sx, sz, yaw in (
        (-5.3, 1.8, 5.5, 0.40, -0.32),
        (5.3, 1.8, 5.5, 0.40, 0.32),
        (-4.3, -5.0, 5.1, 0.34, 0.38),
        (4.3, -5.0, 5.1, 0.34, -0.38),
    ):
        add_box(paths, (x, 0.08, z), (sx, 0.045, sz), (0.0, yaw, 0.0))

    pedestals = GardenMesh()
    trim_cool = GardenMesh()
    trim_warm = GardenMesh()
    for index, (x, z, radius) in enumerate(((-10.5, 3.5, 3.25), (10.5, 3.5, 3.25), (-8.5, -10.0, 2.8), (8.5, -10.0, 3.25))):
        add_cylinder(pedestals, (x, 0.46, z), radius, 0.90, 20)
        target = trim_cool if index % 2 == 0 else trim_warm
        target.add_xyz(*torus(radius * 0.90, 0.055, 30, 5), (x, 0.95, z))

    island_ring = GardenMesh()
    island_ring.add_xyz(*torus(6.5, 0.075, 48, 6), (0.0, 0.18, 0.0))
    island_ring.add_xyz(*torus(4.9, 0.040, 40, 5), (0.0, 0.22, 0.0))

    arches = GardenMesh()
    for radius, y, z in ((7.2, 7.5, -4.6), (8.6, 7.5, -5.0)):
        arches.add_xyz(*torus(radius, 0.055, 48, 5), (0.0, y, z), rotation=(math.pi / 2.0, 0.0, 0.0))

    monoliths = GardenMesh()
    for x, z, height in ((-17.0, -6.0, 4.0), (17.0, -6.0, 4.0), (-15.0, 10.0, 3.0), (15.0, 10.0, 3.0)):
        add_box(monoliths, (x, height * 0.5, z), (0.35, height * 0.5, 0.35))
        add_octahedron(monoliths, (x, height + 0.45, z), (0.32, 0.55, 0.32))

    motes_gold = GardenMesh()
    motes_blue = GardenMesh()
    for index in range(26):
        angle = index * 2.399963
        radius = 7.0 + (index % 5) * 2.4
        x = math.cos(angle) * radius
        z = math.sin(angle) * radius
        y = 2.2 + (index % 7) * 1.15
        target = motes_gold if index % 3 == 0 else motes_blue
        add_octahedron(target, (x, y, z), (0.11, 0.20, 0.11), (0.0, angle, 0.0))

    return (
        (7_000, (17, 27, 40, 255), floor),
        (7_001, (54, 104, 124, 255), floor_inlay),
        (7_002, (66, 82, 94, 255), paths),
        (7_003, (48, 55, 67, 255), pedestals),
        (7_004, (77, 167, 171, 255), trim_cool),
        (7_005, (210, 151, 65, 255), trim_warm),
        (7_006, (62, 183, 195, 255), island_ring),
        (7_007, (52, 104, 139, 255), arches),
        (7_008, (94, 109, 126, 255), monoliths),
        (7_009, (239, 190, 73, 255), motes_gold),
        (7_010, (93, 209, 230, 255), motes_blue),
    )


def validate(label, vertices, faces):
    triangles = sum(len(face) - 2 for face in faces)
    if len(vertices) > 1_000 or triangles > 2_000:
        raise RuntimeError(f"{label} exceeds draw3d budget: {len(vertices)} vertices/{triangles} triangles")


def populate(client):
    client.stop()
    client.clear()
    client.camera(*VIEWS["collection"])

    next_instance = 110_000
    for mesh_id, color, mesh in build_gallery_environment():
        validate(f"environment {mesh_id}", mesh.vertices, mesh.faces)
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(next_instance, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
        next_instance += 1

    next_mesh = 7_100
    asset_stats = []
    for spec in ASSETS:
        source, groups, scale = load_asset(spec)
        asset_vertices = asset_triangles = 0
        for group_name, color, vertices, faces in groups:
            validate(f"{spec.name}/{group_name}", vertices, faces)
            client.mesh(next_mesh, color, vertices, faces)
            client.instance(
                next_instance,
                next_mesh,
                spec.location,
                (scale, scale, scale),
                (0.0, spec.yaw, 0.0),
            )
            asset_vertices += len(vertices)
            asset_triangles += len(faces)
            next_mesh += 1
            next_instance += 1
        asset_stats.append((spec.name, source, len(groups), asset_vertices, asset_triangles))

    client.start(BACKGROUND)
    return next_mesh - 7_100, asset_stats


def capture_view(client, name, output_dir, settle):
    client.camera(*VIEWS[name])
    time.sleep(settle)
    path, image_format, width, height, image = client.render(output_dir / f"archive-gallery-{name}.png")
    if image_format != 2 or not image:
        raise RuntimeError(f"{name} did not return a PNG capture")
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
        imported_meshes, asset_stats = populate(client)
        for view_name in ("character", "mechanical", "studies", "island", "collection"):
            capture_view(client, view_name, args.output_dir, args.settle)
        stats = client.stats()
        for name, source, groups, vertices, triangles in asset_stats:
            print(
                f"asset={name!r} groups={groups} vertices={vertices} triangles={triangles} source={source}"
            )
        print(
            f"imported_meshes={imported_meshes} scene_meshes={stats[0]} instances={stats[1]} "
            f"vertices={stats[2]} faces={stats[4]} mesh_bytes={stats[5]} final_view=collection"
        )
    finally:
        client.close()


if __name__ == "__main__":
    main()
