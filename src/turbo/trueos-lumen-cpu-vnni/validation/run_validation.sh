#!/usr/bin/env bash
set -euo pipefail
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
python3 "$here/check_contract.py"
python3 "$here/check_bundle.py"
python3 "$here/check_transformer.py"

compiler=${CC:-clang}
assembly=$(mktemp)
runtime_probe=$(mktemp)
trap 'rm -f "$assembly" "$runtime_probe"' EXIT

"$compiler" -O3 -mavx2 -mavxvnni -mfma -S -masm=intel \
  "$here/vnni_codegen_probe.c" -o "$assembly"
for instruction in vpsignb vpdpbusd vcvtdq2ps vfmadd; do
  grep -qi "$instruction" "$assembly" || {
    echo "missing expected instruction: $instruction" >&2
    exit 1
  }
done
echo "AVX-VNNI code-generation probe: PASS"

"$compiler" -O3 "$here/vnni_runtime_probe.c" -o "$runtime_probe"
"$runtime_probe"
