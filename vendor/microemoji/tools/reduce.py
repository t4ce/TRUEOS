#!/usr/bin/env python3
"""Extract monochrome stroke candidates from a color Microemoji bundle."""

import argparse
import json
import re
from collections import Counter, defaultdict
from functools import partial
from math import isfinite
from pathlib import Path

try:
    from shapely import hausdorff_distance, line_merge, set_precision, unary_union
    from shapely.errors import GEOSException
    from shapely.geometry import LineString, MultiPolygon, Polygon
except ModuleNotFoundError as error:
    if error.name == "shapely":
        raise SystemExit("Install Shapely in the host environment to run this tool.") from error
    raise

from circular_fit import fold_circular
from meoj_geometry import read_bundle
from stroke_fit import fit_feature
from svg_geometry import read_svg

GRID = 0.0001
MIN_SEAM_LENGTH = 0.5
FEATURE_BOUNDARY_SHARE = 0.9
GLYPH_NAME = re.compile(r"u([0-9a-fA-F]{4,6})\.svg")


def parts(geometry, kind):
    if geometry.is_empty:
        return
    if geometry.geom_type == kind:
        yield geometry
    elif hasattr(geometry, "geoms"):
        for part in geometry.geoms:
            yield from parts(part, kind)


def luminance(rgb):
    srgb = (value / 255 for value in rgb)
    linear = (c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4 for c in srgb)
    return sum(value * weight for value, weight in zip(linear, (0.2126, 0.7152, 0.0722)))


def visible_regions(paints):
    """Resolve opaque paint in reverse order, then dissolve equal-color seams."""
    coverage = Polygon()
    colors = defaultdict(list)
    for paint in reversed(paints):
        if paint.alpha == 0:
            continue
        shape = set_precision(paint.geometry, GRID)
        visible = shape.difference(coverage)
        if not visible.is_empty:
            colors[paint.rgb].append(visible)
        coverage = coverage.union(shape)
    return coverage, [(luminance(rgb), unary_union(shapes)) for rgb, shapes in colors.items()]


def internal_seams(regions, exterior, min_contrast):
    high, low = [], []
    for i, (first, a) in enumerate(regions):
        for second, b in regions[i + 1 :]:
            shared = a.boundary.intersection(b.boundary).difference(exterior)
            if shared.length <= GRID:
                continue
            ratio = (max(first, second) + 0.05) / (min(first, second) + 0.05)
            (high if ratio >= min_contrast else low).extend(parts(shared, "LineString"))
    return unary_union(high), unary_union(low).length


def checked_point(point, size):
    x, y = (round(float(value), 4) for value in point)
    if not (isfinite(x) and isfinite(y) and 0 <= x <= size and 0 <= y <= size):
        raise ValueError("stroke point outside the tile")
    return [x, y]


def make_strip(vertices, closed, size, *, role, width=1.0):
    """Encode actual drawn vertices followed by an independent adjacency index list."""
    points = []
    for vertex in vertices:
        point = checked_point(vertex, size)
        if not points or points[-1] != point:
            points.append(point)
    if closed and points and points[0] == points[-1]:
        points.pop()
    count = len(points)
    width = round(width, 4)
    if count < (3 if closed else 2) or not 0 < width <= size:
        raise ValueError("invalid stroke strip")
    if closed:
        adjacency = [count - 1, *range(count), 0, 1]
    else:
        adjacency = [0, *range(count), count - 1]
    return {
        "kind": "strip",
        "role": role,
        "closed": closed,
        "width": width,
        "vertices": points,
        "adjacency_indices": adjacency,
    }


def simplification_matches(original, simplified, tolerance):
    before = list(parts(original, "Polygon"))
    after = list(parts(simplified, "Polygon"))
    if before:
        if sorted(len(p.interiors) for p in before) != sorted(len(p.interiors) for p in after):
            return False
        original, simplified = original.boundary, simplified.boundary
    elif not simplified.is_simple or original.is_closed != simplified.is_closed:
        return False
    # Densify between vertices for a sampled two-way geometry check.
    return hausdorff_distance(original, simplified, densify=0.25) <= tolerance


def simplify(geometry, tolerance):
    for step in (tolerance, tolerance / 2):
        result = geometry.simplify(step, preserve_topology=True)
        if not result.is_empty and simplification_matches(geometry, result, tolerance):
            return result
    return geometry


def strip_line(strip):
    points = strip["vertices"]
    return LineString(points + points[:1] if strip["closed"] else points)


def outline_strip(source, fallback, size, tolerance, role):
    closed = source.is_ring
    ordinary = make_strip(fallback.coords, closed, size, role=role)
    folded = fold_circular(source.coords, closed, tolerance)
    if folded is None:
        return ordinary
    points, arcs = folded
    if len(points) > len(ordinary["vertices"]):
        return ordinary
    try:
        candidate = make_strip(points, closed, size, role=role)
    except ValueError:
        return ordinary
    if len(candidate["vertices"]) != len(points):
        return ordinary
    if not simplification_matches(source, strip_line(candidate), tolerance):
        return ordinary
    candidate["circular_parts"] = arcs
    return candidate


def fitted_stroke(region, size, tolerance, role):
    fit = fit_feature(region, tolerance)
    if fit is None:
        return None
    if fit["kind"] == "dot":
        center = checked_point(fit["center"], size)
        diameter = round(fit["diameter"], 4)
        if not 0 < diameter <= 2 * min(*center, *(size - p for p in center)):
            return None
        return {
            "kind": "dot", "role": role, "center": center, "diameter": diameter,
            "fit_overlap": round(fit["overlap"], 4),
        }
    strip = make_strip(fit["vertices"], False, size, role=role, width=fit["width"])
    strip["fit_overlap"] = round(fit["overlap"], 4)
    return strip


def boundary_strokes(silhouette, size, tolerance):
    if silhouette.is_empty:
        return
    baseline = simplify(silhouette, tolerance)
    ordinary = []
    for component in parts(baseline, "Polygon"):
        ordinary.append(make_strip(component.exterior.coords, True, size, role="silhouette"))
        ordinary.extend(make_strip(ring.coords, True, size, role="hole") for ring in component.interiors)

    candidates = []
    for component in parts(silhouette, "Polygon"):
        fallback = simplify(component, tolerance)
        outlines = [component.exterior, *component.interiors]
        plain = [fallback.exterior, *fallback.interiors]
        roles = ["silhouette", *("hole" for _ in component.interiors)]
        candidates.append([
            outline_strip(source, ring, size, tolerance, role)
            for source, ring, role in zip(outlines, plain, roles, strict=True)
        ])

    folded_strokes = [stroke for component in candidates for stroke in component]
    if (not any("circular_parts" in stroke for stroke in folded_strokes)
            or sum(len(s["vertices"]) for s in folded_strokes) > sum(len(s["vertices"]) for s in ordinary)):
        yield from ordinary
        return

    # A ring can pass its own error check and still cross a neighboring ring.
    # Verify the entire set of shells and holes before retaining any folds.
    geometry = MultiPolygon([
        Polygon(strokes[0]["vertices"], [s["vertices"] for s in strokes[1:]])
        for strokes in candidates
    ])
    if geometry.is_valid and simplification_matches(silhouette, geometry, tolerance):
        yield from folded_strokes
        return
    yield from ordinary


def summarize(source_segments, strokes, review, *, unsupported=False, omitted_seam_px=0.0):
    strips = [stroke for stroke in strokes if stroke["kind"] == "strip"]
    circular = Counter(part["kind"] for stroke in strips for part in stroke.get("circular_parts", []))
    return {
        "status": "unsupported" if unsupported else "review" if review else "candidate",
        "review": review,
        "source_segments": source_segments,
        "line_segments": sum(len(s["adjacency_indices"]) - 3 for s in strips),
        "vertices": sum(len(s["vertices"]) for s in strips),
        "adjacency_indices": sum(len(s["adjacency_indices"]) for s in strips),
        "strips": len(strips),
        "dots": len(strokes) - len(strips),
        "circles": circular["circle"],
        "arcs": circular["arc"],
        "omitted_seam_px": round(omitted_seam_px, 3),
        "strokes": strokes,
    }


def reduce_glyph(glyph, size, tolerance, min_contrast):
    if any(0 < paint.alpha < 1 for paint in glyph.paints):
        return summarize(glyph.source_segments, [], ["translucent paint"], unsupported=True)

    silhouette, colors = visible_regions(glyph.paints)
    if silhouette.is_empty:
        return summarize(glyph.source_segments, [], ["empty visible geometry"], unsupported=True)

    boundary = silhouette.boundary
    seams, omitted = internal_seams(colors, boundary, min_contrast)
    strokes = []
    review = []
    if omitted >= MIN_SEAM_LENGTH:
        review.append("low-contrast seams omitted")

    # Fit detached exterior marks before simplifying the other rings together.
    unfit = []
    for component in parts(silhouette, "Polygon"):
        stroke = fitted_stroke(component, size, tolerance, "silhouette")
        if stroke is None:
            unfit.append(component)
        else:
            strokes.append(stroke)
    strokes.extend(boundary_strokes(unary_union(unfit), size, tolerance))

    # A strongly contrasted, enclosed paint island can represent a single
    # mark. Fit it only when most of its outline currently contributes a seam.
    for _, color in colors:
        for region in parts(color, "Polygon"):
            edge = region.boundary
            if edge.intersection(boundary).length > GRID:
                continue
            shared = seams.intersection(edge)
            if shared.length < FEATURE_BOUNDARY_SHARE * edge.length:
                continue
            stroke = fitted_stroke(region, size, tolerance, "feature")
            if stroke is not None:
                strokes.append(stroke)
                seams = seams.difference(shared)

    # Leave still-visible contrasts as open or closed boundary strips and
    # flag them for decisions about which marks matter to the glyph.
    seam_lines = list(parts(seams, "LineString"))
    if seam_lines:
        short_seams = 0.0
        for line in parts(line_merge(unary_union(seam_lines)), "LineString"):
            if line.length < MIN_SEAM_LENGTH:
                short_seams += line.length
                continue
            strokes.append(outline_strip(
                line, simplify(line, tolerance), size, tolerance, "contrast seam",
            ))
        if short_seams > GRID:
            review.append("short contrast seams omitted")
            omitted += short_seams
        if any(stroke["role"] == "contrast seam" for stroke in strokes):
            review.append("contrast seams retained")
    return summarize(glyph.source_segments, strokes, review, omitted_seam_px=omitted)


def read_svg_set(directory):
    glyphs = {}
    for path in directory.glob("u*.svg"):
        match = GLYPH_NAME.fullmatch(path.name)
        if match:
            scalar = int(match[1], 16)
            if scalar > 0x10FFFF or 0xD800 <= scalar <= 0xDFFF:
                raise ValueError(f"invalid Unicode scalar in {path.name}")
            if scalar in glyphs:
                raise ValueError(f"duplicate SVG for U+{scalar:04X}")
            glyphs[scalar] = partial(read_svg, path)
    if not glyphs:
        raise ValueError("no microemoji SVG files found")
    return sorted(glyphs.items())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="color microemoji.bin, or a directory of exported SVGs")
    parser.add_argument("output", type=Path, help="geometry and review JSON")
    parser.add_argument("--size", default=17, type=int, help="square tile size in pixels")
    parser.add_argument("--tolerance", default=0.25, type=float, help="line-fit tolerance in pixels")
    parser.add_argument("--contrast", default=3.0, type=float, help="minimum internal seam contrast")
    args = parser.parse_args()
    if not 1 <= args.size <= 256 or not 0 < args.tolerance <= 1 or not 1 <= args.contrast <= 21:
        parser.error("expected size in 1–256, tolerance in (0, 1], and contrast in 1–21")
    try:
        if args.source.is_dir():
            sources = read_svg_set(args.source)
        else:
            sources = [(scalar, glyph.project) for scalar, glyph in read_bundle(args.source)]
            if not sources:
                raise ValueError("no glyphs in the microemoji bundle")
    except (ValueError, OSError) as error:
        parser.error(str(error))

    glyphs = []
    for scalar, load in sources:
        source_segments = 0
        try:
            glyph = load(args.size)
            source_segments = glyph.source_segments
            result = reduce_glyph(glyph, args.size, args.tolerance, args.contrast)
        except (ValueError, OSError, GEOSException) as error:
            result = summarize(source_segments, [], [str(error)], unsupported=True)
        glyphs.append({"glyph": f"U+{scalar:04X}", **result})

    totals = {
        key: sum(glyph[key] for glyph in glyphs)
        for key in (
            "source_segments", "line_segments", "vertices", "adjacency_indices",
            "strips", "dots", "circles", "arcs",
        )
    }
    statuses = Counter(glyph["status"] for glyph in glyphs)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps({
        "tile_px": [args.size, args.size],
        "coordinates": "pixels from the top left, y down",
        "line_tolerance_px": args.tolerance,
        "min_contrast": args.contrast,
        "summary": {
            "glyphs": len(glyphs),
            **{status: statuses[status] for status in ("candidate", "review", "unsupported")},
            **totals,
        },
        "glyphs": glyphs,
    }, indent=2, allow_nan=False) + "\n", encoding="utf-8")
    print(f"{len(glyphs)} glyphs, {totals['line_segments']:,} line segments, {totals['dots']} dots")
    print(f"{totals['circles']} circular loops, {totals['arcs']} circular arcs")
    print(f"{statuses['review']} flagged for review, {statuses['unsupported']} unsupported")
    return 1 if statuses["unsupported"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
