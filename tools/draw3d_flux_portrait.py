#!/usr/bin/env python3
"""Paint an asymmetric flux portrait on the TRUEOS draw3d screen."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh
from draw3d_house_demo import Draw3dClient
from draw3d_signal_atlas import add_diamond, add_polygon, add_polyline, add_ring


BACKGROUND = (6, 8, 15, 255)


def distorted_loop(cx, cy, rx, ry, phase, shear=0.0, count=64):
    points = []
    for index in range(count + 1):
        t = math.tau * index / count
        radial = 1.0 + 0.095 * math.sin(2.0 * t + phase) + 0.045 * math.sin(5.0 * t - phase * 1.7)
        x = rx * radial * math.cos(t) + shear * math.sin(2.0 * t + phase)
        y = ry * radial * math.sin(t) + 0.18 * math.cos(3.0 * t - phase)
        points.append((cx + x, cy + y))
    return points


def stream(start_y, phase, drift, count=56):
    points = []
    for index in range(count):
        x = -6.2 + 12.4 * index / (count - 1)
        y = start_y + drift * x + 0.42 * math.sin(x * 0.67 + phase) + 0.22 * math.sin(x * 1.45 - phase * 0.6)
        y += 0.90 * math.exp(-((x + 1.5) / 1.65) ** 2) * math.sin(x * 1.15 + phase)
        y -= 0.56 * math.exp(-((x - 2.2) / 1.15) ** 2) * math.cos(x * 1.55 - phase)
        points.append((x, y))
    return points


def build_scene():
    deep = GardenMesh()
    add_polygon(deep, [(-6.5, -4.6), (5.8, -4.6), (6.5, -2.2), (5.4, 4.6), (-5.7, 4.6), (-6.5, 1.3)], -1.6)
    add_ring(deep, -1.05, 0.35, (5.55, 3.82), (5.32, 3.59), -1.55, math.radians(-44), math.radians(226), 72, 0.025)

    indigo_a = GardenMesh()
    indigo_b = GardenMesh()
    indigo_c = GardenMesh()
    # Five contour families around the left attractor and four around the right one.
    for index, scale in enumerate((1.0, 0.88, 0.76, 0.64, 0.52)):
        target = indigo_a if index < 3 else indigo_b
        add_polyline(target, distorted_loop(-1.25, 0.30, 4.30 * scale, 2.75 * scale, 0.25 + index * 0.28, 0.28), 0.050 + index * 0.012, -1.22)
    for index, scale in enumerate((1.0, 0.82, 0.64, 0.47)):
        target = indigo_b if index < 1 else indigo_c
        add_polyline(target, distorted_loop(2.25, -0.55, 2.28 * scale, 1.48 * scale, 1.4 + index * 0.36, -0.34), 0.055 + index * 0.011, -1.20)

    ivory_a = GardenMesh()
    ivory_b = GardenMesh()
    for index, scale in enumerate((1.0, 0.84, 0.68)):
        add_polyline(ivory_a, distorted_loop(-0.75, 0.36, 3.48 * scale, 2.12 * scale, 0.6 + index * 0.41, 0.40), 0.12 - index * 0.012, -0.78)
    add_polyline(ivory_b, stream(2.55, 0.25, -0.04), 0.105, -0.74)
    add_polyline(ivory_b, stream(-2.65, 2.5, 0.06), 0.075, -0.73)

    coral = GardenMesh()
    for index, (start_y, phase, drift) in enumerate(((1.45, 0.3, 0.05), (0.15, 1.7, -0.08), (-1.45, 3.1, 0.03))):
        add_polyline(coral, stream(start_y, phase, drift), 0.145 if index == 1 else 0.085, -0.42)
    add_ring(coral, 2.25, -0.55, (1.18, 0.82), (1.00, 0.64), -0.40, math.radians(-50), math.radians(240), 44, 0.06)

    teal = GardenMesh()
    for index, scale in enumerate((1.0, 0.78, 0.55)):
        add_polyline(teal, distorted_loop(-0.75, 0.36, 2.25 * scale, 1.34 * scale, 1.0 + index * 0.65, -0.15), 0.095 - index * 0.012, 0.0)
    for x, y, width, height, angle in (
        (-3.65, 2.20, 0.28, 0.70, -0.4),
        (-2.75, -2.15, 0.22, 0.52, 0.25),
        (2.85, 1.65, 0.27, 0.64, 0.2),
        (3.7, -1.65, 0.25, 0.58, -0.3),
    ):
        add_diamond(teal, x, y, width, height, 0.02, angle)

    gold = GardenMesh()
    for angle, radius in ((0.2, 3.85), (0.9, 3.25), (1.85, 3.65), (2.95, 3.15), (4.15, 3.78), (5.25, 3.28)):
        add_diamond(gold, -0.75 + radius * math.cos(angle), 0.36 + radius * 0.66 * math.sin(angle), 0.17, 0.29, 0.35, angle)
    for x in (-4.8, -3.95, 3.85, 4.75):
        add_polyline(gold, [(x, -4.05), (x + 0.35, -4.05)], 0.06, 0.34)

    void = GardenMesh()
    add_polygon(void, distorted_loop(-0.75, 0.36, 1.10, 0.68, 0.8, 0.12, 36)[:-1], 0.58)
    add_ring(void, 2.25, -0.55, (0.70, 0.48), (0.57, 0.35), 0.60, 0.0, math.tau, 36, 0.03)

    white = GardenMesh()
    for x, y, length, angle in ((-5.1, 3.0, 0.48, 0.1), (-4.6, 3.48, 0.3, -0.12), (4.3, 2.8, 0.42, -0.1), (4.9, 3.4, 0.28, 0.08), (-5.0, -3.0, 0.4, -0.08), (4.8, -2.8, 0.46, 0.1)):
        add_polyline(white, [(x, y), (x + length * math.cos(angle), y + length * math.sin(angle))], 0.06, 0.84)

    return (
        (1801, (20, 30, 55, 255), deep),
        (1802, (50, 65, 114, 255), indigo_a),
        (1803, (50, 65, 114, 255), indigo_b),
        (1804, (50, 65, 114, 255), indigo_c),
        (1805, (222, 211, 182, 255), ivory_a),
        (1806, (222, 211, 182, 255), ivory_b),
        (1807, (193, 70, 69, 255), coral),
        (1808, (49, 181, 166, 255), teal),
        (1809, (235, 174, 74, 255), gold),
        (1810, (5, 7, 14, 255), void),
        (1811, (224, 229, 218, 255), white),
    )


def populate(client):
    client.stop()
    client.clear()
    client.camera((0.0, 0.0, 16.0), (0.0, 0.0, 0.0), 48.0)
    for mesh_id, color, mesh in build_scene():
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(90_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start(BACKGROUND)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=2.0)
    parser.add_argument("--output", type=Path, default=Path("bld/draw3d-captures/flux-portrait-live.png"))
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
