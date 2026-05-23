#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
pattern="${1:-mbgrid}"
width="${2:-2560}"
height="${3:-1440}"
visible_height="${4:-${height}}"
base_name="trueos_h264_diag_${pattern}_${width}x${height}"
ppm_path="${repo_root}/tools/vid/${base_name}.ppm"
mp4_path="${repo_root}/tools/vid/${base_name}.mp4"

python3 "${repo_root}/tools/vid/make_trueos_diag_pattern.py" \
  --pattern "${pattern}" \
  --width "${width}" \
  --height "${height}" \
  --visible-height "${visible_height}" \
  --output "${ppm_path}"

ffmpeg -y \
  -loop 1 -framerate 1 -t 1 \
  -i "${ppm_path}" \
  -vf "format=yuv420p" \
  -c:v libx264 \
  -preset veryslow \
  -crf 1 \
  -g 1 -keyint_min 1 -sc_threshold 0 \
  -bf 0 -refs 1 \
  -x264-params "cabac=0:deblock=0,0:weightp=0:8x8dct=0:aq-mode=0:mbtree=0:rc-lookahead=0" \
  -movflags +faststart \
  "${mp4_path}"

printf 'wrote %s\n' "${mp4_path}"
