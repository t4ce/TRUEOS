#!/usr/bin/env python3

from __future__ import annotations

import argparse
from pathlib import Path


WIDTH = 2560
CODED_HEIGHT = 1440
VISIBLE_HEIGHT = 1440
MB_SIZE = 16
COLS = WIDTH // MB_SIZE
ROWS = CODED_HEIGHT // MB_SIZE

PALETTE = [
    (215, 48, 39),
    (244, 109, 67),
    (253, 174, 97),
    (254, 224, 139),
    (230, 245, 152),
    (171, 221, 164),
    (102, 194, 165),
    (50, 136, 189),
    (94, 79, 162),
    (158, 1, 66),
    (146, 197, 222),
    (247, 247, 247),
]

FONT = {
    " ": ["000", "000", "000", "000", "000"],
    "-": ["000", "000", "111", "000", "000"],
    "/": ["001", "001", "010", "100", "100"],
    "0": ["111", "101", "101", "101", "111"],
    "1": ["010", "110", "010", "010", "111"],
    "2": ["111", "001", "111", "100", "111"],
    "3": ["111", "001", "111", "001", "111"],
    "4": ["101", "101", "111", "001", "001"],
    "5": ["111", "100", "111", "001", "111"],
    "6": ["111", "100", "111", "101", "111"],
    "7": ["111", "001", "001", "001", "001"],
    "8": ["111", "101", "111", "101", "111"],
    "9": ["111", "101", "111", "001", "111"],
    "A": ["111", "101", "111", "101", "101"],
    "B": ["110", "101", "110", "101", "110"],
    "C": ["111", "100", "100", "100", "111"],
    "D": ["110", "101", "101", "101", "110"],
    "E": ["111", "100", "110", "100", "111"],
    "F": ["111", "100", "110", "100", "100"],
    "G": ["111", "100", "101", "101", "111"],
    "H": ["101", "101", "111", "101", "101"],
    "I": ["111", "010", "010", "010", "111"],
    "J": ["001", "001", "001", "101", "111"],
    "K": ["101", "101", "110", "101", "101"],
    "L": ["100", "100", "100", "100", "111"],
    "M": ["101", "111", "111", "101", "101"],
    "N": ["101", "111", "111", "111", "101"],
    "O": ["111", "101", "101", "101", "111"],
    "P": ["111", "101", "111", "100", "100"],
    "Q": ["111", "101", "101", "111", "001"],
    "R": ["111", "101", "111", "110", "101"],
    "S": ["111", "100", "111", "001", "111"],
    "T": ["111", "010", "010", "010", "010"],
    "U": ["101", "101", "101", "101", "111"],
    "V": ["101", "101", "101", "101", "010"],
    "W": ["101", "101", "111", "111", "101"],
    "X": ["101", "101", "010", "101", "101"],
    "Y": ["101", "101", "010", "010", "010"],
    "Z": ["111", "001", "010", "100", "111"],
}


def write_text(image: bytearray, text: str, x: int, y: int, color: tuple[int, int, int], scale: int = 2) -> None:
    cursor_x = x
    for ch in text.upper():
        glyph = FONT.get(ch, FONT[" "])
        for row_idx, row in enumerate(glyph):
            for col_idx, bit in enumerate(row):
                if bit != "1":
                    continue
                for dy in range(scale):
                    py = y + row_idx * scale + dy
                    if py < 0 or py >= CODED_HEIGHT:
                        continue
                    for dx in range(scale):
                        px = cursor_x + col_idx * scale + dx
                        if px < 0 or px >= WIDTH:
                            continue
                        off = (py * WIDTH + px) * 3
                        image[off : off + 3] = bytes(color)
        cursor_x += (len(glyph[0]) + 1) * scale


def block_color(row: int, col: int, mode: str) -> tuple[int, int, int]:
    if mode == "luma":
        shade = 32 + ((row * COLS + col) * 9) % 192
        return (shade, shade, shade)
    if mode == "uv":
        y = 128
        u = int(255 * col / max(COLS - 1, 1))
        v = int(255 * row / max(ROWS - 1, 1))
        return yuv_to_rgb(y, u, v)
    palette = PALETTE[(row + col) % len(PALETTE)]
    boost = ((row * 17 + col * 29) % 36) - 18
    return tuple(max(0, min(255, channel + boost)) for channel in palette)


def yuv_to_rgb(y: int, u: int, v: int) -> tuple[int, int, int]:
    c = max(y - 16, 0)
    d = u - 128
    e = v - 128
    r = (298 * c + 409 * e + 128) >> 8
    g = (298 * c - 100 * d - 208 * e + 128) >> 8
    b = (298 * c + 516 * d + 128) >> 8
    return tuple(max(0, min(255, value)) for value in (r, g, b))


def draw_rect(image: bytearray, x: int, y: int, w: int, h: int, color: tuple[int, int, int]) -> None:
    x0 = max(x, 0)
    y0 = max(y, 0)
    x1 = min(x + w, WIDTH)
    y1 = min(y + h, CODED_HEIGHT)
    if x0 >= x1 or y0 >= y1:
        return
    fill = bytes(color)
    for py in range(y0, y1):
        row_off = (py * WIDTH + x0) * 3
        for px in range(x0, x1):
            off = row_off + (px - x0) * 3
            image[off : off + 3] = fill


def draw_hline(image: bytearray, y: int, color: tuple[int, int, int]) -> None:
    if 0 <= y < CODED_HEIGHT:
        draw_rect(image, 0, y, WIDTH, 1, color)


def draw_vline(image: bytearray, x: int, color: tuple[int, int, int]) -> None:
    if 0 <= x < WIDTH:
        draw_rect(image, x, 0, 1, CODED_HEIGHT, color)


def build_pattern(mode: str) -> bytearray:
    image = bytearray(WIDTH * CODED_HEIGHT * 3)
    for row in range(ROWS):
        for col in range(COLS):
            color = block_color(row, col, mode)
            draw_rect(image, col * MB_SIZE, row * MB_SIZE, MB_SIZE, MB_SIZE, color)

    grid = (18, 18, 18)
    heavy = (240, 240, 240)
    label = (245, 245, 245)
    shadow = (0, 0, 0)
    crop = (255, 40, 40)

    for col in range(COLS + 1):
        draw_vline(image, col * MB_SIZE, heavy if col % 4 == 0 else grid)
    for row in range(ROWS + 1):
        draw_hline(image, row * MB_SIZE, heavy if row % 4 == 0 else grid)

    for row in range(ROWS):
        for col in range(COLS):
            label_text = f"R{row}C{col}"
            x = col * MB_SIZE + 2
            y = row * MB_SIZE + 2
            write_text(image, label_text, x + 1, y + 1, shadow, scale=1)
            write_text(image, label_text, x, y, label, scale=1)

    if VISIBLE_HEIGHT < CODED_HEIGHT:
        for y in range(VISIBLE_HEIGHT, CODED_HEIGHT):
            stripe = crop if ((y - VISIBLE_HEIGHT) // 2) % 2 == 0 else (255, 235, 40)
            draw_rect(image, 0, y, WIDTH, 1, stripe)
        draw_hline(image, VISIBLE_HEIGHT - 1, (255, 255, 255))
        visible_end = f"VISIBLE {WIDTH}X{VISIBLE_HEIGHT} ENDS HERE"
        write_text(image, visible_end, 16, VISIBLE_HEIGHT - 20, shadow, scale=2)
        write_text(image, visible_end, 14, VISIBLE_HEIGHT - 22, (255, 255, 255), scale=2)
        crop_label = f"CROP BAND {CODED_HEIGHT - VISIBLE_HEIGHT}PX"
        write_text(image, crop_label, 16, VISIBLE_HEIGHT + 8, shadow, scale=2)
        write_text(image, crop_label, 14, VISIBLE_HEIGHT + 6, crop, scale=2)
    else:
        draw_hline(image, CODED_HEIGHT - 1, (255, 255, 255))
        write_text(image, f"NATIVE {WIDTH}X{CODED_HEIGHT} NO CROP", 16, CODED_HEIGHT - 30, shadow, scale=2)
        write_text(image, f"NATIVE {WIDTH}X{CODED_HEIGHT} NO CROP", 14, CODED_HEIGHT - 32, (255, 255, 255), scale=2)

    title = {
        "mbgrid": "TRUEOS H264 MACROBLOCK GRID",
        "luma": "TRUEOS H264 LUMA-ONLY GRID",
        "uv": "TRUEOS H264 UV LAYOUT GRID",
    }[mode]
    write_text(image, title, 18, 18, shadow, scale=3)
    write_text(image, title, 14, 14, (255, 255, 255), scale=3)
    dims = f"CODED {WIDTH}X{CODED_HEIGHT} / VISIBLE {WIDTH}X{VISIBLE_HEIGHT} / 16X16 MB LABELS"
    write_text(image, dims, 16, 54, shadow, scale=2)
    write_text(image, dims, 14, 52, (255, 255, 255), scale=2)
    center_left = (COLS // 2) - 1
    center_right = COLS // 2
    bottom_row = ROWS - 1
    bottom_hint = f"BOTTOM CENTER SHOULD BE R{bottom_row}C{center_left} / R{bottom_row}C{center_right}"
    write_text(image, bottom_hint, 16, 78, shadow, scale=2)
    write_text(image, bottom_hint, 14, 76, (255, 255, 255), scale=2)
    return image


def write_ppm(path: Path, image: bytearray) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("wb") as f:
        header = f"P6\n{WIDTH} {CODED_HEIGHT}\n255\n".encode("ascii")
        f.write(header)
        f.write(image)


def main() -> int:
    global WIDTH, CODED_HEIGHT, VISIBLE_HEIGHT, COLS, ROWS
    parser = argparse.ArgumentParser(description="Generate TRUEOS H.264 first-frame diagnostic patterns.")
    parser.add_argument(
        "--pattern",
        choices=("mbgrid", "luma", "uv"),
        default="mbgrid",
        help="Pattern variant to emit.",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("tools/vid/trueos_h264_diag_mbgrid_2560x1440.ppm"),
        help="Output PPM path.",
    )
    parser.add_argument("--width", type=int, default=2560, help="Coded frame width.")
    parser.add_argument("--height", type=int, default=1440, help="Coded frame height.")
    parser.add_argument(
        "--visible-height",
        type=int,
        default=None,
        help="Visible frame height; defaults to the coded height.",
    )
    args = parser.parse_args()
    if args.width <= 0 or args.height <= 0:
        parser.error("width and height must be positive")
    if args.width % MB_SIZE != 0 or args.height % MB_SIZE != 0:
        parser.error("width and height must be multiples of 16")
    visible_height = args.height if args.visible_height is None else args.visible_height
    if visible_height <= 0 or visible_height > args.height:
        parser.error("visible height must be in 1..height")
    WIDTH = args.width
    CODED_HEIGHT = args.height
    VISIBLE_HEIGHT = visible_height
    COLS = WIDTH // MB_SIZE
    ROWS = CODED_HEIGHT // MB_SIZE
    image = build_pattern(args.pattern)
    write_ppm(args.output, image)
    print(f"wrote {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
