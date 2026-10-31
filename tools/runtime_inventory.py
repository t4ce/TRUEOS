#!/usr/bin/env python3
"""Index repository-owned runtime declarations without building or booting TRUEOS.

This is a lexical index, not Rust name resolution or a dead-code detector. The
reviewed startup/caller traces live in tools/docs/runtime-inventory.md.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RAW_STRING = re.compile(r'(?:br|cr|r)(#{0,255})"')
CHAR_LITERAL = re.compile(r"'(?:\\(?:u\{[^}\n]*\}|x[0-9A-Fa-f]{2}|[^\n])|[^\\'\n])'")


def mask_rust(source: str, *, strings: bool = True) -> str:
    """Blank comments (including nested blocks) and optionally string literals.

    Preserve positions/newlines for references. Recognize character literals so
    escaped quotes cannot start strings; leave lifetime names intact.
    """
    out = list(source)
    pos = 0
    while pos < len(source):
        start = pos
        if source.startswith("//", pos):
            end = source.find("\n", pos)
            pos = len(source) if end < 0 else end
        elif source.startswith("/*", pos):
            pos += 2
            depth = 1
            while pos < len(source) and depth:
                if source.startswith("/*", pos):
                    depth += 1
                    pos += 2
                elif source.startswith("*/", pos):
                    depth -= 1
                    pos += 2
                else:
                    pos += 1
        else:
            char = CHAR_LITERAL.match(source, pos) if source[pos] == "'" else None
            raw = RAW_STRING.match(source, pos) if source[pos] in "rbc" else None
            if char:
                pos = char.end()
                if not strings:
                    continue
            elif raw:
                closing = '"' + raw[1]
                end = source.find(closing, raw.end())
                pos = len(source) if end < 0 else end + len(closing)
                if not strings:
                    continue
            elif source[pos] == '"':
                pos += 1
                while pos < len(source):
                    if source[pos] == "\\":
                        pos += 2
                    elif source[pos] == '"':
                        pos += 1
                        break
                    else:
                        pos += 1
                if not strings:
                    continue
            else:
                pos += 1
                continue
        for index in range(start, min(pos, len(source))):
            if source[index] != "\n":
                out[index] = " "
    return "".join(out)


def location(path: str, source: str, offset: int) -> dict:
    return {"file": path, "line": source.count("\n", 0, offset) + 1}


def scan() -> dict:
    paths = subprocess.check_output(
        ["rg", "--files", "src", "crates", "-g", "*.rs", "-g", "!**/vendor/**"],
        cwd=ROOT, text=True,
    ).splitlines()
    sources = {path: (ROOT / path).read_text() for path in sorted(paths)}
    masked = {path: mask_rust(source) for path, source in sources.items()}
    tasks, templates, pools = [], [], []
    task_pattern = re.compile(
        r"#\[\s*(?:trueos_executor::|::trueos_executor::)?task\s*"
        r"(?:\((?P<args>[^\]]*)\))?\s*\]\s*"
        r"(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?"
        r"async\s+fn\s+(?P<name>\w+)"
    )
    for path, code in masked.items():
        for match in task_pattern.finditer(code):
            args = match["args"] or ""
            pool = re.search(r"pool_size\s*=\s*(.*?)(?:,|$)", args)
            entry = {
                **location(path, code, match.start()),
                "symbol": match["name"],
                "pool_size": pool[1].strip() if pool else "1 (macro default)",
                "context": "overlay reference" if "/reference/" in path else "source",
            }
            if "/trueos-executor-macros/" in path:
                entry["context"] = "macro template"
                templates.append(entry)
            else:
                tasks.append(entry)
        # This intentionally indexes named resource pools as well as task pools.
        for match in re.finditer(
            r"\b(?:struct|enum|type)\s+(\w*Pool\w*)\b|"
            r"\bstatic(?:\s+mut)?\s+(\w*POOL\w*)\b", code
        ):
            pools.append({**location(path, code, match.start()),
                          "symbol": match[1] or match[2]})

    calls = {task["symbol"]: [] for task in tasks}
    call_pattern = re.compile(
        r"(?<![\w])(?P<path>(?:[A-Za-z_]\w*::)*"
        r"(?P<name>" + "|".join(sorted(map(re.escape, calls))) + r"))\s*\("
    )
    for path, code in masked.items():
        for match in call_pattern.finditer(code):
            if re.search(r"\bfn\s+$", code[max(0, match.start() - 20):match.start()]):
                continue
            calls[match["name"]].append({
                **location(path, code, match.start()), "spelling": match["path"],
            })
    for task in tasks:
        task["call_candidates"] = calls[task["symbol"]]
        task["call_note"] = (
            "Lexical candidates share the function name; imports, aliases, cfg, "
            "and duplicate names require the reviewed traces."
        )
        path = task["file"]
        kernel = path.startswith(("src/r/", "src/spirit/", "src/chronos/")) or path in {
            "src/cpu.rs", "src/cpu_task_pool.rs", "src/executor_task_profile.rs",
            "src/smp.rs", "src/wait.rs",
        }
        task["trace_report"] = (
            "runtime-crates.md" if path.startswith("crates/") else
            "runtime-kernel.md" if kernel else "runtime-services.md"
        )

    registry_path = "src/r/services/spawn_service.rs"
    registry = mask_rust(sources[registry_path], strings=False)
    registry_entries = []
    pattern = re.compile(
        r"TaskSpec::(?P<kind>enabled_gated|configured_gated|enabled|disabled)\s*\("
    )
    for match in pattern.finditer(registry):
        start = match.end()
        depth, pos = 1, start
        while depth and pos < len(registry):
            if registry[pos] == "(":
                depth += 1
            elif registry[pos] == ")":
                depth -= 1
            pos += 1
        body = registry[start:pos - 1]
        name = re.search(r'"([\w-]+)"', body)
        if name:
            registry_entries.append({
                **location(registry_path, registry, match.start()),
                "name": name[1], "constructor": match["kind"],
                "arguments": " ".join(body.split()),
            })
    services = [path for path in sources if "service" in Path(path).stem]
    return {
        "scope": "src/**/*.rs and crates/**/*.rs, excluding vendor; all cfg variants",
        "limits": [
            "Lexical declarations and call candidates, not a resolved call graph.",
            "No compiler cfg evaluation or runtime liveness inference.",
            "Macro templates are separate; public ABI/callback services need manual tracing.",
            "Named pool symbols are an index, not a count of live pool instances.",
            "Shader host C/C++ command pools are covered in the reviewed report.",
        ],
        "rust_files_scanned": len(sources),
        "task_declarations": tasks,
        "macro_templates": templates,
        "registry_constructors": registry_entries,
        "named_pool_declarations": pools,
        "service_named_files": services,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=ROOT / "tools/docs")
    args = parser.parse_args()
    result = scan()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    (args.output_dir / "runtime-inventory-data.json").write_text(
        json.dumps(result, indent=2) + "\n"
    )
    reviewed_dir = ROOT / "tools/docs"
    overview_link = os.path.relpath(reviewed_dir / "runtime-inventory.md", args.output_dir.resolve())
    lines = [
        "# TRUEOS task declaration index", "",
        "Generated by `python3 tools/runtime_inventory.py`. All cfg variants are included.",
        f"Read [the reviewed inventory]({overview_link}) for startup, usage, and dormant status.",
        "Pool sizes are task storage capacities, not CPU/thread counts. Caller candidates in",
        "[the JSON index](runtime-inventory-data.json) require name and cfg resolution.", "",
        "| Task declaration | Task storage slots | Context | Reviewed trace |",
        "| --- | --- | --- | --- |",
    ]
    for task in result["task_declarations"]:
        path, line = task["file"], task["line"]
        source_link = os.path.relpath(ROOT / path, args.output_dir.resolve())
        trace_link = os.path.relpath(reviewed_dir / task["trace_report"], args.output_dir.resolve())
        lines.append(
            f"| [`{task['symbol']}`]({source_link}#L{line}) "
            f"({path}:{line}) | `{task['pool_size']}` | {task['context']} "
            f"| [trace]({trace_link}) |"
        )
    lines += ["", "Macro templates in the executor procedural macro are indexed separately in JSON.", ""]
    (args.output_dir / "runtime-task-index.md").write_text("\n".join(lines))
    print(json.dumps({
        "rust_files": result["rust_files_scanned"],
        "task_declarations": len(result["task_declarations"]),
        "task_contexts": dict(Counter(t["context"] for t in result["task_declarations"])),
        "macro_templates": len(result["macro_templates"]),
        "registry_constructors": len(result["registry_constructors"]),
        "named_pool_declarations": len(result["named_pool_declarations"]),
        "service_named_files": len(result["service_named_files"]),
    }, indent=2))


if __name__ == "__main__":
    main()
