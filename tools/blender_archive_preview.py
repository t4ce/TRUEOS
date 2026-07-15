"""Render a neutral studio preview of the .blend file Blender opened."""

import math
import sys
from pathlib import Path

import bpy
from mathutils import Vector


PALETTE = (
    (0.08, 0.32, 0.52, 1.0),
    (0.12, 0.48, 0.34, 1.0),
    (0.62, 0.25, 0.12, 1.0),
    (0.42, 0.18, 0.55, 1.0),
    (0.68, 0.48, 0.10, 1.0),
)


def look_at(obj, target):
    direction = Vector(target) - obj.location
    obj.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()


def make_material(name, color, metallic=0.0, roughness=0.55):
    material = bpy.data.materials.new(name)
    material.diffuse_color = color
    material.use_nodes = True
    shader = material.node_tree.nodes.get("Principled BSDF")
    if shader:
        shader.inputs["Base Color"].default_value = color
        shader.inputs["Metallic"].default_value = metallic
        shader.inputs["Roughness"].default_value = roughness
    return material


def add_area(name, location, energy, size, color, target):
    data = bpy.data.lights.new(name, "AREA")
    data.energy = energy
    data.shape = "DISK"
    data.size = size
    data.color = color
    light = bpy.data.objects.new(name, data)
    bpy.context.scene.collection.objects.link(light)
    light.location = location
    look_at(light, target)


def main():
    separator = sys.argv.index("--") if "--" in sys.argv else len(sys.argv)
    arguments = sys.argv[separator + 1 :]
    if not arguments:
        raise SystemExit("output path required after --")
    output = Path(arguments[0]).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)

    mesh_objects = [obj for obj in bpy.data.objects if obj.type == "MESH" and not obj.hide_render]
    if not mesh_objects:
        raise SystemExit("opened file contains no renderable mesh objects")

    # A fresh scene avoids inheriting obsolete render/output settings from old projects.
    scene = bpy.data.scenes.new("Archive Preview Scene")
    bpy.context.window.scene = scene
    for obj in bpy.data.objects:
        if obj.type not in {"CAMERA", "LIGHT"}:
            scene.collection.objects.link(obj)

    bounds_min = Vector((float("inf"),) * 3)
    bounds_max = Vector((float("-inf"),) * 3)
    for obj in mesh_objects:
        for corner in obj.bound_box:
            world = obj.matrix_world @ Vector(corner)
            for axis in range(3):
                bounds_min[axis] = min(bounds_min[axis], world[axis])
                bounds_max[axis] = max(bounds_max[axis], world[axis])
        if not obj.material_slots:
            material = make_material(
                f"Archive Preview {obj.name}",
                PALETTE[sum(ord(char) for char in obj.name) % len(PALETTE)],
            )
            obj.data.materials.append(material)

    center = (bounds_min + bounds_max) * 0.5
    dimensions = bounds_max - bounds_min
    radius = max(dimensions.length * 0.5, 0.01)

    camera_data = bpy.data.cameras.new("Archive Preview Camera")
    camera_data.lens = 58.0
    camera = bpy.data.objects.new("Archive Preview Camera", camera_data)
    scene.collection.objects.link(camera)
    camera.location = center + Vector((1.35, -1.75, 1.05)).normalized() * radius * 2.55
    look_at(camera, center + Vector((0.0, 0.0, dimensions.z * 0.04)))
    scene.camera = camera

    light_scale = max(radius * radius, 0.001)
    add_area(
        "Archive Key",
        center + Vector((-1.4, -1.2, 2.0)).normalized() * radius * 2.2,
        900.0 * light_scale,
        radius * 1.15,
        (1.0, 0.82, 0.64),
        center,
    )
    add_area(
        "Archive Fill",
        center + Vector((1.7, -0.2, 0.7)).normalized() * radius * 2.0,
        520.0 * light_scale,
        radius * 1.4,
        (0.52, 0.72, 1.0),
        center,
    )
    add_area(
        "Archive Rim",
        center + Vector((0.2, 1.5, 1.4)).normalized() * radius * 2.0,
        760.0 * light_scale,
        radius * 0.9,
        (0.64, 0.82, 1.0),
        center,
    )

    floor_material = make_material("Archive Floor", (0.025, 0.035, 0.055, 1.0), metallic=0.08, roughness=0.72)
    bpy.ops.mesh.primitive_plane_add(size=radius * 6.0, location=(center.x, center.y, bounds_min.z - radius * 0.025))
    floor = bpy.context.object
    floor.name = "Archive Preview Floor"
    floor.data.materials.append(floor_material)

    world = bpy.data.worlds.new("Archive Preview World") if not scene.world else scene.world
    scene.world = world
    world.use_nodes = True
    background = world.node_tree.nodes.get("Background")
    background.inputs["Color"].default_value = (0.004, 0.008, 0.020, 1.0)
    background.inputs["Strength"].default_value = 0.16

    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x = 640
    scene.render.resolution_y = 640
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.render.filepath = str(output)
    scene.render.film_transparent = False
    scene.render.image_settings.color_mode = "RGBA"
    scene.view_settings.look = "AgX - Medium High Contrast"
    scene.render.resolution_percentage = 100
    bpy.ops.render.render(write_still=True)
    print(f"ARCHIVE_PREVIEW\t{output}")


if __name__ == "__main__":
    main()
