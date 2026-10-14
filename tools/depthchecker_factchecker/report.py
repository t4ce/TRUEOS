#!/usr/bin/env python3
"""Turn raw first-hit captures into a plain, inspectable stack/depth report."""
import argparse
from functools import lru_cache
import hashlib
import json
from pathlib import Path
import subprocess


@lru_cache(maxsize=None)
def demangle(name):
    return subprocess.check_output(["c++filt", "-s", "rust", name], text=True).strip()


def symbol(frame):
    if frame.get("kernel"):
        return demangle(frame["kernel"]["symbol"])
    if frame.get("module", {}).get("symbols"):
        return " | ".join(map(demangle, frame["module"]["symbols"]))
    return frame.get("kernel_or_external_symbol", "unresolved")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run", type=Path, required=True)
    parser.add_argument("--raw", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    summary = json.loads((args.run / "captures/capture-summary.json").read_text())
    runtime = json.loads((args.run / "result.json").read_text())
    cargo = json.loads((args.raw / "cargo-depth.json").read_text())
    rows, stacks, files = [], [], {}
    for scenario in summary["scenarios"]:
        for target, hit in scenario["hits"].items():
            path = Path(hit["capture"])
            capture = json.loads(path.read_text())
            stack_file = path.parent / "stack.bin"
            stack_hash = hashlib.sha256(stack_file.read_bytes()).hexdigest()
            if stack_hash != capture["raw_stack"]["sha256"] or hit["first_hit_count"] != 1:
                raise ValueError(f"Capture integrity failed: {path}")
            walk = capture["physical_stack"]
            frames = walk["frames"]
            rsp = frames[0]["rsp"]
            row = {"scenario": scenario["id"], "target": target,
                   "entry_alignment_mod16": rsp % 16, "entry_alignment_expected_mod16": 8,
                   "physical_frames": len(frames), "termination": walk["termination"],
                   "recognized_physical_root": walk["complete"],
                   "raw_stack_bytes": capture["raw_stack"]["requested_bytes"],
                   "unreadable_bytes": capture["raw_stack"]["unreadable_bytes"],
                   "capture": str(path.resolve()), "first_hit_count": 1,
                   "scoped_identity": capture.get("scoped_identity", {}),
                   "register_origin": capture.get("register_origin", "see capture register enumeration"),
                   "top_function": symbol(frames[0]), "last_function": symbol(frames[-1])}
            rows.append(row)
            stacks.append(f"{scenario['id']} :: {row['top_function']}\n"
                          f"first_hit=1 frames={len(frames)} rsp={rsp:#x} rsp_mod16={rsp % 16} "
                          f"termination={walk['termination']} complete={walk['complete']}\n"
                          f"capture={path.resolve()}\n")
            for index, frame in enumerate(frames):
                stacks.append(f"  {index:02d} {frame['pc']:#018x} {symbol(frame)}")
                source = frame.get("offline_addr2line", {}).get("output", "").splitlines()
                if len(source) > 1:
                    stacks.append("     " + source[1])
                if len(source) > 2:
                    stacks.append("     DWARF inline chain (shares this physical frame): " + " | ".join(source[2:]))
            stacks.append("")
    checks = {"probe_result": runtime["status"], "probe_pass_mask": summary["probe_pass_mask"],
              "first_hit_coverage_ok": summary["capture_ok"],
              "owned_qemu_stopped": runtime["owned_qemu_stopped"],
              "capture_count": len(rows), "scenarios": summary["scenarios"], "captures": rows,
              "all_entry_alignments_expected": bool(rows) and all(r["entry_alignment_mod16"] == 8 for r in rows),
              "all_physical_roots_recognized": bool(rows) and all(r["recognized_physical_root"] for r in rows),
              "scope": "First-hit physical snapshots at selected real dependency entries. Coverage, unwind completeness and absence of unexpected execution are separate claims; no exhaustive execution trace is inferred."}
    (args.output / "checks.json").write_text(json.dumps(checks, indent=2) + "\n")
    (args.output / "stacks.txt").write_text("\n".join(stacks) + "\n")
    mask_text = f"{summary['probe_pass_mask']:#x}" if summary['probe_pass_mask'] is not None else "unobserved"
    lines = ["# depthchecker_factchecker experiment", "",
             f"Probe result: **{runtime['status']}**, mask `{mask_text}`. "
             f"Captured {len(rows)} selected entries across {len(summary['scenarios'])} scenarios.", "",
             f"The exact probe Cargo plan has {cargo['all_build_units']} build units; "
             f"{cargo['ordinary_target_units']} ordinary target units are reachable through "
             f"{cargo['ordinary_target_edges']} edges, with maximum dependency depth {cargo['max_dependency_depth']}. "
             "These counts describe compilation, not a runtime call stack.", "",
             "| Scenario / entry | Physical frames | Entry RSP mod 16 | Termination |",
             "| --- | ---: | ---: | --- |"]
    lines.extend(f"| {r['scenario']} / `{r['top_function']}` | {r['physical_frames']} | {r['entry_alignment_mod16']} | {r['termination']} |" for r in rows)
    lines += ["", "Every capture retains register evidence, raw stack bytes with readable-range metadata, "
              "and the physical frame chain. The immediate caller at a pre-prologue stop is `[RSP]`. "
              "DWARF inline functions share a physical frame. Async continuations live on separate stacks.", "",
              "A recognized root establishes that this physical chain reached a known entry boundary. "
              "A frame limit, unreadable record, or invalid chain remains incomplete. Neither a passing "
              "probe nor a complete physical chain proves that every execution branch was observed.", "",
              f"Raw run: `{args.run.resolve()}`. Cargo evidence: `{args.raw.resolve()}`. "
              "Read `stacks.txt` for frames and `checks.json` for exact checks.", ""]
    (args.output / "overview.md").write_text("\n".join(lines))
    for root in (args.run, args.raw, args.output):
        for path in root.rglob("*"):
            if path.is_file() and path.name != "evidence-manifest.json" and path.suffix != ".sock":
                with path.open("rb") as stream:
                    files[str(path.resolve())] = {"bytes": path.stat().st_size,
                        "sha256": hashlib.file_digest(stream, "sha256").hexdigest()}
    (args.output / "evidence-manifest.json").write_text(json.dumps(files, indent=2) + "\n")
    print(json.dumps({k: v for k, v in checks.items() if k not in ("scenarios", "captures", "scope")}))


if __name__ == "__main__":
    main()
