#!/usr/bin/env python3
"""Mechanical guard for the read-only Shell2 BIOS schema and dedicated dump."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BIOS_DUMP = ROOT / "src/shell2/cmds/bios_tlb_dump.rs"
BIOS_DUMP_PARTS = sorted((ROOT / "src/shell2/cmds/bios_tlb_dump").glob("*.rs"))
OBSERVED = ROOT / "src/shell2/cmds/bios_observed.rs"
BLUEPRINT = ROOT / "src/shell2/cmds/bios_blueprint.rs"
DEDICATED_WRITER = ROOT / "src/shell2/cmds/bios_dump.rs"
TLB_WRITER = ROOT / "src/shell2/cmds/tlb_hfi_dump.rs"
if len(BIOS_DUMP_PARTS) != 5:
    raise SystemExit("expected five BIOS dump source parts")

SOURCES = [
    ROOT / "src/shell2/cmds/bios_hii.rs",
    *sorted((ROOT / "src/shell2/cmds/bios_hii").glob("*.rs")),
    ROOT / "src/shell2/cmds/bios_ifr.rs",
    *sorted((ROOT / "src/shell2/cmds/bios_ifr").glob("*.rs")),
    ROOT / "src/shell2/cmds/bios_browser.rs",
    BLUEPRINT,
    *sorted((ROOT / "src/shell2/cmds/bios_browser").glob("*.rs")),
    BIOS_DUMP,
    *BIOS_DUMP_PARTS,
    OBSERVED,
    DEDICATED_WRITER,
]
ROUTER = ROOT / "src/shell2/shell2_cmd.rs"

missing = [
    str(path.relative_to(ROOT))
    for path in SOURCES + [ROUTER, TLB_WRITER]
    if not path.is_file()
]
if missing:
    raise SystemExit(f"missing BIOS schema source files: {', '.join(missing)}")

text = "\n".join(path.read_text(encoding="utf-8") for path in SOURCES)
folded = text.casefold()
forbidden_calls = (
    "get_variable(",
    "set_variable(",
    "route_config(",
    "form_browser",
    "update_capsule(",
    "reset_system(",
    "runtime_services(",
)
for token in forbidden_calls:
    if token in folded:
        raise SystemExit(f"read-only BIOS boundary violated by token: {token}")

for token in (
    "active_write_path=none",
    "trueos_write    locked",
    "captured-redacted",
    "question_match=none",
    "validated-question-records-only",
):
    if token not in text:
        raise SystemExit(f"missing BIOS safety/output guard: {token}")

router = ROUTER.read_text(encoding="utf-8")
for module in ("bios_hii::try_parse", "bios_browser::try_parse", "bios_dump::start"):
    if module not in router:
        raise SystemExit(f"Shell2 router does not expose {module}")

browser = "\n".join(
    path.read_text(encoding="utf-8")
    for path in [
        ROOT / "src/shell2/cmds/bios_browser.rs",
        *sorted((ROOT / "src/shell2/cmds/bios_browser").glob("*.rs")),
    ]
)
for command in ("schema", "forms", "find", "show", "options", "storage"):
    if f'"{command}"' not in browser:
        raise SystemExit(f"missing BIOS browser command: {command}")

bios_dump = "\n".join(
    path.read_text(encoding="utf-8")
    for path in [BIOS_DUMP, *BIOS_DUMP_PARTS, OBSERVED, DEDICATED_WRITER]
)
for token in (
    "trueos.bios.dump.ndjson.v2",
    "complete_for_captured_hii=true",
    "complete_motherboard_setup_surface=not-claimed",
    "all-ordered-ifr-nodes",
    '"record": "ifr-node"',
    '"record": "ordered-ifr-summary"',
    "semantically_unresolved_opcodes",
    "redacted-not-included",
    "trueos/pci/bios.txt",
):
    if token not in bios_dump:
        raise SystemExit(f"missing dedicated BIOS dump contract: {token}")

observed = OBSERVED.read_text(encoding="utf-8")
for opcode_name in (
    "subtitle",
    "text",
    "password",
    "ref",
    "eq-id-val",
    "or",
    "set-expression",
    "write-expression",
    "uint64-expression",
    "true-expression",
    "this-expression",
    "guid-extension",
):
    if opcode_name not in observed:
        raise SystemExit(f"observed IFR decoder missing: {opcode_name}")

blueprint = BLUEPRINT.read_text(encoding="utf-8")
for token in (
    "trueos-bios-schema/v2",
    "trueos-bios-presentation/v1",
    '"presentation"',
    '"completeForCapturedHii": true',
    '"completeMotherboardSetupSurface": "not-claimed"',
    '"semanticallyUnresolvedOpcodes"',
    '"nodes": presentation_nodes',
    'object.remove("raw_hex")',
    'details.remove("payload_hex")',
):
    if token not in blueprint:
        raise SystemExit(f"missing Blueprint BIOS presentation contract: {token}")

if "append_ordered_ifr_records" not in blueprint:
    raise SystemExit("Blueprint BIOS snapshot does not consume the ordered IFR presentation stream")

if "bios_tlb_dump::append_dump" in TLB_WRITER.read_text(encoding="utf-8"):
    raise SystemExit("generic tlb dump still embeds the BIOS dump")

print("bios-schema-boundary: read-only decoder, presentation ABI, and dedicated BIOS dump verified")
