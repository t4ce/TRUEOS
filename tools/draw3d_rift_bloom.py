#!/usr/bin/env python3
"""Paint a tectonic rift: two mismatched material fields split by a living seam."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh
from draw3d_house_demo import Draw3dClient
from draw3d_signal_atlas import add_diamond, add_polygon, add_polyline, add_ring


BACKGROUND = (6, 8, 15, 255)
SCREEN_TILT = math.radians(-7.0)


def facet_group(groups, points, center, z, phase):
    cx, cy = center
    for index, (first, second) in enumerate(zip(points, points[1:])):
        band = int(2.3 + 2.1 * math.sin(index * 0.42 + phase) + 1.0 * math.sin(index * 0.16 - phase))
        band = max(0, min(len(groups) - 1, band))
        mesh = groups[band]
        base = len(mesh.vertices)
        mesh.vertices.extend(((cx, cy, z), (first[0], first[1], z), (second[0], second[1], z)))
        mesh.faces.append((base, base + 1, base + 2))


def irregular_field(cx, cy, rx, ry, phase, count=52):
    points = []
    for index in range(count + 1):
        t = math.tau * index / count
        r = 1.0 + 0.08 * math.sin(2.0 * t + phase) + 0.06 * math.sin(5.0 * t - phase)
        points.append((cx + rx * r * math.cos(t) + 0.18 * math.sin(3.0 * t), cy + ry * r * math.sin(t)))
    return points


def flow_line(start, end, bend, phase, count=44):
    sx, sy = start
    ex, ey = end
    dx, dy = ex - sx, ey - sy
    length = math.hypot(dx, dy) or 1.0
    nx, ny = -dy / length, dx / length
    points = []
    for index in range(count):
        u = index / (count - 1)
        offset = bend * math.sin(math.pi * u) + 0.08 * math.sin(u * math.tau * 2.2 + phase)
        points.append((sx + dx * u + nx * offset, sy + dy * u + ny * offset))
    return points


def add_ribbon(mesh, points, width, z):
    for first, second in zip(points, points[1:]):
        x1, y1 = first
        x2, y2 = second
        dx, dy = x2 - x1, y2 - y1
        length = math.hypot(dx, dy) or 1.0
        nx, ny = -dy / length * width * 0.5, dx / length * width * 0.5
        base = len(mesh.vertices)
        mesh.vertices.extend(((x1 + nx, y1 + ny, z), (x2 + nx, y2 + ny, z), (x2 - nx, y2 - ny, z), (x1 - nx, y1 - ny, z)))
        mesh.faces.append((base, base + 1, base + 2, base + 3))


def build_scene():
    field = GardenMesh()
    add_polygon(field, [(-6.6, -4.55), (5.9, -4.55), (6.4, 4.45), (-5.95, 4.45)], -2.6)
    add_ring(field, -3.2, 2.8, (2.65, 1.50), (2.56, 1.41), -2.50, 0.2, 4.5, 58, 0.04)
    add_ring(field, 3.85, -2.3, (2.1, 1.1), (2.01, 1.01), -2.49, 2.0, 5.8, 50, 0.04)

    survey = GardenMesh()
    for y, phase in ((-3.6, 0.2), (-3.0, 1.3), (-2.35, 2.2), (2.55, 0.7), (3.16, 1.9), (3.72, 3.0)):
        points = []
        for index in range(36):
            x = -6.2 + 12.4 * index / 35
            points.append((x, y + 0.14 * math.sin(x * 0.68 + phase) + 0.06 * math.sin(x * 2.3)))
        add_polyline(survey, points, 0.04, -2.18)
    for x, y, angle in ((-5.35, 3.25, 0.1), (-4.8, 3.48, -0.2), (4.35, 3.12, 0.12), (4.95, 2.82, -0.1), (-5.1, -3.45, 0.1), (4.9, -3.35, -0.12)):
        add_diamond(survey, x, y, 0.15, 0.28, -2.1, angle)

    # Two mismatched fields, deliberately not mirror images.
    left_outline = [(-5.95, 3.70), (-4.75, 3.48), (-3.62, 2.82), (-2.95, 1.55), (-3.25, 0.35), (-2.62, -0.72), (-3.42, -1.72), (-4.95, -2.55), (-6.15, -1.35), (-6.2, 1.1)]
    right_outline = [(0.0, 3.72), (1.52, 3.25), (2.65, 2.55), (4.52, 2.25), (5.85, 1.08), (5.6, -0.35), (4.75, -1.15), (4.92, -2.84), (3.28, -3.78), (1.72, -3.22), (0.88, -2.14), (0.52, -0.92), (-0.18, 0.08), (-0.68, 1.42)]
    left_groups = [GardenMesh() for _ in range(6)]
    right_groups = [GardenMesh() for _ in range(6)]
    facet_group(left_groups, left_outline + [left_outline[0]], (-4.65, 0.5), -1.18, 0.8)
    facet_group(right_groups, right_outline + [right_outline[0]], (3.02, -0.18), -1.10, 2.4)

    left_contours = GardenMesh()
    for index, (cx, cy, rx, ry) in enumerate(((-4.45, 1.85, 1.25, 0.68), (-4.62, 0.52, 1.48, 0.72), (-4.6, -0.78, 1.0, 0.48))):
        pts = irregular_field(cx, cy, rx, ry, index * 0.9, 34)
        add_polyline(left_contours, pts, 0.05 + index * 0.012, -0.33)
    right_contours = GardenMesh()
    for index, (cx, cy, rx, ry) in enumerate(((2.85, 2.2, 1.1, 0.52), (3.55, 0.85, 1.48, 0.72), (3.55, -1.45, 1.28, 0.60), (3.42, -2.58, 0.88, 0.38))):
        pts = irregular_field(cx, cy, rx, ry, 0.4 + index * 0.7, 32)
        add_polyline(right_contours, pts, 0.045 + (index % 2) * 0.018, -0.26)

    rift = GardenMesh()
    rift_shape = [(-0.62, 4.52), (-0.10, 3.32), (-0.44, 2.25), (0.12, 1.32), (-0.32, 0.16), (0.18, -0.88), (-0.38, -2.02), (0.20, -3.18), (-0.05, -4.55), (0.72, -4.55), (0.57, -3.26), (0.98, -2.02), (0.64, -0.76), (1.08, 0.24), (0.62, 1.30), (1.12, 2.48), (0.70, 3.55), (1.02, 4.52)]
    add_polygon(rift, rift_shape, 0.05)
    # Broken chalk edges make the seam feel torn rather than like a clean vector cut.
    edge = [(-0.32, 4.48), (0.18, 3.38), (-0.12, 2.25), (0.43, 1.30), (0.02, 0.18), (0.54, -0.88), (-0.02, -2.0), (0.56, -3.2)]
    add_polyline(rift, edge, 0.085, 0.18)

    seam_left = GardenMesh()
    seam_right = GardenMesh()
    for index, (start, end, bend) in enumerate((((-5.8, 2.6), (-0.1, 2.6), 0.42), ((-5.75, 1.62), (0.35, 1.38), -0.32), ((-5.8, 0.25), (0.12, 0.08), 0.30), ((-5.82, -1.18), (0.2, -1.32), -0.28), ((-5.7, -2.45), (0.15, -2.65), 0.35))):
        add_polyline(seam_left, flow_line(start, end, bend, index * 0.8), 0.055 if index != 2 else 0.09, 0.35)
    for index, (start, end, bend) in enumerate((((0.78, 3.1), (5.75, 3.05), -0.28), ((0.62, 1.85), (5.78, 1.5), 0.34), ((0.8, 0.55), (5.8, 0.72), -0.36), ((0.55, -0.75), (5.7, -0.48), 0.30), ((0.75, -2.35), (5.72, -2.62), -0.33))):
        add_polyline(seam_right, flow_line(start, end, bend, 2.2 + index * 0.7), 0.05 if index != 3 else 0.085, 0.38)

    bloom = GardenMesh()
    # The only saturated focus is off-axis, at the lower edge of the rift.
    cx, cy = 1.62, -1.68
    for index in range(9):
        angle = -1.2 + index * 0.27
        length = 1.55 + 0.32 * math.sin(index * 1.4)
        end = (cx + length * math.cos(angle), cy + length * math.sin(angle))
        add_ribbon(bloom, flow_line((cx, cy), end, 0.08 * math.sin(index), 0.7 + index * 0.4, 18), 0.10 if index % 3 else 0.15, 0.58)
    add_ring(bloom, cx, cy, (0.68, 0.48), (0.56, 0.36), 0.64, 0.15, 5.4, 44, 0.08)
    add_diamond(bloom, cx, cy, 0.18, 0.30, 0.68, 0.2)

    marks = GardenMesh()
    for x, y, angle in ((-5.25, 2.62, 0.2), (-4.85, -2.7, -0.18), (5.0, 2.65, -0.2), (5.3, -2.95, 0.16), (2.15, 3.75, 0.08), (2.5, -3.62, -0.12)):
        add_diamond(marks, x, y, 0.12, 0.22, 0.74, angle)
    for x, y, angle in ((-5.0, 3.95, 0.1), (4.5, 3.9, -0.1), (-5.1, -3.9, -0.1), (4.85, -3.88, 0.1)):
        add_polyline(marks, [(x, y), (x + 0.34 * math.cos(angle), y + 0.34 * math.sin(angle))], 0.06, 0.72)

    return (
        (2201, (15, 23, 40, 255), field),
        (2202, (44, 65, 92, 255), survey),
        (2203, (45, 55, 72, 255), left_groups[0]),
        (2204, (54, 65, 83, 255), left_groups[1]),
        (2205, (72, 77, 86, 255), left_groups[2]),
        (2206, (109, 94, 88, 255), left_groups[3]),
        (2207, (146, 103, 82, 255), left_groups[4]),
        (2208, (171, 117, 84, 255), left_groups[5]),
        (2209, (29, 46, 76, 255), right_groups[0]),
        (2210, (39, 63, 91, 255), right_groups[1]),
        (2211, (53, 81, 101, 255), right_groups[2]),
        (2212, (72, 103, 112, 255), right_groups[3]),
        (2213, (112, 121, 116, 255), right_groups[4]),
        (2214, (146, 137, 112, 255), right_groups[5]),
        (2215, (65, 69, 83, 255), left_contours),
        (2216, (73, 97, 108, 255), right_contours),
        (2217, (5, 7, 13, 255), rift),
        (2218, (207, 194, 158, 255), seam_left),
        (2219, (207, 194, 158, 255), seam_right),
        (2220, (193, 63, 85, 255), bloom),
        (2221, (224, 174, 69, 255), marks),
    )


def populate(client):
    client.stop()
    client.clear()
    client.camera((0.0, 0.0, 16.0), (0.0, 0.0, 0.0), 42.0)
    for mesh_id, color, mesh in build_scene():
        sin_t, cos_t = math.sin(SCREEN_TILT), math.cos(SCREEN_TILT)
        for index, (x, y, z) in enumerate(mesh.vertices):
            mesh.vertices[index] = (x * cos_t - y * sin_t, x * sin_t + y * cos_t, z)
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(130_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start(BACKGROUND)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=2.0)
    parser.add_argument("--width", type=int, default=2560)
    parser.add_argument("--height", type=int, default=1440)
    parser.add_argument("--output", type=Path, default=Path("bld/draw3d-captures/rift-bloom-live.png"))
    args = parser.parse_args()
    client = Draw3dClient(args.host)
    try:
        populate(client)
        time.sleep(args.settle)
        output, image_format, width, height, image = client.render(args.output)
        stats = client.stats()
        print(f"scene meshes={stats[0]} instances={stats[1]} vertices={stats[2]} edges={stats[3]} faces={stats[4]} mesh_bytes={stats[5]}")
        print(f"capture format={image_format} size={width}x{height} bytes={len(image)} sha256={hashlib.sha256(image).hexdigest()} path={output}")
        if image_format != 2 or (width, height) != (args.width, args.height):
            raise RuntimeError(f"live scene did not return the expected {args.width}x{args.height} PNG")
    finally:
        client.close()


if __name__ == "__main__":
    main()
