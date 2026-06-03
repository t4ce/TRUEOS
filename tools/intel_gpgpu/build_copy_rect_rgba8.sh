#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
kernel_src="$repo_root/src/intel/kernels/copy_rect_rgba8.cl"
out_dir="${1:-$repo_root/bld/intel-gpgpu/adls/copy_rect_rgba8}"
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

rm -rf "$out_dir"
mkdir -p "$out_dir"

"$ocloc_bin" compile \
    -file "$kernel_src" \
    -device "$device" \
    -output "$out_dir/copy_rect_rgba8"

printf 'copy_rect_rgba8: source=%s device=%s out=%s\n' "$kernel_src" "$device" "$out_dir"
