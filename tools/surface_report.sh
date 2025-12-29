#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SURFACE_MOD="$ROOT/src/surface/mod.rs"

if (($# != 0)); then
  echo "error: this script takes no arguments" >&2
  echo "usage: tools/surface_report.sh" >&2
  exit 2
fi

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command not found: $1" >&2
    exit 1
  fi
}

need_cmd rg
need_cmd rustc
need_cmd sort
need_cmd comm
need_cmd wc
need_cmd sed

sysroot="$(rustc --print sysroot 2>/dev/null || true)"
if [[ -z "$sysroot" ]]; then
  echo "error: unable to locate rustc sysroot" >&2
  exit 1
fi

stdlib="$sysroot/lib/rustlib/src/rust/library/std/src/lib.rs"
core_src="$sysroot/lib/rustlib/src/rust/library/core/src"
alloc_src="$sysroot/lib/rustlib/src/rust/library/alloc/src"

if [[ ! -f "$stdlib" ]]; then
  echo "error: rust-src not installed for this toolchain" >&2
  echo "hint: rustup component add rust-src" >&2
  exit 1
fi

print_header() {
  echo "============================================================"
  echo "$1"
  echo "============================================================"
}

module_exists_in_src() {
  local base="$1" name="$2"
  [[ -f "$base/$name.rs" || -f "$base/$name/mod.rs" || -d "$base/$name" ]]
}

percent() {
  local n="$1" d="$2"
  if ((d == 0)); then
    echo "0"
  else
    echo $((n * 100 / d))
  fi
}

count_lines() {
  sed '/^$/d' | wc -l
}

# Top-level names provided by surface.
readarray -t SURFACE_NAMES < <(
  {
    rg -o -N "^\s*pub\s+mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*(;|\{)" "$SURFACE_MOD" --replace '$1' || true
    rg -o -N "^\s*surface_reexport!\(\s*([A-Za-z_][A-Za-z0-9_]*)\s*=>" "$SURFACE_MOD" --replace '$1' || true
    rg -o -N "^\s*pub\s+use\s+::[A-Za-z_][A-Za-z0-9_]*\s+as\s+([A-Za-z_][A-Za-z0-9_]*)\s*;" "$SURFACE_MOD" --replace '$1' || true
  } | sed '/^$/d' | sort -u
)

# Strict: only `pub mod X` at std crate root.
readarray -t STD_PUBMODS < <(
  rg -o -N "^\s*pub\s+mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*(;|\{)" "$stdlib" --replace '$1' \
    | sed '/^$/d' \
    | sort -u
)

surface_count=${#SURFACE_NAMES[@]}
std_pubmod_count=${#STD_PUBMODS[@]}

surface_sorted="$(printf '%s\n' "${SURFACE_NAMES[@]}" | sort -u)"
std_pubmod_sorted="$(printf '%s\n' "${STD_PUBMODS[@]}" | sort -u)"

overlap_strict_count="$(( $(comm -12 <(printf '%s\n' "$surface_sorted") <(printf '%s\n' "$std_pubmod_sorted") | count_lines) ))"
missing_strict_count="$(( $(comm -23 <(printf '%s\n' "$std_pubmod_sorted") <(printf '%s\n' "$surface_sorted") | count_lines) ))"
surface_only_strict_count="$(( $(comm -13 <(printf '%s\n' "$std_pubmod_sorted") <(printf '%s\n' "$surface_sorted") | count_lines) ))"

coverage_std_strict="$(percent "$overlap_strict_count" "$std_pubmod_count")"
coverage_surface_strict="$(percent "$overlap_strict_count" "$surface_count")"

# Extended: std root pubmods + any matching core/alloc *root modules* for names surface provides.
declare -a STD_EXT=()
STD_EXT+=("${STD_PUBMODS[@]}")

for name in "${SURFACE_NAMES[@]}"; do
  if printf '%s\n' "${STD_PUBMODS[@]}" | rg -qx --fixed-strings "$name"; then
    continue
  fi
  if [[ -d "$core_src" ]] && module_exists_in_src "$core_src" "$name"; then
    STD_EXT+=("$name")
    continue
  fi
  if [[ -d "$alloc_src" ]] && module_exists_in_src "$alloc_src" "$name"; then
    STD_EXT+=("$name")
    continue
  fi
done

std_ext_sorted="$(printf '%s\n' "${STD_EXT[@]}" | sed '/^$/d' | sort -u)"
std_ext_count="$(( $(printf '%s\n' "$std_ext_sorted" | count_lines) ))"

overlap_ext_count="$(( $(comm -12 <(printf '%s\n' "$surface_sorted") <(printf '%s\n' "$std_ext_sorted") | count_lines) ))"
missing_ext_count="$(( $(comm -23 <(printf '%s\n' "$std_ext_sorted") <(printf '%s\n' "$surface_sorted") | count_lines) ))"
surface_only_ext_count="$(( $(comm -13 <(printf '%s\n' "$std_ext_sorted") <(printf '%s\n' "$surface_sorted") | count_lines) ))"

coverage_std_ext="$(percent "$overlap_ext_count" "$std_ext_count")"

print_header "surface vs toolchain std (global table)"
echo "| metric | count | total | % | notes |"
echo "|---|---:|---:|---:|---|"
echo "| surface top-level names | $surface_count | - | - | parsed from src/surface/mod.rs |"
echo "| std crate-root pub mods (strict) | $std_pubmod_count | - | - | parsed from rust-src std/src/lib.rs |"
echo "| overlap: surface ∩ std(root) | $overlap_strict_count | $std_pubmod_count | ${coverage_std_strict}% | coverage of strict std modules |"
echo "| overlap: surface ∩ std(root) | $overlap_strict_count | $surface_count | ${coverage_surface_strict}% | how much of surface matches std root |"
echo "| std(root) missing from surface | $missing_strict_count | $std_pubmod_count | - | std pub mods not exposed by surface |"
echo "| surface-only vs std(root) | $surface_only_strict_count | $surface_count | - | mostly core/alloc modules + aliases |"
echo "| std(root)+core+alloc roots (extended) | $std_ext_count | - | - | adds core/alloc root modules that surface provides |"
echo "| overlap: surface ∩ extended | $overlap_ext_count | $std_ext_count | ${coverage_std_ext}% | 'std-shaped API' name coverage |"
echo "| surface-only vs extended | $surface_only_ext_count | $surface_count | - | names not found as std/core/alloc root modules |"

echo
print_header "std crate-root pub mods missing from surface"
comm -23 <(printf '%s\n' "$std_pubmod_sorted") <(printf '%s\n' "$surface_sorted") || true

echo
print_header "surface-only names (not std crate-root pub mods)"
comm -13 <(printf '%s\n' "$std_pubmod_sorted") <(printf '%s\n' "$surface_sorted") || true
