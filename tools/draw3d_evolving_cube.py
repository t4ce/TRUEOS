#!/usr/bin/env python3
"""Build and continuously evolve a nested cube sculpture over Draw3D TCP."""

import argparse
import colorsys
import math
import struct
import time

from draw3d_celestial_garden import GardenMesh, add_box, add_octahedron
from draw3d_house_demo import CUBE_FACES, CUBE_VERTICES, Draw3dClient


BACKGROUND = (5, 7, 18, 255)
CORE_MESH_ID = 6000
FRAME_MESH_IDS = (6001, 6002, 6003)
NODE_MESH_ID = 6004
SATELLITE_MESH_ID = 6005
CORE_INSTANCE_ID = 86_000
FRAME_INSTANCE_IDS = (86_001, 86_002, 86_003)
NODE_INSTANCE_ID = 86_004
SATELLITE_INSTANCE_IDS = tuple(86_100 + index for index in range(8))


def set_rotation(client, instance_id, rotation):
    client.call(0x16, struct.pack("<Q3f", instance_id, *rotation))


def set_scale(client, instance_id, scale):
    client.call(0x17, struct.pack("<Q3f", instance_id, *scale))


def set_location(client, instance_id, location):
    client.call(0x15, struct.pack("<Q3f", instance_id, *location))


def set_color(client, mesh_id, color):
    client.call(0x07, struct.pack("<Q4B", mesh_id, *color))


def cube_frame(half_extent, thickness):
    mesh = GardenMesh()
    for y in (-half_extent, half_extent):
        for z in (-half_extent, half_extent):
            add_box(mesh, (0.0, y, z), (half_extent, thickness, thickness))
    for x in (-half_extent, half_extent):
        for z in (-half_extent, half_extent):
            add_box(mesh, (x, 0.0, z), (thickness, half_extent, thickness))
    for x in (-half_extent, half_extent):
        for y in (-half_extent, half_extent):
            add_box(mesh, (x, y, 0.0), (thickness, thickness, half_extent))
    return mesh


def crystal_nodes():
    mesh = GardenMesh()
    for location, scale in (
        ((2.32, 0.0, 0.0), (0.30, 0.16, 0.16)),
        ((-2.32, 0.0, 0.0), (0.30, 0.16, 0.16)),
        ((0.0, 2.32, 0.0), (0.16, 0.30, 0.16)),
        ((0.0, -2.32, 0.0), (0.16, 0.30, 0.16)),
        ((0.0, 0.0, 2.32), (0.16, 0.16, 0.30)),
        ((0.0, 0.0, -2.32), (0.16, 0.16, 0.30)),
    ):
        add_octahedron(mesh, location, scale)
    return mesh


def hsv_color(hue, saturation=0.82, value=1.0):
    red, green, blue = colorsys.hsv_to_rgb(hue % 1.0, saturation, value)
    return (round(red * 255), round(green * 255), round(blue * 255), 255)


def upload_mesh_instance(client, mesh_id, instance_id, color, mesh):
    vertices = mesh.vertices if isinstance(mesh, GardenMesh) else CUBE_VERTICES
    faces = mesh.faces if isinstance(mesh, GardenMesh) else CUBE_FACES
    triangles = sum(len(face) - 2 for face in faces)
    if len(vertices) > 1_000 or triangles > 2_000:
        raise RuntimeError(
            f"mesh {mesh_id} exceeds service budget: vertices={len(vertices)} triangles={triangles}"
        )
    client.mesh(mesh_id, color, vertices, faces)
    client.instance(instance_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))


def build_live_scene(client, phase_delay):
    frames = (
        cube_frame(1.28, 0.075),
        cube_frame(1.70, 0.060),
        cube_frame(2.12, 0.045),
    )
    nodes = crystal_nodes()

    client.stop()
    client.clear()
    client.camera((0.0, 1.45, 8.8), (0.0, 0.0, 0.0), 48.0)

    upload_mesh_instance(
        client,
        CORE_MESH_ID,
        CORE_INSTANCE_ID,
        (32, 52, 118, 255),
        CUBE_VERTICES,
    )
    set_scale(client, CORE_INSTANCE_ID, (0.92, 0.92, 0.92))
    upload_mesh_instance(
        client,
        FRAME_MESH_IDS[0],
        FRAME_INSTANCE_IDS[0],
        (35, 235, 255, 255),
        frames[0],
    )
    client.start(BACKGROUND)
    print("phase=core-and-inner-frame live=1", flush=True)
    time.sleep(phase_delay)

    for index in (1, 2):
        upload_mesh_instance(
            client,
            FRAME_MESH_IDS[index],
            FRAME_INSTANCE_IDS[index],
            ((255, 50, 190, 255), (255, 205, 45, 255))[index - 1],
            frames[index],
        )
        print(f"phase=frame-{index + 1} live=1", flush=True)
        time.sleep(phase_delay)

    upload_mesh_instance(
        client,
        NODE_MESH_ID,
        NODE_INSTANCE_ID,
        (235, 245, 255, 255),
        nodes,
    )
    print("phase=face-crystals live=1", flush=True)
    time.sleep(phase_delay)

    client.mesh(SATELLITE_MESH_ID, (105, 255, 105, 255), CUBE_VERTICES, CUBE_FACES)
    for index, instance_id in enumerate(SATELLITE_INSTANCE_IDS):
        sx = -1.0 if index & 1 else 1.0
        sy = -1.0 if index & 2 else 1.0
        sz = -1.0 if index & 4 else 1.0
        client.instance(
            instance_id,
            SATELLITE_MESH_ID,
            (sx * 2.30, sy * 2.30, sz * 2.30),
            (0.18, 0.18, 0.18),
        )
    print("phase=corner-satellites live=1", flush=True)
    return frames


def evolve(client, interval):
    frame = 0
    started = time.monotonic()
    while True:
        now = time.monotonic()
        elapsed = now - started
        pulse = 0.92 + 0.11 * math.sin(elapsed * 1.15)

        set_rotation(
            client,
            CORE_INSTANCE_ID,
            (elapsed * -0.19, elapsed * 0.23, elapsed * -0.11),
        )
        set_scale(client, CORE_INSTANCE_ID, (pulse, pulse, pulse))
        set_rotation(
            client,
            FRAME_INSTANCE_IDS[0],
            (0.24 * math.sin(elapsed * 0.43), elapsed * 0.18, elapsed * 0.09),
        )
        set_rotation(
            client,
            FRAME_INSTANCE_IDS[1],
            (elapsed * -0.13, 0.31 * math.sin(elapsed * 0.31), elapsed * 0.16),
        )
        set_rotation(
            client,
            FRAME_INSTANCE_IDS[2],
            (elapsed * 0.08, elapsed * -0.12, 0.36 * math.cos(elapsed * 0.27)),
        )
        set_rotation(
            client,
            NODE_INSTANCE_ID,
            (elapsed * 0.10, elapsed * 0.15, elapsed * -0.07),
        )

        corner_radius = 2.30 + 0.16 * math.sin(elapsed * 0.72)
        twist = elapsed * 0.21
        cos_twist = math.cos(twist)
        sin_twist = math.sin(twist)
        for index, instance_id in enumerate(SATELLITE_INSTANCE_IDS):
            x = (-1.0 if index & 1 else 1.0) * corner_radius
            y = (-1.0 if index & 2 else 1.0) * corner_radius
            z = (-1.0 if index & 4 else 1.0) * corner_radius
            rotated_x = x * cos_twist + z * sin_twist
            rotated_z = -x * sin_twist + z * cos_twist
            set_location(client, instance_id, (rotated_x, y, rotated_z))
            set_rotation(
                client,
                instance_id,
                (elapsed * (0.18 + index * 0.011), elapsed * -0.24, elapsed * 0.14),
            )

        hue = elapsed * 0.035
        set_color(client, FRAME_MESH_IDS[0], hsv_color(hue + 0.00))
        set_color(client, FRAME_MESH_IDS[1], hsv_color(hue + 0.33))
        set_color(client, FRAME_MESH_IDS[2], hsv_color(hue + 0.66))
        set_color(client, SATELLITE_MESH_ID, hsv_color(hue + 0.48, 0.68))

        frame += 1
        if frame == 1 or frame % 25 == 0:
            stats = client.stats()
            print(
                f"evolve frame={frame} seconds={elapsed:.1f} pulse={pulse:.3f} "
                f"meshes={stats[0]} instances={stats[1]} vertices={stats[2]} faces={stats[4]}",
                flush=True,
            )
        time.sleep(interval)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--interval", type=float, default=0.40)
    parser.add_argument("--phase-delay", type=float, default=0.55)
    args = parser.parse_args()

    client = Draw3dClient(args.host)
    try:
        build_live_scene(client, args.phase_delay)
        evolve(client, max(0.10, args.interval))
    except KeyboardInterrupt:
        print("evolve stopped; final retained frame remains active", flush=True)
    finally:
        client.close()


if __name__ == "__main__":
    main()
