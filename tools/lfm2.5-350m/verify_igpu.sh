#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
runner="$script_dir/runtime/lfm25-fixed"
expected_hi_ai='Hello! How can I help you today?'

for command in strace rg cmp; do
    if ! command -v "$command" >/dev/null 2>&1; then
        printf 'lfm25-igpu: required command is missing: %s\n' "$command" >&2
        exit 1
    fi
done

temp_dir=$(mktemp -d /tmp/trueos-lfm25-igpu.XXXXXX)
cleanup() {
    rm -f -- \
        "$temp_dir/neo.trace" \
        "$temp_dir/parity.out" \
        "$temp_dir/parity.stats" \
        "$temp_dir/igpu.out" \
        "$temp_dir/igpu.stats" \
        "$temp_dir/oracle.out" \
        "$temp_dir/oracle.stats"
    rmdir -- "$temp_dir"
}
trap cleanup EXIT

"$script_dir/build_cpp.sh"

strace \
    -f \
    -qq \
    -yy \
    -e trace=openat,ioctl \
    -o "$temp_dir/neo.trace" \
    "$runner" --parity-igpu-hi \
    >"$temp_dir/parity.out" \
    2>"$temp_dir/parity.stats"

if [[ $(<"$temp_dir/parity.out") != "Hello" ]]; then
    printf 'lfm25-igpu: sealed hi parity output mismatch\n' >&2
    exit 1
fi
if ! rg -q 'PASS igpu-hi .*projection_launches=930 ' "$temp_dir/parity.stats"; then
    printf 'lfm25-igpu: sealed hi projection count or token parity failed\n' >&2
    exit 1
fi
if ! rg -q 'igpu_runtime .*program_binary_bytes=[1-9][0-9]* .*program_binary_sha256=[0-9a-f]{64}' \
    "$temp_dir/parity.stats"; then
    printf 'lfm25-igpu: NEO did not expose an executable device binary\n' >&2
    exit 1
fi
if ! rg -q 'openat\(.*libigdrcl[^"]*"' "$temp_dir/neo.trace"; then
    printf 'lfm25-igpu: Intel NEO libigdrcl was not loaded\n' >&2
    exit 1
fi
if ! rg -q 'openat\(.*(libigc|libigdfcl)[^"]*"' "$temp_dir/neo.trace"; then
    printf 'lfm25-igpu: Intel IGC compiler libraries were not loaded\n' >&2
    exit 1
fi
if ! rg -q 'DRM_IOCTL_I915_GEM_EXECBUFFER2.*= 0' "$temp_dir/neo.trace"; then
    printf 'lfm25-igpu: no successful i915 execution submission was observed\n' >&2
    exit 1
fi

"$runner" --igpu "hi ai" --max-tokens 32 \
    >"$temp_dir/igpu.out" \
    2>"$temp_dir/igpu.stats"
"$runner" "hi ai" --max-tokens 32 --threads 1 \
    >"$temp_dir/oracle.out" \
    2>"$temp_dir/oracle.stats"

if [[ $(<"$temp_dir/igpu.out") != "$expected_hi_ai" ]]; then
    printf 'lfm25-igpu: iGPU hi-ai output mismatch\n' >&2
    exit 1
fi
if ! cmp -s "$temp_dir/igpu.out" "$temp_dir/oracle.out"; then
    printf 'lfm25-igpu: iGPU and pinned b10075 oracle outputs differ\n' >&2
    exit 1
fi
if ! rg -q 'projection_launches=1860 ' "$temp_dir/igpu.stats"; then
    printf 'lfm25-igpu: full hi-ai projection count mismatch\n' >&2
    exit 1
fi

execbuffer_count=$(
    rg -c 'DRM_IOCTL_I915_GEM_EXECBUFFER2' "$temp_dir/neo.trace" |
        awk '{ total += $1 } END { print total + 0 }'
)
neo_path=$(
    rg -m1 -o '/[^"]*/libigdrcl\.so' "$temp_dir/neo.trace" |
        sed -n '1p'
)
igc_path=$(
    rg -m1 -o '/[^"]*/libigc\.so\.[0-9]+' "$temp_dir/neo.trace" |
        sed -n '1p'
)
render_path=$(
    rg -m1 'DRM_IOCTL_I915_GEM_EXECBUFFER2.*= 0' "$temp_dir/neo.trace" |
        rg -o '/dev/dri/renderD[0-9]+' |
        sed -n '1p'
)

cat "$temp_dir/parity.stats"
cat "$temp_dir/igpu.stats"
printf 'lfm25-igpu: PASS output=%q oracle=byte-identical\n' "$expected_hi_ai"
printf 'lfm25-igpu: PASS neo=%s igc=%s drm=%s i915_execbuffer_records=%s\n' \
    "$neo_path" "$igc_path" "$render_path" "$execbuffer_count"
