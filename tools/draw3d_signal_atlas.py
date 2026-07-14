#!/usr/bin/env python3
"""Paint a layered generative signal atlas on the TRUEOS draw3d screen."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh
from draw3d_house_demo import Draw3dClient


BACKGROUND = (6, 8, 16, 255)


def layer_point(x, y, z):
    return (x, y, z)


def add_quad(mesh, points):
    base = len(mesh.vertices)
    mesh.vertices.extend(points)
    mesh.faces.append((base, base + 1, base + 2, base + 3))


def add_polygon(mesh, points, z):
    """Add a convex polygon as a fan; all atlas silhouettes are deliberately convex."""
    base = len(mesh.vertices)
    mesh.vertices.extend((x, y, z) for x, y in points)
    for index in range(1, len(points) - 1):
        mesh.faces.append((base, base + index, base + index + 1))


def add_polyline(mesh, points, thickness, z):
    for first, second in zip(points, points[1:]):
        x1, y1 = first
        x2, y2 = second
        dx, dy = x2 - x1, y2 - y1
        length = math.hypot(dx, dy)
        if length < 0.001:
            continue
        nx, ny = -dy / length * thickness * 0.5, dx / length * thickness * 0.5
        add_quad(
            mesh,
            (
                layer_point(x1 + nx, y1 + ny, z),
                layer_point(x2 + nx, y2 + ny, z),
                layer_point(x2 - nx, y2 - ny, z),
                layer_point(x1 - nx, y1 - ny, z),
            ),
        )


def ellipse_points(cx, cy, rx, ry, count=48, phase=0.0, wobble=0.0):
    points = []
    for index in range(count + 1):
        t = math.tau * index / count + phase
        radial = 1.0 + wobble * math.sin(3.0 * t + 0.8) + wobble * 0.45 * math.sin(7.0 * t)
        points.append((cx + rx * radial * math.cos(t), cy + ry * radial * math.sin(t)))
    return points


def add_ring(mesh, cx, cy, outer, inner, z, start=0.0, end=math.tau, segments=64, wobble=0.0):
    outer_rx, outer_ry = outer
    inner_rx, inner_ry = inner
    for index in range(segments):
        t1 = start + (end - start) * index / segments
        t2 = start + (end - start) * (index + 1) / segments
        wobble1 = 1.0 + wobble * math.sin(3.0 * t1 + 0.8)
        wobble2 = 1.0 + wobble * math.sin(3.0 * t2 + 0.8)
        add_quad(
            mesh,
            (
                (cx + outer_rx * wobble1 * math.cos(t1), cy + outer_ry * wobble1 * math.sin(t1), z),
                (cx + outer_rx * wobble2 * math.cos(t2), cy + outer_ry * wobble2 * math.sin(t2), z),
                (cx + inner_rx * wobble2 * math.cos(t2), cy + inner_ry * wobble2 * math.sin(t2), z),
                (cx + inner_rx * wobble1 * math.cos(t1), cy + inner_ry * wobble1 * math.sin(t1), z),
            ),
        )


def add_diamond(mesh, cx, cy, width, height, z, rotation=0.0):
    points = []
    for angle in (math.pi * 0.5, math.pi, math.pi * 1.5, 0.0):
        angle += rotation
        radius_x = width if abs(math.cos(angle)) < 0.5 else width * 0.18
        radius_y = height if abs(math.sin(angle)) > 0.5 else height * 0.18
        points.append((cx + radius_x * math.cos(angle), cy + radius_y * math.sin(angle)))
    add_polygon(mesh, points, z)


def build_scene():
    # Deep field: large offset discs and a broken contour mass establish a graphic horizon.
    deep = GardenMesh()
    add_polygon(deep, [(-6.5, -4.6), (5.8, -4.6), (6.5, -2.2), (5.4, 4.6), (-5.7, 4.6), (-6.5, 1.3)], -1.6)
    add_ring(deep, -0.35, 0.45, (5.7, 4.0), (5.45, 3.74), -1.55, math.radians(-30), math.radians(226), 72, 0.025)
    add_ring(deep, 2.9, -2.3, (2.8, 1.6), (2.55, 1.37), -1.54, math.radians(15), math.radians(282), 48, 0.04)

    indigo = GardenMesh()
    # A second contour family is rotated and offset, like a map printed out of registration.
    for index, (cx, cy, rx, ry, phase) in enumerate(
        ((-0.2, 0.2, 4.65, 3.35, 0.1), (0.55, 0.52, 4.0, 2.82, 0.31), (1.0, 0.92, 3.33, 2.35, 0.5))
    ):
        points = ellipse_points(cx, cy, rx, ry, 54, phase, 0.018 + index * 0.006)
        add_polyline(indigo, points, 0.065 + index * 0.012, -1.15 + index * 0.01)
    for y in (-3.4, -2.9, 3.2, 3.65):
        add_polyline(indigo, [(-6.2, y), (-3.4, y + 0.10), (0.2, y - 0.04), (3.9, y + 0.08), (6.2, y - 0.04)], 0.045, -1.08)

    ivory = GardenMesh()
    # Main contour eye: a broken outer band and a sharp inner aperture.
    add_ring(ivory, -0.25, 0.35, (3.95, 2.78), (3.72, 2.55), -0.80, math.radians(-18), math.radians(198), 72, 0.035)
    add_ring(ivory, -0.25, 0.35, (3.18, 2.22), (2.99, 2.04), -0.79, math.radians(18), math.radians(224), 64, 0.028)
    # A clean axial cut gives the ornament a designed interruption.
    add_polyline(ivory, [(-4.4, -0.35), (-2.7, -0.12), (-0.5, -0.18), (1.8, 0.08), (4.6, 0.02)], 0.12, -0.74)

    coral = GardenMesh()
    # Three long ribbons shear across the contour system; the curves are related but not identical.
    for offset, phase, slope in ((-1.9, 0.1, 0.18), (-0.15, 1.1, -0.12), (1.75, 2.2, 0.09)):
        points = []
        for index in range(45):
            x = -5.8 + 11.6 * index / 44
            y = offset + 0.62 * math.sin(x * 0.78 + phase) + slope * x
            points.append((x, y))
        add_polyline(coral, points, 0.16 if offset == -0.15 else 0.095, -0.40)
    add_ring(coral, -0.25, 0.35, (2.32, 1.64), (2.12, 1.46), -0.38, math.radians(70), math.radians(318), 52, 0.04)

    teal = GardenMesh()
    # Teal contour fragments are deliberately interrupted and offset from the ivory bands.
    for index, (cx, cy, rx, ry, start, end) in enumerate(
        ((-0.15, 0.3, 2.78, 1.98, -0.15, 2.35), (0.45, 0.35, 2.38, 1.69, 0.45, 3.55), (0.65, 0.48, 1.78, 1.27, 1.25, 5.25))
    ):
        add_ring(teal, cx, cy, (rx, ry), (rx - 0.10, ry - 0.10), 0.0, start, end, 42, 0.035)
    for x, y, width, height, angle in (
        (-3.25, 2.30, 0.30, 0.72, -0.3),
        (2.65, 2.56, 0.26, 0.62, 0.2),
        (-2.75, -2.02, 0.24, 0.52, 0.4),
        (3.45, -1.42, 0.30, 0.68, -0.2),
    ):
        add_diamond(teal, x, y, width, height, 0.04, angle)

    gold = GardenMesh()
    # Registration points turn the center into a measured instrument rather than a logo.
    for angle, radius, size in (
        (0.16, 3.45, 0.22),
        (0.95, 3.25, 0.16),
        (2.18, 3.52, 0.20),
        (3.42, 3.10, 0.16),
        (4.44, 3.62, 0.21),
        (5.38, 3.18, 0.15),
    ):
        add_diamond(gold, -0.25 + radius * math.cos(angle), 0.35 + radius * 0.72 * math.sin(angle), size, size * 1.65, 0.36, angle)
    for x in (-4.8, -3.9, 3.65, 4.55):
        add_polyline(gold, [(x, -4.0), (x + 0.32, -4.0)], 0.07, 0.34)

    void = GardenMesh()
    # The central void is the only filled foreground shape; it gives the linework a visual anchor.
    void_points = ellipse_points(-0.25, 0.35, 1.12, 0.84, 36, 0.08, 0.03)
    add_polygon(void, void_points[:-1], 0.58)
    add_ring(void, -0.25, 0.35, (1.42, 1.08), (1.29, 0.95), 0.60, 0.0, math.tau, 40, 0.02)

    white = GardenMesh()
    for x, y, length, angle in (
        (-5.15, 2.95, 0.52, 0.12),
        (-4.65, 3.42, 0.34, -0.10),
        (4.18, 2.95, 0.48, -0.14),
        (4.85, 3.48, 0.32, 0.12),
        (-5.2, -2.95, 0.42, -0.08),
        (5.0, -2.75, 0.46, 0.08),
    ):
        add_polyline(white, [(x, y), (x + length * math.cos(angle), y + length * math.sin(angle))], 0.06, 0.84)

    return (
        (1701, (20, 30, 55, 255), deep),
        (1702, (50, 65, 114, 255), indigo),
        (1703, (222, 211, 182, 255), ivory),
        (1704, (193, 70, 69, 255), coral),
        (1705, (49, 181, 166, 255), teal),
        (1706, (235, 174, 74, 255), gold),
        (1707, (5, 7, 14, 255), void),
        (1708, (224, 229, 218, 255), white),
    )


def populate(client):
    client.stop()
    client.clear()
    client.camera((0.0, 0.0, 16.0), (0.0, 0.0, 0.0), 48.0)
    for mesh_id, color, mesh in build_scene():
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(80_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start(BACKGROUND)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=2.0)
    parser.add_argument("--output", type=Path, default=Path("bld/draw3d-captures/signal-atlas-live.png"))
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
