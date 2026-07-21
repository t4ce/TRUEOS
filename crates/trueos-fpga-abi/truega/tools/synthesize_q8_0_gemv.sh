#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
RTL_DIR="$PROJECT_DIR/src/compute"
GOWIN_SH="${TRUEGA_GOWIN_SH:-$HOME/Programmme/Gowin/IDE/bin/gw_sh}"
SYSTEM_FREETYPE="/lib/x86_64-linux-gnu/libfreetype.so.6"
STAGE_DIR="$(mktemp -d)"
PROJECT_FILE="$STAGE_DIR/q8_0_gemv.gprj"
TCL_FILE="$STAGE_DIR/synthesize.tcl"
GUARDED_FILES=(
  "$PROJECT_DIR/min_pci_led.gprj"
  "$PROJECT_DIR/src/top.vhd"
  "$PROJECT_DIR/src/generated/truega_functions.v"
  "$PROJECT_DIR/artifacts/min_pci_led.fs"
  "$PROJECT_DIR/artifacts/min_pci_led.fs.sha256"
  "$PROJECT_DIR/artifacts/SHA256SUMS"
)

finish() {
  local rc=$?
  if [[ "${TRUEGA_Q8_KEEP_STAGE:-0}" == 1 ]]; then
    echo "kept isolated synthesis stage=$STAGE_DIR"
  else
    rm -rf -- "$STAGE_DIR"
  fi
  exit "$rc"
}
trap finish EXIT

if [[ ! -x "$GOWIN_SH" ]]; then
  echo "missing gw_sh: $GOWIN_SH" >&2
  exit 1
fi

sha256sum "${GUARDED_FILES[@]}" > "$STAGE_DIR/heartbeat.before.sha256"

cat > "$PROJECT_FILE" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE gowin-fpga-project>
<Project>
    <Template>FPGA</Template>
    <Version>5</Version>
    <Device name="GW5AST-138B" pn="GW5AST-LV138FPG676AES">gw5ast138b-002</Device>
    <FileList>
        <File path="$RTL_DIR/truega_q8_0_dot32.v" type="file.verilog" enable="1"/>
        <File path="$RTL_DIR/truega_q8_0_scale_q30.v" type="file.verilog" enable="1"/>
        <File path="$RTL_DIR/truega_q8_0_gemv.v" type="file.verilog" enable="1"/>
        <File path="$RTL_DIR/truega_q8_0_gemv_standalone.v" type="file.verilog" enable="1"/>
    </FileList>
</Project>
EOF

cat > "$TCL_FILE" <<EOF
open_project $PROJECT_FILE
set_option -top_module truega_q8_0_gemv_standalone
set_option -output_base_name truega_q8_0_gemv_standalone
run syn
EOF

GOWIN_IDE_DIR="$(cd "$(dirname "$GOWIN_SH")/.." && pwd)"
GOWIN_LIB="$GOWIN_IDE_DIR/lib"
(
  cd "$STAGE_DIR"
  env LD_PRELOAD="$SYSTEM_FREETYPE${LD_PRELOAD:+:$LD_PRELOAD}" \
      LD_LIBRARY_PATH="$GOWIN_LIB${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
      "$GOWIN_SH" "$TCL_FILE"
)

sha256sum "${GUARDED_FILES[@]}" > "$STAGE_DIR/heartbeat.after.sha256"
if ! cmp -s "$STAGE_DIR/heartbeat.before.sha256" "$STAGE_DIR/heartbeat.after.sha256"; then
  echo "heartbeat input or bitstream changed during isolated synthesis" >&2
  diff -u "$STAGE_DIR/heartbeat.before.sha256" "$STAGE_DIR/heartbeat.after.sha256" >&2 || true
  exit 1
fi
if find "$STAGE_DIR" -type f -name '*.fs' -print -quit | grep -q .; then
  echo "isolated synthesis unexpectedly produced a flashable .fs image" >&2
  exit 1
fi

report="$(find "$STAGE_DIR" -type f -name '*_syn.rpt.html' -print | head -n 1)"
if [[ -n "$report" ]]; then
  echo "synthesis_report=$report"
  sed -E 's/<[^>]+>//g; s/&nbsp;/ /g' "$report" \
    | grep -E '^(Logic|Register|[[:space:]]*--Register|[[:space:]]*MULT|Actual Fmax|[0-9]+\.[0-9]+\(MHz\))' \
    | head -n 24 || true
fi
echo "PASS isolated_q8_0_synthesis device=GW5AST-138B heartbeat_inputs=unchanged bitstream=not_generated"
