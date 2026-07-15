#!/usr/bin/env python3
"""Build a colorful full-room disco dancehall through the Draw3D TCP API.

The uploader deliberately starts the room and flycam after the base phase,
then adds the rig, equipment, disco balls, and color effects while the scene
stays visible.  This makes the construction itself observable on the monitor.
"""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh, add_box, add_octahedron, radial_frustum
from draw3d_house_demo import Draw3dClient


BACKGROUND = (7, 5, 18, 255)
IDENTITY_LOCATION = (0.0, 0.0, 0.0)
IDENTITY_SCALE = (1.0, 1.0, 1.0)
DEFAULT_ORBIT_SPEED = 0.004

COLOR_MAGENTA = (255, 45, 166, 255)
COLOR_CYAN = (30, 226, 255, 255)
COLOR_GOLD = (255, 211, 45, 255)
COLOR_LIME = (105, 240, 82, 255)
EFFECT_COLORS = (COLOR_MAGENTA, COLOR_CYAN, COLOR_GOLD, COLOR_LIME)

VIEWS = {
    "hero": ((0.0, 7.1, 23.5), (0.0, 2.5, -1.5), 54.0),
    "diagonal": ((15.5, 7.0, 20.5), (0.0, 2.6, -1.3), 55.0),
    "stage": ((0.0, 4.2, 12.0), (0.0, 2.8, -6.5), 58.0),
}


def uv_sphere(latitude_steps=10, longitude_steps=16):
    vertices = [(0.0, -1.0, 0.0)]
    for latitude in range(1, latitude_steps):
        phi = -math.pi * 0.5 + math.pi * latitude / latitude_steps
        radius = math.cos(phi)
        y = math.sin(phi)
        for longitude in range(longitude_steps):
            theta = math.tau * longitude / longitude_steps
            vertices.append((radius * math.cos(theta), y, radius * math.sin(theta)))
    top = len(vertices)
    vertices.append((0.0, 1.0, 0.0))

    faces = []
    first_ring = 1
    for longitude in range(longitude_steps):
        nxt = (longitude + 1) % longitude_steps
        faces.append((0, first_ring + nxt, first_ring + longitude))
    for latitude in range(latitude_steps - 2):
        ring = first_ring + latitude * longitude_steps
        next_ring = ring + longitude_steps
        for longitude in range(longitude_steps):
            nxt = (longitude + 1) % longitude_steps
            faces.append((ring + longitude, ring + nxt, next_ring + nxt, next_ring + longitude))
    last_ring = first_ring + (latitude_steps - 2) * longitude_steps
    for longitude in range(longitude_steps):
        nxt = (longitude + 1) % longitude_steps
        faces.append((top, last_ring + longitude, last_ring + nxt))
    return tuple(vertices), tuple(faces)


def add_cylinder(mesh, location, radius, height, segments=10, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(
        *radial_frustum(segments),
        location,
        (radius, height * 0.5, radius),
        rotation,
    )


def add_sphere(mesh, center, radius, latitude_steps=10, longitude_steps=16):
    mesh.add_xyz(
        *uv_sphere(latitude_steps, longitude_steps),
        center,
        (radius, radius, radius),
    )


def merged_mesh(*meshes):
    """Combine baked geometry while preserving the source face indices."""
    merged = GardenMesh()
    for mesh in meshes:
        base = len(merged.vertices)
        merged.vertices.extend(mesh.vertices)
        merged.faces.extend(tuple(base + index for index in face) for face in mesh.faces)
    return merged


def add_disco_facets(effect_meshes, center, radius, latitudes, longitude_steps):
    cx, cy, cz = center
    for latitude_index, latitude_deg in enumerate(latitudes):
        latitude = math.radians(latitude_deg)
        ring_radius = math.cos(latitude)
        normal_y = math.sin(latitude)
        for longitude in range(longitude_steps):
            theta = math.tau * longitude / longitude_steps
            normal_x = ring_radius * math.cos(theta)
            normal_z = ring_radius * math.sin(theta)
            x = cx + normal_x * radius * 1.025
            y = cy + normal_y * radius * 1.025
            z = cz + normal_z * radius * 1.025
            rotation_x = -math.asin(max(-1.0, min(1.0, normal_y)))
            rotation_y = math.atan2(normal_x, normal_z)
            effect = effect_meshes[(latitude_index + longitude) % len(effect_meshes)]
            tile = 0.13 if radius < 1.0 else 0.17
            add_box(
                effect,
                (x, y, z),
                (tile, tile, 0.035),
                (rotation_x, rotation_y, 0.0),
            )


def add_beam_ribbon(mesh, source, target, target_width=0.65):
    sx, sy, sz = source
    tx, ty, tz = target
    vertices = (
        (sx - 0.10, sy, sz),
        (sx + 0.10, sy, sz),
        (tx + target_width, ty, tz),
        (tx - target_width, ty, tz),
    )
    mesh.add_xyz(vertices, ((0, 1, 2), (0, 2, 3)), IDENTITY_LOCATION)


def add_floor_tiles(floors):
    columns, rows = 10, 8
    for row in range(rows):
        for column in range(columns):
            x = (column - (columns - 1) / 2) * 1.25
            z = 5.4 - row * 1.15
            add_box(
                floors[(row + column) % len(floors)],
                (x, 0.08, z),
                (0.56, 0.08, 0.50),
            )


def add_wall_patterns(effects):
    # Back-wall equalizer: four interleaved colors with a recognizable skyline.
    heights = (1.1, 2.2, 3.0, 1.7, 3.6, 2.6, 1.4, 3.2, 2.0, 3.8, 2.5, 1.2)
    for index, height in enumerate(heights):
        x = (index - (len(heights) - 1) / 2) * 0.95
        add_box(
            effects[index % 4],
            (x, 1.35 + height * 0.5, -9.62),
            (0.30, height * 0.5, 0.055),
        )

    # Side-wall pulse stripes are visible as the flycam drifts diagonally.
    for side in (-1.0, 1.0):
        wall_x = side * 13.72
        rotation_y = math.pi * 0.5
        for index, z in enumerate((-6.5, -3.5, -0.5, 2.5, 5.5, 8.0)):
            height = 1.0 + (index % 3) * 0.65
            add_box(
                effects[(index + (0 if side < 0 else 2)) % 4],
                (wall_x, 3.3, z),
                (0.055, height, 0.34),
                (0.0, rotation_y, 0.0),
            )

    # A four-color diamond around the center of the DJ backdrop.
    diamond = (
        ((-1.35, 5.9, -9.55), -0.62),
        ((-0.45, 6.6, -9.55), 0.62),
        ((0.45, 6.6, -9.55), -0.62),
        ((1.35, 5.9, -9.55), 0.62),
    )
    for index, (location, rotation_z) in enumerate(diamond):
        add_box(effects[index], location, (0.12, 0.80, 0.06), (0.0, 0.0, rotation_z))


def add_stage_color(effects):
    # Equalizer bars on the DJ booth.
    heights = (0.45, 0.75, 1.05, 0.65, 1.20, 0.85, 0.55, 1.05)
    for index, height in enumerate(heights):
        x = (index - 3.5) * 0.70
        add_box(
            effects[index % 4],
            (x, 1.80 + height * 0.5, -5.00),
            (0.19, height * 0.5, 0.055),
        )

    # Speaker cones face the room along +Z.
    for speaker_x in (-5.2, 5.2):
        for row, y in enumerate((1.25, 2.65, 4.05)):
            add_cylinder(
                effects[(row + (0 if speaker_x < 0 else 2)) % 4],
                (speaker_x, y, -5.33),
                0.55 if row != 1 else 0.68,
                0.10,
                12,
                (math.pi * 0.5, 0.0, 0.0),
            )

    # Stage-edge chase lights.
    for index, x in enumerate((-5.8, -4.2, -2.6, -1.0, 1.0, 2.6, 4.2, 5.8)):
        add_sphere(effects[index % 4], (x, 1.12, -5.25), 0.16, 6, 8)


def add_ceiling_decor(effects):
    # Pennant rows give parallax and make the room volume obvious.
    for row, z in enumerate((-0.5, 3.5)):
        for index in range(13):
            x = -10.8 + index * 1.8
            vertices = ((x - 0.35, 6.85, z), (x + 0.35, 6.85, z), (x, 6.25, z))
            effects[(index + row) % 4].add_xyz(vertices, ((0, 1, 2),), IDENTITY_LOCATION)

    # Colored light ribbons converge on distinct dance-floor quadrants.
    sources = ((-7.5, 7.1, -3.0), (-2.5, 7.1, -3.0), (2.5, 7.1, -3.0), (7.5, 7.1, -3.0))
    targets = ((-4.5, 0.3, 4.5), (-1.7, 0.3, 0.5), (1.7, 0.3, 4.5), (4.5, 0.3, 0.5))
    for index in range(4):
        add_beam_ribbon(effects[index], sources[index], targets[index], 0.55)


def disco_scene_batches():
    room = GardenMesh()
    add_box(room, (0.0, -0.35, 0.0), (14.0, 0.35, 10.0))
    add_box(room, (0.0, 4.5, -10.0), (14.0, 4.5, 0.25))
    add_box(room, (-14.0, 4.5, 0.0), (0.25, 4.5, 10.0))
    add_box(room, (14.0, 4.5, 0.0), (0.25, 4.5, 10.0))
    # Dark ceiling strips frame the open top without hiding the disco balls.
    add_box(room, (0.0, 8.7, -8.8), (14.0, 0.20, 1.2))
    add_box(room, (-12.8, 8.7, 0.0), (1.2, 0.20, 8.8))
    add_box(room, (12.8, 8.7, 0.0), (1.2, 0.20, 8.8))

    stage = GardenMesh()
    add_box(stage, (0.0, 0.55, -7.4), (7.2, 0.55, 2.2))
    add_box(stage, (0.0, 4.0, -9.50), (8.0, 3.0, 0.28))
    add_box(stage, (-8.7, 1.15, -7.7), (1.2, 1.15, 1.8))
    add_box(stage, (8.7, 1.15, -7.7), (1.2, 1.15, 1.8))

    floors = [GardenMesh() for _ in range(4)]
    add_floor_tiles(floors)

    rig = GardenMesh()
    # Ceiling trusses and mirrored stage trim.
    for z in (-3.0, 3.0):
        add_box(rig, (0.0, 7.35, z), (10.5, 0.10, 0.10))
        for x in (-10.0, -5.0, 0.0, 5.0, 10.0):
            add_box(rig, (x, 7.35, z), (0.10, 0.38, 0.38), (0.0, 0.0, math.pi * 0.25))
    for x in (-10.5, 10.5):
        add_box(rig, (x, 7.35, 0.0), (0.10, 0.10, 3.1))
    add_box(rig, (0.0, 3.10, -4.88), (3.75, 0.10, 0.10))
    add_box(rig, (0.0, 1.08, -5.18), (7.0, 0.10, 0.10))
    # Ball cables, booth console, side tables, and guard rails.
    for x, y, z in ((0.0, 7.75, -1.5), (-7.0, 7.45, 1.2), (7.0, 7.45, 1.2)):
        add_cylinder(rig, (x, y, z), 0.035, 1.8 if x == 0.0 else 2.2, 8)
    add_box(rig, (0.0, 2.68, -5.35), (3.55, 0.08, 0.72), (0.10, 0.0, 0.0))
    for x in (-11.0, 11.0):
        add_cylinder(rig, (x, 0.85, 3.0), 0.75, 0.12, 14)
        add_cylinder(rig, (x, 0.42, 3.0), 0.08, 0.85, 8)
        for z in (1.6, 4.4):
            add_cylinder(rig, (x, 0.45, z), 0.35, 0.65, 10)

    equipment = GardenMesh()
    add_box(equipment, (0.0, 1.85, -5.65), (3.45, 0.75, 0.65))
    for speaker_x in (-5.2, 5.2):
        add_box(equipment, (speaker_x, 2.65, -5.75), (1.05, 2.15, 0.55))
    for speaker_x in (-8.7, 8.7):
        add_box(equipment, (speaker_x, 1.15, -5.95), (1.20, 1.15, 0.75))
    # Moving-head housings on the truss.
    for z in (-3.0, 3.0):
        for x in (-7.5, -2.5, 2.5, 7.5):
            add_box(equipment, (x, 6.92, z), (0.30, 0.30, 0.32))
            add_cylinder(equipment, (x, 6.52, z), 0.20, 0.45, 8)

    balls = GardenMesh()
    add_sphere(balls, (0.0, 6.15, -1.5), 1.55, 12, 20)
    add_sphere(balls, (-7.0, 5.65, 1.2), 0.90, 9, 14)
    add_sphere(balls, (7.0, 5.65, 1.2), 0.90, 9, 14)

    effects = [GardenMesh() for _ in range(4)]
    add_disco_facets(effects, (0.0, 6.15, -1.5), 1.55, (-60, -40, -20, 0, 20, 40, 60), 16)
    add_disco_facets(effects, (-7.0, 5.65, 1.2), 0.90, (-45, -15, 15, 45), 12)
    add_disco_facets(effects, (7.0, 5.65, 1.2), 0.90, (-45, -15, 15, 45), 12)
    add_wall_patterns(effects)
    add_stage_color(effects)
    add_ceiling_decor(effects)
    # Small floating diamonds complete the corners without cluttering sightlines.
    for index, (x, y, z) in enumerate(
        ((-11.0, 5.8, -6.0), (11.0, 5.8, -6.0), (-11.0, 5.0, 6.5), (11.0, 5.0, 6.5))
    ):
        add_octahedron(effects[index], (x, y, z), (0.55, 0.85, 0.55))

    # Keep broad, high-coverage room surfaces in their own resident draws.
    # The current bare-metal backend reliably retires dense decorative draws,
    # but a single draw combining the room, stage, and equipment can cover the
    # full scanout several times and overrun its per-submit completion window.
    # Nine final jobs is still compact while matching the modest draw shapes
    # used by the known-good tactical scene.
    color_layers = tuple(merged_mesh(floors[index], effects[index]) for index in range(4))

    return {
        "base": (
            (4001, (38, 20, 56, 255), room),
            *((4010 + index, EFFECT_COLORS[index], floors[index]) for index in range(4)),
        ),
        "stage": (
            (4002, (38, 20, 56, 255), stage),
            (4003, (38, 20, 56, 255), equipment),
        ),
        "rig": (
            (4020, (205, 218, 228, 255), rig),
        ),
        "disco-balls": (
            (4021, (205, 218, 228, 255), balls),
        ),
        "color-effects": tuple(
            (4010 + index, EFFECT_COLORS[index], color_layers[index]) for index in range(4)
        ),
    }


def validate_phases(phases):
    colors_by_id = {}
    for phase_name, batches in phases.items():
        for mesh_id, color, mesh in batches:
            previous_color = colors_by_id.setdefault(mesh_id, color)
            if previous_color != color:
                raise RuntimeError(
                    f"{phase_name} mesh {mesh_id} changes material color "
                    f"from {previous_color} to {color}"
                )
            triangles = sum(len(face) - 2 for face in mesh.faces)
            if len(mesh.vertices) > 1_000 or triangles > 2_000:
                raise RuntimeError(
                    f"{phase_name} mesh {mesh_id} exceeds service budget: "
                    f"vertices={len(mesh.vertices)} triangles={triangles}"
                )


def set_static_view(client, name="hero"):
    position, target, fov = VIEWS[name]
    client.camera(position, target, fov)


def set_flycam(client, speed=DEFAULT_ORBIT_SPEED):
    look_at = (0.0, 2.4, -1.2)
    client.camera(
        (16.0, 7.0, 20.0),
        look_at,
        55.0,
        orbit_scale=(24.0, 20.0),
        orbit_rotation=(-0.03, -0.90, 0.28),
        orbit_speed=speed,
    )


def upload_batch(client, batch):
    mesh_id, color, mesh = batch
    client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
    client.instance(70_000 + mesh_id, mesh_id, IDENTITY_LOCATION, IDENTITY_SCALE)


def populate_live(client, speed=DEFAULT_ORBIT_SPEED, phase_delay=0.7):
    phases = disco_scene_batches()
    validate_phases(phases)
    client.stop()
    client.clear()
    set_static_view(client, "hero")

    for batch in phases["base"]:
        upload_batch(client, batch)
    client.start(BACKGROUND)
    set_flycam(client, speed)
    print(f"phase=base live=1 batches={len(phases['base'])} flycam_speed={speed}")
    time.sleep(phase_delay)

    for phase_name in ("stage", "rig", "disco-balls", "color-effects"):
        for batch in phases[phase_name]:
            upload_batch(client, batch)
            time.sleep(phase_delay)
        # Reset phase zero so each major addition is immediately visible from
        # the intended diagonal rather than drifting behind a side wall.
        set_flycam(client, speed)
        print(f"phase={phase_name} live=1 batches={len(phases[phase_name])} view=corrected")
    return phases


def capture_static(client, output, settle):
    set_static_view(client, "hero")
    time.sleep(settle)
    path, image_format, width, height, image = client.render(output)
    if image_format != 2 or width <= 0 or height <= 0:
        print(
            f"capture unavailable format={image_format} size={width}x{height} "
            f"bytes={len(image)} fallback_path={path}"
        )
        return None
    digest = hashlib.sha256(image).hexdigest()
    print(
        f"capture size={width}x{height} bytes={len(image)} "
        f"sha256={digest} path={path}"
    )
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--speed", type=float, default=DEFAULT_ORBIT_SPEED)
    parser.add_argument("--phase-delay", type=float, default=0.7)
    parser.add_argument("--settle", type=float, default=5.0)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("bld/draw3d-captures/disco-dancehall/hero.png"),
    )
    args = parser.parse_args()

    client = Draw3dClient(args.host)
    try:
        phases = populate_live(client, args.speed, args.phase_delay)
        try:
            capture_static(client, args.output, args.settle)
        finally:
            # A screenshot is diagnostic only; local presentation must always
            # return to the live flycam even when capture falls back or fails.
            set_flycam(client, args.speed)
        stats = client.stats()
        total_batches = sum(len(batches) for batches in phases.values())
        print(
            f"scene meshes={stats[0]} instances={stats[1]} vertices={stats[2]} "
            f"faces={stats[4]} mesh_bytes={stats[5]} upload_ops={total_batches} "
            f"material_layers={len({mesh_id for batches in phases.values() for mesh_id, _, _ in batches})} "
            f"flycam_speed={args.speed} final_view=diagonal"
        )
    finally:
        # The kernel keeps presenting the active scene after this client exits.
        client.close()


if __name__ == "__main__":
    main()
