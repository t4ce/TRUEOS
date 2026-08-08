#!/usr/bin/env python3
import struct
import sys
import zlib
from pathlib import Path


def fail(message: str) -> None:
    raise SystemExit(f"sprite-dig atlas validation failed: {message}")


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit(f"usage: {sys.argv[0]} sprite-dig-atlas.trueos.rgba")
    path = Path(sys.argv[1])
    data = path.read_bytes()
    if len(data) < 64 or data[:8] != b"HDIGATL\0":
        fail("bad header")
    version, header_bytes, total_bytes = struct.unpack_from("<HHI", data, 8)
    width, height, pitch = struct.unpack_from("<HHI", data, 16)
    count, entry_bytes = struct.unpack_from("<HH", data, 24)
    entries_offset, pixels_offset, pixels_bytes, pixels_crc = struct.unpack_from("<IIII", data, 28)
    player_width, player_height = struct.unpack_from("<HH", data, 44)
    if (version, header_bytes, total_bytes) != (1, 64, len(data)):
        fail("unsupported version or length")
    if width != 512 or height == 0 or height % 16 or pitch != width * 4:
        fail("invalid atlas extent")
    if count != 38 or entry_bytes != 16 or entries_offset != 64:
        fail("invalid entry table")
    if pixels_offset % 64 or pixels_bytes != pitch * height or pixels_offset + pixels_bytes != len(data):
        fail("invalid pixel payload")
    if (zlib.crc32(data[pixels_offset:]) & 0xFFFFFFFF) != pixels_crc:
        fail("pixel CRC mismatch")
    if player_width == 0 or player_height == 0:
        fail("missing normalized player dimensions")
    if any(data[48:64]):
        fail("nonzero header reserved bytes")
    for index in range(count):
        offset = entries_offset + index * entry_bytes
        sprite_id, x, y, sprite_width, sprite_height, flags = struct.unpack_from("<6H", data, offset)
        reserved = struct.unpack_from("<I", data, offset + 12)[0]
        if sprite_id != index or sprite_width == 0 or sprite_height == 0:
            fail(f"invalid entry {index}")
        if x + sprite_width > width or y + sprite_height > height:
            fail(f"out-of-bounds entry {index}")
        if flags or reserved:
            fail(f"nonzero entry reserved fields {index}")
    print(
        f"validated {path} ({len(data)} bytes, {width}x{height}, {count} sprites, "
        f"player={player_width}x{player_height})"
    )


if __name__ == "__main__":
    main()
