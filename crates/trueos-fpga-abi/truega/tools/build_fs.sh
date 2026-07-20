#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
LOG_DIR="${TRUEGA_LOG_DIR:-/tmp/truega-logs}"
mkdir -p "$LOG_DIR"
LOG="${TRUEGA_BUILD_LOG:-$LOG_DIR/build_fs-$(date +%Y%m%d-%H%M%S).log}"
exec > >(tee -a "$LOG") 2>&1
echo "truega build_fs log=$LOG"
echo "started_at=$(date --iso-8601=seconds)"

find_exe() {
  local name="$1"
  shift
  if command -v "$name" >/dev/null 2>&1; then
    command -v "$name"
    return 0
  fi
  local candidate
  for candidate in "$@"; do
    if [[ -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  return 1
}

GOWIN_SH="$(find_exe gw_sh "$HOME/Programmme/Gowin/IDE/bin/gw_sh" "$HOME/Programmes/Gowin/IDE/bin/gw_sh" "$HOME/Programs/Gowin/IDE/bin/gw_sh")" || {
  echo "missing gw_sh; put Gowin IDE bin on PATH or install under ~/Programmme/Gowin" >&2
  exit 1
}
CARGO_BIN="$(find_exe cargo "$HOME/.cargo/bin/cargo")" || {
  echo "missing cargo; the Ubuntu firmware build requires Rust to generate RTL" >&2
  exit 1
}
GOWIN_IDE_DIR="$(cd "$(dirname "$GOWIN_SH")/.." && pwd)"
GOWIN_LIB="$GOWIN_IDE_DIR/lib"
SYSTEM_FREETYPE="/lib/x86_64-linux-gnu/libfreetype.so.6"
PROJECT_FILE="$PROJECT_DIR/min_pci_led.gprj"
TCL_FILE="$(mktemp)"
PLACE_OPTION="${TRUEGA_PLACE_OPTION:-3}"
ROUTE_OPTION="${TRUEGA_ROUTE_OPTION:-0}"
HOST_TOOLCHAIN="${TRUEGA_HOST_TOOLCHAIN:-1.96}"
HOST_TARGET="${TRUEGA_HOST_TARGET:-x86_64-unknown-linux-gnu}"
GENERATOR_TARGET_DIR="${TRUEGA_GENERATOR_TARGET_DIR:-/tmp/truega-tga-gen-target}"
GENERATOR_MANIFEST="$PROJECT_DIR/tools/tga-gen/Cargo.toml"
GENERATED_RTL="$PROJECT_DIR/src/generated/truega_functions.v"
GENERATED_MANIFEST="$PROJECT_DIR/artifacts/truega_firmware.manifest.bin"
GENERATED_RUST_INTERFACE="$PROJECT_DIR/../src/generated.rs"

finish() {
  local rc=$?
  rm -f "$TCL_FILE"
  echo "finished_at=$(date --iso-8601=seconds) status=$rc"
  exit "$rc"
}
trap finish EXIT

# RustHDL is strictly an Ubuntu build input. Pin the host target and a separate target
# directory so the generator cannot inherit TRUEOS's no_std kernel target/build-std state.
CARGO_TARGET_DIR="$GENERATOR_TARGET_DIR" "$CARGO_BIN" "+$HOST_TOOLCHAIN" run --quiet \
  --manifest-path "$GENERATOR_MANIFEST" \
  --target "$HOST_TARGET" \
  --config 'unstable.build-std=[]' \
  -- \
  --rtl-out "$GENERATED_RTL" \
  --manifest-out "$GENERATED_MANIFEST" \
  --rust-interface-out "$GENERATED_RUST_INTERFACE"

cat > "$TCL_FILE" <<EOF
open_project $PROJECT_FILE
set_option -top_module top
set_option -output_base_name min_pci_led
set_option -place_option $PLACE_OPTION
set_option -route_option $ROUTE_OPTION
set_option -clock_route_order 1
set_option -correct_hold_violation 0
set_option -route_maxfan 23
set_csr $PROJECT_DIR/src/serdes/serdes.csr
run syn
run pnr
EOF

cd "$PROJECT_DIR"
env LD_PRELOAD="$SYSTEM_FREETYPE${LD_PRELOAD:+:$LD_PRELOAD}" LD_LIBRARY_PATH="$GOWIN_LIB${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" "$GOWIN_SH" "$TCL_FILE"

BITSTREAM="$PROJECT_DIR/impl/pnr/min_pci_led.fs"
if [[ ! -s "$BITSTREAM" ]]; then
  echo "Gowin completed without a non-empty bitstream: $BITSTREAM" >&2
  exit 1
fi
sha256sum "$BITSTREAM" > "$PROJECT_DIR/artifacts/min_pci_led.fs.sha256"
echo "bitstream=$BITSTREAM"
echo "manifest=$GENERATED_MANIFEST"
