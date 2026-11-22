"""Fold nearly circular strokes into circles and circular arcs."""

from math import asin, ceil, cos, isfinite, pi, sin, sqrt, tau

import numpy as np
from shapely import hausdorff_distance
from shapely.geometry import LineString


MIN_ARC_SWEEP = pi / 5
MAX_ANGLE_STEP = 5 * pi / 12
FIT_SHARE = 0.35
GRID_MARGIN = 0.0002


def _segment_count(radius, sweep, sagitta):
    # Sagitta = 2*r*sin(chord_angle/4)^2, stable even for shallow arcs.
    angle = 4 * asin(sqrt(min(sagitta / (2 * radius), 0.5)))
    return ceil(abs(sweep) / angle)


def _circle_center(points):
    origin = points.mean(axis=0)
    centered = points - origin
    squared = np.sum(centered * centered, axis=1)
    try:
        offset = np.linalg.solve(centered.T @ centered, (centered.T @ squared) * 0.5)
    except np.linalg.LinAlgError:
        return None
    center = origin + offset
    return center, float(np.linalg.norm(points - center, axis=1).mean())


def _arc_center(points):
    chord = points[-1] - points[0]
    half_length = float(np.linalg.norm(chord)) / 2
    if half_length <= GRID_MARGIN:
        return None
    midpoint = (points[0] + points[-1]) * 0.5
    normal = np.array((-chord[1], chord[0])) / (2 * half_length)
    offset = points[1:-1] - midpoint
    projection = offset @ normal
    divisor = 2 * (projection @ projection)
    if divisor <= GRID_MARGIN ** 2:
        return None
    # Constrain the center to the endpoint bisector so the fitted arc keeps
    # both endpoints when it joins the adjacent portions of the strip.
    lambda_ = float(projection @ (np.sum(offset * offset, axis=1) - half_length ** 2) / divisor)
    center = midpoint + lambda_ * normal
    return center, float(np.linalg.norm(points[0] - center))


def _radial_error(vectors, radius, closed):
    vectors = np.vstack((vectors, vectors[0])) if closed else vectors
    a = vectors[:-1]
    delta = vectors[1:] - a
    t = np.clip(-np.sum(a * delta, axis=1) / np.sum(delta * delta, axis=1), 0, 1)
    # On each source segment, the least distance to the center lies at the
    # projected point. The greatest distance occurs at an endpoint.
    nearest = np.linalg.norm(a + t[:, None] * delta, axis=1)
    endpoints = np.linalg.norm(vectors, axis=1)
    return max(radius - float(nearest.min()), float(endpoints.max()) - radius)


def _fit(points, closed, tolerance):
    if len(points) < (6 if closed else 5):
        return None
    fit = _circle_center(points) if closed else _arc_center(points)
    if fit is None:
        return None
    center, radius = fit
    if radius <= GRID_MARGIN or not isfinite(radius):
        return None

    vectors = points - center
    if np.max(np.abs(np.linalg.norm(vectors, axis=1) - radius)) > tolerance * FIT_SHARE:
        return None
    angles = np.arctan2(vectors[:, 1], vectors[:, 0])
    if closed:
        angles = np.append(angles, angles[0])
    unwrapped = np.unwrap(angles)
    sweep = float(unwrapped[-1] - unwrapped[0])
    steps = np.diff(unwrapped) * (1 if sweep > 0 else -1)
    if np.any(steps <= 0) or np.any(steps > MAX_ANGLE_STEP):
        return None
    if closed:
        if abs(abs(sweep) - tau) > 1e-6:
            return None
        sweep = tau if sweep > 0 else -tau
    elif not MIN_ARC_SWEEP <= abs(sweep) < tau:
        return None

    error = _radial_error(vectors, radius, closed)
    limit = tolerance - GRID_MARGIN
    if error > tolerance * FIT_SHARE or error >= limit:
        return None
    least = max(3 if closed else 1, _segment_count(radius, sweep, limit))
    guaranteed = _segment_count(radius, sweep, limit - error)
    budget = len(points) if closed else len(points) - 1
    if least > budget:
        return None

    source = LineString(np.vstack((points, points[0])) if closed else points)
    start_angle = float(unwrapped[0])
    for count in range(least, min(budget, max(least, guaranteed)) + 1):
        output = [
            [float(center[0] + radius * cos(angle)), float(center[1] + radius * sin(angle))]
            for angle in (start_angle + sweep * i / count for i in range(count + 1))
        ]
        if closed:
            output[-1] = output[0]
        else:
            output[0], output[-1] = points[0].tolist(), points[-1].tolist()
        candidate = LineString(output)
        if candidate.is_simple and hausdorff_distance(source, candidate, densify=0.25) <= limit:
            return output, {
                "kind": "circle" if closed else "arc",
                "center": center.tolist(),
                "radius": radius,
                "start_angle": start_angle,
                "sweep": sweep,
                "segment_count": count,
            }
    return None


def _turns(points):
    edges = np.diff(points, axis=0)
    earlier, later = edges[:-1], edges[1:]
    cross = earlier[:, 0] * later[:, 1] - earlier[:, 1] * later[:, 0]
    return np.arctan2(cross, np.sum(earlier * later, axis=1))


def _search_runs(points, tolerance):
    """Yield useful circular runs, using turn changes to bound the search."""
    turns = _turns(points)
    # Sharp corners and changes of curvature split likely circular runs.
    # A small allowed opposite turn covers paint intersections on almost
    # collinear edges without letting views span unrelated contours.
    breakpoints = [
        np.append(
            np.flatnonzero((turns * sign < -0.08) | (np.abs(turns) > pi / 3)) + 1,
            len(points) - 1,
        )
        for sign in (1, -1)
    ]
    blockers = [np.searchsorted(b, np.arange(1, len(points) + 1)) for b in breakpoints]
    first = 0
    while first + 4 < len(points):
        # Walk the two same-turn ranges with exponentially spaced end
        # candidates. Also try each range's last point for a complete arc.
        bounds = {int(b[indices[first]]) for b, indices in zip(breakpoints, blockers)}
        longest = max(bounds)
        last = first + 4
        endings = {end for end in bounds if end >= last}
        while last < longest:
            endings.add(last)
            last += max(2, (last - first) // 2)
        best = None
        for end in sorted(endings, reverse=True):
            # A shallow run does not need a circular primitive.
            a, m, b = points[[first, (first + end) // 2, end]]
            chord, offset = b - a, m - a
            length = float(np.linalg.norm(chord))
            if length <= GRID_MARGIN:
                continue
            if abs(chord[0] * offset[1] - chord[1] * offset[0]) < length * tolerance * 0.6:
                continue
            fit = _fit(points[first : end + 1], False, tolerance)
            if fit is not None and fit[1]["segment_count"] < end - first:
                best = end, fit
                break
        if best is None:
            first += 1
        else:
            end, (vertices, description) = best
            yield first, end, vertices, description
            first = end


def _append_gap(output, points, tolerance):
    if len(points) < 2:
        return
    gap = LineString(points).simplify(tolerance, preserve_topology=True)
    output.extend(list(point) for point in gap.coords[1:])


def fold_circular(vertices, closed, tolerance):
    """Return a fitted polyline and its circular primitive descriptions.

    Angles are radians with positive sweep from +X toward +Y. A primitive's
    first vertex and segment count index into the emitted polyline, wrapping
    across its closure if needed. Open strips keep their original endpoints.
    Closed strips omit the repeated final vertex.
    """
    points = []
    for x, y in vertices:
        point = [float(x), float(y)]
        if not points or point != points[-1]:
            points.append(point)
    if closed and points and points[0] == points[-1]:
        points.pop()
    if tolerance <= GRID_MARGIN or len(points) < 5:
        return None
    source = np.array(points)
    whole = _fit(source, closed, tolerance)
    if whole is not None:
        output, description = whole
        return (output[:-1] if closed else output), [{"first_vertex": 0, **description}]

    if closed:
        # Break a closed mixed path at its sharpest turn so smooth arcs can
        # span the original SVG ring's arbitrary starting vertex.
        cycle = np.vstack((source[-1], source, source[0]))
        start = int(np.argmax(np.abs(_turns(cycle))))
        source = np.vstack((source[start:], source[: start + 1]))

    output = [source[0].tolist()]
    descriptions = []
    cursor = 0
    for start, stop, arc, description in _search_runs(source, tolerance):
        _append_gap(output, source[cursor : start + 1], tolerance)
        descriptions.append({"first_vertex": len(output) - 1, **description})
        output.extend(arc[1:])
        cursor = stop
    if not descriptions:
        return None
    _append_gap(output, source[cursor:], tolerance)
    candidate = LineString(output)
    if not candidate.is_simple or hausdorff_distance(
        LineString(source), candidate, densify=0.25
    ) > tolerance - GRID_MARGIN:
        return None
    return (output[:-1] if closed else output), descriptions
