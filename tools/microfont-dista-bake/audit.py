#!/usr/bin/env python3
"""Independent, slow Dista geometry oracle; this does not execute a GPU shader."""

from __future__ import annotations

import argparse
import ast
from dataclasses import dataclass
import json
import math
from pathlib import Path
import re


ROOT = Path(__file__).resolve().parents[2]
FONT = ROOT / "vendor/microfont"
DEMO = "ABC123/Yy%"


@dataclass(frozen=True)
class Seed:
    pixels: int
    long_reach: bool = False

    def sites(self) -> list[tuple[int, int]]:
        return [(i % 6, i // 6) for i in range(66) if self.pixels & (1 << (65 - i))]


@dataclass(frozen=True)
class Parameters:
    sigma: float
    thresholds: dict[str, float]
    short_reach: float
    long_reach: float

    def reach(self, seed: Seed) -> float:
        return self.long_reach if seed.long_reach else self.short_reach


def rust_char(literal: str) -> str:
    if literal.startswith(r"\u{"):
        return chr(int(literal[3:-1], 16))
    return ast.literal_eval("'" + literal + "'")


def load_seeds(font: Path = FONT) -> dict[str, Seed]:
    """Read Rust data, including z(bits, shift), without importing bake.py."""
    source = (font / "src/basic.rs").read_text()
    table = source.split("pub const BASIC_SYMBOLS:", 1)[1].split("];", 1)[0]
    entries = re.findall(
        r"\('((?:\\.|[^'\\])*)',\s*(z\(0x[0-9a-fA-F]+,\s*\d+\)|0x[0-9a-fA-F]+|0)\),",
        table,
    )
    seeds = {}
    for literal, value in entries:
        if value.startswith("z("):
            bits, shift = value[2:-1].split(",")
            pixels = int(bits, 16) << int(shift)
        else:
            pixels = int(value, 0)
        seeds[rust_char(literal)] = Seed(pixels)
    if set(seeds) != {chr(i) for i in range(128)}:
        raise ValueError("expected all 128 Rust BASIC_SYMBOLS entries")
    overrides = source.split("pub const BASIC_RASTER_OVERRIDES:", 1)[1].split("];", 1)[0]
    for literal, pixels, reach in re.findall(
        r"\('((?:\\.|[^'\\])*)',\s*crate::raster::GlyphSeed\s*\{\s*"
        r"pixels:\s*(0x[0-9a-fA-F]+),\s*long_reach:\s*(true|false)\s*\}\)",
        overrides,
    ):
        seeds[rust_char(literal)] = Seed(int(pixels, 16), reach == "true")
    if any(seed.pixels >> 66 for seed in seeds.values()):
        raise ValueError("seed exceeds its 6 by 11 cell")
    return seeds


def load_parameters(font: Path = FONT) -> Parameters:
    shape = (font / "src/fontshape.rs").read_text()
    raster = (font / "src/raster.rs").read_text()
    sigma = float(re.search(r"INFLUENCE_WIDTH:\s*f64\s*=\s*([\d.]+)", shape)[1])
    thresholds = {
        name: float(value)
        for name, value in re.findall(r"Self::(Meso|Lean)\s*=>\s*([\d.]+)", shape)
    }
    reaches = {
        name: float(value)
        for name, value in re.findall(
            r"(SHORT|LONG)_CONNECTION_REACH:\s*f64\s*=\s*([\d.]+)", raster
        )
    }
    return Parameters(sigma, thresholds, reaches["SHORT"], reaches["LONG"])


def load_shader_offsets(font: Path = FONT) -> list[tuple[int, int]]:
    shader = (font / "shaders/dista_stamp.vert").read_text()
    # Decode the shader's packed constant values. A literal constant array
    # needs a compiler-owned memory binding in the native compiler backend.
    function = shader.split("ivec2 link_offset(uint index)", 1)[1].split("}", 1)[0]
    x_bits, y_bits = [int(value, 16) for value in re.findall(r"(0x[0-9a-fA-F]+)u", function)]
    return [(((x_bits >> (3 * i)) & 7) - 2, (y_bits >> (2 * i)) & 3) for i in range(10)]


def squared(a: tuple[float, float], b: tuple[float, float]) -> float:
    return (a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2


def canonical_links(seed: Seed, parameters: Parameters) -> set[tuple[tuple[int, int], tuple[int, int]]]:
    """All pairs and all third sites: the full closed-disc CPU definition."""
    sites = seed.sites()
    result = set()
    reach2 = parameters.reach(seed) ** 2
    for i, a in enumerate(sites):
        for b in sites[i + 1 :]:
            d2 = squared(a, b)
            if d2 >= reach2:
                continue
            if any(
                p != a and p != b
                and (2 * p[0] - a[0] - b[0]) ** 2 + (2 * p[1] - a[1] - b[1]) ** 2 <= d2
                for p in sites
            ):
                continue
            result.add((a, b))
    return result


def shader_blockers(delta: tuple[int, int]) -> set[tuple[int, int]]:
    """Host reconstruction of GLSL's expanded-box integer pruning search."""
    dx, dy = delta
    return {
        (x, y)
        for y in range(min(0, dy) - 1, max(0, dy) + 2)
        for x in range(min(0, dx) - 1, max(0, dx) + 2)
        if (x, y) not in [(0, 0), delta]
        and (2 * x - dx) ** 2 + (2 * y - dy) ** 2 <= dx * dx + dy * dy
    }


def shader_links(seed: Seed, parameters: Parameters, offsets: list[tuple[int, int]]) -> set:
    sites = set(seed.sites())
    result = set()
    reach2 = parameters.reach(seed) ** 2
    for a in sites:
        for delta in offsets:
            b = (a[0] + delta[0], a[1] + delta[1])
            if b not in sites or squared(a, b) >= reach2:
                continue
            if any((a[0] + x, a[1] + y) in sites for x, y in shader_blockers(delta)):
                continue
            result.add(tuple(sorted((a, b), key=lambda p: (p[1], p[0]))))
    return result


def link_weight(a: tuple[int, int], b: tuple[int, int], reach: float) -> float:
    t = max(0.0, min(1.0, (reach - math.sqrt(squared(a, b))) / 0.30))
    return t * t * (3.0 - 2.0 * t)


def radius(parameters: Parameters, threshold: float, weight: float = 1.0) -> float:
    return parameters.sigma * math.sqrt(-2.0 * math.log(threshold / weight))


def circle_segments(radius_px: float, error_px: float) -> int:
    if not math.isfinite(radius_px) or radius_px <= 0:
        raise ValueError("radius must be finite and positive")
    if not math.isfinite(error_px) or error_px <= 0:
        raise ValueError("error must be finite and positive")
    angle = math.acos(max(-1.0, 1.0 - error_px / radius_px))
    if angle == 0.0:
        raise ValueError("requested error is below floating point precision")
    return max(8, 4 * math.ceil(math.pi / angle / 4))


def segment_distance2(p: tuple[float, float], a: tuple[int, int], b: tuple[int, int]) -> float:
    # Sites in the Rust field are at lattice coordinates plus half a unit.
    ax, ay = a[0] + 0.5, a[1] + 0.5
    dx, dy = b[0] - a[0], b[1] - a[1]
    t = max(0.0, min(1.0, ((p[0] - ax) * dx + (p[1] - ay) * dy) / (dx * dx + dy * dy)))
    return squared(p, (ax + t * dx, ay + t * dy))


def rectangle_contains(p: tuple[float, float], a: tuple[int, int], b: tuple[int, int], half_width: float) -> bool:
    """Finite link rectangle only; its capsule caps must come from site disks."""
    dx, dy = b[0] - a[0], b[1] - a[1]
    px, py = p[0] - a[0] - 0.5, p[1] - a[1] - 0.5
    length2 = dx * dx + dy * dy
    along = px * dx + py * dy
    cross = px * dy - py * dx
    return 0 <= along <= length2 and cross * cross <= half_width**2 * length2


def compact_gaussian(distance2: float, sigma: float) -> float:
    q2 = distance2 / sigma**2
    if q2 >= 12.25:
        return 0.0
    value = math.exp(-0.5 * q2)
    if q2 > 9.0:
        t = (math.sqrt(q2) - 3.0) / 0.5
        value *= 1.0 - t * t * (3.0 - 2.0 * t)
    return value


def raster_compare(seed: Seed, parameters: Parameters, offsets: list, step: float) -> dict:
    """Compare original Gaussian union with independently reconstructed GPU links.

    This compares ideal envelopes, not polygon approximation or GPU execution.
    """
    if not math.isfinite(step) or step <= 0:
        raise ValueError("raster step must be finite and positive")
    sites = [(x + 0.5, y + 0.5) for x, y in seed.sites()]
    canonical = canonical_links(seed, parameters)
    accelerated = shader_links(seed, parameters, offsets)
    reach = parameters.reach(seed)
    gaussian_links = [(a, b, link_weight(a, b, reach)) for a, b in canonical]
    differences = {name: 0 for name in parameters.thresholds}
    samples = 0
    for iy in range(math.ceil(12 / step)):
        for ix in range(math.ceil(7 / step)):
            p = (-0.5 + (ix + 0.5) * step, -0.5 + (iy + 0.5) * step)
            value = max((compact_gaussian(squared(p, site), parameters.sigma) for site in sites), default=0.0)
            for a, b, weight in gaussian_links:
                value = max(value, weight * compact_gaussian(segment_distance2(p, a, b), parameters.sigma))
            nearest_site = min((squared(p, site) for site in sites), default=math.inf)
            for name, threshold in parameters.thresholds.items():
                inside = nearest_site <= radius(parameters, threshold) ** 2
                for a, b in accelerated:
                    weight = link_weight(a, b, reach)
                    if weight > threshold and rectangle_contains(p, a, b, radius(parameters, threshold, weight)):
                        inside = True
                        break
                differences[name] += int(inside != (value >= threshold))
            samples += 1
    return {"samples_per_shape": samples, "classification_mismatches": differences}


def audit(text: str, error_px: float = 0.125, glyph_width: float = 96.0, raster_step: float = 0.0) -> dict:
    seeds, parameters, offsets = load_seeds(), load_parameters(), load_shader_offsets()
    if not math.isfinite(glyph_width) or glyph_width <= 0:
        raise ValueError("glyph width must be finite and positive")
    scale = glyph_width / 6
    shapes = {}
    for name, threshold in parameters.thresholds.items():
        radius_px = radius(parameters, threshold) * scale
        segments = circle_segments(radius_px, error_px)
        shapes[name] = {
            "threshold": threshold, "radius_source": radius(parameters, threshold),
            "radius_px": radius_px, "circle_segments": segments,
            "sagitta_px": radius_px * (1 - math.cos(math.pi / segments)),
            "active_triangles": 0, "submitted_triangles": 0,
        }
    glyphs = []
    for char in text:
        if char not in seeds:
            raise ValueError(f"not a Basic Latin character: {char!r}")
        seed = seeds[char]
        links = canonical_links(seed, parameters)
        accelerated = shader_links(seed, parameters, offsets)
        if links != accelerated:
            raise AssertionError(f"shader graph differs from full closed-disc oracle for {char!r}")
        sites = len(seed.sites())
        candidate_offsets = sum(dx * dx + dy * dy < parameters.reach(seed) ** 2 for dx, dy in offsets)
        weights = [link_weight(a, b, parameters.reach(seed)) for a, b in links]
        glyph = {
            "glyph": char, "codepoint": f"U+{ord(char):04X}", "sites": sites,
            "long_reach": seed.long_reach, "links": len(links),
            "minimum_link_weight": min(weights, default=None), "shapes": {},
        }
        for name, shape in shapes.items():
            visible_links = sum(weight > shape["threshold"] for weight in weights)
            active = sites * shape["circle_segments"] + 2 * visible_links
            submitted = sites * (shape["circle_segments"] + 2 * candidate_offsets)
            glyph["shapes"][name] = {"active_triangles": active, "submitted_triangles": submitted}
            shape["active_triangles"] += active
            shape["submitted_triangles"] += submitted
        if raster_step:
            glyph["raster_comparison"] = raster_compare(seed, parameters, offsets, raster_step)
        glyphs.append(glyph)
    return {
        "text": text, "glyph_width_px": glyph_width, "requested_error_px": error_px,
        "scope": "host analytic oracle; not GPU execution or a welded contour mesh",
        "sites": sum(g["sites"] for g in glyphs), "links": sum(g["links"] for g in glyphs),
        "active_triangles": sum(s["active_triangles"] for s in shapes.values()),
        "submitted_triangles": sum(s["submitted_triangles"] for s in shapes.values()),
        "shapes": shapes, "glyphs": glyphs,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--text", default=DEMO)
    parser.add_argument("--all-basic", action="store_true", help="audit every printable non-space Basic glyph")
    parser.add_argument("--error-px", type=float, default=0.125)
    parser.add_argument("--glyph-width", type=float, default=96.0)
    parser.add_argument("--raster-step", type=float, default=0.0, help="optional source-unit sampling spacing; zero disables")
    args = parser.parse_args()
    text = "".join(chr(i) for i in range(33, 127)) if args.all_basic else args.text
    try:
        result = audit(text, args.error_px, args.glyph_width, args.raster_step)
    except (ValueError, AssertionError) as error:
        parser.exit(1, str(error) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
