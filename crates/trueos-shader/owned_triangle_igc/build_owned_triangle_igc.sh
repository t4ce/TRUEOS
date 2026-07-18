#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
OUT_DIR="$ROOT/out"
OCLOC="${OCLOC:-/home/t4ce/.local/share/Trash/files/bld/intel-tools/root/usr/bin/ocloc-26.18.1}"
IGC_ROOT="${IGC_ROOT:-/home/t4ce/.local/share/Trash/files/bld/intel-tools/root}"
LD_LIBRARY_PATH="$IGC_ROOT/usr/local/lib:$IGC_ROOT/usr/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export LD_LIBRARY_PATH

mkdir -p "$OUT_DIR"

"$OCLOC" --version > "$OUT_DIR/ocloc.version.txt"
"$OCLOC" compile \
  -file "$ROOT/triangle_owned.cl" \
  -device 0x4680 \
  -64 \
  -output trueos_owned_triangle \
  -out_dir "$OUT_DIR" \
  -output_no_suffix \
  -gen_file

sha256sum "$ROOT/triangle_owned.cl" "$OUT_DIR"/* > "$OUT_DIR/SHA256SUMS"

cat > "$OUT_DIR/ARTIFACT_META.txt" <<EOF
artifact=trueos-owned-3-vertex-triangle
compiler=ocloc
compiler_path=$OCLOC
device=0x4680
device_name=alder-lake-s-gt1
stage=igc-compute-proof
graphics_stage_upload_safe=0
vertex_count=3
vertex_stride_bytes=12
fragment_color_rgba=1.0,0.2,0.1,1.0
note=IGC-owned source compile proof; not a TRUEOS 3DSTATE_VS/3DSTATE_PS binary contract.
EOF
