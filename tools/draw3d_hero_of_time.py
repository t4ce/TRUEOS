#!/usr/bin/env python3
"""Present a low-poly Hero of Time equipment diorama through draw3d TCP."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import (
    GardenMesh,
    add_box,
    add_octahedron,
    radial_frustum,
    torus,
    triangular_prism,
)
from draw3d_house_demo import Draw3dClient
from draw3d_poly_trees import place_tree, upload_tree_kit


IDENTITY_LOCATION = (0.0, 0.0, 0.0)
IDENTITY_SCALE = (1.0, 1.0, 1.0)
HERO_GROUP_LOCATION = (1.65, 0.0, 0.0)
ENVIRONMENT_MESH_IDS = frozenset((1101, 1103, 1104, 1128, 1130, 1131, 1132, 1133)) | frozenset(
    range(1148, 1161)
)

VIEWS = {
    "shield": ((-9.2, 7.1, 19.0), (-0.30, 4.10, -1.15), 46.0),
    "sword": ((11.6, 8.3, 17.5), (0.30, 4.35, -1.05), 47.0),
    "hero": ((7.8, 7.2, 19.2), (0.0, 4.05, -1.10), 46.0),
    "forest": ((5.2, 7.0, 21.0), (-0.30, 4.20, -1.65), 48.0),
}


def extruded_polygon(points, depth):
    half_depth = depth * 0.5
    vertices = [(x, y, -half_depth) for x, y in points]
    vertices.extend((x, y, half_depth) for x, y in points)
    count = len(points)
    faces = [tuple(range(count - 1, -1, -1)), tuple(range(count, count * 2))]
    for index in range(count):
        nxt = (index + 1) % count
        faces.append((index, nxt, count + nxt, count + index))
    return tuple(vertices), tuple(faces)


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


def add_triangle(mesh, points, location, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(tuple((x, y, z) for x, y, z in points), ((0, 1, 2),), location, rotation=rotation)


def offset(location, vector, amount):
    return tuple(location[index] + vector[index] * amount for index in range(3))


def rotate_z(point, angle):
    x, y, z = point
    sine, cosine = math.sin(angle), math.cos(angle)
    return (x * cosine - y * sine, x * sine + y * cosine, z)


def terrain_ribbon(points, width, y):
    """Build a flat, jointed XZ ribbon for rivers, banks, and current lines."""
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
    faces = tuple((index * 2, index * 2 + 1, index * 2 + 3, index * 2 + 2) for index in range(len(points) - 1))
    return tuple(vertices), faces


def shield_geometry():
    return extruded_polygon(
        (
            (-0.86, 0.86),
            (-0.58, 1.10),
            (0.58, 1.10),
            (0.86, 0.86),
            (0.72, -0.42),
            (0.0, -1.16),
            (-0.72, -0.42),
        ),
        0.16,
    )


def build_scene():
    # Distant sacred geometry and a moonlit Sacred Forest Meadow stage.
    triforce = GardenMesh()
    triangle = ((-0.82, -0.62, 0.0), (0.82, -0.62, 0.0), (0.0, 0.78, 0.0))
    for location in ((0.0, 8.72, -6.8), (-0.86, 7.28, -6.8), (0.86, 7.28, -6.8)):
        add_triangle(triforce, triangle, location)
    for x, y, size in ((-6.0, 7.8, 0.10), (-3.8, 9.0, 0.07), (4.4, 8.6, 0.08), (6.4, 6.9, 0.10), (-5.0, 5.7, 0.06)):
        add_octahedron(triforce, (x, y, -6.5), (size, size * 1.8, size))

    sacred_halo = GardenMesh()
    sacred_halo.add_xyz(*torus(1.82, 0.045, 36, 5), (0.0, 8.02, -6.95), rotation=(math.pi / 2, 0.0, 0.0))

    ground = GardenMesh()
    ground.add_xyz(
        *radial_frustum(24, bottom_radius=0.86, top_radius=1.0, height=2.0),
        (0.0, -0.46, 0.0),
        (5.5, 0.42, 4.7),
    )
    for x, z, scale in (
        (-4.7, 1.0, (0.42, 0.65, 0.46)),
        (-2.7, 3.6, (0.32, 0.50, 0.36)),
        (3.3, 3.3, (0.38, 0.58, 0.40)),
        (4.9, -0.7, (0.36, 0.56, 0.39)),
    ):
        add_octahedron(ground, (x, -0.82, z), scale)

    # A river emerges beneath the Deku Tree and bends toward the foreground.
    stream_path = (
        (-3.15, -4.35),
        (-3.48, -3.20),
        (-4.08, -2.10),
        (-4.34, -0.88),
        (-4.02, 0.38),
        (-3.48, 1.48),
        (-2.72, 2.58),
        (-1.76, 3.72),
    )
    river_bank = GardenMesh()
    river_bank.add_xyz(*terrain_ribbon(stream_path, 1.62, 0.015), (0.0, 0.0, 0.0))

    river = GardenMesh()
    river.add_xyz(*terrain_ribbon(stream_path, 1.14, 0.040), (0.0, 0.0, 0.0))

    river_current = GardenMesh()
    current_path = tuple((x + 0.10, z + 0.03) for x, z in stream_path[1:-1])
    river_current.add_xyz(*terrain_ribbon(current_path, 0.13, 0.064), (0.0, 0.0, 0.0))

    river_foam = GardenMesh()
    for x, z, sx, angle in (
        (-3.52, -3.12, 0.22, -0.28),
        (-4.18, -1.88, 0.18, 0.18),
        (-4.12, 0.52, 0.20, -0.22),
        (-3.40, 1.62, 0.16, 0.30),
        (-2.62, 2.72, 0.18, -0.18),
    ):
        add_box(river_foam, (x, 0.078, z), (sx, 0.012, 0.035), (0.0, angle, 0.0))

    stepping_stones = GardenMesh()
    for x, y, z, scale, yaw in (
        (-4.55, 0.18, 0.02, (0.32, 0.13, 0.25), -0.18),
        (-4.18, 0.22, 0.24, (0.29, 0.15, 0.23), 0.10),
        (-3.80, 0.17, 0.47, (0.30, 0.12, 0.24), -0.08),
        (-3.45, 0.20, 0.70, (0.27, 0.14, 0.22), 0.16),
    ):
        add_octahedron(stepping_stones, (x, y, z), scale, (0.0, yaw, 0.0))

    grass_tufts = GardenMesh()
    for x, z, height, lean in (
        (-4.92, -2.70, 0.42, -0.14), (-3.18, -2.35, 0.34, 0.12),
        (-4.96, -0.25, 0.38, -0.10), (-3.15, 0.08, 0.36, 0.14),
        (-4.18, 1.60, 0.44, -0.12), (-2.85, 1.68, 0.34, 0.10),
        (-3.28, 2.92, 0.40, -0.10), (-1.75, 2.90, 0.36, 0.14),
        (3.20, 2.00, 0.38, -0.12), (4.55, 0.55, 0.34, 0.12),
    ):
        add_cone(grass_tufts, (x, 0.16 + height * 0.45, z), 0.12, height, 5, (0.0, 0.0, lean))
        add_box(grass_tufts, (x, 0.17, z), (0.022, 0.15, 0.022))

    blue_flowers = GardenMesh()
    for x, z in ((-4.86, -0.18), (-3.08, 0.12), (-3.86, 1.76), (3.24, 2.08)):
        add_octahedron(blue_flowers, (x, 0.47, z), (0.10, 0.07, 0.10), (0.0, 0.18, 0.0))

    gold_flowers = GardenMesh()
    for x, z in ((-3.22, -2.42), (-4.92, -2.76), (-2.78, 1.72), (-1.70, 2.96)):
        add_octahedron(gold_flowers, (x, 0.45, z), (0.09, 0.065, 0.09), (0.0, -0.16, 0.0))
    add_octahedron(gold_flowers, (-3.70, 0.16, -2.66), (0.075, 0.055, 0.075))
    add_octahedron(gold_flowers, (-3.16, 0.16, 2.02), (0.070, 0.052, 0.070))

    mushrooms = GardenMesh()
    for x, z, size in ((-4.72, 1.30, 0.16), (-4.45, 1.48, 0.11), (4.30, 0.82, 0.14)):
        add_octahedron(mushrooms, (x, 0.29, z), (size, size * 0.48, size))

    fallen_log = GardenMesh()
    add_cylinder(fallen_log, (3.78, 0.35, 1.35), 0.30, 2.35, 9, (0.0, 0.0, math.pi / 2))
    add_cylinder(fallen_log, (4.12, 0.62, 1.38), 0.10, 0.78, 7, (0.0, 0.0, 0.68))

    log_rings = GardenMesh()
    add_cylinder(log_rings, (2.59, 0.35, 1.35), 0.245, 0.045, 9, (0.0, 0.0, math.pi / 2))
    for x, z in ((-4.72, 1.30), (-4.45, 1.48), (4.30, 0.82)):
        add_box(log_rings, (x, 0.18, z), (0.035, 0.14, 0.035))

    hidden_gems = GardenMesh()
    for location, scale, rotation in (
        ((-4.92, 0.42, -1.52), (0.11, 0.20, 0.08), (0.0, 0.24, 0.10)),
        ((-2.95, 0.34, 2.18), (0.09, 0.17, 0.07), (0.0, -0.18, -0.08)),
        ((4.52, 0.38, 0.62), (0.10, 0.19, 0.08), (0.0, 0.20, 0.06)),
    ):
        add_octahedron(hidden_gems, location, scale, rotation)

    lily_pads = GardenMesh()
    for x, z, radius in ((-3.70, -2.66, 0.22), (-4.08, -1.30, 0.18), (-3.16, 2.02, 0.20)):
        add_cylinder(lily_pads, (x, 0.082, z), radius, 0.028, 8)

    stone = GardenMesh()
    add_cylinder(stone, (0.0, 0.03, 0.0), 3.15, 0.55, 20)
    add_cylinder(stone, (0.0, 0.38, 0.0), 2.55, 0.36, 20)
    for x, z, height in ((-4.5, -1.8, 3.2), (4.5, -1.8, 3.7), (-4.1, 2.3, 2.2), (4.2, 2.0, 2.5)):
        add_cylinder(stone, (x, height * 0.5 - 0.25, z), 0.42, height, 10)
        add_box(stone, (x, height - 0.05, z), (0.58, 0.14, 0.58))
    stone_highlight = GardenMesh()
    for x, y in ((-4.5, 1.6), (4.5, 2.1), (-4.1, 0.9), (4.2, 1.1)):
        add_box(stone_highlight, (x, y, -2.66), (0.28, 0.06, 0.035), (0.0, 0.0, 0.12))

    moss = GardenMesh()
    for x, y, z, sx in ((-2.35, 5.92, -3.15, 0.36), (2.35, 5.92, -3.15, 0.36), (-4.5, 3.22, -1.8, 0.46), (4.5, 3.72, -1.8, 0.46)):
        add_box(moss, (x, y, z), (sx, 0.07, 0.34))

    magic_rings = GardenMesh()
    magic_rings.add_xyz(*torus(2.35, 0.035, 36, 4), (0.0, 0.70, 0.0))
    magic_rings.add_xyz(*torus(2.72, 0.025, 36, 4), (0.0, 0.69, 0.0))

    light_motes = GardenMesh()
    for x, y, z, scale in (
        (-2.8, 4.8, 0.5, 0.055), (-1.8, 6.7, -0.4, 0.07), (3.0, 6.8, -0.3, 0.055),
        (3.7, 4.0, 0.6, 0.07), (-3.8, 2.8, 0.9, 0.05), (1.9, 2.3, 1.5, 0.045),
    ):
        add_octahedron(light_motes, (x, y, z), (scale, scale * 1.8, scale))

    # Link's outfit: boots and leggings create a broad, readable stance.
    boots = GardenMesh()
    add_box(boots, (-0.52, 0.95, 0.05), (0.38, 0.82, 0.50), (0.0, 0.0, -0.08))
    add_box(boots, (0.52, 0.95, 0.05), (0.38, 0.82, 0.50), (0.0, 0.0, 0.08))
    add_box(boots, (-0.52, 0.42, 0.42), (0.42, 0.27, 0.70))
    add_box(boots, (0.52, 0.42, 0.42), (0.42, 0.27, 0.70))

    boot_trim = GardenMesh()
    add_box(boot_trim, (-0.52, 1.55, 0.05), (0.40, 0.08, 0.52), (0.0, 0.0, -0.08))
    add_box(boot_trim, (0.52, 1.55, 0.05), (0.40, 0.08, 0.52), (0.0, 0.0, 0.08))
    for x in (-0.52, 0.52):
        for y in (0.72, 1.02, 1.30):
            add_box(boot_trim, (x, y, 0.57), (0.28, 0.035, 0.035), (0.0, 0.0, -0.10 if x < 0 else 0.10))

    leggings = GardenMesh()
    add_box(leggings, (-0.48, 2.18, 0.0), (0.30, 0.72, 0.34), (0.0, 0.0, -0.05))
    add_box(leggings, (0.48, 2.18, 0.0), (0.30, 0.72, 0.34), (0.0, 0.0, 0.05))

    tunic = GardenMesh()
    add_box(tunic, (0.0, 4.05, 0.0), (1.02, 1.34, 0.66))
    tunic.add_xyz(
        *extruded_polygon(((-1.16, -0.82), (1.16, -0.82), (0.88, 0.70), (-0.88, 0.70)), 1.26),
        (0.0, 3.10, 0.0),
    )
    add_box(tunic, (-1.03, 4.63, 0.0), (0.45, 0.52, 0.52), (0.0, 0.0, -0.30))
    add_box(tunic, (1.05, 4.62, 0.0), (0.45, 0.52, 0.52), (0.0, 0.0, 0.25))

    undershirt = GardenMesh()
    # Cream collar and sleeve cuffs separate the arms from the green tunic.
    undershirt.add_xyz(
        *extruded_polygon(((-0.48, 0.22), (0.0, -0.24), (0.48, 0.22), (0.30, 0.34), (0.0, 0.04), (-0.30, 0.34)), 0.10),
        (0.0, 5.08, 0.70),
    )
    add_box(undershirt, (-1.22, 4.31, 0.48), (0.27, 0.13, 0.27), (0.0, 0.0, -0.38))
    add_box(undershirt, (1.20, 4.43, 0.47), (0.27, 0.13, 0.27), (0.0, 0.0, 0.35))

    cap = GardenMesh()
    add_cone(cap, (-0.48, 7.08, -0.20), 0.72, 2.45, 12, (0.0, 0.0, 0.72))
    add_cylinder(cap, (0.0, 6.55, -0.04), 0.72, 0.22, 12)

    leather = GardenMesh()
    add_box(leather, (0.0, 3.55, 0.68), (1.10, 0.16, 0.10))
    add_box(leather, (0.0, 4.25, 0.68), (0.14, 1.25, 0.10), (0.0, 0.0, -0.55))
    add_box(leather, (-0.82, 3.35, 0.62), (0.34, 0.47, 0.20), (0.0, 0.0, -0.22))

    gold = GardenMesh()
    add_box(gold, (0.0, 3.55, 0.82), (0.22, 0.23, 0.08))
    add_octahedron(gold, (0.0, 5.25, 0.70), (0.15, 0.19, 0.08))
    add_box(gold, (-0.82, 3.62, 0.83), (0.07, 0.18, 0.06))

    skin = GardenMesh()
    add_cylinder(skin, (0.0, 5.92, 0.0), 0.64, 1.08, 10)
    add_box(skin, (0.0, 5.30, 0.0), (0.25, 0.23, 0.26))
    # Pointed ears.
    add_triangle(skin, ((0.0, 0.22, 0.0), (-0.62, 0.0, 0.0), (0.0, -0.18, 0.0)), (-0.58, 6.04, 0.04))
    add_triangle(skin, ((0.0, 0.22, 0.0), (0.62, 0.0, 0.0), (0.0, -0.18, 0.0)), (0.58, 6.04, 0.04))
    # Forearms and hands: left supports the shield; right closes around the sword.
    add_box(skin, (-1.34, 4.10, 0.55), (0.24, 0.67, 0.25), (0.0, 0.0, -0.38))
    add_octahedron(skin, (-1.55, 3.58, 0.72), (0.28, 0.30, 0.27))
    add_box(skin, (1.30, 4.22, 0.52), (0.23, 0.54, 0.24), (0.0, 0.0, 0.35))
    add_octahedron(skin, (1.42, 3.80, 0.73), (0.27, 0.29, 0.26))

    hair = GardenMesh()
    for location, scale, angle in (
        ((-0.48, 6.48, 0.02), (0.30, 0.42, 0.48), -0.28),
        ((0.48, 6.48, 0.02), (0.30, 0.42, 0.48), 0.28),
        ((-0.62, 5.92, -0.28), (0.30, 0.54, 0.34), -0.18),
        ((0.62, 5.92, -0.28), (0.30, 0.54, 0.34), 0.18),
        ((0.0, 6.58, -0.50), (0.52, 0.46, 0.34), 0.0),
    ):
        add_octahedron(hair, location, scale, (0.0, 0.0, angle))
    # Angular fringe gives the face the recognizable N64-era silhouette.
    for x, angle in ((-0.40, -0.18), (-0.14, -0.07), (0.16, 0.08), (0.40, 0.18)):
        hair.add_xyz(*triangular_prism(0.32, 0.55, 0.18), (x, 6.28, 0.52), rotation=(0.0, 0.0, math.pi + angle))

    face = GardenMesh()
    add_box(face, (-0.22, 6.05, 0.625), (0.11, 0.075, 0.045))
    add_box(face, (0.22, 6.05, 0.625), (0.11, 0.075, 0.045))
    add_box(face, (0.0, 5.76, 0.61), (0.16, 0.045, 0.045))

    eyes = GardenMesh()
    add_box(eyes, (-0.20, 6.05, 0.675), (0.035, 0.052, 0.025))
    add_box(eyes, (0.20, 6.05, 0.675), (0.035, 0.052, 0.025))

    eyebrow = GardenMesh()
    add_box(eyebrow, (-0.22, 6.19, 0.665), (0.13, 0.028, 0.025), (0.0, 0.0, -0.10))
    add_box(eyebrow, (0.22, 6.19, 0.665), (0.13, 0.028, 0.025), (0.0, 0.0, 0.10))

    # Layered Hylian shield on the left arm.
    shield_location = (-1.42, 4.08, 1.05)
    shield_rotation = (0.0, 0.30, -0.08)
    shield_normal = (math.sin(shield_rotation[1]), 0.0, math.cos(shield_rotation[1]))
    shield_silver = GardenMesh()
    shield_silver.add_xyz(*shield_geometry(), shield_location, (1.34, 1.34, 1.0), shield_rotation)
    shield_blue = GardenMesh()
    shield_blue.add_xyz(
        *shield_geometry(),
        offset(shield_location, shield_normal, 0.13),
        (1.10, 1.10, 0.65),
        shield_rotation,
    )

    shield_red = GardenMesh()
    crest_origin = offset(shield_location, shield_normal, 0.25)
    add_triangle(
        shield_red,
        ((-0.62, 0.30, 0.0), (0.0, -0.54, 0.0), (-0.16, 0.22, 0.0)),
        crest_origin,
        shield_rotation,
    )
    add_triangle(
        shield_red,
        ((0.62, 0.30, 0.0), (0.0, -0.54, 0.0), (0.16, 0.22, 0.0)),
        crest_origin,
        shield_rotation,
    )
    shield_gold = GardenMesh()
    small_triangle = ((-0.22, -0.17, 0.0), (0.22, -0.17, 0.0), (0.0, 0.20, 0.0))
    for local in ((0.0, 0.58, 0.0), (-0.24, 0.17, 0.0), (0.24, 0.17, 0.0)):
        add_triangle(shield_gold, small_triangle, tuple(crest_origin[i] + local[i] for i in range(3)), shield_rotation)

    shield_relief = GardenMesh()
    # Raised silver wings and lower boss make the face read as a crafted relic.
    add_box(shield_relief, (crest_origin[0] - 0.55, crest_origin[1] + 0.48, crest_origin[2] + 0.025), (0.32, 0.045, 0.035), (0.0, 0.30, 0.34))
    add_box(shield_relief, (crest_origin[0] + 0.55, crest_origin[1] + 0.48, crest_origin[2] + 0.025), (0.32, 0.045, 0.035), (0.0, 0.30, -0.34))
    add_octahedron(shield_relief, (crest_origin[0], crest_origin[1] - 0.72, crest_origin[2] + 0.04), (0.14, 0.19, 0.045))

    shield_gem = GardenMesh()
    add_octahedron(shield_gem, (crest_origin[0], crest_origin[1] + 0.86, crest_origin[2] + 0.055), (0.12, 0.15, 0.04))

    # Master Sword: a long tapered silver blade with a violet winged guard.
    sword_angle = -0.34
    sword_rotation = (0.0, 0.04, sword_angle)
    guard_center = (1.44, 3.96, 0.78)
    sword_direction = (-math.sin(sword_angle), math.cos(sword_angle), 0.0)
    blade = GardenMesh()
    blade.add_xyz(
        *extruded_polygon(((-0.19, 0.0), (0.19, 0.0), (0.17, 2.85), (0.0, 3.30), (-0.17, 2.85)), 0.15),
        guard_center,
        rotation=sword_rotation,
    )
    blade_highlight = GardenMesh()
    blade_highlight.add_xyz(
        *extruded_polygon(((-0.045, 0.16), (0.045, 0.16), (0.038, 2.82), (0.0, 3.12)), 0.035),
        offset(guard_center, (0.0, 0.0, 1.0), 0.10),
        rotation=sword_rotation,
    )

    blade_rune = GardenMesh()
    blade_rune.add_xyz(
        *extruded_polygon(((-0.11, 0.08), (0.11, 0.08), (0.08, 0.60), (0.0, 0.78), (-0.08, 0.60)), 0.025),
        offset(guard_center, (0.0, 0.0, 1.0), 0.125),
        rotation=sword_rotation,
    )

    hilt = GardenMesh()
    add_box(hilt, guard_center, (0.82, 0.13, 0.18), sword_rotation)
    # Swept violet wings follow the blade's local basis instead of screen axes.
    sword_cross = (math.cos(sword_angle), math.sin(sword_angle), 0.0)
    for side in (-1.0, 1.0):
        wing = tuple(
            guard_center[index] + sword_cross[index] * 0.62 * side + sword_direction[index] * 0.20
            for index in range(3)
        )
        add_box(hilt, wing, (0.50, 0.12, 0.20), (0.0, 0.04, sword_angle - side * 0.22))
    grip_center = offset(guard_center, sword_direction, -0.47)
    add_box(hilt, grip_center, (0.16, 0.48, 0.18), sword_rotation)
    add_octahedron(hilt, offset(guard_center, sword_direction, -1.02), (0.28, 0.34, 0.24), sword_rotation)

    grip_wrap = GardenMesh()
    for distance in (-0.22, -0.43, -0.64, -0.82):
        band_center = offset(guard_center, sword_direction, distance)
        add_box(grip_wrap, band_center, (0.19, 0.045, 0.205), sword_rotation)

    hilt_gold = GardenMesh()
    add_octahedron(hilt_gold, offset(guard_center, (0.0, 0.0, 1.0), 0.22), (0.17, 0.21, 0.09), sword_rotation)

    hilt_gem = GardenMesh()
    add_octahedron(hilt_gem, offset(guard_center, (0.0, 0.0, 1.0), 0.31), (0.085, 0.11, 0.05), sword_rotation)

    # A separate display boomerang keeps the silhouette readable from every camera.
    boomerang = GardenMesh()
    boomerang_center = (-3.15, 2.05, 1.05)
    add_box(boomerang, (-3.58, 2.42, 1.05), (0.18, 0.83, 0.16), (0.0, 0.0, -0.58))
    add_box(boomerang, (-2.72, 2.42, 1.05), (0.18, 0.83, 0.16), (0.0, 0.0, 0.58))
    add_octahedron(boomerang, boomerang_center, (0.30, 0.30, 0.22))
    boomerang_wrap = GardenMesh()
    add_box(boomerang_wrap, (-3.36, 2.12, 1.20), (0.17, 0.22, 0.08), (0.0, 0.0, -0.58))
    add_box(boomerang_wrap, (-2.94, 2.12, 1.20), (0.17, 0.22, 0.08), (0.0, 0.0, 0.58))

    boomerang_edge = GardenMesh()
    add_octahedron(boomerang_edge, (-3.96, 2.79, 1.05), (0.18, 0.30, 0.18), (0.0, 0.0, -0.58))
    add_octahedron(boomerang_edge, (-2.34, 2.79, 1.05), (0.18, 0.30, 0.18), (0.0, 0.0, 0.58))

    # Small blue ocarina/flute presented on its own velvet-topped plinth.
    prop_plinth = GardenMesh()
    add_cylinder(prop_plinth, (0.62, 0.58, 2.55), 0.88, 0.46, 12)

    prop_velvet = GardenMesh()
    add_cylinder(prop_velvet, (0.62, 0.83, 2.55), 0.76, 0.06, 12)

    ocarina = GardenMesh()
    add_cylinder(ocarina, (0.60, 1.06, 2.52), 0.34, 1.02, 14, (0.0, 0.0, math.pi / 2))
    add_box(ocarina, (1.22, 1.08, 2.52), (0.44, 0.14, 0.20), (0.0, 0.0, -0.12))
    add_cone(ocarina, (0.06, 1.06, 2.52), 0.36, 0.54, 12, (0.0, 0.0, math.pi / 2))
    ocarina_holes = GardenMesh()
    for x, y in ((0.38, 1.29), (0.64, 1.31), (0.88, 1.24), (0.52, 1.04), (0.80, 1.03)):
        add_octahedron(ocarina_holes, (x, y, 2.84), (0.072, 0.050, 0.038))

    # Navi-like fairy accent with a visible orbital trail.
    fairy = GardenMesh()
    add_octahedron(fairy, (2.45, 5.75, 1.0), (0.16, 0.20, 0.16))
    for dx, dy in ((-0.28, 0.20), (0.28, 0.20), (-0.28, -0.20), (0.28, -0.20)):
        add_octahedron(fairy, (2.45 + dx, 5.75 + dy, 0.96), (0.22, 0.12, 0.05), (0.0, 0.0, math.atan2(dy, dx)))

    fairy_aura = GardenMesh()
    add_octahedron(fairy_aura, (2.45, 5.75, 0.94), (0.48, 0.50, 0.10))

    fairy_trail = GardenMesh()
    for x, y, size in ((2.85, 5.45, 0.055), (3.18, 5.12, 0.045), (3.34, 4.72, 0.036), (3.28, 4.32, 0.028)):
        add_octahedron(fairy_trail, (x, y, 0.72), (size, size, size))

    backlight = GardenMesh()
    add_triangle(
        backlight,
        ((-3.2, 0.0, 0.0), (3.2, 0.0, 0.0), (0.65, 8.8, -0.4)),
        (0.0, 0.20, -2.0),
    )

    return (
        (1101, (241, 193, 62, 255), triforce),
        (1103, (28, 67, 54, 255), ground),
        (1104, (78, 83, 91, 255), stone),
        (1105, (77, 45, 28, 255), boots),
        (1106, (220, 213, 181, 255), leggings),
        (1107, (39, 130, 65, 255), tunic),
        (1108, (27, 91, 49, 255), cap),
        (1109, (94, 54, 31, 255), leather),
        (1110, (217, 164, 61, 255), gold),
        (1111, (226, 175, 129, 255), skin),
        (1112, (235, 190, 67, 255), hair),
        (1113, (24, 30, 39, 255), face),
        (1114, (191, 203, 211, 255), shield_silver),
        (1115, (36, 77, 158, 255), shield_blue),
        (1116, (176, 42, 48, 255), shield_red),
        (1117, (240, 191, 55, 255), shield_gold),
        (1118, (207, 225, 232, 255), blade),
        (1119, (133, 219, 248, 255), blade_highlight),
        (1120, (72, 62, 153, 255), hilt),
        (1121, (239, 188, 61, 255), hilt_gold),
        (1122, (211, 158, 63, 255), boomerang),
        (1123, (157, 48, 40, 255), boomerang_wrap),
        (1124, (48, 157, 211, 255), ocarina),
        (1125, (19, 47, 80, 255), ocarina_holes),
        (1126, (130, 240, 255, 255), fairy),
        (1127, (100, 224, 136, 34), backlight),
        (1128, (247, 211, 84, 190), sacred_halo),
        (1130, (117, 126, 134, 255), stone_highlight),
        (1131, (44, 104, 58, 255), moss),
        (1132, (70, 204, 210, 210), magic_rings),
        (1133, (244, 220, 111, 255), light_motes),
        (1134, (137, 82, 42, 255), boot_trim),
        (1135, (238, 233, 204, 255), undershirt),
        (1136, (42, 118, 174, 255), eyes),
        (1137, (126, 84, 35, 255), eyebrow),
        (1138, (224, 234, 238, 255), shield_relief),
        (1139, (104, 208, 246, 255), shield_gem),
        (1140, (75, 136, 201, 255), blade_rune),
        (1141, (39, 31, 84, 255), grip_wrap),
        (1142, (188, 48, 66, 255), hilt_gem),
        (1143, (247, 214, 132, 255), boomerang_edge),
        (1144, (100, 104, 112, 255), prop_plinth),
        (1145, (75, 45, 126, 255), prop_velvet),
        (1146, (104, 235, 255, 46), fairy_aura),
        (1147, (143, 243, 255, 255), fairy_trail),
        (1148, (44, 110, 164, 255), river),
        (1149, (103, 210, 225, 255), river_current),
        (1150, (130, 104, 67, 255), river_bank),
        (1151, (112, 124, 128, 255), stepping_stones),
        (1152, (50, 127, 61, 255), grass_tufts),
        (1153, (74, 157, 220, 255), blue_flowers),
        (1154, (241, 196, 70, 255), gold_flowers),
        (1155, (190, 63, 57, 255), mushrooms),
        (1156, (91, 56, 34, 255), fallen_log),
        (1157, (175, 113, 58, 255), log_rings),
        (1158, (63, 224, 220, 255), hidden_gems),
        (1159, (47, 121, 67, 255), lily_pads),
        (1160, (216, 242, 238, 255), river_foam),
    )


def populate(client):
    client.stop()
    client.clear()
    position, target, fov = VIEWS["forest"]
    client.camera(position, target, fov)
    for mesh_id, color, mesh in build_scene():
        triangles = sum(len(face) - 2 for face in mesh.faces)
        if len(mesh.vertices) > 1_000 or triangles > 2_000:
            raise RuntimeError(
                f"mesh {mesh_id} exceeds budget: vertices={len(mesh.vertices)} triangles={triangles}"
            )
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        location = IDENTITY_LOCATION if mesh_id in ENVIRONMENT_MESH_IDS else HERO_GROUP_LOCATION
        client.instance(40_000 + mesh_id, mesh_id, location, IDENTITY_SCALE)

    # The tree kit is uploaded once; every forest member below is a cheap transform-only instance.
    uploaded_trees = upload_tree_kit(client)
    next_tree_instance = 70_000
    for tree_name, location, scale, yaw in (
        ("deku", (-3.20, -0.18, -4.85), 1.02, 0.02),
        ("oak", (4.95, -0.18, -4.70), 0.78, -0.22),
        ("oak", (-6.25, -0.18, -5.15), 0.48, 0.28),
        ("pine", (-6.00, -0.18, -2.60), 0.50, 0.18),
        ("pine", (3.85, -0.18, -5.30), 0.66, -0.16),
        ("pine", (6.20, -0.18, -2.85), 0.54, 0.26),
        ("guardian", (-5.80, -0.18, -0.90), 0.46, -0.22),
        ("guardian", (5.70, -0.18, -1.40), 0.50, 0.24),
    ):
        next_tree_instance = place_tree(
            client,
            uploaded_trees,
            tree_name,
            next_tree_instance,
            location,
            scale,
            yaw,
        )
    # Empty StartScene is the protocol's transparent-background form.  UI4
    # receives the renderer's premultiplied RGBA output on alpha-enabled slot 3.
    client.start()


def capture_view(client, name, output_dir, settle):
    position, target, fov = VIEWS[name]
    client.camera(position, target, fov)
    time.sleep(settle)
    path, image_format, width, height, image = client.render(output_dir / f"hero-of-time-v3-{name}.png")
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
        for view_name in ("sword", "hero", "forest", "shield"):
            capture_view(client, view_name, args.output_dir, args.settle)
        stats = client.stats()
        print(
            f"scene meshes={stats[0]} instances={stats[1]} vertices={stats[2]} "
            f"edges={stats[3]} faces={stats[4]} mesh_bytes={stats[5]} final_view=shield"
        )
    finally:
        client.close()


if __name__ == "__main__":
    main()
