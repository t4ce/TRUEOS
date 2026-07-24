#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
runner="$script_dir/runtime/lfm25-fixed"
expected_hi_ai='Hello! How can I help you today?'

for command in strace rg cmp python3; do
    if ! command -v "$command" >/dev/null 2>&1; then
        printf 'lfm25-igpu-packed: required command is missing: %s\n' "$command" >&2
        exit 1
    fi
done

python3 "$script_dir/verify_packed_isa.py"
"$script_dir/build_cpp.sh"

temp_dir=$(mktemp -d /tmp/trueos-lfm25-igpu-packed.XXXXXX)
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

strace \
    -f \
    -qq \
    -yy \
    -e trace=openat,ioctl \
    -o "$temp_dir/neo.trace" \
    "$runner" --parity-igpu-packed-hi \
    >"$temp_dir/parity.out" \
    2>"$temp_dir/parity.stats"

if [[ $(<"$temp_dir/parity.out") != "Hello" ]]; then
    printf 'lfm25-igpu-packed: sealed hi parity output mismatch\n' >&2
    exit 1
fi
if ! rg -q 'PASS igpu-packed-hi .*projection_launches=930 ' \
    "$temp_dir/parity.stats"; then
    printf 'lfm25-igpu-packed: sealed hi projection count or token parity failed\n' >&2
    exit 1
fi
if ! rg -q \
    'igpu_runtime .*weight_layout=pair1088-x16-dp4a model_bytes=376701952 subnormal_scales=25994 .*program_binary_bytes=[1-9][0-9]* .*program_binary_sha256=[0-9a-f]{64}' \
    "$temp_dir/parity.stats"; then
    printf 'lfm25-igpu-packed: packed model or NEO binary admission failed\n' >&2
    exit 1
fi
if ! rg -q 'openat\(.*libigdrcl[^"]*"' "$temp_dir/neo.trace"; then
    printf 'lfm25-igpu-packed: Intel NEO libigdrcl was not loaded\n' >&2
    exit 1
fi
if ! rg -q 'openat\(.*(libigc|libigdfcl)[^"]*"' "$temp_dir/neo.trace"; then
    printf 'lfm25-igpu-packed: Intel IGC compiler libraries were not loaded\n' >&2
    exit 1
fi
if ! rg -q 'DRM_IOCTL_I915_GEM_EXECBUFFER2.*= 0' "$temp_dir/neo.trace"; then
    printf 'lfm25-igpu-packed: no successful i915 submission was observed\n' >&2
    exit 1
fi

"$runner" --igpu-packed "hi ai" --max-tokens 32 \
    >"$temp_dir/igpu.out" \
    2>"$temp_dir/igpu.stats"
"$runner" "hi ai" --max-tokens 32 --threads 1 \
    >"$temp_dir/oracle.out" \
    2>"$temp_dir/oracle.stats"

if [[ $(<"$temp_dir/igpu.out") != "$expected_hi_ai" ]]; then
    printf 'lfm25-igpu-packed: iGPU hi-ai output mismatch\n' >&2
    exit 1
fi
if ! cmp -s "$temp_dir/igpu.out" "$temp_dir/oracle.out"; then
    printf 'lfm25-igpu-packed: iGPU and pinned b10075 oracle outputs differ\n' >&2
    exit 1
fi
if ! rg -q 'projection_launches=1860 ' "$temp_dir/igpu.stats"; then
    printf 'lfm25-igpu-packed: full hi-ai projection count mismatch\n' >&2
    exit 1
fi

cat "$temp_dir/parity.stats"
cat "$temp_dir/igpu.stats"
printf 'lfm25-igpu-packed: PASS output=%q oracle=byte-identical\n' "$expected_hi_ai"
