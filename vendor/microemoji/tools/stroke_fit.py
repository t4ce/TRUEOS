"""Fit dots and round-capped centerlines to visible, filled glyph features."""

from math import atan2, degrees, pi, sqrt
from statistics import median

from shapely.affinity import rotate
from shapely.geometry import LineString, Polygon


def _coverage_overlap(region, footprint):
    intersection = region.intersection(footprint).area
    return intersection / (region.area + footprint.area - intersection)


def _section(region, x, bottom, top):
    crossing = region.intersection(LineString(((x, bottom), (x, top))))
    # Multiple intervals suggest a hole, fork, or a feature that folds back
    # on itself. Leave those regions to the outline pass.
    if crossing.geom_type != "LineString" or crossing.length == 0:
        return None
    _, low, _, high = crossing.bounds
    return (x, (low + high) * 0.5), high - low


def _fit_centerline(region, tolerance):
    box = region.minimum_rotated_rectangle
    if box.geom_type != "Polygon":
        return None
    corners = list(box.exterior.coords)
    edges = [(b[0] - a[0], b[1] - a[1]) for a, b in zip(corners, corners[1:])]
    dx, dy = max(edges, key=lambda edge: edge[0] ** 2 + edge[1] ** 2)
    angle = degrees(atan2(dy, dx)) % 180
    origin = region.centroid.coords[0]
    aligned = rotate(region, -angle, origin=origin)
    left, bottom, right, top = aligned.bounds
    length = right - left
    if length < 2:
        return None

    # Estimate the width away from the ends, where round caps taper toward
    # zero. A narrow band can have a taller overall box when it curves.
    sample_count = max(16, int(length * 4))
    sections = [
        _section(aligned, left + length * i / sample_count, bottom - 1, top + 1)
        for i in range(1, sample_count)
    ]
    if any(section is None for section in sections):
        return None
    width = median(section[1] for section in sections)
    radius = width * 0.5
    if width > 2 or width * 2 > length:
        return None

    first = _section(aligned, left + radius, bottom - 1, top + 1)
    last = _section(aligned, right - radius, bottom - 1, top + 1)
    if first is None or last is None:
        return None
    first_point, _ = first
    last_point, _ = last
    points = [first_point]
    points.extend(point for point, _ in sections if first_point[0] < point[0] < last_point[0])
    points.append(last_point)
    sampled = rotate(LineString(points), angle, origin=origin)
    for error in (tolerance, tolerance * 0.5, 0):
        line = sampled.simplify(error)
        if not region.covers(line) or not line.is_simple:
            continue
        # A round stroke of radius r has area 2*r*length + pi*r*r
        # on a simple segment. Use that width as a candidate and measure
        # the actual buffer for bent strokes.
        area_radius = region.area / (
            sqrt(line.length ** 2 + pi * region.area) + line.length
        )
        candidates = [
            (_coverage_overlap(region, line.buffer(r, quad_segs=16)), r)
            for r in (radius, area_radius)
            if r <= 1
        ]
        overlap, fitted_radius = max(candidates)
        if overlap >= 0.74:
            return {
                "kind": "strip",
                "vertices": [list(point) for point in line.coords],
                "closed": False,
                "width": fitted_radius * 2,
                "overlap": overlap,
            }
    return None


def fit_feature(region: Polygon, tolerance: float) -> dict | None:
    """Return a candidate in tile pixels that passes the geometric fit checks."""
    if region.is_empty or region.interiors or region.area == 0:
        return None
    radius = sqrt(region.area / pi)
    if radius <= 1.75 and region.covers(region.centroid):
        dot = region.centroid.buffer(radius, quad_segs=32)
        overlap = _coverage_overlap(region, dot)
        if overlap >= 0.76 and region.boundary.hausdorff_distance(dot.boundary) <= tolerance:
            return {
                "kind": "dot",
                "center": list(region.centroid.coords[0]),
                "diameter": radius * 2,
                "overlap": overlap,
            }
    return _fit_centerline(region, tolerance)
