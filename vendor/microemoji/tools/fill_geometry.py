"""Build visible fill geometry from projected outline commands."""

from collections.abc import Callable, Iterable, Sequence
from dataclasses import dataclass
from math import hypot

from shapely.geometry import LineString, Polygon
from shapely.geometry.base import BaseGeometry
from shapely.ops import polygonize, unary_union


Point = tuple[float, float]
Contour = list[Point]
_FLATNESS_PX = 0.02


@dataclass(frozen=True)
class Paint:
    geometry: BaseGeometry
    rgb: tuple[int, int, int]
    alpha: float


@dataclass(frozen=True)
class Glyph:
    paints: list[Paint]
    # Nonzero source commands, including a closing edge when needed.
    source_segments: int


def _distance_to_segment(point: Point, start: Point, end: Point) -> float:
    dx, dy = end[0] - start[0], end[1] - start[1]
    x, y = point[0] - start[0], point[1] - start[1]
    length_squared = dx * dx + dy * dy
    if length_squared == 0:
        return hypot(x, y)
    t = min(1.0, max(0.0, (x * dx + y * dy) / length_squared))
    return hypot(x - dx * t, y - dy * t)


def _flatten_curve(points: list[Point], output: Contour, depth: int = 0) -> None:
    # A Bézier lies in the control polygon's convex hull. Its distance to
    # the chord is bounded by the controls' distances to that segment.
    if all(
        _distance_to_segment(point, points[0], points[-1]) <= _FLATNESS_PX
        for point in points[1:-1]
    ):
        if output[-1] != points[-1]:
            output.append(points[-1])
        return
    if depth >= 32:
        raise ValueError("Bézier exceeds the flattening depth limit")

    # De Casteljau split at t = 1/2, for either a quadratic or a cubic.
    left = [points[0]]
    right = [points[-1]]
    while len(points) > 1:
        points = [
            ((a[0] + b[0]) / 2, (a[1] + b[1]) / 2)
            for a, b in zip(points, points[1:])
        ]
        left.append(points[0])
        right.append(points[-1])
    _flatten_curve(left, output, depth + 1)
    _flatten_curve(right[::-1], output, depth + 1)


def tile_projection(
    left: float, top: float, width: float, height: float, size: int
) -> Callable[[float, float], Point]:
    """Center font-space coordinates in a tile with Y down."""
    if size < 1:
        raise ValueError("tile size must be positive")
    # Match Outline::new in raster_stroke.rs at the one-pixel stroke width.
    margin = min(0.5, size * 0.25)
    scale = (size - 2 * margin) / max(width, height)
    x0 = (size - width * scale) / 2
    y0 = (size - height * scale) / 2

    def project(x: float, y: float) -> Point:
        if not (left <= x <= left + width and top <= -y <= top + height):
            raise ValueError("control point lies outside the glyph viewport")
        return x0 + (x - left) * scale, y0 + (-y - top) * scale

    return project


def _contours(commands: Iterable[tuple[str, Sequence[Point]]]) -> tuple[list[Contour], int]:
    contours = []
    contour: Contour = []
    source_segments = 0
    for command, points in commands:
        if command == "M":
            if contour:
                raise ValueError("expected Z before the next contour")
            contour = list(points)
        elif not contour:
            raise ValueError("expected M before drawing a contour")
        elif command == "Z":
            if contour[0] != contour[-1]:
                contour.append(contour[0])
                source_segments += 1
            if len(contour) >= 4:
                contours.append(contour)
            contour = []
        elif command == "L":
            if points[0] != contour[-1]:
                contour.append(points[0])
                source_segments += 1
        else:
            if any(point != contour[-1] for point in points):
                source_segments += 1
                _flatten_curve([contour[-1], *points], contour)

    if contour:
        raise ValueError("expected Z at the end of a contour")
    return contours, source_segments


def _winding(contours: list[Contour], x: float, y: float) -> int:
    winding = 0
    for contour in contours:
        for (ax, ay), (bx, by) in zip(contour, contour[1:]):
            cross = (bx - ax) * (y - ay) - (by - ay) * (x - ax)
            if ay <= y < by and cross > 0:
                winding += 1
            elif by <= y < ay and cross < 0:
                winding -= 1
    return winding


def _nonzero_fill(contours: list[Contour]) -> BaseGeometry:
    if not contours:
        return Polygon()
    if len(contours) == 1:
        polygon = Polygon(contours[0])
        if polygon.is_valid:
            return polygon

    # Node intersections before polygonizing. Test each face against all
    # original contours so same-winding overlaps stay filled and holes remain.
    linework = unary_union([LineString(contour) for contour in contours])
    faces = []
    for face in polygonize(linework):
        point = face.representative_point()
        if _winding(contours, point.x, point.y):
            faces.append(face)
    return unary_union(faces) if faces else Polygon()


def build_paint(
    commands: Iterable[tuple[str, Sequence[Point]]],
    project: Callable[[float, float], Point],
    rgb: tuple[int, int, int],
    alpha: float,
) -> tuple[Paint, int]:
    """Resolve one nonzero-winding paint in tile coordinates."""
    contours, count = _contours(
        (command, [project(x, y) for x, y in points]) for command, points in commands
    )
    return Paint(_nonzero_fill(contours), rgb, alpha), count
