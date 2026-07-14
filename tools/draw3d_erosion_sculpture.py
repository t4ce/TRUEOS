#!/usr/bin/env python3
"""Paint a faceted eroded specimen on the TRUEOS draw3d screen."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh, radial_frustum
from draw3d_house_demo import CUBE_FACES, CUBE_VERTICES, Draw3dClient, OCTAHEDRON_FACES, OCTAHEDRON_VERTICES


BACKGROUND = (5, 7, 12, 255)


def add_box(mesh, location, scale, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(CUBE_VERTICES, CUBE_FACES, location, scale, rotation)


def add_diamond(mesh, location, scale, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(OCTAHEDRON_VERTICES, OCTAHEDRON_FACES, location, scale, rotation)


def blob_surface(segments=28, rings=13, scale=(2.7, 3.3, 1.72), rotation=0.0):
    vertices = []
    for ring in range(rings + 1):
        phi = math.pi * ring / rings
        sin_phi, cos_phi = math.sin(phi), math.cos(phi)
        for segment in range(segments):
            theta = math.tau * segment / segments + rotation
            swell = 1.0 + 0.16 * math.sin(3.0 * theta + 0.65 * math.sin(phi)) * sin_phi
            swell += 0.10 * math.cos(5.0 * theta - 1.2 * phi) * sin_phi * sin_phi
            x = scale[0] * swell * sin_phi * math.cos(theta)
            y = scale[1] * swell * cos_phi
            z = scale[2] * swell * sin_phi * math.sin(theta)
            vertices.append((x, y, z))
    faces = []
    for ring in range(rings):
        for segment in range(segments):
            nxt = (segment + 1) % segments
            faces.append((ring * segments + segment, (ring + 1) * segments + segment, (ring + 1) * segments + nxt, ring * segments + nxt))
    return tuple(vertices), tuple(faces)


def split_surface(vertices, faces, buckets=4):
    meshes = [GardenMesh() for _ in range(buckets)]
    for face in faces:
        center = tuple(sum(vertices[index][axis] for index in face) / len(face) for axis in range(3))
        # Front-facing and high-facing facets receive lighter material layers.
        score = 0.62 * (center[2] / 1.72) + 0.25 * (center[1] / 3.3) - 0.18 * (center[0] / 2.7)
        bucket = max(0, min(buckets - 1, int((score + 1.0) * buckets * 0.5)))
        mesh = meshes[bucket]
        base = len(mesh.vertices)
        mesh.vertices.extend(vertices[index] for index in face)
        mesh.faces.append(tuple(base + offset for offset in range(len(face))))
    return tuple(meshes)


def add_strip(mesh, points, width, z):
    for first, second in zip(points, points[1:]):
        x1, y1 = first
        x2, y2 = second
        dx, dy = x2 - x1, y2 - y1
        length = math.hypot(dx, dy)
        if length < 0.001:
            continue
        nx, ny = -dy / length * width * 0.5, dx / length * width * 0.5
        base = len(mesh.vertices)
        mesh.vertices.extend(((x1 + nx, y1 + ny, z), (x2 + nx, y2 + ny, z), (x2 - nx, y2 - ny, z), (x1 - nx, y1 - ny, z)))
        mesh.faces.append((base, base + 1, base + 2, base + 3))


def build_scene():
    back = GardenMesh()
    add_box(back, (0.0, 2.4, -6.6), (7.8, 4.8, 0.12))
    add_box(back, (-5.8, 2.7, -6.35), (0.18, 4.9, 0.18), (0.0, 0.0, -0.08))
    add_box(back, (5.8, 3.0, -6.30), (0.18, 4.4, 0.18), (0.0, 0.0, 0.08))

    floor = GardenMesh()
    floor.add_xyz(*radial_frustum(8, 1.0, 0.88, 0.34), (0.0, -1.58, 0.0), (5.6, 1.0, 3.5))
    floor.add_xyz(*radial_frustum(8, 0.82, 0.62, 0.18), (0.0, -1.27, 0.0), (4.6, 1.0, 2.6))
    add_box(floor, (0.0, -0.94, 0.95), (2.9, 0.04, 0.04))

    vertices, faces = blob_surface()
    shades = split_surface(vertices, faces)
    # The crack is a separate foreground incision, not a texture: it is all geometry.
    crack = GardenMesh()
    crack_points = [(-1.35, 4.55), (-0.92, 3.85), (-1.12, 3.12), (-0.62, 2.55), (-0.78, 1.92), (-0.22, 1.30), (-0.35, 0.70)]
    add_strip(crack, crack_points, 0.14, 1.66)
    add_strip(crack, [(-0.78, 1.92), (0.02, 2.20), (0.50, 2.80), (0.72, 3.45)], 0.10, 1.67)
    add_strip(crack, [(-1.12, 3.12), (-1.78, 3.45), (-2.12, 4.05)], 0.08, 1.65)

    plates = GardenMesh()
    # A handful of shallow planes make the front surface read as cut stone rather than a smooth blob.
    for location, scale, rotation in (
        ((-1.45, 3.75, 1.72), (0.44, 0.26, 0.05), -0.28),
        ((-0.40, 4.15, 1.68), (0.36, 0.20, 0.05), 0.12),
        ((1.10, 2.90, 1.70), (0.30, 0.48, 0.05), -0.30),
        ((-1.25, 1.45, 1.70), (0.28, 0.42, 0.05), 0.34),
    ):
        add_box(plates, location, scale, (0.0, 0.0, rotation))

    relic = GardenMesh()
    add_diamond(relic, (0.10, 3.05, 1.88), (0.22, 0.38, 0.08), (0.0, 0.0, math.pi / 4))
    add_diamond(relic, (0.10, 3.05, 2.06), (0.08, 0.18, 0.04), (0.0, 0.0, math.pi / 4))
    add_box(relic, (0.10, 3.05, 1.95), (0.03, 0.62, 0.03), (0.0, 0.0, math.pi / 4))

    return (
        (2001, (16, 23, 38, 255), back),
        (2002, (43, 48, 54, 255), floor),
        (2003, (77, 82, 88, 255), shades[0]),
        (2004, (120, 121, 116, 255), shades[1]),
        (2005, (171, 166, 148, 255), shades[2]),
        (2006, (219, 207, 178, 255), shades[3]),
        (2007, (151, 52, 48, 255), crack),
        (2008, (45, 123, 115, 255), plates),
        (2009, (225, 171, 75, 255), relic),
    )


def populate(client):
    client.stop()
    client.clear()
    client.camera((8.7, 5.4, 17.4), (0.0, 2.40, 0.20), 44.0)
    for mesh_id, color, mesh in build_scene():
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(110_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start(BACKGROUND)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=2.0)
    parser.add_argument("--output", type=Path, default=Path("bld/draw3d-captures/erosion-sculpture-live.png"))
    args = parser.parse_args()

    client = Draw3dClient(args.host)
    try:
        populate(client)
        time.sleep(args.settle)
        output, image_format, width, height, image = client.render(args.output)
        stats = client.stats()
        print(
            f"scene meshes={stats[0]} instances={stats[1]} vertices={stats[2]} "
            f"edges={stats[3]} faces={stats[4]} mesh_bytes={stats[5]}"
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
