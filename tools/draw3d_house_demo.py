#!/usr/bin/env python3
"""Populate the TRUEOS draw3d service with a small house-and-tree scene."""

import argparse
import hashlib
import math
import socket
import struct
import time
from pathlib import Path


PORT = 4246


def frame(opcode, request_id, payload=b""):
    return b"D3" + bytes((1, opcode)) + struct.pack("<II", request_id, len(payload)) + payload


def recv_exact(sock, length):
    chunks = []
    received = 0
    while received < length:
        chunk = sock.recv(length - received)
        if not chunk:
            raise RuntimeError("draw3d connection closed")
        chunks.append(chunk)
        received += len(chunk)
    return b"".join(chunks)


class Draw3dClient:
    def __init__(self, host):
        self.sock = socket.create_connection((host, PORT), timeout=5)
        self.request_id = 1

    def close(self):
        self.sock.close()

    def call(self, opcode, payload=b""):
        request_id = self.request_id
        self.request_id += 1
        self.sock.sendall(frame(opcode, request_id, payload))
        header = recv_exact(self.sock, 12)
        if header[:3] != b"D3\x01" or header[3] != opcode | 0x80:
            raise RuntimeError(f"invalid reply header: {header!r}")
        reply_id, payload_len = struct.unpack("<II", header[4:12])
        if reply_id != request_id:
            raise RuntimeError(f"reply ID {reply_id} != request ID {request_id}")
        reply = recv_exact(self.sock, payload_len)
        if not reply or reply[0] != 0:
            status = reply[0] if reply else "empty"
            raise RuntimeError(f"opcode 0x{opcode:02x} failed with status {status}")
        return reply

    def clear(self):
        self.call(0x18)

    def start(self, clear=None):
        payload = bytes(clear) if clear is not None else b""
        if len(payload) not in (0, 4):
            raise ValueError("scene clear must be an RGBA four-tuple")
        self.call(0x19, payload)

    def stop(self, permanent=False):
        """Pause the scene, or permanently discard it and its resident meshes."""
        self.call(0x1A, b"\x01" if permanent else b"")

    def camera(
        self,
        position,
        target,
        fov_degrees=54.0,
        *,
        orbit_scale=None,
        orbit_rotation=(0.0, 0.0, 0.0),
        orbit_speed=0.0,
    ):
        """Set a static camera or an optional elliptical look-at orbit.

        Orbit rotation and speed are radians and radians/second. The two scale
        values are the radii of the source ellipse's X and Z axes. Supplying no
        orbit scale emits the original 48-byte static-camera packet.
        """
        direction = tuple(target[index] - position[index] for index in range(3))
        payload = struct.pack(
            "<12f",
            *position,
            *direction,
            0.0,
            1.0,
            0.0,
            0.1,
            100.0,
            math.radians(fov_degrees),
        )
        if orbit_scale is None:
            if orbit_speed != 0.0:
                raise ValueError("orbit_speed requires orbit_scale=(x_radius, z_radius)")
        else:
            if len(orbit_scale) != 2:
                raise ValueError("orbit_scale must contain X and Z radii")
            if len(orbit_rotation) != 3:
                raise ValueError("orbit_rotation must contain XYZ Euler radians")
            payload += struct.pack(
                "<9f",
                *target,
                *orbit_rotation,
                *orbit_scale,
                orbit_speed,
            )
        self.call(0x22, payload)

    def mesh(self, mesh_id, color, vertices, faces):
        payload = bytearray(struct.pack("<Q4B", mesh_id, *color))
        payload.extend(struct.pack("<I", len(vertices)))
        for vertex in vertices:
            payload.extend(struct.pack("<3f", *vertex))
        payload.extend(struct.pack("<I", 0))  # edges are not needed by the triangle renderer
        payload.extend(struct.pack("<I", len(faces)))
        for face_indices in faces:
            payload.extend(struct.pack("<H", len(face_indices)))
            payload.extend(struct.pack(f"<{len(face_indices)}I", *face_indices))
        self.call(0x01, payload)

    def instance(self, instance_id, mesh_id, location, scale, rotation=(0.0, 0.0, 0.0)):
        payload = struct.pack("<QQ9f", instance_id, mesh_id, *location, *rotation, *scale)
        self.call(0x10, payload)

    def stats(self):
        reply = self.call(0x20)
        return struct.unpack("<IIQQQQ", reply[2:])

    def render(self, output_path):
        reply = self.call(0x23)
        if reply[1] != 3:
            raise RuntimeError(f"unexpected render reply kind {reply[1]}")
        image_format = reply[2]
        width, height = struct.unpack("<II", reply[3:11])
        image = reply[11:]
        expected_suffix = ".png" if image_format == 2 else ".jpg"
        output_path = output_path.with_suffix(expected_suffix)
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_bytes(image)
        return output_path, image_format, width, height, image


CUBE_VERTICES = (
    (-1.0, -1.0, -1.0),
    (1.0, -1.0, -1.0),
    (1.0, 1.0, -1.0),
    (-1.0, 1.0, -1.0),
    (-1.0, -1.0, 1.0),
    (1.0, -1.0, 1.0),
    (1.0, 1.0, 1.0),
    (-1.0, 1.0, 1.0),
)
CUBE_FACES = (
    (0, 3, 2, 1),
    (4, 5, 6, 7),
    (0, 4, 7, 3),
    (1, 2, 6, 5),
    (0, 1, 5, 4),
    (3, 7, 6, 2),
)

ROOF_VERTICES = (
    (-1.0, 0.0, 1.0),
    (1.0, 0.0, 1.0),
    (0.0, 1.0, 1.0),
    (-1.0, 0.0, -1.0),
    (0.0, 1.0, -1.0),
    (1.0, 0.0, -1.0),
)
ROOF_FACES = ((0, 1, 2), (3, 4, 5), (0, 2, 4, 3), (1, 5, 4, 2), (0, 3, 5, 1))

OCTAHEDRON_VERTICES = (
    (0.0, 1.0, 0.0),
    (1.0, 0.0, 0.0),
    (0.0, 0.0, 1.0),
    (-1.0, 0.0, 0.0),
    (0.0, 0.0, -1.0),
    (0.0, -1.0, 0.0),
)
OCTAHEDRON_FACES = (
    (0, 1, 2),
    (0, 2, 3),
    (0, 3, 4),
    (0, 4, 1),
    (5, 2, 1),
    (5, 3, 2),
    (5, 4, 3),
    (5, 1, 4),
)


class MeshBuilder:
    def __init__(self):
        self.vertices = []
        self.faces = []

    def add(self, vertices, faces, location, scale, rotation_y=0.0, rotation_z=0.0):
        base = len(self.vertices)
        sin_y = math.sin(rotation_y)
        cos_y = math.cos(rotation_y)
        sin_z = math.sin(rotation_z)
        cos_z = math.cos(rotation_z)
        for x, y, z in vertices:
            x *= scale[0]
            y *= scale[1]
            z *= scale[2]
            x, y = x * cos_z - y * sin_z, x * sin_z + y * cos_z
            self.vertices.append(
                (
                    x * cos_y + z * sin_y + location[0],
                    y + location[1],
                    -x * sin_y + z * cos_y + location[2],
                )
            )
        self.faces.extend(tuple(base + index for index in face) for face in faces)

def populate(client):
    client.stop()
    client.clear()
    client.camera((8.0, 5.5, 13.0), (0.0, 2.0, 0.0), 48.0)

    # Group disconnected cuboids by color. This produces the whole composition in four resident
    # jobs, matching the draw count proven reliable on the current bare-metal render path.
    plaster = MeshBuilder()
    plaster.add(CUBE_VERTICES, CUBE_FACES, (-1.4, 1.5, 0.0), (2.5, 1.5, 2.0))

    roof = MeshBuilder()
    roof.add(CUBE_VERTICES, CUBE_FACES, (-2.72, 3.63, 0.0), (1.5, 0.18, 2.3), rotation_z=0.42)
    roof.add(CUBE_VERTICES, CUBE_FACES, (-0.08, 3.63, 0.0), (1.5, 0.18, 2.3), rotation_z=-0.42)

    accents = MeshBuilder()
    accents.add(CUBE_VERTICES, CUBE_FACES, (-2.1, 0.8, 2.04), (0.5, 0.8, 0.08))
    accents.add(CUBE_VERTICES, CUBE_FACES, (-0.35, 1.65, 2.04), (0.55, 0.45, 0.08))
    accents.add(CUBE_VERTICES, CUBE_FACES, (4.5, 1.05, -0.1), (0.38, 1.05, 0.38))

    foliage = MeshBuilder()
    foliage.add(CUBE_VERTICES, CUBE_FACES, (4.5, 3.15, -0.1), (1.35, 1.2, 1.3))
    foliage.add(CUBE_VERTICES, CUBE_FACES, (3.85, 3.0, 0.1), (0.75, 0.85, 0.75))
    foliage.add(CUBE_VERTICES, CUBE_FACES, (5.15, 3.0, -0.2), (0.75, 0.85, 0.75))

    meshes = (
        (601, (238, 205, 139, 255), plaster),
        (602, (171, 54, 45, 255), roof),
        (603, (91, 52, 31, 255), accents),
        (604, (48, 137, 60, 255), foliage),
    )
    for mesh_id, color, mesh in meshes:
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(6000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    client.start((255, 255, 255, 255))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument(
        "--output", type=Path, default=Path("bld/draw3d-captures/house-tree-gabled.png")
    )
    parser.add_argument("--settle", type=float, default=1.5)
    parser.add_argument(
        "--orbit-speed",
        type=float,
        default=0.18,
        help="camera-orbit speed in radians per second; use 0 for a static camera",
    )
    parser.add_argument("--expect-width", type=int, default=2560)
    parser.add_argument("--expect-height", type=int, default=1440)
    args = parser.parse_args()

    client = Draw3dClient(args.host)
    try:
        populate(client)
        client.stop()
        if args.orbit_speed != 0.0:
            client.camera(
                (14.0, 2.0, 0.0),
                (0.0, 2.0, 0.0),
                48.0,
                orbit_scale=(14.0, 9.0),
                orbit_rotation=(math.radians(-8.0), 0.0, math.radians(3.0)),
                orbit_speed=args.orbit_speed,
            )
        # Starting without RGBA selects the protocol's transparent clear color.
        client.start()
        time.sleep(args.settle)
        output, image_format, width, height, image = client.render(args.output)
        mesh_count, instance_count, vertices, edges, faces, mesh_bytes = client.stats()
        print(
            f"scene meshes={mesh_count} instances={instance_count} vertices={vertices} "
            f"edges={edges} faces={faces} mesh_bytes={mesh_bytes} "
            f"orbit_speed={args.orbit_speed} transparent_background=1"
        )
        print(
            f"capture format={image_format} size={width}x{height} bytes={len(image)} "
            f"sha256={hashlib.sha256(image).hexdigest()} path={output}"
        )
        if image_format != 2 or (width, height) != (args.expect_width, args.expect_height):
            raise RuntimeError(
                "live scene did not return the expected "
                f"{args.expect_width}x{args.expect_height} PNG"
            )
    finally:
        client.close()


if __name__ == "__main__":
    main()
