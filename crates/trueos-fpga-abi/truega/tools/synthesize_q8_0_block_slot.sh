#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
RTL_DIR="$PROJECT_DIR/src/compute"
GOWIN_SH="${TRUEGA_GOWIN_SH:-$HOME/Programmme/Gowin/IDE/bin/gw_sh}"
SYSTEM_FREETYPE="/lib/x86_64-linux-gnu/libfreetype.so.6"
STAGE_DIR="$(mktemp -d)"
PROJECT_FILE="$STAGE_DIR/q8_0_block_slot.gprj"
SDC_FILE="$STAGE_DIR/q8_0_block_slot.sdc"
TCL_FILE="$STAGE_DIR/synthesize.tcl"

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

cat > "$SDC_FILE" <<'EOF'
create_clock -name clk -period 10.000 [get_ports {clk}]
EOF

cat > "$PROJECT_FILE" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE gowin-fpga-project>
<Project>
    <Template>FPGA</Template>
    <Version>5</Version>
    <Device name="GW5AST-138B" pn="GW5AST-LV138FPG676AES">gw5ast138b-002</Device>
    <FileList>
        <File path="$RTL_DIR/truega_q8_0_dot32.v" type="file.verilog" enable="1"/>
        <File path="$RTL_DIR/truega_q8_0_scale_q30_seq.v" type="file.verilog" enable="1"/>
        <File path="$RTL_DIR/truega_q8_0_block_slot.v" type="file.verilog" enable="1"/>
        <File path="$SDC_FILE" type="file.sdc" enable="1"/>
    </FileList>
</Project>
EOF

cat > "$TCL_FILE" <<EOF
open_project $PROJECT_FILE
set_option -top_module truega_q8_0_block_slot
set_option -output_base_name truega_q8_0_block_slot
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

if find "$STAGE_DIR" -type f -name '*.fs' -print -quit | grep -q .; then
  echo "isolated synthesis unexpectedly produced a flashable .fs image" >&2
  exit 1
fi

REPORT="$(find "$STAGE_DIR" -type f -name '*_syn.rpt.html' -print | head -n 1)"
if [[ -z "$REPORT" ]]; then
  echo "missing synthesis report" >&2
  exit 1
fi
REPORT_TEXT="$STAGE_DIR/synthesis-report.txt"
sed -E 's/<[^>]+>//g; s/&nbsp;/ /g' "$REPORT" > "$REPORT_TEXT"
FMAX="$(grep -A12 'Actual Fmax' "$REPORT_TEXT" | grep -Eo '[0-9]+\.[0-9]+' | tail -n 1 || true)"
if [[ -z "$FMAX" ]]; then
  echo "could not read Actual Fmax from $REPORT" >&2
  exit 1
fi
if ! awk -v fmax="$FMAX" 'BEGIN { exit !(fmax >= 100.0) }'; then
  echo "Q8_0 block slot misses 100 MHz: Actual Fmax ${FMAX} MHz" >&2
  exit 1
fi

echo "synthesis_report=$REPORT"
grep -E '^(Logic|Register|[[:space:]]*--Register|[[:space:]]*MULT|Actual Fmax|[0-9]+\.[0-9]+\(MHz\))' "$REPORT_TEXT" \
  | head -n 24 || true
echo "PASS isolated_q8_0_block_slot_synthesis device=GW5AST-138B required_fmax_mhz=100 actual_fmax_mhz=$FMAX bitstream=not_generated"
