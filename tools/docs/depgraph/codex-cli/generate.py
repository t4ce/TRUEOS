#!/usr/bin/env python3
"""Derive a depth graph and ownership metrics from captured Cargo build units."""
from collections import Counter, defaultdict, deque
import csv
import json
from pathlib import Path


HERE = Path(__file__).resolve().parent


def reachable(root, edges, blocked=frozenset()):
    seen, pending = set(), [root]
    while pending:
        node = pending.pop()
        if node in seen or node in blocked:
            continue
        seen.add(node)
        pending.extend(edges.get(node, ()))
    return seen


def main():
    metadata = json.loads((HERE / "raw/metadata.json").read_text())
    plan = json.loads((HERE / "raw/unit-graph.json").read_text())
    packages = {p["id"]: p for p in metadata["packages"]}
    units = plan["units"]
    # Build-script executables and proc macros execute on the host. Target
    # build-script runners also are not linked Rust libraries.
    selected = {i for i, unit in enumerate(units) if unit["platform"] and unit["mode"] == "build"
                and "custom-build" not in unit["target"]["kind"] and "proc-macro" not in unit["target"]["kind"]}
    unit_edges = {i: {dep["index"] for dep in units[i]["dependencies"]
                      if dep["index"] in selected and not dep["noprelude"]} for i in selected}
    active_units = set().union(*(reachable(i, unit_edges) for i in plan["roots"]))
    grouped = defaultdict(list)
    for index in sorted(selected):
        grouped[units[index]["pkg_id"]].append(index)
    ids = {pkg: str(i) for i, pkg in enumerate(sorted(grouped))}
    root = ids[units[plan["roots"][0]]["pkg_id"]]
    edges = defaultdict(set)
    implicit = set()
    for index in selected:
        parent = ids[units[index]["pkg_id"]]
        for dep in units[index]["dependencies"]:
            if dep["index"] not in selected:
                continue
            child = ids[units[dep["index"]]["pkg_id"]]
            if parent == child:
                continue
            if dep["noprelude"]:
                implicit.add((parent, child))
            elif index in active_units:
                # Reach units before merging package identities. Merging
                # build-std and app variants first invents cross-variant paths.
                edges[parent].add(child)
    active = reachable(root, edges)
    parents = defaultdict(set)
    for parent, children in edges.items():
        for child in children:
            parents[child].add(parent)
    depths, pending = {root: 0}, deque([root])
    while pending:
        parent = pending.popleft()
        for child in sorted(edges[parent]):
            if child not in depths:
                depths[child] = depths[parent] + 1
                pending.append(child)
    memo, visiting = {}, set()
    def height(node):
        if node in memo:
            return memo[node]
        if node in visiting:
            raise ValueError("cycle in target package graph")
        visiting.add(node)
        memo[node] = max((height(child) + 1 for child in edges[node]), default=0)
        visiting.remove(node)
        return memo[node]
    nodes = []
    closures = {node: reachable(node, edges) for node in active}
    direct = sorted(edges[root])
    ownership = Counter(node for branch in direct for node in closures[branch])
    inventory_path = HERE / "raw/rlink-closure.json"
    inventory = json.loads(inventory_path.read_text()) if inventory_path.exists() else {}
    archives = inventory.get("rlibs", inventory.get("archives", []))
    native_fold_path = HERE / "implementation/native-folds.json"
    native_fold = json.loads(native_fold_path.read_text()) if native_fold_path.exists() else None
    authenticated_packages = {archive["path"]: archive["package_id"] for archive in native_fold["archives"]} if native_fold else {}
    by_stem = defaultdict(list)
    for archive in archives:
        path = archive["path"] if isinstance(archive, dict) else archive
        stem = Path(path).name.removeprefix("lib").rsplit("-", 1)[0]
        # Same crate stem can cover multiple versions. A .d source path ties
        # each selected archive to its actual Cargo package directory.
        depfile = Path(path).with_name(Path(path).name.removeprefix("lib").removesuffix(".rlib") + ".d")
        deptext = depfile.read_text(errors="replace") if depfile.exists() else ""
        by_stem[stem].append((archive, deptext))
    for pkg, indices in grouped.items():
        key = ids[pkg]
        p = packages.get(pkg, {})
        first = units[indices[0]]
        name = p.get("name", first["target"]["name"].replace("_", "-"))
        stems = {units[i]["target"]["name"] for i in indices}
        directory = str(Path(p.get("manifest_path", first["target"]["src_path"])).parent)
        mapped = [a for stem in stems for a, deptext in by_stem[stem]
                  if authenticated_packages.get(a["path"]) == pkg
                  or (a["path"] not in authenticated_packages and directory + "/" in deptext)]
        if pkg == units[plan["roots"][0]]["pkg_id"]:
            # The standalone root .d records src/... relative to its manifest.
            mapped.extend(a for stem in stems for a, deptext in by_stem[stem]
                          if a not in mapped and "env-dep:CARGO_PKG_NAME=codex-cli" in deptext)
        visible_indices = [i for i in indices if i in active_units]
        feature_sets = sorted({tuple(units[i]["features"]) for i in (visible_indices or indices)})
        src = p.get("manifest_path", first["target"]["src_path"])
        source = "registry" if p.get("source", "") and p["source"].startswith("registry") else "git" if p.get("source", "") else "path"
        fallback_version = pkg.split("#")[-1].split("@")[-1]
        nodes.append({"id": key, "package_id": pkg, "name": name, "version": p.get("version", fallback_version),
                      "manifest_path": src, "source": source, "codex": name.startswith("codex-"),
                      "sysroot_only": key not in active, "unit_indices": indices, "feature_sets": feature_sets,
                      "ordinary_unit_indices": visible_indices,
                      "depth": depths.get(key), "height": height(key), "children": sorted(edges[key]),
                      "parents": sorted(parents[key]), "closure": len(closures.get(key, reachable(key, edges))),
                      "direct_root_owners": ownership[key], "rlink_archives": mapped})
    index = {n["id"]: n for n in nodes}
    branches = []
    for branch in direct:
        # Cutting only this root edge preserves shared consumers, unlike
        # deleting the package globally.
        cut_edges = dict(edges)
        cut_edges[root] = edges[root] - {branch}
        removed = active - reachable(root, cut_edges)
        branches.append({"id": branch, "name": index[branch]["name"], "closure": len(closures[branch]),
                         "exclusive_root_edge_cut": len(removed), "removed": sorted(removed)})
    branches.sort(key=lambda row: (-row["exclusive_root_edge_cut"], -row["closure"], row["name"]))
    roles = Counter(("target" if u["platform"] else "host") + ":" + u["mode"] + ":" + ",".join(u["target"]["kind"]) for u in units)
    graph = {"root": root, "nodes": sorted(nodes, key=lambda n: (n["depth"] is None, n["depth"] or 0, n["name"], n["version"])),
             "branches": branches, "implicit_sysroot_edges": sorted(implicit),
             "summary": {"metadata_packages": len(packages), "build_units": len(units), "unit_roles": dict(roles),
                         "target_packages": len(grouped), "reachable_packages": len(active), "codex_packages": sum(index[i]["codex"] for i in active),
                         "reachable_ordinary_target_units": len(active_units),
                         "visible_edges": sum(len(edges[i]) for i in active), "max_shortest_depth": max(depths.values()),
                         "longest_path": height(root), "depth_histogram": dict(sorted(Counter(depths.values()).items())),
                         "root_direct_dependencies": len(direct)},
             "notes": ["Traverse ordinary target build-unit dependencies before merging package identities. Same package bin/lib units merge; host, build scripts and implicit build-std units remain in raw/unit-graph.json.",
                       "Implicit noprelude build-std edges are separated to avoid a std/core edge on every node. Other target dependencies are retained.",
                       "Each shared package appears once. Root edge cut counts are exact reachability under that graph cut, not packed byte savings.",
                       "Rlink selects archives; archive bytes include metadata and dead code. Actual BP payload requires relinking and loader validation.",
                       "Cargo unit profile panic field precedes the packer's explicit -C panic=abort. Rlink closure establishes the selected panic runtime."]}
    graph["folds"] = []
    package_ids = set(grouped)
    package_names = {node["name"] for node in nodes if not node["sysroot_only"]}
    for path in sorted((HERE / "experiments").glob("*/folds.json")):
        for fold in json.loads(path.read_text()):
            required = set(fold.get("package_ids", fold["package_names"]))
            available = package_ids if "package_ids" in fold else package_names
            # Historical projections are retained in implementation/before.
            # Removed source crates cannot be offered as an empty live fold.
            if required <= available:
                graph["folds"].append(fold)
    if native_fold:
        members = [member["pkg_id"] for member in native_fold["members"]]
        if set(members) <= package_ids:
            graph["folds"] = [fold for fold in graph["folds"] if fold["id"] != "network-owner"]
            graph["folds"].insert(0, {"id": "network-owner", "title": "Native network ownership", "package_ids": members,
                                      "package_names": [node["name"] for node in nodes if node["package_id"] in members],
                                      "status": "implemented-response-fold", "note": native_fold["method"]})
    critical = [root]
    while edges[critical[-1]]:
        critical.append(max(sorted(edges[critical[-1]]), key=height))
    graph["critical_package_path"] = [index[i]["name"] + "@" + index[i]["version"] for i in critical]
    graph["summary"]["selected_native_archives"] = inventory.get("archive_count")
    graph["summary"]["mapped_native_archives"] = sum(len(n["rlink_archives"]) for n in nodes)
    (HERE / "graph.json").write_text(json.dumps(graph, indent=2) + "\n")
    with (HERE / "packages.tsv").open("w") as file:
        writer = csv.writer(file, delimiter="\t", lineterminator="\n")
        writer.writerow(["name", "version", "shortest_depth", "longest_remaining_depth", "children", "parents", "closure_packages", "root_branch_owners", "source", "package_id"])
        for n in graph["nodes"]:
            writer.writerow([n["name"], n["version"], n["depth"], n["height"], len(n["children"]), len(n["parents"]), n["closure"], n["direct_root_owners"], n["source"], n["package_id"]])
    lines = ["codex-cli: target package graph from the staged TRUEOS pack", json.dumps(graph["summary"], indent=2), "", "Root branch / closure / exclusively freed by cutting only its root edge"]
    lines += [f"{b['name']:42} {b['closure']:5} {b['exclusive_root_edge_cut']:5}" for b in branches]
    lines += ["", "One longest package path:", " -> ".join(graph["critical_package_path"])]
    (HERE / "depth-overview.txt").write_text("\n".join(lines) + "\n")
    template = (HERE / "viewer.html").read_text()
    (HERE / "index.html").write_text(template.replace("/*GRAPH_DATA*/null", json.dumps(graph).replace("</", "<\\/")))
    print(json.dumps(graph["summary"], indent=2))


if __name__ == "__main__":
    main()
