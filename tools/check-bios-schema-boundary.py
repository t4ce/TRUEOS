#!/usr/bin/env python3
"""Mechanical guard for the read-only Shell2 BIOS schema cycle."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SOURCES = [
    ROOT / "src/shell2/cmds/bios_hii.rs",
    *sorted((ROOT / "src/shell2/cmds/bios_hii").glob("*.rs")),
    ROOT / "src/shell2/cmds/bios_ifr.rs",
    *sorted((ROOT / "src/shell2/cmds/bios_ifr").glob("*.rs")),
    ROOT / "src/shell2/cmds/bios_browser.rs",
    ROOT / "src/shell2/cmds/bios_blueprint.rs",
    *sorted((ROOT / "src/shell2/cmds/bios_browser").glob("*.rs")),
]
ROUTER = ROOT / "src/shell2/shell2_cmd.rs"

missing = [str(path.relative_to(ROOT)) for path in SOURCES + [ROUTER] if not path.is_file()]
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

required_output_guards = (
    "active_write_path=none",
    "trueos_write    locked",
    "captured-redacted",
    "question_match=none",
    "validated-question-records-only",
)
for token in required_output_guards:
    if token not in text:
        raise SystemExit(f"missing BIOS safety/output guard: {token}")

router = ROUTER.read_text(encoding="utf-8")
for module in ("bios_hii::try_parse", "bios_browser::try_parse"):
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

print("bios-schema-boundary: read-only surface verified")
