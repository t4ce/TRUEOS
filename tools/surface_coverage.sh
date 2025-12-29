#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SURFACE_MOD="$ROOT/src/surface/mod.rs"

MODE="usage"
if (($# >= 1)); then
  case "$1" in
    --vs-std|--compare-std)
      MODE="vs-std"
      ;;
    -h|--help)
      cat <<'EOF'
usage: tools/surface_coverage.sh [--vs-std]

Default mode prints which `std::X` modules are referenced in src/ and vendor/ and
which of those are not provided by `src/surface/mod.rs`.

--vs-std: compare provided surface modules against the toolchain's actual `std`
          crate root modules (from rust-src) and print a coverage summary.
EOF
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      exit 2
      ;;
  esac
fi

if ! command -v rg >/dev/null 2>&1; then
  echo "error: ripgrep (rg) not found" >&2
  exit 1
fi

readarray -t PROVIDED < <(
  {
    # Plain module declarations.
    rg -o -N "^\s*pub\s+mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*(;|\{)" "$SURFACE_MOD" --replace '$1' || true
    # Modules generated via the surface_reexport! macro.
    rg -o -N "^\s*surface_reexport!\(\s*([A-Za-z_][A-Za-z0-9_]*)\s*=>" "$SURFACE_MOD" --replace '$1' || true
  } | sort -u
)

print_header() {
  echo "============================================================"
  echo "$1"
  echo "============================================================"
}

collect_std_root_modules() {
  local sysroot stdlib
  sysroot="$(rustc --print sysroot 2>/dev/null || true)"
  if [[ -z "$sysroot" ]]; then
    echo "error: rustc not found" >&2
    return 1
  fi

  stdlib="$sysroot/lib/rustlib/src/rust/library/std/src/lib.rs"
  if [[ ! -f "$stdlib" ]]; then
    echo "error: rust-src not installed for this toolchain" >&2
    echo "hint: rustup component add rust-src" >&2
    return 1
  fi

  # Parse module-like names exported by std:
  # - `pub mod foo;` / `pub mod foo {`
  # - `pub use core::{foo, bar, ...};` (fmt/mem/etc are commonly exported this way)
  # - `pub use alloc::{...};`
  python3 - "$stdlib" <<'PY'
import re
import sys

path = sys.argv[1]
text = open(path, 'r', encoding='utf-8', errors='ignore').read()

mods = set()

# pub mod foo;
for m in re.finditer(r"^\s*pub\s+mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?:;|\{)", text, re.M):
    mods.add(m.group(1))

# pub use core::foo; / pub use core::foo::bar; / pub use core::foo::{...}
for m in re.finditer(r"^\s*pub\s+use\s+(core|alloc)::([A-Za-z_][A-Za-z0-9_]*)\b", text, re.M):
    mods.add(m.group(2))

# pub use core::{a, b, c}; and multi-line variants.
for m in re.finditer(r"^\s*pub\s+use\s+(core|alloc)::\{(.*?)\};", text, re.M | re.S):
    body = m.group(2)
    # strip comments
    body = re.sub(r"//.*", "", body)
    body = re.sub(r"/\*.*?\*/", "", body, flags=re.S)
    for part in body.split(','):
        part = part.strip()
        if not part:
            continue
        # handle `name as alias` and `self`/`super`
        name = part.split()[0]
        if name in {"self", "super"}:
            continue
        # keep lowercase-ish identifiers only (filters out types like `String`)
        if re.match(r"^[a-z][a-z0-9_]*$", name):
            mods.add(name)

for name in sorted(mods):
    print(name)
PY
}

array_contains() {
  local needle="$1"; shift
  local x
  for x in "$@"; do
    if [[ "$x" == "$needle" ]]; then
      return 0
    fi
  done
  return 1
}

print_list() {
  local title="$1"; shift
  print_header "$title"
  if ((${#PROVIDED[@]} == 0)); then
    echo "(none)"
  else
    printf '%s\n' "${PROVIDED[@]}"
  fi
  echo
}

collect_std_modules() {
  local scope_dir="$1"

  # Emit only the first segment after std::, like `io`, `sync`, `collections`.
  rg -o --no-filename "std::[A-Za-z_][A-Za-z0-9_]*" "$scope_dir" \
    -g"*.rs" \
    -g"!target/**" -g"!bld/**" -g"!limine/**" \
    2>/dev/null \
    | sed -E 's/^std:://' \
    | sort \
    | uniq -c \
    | sort -nr
}

unique_std_modules() {
  local scope_dir="$1"
  rg -o --no-filename "std::[A-Za-z_][A-Za-z0-9_]*" "$scope_dir" \
    -g"*.rs" \
    -g"!target/**" -g"!bld/**" -g"!limine/**" \
    2>/dev/null \
    | sed -E 's/^std:://' \
    | sort -u
}

print_header "surface coverage report"
echo "root: $ROOT"
echo "surface: $SURFACE_MOD"
echo "mode: $MODE"
echo

print_header "provided surface modules"
if ((${#PROVIDED[@]} == 0)); then
  echo "(none)"
else
  printf '%s\n' "${PROVIDED[@]}"
fi
echo

for SCOPE in src vendor; do
  DIR="$ROOT/$SCOPE"
  if [[ ! -d "$DIR" ]]; then
    continue
  fi

  print_header "std::X usage counts in $SCOPE/"
  if rg -q "std::" "$DIR" -g"*.rs" -g"!target/**" -g"!bld/**" -g"!limine/**" 2>/dev/null; then
    collect_std_modules "$DIR"
  else
    echo "(no std:: references found)"
  fi
  echo

done

USED_ALL="$(mktemp)"
trap 'rm -f "$USED_ALL"' EXIT

{
  unique_std_modules "$ROOT/src" || true
  unique_std_modules "$ROOT/vendor" || true
} | sort -u >"$USED_ALL"

print_header "std::X modules referenced but not provided by surface"
if ((${#PROVIDED[@]} == 0)); then
  cat "$USED_ALL" || true
  exit 0
fi

# Print modules present in USED_ALL but not in PROVIDED.
missing=0
while IFS= read -r mod; do
  [[ -z "$mod" ]] && continue
  found=0
  for p in "${PROVIDED[@]}"; do
    if [[ "$mod" == "$p" ]]; then
      found=1
      break
    fi
  done
  if ((found == 0)); then
    echo "$mod"
    missing=$((missing + 1))
  fi
done <"$USED_ALL"

if ((missing == 0)); then
  echo "(none)"
fi

if [[ "$MODE" == "vs-std" ]]; then
  print_header "surface vs toolchain std (top-level modules)"

  readarray -t STD_ROOT < <(collect_std_root_modules || true)
  if ((${#STD_ROOT[@]} == 0)); then
    echo "(unable to load std module list; ensure rust-src is installed)"
    exit 0
  fi

  std_count=${#STD_ROOT[@]}
  surface_count=${#PROVIDED[@]}

  overlap=0
  for s in "${STD_ROOT[@]}"; do
    if array_contains "$s" "${PROVIDED[@]}"; then
      overlap=$((overlap + 1))
    fi
  done

  extra=0
  for p in "${PROVIDED[@]}"; do
    if ! array_contains "$p" "${STD_ROOT[@]}"; then
      extra=$((extra + 1))
    fi
  done

  pct=0
  if ((std_count > 0)); then
    pct=$((overlap * 100 / std_count))
  fi

  echo "std modules: $std_count"
  echo "surface modules: $surface_count"
  echo "overlap: $overlap ($pct%)"
  echo "surface-only: $extra"
  echo

  print_header "std modules missing from surface"
  missing_any=0
  for s in "${STD_ROOT[@]}"; do
    if ! array_contains "$s" "${PROVIDED[@]}"; then
      echo "$s"
      missing_any=1
    fi
  done
  if ((missing_any == 0)); then
    echo "(none)"
  fi
  echo

  print_header "surface modules not in std"
  extra_any=0
  for p in "${PROVIDED[@]}"; do
    if ! array_contains "$p" "${STD_ROOT[@]}"; then
      echo "$p"
      extra_any=1
    fi
  done
  if ((extra_any == 0)); then
    echo "(none)"
  fi
fi
