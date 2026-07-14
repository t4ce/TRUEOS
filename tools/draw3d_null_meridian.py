#!/usr/bin/env python3
"""Paint an abstract impossible machine on the TRUEOS draw3d screen."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh, radial_frustum, torus
from draw3d_house_demo import (
    CUBE_FACES,
    CUBE_VERTICES,
    OCTAHEDRON_FACES,
    OCTAHEDRON_VERTICES,
    Draw3dClient,
)


BACKGROUND = (4, 5, 18, 255)


def add_xyz(mesh, vertices, faces, location, scale=(1.0, 1.0, 1.0), rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(vertices, faces, location, scale, rotation)


def add_box(mesh, location, scale, rotation=(0.0, 0.0, 0.0)):
    add_xyz(mesh, CUBE_VERTICES, CUBE_FACES, location, scale, rotation)


def add_diamond(mesh, location, scale, rotation=(0.0, 0.0, 0.0)):
    add_xyz(mesh, OCTAHEDRON_VERTICES, OCTAHEDRON_FACES, location, scale, rotation)


def wedge(width=2.0, height=2.0, depth=1.0, peak=0.35):
    """A four-sided asymmetric blade, deliberately not a stock primitive."""
    w, d = width * 0.5, depth * 0.5
    vertices = (
        (-w, -height * 0.5, -d),
        (w, -height * 0.5, -d),
        (w * peak, height * 0.5, -d),
        (-w * peak, height * 0.5, -d),
        (-w, -height * 0.5, d),
        (w, -height * 0.5, d),
        (w * peak, height * 0.5, d),
        (-w * peak, height * 0.5, d),
    )
    faces = (
        (0, 1, 2, 3),
        (4, 7, 6, 5),
        (0, 4, 5, 1),
        (3, 2, 6, 7),
        (0, 3, 7, 4),
        (1, 5, 6, 2),
    )
    return vertices, faces


def faceted_orb(segments=24, rings=7, radius=1.0):
    """Low-poly orb with enough latitude to read as a deliberate black sun."""
    vertices = [(0.0, radius, 0.0)]
    for ring in range(1, rings):
        phi = math.pi * ring / rings
        y = math.cos(phi) * radius
        ring_radius = math.sin(phi) * radius
        for segment in range(segments):
            theta = math.tau * segment / segments
            vertices.append((ring_radius * math.cos(theta), y, ring_radius * math.sin(theta)))
    vertices.append((0.0, -radius, 0.0))
    bottom = len(vertices) - 1
    faces = []
    for segment in range(segments):
        nxt = (segment + 1) % segments
        faces.append((0, 1 + segment, 1 + nxt))
    for ring in range(rings - 2):
        row = 1 + ring * segments
        next_row = row + segments
        for segment in range(segments):
            nxt = (segment + 1) % segments
            faces.append((row + segment, next_row + segment, next_row + nxt, row + nxt))
    last_row = 1 + (rings - 2) * segments
    for segment in range(segments):
        nxt = (segment + 1) % segments
        faces.append((last_row + segment, bottom, last_row + nxt))
    return tuple(vertices), tuple(faces)


def blade_ring(count, radius, y, z, width, height, depth, phase=0.0):
    blades = []
    primitive = wedge(width, height, depth, 0.28)
    for index in range(count):
        angle = phase + math.tau * index / count
        blades.append(
            (
                radius * math.cos(angle),
                y + 0.20 * math.sin(angle * 2.0),
                z + radius * 0.34 * math.sin(angle),
                (0.0, -angle, 0.22 * math.sin(angle)),
            )
        )
    return primitive, blades


def build_scene():
    # Backplane: a sparse, architectural horizon. It keeps the silhouette legible without
    # pretending that the renderer has a real skybox or lighting model.
    backdrop = GardenMesh()
    # A distant broken halo is the scene's graphic silhouette: the machine is an aperture
    # cut from a much larger instrument that continues beyond the frame.
    backdrop.add_xyz(*torus(3.45, 0.095, 32, 6), (0.0, 3.15, -4.6), (1.06, 1.0, 0.90), (math.pi * 0.5, 0.0, 0.0))
    backdrop.add_xyz(*torus(2.72, 0.06, 28, 6), (0.0, 3.15, -4.42), (1.06, 1.0, 0.90), (math.pi * 0.5, 0.0, 0.0))
    for x, height, lean in (
        (-6.2, 7.0, -0.06),
        (-4.7, 4.6, 0.08),
        (4.4, 5.5, -0.05),
        (6.1, 7.8, 0.07),
    ):
        add_box(backdrop, (x, height * 0.5 - 0.8, -6.8), (0.28, height, 0.22), (0.0, 0.0, lean))
    # A broken horizon line gives the backplane a designed edge.
    add_box(backdrop, (0.0, -0.45, -6.5), (7.4, 0.08, 0.18))

    # Heavy stepped plinth, built from a few shallow frustums so the base reads as an object,
    # not as a floating island.
    plinth = GardenMesh()
    plinth.add_xyz(*radial_frustum(8, 1.0, 0.93, 0.34), (0.0, -1.40, 0.0), (5.5, 1.0, 3.2))
    plinth.add_xyz(*radial_frustum(8, 0.80, 0.93, 0.24), (0.0, -1.12, 0.0), (4.4, 1.0, 2.55))
    plinth.add_xyz(*radial_frustum(8, 0.76, 0.66, 0.22), (0.0, -0.90, 0.0), (3.3, 1.0, 1.95))
    for x in (-3.8, 3.8):
        add_box(plinth, (x, -0.72, -0.3), (0.36, 0.42, 1.8), (0.0, 0.0, 0.10 * (1 if x < 0 else -1)))

    # The central monolith is deliberately almost black: it is the void that the color system
    # cuts into. The stacked silhouettes make it feel taller than the 512px frame can afford.
    monolith = GardenMesh()
    monolith.add_xyz(*radial_frustum(6, 1.0, 0.72, 5.4), (0.0, 1.82, -0.15), (1.55, 1.0, 1.25), (0.0, 0.12, 0.0))
    monolith.add_xyz(*radial_frustum(6, 0.72, 0.42, 2.1), (0.0, 5.45, -0.12), (1.0, 1.0, 0.82), (0.0, -0.08, 0.0))
    add_box(monolith, (0.0, 1.22, 1.18), (0.78, 1.9, 0.055))
    add_box(monolith, (0.0, 4.92, 0.52), (0.25, 0.64, 0.055))

    void = GardenMesh()
    void.add_xyz(*faceted_orb(24, 7, 1.0), (0.0, 3.16, 0.66), (1.24, 1.24, 0.34), (0.0, 0.16, 0.0))
    # A smaller off-axis facet keeps the “sun” from reading like a stock sphere.
    void.add_xyz(*faceted_orb(16, 5, 0.42), (0.28, 3.30, 1.03), (1.0, 1.0, 0.32), (0.0, -0.32, 0.0))

    # Three vertical portal rings, each on its own material layer. The different rotations are
    # the signature of the scene: an impossible machine folding through itself.
    portal = GardenMesh()
    portal.add_xyz(*torus(2.55, 0.11, 32, 7), (0.0, 3.15, 0.55), (1.0, 1.0, 1.0), (math.pi * 0.5, 0.0, 0.0))
    portal.add_xyz(*torus(1.78, 0.08, 28, 7), (0.0, 3.15, 0.72), (1.0, 1.0, 1.0), (math.pi * 0.5, 0.0, 0.42))
    portal.add_xyz(*torus(1.08, 0.06, 24, 7), (0.0, 3.15, 0.92), (1.0, 1.0, 1.0), (math.pi * 0.5, 0.0, -0.46))

    # A hard-edged luminous glyph / core, not a generic sphere.
    core = GardenMesh()
    add_diamond(core, (0.0, 3.18, 1.12), (0.70, 1.16, 0.24), (0.0, 0.0, math.pi / 4))
    add_diamond(core, (0.0, 3.18, 1.38), (0.22, 0.62, 0.09), (0.0, 0.0, math.pi / 4))
    add_box(core, (0.0, 3.18, 1.50), (0.07, 1.18, 0.05), (0.0, 0.0, math.pi / 4))

    crown = GardenMesh()
    # Keep the high relay separate: the current renderer sorts whole meshes by average depth.
    add_diamond(crown, (0.0, 5.68, 0.10), (0.34, 0.64, 0.20), (0.0, 0.0, math.pi / 4))
    add_diamond(crown, (-0.48, 5.48, 0.10), (0.18, 0.36, 0.12), (0.0, 0.0, -math.pi / 4))
    add_diamond(crown, (0.48, 5.48, 0.10), (0.18, 0.36, 0.12), (0.0, 0.0, -math.pi / 4))
    add_box(crown, (0.0, 5.02, 0.10), (0.035, 0.52, 0.035))

    # Amber calibration marks: intentionally sparse, like an instrument interface rendered in 3D.
    amber = GardenMesh()
    amber.add_xyz(*torus(1.0, 0.055, 32, 6), (0.0, -0.73, 0.0), (4.15, 1.0, 2.25))
    amber.add_xyz(*torus(0.82, 0.045, 28, 6), (0.0, -0.54, 0.0), (3.35, 1.0, 1.82))
    for angle in (0.0, 0.42, 1.85, 2.75, 4.05, 5.35):
        radius = 2.75 if angle not in (1.85, 5.35) else 3.35
        x, z = radius * math.cos(angle), 0.65 + radius * 0.34 * math.sin(angle)
        add_box(amber, (x, 2.95 + 0.18 * math.sin(angle), z), (0.10, 0.52, 0.035), (0.0, -angle, 0.12))
    # Horizontal calibration bars deliberately break the portal silhouette.
    for y, width in ((0.15, 2.5), (1.25, 3.25), (5.40, 1.8)):
        add_box(amber, (0.0, y, 1.48 if y < 2 else 0.78), (width, 0.035, 0.035), (0.0, 0.0, 0.06))

    # Cyan shard field: asymmetrical and directional, like signal debris caught in orbit.
    cyan = GardenMesh()
    shard = wedge(0.45, 1.5, 0.24, 0.18)
    for location, scale, rotation in (
        ((-3.65, 0.45, 1.25), (1.0, 1.2, 1.0), (0.0, 0.0, -0.20)),
        ((-3.15, 1.55, 0.55), (0.7, 1.5, 0.8), (0.0, 0.0, 0.34)),
        ((-2.55, 0.12, -0.55), (0.65, 1.1, 0.8), (0.0, 0.0, -0.55)),
        ((3.60, 0.75, 1.30), (0.9, 1.65, 0.9), (0.0, 0.0, 0.22)),
        ((3.25, 1.75, -0.15), (0.75, 1.25, 0.8), (0.0, 0.0, -0.28)),
        ((2.85, 0.05, -0.80), (0.55, 1.05, 0.75), (0.0, 0.0, 0.48)),
        ((-1.95, 0.55, 2.05), (0.44, 0.90, 0.50), (0.0, 0.0, 0.2)),
        ((2.10, 0.35, 2.20), (0.42, 1.0, 0.50), (0.0, 0.0, -0.16)),
    ):
        add_xyz(cyan, *shard, location, scale, rotation)

    # Coral counter-rotation: a second shard family and two diagonal braces give the composition
    # a red/cyan tension without needing transparency or lighting.
    coral = GardenMesh()
    for location, scale, rotation in (
        ((-4.35, 1.10, 0.25), (0.30, 1.05, 0.38), (0.0, 0.0, -0.45)),
        ((-2.95, 2.35, 0.30), (0.26, 0.90, 0.34), (0.0, 0.0, 0.55)),
        ((3.95, 1.35, 0.40), (0.32, 1.15, 0.40), (0.0, 0.0, 0.40)),
        ((2.60, 2.25, 0.55), (0.24, 0.82, 0.32), (0.0, 0.0, -0.62)),
    ):
        add_xyz(coral, *shard, location, scale, rotation)
    add_box(coral, (-2.0, 2.55, 1.25), (1.25, 0.07, 0.07), (0.0, 0.0, -0.42))
    add_box(coral, (2.0, 3.72, 1.15), (1.20, 0.07, 0.07), (0.0, 0.0, 0.42))

    # Tiny white vector ticks are intentionally not stars: they describe an invisible grid around
    # the machine and make the negative space feel measured.
    ticks = GardenMesh()
    for x, y, z, sx, sy in (
        (-5.2, 4.7, -5.7, 0.12, 0.60),
        (-4.4, 5.6, -5.5, 0.08, 0.36),
        (4.6, 4.9, -5.4, 0.10, 0.52),
        (5.4, 6.2, -5.9, 0.07, 0.32),
        (-5.8, 1.8, -5.2, 0.10, 0.44),
        (5.4, 1.4, -5.0, 0.11, 0.40),
    ):
        add_box(ticks, (x, y, z), (sx, sy, 0.025))

    return (
        (1201, (25, 28, 74, 255), backdrop),
        (1202, (18, 24, 54, 255), plinth),
        (1203, (11, 12, 36, 255), monolith),
        (1204, (37, 25, 84, 255), void),
        (1205, (218, 50, 154, 255), portal),
        (1206, (78, 238, 222, 255), core),
        (1207, (78, 238, 222, 255), crown),
        (1208, (247, 179, 61, 255), amber),
        (1209, (42, 213, 226, 255), cyan),
        (1210, (239, 66, 113, 255), coral),
        (1211, (226, 232, 238, 255), ticks),
    )


def populate(client):
    client.stop()
    client.clear()
    client.camera((9.5, 6.4, 16.2), (0.0, 2.25, 0.25), 43.0)
    for mesh_id, color, mesh in build_scene():
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(30_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start(BACKGROUND)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--settle", type=float, default=2.0)
    parser.add_argument("--output", type=Path, default=Path("bld/draw3d-captures/null-meridian-live.png"))
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
