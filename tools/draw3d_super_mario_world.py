#!/usr/bin/env python3
"""Build a colorful, orbit-readable Super Mario world through draw3d TCP."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh, add_box, radial_frustum, torus, triangular_prism
from draw3d_house_demo import Draw3dClient
from draw3d_super_mario_scene import extruded_polygon, uv_sphere


IDENTITY = (0.0, 0.0, 0.0)
UNIT_SCALE = (1.0, 1.0, 1.0)
WORLD_SPHERE = uv_sphere(8, 5)


def add_sphere(mesh, location, scale, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(*WORLD_SPHERE, location, scale, rotation)


def add_cylinder(mesh, location, radius, height, segments=12, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(
        *radial_frustum(segments),
        location,
        (radius, height * 0.5, radius),
        rotation,
    )


def add_cone(mesh, location, radius, height, segments=12, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(
        *radial_frustum(segments, bottom_radius=1.0, top_radius=0.0, height=2.0),
        location,
        (radius, height * 0.5, radius),
        rotation,
    )


def ribbon(points, width, y):
    """Create an XZ strip with simple mitered joints."""
    vertices = []
    half_width = width * 0.5
    for index, (x, z) in enumerate(points):
        previous = points[max(0, index - 1)]
        following = points[min(len(points) - 1, index + 1)]
        tangent_x = following[0] - previous[0]
        tangent_z = following[1] - previous[1]
        length = math.hypot(tangent_x, tangent_z) or 1.0
        normal_x, normal_z = -tangent_z / length, tangent_x / length
        vertices.append((x + normal_x * half_width, y, z + normal_z * half_width))
        vertices.append((x - normal_x * half_width, y, z - normal_z * half_width))
    faces = tuple(
        (step * 2, step * 2 + 1, step * 2 + 3, step * 2 + 2)
        for step in range(len(points) - 1)
    )
    return tuple(vertices), faces


def add_question_mark(mesh, x, y, z, outward=1.0):
    depth = 0.035
    for location, scale in (
        ((x - 0.10, y + 0.20, z), (0.18, 0.065, depth)),
        ((x + 0.095, y + 0.105, z), (0.065, 0.15, depth)),
        ((x, y - 0.035, z), (0.105, 0.060, depth)),
        ((x - 0.065, y - 0.145, z), (0.050, 0.085, depth)),
        ((x - 0.065, y - 0.335, z), (0.060, 0.060, depth)),
    ):
        add_box(mesh, location, scale)


def add_coin(gold, highlight, location, radius=0.30):
    x, y, z = location
    add_cylinder(gold, (x, y, z), radius, 0.09, 12, (math.pi / 2.0, 0.0, 0.0))
    highlight.add_xyz(
        *torus(radius, radius * 0.18, 14, 4),
        (x, y, z - 0.055),
        rotation=(math.pi / 2.0, 0.0, 0.0),
    )


def add_tree(trunks, crowns_dark, crowns_light, x, z, ground, scale, yaw=0.0):
    add_cylinder(trunks, (x, ground + scale * 0.78, z), scale * 0.17, scale * 1.56, 8)
    add_cone(crowns_dark, (x, ground + scale * 1.76, z), scale * 0.68, scale * 1.55, 9, (0.0, yaw, 0.0))
    add_cone(
        crowns_light,
        (x + 0.06, ground + scale * 2.34, z - 0.03),
        scale * 0.50,
        scale * 1.15,
        8,
        (0.0, yaw + 0.12, 0.0),
    )


def build_world():
    meshes = {name: GardenMesh() for name in (
        "soil", "cliff", "grass", "grass_edge", "sand", "water", "foam", "stone",
        "hill", "hill_dark", "trunks", "crowns", "crowns_light", "cloud", "cloud_shadow",
        "castle_white", "castle_pink", "castle_red", "castle_dark", "metal",
        "house", "house_dark", "house_cream", "pipe", "pipe_dark", "brick", "block",
        "block_light", "outline", "wood", "gold", "gold_light", "flora_red", "flora_white",
        "mario_red", "mario_blue", "skin", "brown", "character_white", "black",
        "yoshi", "yoshi_light", "yellow", "orange",
    )}
    m = meshes

    # --- Phase 0: a floating overworld with real depth and multiple elevations.
    m["soil"].add_xyz(
        *radial_frustum(32, bottom_radius=0.80, top_radius=1.0),
        (0.0, 0.0, 0.0),
        (13.2, 1.05, 10.2),
    )
    m["grass"].add_xyz(
        *radial_frustum(32, bottom_radius=0.96, top_radius=1.0),
        (0.0, 1.14, 0.0),
        (13.45, 0.12, 10.45),
    )
    m["grass_edge"].add_xyz(*torus(1.0, 0.028, 48, 4), (0.0, 1.27, 0.0), (13.35, 1.0, 10.35))

    # Castle and block districts rise from the main island.
    for location, scale in (
        ((0.0, 1.52, 5.10), (4.1, 0.48, 3.05)),
        ((6.25, 1.40, -1.25), (2.25, 0.36, 2.15)),
        ((-7.25, 1.38, 2.45), (2.55, 0.34, 2.25)),
    ):
        m["soil"].add_xyz(
            *radial_frustum(20, bottom_radius=0.88, top_radius=1.0),
            location,
            scale,
        )
        m["grass"].add_xyz(
            *radial_frustum(20, bottom_radius=0.97, top_radius=1.0),
            (location[0], location[1] + scale[1] + 0.10, location[2]),
            (scale[0] * 1.03, 0.10, scale[2] * 1.03),
        )

    # Faceted roots make the island silhouette read as a suspended world.
    for x, y, z, sx, sy in (
        (-9.4, -1.45, -2.7, 0.72, 1.55),
        (-5.5, -1.72, 5.9, 0.58, 1.85),
        (0.0, -1.85, -7.5, 0.75, 2.0),
        (5.8, -1.62, 5.1, 0.63, 1.75),
        (9.8, -1.32, -2.2, 0.68, 1.45),
    ):
        add_cone(m["cliff"], (x, y, z), sx, sy, 9)

    river_path = ((-12.0, 5.0), (-8.6, 3.8), (-5.0, 2.0), (-1.7, 0.2), (1.6, -1.0), (5.0, -2.6), (9.0, -5.3), (12.2, -6.2))
    m["sand"].add_xyz(*ribbon(river_path, 2.10, 1.285), IDENTITY)
    m["water"].add_xyz(*ribbon(river_path, 1.48, 1.315), IDENTITY)
    m["foam"].add_xyz(*ribbon(river_path[1:-1], 0.12, 1.338), (0.12, 0.0, -0.03))

    path = ((-7.2, -7.0), (-5.4, -4.8), (-3.2, -3.0), (-1.4, -1.7), (0.1, 0.2), (0.0, 2.4), (0.0, 4.0))
    m["sand"].add_xyz(*ribbon(path, 1.18, 1.345), IDENTITY)
    for x, z in path[1:-1]:
        add_cylinder(m["gold"], (x, 1.39, z), 0.18, 0.055, 10)

    # Stone stepping islands and a plank bridge cross the water.
    for x, z, scale, yaw in (
        (-3.2, 1.05, (0.52, 0.12, 0.42), 0.12),
        (-2.55, 0.70, (0.48, 0.14, 0.38), -0.10),
        (-1.90, 0.32, (0.50, 0.13, 0.40), 0.08),
    ):
        add_sphere(m["stone"], (x, 1.40, z), scale, (0.0, yaw, 0.0))
    for step in range(7):
        add_box(m["wood"], (1.00 + step * 0.42, 1.55, -0.62 - step * 0.20), (0.18, 0.07, 0.72), (0.0, -0.45, 0.0))
    add_box(m["wood"], (2.18, 1.44, -1.22), (1.72, 0.055, 0.07), (0.0, -0.45, 0.0))

    # --- Phase 1: hills, trees, clouds, and small color landmarks.
    for location, scale in (
        ((-9.2, 2.35, 6.1), (2.35, 1.70, 1.15)),
        ((-3.7, 2.05, 7.9), (1.80, 1.35, 0.92)),
        ((5.5, 2.20, 7.2), (2.10, 1.55, 1.02)),
        ((9.3, 1.95, 4.4), (1.65, 1.25, 0.85)),
    ):
        add_sphere(m["hill"], location, scale)
    for x, y, z in ((-9.7, 2.60, 5.03), (-8.6, 2.05, 5.02), (-4.0, 2.16, 7.02), (5.1, 2.48, 6.24), (6.0, 1.98, 6.20), (9.4, 2.05, 3.57)):
        add_sphere(m["hill_dark"], (x, y, z), (0.19, 0.30, 0.07))

    for x, z, scale, yaw in (
        (-11.0, -4.2, 1.35, 0.10), (-9.8, -7.0, 1.10, -0.12), (-6.8, 6.4, 1.18, 0.18),
        (-4.9, 8.3, 1.02, -0.18), (4.0, 8.1, 1.20, 0.12), (7.1, 6.2, 1.05, -0.20),
        (10.7, 3.0, 1.28, 0.16), (11.0, -1.0, 1.02, -0.10), (8.6, -7.1, 1.12, 0.20),
        (-1.0, -8.7, 1.06, -0.16),
    ):
        add_tree(m["trunks"], m["crowns"], m["crowns_light"], x, z, 1.27, scale, yaw)

    for x, y, z, scale in (
        (-8.1, 8.15, 7.8, (1.40, 0.52, 0.58)), (-9.2, 8.02, 7.8, (0.82, 0.68, 0.50)), (-7.0, 8.03, 7.8, (0.90, 0.70, 0.52)),
        (3.9, 9.15, 8.3, (1.35, 0.50, 0.56)), (2.8, 9.05, 8.3, (0.78, 0.64, 0.48)), (5.0, 9.02, 8.3, (0.86, 0.67, 0.50)),
        (9.0, 7.40, 1.2, (1.05, 0.42, 0.50)), (8.2, 7.34, 1.2, (0.68, 0.55, 0.43)), (9.8, 7.32, 1.2, (0.70, 0.57, 0.44)),
    ):
        add_sphere(m["cloud"], (x, y, z), scale)
    for x, y, z, sx in ((-8.1, 7.84, 7.76, 1.55), (3.9, 8.84, 8.26, 1.48), (9.0, 7.13, 1.16, 1.16)):
        add_sphere(m["cloud_shadow"], (x, y, z), (sx, 0.20, 0.47))

    for index, (x, z) in enumerate(((-9.0, -2.0), (-7.8, -1.1), (-5.8, 4.5), (3.4, 3.1), (7.8, 1.8), (8.8, -3.7), (1.8, -7.0))):
        target = m["flora_red"] if index % 2 == 0 else m["gold"]
        add_cylinder(m["grass_edge"], (x, 1.48, z), 0.035, 0.32, 6)
        add_sphere(target, (x, 1.72, z), (0.13, 0.08, 0.13))

    # --- Phase 2: Peach-style castle, ghost house, pipes, and airborne blocks.
    castle_ground = 2.10
    add_box(m["castle_white"], (0.0, castle_ground + 1.35, 5.20), (2.15, 1.35, 1.35))
    add_box(m["castle_pink"], (0.0, castle_ground + 2.00, 5.20), (1.50, 0.24, 1.42))
    for x in (-2.25, 2.25):
        add_cylinder(m["castle_white"], (x, castle_ground + 1.55, 5.20), 0.82, 3.10, 12)
        add_cone(m["castle_red"], (x, castle_ground + 3.55, 5.20), 1.08, 1.15, 12)
    add_cylinder(m["castle_white"], (0.0, castle_ground + 2.45, 5.20), 0.92, 2.45, 12)
    add_cone(m["castle_red"], (0.0, castle_ground + 4.10, 5.20), 1.20, 1.38, 12)
    for x in (-1.80, -0.90, 0.0, 0.90, 1.80):
        add_box(m["castle_white"], (x, castle_ground + 2.95, 4.02), (0.27, 0.28, 0.28))
    add_box(m["castle_dark"], (0.0, castle_ground + 0.75, 3.82), (0.52, 0.78, 0.08))
    add_sphere(m["castle_dark"], (0.0, castle_ground + 1.46, 3.82), (0.52, 0.42, 0.08))
    for x, y in ((-1.15, castle_ground + 1.65), (1.15, castle_ground + 1.65), (0.0, castle_ground + 2.85)):
        add_box(m["castle_dark"], (x, y, 3.80), (0.20, 0.31, 0.07))
    for x in (-2.25, 0.0, 2.25):
        add_cylinder(m["metal"], (x, castle_ground + 4.55, 5.20), 0.035, 1.10, 8)
    for x, y in ((-2.25, castle_ground + 5.00), (0.0, castle_ground + 5.10), (2.25, castle_ground + 5.00)):
        flag = extruded_polygon(((0.0, 0.0), (0.62, -0.24), (0.0, -0.48)), 0.08)
        m["castle_pink"].add_xyz(*flag, (x, y, 5.20))
    add_sphere(m["gold"], (0.0, castle_ground + 3.12, 3.70), (0.25, 0.25, 0.08))
    add_sphere(m["castle_pink"], (0.0, castle_ground + 3.12, 3.60), (0.12, 0.12, 0.05))

    # Ghost house and orbit-visible Boo.
    add_box(m["house"], (-7.20, 3.05, 2.45), (1.65, 1.35, 1.15))
    m["house_dark"].add_xyz(*triangular_prism(), (-7.20, 4.38, 2.45), (1.95, 0.88, 1.42))
    add_box(m["house_dark"], (-7.20, 2.65, 1.27), (0.48, 0.93, 0.08))
    for x in (-8.20, -6.20):
        add_box(m["house_cream"], (x, 3.35, 1.26), (0.32, 0.42, 0.07))
        add_box(m["house_dark"], (x, 3.35, 1.18), (0.055, 0.42, 0.04))
        add_box(m["house_dark"], (x, 3.35, 1.18), (0.32, 0.055, 0.04))
    add_sphere(m["character_white"], (-8.75, 4.75, 0.55), (0.72, 0.62, 0.52))
    add_cone(m["character_white"], (-9.22, 4.68, 0.58), 0.32, 0.75, 7, (0.0, 0.0, math.pi / 2.0))
    for x in (-8.98, -8.60):
        add_sphere(m["black"], (x, 4.88, 0.02), (0.09, 0.15, 0.045))
    add_box(m["flora_red"], (-8.62, 4.50, 0.01), (0.22, 0.10, 0.045))

    # Two pipes make the island read from multiple orbit angles.
    for x, z, ground, radius, height in ((5.85, -2.10, 1.95, 0.72, 1.62), (-9.45, -4.90, 1.27, 0.58, 1.18)):
        add_cylinder(m["pipe"], (x, ground + height * 0.5, z), radius, height, 14)
        add_cylinder(m["pipe"], (x, ground + height + 0.18, z), radius * 1.30, 0.38, 14)
        add_box(m["pipe_dark"], (x - radius * 0.44, ground + height * 0.52, z - radius * 0.78), (radius * 0.18, height * 0.46, 0.05))
        add_cylinder(m["outline"], (x, ground + height + 0.39, z), radius * 0.86, 0.035, 14)

    block_centers = ((-3.0, 4.10, -2.35), (-1.85, 4.10, -2.35), (-0.70, 4.10, -2.35), (3.20, 5.15, 0.20), (4.35, 5.15, 0.20))
    for index, (x, y, z) in enumerate(block_centers):
        target = m["block"] if index in (1, 3) else m["brick"]
        add_box(target, (x, y, z), (0.53, 0.53, 0.53), (0.0, 0.16, 0.0))
        if index in (1, 3):
            add_box(m["block_light"], (x, y, z - 0.59), (0.43, 0.43, 0.025), (0.0, 0.16, 0.0))
            add_box(m["block_light"], (x, y, z + 0.59), (0.43, 0.43, 0.025), (0.0, 0.16, 0.0))
            add_question_mark(m["outline"], x, y, z - 0.64)
            add_question_mark(m["outline"], x, y, z + 0.64, -1.0)
        else:
            add_box(m["outline"], (x, y, z - 0.59), (0.45, 0.035, 0.025))
            add_box(m["outline"], (x, y, z + 0.59), (0.45, 0.035, 0.025))

    # --- Phase 3: coins, Piranha Plant, mushrooms, and the goal landmark.
    for location in (
        (-5.7, 3.05, -5.0), (-4.9, 3.55, -4.5), (-4.0, 3.85, -3.9),
        (-3.0, 5.35, -2.35), (-1.85, 5.65, -2.35), (-0.70, 5.35, -2.35),
        (3.2, 6.38, 0.20), (4.35, 6.38, 0.20),
    ):
        add_coin(m["gold"], m["gold_light"], location, 0.29)

    # Piranha Plant rises above the large pipe.
    add_cylinder(m["pipe"], (5.85, 4.25, -2.10), 0.13, 1.40, 9)
    add_sphere(m["flora_red"], (5.85, 5.05, -2.10), (0.66, 0.62, 0.55))
    add_box(m["outline"], (5.85, 4.91, -2.67), (0.48, 0.13, 0.045))
    for x, y in ((5.58, 5.23), (6.12, 5.12), (5.82, 5.47)):
        add_sphere(m["flora_white"], (x, y, -2.61), (0.10, 0.10, 0.04))
    for side in (-1.0, 1.0):
        add_sphere(m["pipe"], (5.85 + side * 0.34, 4.20, -2.10), (0.40, 0.13, 0.20), (0.0, 0.0, side * 0.45))

    # Super mushrooms beside the path.
    for x, z, size in ((-6.4, -2.1, 0.42), (7.7, 2.6, 0.34)):
        add_cylinder(m["flora_white"], (x, 1.55, z), size * 0.30, 0.48, 8)
        add_sphere(m["flora_red"], (x, 1.90, z), (size, size * 0.48, size))
        add_sphere(m["flora_white"], (x - size * 0.18, 1.96, z - size * 0.37), (size * 0.10, size * 0.09, size * 0.055))
        add_sphere(m["flora_white"], (x + size * 0.22, 1.90, z - size * 0.39), (size * 0.11, size * 0.10, size * 0.055))

    add_cylinder(m["metal"], (9.55, 4.05, -4.65), 0.065, 5.55, 10)
    add_sphere(m["gold"], (9.55, 6.88, -4.65), (0.16, 0.16, 0.16))
    flag = extruded_polygon(((0.0, 0.0), (-1.55, -0.46), (0.0, -0.92)), 0.11)
    m["castle_red"].add_xyz(*flag, (9.49, 6.56, -4.65))
    add_sphere(m["character_white"], (8.98, 6.10, -4.72), (0.22, 0.22, 0.07))
    add_sphere(m["castle_red"], (8.98, 6.10, -4.80), (0.095, 0.095, 0.035))

    # --- Phase 4: Mario, Yoshi, Goombas, and a Koopa populate the world.
    mx, mz, gy = -5.55, -5.25, 1.27
    add_box(m["brown"], (mx - 0.42, gy + 0.23, mz), (0.42, 0.20, 0.42), (0.0, 0.0, -0.08))
    add_box(m["brown"], (mx + 0.40, gy + 0.24, mz), (0.43, 0.21, 0.44), (0.0, 0.0, 0.07))
    add_box(m["mario_blue"], (mx - 0.27, gy + 0.66, mz), (0.23, 0.40, 0.28), (0.0, 0.0, -0.10))
    add_box(m["mario_blue"], (mx + 0.29, gy + 0.70, mz), (0.23, 0.42, 0.28), (0.0, 0.0, 0.10))
    add_sphere(m["mario_red"], (mx, gy + 1.38, mz), (0.62, 0.64, 0.42))
    add_box(m["mario_blue"], (mx, gy + 1.22, mz - 0.40), (0.44, 0.47, 0.055))
    add_box(m["mario_blue"], (mx - 0.32, gy + 1.62, mz - 0.40), (0.075, 0.38, 0.055), (0.0, 0.0, -0.20))
    add_box(m["mario_blue"], (mx + 0.32, gy + 1.62, mz - 0.40), (0.075, 0.38, 0.055), (0.0, 0.0, 0.20))
    add_sphere(m["gold"], (mx - 0.30, gy + 1.39, mz - 0.47), (0.07, 0.07, 0.03))
    add_sphere(m["gold"], (mx + 0.30, gy + 1.39, mz - 0.47), (0.07, 0.07, 0.03))
    add_box(m["mario_red"], (mx - 0.58, gy + 1.48, mz), (0.28, 0.21, 0.28), (0.0, 0.0, -0.45))
    add_box(m["mario_red"], (mx + 0.58, gy + 1.49, mz), (0.30, 0.21, 0.28), (0.0, 0.0, 0.42))
    add_sphere(m["skin"], (mx - 0.82, gy + 1.24, mz), (0.25, 0.25, 0.27))
    add_sphere(m["skin"], (mx + 0.85, gy + 1.72, mz), (0.26, 0.26, 0.28))
    add_sphere(m["skin"], (mx + 0.03, gy + 2.37, mz), (0.52, 0.55, 0.44))
    add_sphere(m["skin"], (mx + 0.54, gy + 2.39, mz - 0.02), (0.24, 0.23, 0.25))
    add_sphere(m["brown"], (mx - 0.38, gy + 2.36, mz + 0.05), (0.20, 0.43, 0.36))
    add_sphere(m["brown"], (mx + 0.48, gy + 2.18, mz - 0.34), (0.23, 0.10, 0.07))
    add_sphere(m["mario_red"], (mx - 0.02, gy + 2.80, mz), (0.57, 0.26, 0.45))
    add_box(m["mario_red"], (mx + 0.39, gy + 2.66, mz), (0.45, 0.07, 0.47))
    add_sphere(m["character_white"], (mx + 0.30, gy + 2.52, mz - 0.41), (0.11, 0.14, 0.04))
    add_sphere(m["black"], (mx + 0.34, gy + 2.52, mz - 0.465), (0.045, 0.07, 0.02))

    # Yoshi's long head, white belly, saddle, dorsal plates, and orange shoes.
    yx, yz = -3.45, -4.15
    add_sphere(m["yoshi"], (yx, 2.10, yz), (0.58, 0.78, 0.48))
    add_sphere(m["yoshi_light"], (yx + 0.06, 2.00, yz - 0.45), (0.38, 0.56, 0.07))
    add_sphere(m["yoshi"], (yx + 0.16, 3.00, yz), (0.58, 0.60, 0.50))
    add_sphere(m["yoshi_light"], (yx + 0.65, 2.95, yz - 0.02), (0.52, 0.35, 0.42))
    add_sphere(m["character_white"], (yx + 0.18, 3.30, yz - 0.40), (0.16, 0.24, 0.07))
    add_sphere(m["black"], (yx + 0.22, 3.32, yz - 0.48), (0.055, 0.10, 0.025))
    add_sphere(m["mario_red"], (yx - 0.42, 2.32, yz), (0.38, 0.16, 0.40))
    for y in (2.25, 2.58, 2.90):
        add_cone(m["mario_red"], (yx - 0.54, y, yz + 0.08), 0.14, 0.32, 6, (0.0, 0.0, -math.pi / 2.0))
    add_box(m["orange"], (yx - 0.35, 1.46, yz - 0.05), (0.36, 0.18, 0.38))
    add_box(m["orange"], (yx + 0.36, 1.46, yz - 0.05), (0.36, 0.18, 0.38))
    add_cone(m["yoshi"], (yx - 0.58, 2.10, yz + 0.08), 0.28, 0.80, 8, (0.0, 0.0, -math.pi / 2.0))

    # Two Goombas face the initial camera, while a green Koopa guards the bridge.
    for gx, gz, ground, scale in ((2.75, -4.25, 1.27, 0.88), (8.35, 0.35, 1.27, 0.72)):
        add_sphere(m["brown"], (gx, ground + scale * 0.72, gz), (scale * 0.68, scale * 0.72, scale * 0.52))
        add_box(m["brown"], (gx - scale * 0.48, ground + 0.16, gz), (scale * 0.40, 0.16, scale * 0.39), (0.0, 0.0, -0.07))
        add_box(m["brown"], (gx + scale * 0.48, ground + 0.16, gz), (scale * 0.40, 0.16, scale * 0.39), (0.0, 0.0, 0.07))
        for ex in (gx - scale * 0.22, gx + scale * 0.22):
            add_sphere(m["character_white"], (ex, ground + scale * 0.90, gz - scale * 0.50), (scale * 0.13, scale * 0.20, 0.045))
            add_sphere(m["black"], (ex, ground + scale * 0.89, gz - scale * 0.56), (scale * 0.05, scale * 0.095, 0.023))
    kx, kz = 1.0, -2.85
    add_sphere(m["yellow"], (kx, 1.95, kz), (0.42, 0.62, 0.38))
    add_sphere(m["pipe"], (kx - 0.18, 2.20, kz + 0.24), (0.55, 0.64, 0.28))
    add_sphere(m["pipe_dark"], (kx - 0.20, 2.20, kz + 0.52), (0.32, 0.40, 0.055))
    add_sphere(m["yellow"], (kx + 0.25, 2.72, kz), (0.34, 0.42, 0.32))
    add_sphere(m["character_white"], (kx + 0.38, 2.83, kz - 0.28), (0.085, 0.13, 0.045))
    add_sphere(m["black"], (kx + 0.40, 2.83, kz - 0.33), (0.035, 0.065, 0.022))
    for dx in (-0.23, 0.25):
        add_box(m["orange"], (kx + dx, 1.42, kz), (0.28, 0.15, 0.32))

    specs = (
        (0, 8_300, (158, 82, 39, 255), "soil"), (0, 8_301, (84, 51, 42, 255), "cliff"),
        (0, 8_302, (75, 185, 67, 255), "grass"), (0, 8_303, (26, 118, 48, 255), "grass_edge"),
        (0, 8_304, (218, 183, 93, 255), "sand"), (0, 8_305, (42, 143, 210, 255), "water"),
        (0, 8_306, (174, 229, 244, 255), "foam"), (0, 8_307, (112, 119, 121, 255), "stone"),
        (1, 8_310, (85, 194, 82, 255), "hill"), (1, 8_311, (33, 130, 57, 255), "hill_dark"),
        (1, 8_312, (103, 63, 37, 255), "trunks"), (1, 8_313, (22, 112, 57, 255), "crowns"),
        (1, 8_314, (51, 157, 72, 255), "crowns_light"), (1, 8_315, (251, 250, 235, 255), "cloud"),
        (1, 8_316, (185, 217, 232, 255), "cloud_shadow"),
        (2, 8_320, (244, 232, 198, 255), "castle_white"), (2, 8_321, (241, 121, 151, 255), "castle_pink"),
        (2, 8_322, (211, 47, 43, 255), "castle_red"), (2, 8_323, (61, 42, 47, 255), "castle_dark"),
        (2, 8_324, (223, 229, 220, 255), "metal"), (2, 8_325, (132, 82, 46, 255), "house"),
        (2, 8_326, (62, 42, 42, 255), "house_dark"), (2, 8_327, (236, 208, 137, 255), "house_cream"),
        (2, 8_328, (43, 184, 70, 255), "pipe"), (2, 8_329, (18, 94, 40, 255), "pipe_dark"),
        (2, 8_330, (194, 72, 43, 255), "brick"), (2, 8_331, (245, 176, 37, 255), "block"),
        (2, 8_332, (255, 232, 93, 255), "block_light"), (2, 8_333, (48, 37, 34, 255), "outline"),
        (2, 8_334, (119, 70, 37, 255), "wood"),
        (3, 8_340, (247, 181, 39, 255), "gold"), (3, 8_341, (255, 235, 104, 255), "gold_light"),
        (3, 8_342, (220, 48, 45, 255), "flora_red"), (3, 8_343, (252, 247, 224, 255), "flora_white"),
        (4, 8_350, (218, 43, 38, 255), "mario_red"), (4, 8_351, (39, 88, 178, 255), "mario_blue"),
        (4, 8_352, (246, 174, 123, 255), "skin"), (4, 8_353, (112, 62, 38, 255), "brown"),
        (4, 8_354, (250, 248, 225, 255), "character_white"), (4, 8_355, (32, 31, 31, 255), "black"),
        (4, 8_356, (55, 183, 75, 255), "yoshi"), (4, 8_357, (230, 239, 187, 255), "yoshi_light"),
        (4, 8_358, (235, 190, 58, 255), "yellow"), (4, 8_359, (219, 119, 41, 255), "orange"),
    )
    return tuple((phase, mesh_id, color, name, m[name]) for phase, mesh_id, color, name in specs if m[name].faces)


def validate_world(layers):
    total_vertices = total_triangles = 0
    for _phase, mesh_id, _color, name, mesh in layers:
        triangles = sum(len(face) - 2 for face in mesh.faces)
        if len(mesh.vertices) > 1_000 or triangles > 2_000:
            raise RuntimeError(
                f"{name}/{mesh_id} exceeds draw3d budget: "
                f"{len(mesh.vertices)} vertices/{triangles} triangles"
            )
        total_vertices += len(mesh.vertices)
        total_triangles += triangles
    return total_vertices, total_triangles


def set_orbit(client, speed):
    target = (0.0, 5.0, 0.0)
    client.camera(
        (24.0, 12.0, -24.0),
        target,
        46.0,
        orbit_scale=(27.0, 24.0),
        orbit_rotation=(0.0, math.radians(45.0), math.radians(15.0)),
        orbit_speed=speed,
    )


def populate_live(client, orbit_speed=0.045, phase_delay=0.30):
    layers = build_world()
    vertices, triangles = validate_world(layers)

    # Clear while running, then establish the moving camera before the first mesh.
    # Each phase becomes visible as its instances arrive.
    client.clear()
    client.start()
    set_orbit(client, orbit_speed)

    phase_counts = []
    for phase in sorted({entry[0] for entry in layers}):
        count = 0
        for _phase, mesh_id, color, _name, mesh in layers:
            if _phase != phase:
                continue
            client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
            client.instance(90_000 + mesh_id, mesh_id, IDENTITY, UNIT_SCALE)
            count += 1
        phase_counts.append(count)
        if phase_delay > 0.0:
            time.sleep(phase_delay)
    return len(layers), vertices, triangles, tuple(phase_counts)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--orbit-speed", type=float, default=0.045)
    parser.add_argument("--phase-delay", type=float, default=0.30)
    parser.add_argument("--settle", type=float, default=1.2)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("bld/draw3d-captures/super-mario-world.png"),
    )
    args = parser.parse_args()

    client = Draw3dClient(args.host)
    try:
        mesh_count, vertices, triangles, phase_counts = populate_live(
            client,
            args.orbit_speed,
            args.phase_delay,
        )
        time.sleep(args.settle)
        output, image_format, width, height, image = client.render(args.output)
        stats = client.stats()
        print(
            f"world meshes={mesh_count} vertices={vertices} triangles={triangles} "
            f"phases={phase_counts} service_stats={stats}"
        )
        print(
            f"capture format={image_format} size={width}x{height} bytes={len(image)} "
            f"sha256={hashlib.sha256(image).hexdigest()} path={output}"
        )
        if image_format != 2 or width <= 0 or height <= 0:
            raise RuntimeError("Super Mario world did not return a live PNG target")
    finally:
        client.close()


if __name__ == "__main__":
    main()
