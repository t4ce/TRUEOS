#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# These are intentionally *not pinned* (by user choice). You can override them per-invocation:
#   LIMINE_REF=v10.x ./scripts/fetch-deps.sh
LIMINE_REPO="${LIMINE_REPO:-https://github.com/limine-bootloader/limine.git}"
LIMINE_REF="${LIMINE_REF:-v10.x}"

clone_or_update() {
  local name="$1" path="$2" repo="$3" ref="$4"

  if [[ -d "$path/.git" ]] || [[ -f "$path/.git" ]]; then
    echo "[deps] $name: already present at $path"
    return 0
  fi

  echo "[deps] $name: cloning $repo ($ref) -> $path"
  rm -rf "$path"
  git clone --depth 1 --branch "$ref" "$repo" "$path"
}

clone_or_update "limine"  "$ROOT_DIR/limine"  "$LIMINE_REPO"  "$LIMINE_REF"
