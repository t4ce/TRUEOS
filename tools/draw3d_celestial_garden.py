#!/usr/bin/env python3
"""Paint a celestial signal garden on the TRUEOS draw3d screen."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_house_demo import (
    CUBE_FACES,
    CUBE_VERTICES,
    OCTAHEDRON_FACES,
    OCTAHEDRON_VERTICES,
    Draw3dClient,
    MeshBuilder,
)


BACKGROUND = (7, 10, 30, 255)


class GardenMesh(MeshBuilder):
    """MeshBuilder with arbitrary baked XYZ rotation."""

    def add_xyz(self, vertices, faces, location, scale=(1.0, 1.0, 1.0), rotation=(0.0, 0.0, 0.0)):
        base = len(self.vertices)
        sx, sy, sz = scale
        rx, ry, rz = rotation
        sin_x, cos_x = math.sin(rx), math.cos(rx)
        sin_y, cos_y = math.sin(ry), math.cos(ry)
        sin_z, cos_z = math.sin(rz), math.cos(rz)

        for x, y, z in vertices:
            x, y, z = x * sx, y * sy, z * sz
            y, z = y * cos_x - z * sin_x, y * sin_x + z * cos_x
            x, z = x * cos_y + z * sin_y, -x * sin_y + z * cos_y
            x, y = x * cos_z - y * sin_z, x * sin_z + y * cos_z
            self.vertices.append((x + location[0], y + location[1], z + location[2]))
        self.faces.extend(tuple(base + index for index in face) for face in faces)


def radial_frustum(segments=16, bottom_radius=1.0, top_radius=1.0, height=2.0):
    vertices = []
    half_height = height * 0.5
    for y, radius in ((-half_height, bottom_radius), (half_height, top_radius)):
        for step in range(segments):
            angle = math.tau * step / segments
            vertices.append((math.cos(angle) * radius, y, math.sin(angle) * radius))
    vertices.extend(((0.0, -half_height, 0.0), (0.0, half_height, 0.0)))
    bottom_center, top_center = segments * 2, segments * 2 + 1
    faces = []
    for step in range(segments):
        nxt = (step + 1) % segments
        faces.append((step, nxt, segments + nxt, segments + step))
        faces.append((bottom_center, nxt, step))
        faces.append((top_center, segments + step, segments + nxt))
    return tuple(vertices), tuple(faces)


def torus(major_radius=1.0, minor_radius=0.08, major_steps=24, minor_steps=6):
    vertices = []
    for major in range(major_steps):
        u = math.tau * major / major_steps
        for minor in range(minor_steps):
            v = math.tau * minor / minor_steps
            radius = major_radius + minor_radius * math.cos(v)
            vertices.append((radius * math.cos(u), minor_radius * math.sin(v), radius * math.sin(u)))
    faces = []
    for major in range(major_steps):
        next_major = (major + 1) % major_steps
        for minor in range(minor_steps):
            next_minor = (minor + 1) % minor_steps
            faces.append(
                (
                    major * minor_steps + minor,
                    next_major * minor_steps + minor,
                    next_major * minor_steps + next_minor,
                    major * minor_steps + next_minor,
                )
            )
    return tuple(vertices), tuple(faces)


def triangular_prism(width=2.0, height=2.0, depth=1.0):
    half_width, half_depth = width * 0.5, depth * 0.5
    vertices = (
        (-half_width, 0.0, -half_depth),
        (half_width, 0.0, -half_depth),
        (0.0, height, -half_depth),
        (-half_width, 0.0, half_depth),
        (half_width, 0.0, half_depth),
        (0.0, height, half_depth),
    )
    faces = ((0, 2, 1), (3, 4, 5), (0, 3, 5, 2), (1, 2, 5, 4), (0, 1, 4, 3))
    return vertices, faces


def crescent(steps=20, cutout_offset=0.45, cutout_radius=0.95):
    """A strip of convex quads forming a crescent in the XY plane."""
    intersection_x = (1.0 - cutout_radius**2 + cutout_offset**2) / (2.0 * cutout_offset)
    intersection_y = math.sqrt(1.0 - intersection_x**2)
    outer_start = math.atan2(intersection_y, intersection_x)
    inner_start = math.atan2(intersection_y, intersection_x - cutout_offset)
    vertices = []
    for step in range(steps + 1):
        progress = step / steps
        outer_angle = outer_start + (math.tau - 2.0 * outer_start) * progress
        inner_angle = inner_start + (math.tau - 2.0 * inner_start) * progress
        vertices.append((math.cos(outer_angle), math.sin(outer_angle), 0.0))
        vertices.append(
            (
                cutout_offset + cutout_radius * math.cos(inner_angle),
                cutout_radius * math.sin(inner_angle),
                0.0,
            )
        )
    faces = tuple((step * 2, step * 2 + 1, step * 2 + 3, step * 2 + 2) for step in range(steps))
    return tuple(vertices), faces


def add_box(mesh, location, scale, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(CUBE_VERTICES, CUBE_FACES, location, scale, rotation)


def add_octahedron(mesh, location, scale, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(OCTAHEDRON_VERTICES, OCTAHEDRON_FACES, location, scale, rotation)


def build_scene():
    frustum_16 = radial_frustum(16)
    frustum_12 = radial_frustum(12)
    cone_12 = radial_frustum(12, bottom_radius=1.0, top_radius=0.0, height=2.0)
    ring = torus()
    mountain = triangular_prism()

    # The moon and tiny stars form a warm, distant constellation layer.
    moon = GardenMesh()
    moon.add_xyz(*crescent(), (-2.75, 5.25, -7.4), (2.35, 2.35, 2.35))
    for x, y, size in (
        (-6.0, 6.2, 0.13),
        (-4.9, 3.9, 0.10),
        (0.1, 7.0, 0.09),
        (2.0, 5.9, 0.12),
        (4.3, 6.8, 0.08),
        (6.2, 4.6, 0.11),
    ):
        add_octahedron(moon, (x, y, -7.0), (size, size * 1.8, size))

    mountains = GardenMesh()
    for location, scale in (
        ((-5.6, -0.1, -5.5), (2.8, 2.9, 1.0)),
        ((-2.1, -0.2, -5.2), (2.4, 2.1, 1.0)),
        ((1.1, -0.25, -5.4), (3.0, 3.2, 1.0)),
        ((5.0, -0.1, -5.1), (3.1, 2.5, 1.0)),
    ):
        mountains.add_xyz(*mountain, location, scale)

    # A faceted island gives the composition a strong silhouette and visible depth.
    underside = GardenMesh()
    underside.add_xyz(*radial_frustum(18, 0.18, 1.0, 2.0), (0.0, -1.58, 0.0), (4.65, 1.0, 4.0))
    for x, y, z, scale in (
        (-3.5, -2.15, 0.4, (0.38, 1.4, 0.42)),
        (-1.8, -2.65, -0.3, (0.28, 1.7, 0.30)),
        (1.8, -2.45, 0.2, (0.32, 1.55, 0.34)),
        (3.5, -2.0, -0.5, (0.35, 1.2, 0.38)),
    ):
        underside.add_xyz(*cone_12, (x, y, z), scale)

    garden = GardenMesh()
    garden.add_xyz(*radial_frustum(18, 0.90, 1.0, 0.55), (0.0, -0.30, 0.0), (4.85, 1.0, 4.15))
    ivory = GardenMesh()
    ivory.add_xyz(*radial_frustum(16, 1.0, 0.78, 2.0), (0.0, 1.15, 0.0), (1.25, 1.0, 1.25))
    ivory.add_xyz(*radial_frustum(12, 0.60, 0.43, 2.0), (0.0, 3.02, 0.0), (1.0, 1.0, 1.0))
    # Four slim garden pylons echo the central tower.
    for x, z, height in ((-3.15, 0.65, 1.3), (3.10, 0.40, 1.55), (-2.70, -1.65, 1.05), (2.65, -1.85, 1.15)):
        ivory.add_xyz(*radial_frustum(8, 1.0, 0.72, 2.0), (x, height * 0.5, z), (0.17, height * 0.5, 0.17))

    shadow = GardenMesh()
    shadow.add_xyz(*radial_frustum(16, 1.0, 1.0, 0.34), (0.0, 0.28, 0.0), (1.62, 1.0, 1.62))
    shadow.add_xyz(*radial_frustum(16, 1.0, 1.0, 0.28), (0.0, 2.18, 0.0), (0.88, 1.0, 0.88))
    shadow.add_xyz(*radial_frustum(12, 0.75, 0.0, 2.0), (0.0, 4.72, 0.0), (0.72, 0.62, 0.72))
    # Observatory door and slit windows are shallow geometry, not decals.
    add_box(shadow, (0.0, 1.05, 1.255), (0.38, 0.72, 0.055))
    for y in (2.75, 3.22, 3.69):
        add_box(shadow, (0.0, y, 0.455), (0.13, 0.16, 0.035))

    gold = GardenMesh()
    gold.add_xyz(*ring, (0.0, 2.27, 0.0), (1.08, 1.0, 1.08))
    gold.add_xyz(*ring, (0.0, 4.17, 0.0), (0.62, 0.75, 0.62), (0.18, 0.0, 0.0))
    gold.add_xyz(*ring, (0.0, 4.68, 0.0), (0.42, 0.65, 0.42), (-0.18, 0.0, 0.0))
    # Contrasting stepping stones lead the eye from the foreground to the beacon.
    for step, (x, z) in enumerate(((-0.85, 2.95), (-0.58, 2.35), (-0.30, 1.78), (-0.12, 1.25))):
        add_box(gold, (x, 0.08 + step * 0.035, z), (0.38, 0.07, 0.28), (0.0, -0.10 * step, 0.0))
    for x, z in ((-3.15, 0.65), (3.10, 0.40), (-2.70, -1.65), (2.65, -1.85)):
        add_octahedron(gold, (x, 1.63 if x > 0 else 1.42, z), (0.25, 0.38, 0.25))

    cyan = GardenMesh()
    add_octahedron(cyan, (0.0, 4.12, 0.0), (0.55, 0.82, 0.55), (0.0, 0.0, math.pi / 4))
    # Tall crystalline plants around the island's rim.
    for location, scale, angle in (
        ((-3.55, 0.66, 1.65), (0.34, 0.88, 0.34), -0.16),
        ((-3.95, 0.48, -0.45), (0.28, 0.67, 0.28), 0.20),
        ((2.85, 0.70, 1.75), (0.35, 0.92, 0.35), 0.12),
        ((3.70, 0.52, -0.65), (0.29, 0.70, 0.29), -0.18),
    ):
        add_octahedron(cyan, location, scale, (0.0, 0.0, angle))

    coral = GardenMesh()
    for location, scale, angle in (
        ((-2.72, 0.47, 2.18), (0.24, 0.57, 0.24), 0.22),
        ((-2.25, 0.38, -2.52), (0.20, 0.44, 0.20), -0.15),
        ((2.05, 0.45, 2.60), (0.26, 0.62, 0.26), -0.18),
        ((3.35, 0.36, -2.12), (0.19, 0.43, 0.19), 0.20),
    ):
        add_octahedron(coral, location, scale, (0.0, 0.0, angle))
    # A tilted orbital ribbon makes the tower read as a signal instrument.
    coral.add_xyz(*ring, (0.0, 4.13, 0.0), (1.18, 1.0, 1.18), (0.0, 0.0, 0.47))

    foliage = GardenMesh()
    for x, z, lean in ((-4.0, 1.05, -0.12), (-3.45, -2.25, 0.16), (3.85, 1.10, 0.14), (3.25, -2.40, -0.15)):
        add_box(foliage, (x, 0.38, z), (0.075, 0.42, 0.075), (0.0, 0.0, lean))
        add_octahedron(foliage, (x + lean * 0.35, 0.95, z), (0.45, 0.54, 0.45))
        add_octahedron(foliage, (x - 0.25, 0.68, z + 0.06), (0.30, 0.36, 0.30))

    return (
        (901, (247, 210, 119, 255), moon),
        (903, (34, 28, 76, 255), mountains),
        (904, (31, 45, 86, 255), underside),
        (905, (40, 110, 105, 255), garden),
        (906, (229, 218, 187, 255), ivory),
        (907, (23, 27, 54, 255), shadow),
        (908, (238, 173, 73, 255), gold),
        (909, (60, 218, 207, 255), cyan),
        (910, (233, 82, 128, 255), coral),
        (911, (54, 139, 143, 255), foliage),
    )


def populate(client):
    client.stop()
    client.clear()
    client.camera((9.8, 6.6, 16.5), (0.0, 1.35, 0.0), 47.0)
    for mesh_id, color, mesh in build_scene():
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(20_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start(BACKGROUND)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=2.0)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("bld/draw3d-captures/celestial-signal-garden.png"),
    )
    args = parser.parse_args()

    client = Draw3dClient(args.host)
    try:
        populate(client)
        time.sleep(args.settle)
        output, image_format, width, height, image = client.render(args.output)
        mesh_count, instance_count, vertices, edges, faces, mesh_bytes = client.stats()
        print(
            f"scene meshes={mesh_count} instances={instance_count} vertices={vertices} "
            f"edges={edges} faces={faces} mesh_bytes={mesh_bytes}"
        )
        print(
            f"capture format={image_format} size={width}x{height} bytes={len(image)} "
            f"sha256={hashlib.sha256(image).hexdigest()} path={output}"
        )
        if image_format != 2 or (width, height) != (512, 512):
            raise RuntimeError("live scene did not return the expected 512x512 PNG")
    finally:
        client.close()


if __name__ == "__main__":
    main()
