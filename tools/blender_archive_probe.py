"""Print a compact JSON inventory for the .blend file Blender opened."""

import json
import sys

import bpy


def rounded(vector):
    return [round(float(value), 4) for value in vector]


def main():
    mesh_objects = [obj for obj in bpy.data.objects if obj.type == "MESH"]
    type_counts = {}
    for obj in bpy.data.objects:
        type_counts[obj.type] = type_counts.get(obj.type, 0) + 1

    bounds_min = [float("inf")] * 3
    bounds_max = [float("-inf")] * 3
    for obj in mesh_objects:
        for corner in obj.bound_box:
            world = obj.matrix_world @ __import__("mathutils").Vector(corner)
            for axis in range(3):
                bounds_min[axis] = min(bounds_min[axis], world[axis])
                bounds_max[axis] = max(bounds_max[axis], world[axis])

    if not mesh_objects:
        bounds_min = bounds_max = [0.0, 0.0, 0.0]

    payload = {
        "file": bpy.data.filepath,
        "version": list(bpy.app.version),
        "scenes": [scene.name for scene in bpy.data.scenes],
        "objects": len(bpy.data.objects),
        "types": type_counts,
        "mesh_objects": [
            {
                "name": obj.name,
                "vertices": len(obj.data.vertices),
                "polygons": len(obj.data.polygons),
                "materials": [slot.material.name for slot in obj.material_slots if slot.material],
            }
            for obj in sorted(mesh_objects, key=lambda item: item.name)
        ],
        "vertices": sum(len(obj.data.vertices) for obj in mesh_objects),
        "polygons": sum(len(obj.data.polygons) for obj in mesh_objects),
        "materials": [material.name for material in bpy.data.materials],
        "images": [image.filepath for image in bpy.data.images if image.filepath],
        "bounds": [rounded(bounds_min), rounded(bounds_max)],
        "dimensions": rounded([bounds_max[index] - bounds_min[index] for index in range(3)]),
    }
    print("ARCHIVE_PROBE\t" + json.dumps(payload, ensure_ascii=False, separators=(",", ":")))
    summary = {
        "file": bpy.data.filepath,
        "objects": len(bpy.data.objects),
        "types": type_counts,
        "vertices": payload["vertices"],
        "polygons": payload["polygons"],
        "dimensions": payload["dimensions"],
        "mesh_names": [obj.name for obj in sorted(mesh_objects, key=lambda item: item.name)],
        "materials": payload["materials"],
    }
    print("ARCHIVE_SUMMARY\t" + json.dumps(summary, ensure_ascii=False, separators=(",", ":")))


if __name__ == "__main__":
    main()
