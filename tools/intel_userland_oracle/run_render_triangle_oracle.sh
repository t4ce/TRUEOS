#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
root=$(cd -- "$script_dir/../.." && pwd)

out_dir=${TRUEOS_RENDER_ORACLE_OUT_DIR:-"$root/.codex_tmp/intel_userland_oracle/render-simple-triangle"}
build_dir="$out_dir/build"
exec_dir="$out_dir/pipeline_exec"
trace_so="$build_dir/libtrueos_ioctl_trace.so"
dumper="$build_dir/simple_triangle_dump"

trace_src="$root/tools/intel_userland_oracle/ioctl_trace.c"
dumper_src="$root/crates/trueos-shader/xe_lp_shader_bake/simple_triangle_dump.c"
existing_dumper="$root/crates/trueos-shader/host_shader_validation/simple_triangle_dump"
vs_src="$root/crates/trueos-shader/xe_lp_shader_bake/simple_triangle.vert"
fs_src="$root/crates/trueos-shader/xe_lp_shader_bake/simple_triangle.frag"
vs_spv=${TRUEOS_RENDER_ORACLE_VS_SPV:-"$root/crates/trueos-shader/host_shader_validation/simple_triangle.vert.spv"}
fs_spv=${TRUEOS_RENDER_ORACLE_FS_SPV:-"$root/crates/trueos-shader/host_shader_validation/simple_triangle.frag.spv"}

mkdir -p "$build_dir" "$exec_dir" "$out_dir/dumps"
rm -f "$out_dir/log.txt" "$out_dir/replay_manifest.json" "$out_dir/summary.txt"

if [[ ! -f "$vs_spv" || ! -f "$fs_spv" ]]; then
    vs_spv="$build_dir/simple_triangle.vert.spv"
    fs_spv="$build_dir/simple_triangle.frag.spv"
    if command -v glslangValidator >/dev/null 2>&1; then
        glslangValidator -V -S vert -o "$vs_spv" "$vs_src"
        glslangValidator -V -S frag -o "$fs_spv" "$fs_src"
    elif command -v glslc >/dev/null 2>&1; then
        glslc -o "$vs_spv" "$vs_src"
        glslc -o "$fs_spv" "$fs_src"
    else
        echo "missing SPIR-V inputs and no glslangValidator/glslc in PATH" >&2
        exit 1
    fi
fi

cc -shared -fPIC -O2 -Wall -Wextra "$trace_src" -o "$trace_so" -ldl

if pkg-config --exists vulkan 2>/dev/null; then
    # shellcheck disable=SC2046
    cc "$dumper_src" -O2 -Wall -Wextra -o "$dumper" $(pkg-config --cflags --libs vulkan)
elif [[ -f /usr/include/vulkan/vulkan.h || -f /usr/local/include/vulkan/vulkan.h ]] && \
    cc "$dumper_src" -O2 -Wall -Wextra -o "$dumper" -lvulkan; then
    true
elif [[ -x "$existing_dumper" ]]; then
    cp "$existing_dumper" "$dumper"
else
    echo "failed to build simple_triangle_dump and no existing host binary is available" >&2
    echo "install Vulkan headers or run crates/trueos-shader/xe_lp_shader_bake/run_host_validation.py once" >&2
    exit 1
fi

env \
    LD_PRELOAD="$trace_so" \
    TRUEOS_ORACLE_LOG_DIR="$out_dir" \
    TRUEOS_ORACLE_MAX_DUMP_BYTES="${TRUEOS_ORACLE_MAX_DUMP_BYTES:-0x200000}" \
    TRUEOS_ORACLE_DUMP_EVERY_EXEC="${TRUEOS_ORACLE_DUMP_EVERY_EXEC:-1}" \
    TRUEOS_ORACLE_TRACE_STACKS="${TRUEOS_ORACLE_TRACE_STACKS:-0}" \
    TRUEOS_ORACLE_TRACE_SNAPSHOTS="${TRUEOS_ORACLE_TRACE_SNAPSHOTS:-0}" \
    TRUEOS_EXECUTABLE_DUMP_DIR="$exec_dir" \
    TRUEOS_VK_DEVICE_ID="${TRUEOS_VK_DEVICE_ID:-0xA780}" \
    "$dumper" "$vs_spv" "$fs_spv" | tee "$out_dir/simple_triangle_dump.log"

python3 "$root/tools/intel_userland_oracle/extract_replay_manifest.py" \
    "$out_dir/log.txt" \
    --hash \
    > "$out_dir/replay_manifest.json"

{
    echo "render-triangle-oracle out_dir=$out_dir"
    rg -n "simple_triangle_dump: (selected|verified=|center=|pipeline_cache_size|executable_count)" \
        "$out_dir/simple_triangle_dump.log" || true
    rg -n "trace-start|execbuffer-pre buffers_ptr|execbuffer-pre object\\[|bo-dump phase=pre_exec|ioctl-exit .*DRM_IOCTL_I915_GEM_EXECBUFFER2" \
        "$out_dir/log.txt" || true
    python3 - "$out_dir/replay_manifest.json" <<'PY'
import json
import sys
from pathlib import Path

manifest = json.loads(Path(sys.argv[1]).read_text())
print(f"manifest submit_count={manifest.get('submit_count')}")
for submit in manifest.get("submits", []):
    print(
        "submit "
        f"seq={submit.get('seq')} "
        f"buffers={submit.get('buffer_count')} "
        f"batch_start=0x{int(submit.get('batch_start') or 0):X} "
        f"flags=0x{int(submit.get('flags') or 0):X} "
        f"dumped={submit.get('dumped_object_count')} "
        f"missing={submit.get('missing_dump_count')} "
        f"ret={submit.get('ret')}"
    )
PY
} | tee "$out_dir/summary.txt"

echo "render oracle complete: $out_dir"
