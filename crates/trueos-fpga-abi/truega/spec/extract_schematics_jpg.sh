#!/usr/bin/env bash
set -euo pipefail

PDF="/home/t4ce/Repos/TRUEGA/TANG_MEGA-138K_Dock-4071f3__Schematics.pdf"
OUT_DIR="/home/t4ce/Repos/TRUEGA/extracted"

mkdir -p "$OUT_DIR"
rm -f "$OUT_DIR"/page_*.jpg "$OUT_DIR"/page-*.jpg

# High-res JPEG render
pdftoppm -jpeg -r 360 -jpegopt quality=96 "$PDF" "$OUT_DIR/page"

# Normalize names to page_01.jpg ... page_19.jpg
i=1
for f in "$OUT_DIR"/page-*.jpg; do
  printf -v n "%02d" "$i"
  mv "$f" "$OUT_DIR/page_${n}.jpg"
  i=$((i+1))
done

echo "Done: rendered $((i-1)) pages into $OUT_DIR"
