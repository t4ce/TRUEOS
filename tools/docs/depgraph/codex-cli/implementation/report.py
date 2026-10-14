#!/usr/bin/env python3
"""Summarize measured changes between the preserved baseline and actual pack."""
import json
from pathlib import Path


HERE = Path(__file__).resolve().parent


def main():
    before = json.loads((HERE / "before/graph.json").read_text())
    after = json.loads((HERE.parent / "graph.json").read_text())
    validation = json.loads((HERE / "pack-validation.json").read_text())
    prior_elf = json.loads((HERE.parent / "experiments/coalesce/results.json").read_text())["baseline"]
    prior_capture = json.loads((HERE / "before/raw/capture.json").read_text())
    before_bp = next(row["bytes"] for path, row in prior_capture["existing_artifacts"].items() if path.endswith("/dist/codex.bp"))
    metrics = {}
    for key in ["metadata_packages", "build_units", "reachable_packages", "codex_packages", "reachable_ordinary_target_units",
                "visible_edges", "longest_path", "selected_native_archives"]:
        metrics[key] = {"before": before["summary"][key], "after": after["summary"][key],
                        "delta": after["summary"][key] - before["summary"][key]}
    metrics["blueprint_bytes"] = {"before": before_bp, "after": validation["blueprint"]["bytes"],
                                  "saved": before_bp - validation["blueprint"]["bytes"],
                                  "saved_percent": round(100 * (before_bp - validation["blueprint"]["bytes"]) / before_bp, 3)}
    metrics["elf_sections"] = {"before": prior_elf["section_count"], "after": validation["elf"]["section_count"]}
    metrics["stripped_elf_bytes"] = {"before": prior_elf["file_bytes"], "after": validation["elf"]["file_bytes"]}
    result = {"metrics": metrics,
              "source_folds": {"original_packages": 13, "replacement_packages": ["codex-utils-text", "codex-utils-paths", "codex-transport", "codex-network-foundation"],
                               "cycle_resolution": "Shared error, retry, revocation and TLS-provider definitions live below protocol and transport in the independent foundation crate."},
              "tests": {"utility_text": 54, "utility_paths": 124, "transport_foundation_protocol": 720, "packer": 94, "total": 992,
                        "rustfmt_workspace_packages": 151, "bazel": "Static source/label/fixture audit; executable unavailable."},
              "actual_pack": validation["blueprint"], "native_fold": validation["native_fold"],
              "scope": "Actual integrated local TRUEOS package; original functionality retained, no command feature cuts.",
              "attribution": "Baseline-to-final measurements include the contemporaneous std backend/cache revision change. The prior same-input experiment independently established 16.4% with coalescing and 17.8% with additional local-symbol cleanup; source fold sizes are not inferred from archive input sizes.",
              "deployment": "Local dist/codex.bp rebuilt with apps publication disabled; no rig execution."}
    (HERE / "results.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(metrics, indent=2))


if __name__ == "__main__":
    main()
