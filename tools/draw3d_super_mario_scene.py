#!/usr/bin/env python3
"""Present a compact, colorful Super Mario-inspired diorama through draw3d TCP."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh, add_box, radial_frustum, torus
from draw3d_house_demo import Draw3dClient


BACKGROUND = (91, 170, 235, 255)
IDENTITY = (0.0, 0.0, 0.0)
UNIT_SCALE = (1.0, 1.0, 1.0)


def uv_sphere(segments=12, rings=7):
    """Return a modest low-poly sphere with quad latitude bands."""
    vertices = [(0.0, 1.0, 0.0)]
    for ring in range(1, rings):
        latitude = math.pi * ring / rings
        radius = math.sin(latitude)
        y = math.cos(latitude)
        for segment in range(segments):
            longitude = math.tau * segment / segments
            vertices.append((math.cos(longitude) * radius, y, math.sin(longitude) * radius))
    vertices.append((0.0, -1.0, 0.0))

    top = 0
    bottom = len(vertices) - 1
    faces = []
    for segment in range(segments):
        nxt = (segment + 1) % segments
        faces.append((top, 1 + segment, 1 + nxt))
    for ring in range(rings - 2):
        first = 1 + ring * segments
        following = first + segments
        for segment in range(segments):
            nxt = (segment + 1) % segments
            faces.append((first + segment, following + segment, following + nxt, first + nxt))
    last_ring = 1 + (rings - 2) * segments
    for segment in range(segments):
        nxt = (segment + 1) % segments
        faces.append((last_ring + segment, bottom, last_ring + nxt))
    return tuple(vertices), tuple(faces)


def extruded_polygon(points, depth):
    """Extrude an XY silhouette along Z for flags and graphic accents."""
    half_depth = depth * 0.5
    vertices = [(x, y, -half_depth) for x, y in points]
    vertices.extend((x, y, half_depth) for x, y in points)
    count = len(points)
    faces = [tuple(range(count - 1, -1, -1)), tuple(range(count, count * 2))]
    for index in range(count):
        nxt = (index + 1) % count
        faces.append((index, nxt, count + nxt, count + index))
    return tuple(vertices), tuple(faces)


SPHERE = uv_sphere()
CYLINDER_16 = radial_frustum(16)
CYLINDER_12 = radial_frustum(12)


def add_sphere(mesh, location, scale, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(*SPHERE, location, scale, rotation)


def add_cylinder(mesh, location, radius, height, segments=16, rotation=(0.0, 0.0, 0.0)):
    primitive = CYLINDER_16 if segments == 16 else CYLINDER_12
    mesh.add_xyz(*primitive, location, (radius, height * 0.5, radius), rotation)


def add_question_mark(mesh, x, y, z):
    """Build a chunky, readable question mark on a block's front face."""
    for location, scale in (
        ((x - 0.10, y + 0.20, z), (0.18, 0.065, 0.025)),
        ((x + 0.095, y + 0.105, z), (0.065, 0.15, 0.025)),
        ((x, y - 0.035, z), (0.105, 0.060, 0.025)),
        ((x - 0.065, y - 0.145, z), (0.050, 0.085, 0.025)),
        ((x - 0.065, y - 0.335, z), (0.060, 0.060, 0.025)),
    ):
        add_box(mesh, location, scale)


def build_scene():
    # One mesh per palette color keeps repeated details cheap while still giving
    # the unlit renderer purposeful highlight and shadow planes.
    soil = GardenMesh()
    soil_dark = GardenMesh()
    grass = GardenMesh()
    grass_dark = GardenMesh()
    green = GardenMesh()
    green_dark = GardenMesh()
    brick = GardenMesh()
    outline = GardenMesh()
    gold = GardenMesh()
    gold_light = GardenMesh()
    red = GardenMesh()
    blue = GardenMesh()
    skin = GardenMesh()
    brown = GardenMesh()
    cream = GardenMesh()
    white = GardenMesh()
    cloud_shadow = GardenMesh()
    pole = GardenMesh()

    # Chunky side-scrolling ground with a checkerboard soil face.
    add_box(soil, (0.0, -0.58, 0.0), (8.55, 0.82, 3.0))
    add_box(grass, (0.0, 0.30, 0.0), (8.78, 0.13, 3.12))
    add_box(grass_dark, (0.0, 0.13, -3.13), (8.78, 0.075, 0.055))
    for row, y in enumerate((-0.16, -0.72, -1.25)):
        for column, x in enumerate(tuple(-8.0 + step * 0.94 for step in range(18))):
            if (row + column) % 2 == 0:
                add_box(soil_dark, (x, y, -3.055), (0.37, 0.20, 0.035))

    # Layered Mushroom Kingdom hills and bushes sit behind the playable strip.
    for location, scale in (
        ((-5.9, 1.08, 3.72), (2.35, 1.60, 0.82)),
        ((1.1, 0.92, 3.88), (1.85, 1.35, 0.70)),
        ((6.3, 1.18, 3.64), (2.25, 1.72, 0.78)),
    ):
        add_sphere(grass, location, scale)
    for x, y, z, sx, sy in (
        (-6.35, 1.25, 2.93, 0.23, 0.34),
        (-5.52, 1.70, 2.94, 0.20, 0.30),
        (0.75, 1.08, 3.20, 0.18, 0.27),
        (5.82, 1.26, 2.91, 0.22, 0.33),
        (6.65, 1.82, 2.94, 0.20, 0.30),
    ):
        add_sphere(grass_dark, (x, y, z), (sx, sy, 0.055))
    for x, z, size in ((-2.2, 2.35, 0.72), (2.9, 2.55, 0.62), (7.6, 2.25, 0.52)):
        add_sphere(green, (x - size * 0.55, 0.62, z), (size, size * 0.66, size * 0.48))
        add_sphere(green, (x + size * 0.48, 0.58, z), (size * 0.88, size * 0.58, size * 0.43))
        add_sphere(green_dark, (x, 0.43, z - size * 0.43), (size * 1.25, 0.18, 0.12))

    # Soft cloud clumps have a blue-grey underside for depth without alpha.
    for x, y, z, scale in (
        (-5.9, 5.75, 4.65, (1.10, 0.47, 0.48)),
        (-6.75, 5.65, 4.66, (0.72, 0.58, 0.43)),
        (-5.05, 5.65, 4.66, (0.78, 0.62, 0.44)),
        (3.9, 6.25, 4.75, (1.02, 0.43, 0.46)),
        (3.15, 6.18, 4.75, (0.66, 0.54, 0.40)),
        (4.68, 6.17, 4.75, (0.72, 0.56, 0.42)),
    ):
        add_sphere(white, (x, y, z), scale)
    for x, y, z, sx in ((-5.9, 5.47, 4.62, 1.28), (3.9, 5.98, 4.72, 1.18)):
        add_sphere(cloud_shadow, (x, y, z), (sx, 0.22, 0.43))

    # Floating brick run and two question blocks.
    brick_centers = (-2.65, -1.50, 1.55, 2.70)
    for x in brick_centers:
        add_box(brick, (x, 3.18, 0.0), (0.53, 0.53, 0.53))
        add_box(outline, (x, 3.18, -0.555), (0.47, 0.035, 0.025))
        add_box(outline, (x - 0.22, 3.42, -0.557), (0.025, 0.20, 0.026))
        add_box(outline, (x + 0.22, 2.94, -0.557), (0.025, 0.20, 0.026))
    for x, y in ((-0.35, 3.18), (4.25, 4.20)):
        add_box(gold, (x, y, 0.0), (0.53, 0.53, 0.53))
        add_box(gold_light, (x, y, -0.555), (0.43, 0.43, 0.025))
        add_question_mark(outline, x, y, -0.605)

    # Four proper 3D coins hover in a shallow arc.
    for x, y in ((-2.65, 4.58), (-1.48, 4.85), (1.58, 4.85), (2.72, 4.58)):
        add_cylinder(gold, (x, y, -0.22), 0.27, 0.075, rotation=(math.pi / 2.0, 0.0, 0.0))
        gold_light.add_xyz(
            *torus(0.27, 0.052, 16, 5),
            (x, y, -0.275),
            rotation=(math.pi / 2.0, 0.0, 0.0),
        )

    # A shaded warp pipe anchors the right side.
    add_cylinder(green, (5.15, 1.13, 0.0), 0.64, 1.55)
    add_cylinder(green, (5.15, 1.96, 0.0), 0.84, 0.42)
    add_box(green_dark, (4.80, 1.13, -0.56), (0.15, 0.74, 0.055))
    add_box(green_dark, (4.70, 1.96, -0.73), (0.17, 0.17, 0.055))
    add_cylinder(outline, (5.15, 2.18, 0.0), 0.58, 0.035)

    # Mario: a readable running pose assembled from low-poly volumes.
    add_box(brown, (-5.60, 0.54, -0.03), (0.46, 0.22, 0.46), rotation=(0.0, 0.0, -0.07))
    add_box(brown, (-4.76, 0.55, -0.05), (0.47, 0.23, 0.48), rotation=(0.0, 0.0, 0.06))
    add_box(blue, (-5.44, 0.96, 0.0), (0.25, 0.42, 0.30), rotation=(0.0, 0.0, -0.09))
    add_box(blue, (-4.86, 1.00, 0.0), (0.25, 0.44, 0.30), rotation=(0.0, 0.0, 0.10))
    add_sphere(red, (-5.13, 1.72, 0.0), (0.66, 0.68, 0.43))
    add_box(blue, (-5.08, 1.56, -0.39), (0.46, 0.49, 0.055))
    add_box(blue, (-5.43, 1.96, -0.40), (0.08, 0.41, 0.055), rotation=(0.0, 0.0, -0.22))
    add_box(blue, (-4.76, 1.96, -0.40), (0.08, 0.41, 0.055), rotation=(0.0, 0.0, 0.22))
    add_sphere(gold, (-5.40, 1.72, -0.47), (0.075, 0.075, 0.035))
    add_sphere(gold, (-4.78, 1.72, -0.47), (0.075, 0.075, 0.035))
    add_box(red, (-5.70, 1.86, 0.0), (0.29, 0.22, 0.30), rotation=(0.0, 0.0, -0.48))
    add_box(red, (-4.50, 1.87, 0.0), (0.32, 0.22, 0.30), rotation=(0.0, 0.0, 0.42))
    add_sphere(skin, (-5.93, 1.61, 0.0), (0.27, 0.27, 0.29))
    add_sphere(skin, (-4.20, 2.10, 0.0), (0.28, 0.28, 0.30))
    add_sphere(skin, (-5.05, 2.75, 0.0), (0.55, 0.58, 0.46))
    add_sphere(skin, (-4.52, 2.78, -0.02), (0.26, 0.25, 0.27))
    add_sphere(brown, (-5.48, 2.76, 0.05), (0.22, 0.46, 0.38))
    add_sphere(brown, (-4.57, 2.58, -0.34), (0.25, 0.11, 0.075))
    add_sphere(red, (-5.12, 3.21, 0.0), (0.60, 0.28, 0.48))
    add_box(red, (-4.69, 3.06, -0.03), (0.48, 0.075, 0.50))
    add_sphere(white, (-4.77, 2.92, -0.415), (0.12, 0.15, 0.045))
    add_sphere(outline, (-4.72, 2.92, -0.468), (0.050, 0.075, 0.025))

    # A tiny Goomba gives Mario something to run toward.
    add_sphere(brown, (1.02, 1.00, -0.04), (0.72, 0.73, 0.54))
    add_box(brown, (0.48, 0.48, 0.0), (0.46, 0.18, 0.43), rotation=(0.0, 0.0, -0.08))
    add_box(brown, (1.56, 0.48, 0.0), (0.46, 0.18, 0.43), rotation=(0.0, 0.0, 0.08))
    for x in (0.77, 1.27):
        add_sphere(white, (x, 1.18, -0.50), (0.15, 0.22, 0.055))
        add_sphere(outline, (x + (0.04 if x < 1.0 else -0.04), 1.16, -0.565), (0.055, 0.105, 0.027))
    add_box(outline, (0.78, 1.42, -0.56), (0.21, 0.035, 0.026), rotation=(0.0, 0.0, 0.22))
    add_box(outline, (1.26, 1.42, -0.56), (0.21, 0.035, 0.026), rotation=(0.0, 0.0, -0.22))
    add_box(cream, (0.79, 0.87, -0.54), (0.075, 0.13, 0.035), rotation=(0.0, 0.0, -0.20))
    add_box(cream, (1.25, 0.87, -0.54), (0.075, 0.13, 0.035), rotation=(0.0, 0.0, 0.20))

    # Goal pole and a thick triangular pennant finish the miniature level.
    add_cylinder(pole, (7.55, 2.92, 0.10), 0.075, 5.18, 12)
    add_sphere(gold, (7.55, 5.59, 0.10), (0.17, 0.17, 0.17))
    flag = extruded_polygon(((0.0, 0.0), (-1.65, -0.48), (0.0, -0.96)), 0.12)
    red.add_xyz(*flag, (7.48, 5.25, 0.10))
    add_sphere(white, (6.95, 4.77, 0.025), (0.23, 0.23, 0.07))
    add_sphere(red, (6.95, 4.77, -0.055), (0.10, 0.10, 0.035))

    palette = (
        (8_001, (161, 83, 37, 255), soil),
        (8_002, (105, 50, 28, 255), soil_dark),
        (8_003, (77, 188, 70, 255), grass),
        (8_004, (30, 126, 50, 255), grass_dark),
        (8_005, (52, 190, 72, 255), green),
        (8_006, (22, 105, 43, 255), green_dark),
        (8_007, (200, 76, 44, 255), brick),
        (8_008, (49, 38, 34, 255), outline),
        (8_009, (247, 179, 35, 255), gold),
        (8_010, (255, 232, 92, 255), gold_light),
        (8_011, (220, 45, 39, 255), red),
        (8_012, (37, 91, 181, 255), blue),
        (8_013, (246, 174, 124, 255), skin),
        (8_014, (114, 63, 37, 255), brown),
        (8_015, (255, 241, 190, 255), cream),
        (8_016, (250, 250, 238, 255), white),
        (8_017, (190, 219, 232, 255), cloud_shadow),
        (8_018, (224, 229, 219, 255), pole),
    )
    return tuple(entry for entry in palette if entry[2].faces)


def validate_scene(scene):
    total_vertices = 0
    total_triangles = 0
    for mesh_id, _color, mesh in scene:
        triangles = sum(len(face) - 2 for face in mesh.faces)
        if len(mesh.vertices) > 1_000 or triangles > 2_000:
            raise RuntimeError(
                f"mesh {mesh_id} exceeds draw3d budget: "
                f"{len(mesh.vertices)} vertices/{triangles} triangles"
            )
        total_vertices += len(mesh.vertices)
        total_triangles += triangles
    return total_vertices, total_triangles


def populate(client, orbit_speed=0.16):
    scene = build_scene()
    total_vertices, total_triangles = validate_scene(scene)

    client.stop()
    client.clear()
    client.camera((0.0, 3.0, -17.0), (0.0, 2.35, 0.0), 46.0)
    for mesh_id, color, mesh in scene:
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(80_000 + mesh_id, mesh_id, IDENTITY, UNIT_SCALE)
    client.start(BACKGROUND)
    if orbit_speed != 0.0:
        client.camera(
            (0.0, 3.0, -17.0),
            (0.0, 2.35, 0.0),
            46.0,
            orbit_scale=(17.0, 12.0),
            orbit_rotation=(math.radians(-6.0), math.radians(90.0), 0.0),
            orbit_speed=orbit_speed,
        )
    return len(scene), total_vertices, total_triangles


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=1.5)
    parser.add_argument("--orbit-speed", type=float, default=0.16)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("bld/draw3d-captures/super-mario-scene.png"),
    )
    args = parser.parse_args()

    client = Draw3dClient(args.host)
    try:
        mesh_count, built_vertices, built_triangles = populate(client, args.orbit_speed)
        time.sleep(args.settle)
        output, image_format, width, height, image = client.render(args.output)
        stats = client.stats()
        print(
            f"scene meshes={mesh_count} built_vertices={built_vertices} "
            f"built_triangles={built_triangles} service_stats={stats}"
        )
        print(
            f"capture format={image_format} size={width}x{height} bytes={len(image)} "
            f"sha256={hashlib.sha256(image).hexdigest()} path={output}"
        )
        if image_format != 2 or width <= 0 or height <= 0:
            raise RuntimeError("Super Mario scene did not return a live PNG target")
    finally:
        client.close()


if __name__ == "__main__":
    main()
