#!/usr/bin/env python3
"""Compare two raw Spirit background captures without scaling or sprite blending."""

import argparse
from pathlib import Path

from PIL import Image, ImageChops, ImageColor, ImageDraw

SIZE = 256
FIRST_ID = 2
CELL_COUNT = 12
COLUMNS = 4
ROWS = 3
CELL_BYTES = SIZE * SIZE * 4
PIXELS = CELL_COUNT * SIZE * SIZE


def read_cells(path):
    data = path.read_bytes()
    expected = CELL_COUNT * CELL_BYTES
    if len(data) != expected:
        raise ValueError(f"{path}: expected {expected} BGRA bytes, got {len(data)}")
    return [
        Image.frombytes("RGBA", (SIZE, SIZE), data[offset:offset + CELL_BYTES], "raw", "BGRA")
        for offset in range(0, expected, CELL_BYTES)
    ]


def opaque_rgb(cell, backdrop):
    red, green, blue, alpha = cell.split()
    # GPU color is already premultiplied. Add the backdrop under it directly.
    channels = [
        ImageChops.add(channel, alpha.point([(color * (255 - a) + 127) // 255 for a in range(256)]))
        for channel, color in zip((red, green, blue), backdrop, strict=True)
    ]
    return Image.merge("RGB", channels)


def diff_stats(left, right):
    bands = ImageChops.difference(left, right).split()
    any_delta = bands[0]
    for band in bands[1:]:
        any_delta = ImageChops.lighter(any_delta, band)
    return SIZE * SIZE - any_delta.histogram()[0], [band.histogram() for band in bands]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("preview", type=Path, help="Host OpenCL .bgra capture")
    parser.add_argument("renderer", type=Path, help="TRUEOS renderer .bgra capture")
    parser.add_argument("--output", type=Path, default=Path("bld/spirit-vfx-comparison.png"))
    parser.add_argument("--backdrop", default="#20242c", help="Common RGB backdrop color")
    args = parser.parse_args()
    try:
        backdrop = ImageColor.getcolor(args.backdrop, "RGB")
        preview, renderer = read_cells(args.preview), read_cells(args.renderer)
    except (OSError, ValueError) as error:
        parser.error(str(error))

    heading_height, gap = 27, 12
    grid_width = COLUMNS * SIZE
    image = Image.new("RGB", (2 * grid_width + gap, ROWS * SIZE + heading_height), backdrop)
    draw = ImageDraw.Draw(image)
    draw.rectangle((0, 0, image.width, heading_height - 1), fill="#15181e")
    draw.text((8, 7), "OpenCL preview", fill="white")
    draw.text((grid_width + gap + 8, 7), "TRUEOS renderer", fill="white")

    changed = 0
    total_histograms = [[0] * 256 for _ in range(4)]
    for index, (left, right) in enumerate(zip(preview, renderer, strict=True)):
        x = (index % COLUMNS) * SIZE
        y = heading_height + (index // COLUMNS) * SIZE
        image.paste(opaque_rgb(left, backdrop), (x, y))
        image.paste(opaque_rgb(right, backdrop), (x + grid_width + gap, y))
        count, histograms = diff_stats(left, right)
        changed += count
        for total, band in zip(total_histograms, histograms, strict=True):
            for delta, frequency in enumerate(band):
                total[delta] += frequency
        print(f"ID {FIRST_ID + index:2d}  changed {count:5d}/{SIZE * SIZE} pixels"
              f"  alpha changed {SIZE * SIZE - histograms[3][0]:5d}")

    print(f"Changed pixels {changed}/{PIXELS} ({changed / PIXELS:.2%})")
    for channel, hist in zip("RGBA", total_histograms, strict=True):
        maximum = max(index for index, count in enumerate(hist) if count)
        mean = sum(index * count for index, count in enumerate(hist)) / PIXELS
        print(f"{channel}  changed {PIXELS - hist[0]:6d}  max delta {maximum:3d}  mean delta {mean:.4f}")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    image.save(args.output)
    print(f"Comparison image {args.output}")


if __name__ == "__main__":
    main()
