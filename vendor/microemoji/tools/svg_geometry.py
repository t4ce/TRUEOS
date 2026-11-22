"""Read standalone Microemoji SVGs for outline comparisons."""

from math import isfinite
from pathlib import Path
import re
from xml.etree import ElementTree

from fill_geometry import Glyph, build_paint, tile_projection


_SVG = "{http://www.w3.org/2000/svg}"
_TOKEN = re.compile(r"[MLQCZ]|-?[0-9]+(?:\.[0-9]+)?")
_FILL = re.compile(r"#[0-9a-fA-F]{6}")
_COORDINATES = {"M": 2, "L": 2, "Q": 4, "C": 6, "Z": 0}


def _commands(data: str):
    tokens = _TOKEN.findall(data)
    if not tokens or "".join(tokens) != "".join(data.split()):
        raise ValueError("expected a Microemoji path with M/L/Q/C/Z commands")

    index = 0
    while index < len(tokens):
        command = tokens[index]
        index += 1
        count = _COORDINATES.get(command)
        if count is None or index + count > len(tokens):
            raise ValueError("invalid Microemoji path command or point count")
        try:
            coordinates = [float(token) for token in tokens[index : index + count]]
        except ValueError as error:
            raise ValueError(f"invalid coordinates for {command}") from error
        index += count
        yield command, list(zip(coordinates[::2], coordinates[1::2]))


def read_svg(path: Path, size: int) -> Glyph:
    """Read a standalone exporter SVG in square tile coordinates, with Y down."""
    if size < 1:
        raise ValueError("tile size must be positive")
    try:
        root = ElementTree.parse(path).getroot()
    except ElementTree.ParseError as error:
        raise ValueError(f"invalid SVG XML: {error}") from error
    if root.tag != _SVG + "svg" or set(root.attrib) != {"viewBox", "width", "height"}:
        raise ValueError("expected a standalone Microemoji SVG")
    try:
        left, top, width, height = map(float, root.attrib["viewBox"].split())
        actual_size = float(root.attrib["width"]), float(root.attrib["height"])
    except ValueError as error:
        raise ValueError("invalid Microemoji SVG viewport") from error
    if (
        not all(map(isfinite, (left, top, width, height, left + width, top + height)))
        or min(width, height) <= 0
        or actual_size != (width, height)
    ):
        raise ValueError("invalid Microemoji SVG viewport")

    if len(root) != 1:
        raise ValueError("expected one Microemoji outline group")
    group = root[0]
    expected_group = {
        "transform": "scale(1 -1)",
        "fill-rule": "nonzero",
        "fill-opacity": "1",
        "stroke": "none",
    }
    if group.tag != _SVG + "g" or group.attrib != expected_group:
        raise ValueError("unsupported transform or paint attributes in Microemoji group")

    project = tile_projection(left, top, width, height, size)

    paints = []
    source_segments = 0
    for child in group:
        color = child.get("fill", "")
        if (
            child.tag != _SVG + "path"
            or len(child) != 0
            or set(child.attrib) - {"d", "fill", "fill-opacity"}
            or "d" not in child.attrib
            or not _FILL.fullmatch(color)
        ):
            raise ValueError("expected a solid Microemoji path")
        try:
            alpha = float(child.get("fill-opacity", "1"))
        except ValueError as error:
            raise ValueError("invalid SVG path opacity") from error
        if not 0 <= alpha <= 1:
            raise ValueError("invalid SVG path opacity")

        rgb = (int(color[1:3], 16), int(color[3:5], 16), int(color[5:7], 16))
        paint, count = build_paint(_commands(child.attrib["d"]), project, rgb, alpha)
        paints.append(paint)
        source_segments += count

    return Glyph(paints, source_segments)
