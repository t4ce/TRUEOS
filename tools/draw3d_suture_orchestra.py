#!/usr/bin/env python3
"""Paint a cutaway observatory: a layered machine grown around a dark well."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh
from draw3d_house_demo import Draw3dClient
from draw3d_signal_atlas import add_polygon, add_polyline, add_ring


BACKGROUND = (5, 7, 13, 255)


def warped_loop(cx, cy, rx, ry, phase, count=72, shear=0.0):
    points = []
    for index in range(count + 1):
        t = math.tau * index / count
        radial = 1.0 + 0.10 * math.sin(3.0 * t + phase) + 0.045 * math.sin(7.0 * t - phase)
        x = rx * radial * math.cos(t) + shear * math.sin(2.0 * t + phase)
        y = ry * radial * math.sin(t) + 0.15 * math.cos(4.0 * t - phase)
        points.append((cx + x, cy + y))
    return points


def branch_points(start, end, phase, steps=34):
    sx, sy = start
    ex, ey = end
    points = []
    dx, dy = ex - sx, ey - sy
    length = math.hypot(dx, dy) or 1.0
    nx, ny = -dy / length, dx / length
    for index in range(steps):
        u = index / (steps - 1)
        bend = math.sin(math.pi * u) * (0.26 * math.sin(phase) + 0.18 * math.sin(phase * 1.7))
        tremor = 0.06 * math.sin(u * math.tau * 2.2 + phase)
        points.append((sx + dx * u + nx * (bend + tremor), sy + dy * u + ny * bend))
    return points


def add_strata(mesh, y, phase, z, width=6.4):
    points = []
    for index in range(28):
        x = -width + 2.0 * width * index / 27
        points.append((x, y + 0.13 * math.sin(x * 0.75 + phase) + 0.06 * math.sin(x * 2.2 - phase)))
    add_polyline(mesh, points, 0.045, z)


def add_arch(mesh, cx, base_y, width, height, lean, z, thickness):
    points = []
    for index in range(48):
        u = index / 47
        x = cx + width * (u - 0.5)
        y = base_y + height * math.sin(math.pi * u) ** 0.72 + lean * (u - 0.5)
        points.append((x, y))
    add_polyline(mesh, points, thickness, z)


def build_scene():
    # A skewed backplate behaves like a stage wall instead of a blank rectangle.
    back = GardenMesh()
    add_polygon(back, [(-6.5, -4.6), (5.9, -4.6), (6.45, 4.5), (-5.8, 4.5)], -3.0)
    for y, phase in ((-3.75, 0.2), (-2.95, 1.0), (-2.15, 2.0), (2.70, 0.9), (3.45, 2.2)):
        add_strata(back, y, phase, -2.94)
    add_ring(back, -2.7, 2.35, (1.30, 0.74), (1.20, 0.64), -2.91, 0.15, 5.35, 48, 0.06)
    add_ring(back, 3.45, -2.95, (1.02, 0.55), (0.92, 0.45), -2.90, 2.7, 6.0, 40, 0.04)

    strata = GardenMesh()
    for y, phase in ((-3.2, 0.4), (-2.65, 1.7), (-2.10, 3.1), (1.95, 0.2), (2.42, 1.5), (2.88, 2.8)):
        add_strata(strata, y, phase, -2.25, 6.1)
    for x in (-4.9, -4.35, 4.3, 4.86):
        add_polyline(strata, [(x, -4.25), (x + 0.1, -2.6), (x - 0.04, -0.95)], 0.052, -2.18)

    chamber = GardenMesh()
    add_polygon(chamber, [(-5.55, -3.75), (-3.95, -3.35), (-3.25, -0.5), (-1.7, 0.22), (-0.45, 2.8), (2.25, 3.26), (4.65, 1.7), (5.35, -2.8), (3.72, -3.72), (0.1, -4.1)], -1.62)
    # A cut in the chamber gives the well a geological context.
    add_polyline(chamber, [(-5.1, -1.9), (-3.7, -1.35), (-2.1, -1.62), (-0.4, -1.2), (1.8, -1.48), (4.75, -0.72)], 0.09, -1.51)
    add_polyline(chamber, [(-4.7, 1.45), (-3.2, 1.0), (-2.35, 1.55), (-0.7, 1.18), (1.4, 1.64), (3.9, 1.06)], 0.06, -1.50)

    well_outer = GardenMesh()
    add_ring(well_outer, 0.10, -0.05, (3.35, 2.45), (3.05, 2.16), -1.08, 0.0, math.tau, 86, 0.07)
    add_ring(well_outer, 0.18, -0.14, (2.78, 1.96), (2.53, 1.70), -1.02, 0.12, 6.0, 72, 0.08)

    well_mid = GardenMesh()
    add_ring(well_mid, 0.22, -0.12, (2.30, 1.58), (2.08, 1.37), -0.72, 0.0, math.tau, 68, 0.10)
    add_ring(well_mid, 0.30, -0.08, (1.86, 1.23), (1.66, 1.04), -0.67, 0.34, 5.88, 60, 0.08)

    well_inner = GardenMesh()
    add_ring(well_inner, 0.34, -0.07, (1.50, 0.93), (1.27, 0.70), -0.32, 0.0, math.tau, 54, 0.07)
    add_polyline(well_inner, [(-1.35, 0.05), (-0.88, 0.23), (-0.25, 0.18), (0.33, 0.34), (1.08, 0.12), (1.75, 0.24)], 0.07, -0.27)

    aperture = GardenMesh()
    add_polygon(aperture, warped_loop(0.36, -0.04, 0.92, 0.55, 1.4, 48, 0.15)[:-1], 0.14)
    add_ring(aperture, 0.36, -0.04, (1.06, 0.66), (0.93, 0.53), 0.18, 0.0, math.tau, 48, 0.04)

    # Bone-colored ribs are the foreground architecture. Each stays in its own
    # depth band so the retained painter can preserve the silhouette.
    ribs_a = GardenMesh()
    add_arch(ribs_a, -0.12, -3.72, 8.7, 5.65, -0.36, -0.05, 0.14)
    add_arch(ribs_a, -0.06, -3.55, 7.35, 4.70, 0.48, 0.01, 0.075)
    add_arch(ribs_a, 0.04, -3.35, 6.1, 3.82, -0.58, 0.05, 0.055)

    ribs_b = GardenMesh()
    add_arch(ribs_b, -0.22, -3.55, 8.2, 5.05, 0.62, 0.13, 0.07)
    add_arch(ribs_b, 0.12, -3.25, 5.5, 3.50, -0.44, 0.16, 0.09)
    add_arch(ribs_b, 0.32, -3.10, 4.3, 2.72, 0.38, 0.19, 0.055)

    # Suture lines stitch the distant geology to the aperture.
    sutures_a = GardenMesh()
    sutures_b = GardenMesh()
    starts = [(-5.7, 2.9), (-5.5, 1.55), (-5.4, -0.8), (-4.8, -2.6), (5.75, 2.45), (5.45, 0.95), (5.6, -1.05), (5.1, -2.75)]
    targets = [(-0.2, 0.42), (0.0, 0.22), (0.35, 0.05), (0.18, -0.36), (0.75, 0.42), (0.52, 0.12), (0.33, -0.18), (0.08, -0.38)]
    for index, (start, target) in enumerate(zip(starts, targets)):
        target_mesh = sutures_a if index < 4 else sutures_b
        add_polyline(target_mesh, branch_points(start, target, index * 0.83 + 0.4), 0.055 if index % 3 else 0.085, 0.27)

    rust = GardenMesh()
    for index, (start, target) in enumerate(
        [((-4.85, 3.0), (-1.35, 1.06)), ((-4.55, -3.0), (-1.05, -0.88)), ((4.8, 2.7), (1.45, 0.88)), ((4.7, -2.95), (1.22, -0.95))]
    ):
        add_polyline(rust, branch_points(start, target, 1.1 + index * 1.8, 28), 0.13, 0.42)

    cyan = GardenMesh()
    # Small crossbars make the branches read as an instrument, not decoration.
    for x, y, angle in ((-4.0, 2.45, 0.14), (-3.55, -2.15, -0.20), (-2.7, 1.52, 0.32), (2.8, 1.86, -0.18), (3.95, -1.92, 0.23), (4.35, 0.8, -0.30)):
        length = 0.55 if abs(x) < 3.5 else 0.75
        dx, dy = math.cos(angle) * length, math.sin(angle) * length
        add_polyline(cyan, [(x - dx, y - dy), (x + dx, y + dy)], 0.11, 0.56)
    for angle in (0.25, 1.5, 2.8, 4.05, 5.25):
        x = 0.36 + 1.62 * math.cos(angle)
        y = -0.04 + 0.92 * math.sin(angle)
        add_polyline(cyan, [(x - 0.10, y), (x + 0.10, y)], 0.075, 0.58)

    sulfur = GardenMesh()
    for x, y, length, angle in ((-5.3, 3.8, 0.42, -0.1), (-4.7, 3.95, 0.28, 0.16), (4.35, 3.55, 0.40, 0.08), (5.0, 3.4, 0.25, -0.15), (-5.15, -3.8, 0.35, 0.12), (4.9, -3.78, 0.38, -0.08)):
        dx, dy = math.cos(angle) * length, math.sin(angle) * length
        add_polyline(sulfur, [(x, y), (x + dx, y + dy)], 0.065, 0.74)

    return (
        (1901, (17, 24, 44, 255), back),
        (1902, (31, 45, 77, 255), strata),
        (1903, (42, 47, 72, 255), chamber),
        (1904, (116, 126, 165, 255), well_outer),
        (1905, (196, 197, 176, 255), well_mid),
        (1906, (232, 220, 184, 255), well_inner),
        (1907, (4, 7, 14, 255), aperture),
        (1908, (213, 205, 177, 255), ribs_a),
        (1909, (107, 117, 152, 255), ribs_b),
        (1910, (71, 115, 125, 255), sutures_a),
        (1911, (71, 115, 125, 255), sutures_b),
        (1912, (170, 65, 53, 255), rust),
        (1913, (72, 204, 190, 255), cyan),
        (1914, (234, 183, 73, 255), sulfur),
    )


def populate(client):
    client.stop()
    client.clear()
    client.camera((0.0, 0.0, 16.0), (0.0, 0.0, 0.0), 48.0)
    for mesh_id, color, mesh in build_scene():
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(100_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start(BACKGROUND)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=2.0)
    parser.add_argument("--output", type=Path, default=Path("bld/draw3d-captures/suture-orchestra-live.png"))
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
