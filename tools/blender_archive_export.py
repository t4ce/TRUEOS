"""Export the .blend file Blender opened into draw3d-safe material groups."""

import gzip
import json
import math
import sys
from collections import defaultdict
from pathlib import Path

import bpy


MAX_VERTICES = 950
MAX_TRIANGLES = 1_800


def linear_to_srgb(value):
    value = max(0.0, min(1.0, float(value)))
    if value <= 0.0031308:
        return value * 12.92
    return 1.055 * value ** (1.0 / 2.4) - 0.055


def material_color(material):
    if material is None:
        return (132, 151, 166, 255)
    color = material.diffuse_color
    if material.use_nodes and material.node_tree:
        shader = next((node for node in material.node_tree.nodes if node.type == "BSDF_PRINCIPLED"), None)
        if shader and "Base Color" in shader.inputs and not shader.inputs["Base Color"].is_linked:
            color = shader.inputs["Base Color"].default_value
    return tuple(round(linear_to_srgb(color[index]) * 255.0) for index in range(3)) + (round(color[3] * 255.0),)


def deduplicate(triangles, precision=6):
    vertices = []
    faces = []
    lookup = {}
    seen_faces = set()
    for triangle in triangles:
        face = []
        for point in triangle:
            key = tuple(round(float(value), precision) for value in point)
            index = lookup.get(key)
            if index is None:
                index = len(vertices)
                lookup[key] = index
                vertices.append(tuple(float(value) for value in point))
            face.append(index)
        if len(set(face)) != 3:
            continue
        canonical = tuple(sorted(face))
        if canonical not in seen_faces:
            seen_faces.add(canonical)
            faces.append(tuple(face))
    return vertices, faces


def cluster(vertices, faces, cell_size):
    minimum = [min(vertex[axis] for vertex in vertices) for axis in range(3)]
    sums = []
    counts = []
    lookup = {}
    remap = []
    for vertex in vertices:
        key = tuple(math.floor((vertex[axis] - minimum[axis]) / cell_size) for axis in range(3))
        index = lookup.get(key)
        if index is None:
            index = len(sums)
            lookup[key] = index
            sums.append([0.0, 0.0, 0.0])
            counts.append(0)
        for axis in range(3):
            sums[index][axis] += vertex[axis]
        counts[index] += 1
        remap.append(index)

    clustered_vertices = [
        tuple(sums[index][axis] / counts[index] for axis in range(3))
        for index in range(len(sums))
    ]
    clustered_faces = []
    seen = set()
    for face in faces:
        mapped = tuple(remap[index] for index in face)
        if len(set(mapped)) != 3:
            continue
        canonical = tuple(sorted(mapped))
        if canonical not in seen:
            seen.add(canonical)
            clustered_faces.append(mapped)
    return clustered_vertices, clustered_faces


def simplify(triangles):
    vertices, faces = deduplicate(triangles)
    original = (len(vertices), len(faces))
    if len(vertices) <= MAX_VERTICES and len(faces) <= MAX_TRIANGLES:
        return vertices, faces, original

    spans = [max(vertex[axis] for vertex in vertices) - min(vertex[axis] for vertex in vertices) for axis in range(3)]
    extent = max(spans) or 1.0
    cell_size = extent / 256.0
    best = (vertices, faces)
    for _ in range(24):
        candidate = cluster(vertices, faces, cell_size)
        best = candidate
        if len(candidate[0]) <= MAX_VERTICES and len(candidate[1]) <= MAX_TRIANGLES:
            break
        cell_size *= 1.32
    if len(best[0]) > MAX_VERTICES or len(best[1]) > MAX_TRIANGLES:
        raise RuntimeError(f"could not simplify group below draw3d budget: {len(best[0])} vertices/{len(best[1])} triangles")
    return best[0], best[1], original


def main():
    separator = sys.argv.index("--") if "--" in sys.argv else len(sys.argv)
    arguments = sys.argv[separator + 1 :]
    if not arguments:
        raise SystemExit("output path required after --")
    output = Path(arguments[0]).resolve()
    split_unassigned = "split-unassigned" in arguments[1:]
    output.parent.mkdir(parents=True, exist_ok=True)

    depsgraph = bpy.context.evaluated_depsgraph_get()
    groups = defaultdict(list)
    colors = {}
    source_objects = []
    for obj in sorted((item for item in bpy.data.objects if item.type == "MESH" and not item.hide_render), key=lambda item: item.name):
        evaluated = obj.evaluated_get(depsgraph)
        mesh = evaluated.to_mesh(preserve_all_data_layers=False, depsgraph=depsgraph)
        try:
            mesh.calc_loop_triangles()
            source_objects.append(obj.name)
            slots = [slot.material for slot in obj.material_slots]
            for triangle in mesh.loop_triangles:
                material = slots[triangle.material_index] if triangle.material_index < len(slots) else None
                material_name = material.name if material else (f"Unassigned/{obj.name}" if split_unassigned else "Unassigned")
                key = (material_name, material_color(material))
                colors[key] = key[1]
                points = []
                for vertex_index in triangle.vertices:
                    world = obj.matrix_world @ mesh.vertices[vertex_index].co
                    # Blender is Z-up; draw3d uses Y-up.
                    points.append((float(world.x), float(world.z), float(-world.y)))
                groups[key].append(tuple(points))
        finally:
            evaluated.to_mesh_clear()

    exported = []
    for (name, color), triangles in sorted(groups.items(), key=lambda item: item[0][0]):
        vertices, faces, original = simplify(triangles)
        exported.append(
            {
                "name": name,
                "color": color,
                "vertices": [[round(value, 6) for value in vertex] for vertex in vertices],
                "faces": [list(face) for face in faces],
                "source_vertices": original[0],
                "source_triangles": original[1],
            }
        )

    payload = {
        "source": bpy.data.filepath,
        "objects": source_objects,
        "groups": exported,
    }
    with gzip.open(output, "wt", encoding="utf-8") as handle:
        json.dump(payload, handle, ensure_ascii=False, separators=(",", ":"))
    print(
        "ARCHIVE_EXPORT\t"
        + json.dumps(
            {
                "output": str(output),
                "objects": len(source_objects),
                "groups": len(exported),
                "vertices": sum(len(group["vertices"]) for group in exported),
                "triangles": sum(len(group["faces"]) for group in exported),
            },
            separators=(",", ":"),
        )
    )


if __name__ == "__main__":
    main()
