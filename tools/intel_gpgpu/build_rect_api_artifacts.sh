#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
out_root="${1:-$repo_root/bld/intel-gpgpu/adls}"
device="${TRUEOS_INTEL_GPGPU_DEVICE:-adls}"
local_tool_root="$repo_root/bld/intel-tools/root"
local_ocloc="$(find "$local_tool_root/usr/bin" -maxdepth 1 -type f -name 'ocloc*' 2>/dev/null | sort | tail -n 1 || true)"

if [[ -n "$local_ocloc" ]]; then
    ocloc_bin="$local_ocloc"
    export LD_LIBRARY_PATH="$local_tool_root/usr/lib/x86_64-linux-gnu:$local_tool_root/usr/local/lib:${LD_LIBRARY_PATH:-}"
elif command -v ocloc >/dev/null 2>&1; then
    ocloc_bin="$(command -v ocloc)"
else
    cat >&2 <<'EOF'
error: ocloc not found

Install Intel's OpenCL Offline Compiler package, usually named intel-ocloc.
Alternatively extract intel-ocloc plus intel-igc-core/opencl packages under
bld/intel-tools/root; this script will use that repo-local toolchain.
The Intel oneAPI AOT docs list `adls` as Alder Lake S / Gen12.2.
EOF
    exit 127
fi

kernels=(
    fill_rect_rgba8
    fill_rect_worklist_rgba8
    fill_circle_rgba8
    blit_rgba8_nearest
    alpha_blend_rgba8_over
    alpha_blend_worklist_rgba8
    glyph_mask_rgba8
    present_rgba8_to_primary_xrgb_rect
    stamp_mandel_rgba8
    sprite64_worklist_rgba8
    canvas3d_project_rgba8
    canvas3d_transform_q16
    canvas3d_clip_box_q16
)

for kernel in "${kernels[@]}"; do
    kernel_src="$repo_root/src/intel/kernels/$kernel.cl"
    out_dir="$out_root/$kernel"
    rm -rf "$out_dir"
    mkdir -p "$out_dir"
    "$ocloc_bin" compile \
        -file "$kernel_src" \
        -device "$device" \
        -output "$out_dir/$kernel"
    printf '%s: source=%s device=%s out=%s\n' "$kernel" "$kernel_src" "$device" "$out_dir"
done
