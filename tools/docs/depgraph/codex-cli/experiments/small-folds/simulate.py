#!/usr/bin/env python3
"""Simulate source crate folds against the staged pack's exact unit graph.

Contraction preserves every incoming/outgoing dependency and rejects cycles.
It estimates graph shape and compilation boundaries, not executable size.
"""

import argparse
from collections import defaultdict, deque
import json
from pathlib import Path


CANDIDATES = {
    "pure-text-utilities": [
        "codex-utils-elapsed",
        "codex-utils-fuzzy-match",
        "codex-utils-stream-parser",
        "codex-utils-template",
    ],
    "path-primitives": [
        "codex-utils-absolute-path",
        "codex-utils-home-dir",
        "codex-utils-path",
        "codex-utils-path-uri",
    ],
    "http-transport-wide-attempt": [
        "codex-http-client",
        "codex-client",
        "codex-api",
        "codex-utils-rustls-provider",
    ],
    "http-transport-narrow": [
        "codex-http-client",
        "codex-client",
        "codex-utils-rustls-provider",
    ],
}


def reach(graph, roots):
    found = set()
    pending = list(roots)
    while pending:
        node = pending.pop()
        if node in found:
            continue
        found.add(node)
        pending.extend(graph.get(node, ()))
    return found


def stats(graph, roots):
    active = reach(graph, roots)
    graph = {node: set(graph[node]) & active for node in active}
    incoming = defaultdict(set)
    for node, deps in graph.items():
        for dep in deps:
            incoming[dep].add(node)
    pending = deque(node for node in active if not incoming[node])
    indegree = {node: len(incoming[node]) for node in active}
    longest = {node: 0 for node in pending}
    order = []
    while pending:
        node = pending.popleft()
        order.append(node)
        for dep in graph[node]:
            longest[dep] = max(longest.get(dep, 0), longest[node] + 1)
            indegree[dep] -= 1
            if indegree[dep] == 0:
                pending.append(dep)
    blocked = active - set(order)
    cyclic = []
    if blocked:
        state = {}
        stack = []

        def visit(node):
            state[node] = 1
            stack.append(node)
            for dep in sorted(graph[node] & blocked):
                if state.get(dep, 0) == 1:
                    return stack[stack.index(dep):] + [dep]
                if state.get(dep, 0) == 0:
                    cycle = visit(dep)
                    if cycle:
                        return cycle
            stack.pop()
            state[node] = 2
            return []

        for node in sorted(blocked):
            if state.get(node, 0) == 0:
                cyclic = visit(node)
                if cyclic:
                    break
    shortest = {node: 0 for node in roots}
    pending = deque(roots)
    while pending:
        node = pending.popleft()
        for dep in graph[node]:
            if dep not in shortest:
                shortest[dep] = shortest[node] + 1
                pending.append(dep)
    return {
        "nodes": len(active),
        "edges": sum(len(deps) for deps in graph.values()),
        "maximum_shortest_depth": max(shortest.values(), default=0),
        "maximum_longest_depth": None if cyclic else max(longest.values(), default=0),
        "cycle_nodes": cyclic,
    }


def graph_from_units(data):
    units = data["units"]
    # Unit dependencies with noprelude are automatically injected build-std
    # edges, not source dependencies. Host proc macros/build scripts have no
    # runtime crate node. Keep all source target lib/bin variants separately.
    eligible = {
        index for index, unit in enumerate(units)
        if unit["platform"] is not None
        and unit["mode"] == "build"
        and any(kind in ("lib", "bin") for kind in unit["target"]["kind"])
        and not unit["pkg_id"].startswith("path+file:///home/t4ce/.rustup/")
    }
    graph = {
        str(index): {
            str(dep["index"]) for dep in units[index]["dependencies"]
            if dep["index"] in eligible and not dep.get("noprelude", False)
        }
        for index in eligible
    }
    roots = [str(index) for index in data["roots"] if index in eligible]
    active = reach(graph, roots)
    graph = {node: graph[node] for node in active}
    return graph, roots


def package_name(unit):
    location, fragment = unit["pkg_id"].rsplit("#", 1)
    return fragment.rsplit("@", 1)[0] if "@" in fragment else location.rstrip("/").rsplit("/", 1)[-1]


def contract(graph, members, replacement):
    folded = {}
    for node, deps in graph.items():
        mapped_node = replacement if node in members else node
        mapped_deps = {replacement if dep in members else dep for dep in deps}
        mapped_deps.discard(mapped_node)
        folded.setdefault(mapped_node, set()).update(mapped_deps)
    return folded


def describe(unit, index):
    return {
        "unit_index": index,
        "package": package_name(unit),
        "package_id": unit["pkg_id"],
        "crate": unit["target"]["name"],
        "features": unit["features"],
        "source": unit["target"]["src_path"],
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--unit-graph", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    data = json.loads(args.unit_graph.read_text())
    graph, roots = graph_from_units(data)
    units = data["units"]
    baseline = stats(graph, roots)
    incoming = defaultdict(set)
    for node, deps in graph.items():
        for dep in deps:
            incoming[dep].add(node)
    records = []
    for name, names in CANDIDATES.items():
        members = {node for node in graph if package_name(units[int(node)]) in names}
        actual_names = {package_name(units[int(node)]) for node in members}
        if actual_names != set(names):
            raise ValueError(f"{name}: requested source crates absent from active units: {set(names) - actual_names}")
        if len(members) != len(names):
            raise ValueError(f"{name}: multiple target feature/profile variants need separate ABI decisions")
        folded = contract(graph, members, name)
        after = stats(folded, roots)
        external_consumers = set.union(*(incoming[node] for node in members)) - members
        external_dependencies = set.union(*(graph[node] for node in members)) - members
        impacted = reach({node: incoming[node] for node in graph}, members) - members
        source_rows = []
        for node in sorted(members, key=lambda n: package_name(units[int(n)])):
            unit = units[int(node)]
            source_dir = Path(unit["target"]["src_path"]).parent
            files = list(source_dir.rglob("*.rs"))
            source_rows.append({
                **describe(unit, int(node)),
                "rust_source_files": len(files),
                "rust_source_bytes": sum(path.stat().st_size for path in files),
                "rust_source_lines": sum(len(path.read_text().splitlines()) for path in files),
            })
        cycle_names = [name if n == name else package_name(units[int(n)]) for n in after["cycle_nodes"]]
        records.append({
            "candidate": name,
            "status": "rejected-cycle" if cycle_names else "acyclic-projection",
            "before": baseline,
            "after": after,
            "compile_unit_reduction": len(members) - 1,
            "internal_edges_removed": sum(len(graph[node] & members) for node in members),
            "affected_external_consumer_units": len(external_consumers),
            "affected_external_consumer_packages": len({units[int(node)]["pkg_id"] for node in external_consumers}),
            "external_dependency_units_after_union": len(external_dependencies),
            "reverse_rebuild_closure_units": len(impacted),
            "source_crates": source_rows,
            "consumers": [describe(units[int(n)], int(n)) for n in sorted(external_consumers, key=int)],
            "dependencies": [describe(units[int(n)], int(n)) for n in sorted(external_dependencies, key=int)],
            "cycle_package_names": cycle_names,
            "projected_edges": {node: sorted(deps) for node, deps in sorted(folded.items())},
            "bp_byte_savings": None,
        })
    report = {
        "schema_version": 1,
        "input": str(args.unit_graph.resolve()),
        "scope": "Exact active target source lib/bin compilation units; injected build-std edges, host proc macros, build scripts excluded.",
        "baseline": baseline,
        "candidates": records,
    }
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"baseline": baseline, "candidates": [{k: v for k, v in record.items() if k in ("candidate", "status", "after", "compile_unit_reduction", "internal_edges_removed", "affected_external_consumer_units", "external_dependency_units_after_union", "reverse_rebuild_closure_units")} for record in records]}, indent=2))


if __name__ == "__main__":
    main()
