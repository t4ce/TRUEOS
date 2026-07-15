#!/usr/bin/env python3
"""Build and stage a reusable low-poly tree kit through the draw3d TCP API."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh, add_box, add_octahedron, radial_frustum, torus
from draw3d_house_demo import Draw3dClient


BACKGROUND = (5, 9, 22, 255)
TREE_MESH_BASE = 2_100
TREE_INSTANCE_BASE = 52_000

STUDY_VIEWS = {
    "grove": ((10.5, 6.8, 17.5), (0.0, 2.25, 0.0), 48.0),
    "crown": ((-9.5, 7.7, 14.0), (-0.2, 2.75, -0.2), 44.0),
    "roots": ((8.0, 3.8, 15.5), (0.0, 1.55, 0.5), 43.0),
}


def add_frustum(mesh, location, radius, height, segments=7, top_ratio=0.72, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(
        *radial_frustum(segments, bottom_radius=1.0, top_radius=top_ratio, height=2.0),
        location,
        (radius, height * 0.5, radius),
        rotation,
    )


def add_cone(mesh, location, radius, height, segments=8, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(
        *radial_frustum(segments, bottom_radius=1.0, top_radius=0.0, height=2.0),
        location,
        (radius, height * 0.5, radius),
        rotation,
    )


def add_roots(mesh, radius=0.82, count=7, y=0.13):
    for index in range(count):
        angle = math.tau * index / count + 0.18
        distance = radius * 0.55
        location = (math.sin(angle) * distance, y, math.cos(angle) * distance)
        add_box(
            mesh,
            location,
            (0.13, 0.11, radius * 0.62),
            (0.0, angle, 0.10 if index % 2 else -0.08),
        )


def root_wedge(width=1.0, height=0.6, length=2.0):
    """A faceted buttress root extending along local +Z."""
    half_width = width * 0.5
    tip_width = half_width * 0.18
    vertices = (
        (-half_width, 0.0, 0.0),
        (half_width, 0.0, 0.0),
        (-tip_width, 0.0, length),
        (tip_width, 0.0, length),
        (-half_width * 0.72, height, 0.0),
        (half_width * 0.72, height, 0.0),
        (-tip_width, height * 0.08, length),
        (tip_width, height * 0.08, length),
    )
    faces = (
        (0, 2, 3, 1),
        (4, 5, 7, 6),
        (0, 4, 6, 2),
        (1, 3, 7, 5),
        (0, 1, 5, 4),
        (2, 6, 7, 3),
    )
    return vertices, faces


def build_ancient_oak():
    bark = GardenMesh()
    add_frustum(bark, (0.0, 0.85, 0.0), 0.48, 1.70, 8, 0.72)
    add_frustum(bark, (0.0, 2.05, 0.0), 0.35, 1.35, 8, 0.58)
    add_roots(bark, 1.05, 8)
    # Broad load-bearing boughs, including one aimed toward the camera.
    for location, radius, length, rotation in (
        ((-0.48, 2.52, 0.02), 0.22, 1.65, (0.0, 0.0, -0.92)),
        ((0.48, 2.48, -0.02), 0.21, 1.55, (0.0, 0.0, 0.88)),
        ((0.02, 2.38, 0.48), 0.18, 1.25, (0.96, 0.0, 0.0)),
        ((-0.06, 2.72, -0.40), 0.16, 1.10, (-0.82, 0.18, 0.12)),
    ):
        add_frustum(bark, location, radius, length, 7, 0.48, rotation)

    bark_light = GardenMesh()
    add_box(bark_light, (0.0, 1.28, 0.445), (0.095, 0.65, 0.035), (0.0, 0.0, -0.08))
    add_box(bark_light, (-0.30, 2.47, 0.20), (0.055, 0.48, 0.035), (0.0, 0.0, -0.76))
    for x, y, z in ((-0.28, 0.52, 0.42), (0.24, 0.86, 0.40), (-0.18, 1.64, 0.34)):
        add_box(bark_light, (x, y, z), (0.10, 0.035, 0.025), (0.0, 0.0, 0.22))

    shadow = GardenMesh()
    for location, scale, angle in (
        ((-0.88, 3.05, -0.28), (1.08, 0.82, 0.90), -0.12),
        ((0.82, 3.02, -0.35), (1.05, 0.80, 0.88), 0.12),
        ((0.0, 3.38, -0.48), (1.15, 0.82, 0.92), 0.0),
        ((-1.45, 2.88, -0.10), (0.72, 0.62, 0.70), -0.24),
        ((1.42, 2.86, -0.08), (0.70, 0.60, 0.68), 0.24),
    ):
        add_octahedron(shadow, location, scale, (0.0, angle, angle * 0.35))

    mid = GardenMesh()
    for location, scale, angle in (
        ((-0.75, 3.44, 0.08), (0.92, 0.74, 0.76), -0.14),
        ((0.72, 3.42, 0.06), (0.92, 0.72, 0.76), 0.14),
        ((0.02, 3.72, 0.02), (0.96, 0.72, 0.78), 0.0),
        ((-1.35, 3.18, 0.20), (0.68, 0.56, 0.62), -0.22),
        ((1.33, 3.15, 0.18), (0.68, 0.55, 0.62), 0.22),
        ((-0.12, 3.10, 0.58), (0.84, 0.62, 0.66), 0.0),
    ):
        add_octahedron(mid, location, scale, (0.0, angle, angle))

    light = GardenMesh()
    for location, scale in (
        ((-0.52, 3.76, 0.42), (0.52, 0.38, 0.42)),
        ((0.50, 3.72, 0.44), (0.50, 0.37, 0.42)),
        ((0.0, 4.05, 0.18), (0.48, 0.36, 0.40)),
        ((-1.16, 3.40, 0.48), (0.38, 0.30, 0.34)),
        ((1.14, 3.36, 0.46), (0.38, 0.30, 0.34)),
    ):
        add_octahedron(light, location, scale, (0.0, 0.15, 0.08))

    return (
        ("bark", (83, 50, 31, 255), bark),
        ("bark_light", (139, 91, 48, 255), bark_light),
        ("shadow", (17, 58, 47, 255), shadow),
        ("mid", (32, 103, 61, 255), mid),
        ("light", (71, 151, 76, 255), light),
    )


def build_layered_pine():
    bark = GardenMesh()
    add_frustum(bark, (0.0, 1.52, 0.0), 0.22, 3.04, 7, 0.55)
    add_roots(bark, 0.68, 6, y=0.10)

    shadow = GardenMesh()
    for y, radius, height, twist in ((1.25, 1.18, 1.45, 0.0), (2.05, 0.98, 1.35, 0.20), (2.82, 0.76, 1.28, -0.15), (3.48, 0.48, 1.02, 0.10)):
        add_cone(shadow, (0.0, y, -0.10), radius, height, 9, (0.0, twist, 0.0))

    mid = GardenMesh()
    for y, radius, height, x in ((1.48, 1.00, 1.05, -0.10), (2.28, 0.82, 1.02, 0.08), (3.00, 0.62, 0.94, -0.05), (3.62, 0.36, 0.72, 0.04)):
        add_cone(mid, (x, y, 0.18), radius, height, 8)

    light = GardenMesh()
    for location, scale in (
        ((-0.42, 1.62, 0.56), (0.46, 0.20, 0.26)),
        ((0.34, 2.35, 0.45), (0.40, 0.18, 0.24)),
        ((-0.24, 3.02, 0.34), (0.31, 0.16, 0.20)),
        ((0.10, 3.60, 0.22), (0.20, 0.13, 0.15)),
    ):
        add_octahedron(light, location, scale)

    return (
        ("bark", (72, 48, 33, 255), bark),
        ("shadow", (11, 48, 49, 255), shadow),
        ("mid", (24, 86, 62, 255), mid),
        ("light", (55, 131, 77, 255), light),
    )


def build_twisted_guardian():
    bark = GardenMesh()
    add_frustum(bark, (-0.10, 0.82, 0.0), 0.42, 1.64, 7, 0.68, (0.0, 0.0, -0.10))
    add_frustum(bark, (0.08, 1.98, 0.0), 0.30, 1.32, 7, 0.52, (0.0, 0.0, 0.25))
    add_roots(bark, 0.98, 7)
    for location, radius, length, rotation in (
        ((-0.48, 2.34, 0.02), 0.19, 1.55, (0.0, 0.0, -1.02)),
        ((0.46, 2.52, -0.04), 0.18, 1.75, (0.0, 0.0, 0.82)),
        ((0.82, 3.04, 0.0), 0.12, 1.05, (0.0, 0.0, 0.38)),
        ((-0.86, 2.84, 0.02), 0.12, 0.92, (0.0, 0.0, -0.42)),
    ):
        add_frustum(bark, location, radius, length, 6, 0.38, rotation)

    bark_light = GardenMesh()
    for location, scale, rotation in (
        ((0.18, 0.78, 0.38), (0.07, 0.55, 0.03), (0.0, 0.0, -0.12)),
        ((-0.18, 1.68, 0.28), (0.06, 0.42, 0.03), (0.0, 0.0, 0.24)),
        ((0.56, 2.70, 0.18), (0.05, 0.34, 0.03), (0.0, 0.0, 0.76)),
    ):
        add_box(bark_light, location, scale, rotation)

    shadow = GardenMesh()
    for location, scale in (
        ((-1.18, 3.00, -0.18), (0.76, 0.68, 0.68)),
        ((1.18, 3.32, -0.22), (0.82, 0.72, 0.72)),
        ((0.24, 3.72, -0.32), (0.84, 0.70, 0.72)),
    ):
        add_octahedron(shadow, location, scale, (0.0, 0.20, 0.14))

    mid = GardenMesh()
    for location, scale in (
        ((-1.24, 3.30, 0.18), (0.66, 0.56, 0.60)),
        ((1.25, 3.62, 0.16), (0.70, 0.58, 0.62)),
        ((0.16, 4.00, 0.16), (0.72, 0.58, 0.60)),
        ((0.38, 3.34, 0.54), (0.58, 0.50, 0.52)),
    ):
        add_octahedron(mid, location, scale, (0.0, -0.15, -0.10))

    light = GardenMesh()
    for location, scale in (
        ((-1.38, 3.52, 0.46), (0.34, 0.27, 0.30)),
        ((1.36, 3.86, 0.42), (0.35, 0.28, 0.30)),
        ((0.05, 4.24, 0.38), (0.38, 0.30, 0.32)),
    ):
        add_octahedron(light, location, scale)

    return (
        ("bark", (75, 45, 31, 255), bark),
        ("bark_light", (156, 95, 48, 255), bark_light),
        ("shadow", (20, 52, 48, 255), shadow),
        ("mid", (38, 100, 63, 255), mid),
        ("light", (82, 148, 73, 255), light),
    )


def build_great_deku():
    """A monumental, face-bearing tree built as reusable material components."""
    bark = GardenMesh()
    # Wide flared trunk, narrowing into a short crown-bearing column.
    add_frustum(bark, (0.0, 1.55, 0.0), 1.34, 3.10, 10, 0.78)
    add_frustum(bark, (0.0, 3.85, -0.04), 1.02, 2.15, 9, 0.68)
    add_frustum(bark, (0.0, 5.20, -0.05), 0.70, 1.35, 8, 0.48)

    # Eight large buttress roots make the scale and weight visible immediately.
    for index, (length, width, height) in enumerate(
        (
            (2.65, 1.16, 0.82), (2.10, 0.88, 0.62), (2.42, 1.02, 0.70),
            (1.95, 0.82, 0.58), (2.52, 1.08, 0.76), (2.12, 0.86, 0.60),
            (2.34, 0.98, 0.68), (1.88, 0.80, 0.56),
        )
    ):
        yaw = math.tau * index / 8.0 + 0.10
        bark.add_xyz(*root_wedge(width, height, length), (0.0, 0.0, 0.0), rotation=(0.0, yaw, 0.0))

    # Old structural branches remain visible below the crown.
    for location, radius, length, rotation in (
        ((-0.72, 4.92, -0.04), 0.34, 2.65, (0.0, 0.0, -1.02)),
        ((0.72, 4.86, -0.06), 0.33, 2.55, (0.0, 0.0, 0.98)),
        ((-0.28, 5.44, -0.48), 0.25, 1.82, (-0.92, 0.18, -0.22)),
        ((0.32, 5.48, 0.42), 0.24, 1.70, (0.88, -0.12, 0.18)),
        ((-1.58, 5.58, -0.05), 0.18, 1.45, (0.0, 0.0, -0.62)),
        ((1.56, 5.52, -0.08), 0.18, 1.42, (0.0, 0.0, 0.60)),
    ):
        add_frustum(bark, location, radius, length, 7, 0.40, rotation)

    bark_light = GardenMesh()
    # Long vertical plates make the trunk feel ancient rather than smooth.
    for location, scale, rotation in (
        ((-0.54, 1.58, 1.06), (0.14, 1.12, 0.045), (0.0, 0.0, -0.08)),
        ((0.58, 1.32, 1.04), (0.12, 0.84, 0.045), (0.0, 0.0, 0.10)),
        ((-0.78, 3.96, 0.62), (0.10, 0.68, 0.040), (0.0, 0.0, -0.20)),
        ((0.74, 4.18, 0.58), (0.10, 0.72, 0.040), (0.0, 0.0, 0.18)),
        ((0.06, 4.82, 0.68), (0.08, 0.56, 0.035), (0.0, 0.0, 0.04)),
    ):
        add_box(bark_light, location, scale, rotation)
    for x, y, z, sx in ((-0.46, 0.66, 1.18, 0.22), (0.34, 1.02, 1.20, 0.18), (-0.30, 2.05, 1.14, 0.20), (0.40, 4.54, 0.78, 0.16)):
        add_box(bark_light, (x, y, z), (sx, 0.045, 0.035), (0.0, 0.0, 0.16))

    face_bark = GardenMesh()
    # Heavy brows, long nose, and swept moustache are separate raised bark pieces.
    add_box(face_bark, (-0.48, 3.74, 1.08), (0.42, 0.13, 0.10), (0.0, 0.0, -0.16))
    add_box(face_bark, (0.48, 3.74, 1.08), (0.42, 0.13, 0.10), (0.0, 0.0, 0.16))
    add_frustum(face_bark, (0.0, 3.18, 1.34), 0.29, 0.98, 7, 0.70, (math.pi / 2, 0.0, 0.0))
    add_octahedron(face_bark, (0.0, 2.98, 1.73), (0.34, 0.26, 0.22))
    add_frustum(face_bark, (-0.48, 2.74, 1.22), 0.18, 1.38, 6, 0.32, (0.0, 0.0, -1.05))
    add_frustum(face_bark, (0.48, 2.74, 1.22), 0.18, 1.38, 6, 0.32, (0.0, 0.0, 1.05))

    face_dark = GardenMesh()
    # Deep-set eyes and a quiet mouth survive even at the wide establishing camera.
    add_box(face_dark, (-0.46, 3.47, 1.20), (0.29, 0.115, 0.075), (0.0, 0.0, -0.06))
    add_box(face_dark, (0.46, 3.47, 1.20), (0.29, 0.115, 0.075), (0.0, 0.0, 0.06))
    add_box(face_dark, (0.0, 2.34, 1.19), (0.46, 0.095, 0.075))
    add_box(face_dark, (-0.66, 4.18, 0.98), (0.06, 0.30, 0.05), (0.0, 0.0, -0.28))
    add_box(face_dark, (0.66, 4.18, 0.98), (0.06, 0.30, 0.05), (0.0, 0.0, 0.28))

    eye_glow = GardenMesh()
    add_octahedron(eye_glow, (-0.43, 3.48, 1.29), (0.065, 0.055, 0.035))
    add_octahedron(eye_glow, (0.43, 3.48, 1.29), (0.065, 0.055, 0.035))

    crown_shadow = GardenMesh()
    for location, scale, rotation in (
        ((-2.10, 5.92, -0.38), (1.72, 1.12, 1.28), -0.16),
        ((0.0, 6.28, -0.62), (1.88, 1.22, 1.38), 0.0),
        ((2.06, 5.94, -0.40), (1.70, 1.10, 1.26), 0.16),
        ((-1.10, 7.18, -0.64), (1.52, 1.02, 1.18), -0.12),
        ((1.12, 7.16, -0.62), (1.52, 1.02, 1.18), 0.12),
        ((0.0, 8.00, -0.58), (1.30, 0.94, 1.04), 0.0),
    ):
        add_octahedron(crown_shadow, location, scale, (0.0, rotation, rotation * 0.6))

    crown_mid = GardenMesh()
    for location, scale, rotation in (
        ((-2.28, 6.34, 0.22), (1.40, 0.94, 1.04), -0.18),
        ((-0.72, 6.72, 0.36), (1.46, 0.98, 1.08), -0.08),
        ((0.78, 6.70, 0.34), (1.46, 0.98, 1.08), 0.08),
        ((2.26, 6.30, 0.20), (1.38, 0.92, 1.02), 0.18),
        ((-1.18, 7.60, 0.14), (1.24, 0.86, 0.96), -0.10),
        ((1.16, 7.58, 0.12), (1.24, 0.86, 0.96), 0.10),
        ((0.0, 8.32, 0.06), (1.06, 0.74, 0.84), 0.0),
    ):
        add_octahedron(crown_mid, location, scale, (0.0, rotation, rotation * 0.7))

    crown_light = GardenMesh()
    for location, scale in (
        ((-2.42, 6.70, 0.72), (0.62, 0.42, 0.48)),
        ((-0.88, 7.10, 0.82), (0.68, 0.46, 0.52)),
        ((0.76, 7.08, 0.80), (0.66, 0.45, 0.50)),
        ((2.32, 6.66, 0.68), (0.60, 0.41, 0.46)),
        ((-0.88, 7.92, 0.60), (0.56, 0.38, 0.44)),
        ((0.78, 7.88, 0.58), (0.56, 0.38, 0.44)),
        ((0.0, 8.58, 0.38), (0.48, 0.34, 0.38)),
    ):
        add_octahedron(crown_light, location, scale, (0.0, 0.16, 0.08))

    return (
        ("bark", (72, 43, 29, 255), bark),
        ("bark_light", (137, 84, 44, 255), bark_light),
        ("face_bark", (108, 65, 36, 255), face_bark),
        ("face_dark", (22, 28, 27, 255), face_dark),
        ("eye_glow", (126, 207, 158, 255), eye_glow),
        ("crown_shadow", (11, 47, 43, 255), crown_shadow),
        ("crown_mid", (27, 92, 55, 255), crown_mid),
        ("crown_light", (65, 142, 68, 255), crown_light),
    )


def build_tree_kit():
    return {
        "oak": build_ancient_oak(),
        "pine": build_layered_pine(),
        "guardian": build_twisted_guardian(),
        "deku": build_great_deku(),
    }


def upload_tree_kit(client, mesh_base=TREE_MESH_BASE):
    uploaded = {}
    next_mesh_id = mesh_base
    for tree_name, components in build_tree_kit().items():
        uploaded[tree_name] = []
        for component_name, color, mesh in components:
            triangles = sum(len(face) - 2 for face in mesh.faces)
            if len(mesh.vertices) > 1_000 or triangles > 2_000:
                raise RuntimeError(
                    f"tree mesh {tree_name}/{component_name} exceeds budget: "
                    f"vertices={len(mesh.vertices)} triangles={triangles}"
                )
            client.mesh(next_mesh_id, color, mesh.vertices, mesh.faces)
            uploaded[tree_name].append(next_mesh_id)
            next_mesh_id += 1
    return uploaded


def place_tree(client, uploaded, tree_name, instance_base, location, scale=1.0, yaw=0.0):
    scale_vector = (scale, scale, scale) if isinstance(scale, (int, float)) else scale
    for component_index, mesh_id in enumerate(uploaded[tree_name]):
        client.instance(
            instance_base + component_index,
            mesh_id,
            location,
            scale_vector,
            (0.0, yaw, 0.0),
        )
    return instance_base + len(uploaded[tree_name])


def build_study_environment():
    ground = GardenMesh()
    ground.add_xyz(*radial_frustum(24, bottom_radius=0.88, top_radius=1.0, height=2.0), (0.0, -0.34, 0.0), (6.3, 0.34, 4.2))

    stones = GardenMesh()
    for x, z, scale in ((-4.2, 1.0, 0.38), (-1.0, 2.7, 0.26), (2.5, 2.4, 0.32), (4.7, -0.4, 0.42)):
        add_octahedron(stones, (x, 0.05, z), (scale, scale * 0.65, scale * 0.88))

    rings = GardenMesh()
    for x in (-3.1, 0.0, 3.1):
        rings.add_xyz(*torus(1.18, 0.025, 28, 4), (x, 0.08, 0.0))

    motes = GardenMesh()
    for x, y, z, size in ((-4.0, 3.5, 0.5, 0.05), (-2.0, 4.7, -0.2, 0.07), (0.5, 3.8, 0.8, 0.045), (2.0, 5.0, -0.3, 0.06), (4.2, 3.2, 0.5, 0.05)):
        add_octahedron(motes, (x, y, z), (size, size * 1.7, size))

    return (
        (2_180, (24, 63, 52, 255), ground),
        (2_181, (82, 91, 96, 255), stones),
        (2_182, (66, 202, 190, 210), rings),
        (2_183, (240, 213, 108, 255), motes),
    )


def populate_study(client):
    client.stop()
    client.clear()
    position, target, fov = STUDY_VIEWS["grove"]
    client.camera(position, target, fov)

    uploaded = upload_tree_kit(client)
    next_instance = TREE_INSTANCE_BASE
    for tree_name, location, scale, yaw in (
        ("oak", (-3.1, 0.0, 0.0), 1.08, 0.15),
        ("pine", (0.0, 0.0, -0.15), 1.10, -0.20),
        ("guardian", (3.1, 0.0, 0.0), 1.04, -0.12),
        ("pine", (-5.0, 0.0, -2.2), 0.70, 0.25),
        ("oak", (5.0, 0.0, -2.4), 0.62, -0.30),
    ):
        next_instance = place_tree(client, uploaded, tree_name, next_instance, location, scale, yaw)

    for mesh_id, color, mesh in build_study_environment():
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(60_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start(BACKGROUND)


def capture_view(client, name, output_dir, settle):
    position, target, fov = STUDY_VIEWS[name]
    client.camera(position, target, fov)
    time.sleep(settle)
    path, image_format, width, height, image = client.render(output_dir / f"poly-tree-study-{name}.png")
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
        populate_study(client)
        for view_name in ("roots", "crown", "grove"):
            capture_view(client, view_name, args.output_dir, args.settle)
        stats = client.stats()
        print(
            f"tree-study meshes={stats[0]} instances={stats[1]} vertices={stats[2]} "
            f"edges={stats[3]} faces={stats[4]} mesh_bytes={stats[5]} final_view=grove"
        )
    finally:
        client.close()


if __name__ == "__main__":
    main()
