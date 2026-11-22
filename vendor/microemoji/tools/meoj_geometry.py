"""Read color MEOJ v1 outlines for offline geometry reduction."""

from dataclasses import dataclass
from math import ceil, floor
from pathlib import Path
from struct import Struct

from fill_geometry import Glyph, Paint, build_paint, tile_projection


_HEADER = Struct("<4sHHhhI")
_INDEX_ENTRY = Struct("<IIIHH")
_LAYER_HEADER = Struct("<BBBBI")
_OPCODES = (
    ("M", Struct("<ii")),
    ("L", Struct("<ii")),
    ("Q", Struct("<iiii")),
    ("C", Struct("<iiiiii")),
    ("Z", Struct("<")),
)
_FONT_UNIT = 64


def _read_path(data: memoryview) -> list[tuple[str, list[tuple[float, float]]]]:
    if not data:
        raise ValueError("empty MEOJ path")
    commands = []
    offset = 0
    while offset < len(data):
        opcode = data[offset]
        if opcode >= len(_OPCODES):
            raise ValueError("invalid MEOJ path opcode")
        command, point_data = _OPCODES[opcode]
        offset += 1
        if offset + point_data.size > len(data):
            raise ValueError("truncated MEOJ path points")
        coords = point_data.unpack_from(data, offset)
        points = [
            (x / _FONT_UNIT, y / _FONT_UNIT)
            for x, y in zip(coords[::2], coords[1::2])
        ]
        commands.append((command, points))
        offset += point_data.size
    return commands


@dataclass(frozen=True)
class GlyphSource:
    """A glyph's ordered path layers, decoded only when projected to a tile."""

    payload: memoryview
    layer_count: int
    advance: int
    ascender: int
    descender: int

    def project(self, size: int) -> Glyph:
        layers = []
        offset = 0
        # Match Bounds::for_glyph, including overhangs from control points.
        left, top, right, bottom = 0, -self.ascender, self.advance, -self.descender
        for _ in range(self.layer_count):
            if offset + _LAYER_HEADER.size > len(self.payload):
                raise ValueError("truncated MEOJ layer header")
            red, green, blue, alpha, length = _LAYER_HEADER.unpack_from(self.payload, offset)
            offset += _LAYER_HEADER.size
            end = offset + length
            if end > len(self.payload):
                raise ValueError("truncated MEOJ layer path")
            commands = _read_path(self.payload[offset:end])
            offset = end
            for _, points in commands:
                for x, y in points:
                    left, right = min(left, floor(x)), max(right, ceil(x))
                    top, bottom = min(top, floor(-y)), max(bottom, ceil(-y))
            layers.append((commands, (red, green, blue), alpha / 255))
        if offset != len(self.payload):
            raise ValueError("trailing bytes in MEOJ glyph payload")

        project = tile_projection(left, top, right - left, bottom - top, size)
        paints: list[Paint] = []
        source_segments = 0
        for commands, rgb, alpha in layers:
            paint, count = build_paint(commands, project, rgb, alpha)
            paints.append(paint)
            source_segments += count
        return Glyph(paints, source_segments)


def read_bundle(path: Path) -> list[tuple[int, GlyphSource]]:
    """Read the bundle index in scalar order and borrow each glyph's payload."""
    data = memoryview(path.read_bytes())
    if len(data) < _HEADER.size:
        raise ValueError("truncated MEOJ header")
    magic, version, units_per_em, ascender, descender, count = _HEADER.unpack_from(data)
    if magic != b"MEOJ":
        raise ValueError("invalid MEOJ magic")
    if version != 1:
        raise ValueError(f"unsupported MEOJ version {version}")
    if not units_per_em or ascender <= descender:
        raise ValueError("invalid MEOJ metrics")

    next_offset = _HEADER.size + count * _INDEX_ENTRY.size
    if next_offset > len(data):
        raise ValueError("truncated MEOJ index")
    glyphs = []
    previous = -1
    for index in range(count):
        entry = _HEADER.size + index * _INDEX_ENTRY.size
        scalar, offset, length, advance, layers = _INDEX_ENTRY.unpack_from(data, entry)
        if scalar <= previous or scalar > 0x10FFFF or 0xD800 <= scalar <= 0xDFFF:
            raise ValueError("invalid or unordered Unicode scalar in MEOJ index")
        previous = scalar
        end = offset + length
        if offset != next_offset or end > len(data) or not layers:
            raise ValueError(f"invalid MEOJ payload for U+{scalar:04X}")
        glyph = GlyphSource(data[offset:end], layers, advance or units_per_em, ascender, descender)
        glyphs.append((scalar, glyph))
        next_offset = end
    if next_offset != len(data):
        raise ValueError("trailing bytes after MEOJ glyphs")
    return glyphs
