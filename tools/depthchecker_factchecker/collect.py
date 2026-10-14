#!/usr/bin/env python3
"""Save the exact packer's unit/link evidence and a plain Cargo depth overview."""
import argparse
from collections import deque
import hashlib
import json
from pathlib import Path
import shutil


def overview(graph):
    units = graph["units"]
    eligible = {i for i, u in enumerate(units) if u["platform"] and u["mode"] == "build"
                and not set(u["target"]["kind"]) & {"custom-build", "proc-macro"}}
    adjacency = {i: [d["index"] for d in u["dependencies"]
                     if not d.get("noprelude") and d["index"] in eligible]
                 for i, u in enumerate(units) if i in eligible}
    roots = [i for i in graph["roots"] if i in eligible]
    shortest, queue = {i: 0 for i in roots}, deque(roots)
    while queue:
        parent = queue.popleft()
        for child in adjacency[parent]:
            if child not in shortest:
                shortest[child] = shortest[parent] + 1
                queue.append(child)
    memo, visiting = {}, set()

    def depth(index):
        if index in visiting:
            raise ValueError("Cargo unit graph unexpectedly cycles")
        if index not in memo:
            visiting.add(index)
            memo[index] = 1 + max((depth(c) for c in adjacency[index]), default=-1)
            visiting.remove(index)
        return memo[index]

    result = {"all_build_units": len(units), "ordinary_target_units": len(shortest),
              "ordinary_target_edges": sum(len(adjacency[i]) for i in shortest),
              "max_dependency_depth": max(map(depth, roots), default=0), "roots": roots,
              "scope": "Target build units reachable through ordinary extern edges; excludes implicit sysroot, proc macros and build scripts. Compilation is not runtime execution."}
    lines = [result["scope"], json.dumps({k: v for k, v in result.items() if k != "scope"}), "",
             "unit\tshortest_from_root\tlongest_to_leaf\tcrate\tpackage_id\tfeatures\tdependencies"]
    for index in sorted(shortest, key=lambda i: (shortest[i], units[i]["target"]["name"], i)):
        unit = units[index]
        lines.append("\t".join((str(index), str(shortest[index]), str(depth(index)),
                      unit["target"]["name"], unit["pkg_id"], ",".join(unit["features"]),
                      ",".join(map(str, adjacency[index])))))
    return result, "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    files = {}
    for name in ("native-unit-graph.json", "native-link-inputs.json"):
        source = args.work / name
        shutil.copy2(source, args.output / name)
        files[name] = hashlib.sha256(source.read_bytes()).hexdigest()
    result, table = overview(json.loads((args.output / "native-unit-graph.json").read_text()))
    result["raw_sha256"] = files
    (args.output / "cargo-depth.tsv").write_text(table)
    (args.output / "cargo-depth.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
