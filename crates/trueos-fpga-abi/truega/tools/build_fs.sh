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
PLACE_OPTION="${TRUEGA_PLACE_OPTION:-4}"
ROUTE_OPTION="${TRUEGA_ROUTE_OPTION:-0}"
CLOCK_CONVERSION="${TRUEGA_CLOCK_CONVERSION:-1}"
REQUIRED_TLP_FMAX_MHZ="${TRUEGA_REQUIRED_TLP_FMAX_MHZ:-100.5}"
HOST_TOOLCHAIN="${TRUEGA_HOST_TOOLCHAIN:-1.96}"
HOST_TARGET="${TRUEGA_HOST_TARGET:-x86_64-unknown-linux-gnu}"
GENERATOR_TARGET_DIR="${TRUEGA_GENERATOR_TARGET_DIR:-/tmp/truega-tga-gen-target}"
GENERATOR_MANIFEST="$PROJECT_DIR/tools/tga-gen/Cargo.toml"
GENERATED_RTL="$PROJECT_DIR/src/generated/truega_functions.v"
GENERATED_MANIFEST="$PROJECT_DIR/artifacts/truega_firmware.manifest.bin"
GENERATED_RUST_INTERFACE="$PROJECT_DIR/../src/generated.rs"
PUBLISHED_BITSTREAM="$PROJECT_DIR/artifacts/min_pci_led.fs"
PUBLISHED_CHECKSUMS="$PROJECT_DIR/artifacts/SHA256SUMS"
PUBLISHED_BITSTREAM_SHA256="$PROJECT_DIR/artifacts/min_pci_led.fs.sha256"
STAGE_DIR="$(mktemp -d)"
TCL_FILE="$STAGE_DIR/build.tcl"
BUILD_MARKER="$STAGE_DIR/build.started"
STAGED_RTL="$STAGE_DIR/truega_functions.v"
STAGED_MANIFEST="$STAGE_DIR/truega_firmware.manifest.bin"
STAGED_RUST_INTERFACE="$STAGE_DIR/generated.rs"
RTL_BACKUP="$STAGE_DIR/truega_functions.previous.v"
RTL_HAD_PRIOR=0
RTL_IS_STAGED=0
RTL_SWAP_TMP=""
PUBLISH_TEMPS=()

restore_rtl() {
  if (( ! RTL_IS_STAGED )); then
    return 0
  fi

  if (( RTL_HAD_PRIOR )); then
    local restore_tmp
    restore_tmp="$(mktemp "$GENERATED_RTL.restore.XXXXXX")"
    cp -p -- "$RTL_BACKUP" "$restore_tmp"
    mv -f -- "$restore_tmp" "$GENERATED_RTL"
  else
    rm -f -- "$GENERATED_RTL"
  fi
  RTL_IS_STAGED=0
}

prepare_publication() {
  local source="$1"
  local destination="$2"
  local result_variable="$3"
  local destination_dir
  local publication_tmp

  destination_dir="$(dirname "$destination")"
  mkdir -p "$destination_dir"
  publication_tmp="$(mktemp "$destination_dir/.truega-publish.XXXXXX")"
  PUBLISH_TEMPS+=("$publication_tmp")
  cp -p -- "$source" "$publication_tmp"
  printf -v "$result_variable" '%s' "$publication_tmp"
}

finish() {
  local rc=$?
  local publication_tmp

  set +e
  restore_rtl
  if [[ -n "${RTL_SWAP_TMP:-}" ]]; then
    rm -f -- "$RTL_SWAP_TMP"
  fi
  for publication_tmp in "${PUBLISH_TEMPS[@]}"; do
    rm -f -- "$publication_tmp"
  done
  if [[ -n "${STAGE_DIR:-}" && -d "$STAGE_DIR" ]]; then
    rm -rf -- "$STAGE_DIR"
  fi
  echo "finished_at=$(date --iso-8601=seconds) status=$rc"
  exit "$rc"
}
trap finish EXIT

# RustHDL is strictly an Ubuntu build input. Pin the host target and a separate target
# directory so the generator cannot inherit TRUEOS's no_std kernel target/build-std state.
CARGO_TARGET_DIR="$GENERATOR_TARGET_DIR" "$CARGO_BIN" "+$HOST_TOOLCHAIN" test --quiet \
  --manifest-path "$GENERATOR_MANIFEST" \
  --target "$HOST_TARGET" \
  --config 'unstable.build-std=[]'

CARGO_TARGET_DIR="$GENERATOR_TARGET_DIR" "$CARGO_BIN" "+$HOST_TOOLCHAIN" run --quiet \
  --manifest-path "$GENERATOR_MANIFEST" \
  --target "$HOST_TARGET" \
  --config 'unstable.build-std=[]' \
  -- \
  --rtl-out "$STAGED_RTL" \
  --manifest-out "$STAGED_MANIFEST" \
  --rust-interface-out "$STAGED_RUST_INTERFACE"

for staged_output in "$STAGED_RTL" "$STAGED_MANIFEST" "$STAGED_RUST_INTERFACE"; do
  if [[ ! -s "$staged_output" ]]; then
    echo "generator did not produce a non-empty staged output: $staged_output" >&2
    exit 1
  fi
done

# The Gowin project names the checked-in RTL path. Temporarily replace that one input,
# but retain the published copy until this exact generated design completes PnR.
mkdir -p "$(dirname "$GENERATED_RTL")"
if [[ -e "$GENERATED_RTL" ]]; then
  cp -p -- "$GENERATED_RTL" "$RTL_BACKUP"
  RTL_HAD_PRIOR=1
fi
RTL_SWAP_TMP="$(mktemp "$GENERATED_RTL.staged.XXXXXX")"
cp -p -- "$STAGED_RTL" "$RTL_SWAP_TMP"
RTL_IS_STAGED=1
mv -f -- "$RTL_SWAP_TMP" "$GENERATED_RTL"
RTL_SWAP_TMP=""

cat > "$TCL_FILE" <<EOF
open_project $PROJECT_FILE
set_option -top_module top
set_option -output_base_name min_pci_led
set_option -fix_gated_and_generated_clocks $CLOCK_CONVERSION
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
: > "$BUILD_MARKER"
env LD_PRELOAD="$SYSTEM_FREETYPE${LD_PRELOAD:+:$LD_PRELOAD}" LD_LIBRARY_PATH="$GOWIN_LIB${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" "$GOWIN_SH" "$TCL_FILE"

BITSTREAM="$PROJECT_DIR/impl/pnr/min_pci_led.fs"
if [[ ! -s "$BITSTREAM" || ! "$BITSTREAM" -nt "$BUILD_MARKER" ]]; then
  echo "Gowin completed without updating a non-empty bitstream: $BITSTREAM" >&2
  exit 1
fi

# A successful router invocation is not enough: this image must close the live
# 100 MHz TLP clock and have no setup/hold violations.  Keep the check local to
# the generated report so a marginal or unconstrained image cannot be published.
TIMING_REPORT="$PROJECT_DIR/impl/pnr/min_pci_led_tr_content.html"
if [[ ! -s "$TIMING_REPORT" ]]; then
  echo "Gowin completed without a timing report: $TIMING_REPORT" >&2
  exit 1
fi
TLP_FMAX="$({
  sed -E 's/<[^>]+>//g' "$TIMING_REPORT" |
    awk '
      $0 == "tlp_clk" { saw_clock = 1; next }
      saw_clock && /\(MHz\)/ {
        gsub(/\(MHz\)/, "");
        if ($0 ~ /^[0-9]+([.][0-9]+)?$/) values[++count] = $0;
        if (count == 2) { print values[2]; exit }
      }
    '
} || true)"
VIOLATED_ENDPOINTS="$(
  sed -E 's/<[^>]+>//g' "$TIMING_REPORT" |
    awk '
      /Numbers of Setup Violated Endpoints/ { getline; setup = $0 }
      /Numbers of Hold Violated Endpoints/ { getline; hold = $0 }
      END { if (setup != "" && hold != "") print setup + hold }
    '
)"
if [[ -z "$TLP_FMAX" || -z "$VIOLATED_ENDPOINTS" ]]; then
  echo "could not parse TLP Fmax/violations from $TIMING_REPORT" >&2
  exit 1
fi
if ! awk -v actual="$TLP_FMAX" -v required="$REQUIRED_TLP_FMAX_MHZ" \
  'BEGIN { exit !(actual >= required) }'; then
  echo "timing failure: tlp_clk actual_fmax_mhz=$TLP_FMAX required_fmax_mhz=$REQUIRED_TLP_FMAX_MHZ" >&2
  exit 1
fi
if [[ "$VIOLATED_ENDPOINTS" != "0" ]]; then
  echo "timing failure: violated_endpoints=$VIOLATED_ENDPOINTS" >&2
  exit 1
fi
echo "timing=pass tlp_actual_fmax_mhz=$TLP_FMAX required_fmax_mhz=$REQUIRED_TLP_FMAX_MHZ violated_endpoints=0"

# End the temporary Gowin input swap before publishing anything. Each destination is
# prepared on its own filesystem and renamed into place; SHA256SUMS is the final seal.
restore_rtl
STAGED_ARTIFACTS="$STAGE_DIR/artifacts"
mkdir -p "$STAGED_ARTIFACTS"
cp -p -- "$BITSTREAM" "$STAGED_ARTIFACTS/min_pci_led.fs"
cp -p -- "$STAGED_MANIFEST" "$STAGED_ARTIFACTS/truega_firmware.manifest.bin"
chmod 0644 "$STAGED_ARTIFACTS/min_pci_led.fs" "$STAGED_ARTIFACTS/truega_firmware.manifest.bin"
(
  cd "$STAGED_ARTIFACTS"
  sha256sum min_pci_led.fs truega_firmware.manifest.bin > SHA256SUMS
  sha256sum min_pci_led.fs > min_pci_led.fs.sha256
)

prepare_publication "$STAGED_RTL" "$GENERATED_RTL" PUBLISH_RTL_TMP
prepare_publication "$STAGED_ARTIFACTS/truega_firmware.manifest.bin" "$GENERATED_MANIFEST" PUBLISH_MANIFEST_TMP
prepare_publication "$STAGED_RUST_INTERFACE" "$GENERATED_RUST_INTERFACE" PUBLISH_RUST_TMP
prepare_publication "$STAGED_ARTIFACTS/min_pci_led.fs" "$PUBLISHED_BITSTREAM" PUBLISH_BITSTREAM_TMP
prepare_publication "$STAGED_ARTIFACTS/min_pci_led.fs.sha256" "$PUBLISHED_BITSTREAM_SHA256" PUBLISH_BITSTREAM_SHA256_TMP
prepare_publication "$STAGED_ARTIFACTS/SHA256SUMS" "$PUBLISHED_CHECKSUMS" PUBLISH_CHECKSUMS_TMP

rm -f -- "$PUBLISHED_CHECKSUMS"
mv -f -- "$PUBLISH_RTL_TMP" "$GENERATED_RTL"
mv -f -- "$PUBLISH_MANIFEST_TMP" "$GENERATED_MANIFEST"
mv -f -- "$PUBLISH_RUST_TMP" "$GENERATED_RUST_INTERFACE"
mv -f -- "$PUBLISH_BITSTREAM_TMP" "$PUBLISHED_BITSTREAM"
mv -f -- "$PUBLISH_BITSTREAM_SHA256_TMP" "$PUBLISHED_BITSTREAM_SHA256"
mv -f -- "$PUBLISH_CHECKSUMS_TMP" "$PUBLISHED_CHECKSUMS"

echo "bitstream=$PUBLISHED_BITSTREAM"
echo "manifest=$GENERATED_MANIFEST"
echo "rust_interface=$GENERATED_RUST_INTERFACE"
echo "checksums=$PUBLISHED_CHECKSUMS"
