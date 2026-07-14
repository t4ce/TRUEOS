#!/usr/bin/env python3
"""Paint an original graphic scene: a weather system folded around a black aperture."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh
from draw3d_house_demo import Draw3dClient
from draw3d_signal_atlas import add_diamond, add_polygon, add_polyline, add_ring


BACKGROUND = (5, 8, 15, 255)


def ribbon(mesh, points, widths, z):
    """Add a smoothly varying filled ribbon, not a stack of disconnected bars."""
    if len(points) != len(widths):
        raise ValueError("ribbon points and widths must match")
    left = []
    right = []
    for index, ((x, y), width) in enumerate(zip(points, widths)):
        if index == 0:
            dx, dy = points[1][0] - x, points[1][1] - y
        elif index == len(points) - 1:
            dx, dy = x - points[index - 1][0], y - points[index - 1][1]
        else:
            dx = points[index + 1][0] - points[index - 1][0]
            dy = points[index + 1][1] - points[index - 1][1]
        length = math.hypot(dx, dy) or 1.0
        nx, ny = -dy / length * width * 0.5, dx / length * width * 0.5
        left.append((x + nx, y + ny, z))
        right.append((x - nx, y - ny, z))
    base = len(mesh.vertices)
    mesh.vertices.extend(left + right)
    for index in range(len(points) - 1):
        mesh.faces.append((base + index, base + index + 1, base + len(points) + index + 1, base + len(points) + index))


def spline(start, end, bends, count=52):
    points = []
    sx, sy = start
    ex, ey = end
    dx, dy = ex - sx, ey - sy
    for index in range(count):
        u = index / (count - 1)
        x = sx + dx * u
        y = sy + dy * u
        for amount, frequency, phase in bends:
            y += amount * math.sin(math.pi * u * frequency + phase) * math.sin(math.pi * u)
        points.append((x, y))
    return points


def warped_oval(cx, cy, rx, ry, phase, count=72):
    points = []
    for index in range(count + 1):
        t = math.tau * index / count
        radial = 1.0 + 0.08 * math.sin(3.0 * t + phase) + 0.04 * math.sin(7.0 * t - phase)
        points.append((cx + rx * radial * math.cos(t) + 0.12 * math.sin(2.0 * t), cy + ry * radial * math.sin(t)))
    return points


def add_shard(mesh, cx, cy, scale, angle, z):
    points = []
    for x, y in ((0.0, 1.0), (0.46, 0.12), (0.20, -1.0), (-0.38, -0.30), (-0.62, 0.45)):
        px, py = x * scale, y * scale
        px, py = px * math.cos(angle) - py * math.sin(angle), px * math.sin(angle) + py * math.cos(angle)
        points.append((cx + px, cy + py))
    add_polygon(mesh, points, z)


def build_scene():
    # The stage is intentionally an imperfect print field, not a centered card.
    field = GardenMesh()
    add_polygon(field, [(-6.58, -4.58), (5.72, -4.58), (6.45, 4.40), (-5.92, 4.40)], -2.6)
    add_ring(field, -2.8, 2.8, (3.2, 1.7), (3.1, 1.6), -2.50, 0.0, 4.9, 66, 0.03)
    add_ring(field, 3.55, -2.4, (2.25, 1.4), (2.15, 1.3), -2.49, 2.0, math.tau, 54, 0.05)

    contour = GardenMesh()
    for index, (y, amp, phase) in enumerate(((-3.7, 0.18, 0.2), (-3.05, 0.24, 1.2), (-2.48, 0.13, 2.4), (2.55, 0.18, 0.8), (3.14, 0.27, 2.0), (3.67, 0.14, 3.1))):
        points = []
        for step in range(34):
            x = -6.0 + 12.0 * step / 33
            points.append((x, y + amp * math.sin(x * 0.72 + phase) + 0.06 * math.sin(x * 2.2 - phase)))
        add_polyline(contour, points, 0.042, -2.15)
    # A few measured cuts keep the scene from becoming a generic gradient.
    for x, y, length, angle in ((-5.2, 1.8, 0.7, -0.2), (-4.65, 2.15, 0.4, 0.12), (4.2, 1.85, 0.62, 0.16), (4.82, 1.42, 0.34, -0.18), (-4.9, -2.1, 0.45, 0.1), (4.65, -1.8, 0.58, -0.12)):
        dx, dy = math.cos(angle) * length, math.sin(angle) * length
        add_polyline(contour, [(x, y), (x + dx, y + dy)], 0.055, -2.08)

    shadow = GardenMesh()
    # Deep red under-fold: it gives the main gesture a cast shadow without alpha.
    shadow_path = spline((-5.9, 1.65), (5.95, -1.10), ((1.18, 1.0, 0.15), (0.62, 2.0, 1.5), (0.35, 3.0, -0.7)), 58)
    ribbon(shadow, shadow_path, [0.70 + 0.55 * math.sin(math.pi * u) ** 0.55 for u in [i / 57 for i in range(58)]], -1.18)
    add_shard(shadow, -4.85, 2.95, 0.58, -0.4, -1.10)
    add_shard(shadow, 4.65, -2.6, 0.64, 0.6, -1.10)

    red = GardenMesh()
    main_path = spline((-5.65, -1.45), (5.75, 1.68), ((1.05, 1.0, 2.2), (0.72, 2.0, -0.4), (0.42, 3.0, 1.2)), 64)
    ribbon(red, main_path, [0.46 + 0.92 * math.sin(math.pi * u) ** 0.62 for u in [i / 63 for i in range(64)]], -0.55)
    # A second red plane folds back behind the aperture.
    back_path = spline((-5.8, 2.55), (5.55, -2.55), ((0.85, 1.0, 0.3), (0.50, 2.0, 2.1)), 54)
    ribbon(red, back_path, [0.15 + 0.50 * math.sin(math.pi * u) ** 0.8 for u in [i / 53 for i in range(54)]], -0.48)

    bone = GardenMesh()
    # The bone ribbon is the highlight edge of the main fold; its gaps are deliberate.
    highlight = spline((-5.25, -1.34), (5.35, 1.62), ((0.94, 1.0, 2.1), (0.60, 2.0, -0.2), (0.34, 3.0, 1.0)), 60)
    ribbon(bone, highlight[0:21], [0.075 + 0.13 * math.sin(math.pi * (i / 20)) for i in range(21)], 0.05)
    ribbon(bone, highlight[25:46], [0.06 + 0.16 * math.sin(math.pi * (i / 20)) for i in range(21)], 0.05)
    ribbon(bone, highlight[49:], [0.055 + 0.10 * math.sin(math.pi * (i / 10)) for i in range(11)], 0.05)

    cyan = GardenMesh()
    # Cyan pressure lines do not follow the red shape exactly: they shear through it like wind.
    wind_a = spline((-5.95, 0.55), (5.9, 0.30), ((0.65, 1.0, 0.7), (0.38, 2.0, -1.1), (0.20, 4.0, 1.8)), 58)
    wind_b = spline((-5.7, -2.70), (5.95, 2.85), ((0.75, 1.0, 2.2), (0.34, 3.0, -0.4)), 56)
    add_polyline(cyan, wind_a, 0.075, 0.24)
    add_polyline(cyan, wind_b, 0.055, 0.26)
    add_ring(cyan, -0.15, 0.10, (1.93, 1.21), (1.82, 1.10), 0.28, 0.92, 5.05, 54, 0.09)

    aperture = GardenMesh()
    # One hard negative shape anchors the whole composition.
    hole = []
    for index in range(61):
        t = math.tau * index / 60
        radial_x = 1.0 + 0.16 * math.sin(t - 0.7) + 0.08 * math.sin(3.0 * t + 1.1)
        radial_y = 1.0 + 0.20 * math.cos(t + 0.4)
        x = 1.32 * radial_x * math.cos(t)
        y = 0.83 * radial_y * math.sin(t)
        x, y = x * math.cos(-0.18) - y * math.sin(-0.18), x * math.sin(-0.18) + y * math.cos(-0.18)
        hole.append((-0.10 + x, 0.08 + y))
    add_polygon(aperture, hole[:-1], 0.47)
    add_ring(aperture, -0.10, 0.08, (1.52, 0.98), (1.39, 0.85), 0.51, 0.34, 4.92, 46, 0.05)
    add_polyline(aperture, [(-1.48, 0.32), (-0.88, 0.25), (-0.25, 0.34), (0.38, 0.23), (0.96, 0.29), (1.42, 0.16)], 0.065, 0.54)

    teal = GardenMesh()
    # A narrow inner current makes the void feel active rather than merely painted black.
    inner_path = spline((-3.85, -0.18), (3.75, 0.42), ((0.58, 1.0, 1.0), (0.44, 2.0, -1.0), (0.15, 4.0, 2.0)), 52)
    ribbon(teal, inner_path, [0.08 + 0.19 * math.sin(math.pi * u) ** 0.8 for u in [i / 51 for i in range(52)]], 0.62)
    add_ring(teal, 2.55, -1.52, (0.78, 0.44), (0.65, 0.31), 0.60, 0.0, 5.3, 42, 0.04)

    gold = GardenMesh()
    # Sparse punctuation: marks that suggest notation without turning into UI.
    for x, y, length, angle in ((-5.22, 3.28, 0.46, -0.1), (-4.65, 3.52, 0.26, 0.12), (4.2, 3.08, 0.42, 0.12), (4.75, 2.84, 0.28, -0.1), (-5.12, -3.55, 0.36, 0.1), (4.82, -3.38, 0.44, -0.08)):
        dx, dy = math.cos(angle) * length, math.sin(angle) * length
        add_polyline(gold, [(x, y), (x + dx, y + dy)], 0.07, 0.76)
    for x, y, angle in ((-2.8, 2.55, 0.2), (2.45, 2.85, -0.12), (-3.1, -2.65, -0.2), (2.95, -2.7, 0.2)):
        add_diamond(gold, x, y, 0.14, 0.24, 0.76, angle)

    return (
        (2101, (16, 23, 40, 255), field),
        (2102, (39, 61, 91, 255), contour),
        (2103, (76, 25, 35, 255), shadow),
        (2104, (194, 54, 53, 255), red),
        (2105, (224, 211, 181, 255), bone),
        (2106, (44, 188, 181, 255), cyan),
        (2107, (4, 6, 12, 255), aperture),
        (2108, (58, 181, 169, 255), teal),
        (2109, (232, 177, 69, 255), gold),
    )


def populate(client):
    client.stop()
    client.clear()
    client.camera((0.0, 0.0, 16.0), (0.0, 0.0, 0.0), 42.0)
    for mesh_id, color, mesh in build_scene():
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(120_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start(BACKGROUND)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=2.0)
    parser.add_argument("--output", type=Path, default=Path("bld/draw3d-captures/ink-weather-live.png"))
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
