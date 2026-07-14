#!/usr/bin/env python3
"""Paint a faceted cutaway monument with suspended fragments and survey marks."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh
from draw3d_house_demo import Draw3dClient
from draw3d_signal_atlas import add_diamond, add_polygon, add_polyline, add_ring


BACKGROUND = (7, 8, 12, 255)


def outline(cx, cy, rx, ry, phase, count=64):
    points = []
    for index in range(count + 1):
        t = math.tau * index / count
        radial = 1.0 + 0.12 * math.sin(2.0 * t + phase) + 0.075 * math.sin(5.0 * t - phase * 0.8)
        x = cx + rx * radial * math.cos(t) + 0.18 * math.sin(3.0 * t + phase)
        y = cy + ry * radial * math.sin(t) + 0.16 * math.cos(4.0 * t - phase)
        points.append((x, y))
    return points


def add_facet_group(groups, points, center, z, palette_shift=0):
    cx, cy = center
    for index, (first, second) in enumerate(zip(points, points[1:])):
        # Low-frequency material bands keep the facets intentional rather than noisy.
        band = int(2.0 + 2.0 * math.sin(index * 0.47 + palette_shift) + 1.2 * math.sin(index * 0.13 - 1.4))
        band = max(0, min(len(groups) - 1, band))
        mesh = groups[band]
        base = len(mesh.vertices)
        mesh.vertices.extend(((cx, cy, z), (first[0], first[1], z), (second[0], second[1], z)))
        mesh.faces.append((base, base + 1, base + 2))


def add_ribbon(mesh, points, thickness, z):
    for first, second in zip(points, points[1:]):
        x1, y1 = first
        x2, y2 = second
        dx, dy = x2 - x1, y2 - y1
        length = math.hypot(dx, dy) or 1.0
        nx, ny = -dy / length * thickness * 0.5, dx / length * thickness * 0.5
        base = len(mesh.vertices)
        mesh.vertices.extend(((x1 + nx, y1 + ny, z), (x2 + nx, y2 + ny, z), (x2 - nx, y2 - ny, z), (x1 - nx, y1 - ny, z)))
        mesh.faces.append((base, base + 1, base + 2, base + 3))


def contour(cx, cy, rx, ry, phase, count=42):
    points = []
    for index in range(count + 1):
        t = math.tau * index / count
        r = 1.0 + 0.065 * math.sin(3.0 * t + phase) + 0.025 * math.sin(8.0 * t)
        points.append((cx + rx * r * math.cos(t), cy + ry * r * math.sin(t)))
    return points


def build_scene():
    # A dark paper-like field with a few registration arcs gives the object a world.
    field = GardenMesh()
    add_polygon(field, [(-6.55, -4.55), (5.95, -4.55), (6.35, 4.42), (-5.75, 4.42)], -2.2)
    for cx, cy, rx, ry, start, end in ((-2.7, 2.7, 2.8, 1.7, 0.2, 3.9), (3.3, -2.7, 2.3, 1.2, 2.1, 6.0), (-3.1, -2.8, 1.5, 0.8, 0.4, 5.4)):
        add_ring(field, cx, cy, (rx, ry), (rx - 0.075, ry - 0.075), -2.10, start, end, 48, 0.04)
    for y in (-3.8, -2.9, 2.85, 3.55):
        add_polyline(field, [(-6.0, y), (-3.9, y + 0.10), (-1.4, y - 0.06), (1.5, y + 0.08), (4.15, y - 0.05), (6.0, y + 0.09)], 0.04, -2.08)

    survey = GardenMesh()
    for x in (-5.15, -4.72, 4.2, 4.75):
        add_polyline(survey, [(x, -4.15), (x + 0.16, -2.2), (x - 0.07, -0.8)], 0.05, -1.55)
    for x, y, angle in ((-4.6, 3.55, 0.1), (-3.9, 3.82, -0.2), (4.05, 3.58, 0.16), (4.8, 3.3, -0.12), (-5.05, -3.42, 0.1), (5.1, -3.6, -0.1)):
        add_diamond(survey, x, y, 0.18, 0.30, -1.46, angle)

    shadow = GardenMesh()
    add_polygon(shadow, [(-4.6, -3.62), (3.9, -3.62), (4.55, -3.15), (2.7, -2.72), (-3.55, -2.9)], -1.18)
    add_ring(shadow, -0.05, -2.98, (3.8, 0.36), (3.5, 0.22), -1.10, 0.0, math.tau, 64, 0.04)

    # Main silhouette: a leaning, weathered monolith with enough asymmetry to avoid a logo shape.
    body_outline = outline(-0.10, 0.05, 2.55, 3.70, 0.8, 78)
    body_groups = [GardenMesh() for _ in range(6)]
    add_facet_group(body_groups, body_outline, (-0.10, 0.05), -0.86, 0.9)

    shoulder = GardenMesh()
    add_polygon(shoulder, [(-2.4, 2.3), (-1.4, 3.58), (0.1, 3.95), (1.65, 3.35), (2.6, 2.4), (1.55, 2.7), (-0.2, 2.42)], -0.30)

    left_fragment_outline = outline(-3.55, 0.35, 1.22, 1.85, 1.7, 36)
    right_fragment_outline = outline(3.35, -0.15, 1.15, 1.62, 2.3, 36)
    fragment_a = [GardenMesh() for _ in range(4)]
    fragment_b = [GardenMesh() for _ in range(4)]
    add_facet_group(fragment_a, left_fragment_outline, (-3.55, 0.35), -0.64, 1.7)
    add_facet_group(fragment_b, right_fragment_outline, (3.35, -0.15), -0.58, 2.3)

    cut = GardenMesh()
    # The chasm is not centered: it turns the sculpture into a cross-section.
    add_polygon(cut, [(-0.70, 3.15), (-0.18, 2.15), (-0.52, 0.90), (0.02, -0.35), (-0.34, -1.55), (0.08, -3.3), (0.58, -2.30), (0.45, -0.85), (0.92, 0.28), (0.52, 1.55), (0.98, 2.78)], 0.02)
    add_polyline(cut, [(-0.47, 3.18), (-0.06, 2.15), (-0.33, 0.92), (0.20, -0.32), (-0.13, -1.55), (0.27, -3.2)], 0.075, 0.08)

    strata = GardenMesh()
    for index, (cx, cy, rx, ry) in enumerate(((-0.30, 2.25, 1.20, 0.48), (0.38, 1.22, 1.48, 0.52), (-0.10, 0.18, 1.42, 0.42), (0.26, -1.05, 1.26, 0.42), (-0.28, -2.05, 0.92, 0.34))):
        add_polyline(strata, contour(cx, cy, rx, ry, index * 0.65), 0.065 if index in (1, 3) else 0.045, 0.16)
    for x, y, length, angle in ((-1.6, 2.85, 0.55, -0.22), (1.12, 2.05, 0.35, 0.2), (-1.3, -0.68, 0.42, 0.16), (1.0, -2.2, 0.52, -0.12)):
        dx, dy = math.cos(angle) * length, math.sin(angle) * length
        add_ribbon(strata, [(x, y), (x + dx, y + dy)], 0.09, 0.18)

    seams = GardenMesh()
    # Deliberate seams on the fragments and shoulder make the material legible.
    for points in (
        [(-4.35, 0.05), (-3.85, 0.32), (-3.42, 0.18), (-2.82, 0.48)],
        [(-4.0, -0.86), (-3.62, -0.62), (-3.23, -0.72), (-2.88, -0.38)],
        [(2.8, 0.18), (3.25, -0.08), (3.68, 0.12), (4.0, -0.22)],
        [(2.9, -0.8), (3.4, -0.62), (3.82, -0.94)],
        [(-1.6, 3.18), (-0.7, 3.38), (0.16, 3.08), (1.05, 3.22)],
    ):
        add_ribbon(seams, points, 0.055, 0.27)

    rust = GardenMesh()
    for points in (
        [(-4.62, 1.52), (-3.95, 1.14), (-3.25, 1.3), (-2.55, 1.02)],
        [(-4.25, -1.45), (-3.62, -1.08), (-3.0, -1.22), (-2.34, -0.92)],
        [(2.38, 1.72), (3.02, 1.42), (3.62, 1.56), (4.2, 1.24)],
        [(2.45, -1.56), (3.05, -1.3), (3.56, -1.48), (4.0, -1.18)],
    ):
        add_ribbon(rust, points, 0.12, 0.48)

    ivory = GardenMesh()
    for points in (
        [(-5.0, 2.45), (-4.5, 2.7), (-4.05, 2.54)],
        [(-4.95, -2.55), (-4.38, -2.35), (-3.95, -2.52)],
        [(4.1, 2.55), (4.62, 2.75), (5.12, 2.55)],
        [(4.0, -2.52), (4.55, -2.32), (5.0, -2.52)],
    ):
        add_ribbon(ivory, points, 0.075, 0.67)

    return (
        (2001, (15, 18, 29, 255), field),
        (2002, (103, 112, 137, 255), survey),
        (2003, (20, 22, 30, 255), shadow),
        (2004, (42, 54, 67, 255), body_groups[0]),
        (2005, (55, 70, 79, 255), body_groups[1]),
        (2006, (72, 85, 87, 255), body_groups[2]),
        (2007, (94, 97, 91, 255), body_groups[3]),
        (2008, (128, 118, 98, 255), body_groups[4]),
        (2009, (171, 145, 109, 255), body_groups[5]),
        (2010, (71, 76, 84, 255), shoulder),
        (2011, (53, 63, 72, 255), fragment_a[0]),
        (2012, (76, 82, 85, 255), fragment_a[1]),
        (2013, (118, 109, 92, 255), fragment_a[2]),
        (2014, (154, 128, 98, 255), fragment_a[3]),
        (2015, (48, 58, 69, 255), fragment_b[0]),
        (2016, (71, 80, 84, 255), fragment_b[1]),
        (2017, (112, 109, 95, 255), fragment_b[2]),
        (2018, (145, 125, 99, 255), fragment_b[3]),
        (2019, (4, 6, 10, 255), cut),
        (2020, (185, 177, 151, 255), strata),
        (2021, (205, 191, 150, 255), seams),
        (2022, (158, 60, 46, 255), rust),
        (2023, (227, 216, 183, 255), ivory),
    )


def populate(client):
    client.stop()
    client.clear()
    client.camera((0.0, 0.0, 16.0), (0.0, 0.0, 0.0), 47.0)
    for mesh_id, color, mesh in build_scene():
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(110_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start(BACKGROUND)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=2.0)
    parser.add_argument("--output", type=Path, default=Path("bld/draw3d-captures/sundered-monument-live.png"))
    args = parser.parse_args()
    client = Draw3dClient(args.host)
    try:
        populate(client)
        time.sleep(args.settle)
        output, image_format, width, height, image = client.render(args.output)
        stats = client.stats()
        print(f"scene meshes={stats[0]} instances={stats[1]} vertices={stats[2]} edges={stats[3]} faces={stats[4]} mesh_bytes={stats[5]}")
        print(f"capture format={image_format} size={width}x{height} bytes={len(image)} sha256={hashlib.sha256(image).hexdigest()} path={output}")
        if image_format != 2 or (width, height) != (512, 512):
            raise RuntimeError("live scene did not return the expected 512x512 PNG")
    finally:
        client.close()


if __name__ == "__main__":
    main()
