#!/usr/bin/env python3
"""Present one monumental reusable Great Deku Tree through draw3d TCP."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh, add_box, add_octahedron, radial_frustum, torus
from draw3d_house_demo import Draw3dClient
from draw3d_poly_trees import build_great_deku


BACKGROUND = (4, 8, 20, 255)
VIEWS = {
    "low": ((0.0, 2.65, 15.8), (0.0, 3.95, 0.0), 49.0),
    "three-quarter": ((8.8, 5.1, 16.2), (0.0, 4.15, 0.0), 47.0),
    "portrait": ((0.0, 4.85, 18.2), (0.0, 4.20, 0.0), 45.0),
}


def build_environment():
    ground = GardenMesh()
    ground.add_xyz(
        *radial_frustum(28, bottom_radius=0.86, top_radius=1.0, height=2.0),
        (0.0, -0.40, 0.0),
        (6.6, 0.40, 5.2),
    )

    moss = GardenMesh()
    for x, z, sx, sz, angle in (
        (-2.4, 0.8, 1.2, 0.55, -0.18),
        (2.6, 0.4, 1.0, 0.50, 0.22),
        (-1.2, 2.2, 0.72, 0.42, 0.14),
        (1.5, 2.4, 0.80, 0.44, -0.12),
    ):
        add_box(moss, (x, 0.09, z), (sx, 0.025, sz), (0.0, angle, 0.0))

    stones = GardenMesh()
    for x, z, scale, angle in (
        (-4.4, 1.0, 0.42, -0.12),
        (4.5, 0.8, 0.38, 0.18),
        (-3.6, -2.0, 0.31, 0.10),
        (3.7, -2.2, 0.34, -0.14),
        (-0.4, 3.6, 0.24, 0.20),
    ):
        add_octahedron(stones, (x, 0.08, z), (scale, scale * 0.65, scale * 0.86), (0.0, angle, 0.0))

    root_ring = GardenMesh()
    root_ring.add_xyz(*torus(3.15, 0.035, 42, 5), (0.0, 0.10, 0.0))
    root_ring.add_xyz(*torus(3.55, 0.020, 42, 4), (0.0, 0.09, 0.0))

    moon_halo = GardenMesh()
    moon_halo.add_xyz(*torus(2.80, 0.045, 48, 5), (0.0, 6.65, -4.1), rotation=(math.pi / 2, 0.0, 0.0))

    motes = GardenMesh()
    for x, y, z, size in (
        (-4.7, 5.6, 0.4, 0.07), (-3.3, 7.3, -0.4, 0.055), (-2.0, 4.5, 1.5, 0.045),
        (2.1, 5.1, 1.3, 0.050), (3.5, 7.0, -0.2, 0.060), (4.8, 4.8, 0.6, 0.070),
        (-3.9, 2.2, 1.0, 0.040), (3.8, 2.6, 1.2, 0.044),
    ):
        add_octahedron(motes, (x, y, z), (size, size * 1.8, size))

    fairies = GardenMesh()
    for x, y, z, flip in ((-2.75, 4.15, 1.4, -1.0), (2.95, 5.55, 0.9, 1.0)):
        add_octahedron(fairies, (x, y, z), (0.13, 0.16, 0.11))
        add_octahedron(fairies, (x - 0.20 * flip, y + 0.12, z - 0.02), (0.18, 0.09, 0.045), (0.0, 0.0, 0.40 * flip))
        add_octahedron(fairies, (x + 0.20 * flip, y - 0.10, z - 0.02), (0.18, 0.09, 0.045), (0.0, 0.0, -0.40 * flip))

    mushrooms = GardenMesh()
    for x, z, scale in ((-2.10, 2.35, 0.20), (-1.70, 2.55, 0.14), (2.35, 2.10, 0.18)):
        add_box(mushrooms, (x, 0.20 * scale / 0.20, z), (0.035, 0.16 * scale / 0.20, 0.035))
        add_octahedron(mushrooms, (x, 0.39 * scale / 0.20, z), (scale, scale * 0.45, scale))

    return (
        (3_200, (23, 60, 47, 255), ground),
        (3_201, (48, 111, 58, 255), moss),
        (3_202, (77, 87, 92, 255), stones),
        (3_203, (60, 207, 190, 215), root_ring),
        (3_204, (231, 194, 74, 205), moon_halo),
        (3_205, (241, 218, 111, 255), motes),
        (3_206, (124, 236, 255, 255), fairies),
        (3_207, (191, 74, 67, 255), mushrooms),
    )


def populate(client):
    client.stop()
    client.clear()
    position, target, fov = VIEWS["portrait"]
    client.camera(position, target, fov)

    for component_index, (_, color, mesh) in enumerate(build_great_deku()):
        triangles = sum(len(face) - 2 for face in mesh.faces)
        if len(mesh.vertices) > 1_000 or triangles > 2_000:
            raise RuntimeError(
                f"Deku component {component_index} exceeds budget: "
                f"vertices={len(mesh.vertices)} triangles={triangles}"
            )
        mesh_id = 3_100 + component_index
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(63_100 + component_index, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))

    for mesh_id, color, mesh in build_environment():
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(60_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))

    client.start(BACKGROUND)


def capture_view(client, name, output_dir, settle):
    position, target, fov = VIEWS[name]
    client.camera(position, target, fov)
    time.sleep(settle)
    path, image_format, width, height, image = client.render(output_dir / f"great-deku-tree-{name}.png")
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
        for view_name in ("low", "three-quarter", "portrait"):
            capture_view(client, view_name, args.output_dir, args.settle)
        stats = client.stats()
        print(
            f"deku-tree meshes={stats[0]} instances={stats[1]} vertices={stats[2]} "
            f"edges={stats[3]} faces={stats[4]} mesh_bytes={stats[5]} final_view=portrait"
        )
    finally:
        client.close()


if __name__ == "__main__":
    main()
