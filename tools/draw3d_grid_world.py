#!/usr/bin/env python3
"""Generate a large reusable chunked terrain world through the draw3d TCP API."""

import argparse
import hashlib
import math
import time
from pathlib import Path

from draw3d_celestial_garden import GardenMesh, add_box, add_octahedron, radial_frustum, torus
from draw3d_house_demo import Draw3dClient


DEFAULT_CELLS = 128
DEFAULT_CHUNK_CELLS = 26
DEFAULT_WORLD_SIZE = 64.0
DEFAULT_ORBIT_SPEED = math.radians(8.0)

VIEWS = {
    "river": ((-38.0, 25.0, 42.0), (0.0, 0.5, 0.0), 52.0),
    "platforms": ((39.0, 30.0, -42.0), (1.0, 2.0, 0.0), 52.0),
    "overview": ((45.0, 39.0, 51.0), (0.0, 1.2, 0.0), 51.0),
}


def smoothstep(edge0, edge1, value):
    if edge0 == edge1:
        return 0.0
    t = max(0.0, min(1.0, (value - edge0) / (edge1 - edge0)))
    return t * t * (3.0 - 2.0 * t)


def lerp(a, b, amount):
    return a + (b - a) * amount


def hash_noise(ix, iz):
    value = (ix * 0x1F123BB5) ^ (iz * 0x5F356495) ^ 0x6C8E9CF5
    value = (value ^ (value >> 16)) * 0x45D9F3B
    value = (value ^ (value >> 16)) * 0x45D9F3B
    value ^= value >> 16
    return (value & 0xFFFFFFFF) / 0xFFFFFFFF


def value_noise(x, z):
    ix, iz = math.floor(x), math.floor(z)
    fx, fz = x - ix, z - iz
    sx, sz = smoothstep(0.0, 1.0, fx), smoothstep(0.0, 1.0, fz)
    bottom = lerp(hash_noise(ix, iz), hash_noise(ix + 1, iz), sx)
    top = lerp(hash_noise(ix, iz + 1), hash_noise(ix + 1, iz + 1), sx)
    return lerp(bottom, top, sz) * 2.0 - 1.0


def river_center_z(x):
    return math.sin(x * 0.12) * 4.2 + math.sin(x * 0.035 + 0.8) * 1.7


def flatten_platform(height, x, z, center_x, center_z, radius, target, falloff=2.4):
    distance = math.hypot(x - center_x, z - center_z)
    weight = 1.0 - smoothstep(radius, radius + falloff, distance)
    return lerp(height, target, weight)


def terrain_height(x, z):
    broad = value_noise(x / 9.5, z / 9.5) * 2.4
    medium = value_noise(x / 4.2 + 13.0, z / 4.2 - 7.0) * 0.95
    ridges = abs(math.sin(x * 0.095 + value_noise(x / 12.0, z / 12.0))) * 0.85
    height = broad + medium + ridges - 0.45

    # Designed mesas are flattened after noise so they remain useful building pads.
    height = flatten_platform(height, x, z, -14.0, -10.0, 5.8, 5.2, 2.0)
    height = flatten_platform(height, x, z, 14.0, 11.0, 4.8, 6.8, 2.2)
    height = flatten_platform(height, x, z, 18.0, -15.0, 3.6, 3.6, 1.8)

    # Terrace the northeastern highland while retaining some underlying noise.
    terrace_weight = 1.0 - smoothstep(7.0, 13.0, math.hypot(x - 14.0, z - 11.0))
    terraced = round(height / 1.15) * 1.15
    height = lerp(height, terraced, terrace_weight * 0.72)

    # The river is an actual carved channel, not a flat overlay on top of hills.
    river_distance = abs(z - river_center_z(x))
    channel_weight = 1.0 - smoothstep(1.55, 4.3, river_distance)
    channel_floor = -1.55 + river_distance * 0.16 + math.sin(x * 0.17) * 0.08
    height = lerp(height, channel_floor, channel_weight)
    return height


def cell_material(x, z, corner_heights):
    river_distance = abs(z - river_center_z(x))
    slope = max(corner_heights) - min(corner_heights)
    average = sum(corner_heights) / 4.0
    if river_distance < 1.42:
        return "water"
    if river_distance < 3.9:
        return "sand"
    if slope > 0.78 or average > 6.25 or average < -1.0:
        return "rock"
    return "grass"


def chunk_color(material, chunk_x, chunk_z):
    variation = ((chunk_x * 19 + chunk_z * 31) % 13) - 6
    if material == "grass":
        return (39 + variation, 103 + variation, 57 + variation // 2, 255)
    if material == "rock":
        return (91 + variation, 99 + variation, 103 + variation, 255)
    if material == "water":
        return (42 + variation // 2, 120 + variation, 177 + variation, 255)
    return (151 + variation, 126 + variation, 78 + variation // 2, 255)


def build_heightfield_chunks(cells=DEFAULT_CELLS, chunk_cells=DEFAULT_CHUNK_CELLS, world_size=DEFAULT_WORLD_SIZE):
    if cells < 8 or chunk_cells < 2:
        raise ValueError("terrain requires at least 8 cells and chunks of at least 2 cells")
    chunks_per_axis = math.ceil(cells / chunk_cells)
    minimum_meshes = chunks_per_axis * chunks_per_axis
    if minimum_meshes + 10 > 100:
        raise ValueError(
            f"{cells}x{cells} with {chunk_cells}-cell chunks needs at least {minimum_meshes} terrain meshes; "
            "the standalone world reserves 10 of draw3d's 100 meshes for paths and landmarks"
        )

    cell_size = world_size / cells
    half_world = world_size * 0.5
    result = []
    mesh_id = 5_000
    for chunk_z, start_z in enumerate(range(0, cells, chunk_cells)):
        height_cells = min(chunk_cells, cells - start_z)
        for chunk_x, start_x in enumerate(range(0, cells, chunk_cells)):
            width_cells = min(chunk_cells, cells - start_x)
            vertices = []
            for local_z in range(height_cells + 1):
                grid_z = start_z + local_z
                z = -half_world + grid_z * cell_size
                for local_x in range(width_cells + 1):
                    grid_x = start_x + local_x
                    x = -half_world + grid_x * cell_size
                    vertices.append((x, terrain_height(x, z), z))

            faces_by_material = {"grass": [], "rock": [], "sand": [], "water": []}
            row = width_cells + 1
            for local_z in range(height_cells):
                grid_z = start_z + local_z
                z = -half_world + (grid_z + 0.5) * cell_size
                for local_x in range(width_cells):
                    grid_x = start_x + local_x
                    x = -half_world + (grid_x + 0.5) * cell_size
                    v00 = local_z * row + local_x
                    v10 = v00 + 1
                    v01 = v00 + row
                    v11 = v01 + 1
                    heights = (vertices[v00][1], vertices[v10][1], vertices[v01][1], vertices[v11][1])
                    material = cell_material(x, z, heights)
                    if (grid_x + grid_z) % 2:
                        faces_by_material[material].extend(((v00, v01, v10), (v10, v01, v11)))
                    else:
                        faces_by_material[material].extend(((v00, v01, v11), (v00, v11, v10)))

            for material in ("grass", "rock", "sand", "water"):
                faces = faces_by_material[material]
                if not faces:
                    continue
                material_vertices = tuple(vertices)
                if material in ("water", "sand"):
                    material_vertices = tuple(
                        (x, -0.88, z) if abs(z - river_center_z(x)) < 1.95 else (x, y, z)
                        for x, y, z in vertices
                    )
                if len(material_vertices) > 1_000 or len(faces) > 2_000:
                    raise RuntimeError(
                        f"terrain chunk {chunk_x},{chunk_z}/{material} exceeds budget: "
                        f"vertices={len(material_vertices)} triangles={len(faces)}"
                    )
                result.append((mesh_id, chunk_color(material, chunk_x, chunk_z), material_vertices, tuple(faces)))
                mesh_id += 1
    return result


def ribbon(points, width):
    vertices = []
    half_width = width * 0.5
    for index, (x, y, z) in enumerate(points):
        previous = points[max(index - 1, 0)]
        following = points[min(index + 1, len(points) - 1)]
        dx, dz = following[0] - previous[0], following[2] - previous[2]
        length = math.hypot(dx, dz) or 1.0
        nx, nz = -dz / length, dx / length
        vertices.extend(((x + nx * half_width, y, z + nz * half_width), (x - nx * half_width, y, z - nz * half_width)))
    faces = tuple((index * 2, index * 2 + 1, index * 2 + 3, index * 2 + 2) for index in range(len(points) - 1))
    return tuple(vertices), faces


def add_cylinder(mesh, location, radius, height, segments=12, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(*radial_frustum(segments), location, (radius, height * 0.5, radius), rotation)


def add_cone(mesh, location, radius, height, segments=9, rotation=(0.0, 0.0, 0.0)):
    mesh.add_xyz(
        *radial_frustum(segments, bottom_radius=1.0, top_radius=0.0, height=2.0),
        location,
        (radius, height * 0.5, radius),
        rotation,
    )


def build_world_details(world_size=DEFAULT_WORLD_SIZE):
    _ = world_size

    road = GardenMesh()
    road_xz = ((2.0, 29.0), (2.0, 20.0), (1.5, 12.0), (0.5, 5.0), (0.0, -3.5), (-5.0, -8.0), (-12.5, -10.0))
    road_points = tuple((x, terrain_height(x, z) + 0.09, z) for x, z in road_xz)
    road.add_xyz(*ribbon(road_points, 1.45), (0.0, 0.0, 0.0))

    platform_stone = GardenMesh()
    add_cylinder(platform_stone, (-14.0, 5.24, -10.0), 5.15, 0.28, 18)
    add_cylinder(platform_stone, (14.0, 6.84, 11.0), 4.15, 0.28, 16)
    add_cylinder(platform_stone, (18.0, 3.64, -15.0), 3.05, 0.24, 14)
    # Two compact stair runs connect the designed pads back to the terrain.
    for step in range(6):
        add_box(platform_stone, (-8.4 + step * 0.55, 0.48 + step * 0.78, -10.0), (0.34, 0.39, 1.05))
    for step in range(6):
        add_box(platform_stone, (9.0 + step * 0.52, 1.02 + step * 0.96, 11.0), (0.32, 0.48, 0.92))

    platform_trim = GardenMesh()
    platform_trim.add_xyz(*torus(4.72, 0.06, 36, 5), (-14.0, 5.42, -10.0))
    platform_trim.add_xyz(*torus(3.80, 0.055, 32, 5), (14.0, 7.02, 11.0))
    platform_trim.add_xyz(*torus(2.78, 0.05, 28, 5), (18.0, 3.79, -15.0))

    bridge = GardenMesh()
    add_box(bridge, (0.0, 0.15, river_center_z(0.0)), (1.35, 0.20, 3.15))
    for z_offset in (-2.5, -1.5, -0.5, 0.5, 1.5, 2.5):
        add_box(bridge, (0.0, 0.39, river_center_z(0.0) + z_offset), (1.48, 0.055, 0.16))

    bridge_rails = GardenMesh()
    for x in (-1.28, 1.28):
        add_box(bridge_rails, (x, 0.75, river_center_z(0.0)), (0.07, 0.07, 3.05))
        for z_offset in (-2.5, 0.0, 2.5):
            add_box(bridge_rails, (x, 0.50, river_center_z(0.0) + z_offset), (0.09, 0.48, 0.09))

    rocks = GardenMesh()
    for x, z, scale, yaw in (
        (-26.0, -18.0, 1.20, 0.10), (-22.0, 18.0, 0.88, -0.18), (-8.0, 24.0, 1.05, 0.22),
        (8.0, -25.0, 1.15, -0.20), (25.0, 20.0, 1.30, 0.16), (27.0, -5.0, 0.92, -0.12),
    ):
        y = terrain_height(x, z)
        add_octahedron(rocks, (x, y + scale * 0.55, z), (scale, scale * 0.68, scale * 0.84), (0.0, yaw, 0.0))

    trunks = GardenMesh()
    crowns_dark = GardenMesh()
    crowns_light = GardenMesh()
    tree_sites = ((-25.0, -23.0, 2.3), (-24.0, 14.0, 2.0), (-5.0, 23.0, 2.2), (23.0, 23.0, 2.4), (26.0, -19.0, 2.1), (7.0, -25.0, 1.8))
    for x, z, scale in tree_sites:
        y = terrain_height(x, z)
        add_cylinder(trunks, (x, y + scale * 0.75, z), scale * 0.18, scale * 1.50, 7)
        add_cone(crowns_dark, (x, y + scale * 1.65, z - 0.10), scale * 0.78, scale * 1.65, 9)
        add_cone(crowns_light, (x, y + scale * 2.05, z + 0.12), scale * 0.55, scale * 1.18, 8)

    motes = GardenMesh()
    for x, z, lift in ((-18.0, -12.0, 3.0), (-2.0, 3.0, 2.0), (11.0, 13.0, 3.5), (20.0, -12.0, 2.4), (26.0, 18.0, 3.0)):
        y = terrain_height(x, z) + lift
        add_octahedron(motes, (x, y, z), (0.16, 0.28, 0.16))

    return (
        (5_260, (126, 99, 62, 255), road),
        (5_261, (94, 101, 105, 255), platform_stone),
        (5_262, (161, 170, 171, 255), platform_trim),
        (5_263, (111, 69, 39, 255), bridge),
        (5_264, (70, 43, 29, 255), bridge_rails),
        (5_265, (78, 86, 91, 255), rocks),
        (5_266, (77, 48, 30, 255), trunks),
        (5_267, (18, 66, 51, 255), crowns_dark),
        (5_268, (48, 125, 67, 255), crowns_light),
        (5_269, (237, 209, 92, 255), motes),
    )


def set_orbit(client, speed=DEFAULT_ORBIT_SPEED):
    look_at = (0.0, 7.0, 0.0)
    client.camera(
        (68.0, 11.0, 0.0),
        look_at,
        VIEWS["overview"][2],
        orbit_scale=(68.0, 62.0),
        orbit_rotation=(math.radians(-3.5), 0.0, math.radians(3.5)),
        orbit_speed=speed,
    )


def populate(
    client,
    cells=DEFAULT_CELLS,
    chunk_cells=DEFAULT_CHUNK_CELLS,
    world_size=DEFAULT_WORLD_SIZE,
    orbit_speed=DEFAULT_ORBIT_SPEED,
):
    client.stop()
    client.clear()
    position, target, fov = VIEWS["overview"]
    client.camera(position, target, fov)

    terrain_chunks = build_heightfield_chunks(cells, chunk_cells, world_size)
    detail_meshes = build_world_details(world_size)
    if len(terrain_chunks) + len(detail_meshes) > 100:
        raise RuntimeError(
            f"world needs {len(terrain_chunks) + len(detail_meshes)} meshes/instances, above draw3d's limit of 100"
        )

    for mesh_id, color, vertices, faces in terrain_chunks:
        client.mesh(mesh_id, color, vertices, faces)
        client.instance(80_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    for mesh_id, color, mesh in detail_meshes:
        triangles = sum(len(face) - 2 for face in mesh.faces)
        if len(mesh.vertices) > 1_000 or triangles > 2_000:
            raise RuntimeError(
                f"detail mesh {mesh_id} exceeds budget: vertices={len(mesh.vertices)} triangles={triangles}"
            )
        client.mesh(mesh_id, color, mesh.vertices, mesh.faces)
        client.instance(80_000 + mesh_id, mesh_id, (0.0, 0.0, 0.0), (1.0, 1.0, 1.0))
    set_orbit(client, orbit_speed)
    # Empty StartScene preserves alpha so UI4 can composite the world over its desktop.
    client.start()
    return len(terrain_chunks), len(detail_meshes)


def capture_view(client, name, output_dir, settle):
    position, target, fov = VIEWS[name]
    client.camera(position, target, fov)
    time.sleep(settle)
    path, image_format, width, height, image = client.render(output_dir / f"grid-world-{name}.png")
    if image_format != 2 or width <= 0 or height <= 0:
        raise RuntimeError(f"{name} view did not return a non-empty PNG target")
    digest = hashlib.sha256(image).hexdigest()
    print(f"view={name} size={width}x{height} bytes={len(image)} sha256={digest} path={path}")
    return path, digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--cells", type=int, default=DEFAULT_CELLS)
    parser.add_argument("--chunk-cells", type=int, default=DEFAULT_CHUNK_CELLS)
    parser.add_argument("--world-size", type=float, default=DEFAULT_WORLD_SIZE)
    parser.add_argument(
        "--orbit-speed",
        type=float,
        default=DEFAULT_ORBIT_SPEED,
        help="camera-orbit speed in radians per second (default: 8 degrees/second)",
    )
    parser.add_argument("--settle", type=float, default=4.0)
    parser.add_argument("--output-dir", type=Path, default=Path("bld/draw3d-captures"))
    parser.add_argument(
        "--capture",
        action="store_true",
        help="opt in to rendering the three diagnostic PNG views",
    )
    args = parser.parse_args()

    client = Draw3dClient(args.host)
    try:
        terrain_count, detail_count = populate(
            client,
            args.cells,
            args.chunk_cells,
            args.world_size,
            args.orbit_speed,
        )
        if args.capture:
            try:
                for view_name in ("river", "platforms", "overview"):
                    capture_view(client, view_name, args.output_dir, args.settle)
            finally:
                set_orbit(client, args.orbit_speed)
        stats = client.stats()
        print(
            f"grid={args.cells}x{args.cells} logical_quads={args.cells * args.cells} "
            f"terrain_meshes={terrain_count} detail_meshes={detail_count} "
            f"scene_meshes={stats[0]} instances={stats[1]} vertices={stats[2]} "
            f"faces={stats[4]} mesh_bytes={stats[5]} "
            f"orbit_speed={args.orbit_speed:.9f} background=transparent "
            f"captures={int(args.capture)} final_view=orbit"
        )
    finally:
        client.close()


if __name__ == "__main__":
    main()
