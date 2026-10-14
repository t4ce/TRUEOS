#!/usr/bin/env python3
"""Try a semantically preserving native network-owner archive fold.

Uses the exact target Cargo unit graph and newest root .rlink inventory. This
does not change Cargo manifests, source, shared caches, or published apps.
"""
from collections import Counter, defaultdict
import argparse
import hashlib
import json
import mmap
from pathlib import Path
import re
import struct
import subprocess
import tempfile
import time

HERE = Path(__file__).resolve().parent
GRAPH = HERE.parents[1]
REPOS = GRAPH.parents[4]
TOOLBIN = REPOS / "TRUEOS-Rust-Toolchain-nightly-2026-07-10/lib/rustlib/x86_64-unknown-linux-gnu/bin"
WORK = REPOS / "codex/codex-rs/cli/target/trueos-blueprint/work/codex"


def reachable(seeds, edges, blocked=frozenset()):
    seen, todo = set(), list(seeds)
    while todo:
        node = todo.pop()
        if node in seen or node in blocked:
            continue
        seen.add(node)
        todo.extend(edges.get(node, ()))
    return seen


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def fold_graph():
    plan = json.loads((GRAPH / "raw/unit-graph.json").read_text())
    packages = {p["id"]: p for p in json.loads((GRAPH / "raw/metadata.json").read_text())["packages"]}
    units = plan["units"]
    selected = {i for i, unit in enumerate(units) if unit["platform"] and unit["mode"] == "build"
                and "custom-build" not in unit["target"]["kind"] and "proc-macro" not in unit["target"]["kind"]}
    edges = {i: {d["index"] for d in units[i]["dependencies"]
                 if d["index"] in selected and not d["noprelude"]} for i in selected}
    roots = plan["roots"]
    active = reachable(roots, edges)
    owners = {i for i in active if packages.get(units[i]["pkg_id"], {}).get("name") == "codex-network-proxy"}
    assert len(owners) == 1, owners
    owner = next(iter(owners))
    exclusive = active - reachable(roots, edges, owners)
    inclusive = reachable(owners, edges)
    # All inbound edges must enter the owner. Folding an inclusive subtree
    # would falsely swallow shared libraries and can introduce cycles.
    incoming = {(i, j) for i in active - exclusive for j in edges[i] if j in exclusive}
    assert all(j == owner for _, j in incoming)
    outgoing = {(i, j) for i in exclusive for j in edges[i] if j not in exclusive}
    projected = {(owner if i in exclusive else i, owner if j in exclusive else j)
                 for i in active for j in edges[i] if not (i in exclusive and j in exclusive)}
    root_package = units[roots[0]]["pkg_id"]
    root_units = {i for i in selected if units[i]["pkg_id"] == root_package}
    helper_names = ["codex_app_server", "codex_app_server_daemon", "codex_exec_server", "codex_tcp_tunnel",
                    "codex_cloud_tasks", "codex_responses_api_proxy", "codex_app_server_test_client", "codex_stdio_to_uds"]
    cuts, helper_rows = set(), []
    for name in helper_names:
        single_cut = {(i, j) for i in root_units for j in edges[i] if units[j]["target"]["name"] == name}
        cuts |= single_cut
        cut_edges = {i: {j for j in children if (i, j) not in single_cut} for i, children in edges.items()}
        lost = active - reachable(roots, cut_edges)
        helper_rows.append({"cli_package_edge_to": name, "removed_units": len(lost),
                            "removed_crates": [units[i]["target"]["name"] for i in sorted(lost)]})
    combined_edges = {i: {j for j in children if (i, j) not in cuts} for i, children in edges.items()}
    combined_lost = active - reachable(roots, combined_edges)
    (HERE / "command-plane-rejection.json").write_text(json.dumps({
        "scope": "Exact captured target units; cuts remove selected dependencies from both codex-cli bin/lib Cargo units. Simulation only, no feature/source changed.",
        "baseline_target_units": len(active), "single_cuts": helper_rows,
        "combined_removed_units": len(combined_lost),
        "combined_removed_crates": [units[i]["target"]["name"] for i in sorted(combined_lost)]}, indent=2) + "\n")
    def label(i):
        unit = units[i]
        pkg = packages.get(unit["pkg_id"], {})
        return {"unit_index": i, "package_id": unit["pkg_id"], "package_name": pkg.get("name", unit["target"]["name"]),
                "crate_name": unit["target"]["name"], "source_path": unit["target"]["src_path"], "features": unit["features"]}
    result = {"method": "Dominator ownership: nodes unreachable after blocking codex-network-proxy. Implicit noprelude sysroot edges are omitted.",
              "source_sha256": {str(path.relative_to(GRAPH)): digest(path) for path in [GRAPH / "raw/unit-graph.json", GRAPH / "raw/metadata.json", GRAPH / "raw/rlink-closure.json"]},
              "active_units": len(active), "active_packages": len({units[i]["pkg_id"] for i in active}),
              "inclusive_units": len(inclusive), "exclusive_units": len(exclusive), "shared_units": len(inclusive - exclusive),
              "members": [label(i) for i in sorted(exclusive)],
              "incoming": [{"from": label(i), "to": label(j)} for i, j in sorted(incoming)],
              "outgoing": [{"from": label(i), "to": label(j)} for i, j in sorted(outgoing)],
              "source_graph_edges": sum(len(edges[i]) for i in active),
              "folded_units": len(active) - len(exclusive) + 1,
              "folded_edges": len(projected), "external_dependency_units": len({j for _, j in outgoing}),
              "status": "graph ownership established; archive experiment pending"}
    return result


def native_archive_members(path):
    """Read GNU/BSD ar members and retain only native ELF object bytes."""
    data = path.read_bytes()
    assert data.startswith(b"!<arch>\n"), path
    offset, names, members = 8, b"", []
    while offset < len(data):
        header = data[offset:offset + 60]
        assert len(header) == 60 and header[58:60] == b"`\n", path
        size = int(header[48:58])
        body = data[offset + 60:offset + 60 + size]
        name = header[:16].decode().strip()
        if name == "//":
            names = body
        elif name.startswith("#1/"):
            length = int(name[3:])
            name, body = body[:length].decode().rstrip("\0"), body[length:]
        elif name.startswith("/") and name[1:].isdigit():
            start = int(name[1:])
            name = names[start:names.index(b"\n", start)].decode().rstrip("/")
        else:
            name = name.rstrip("/")
        if body.startswith(b"\x7fELF"):
            members.append((name, body))
        elif name.endswith(".o"):
            raise ValueError(f"Non-ELF object cannot be silently folded: {path}({name})")
        offset += 60 + size + size % 2
    return members


def elf_evidence(path):
    """Order-independent allocated-section evidence; do not claim byte savings from archive size."""
    with path.open("rb") as stream, mmap.mmap(stream.fileno(), 0, access=mmap.ACCESS_READ) as data:
        assert data[:6] == b"\x7fELF\x02\x01", path
        header = struct.unpack_from("<16sHHIQQQIHHHHHH", data)
        shoff, shent, count, strings = header[6], header[11], header[12], header[13]
        first = struct.unpack_from("<IIQQQQIIQQ", data, shoff)
        if count == 0:
            count = first[5]
        if strings == 0xffff:
            strings = first[6]
        headers = [struct.unpack_from("<IIQQQQIIQQ", data, shoff + i * shent) for i in range(count)]
        string_header = headers[strings]
        string_data = data[string_header[4]:string_header[4] + string_header[5]]
        allocated = Counter()
        totals = Counter()
        for section in headers:
            name_offset, kind, flags, _, offset, size, _, _, align, _ = section
            if not flags & 2:
                continue
            name = string_data[name_offset:string_data.find(b"\0", name_offset)].decode(errors="replace")
            payload_hash = "NOBITS" if kind == 8 else hashlib.sha256(data[offset:offset + size]).hexdigest()
            allocated[(name, kind, flags, size, align, payload_hash)] += 1
            totals["allocated_bytes"] += size
            totals["nobits_bytes" if kind == 8 else "file_allocated_bytes"] += size
        canon = sorted([list(key) + [value] for key, value in allocated.items()])
        return {"file_bytes": path.stat().st_size, "section_count": count,
                "allocated_section_count": sum(allocated.values()), **dict(totals),
                "allocated_multiset_sha256": hashlib.sha256(json.dumps(canon, separators=(",", ":")).encode()).hexdigest()}, allocated


def run_link(result):
    inventory = json.loads((GRAPH / "raw/rlink-closure.json").read_text())
    all_archives = [Path(row["path"]) for row in inventory.get("archives", inventory.get("rlibs", []))]
    selected, not_in_rlink = [], []
    for member in result["members"]:
        matches = [p for p in all_archives if re.fullmatch(r"lib" + re.escape(member["crate_name"]) + r"-[^.]+\.rlib", p.name)]
        if not matches:
            not_in_rlink.append(member)
            continue
        if len(matches) > 1:
            # Versioned duplicate crates share a stem. Authenticate their
            # actual source path from Cargo's native dep-info, not filename.
            matches = [p for p in matches if member["source_path"] in
                       p.with_name(p.name.removeprefix("lib").removesuffix(".rlib") + ".d").read_text()]
        assert len(matches) == 1, (member, matches)
        selected.append(matches[0])
    # Preserve the exact .rlink archive order inside the bundle. The bundle
    # itself replaces its first original member in the enclosing link group.
    selected_set = set(selected)
    selected = [p for p in all_archives if p in selected_set]
    before_inputs = {str(path): digest(path) for path in [WORK / "app.rooted.o", WORK / "module.o", *all_archives]}
    readelf = subprocess.check_output(["readelf", "-Ws", str(WORK / "app.rooted.o")], text=True)
    symbols = [line.split()[-1] for line in readelf.splitlines() if len(line.split()) >= 8 and line.split()[3] == "FUNC"]
    entry = next((s for s in symbols if s == "_start"), None) or next((s for s in symbols if s == "main"), None)
    if not entry:
        entry = min([s for s in symbols if (s.startswith("_R") and s.endswith("4main")) or (s.startswith("_ZN") and "4main17h" in s and s.endswith("E"))], key=len)
    with tempfile.TemporaryDirectory(prefix="codex-network-fold-", dir=HERE) as temporary:
        temp = Path(temporary)
        objects, object_bytes = [], 0
        for number, archive in enumerate(selected):
            members = native_archive_members(archive)
            assert members, archive
            for name, data in members:
                path = temp / (str(number) + "-" + name)
                path.write_bytes(data)
                objects.append(path)
                object_bytes += len(data)
        bundle = temp / "network-owner.a"
        subprocess.run([str(TOOLBIN / "llvm-ar"), "rcs", str(bundle), *map(str, objects)], check=True)
        folded = temp / "module.network-fold.o"
        arguments = [str(TOOLBIN / "rust-lld"), "-flavor", "gnu", "-r", "--gc-sections", "-o", str(folded),
                     "--undefined", entry, str(WORK / "app.rooted.o"), "--start-group"]
        inserted = False
        for archive in all_archives:
            if archive in selected:
                if not inserted:
                    arguments.append(str(bundle))
                    inserted = True
            else:
                arguments.append(str(archive))
        arguments.append("--end-group")
        result["link_command"] = arguments
        started = time.monotonic()
        subprocess.run(arguments, check=True, stdout=subprocess.DEVNULL, stderr=(HERE / "link.stderr").open("wb"))
        result["link_seconds"] = round(time.monotonic() - started, 3)
        baseline_info, baseline_sections = elf_evidence(WORK / "module.o")
        folded_info, folded_sections = elf_evidence(folded)
        nm = TOOLBIN / "llvm-nm"
        imports = lambda path: sorted(subprocess.check_output([str(nm), "--undefined-only", str(path)], text=True).splitlines())
        baseline_imports, folded_imports = imports(WORK / "module.o"), imports(folded)
        (HERE / "undefined-symbols.txt").write_text("\n".join(baseline_imports) + "\n")
        def section_changes(sections):
            return [{"name": key[0], "kind": key[1], "flags": key[2], "bytes": key[3], "align": key[4], "sha256": key[5], "copies": value}
                    for key, value in sorted(sections.items())]
        result.update({"native_archive_fold": {"selected_archives": [str(p) for p in selected], "archive_count": len(selected),
                                               "selected_archive_bytes": sum(p.stat().st_size for p in selected),
                                               "native_object_count": len(objects), "native_object_bytes": object_bytes,
                                               "bundle_bytes": bundle.stat().st_size, "planned_units_not_in_native_rlink": not_in_rlink},
                       "baseline_elf": baseline_info, "folded_elf": folded_info,
                       "same_allocated_sections": baseline_sections == folded_sections,
                       "same_undefined_symbols": baseline_imports == folded_imports,
                       "baseline_only_sections": section_changes(baseline_sections - folded_sections),
                       "folded_only_sections": section_changes(folded_sections - baseline_sections),
                       "native_inputs_unchanged": before_inputs == {str(p): digest(Path(p)) for p in before_inputs},
                       "status": "linked isolated native archive fold; zero allocated-byte reduction; byte identity and runtime behavior unverified",
                       "temporary_binary_outputs_removed": True})
        # The evidence retained in JSON is sufficient; large scratch objects
        # are deliberately not kept in the documentation tree.
    return result


def main():
    global GRAPH, WORK
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--link", action="store_true", help="perform isolated native archive fold and relink; never compiles or publishes")
    historical = GRAPH / "implementation/before"
    parser.add_argument("--graph-dir", type=Path, default=historical if historical.exists() else GRAPH)
    parser.add_argument("--work", type=Path, help="original pre-migration module directory for historical native relinking")
    parser.add_argument("--output", type=Path, help="new experiment result; original recorded result.json is retained")
    args = parser.parse_args()
    GRAPH = args.graph_dir.resolve()
    if args.work:
        WORK = args.work.resolve()
    if args.link and GRAPH == historical.resolve():
        recorded = json.loads((HERE / "result.json").read_text())
        if (WORK / "module.o").stat().st_size != recorded["baseline_elf"]["file_bytes"]:
            parser.error("historical native relink requires --work with the original module; validate the applied pack with implementation/validate_pack.py")
    result = fold_graph()
    if args.link:
        result = run_link(result)
    output = args.output or HERE / ("link-result.json" if args.link else "simulation-result.json")
    output.write_text(json.dumps(result, indent=2) + "\n")
    (HERE / "simulation-folds.json").write_text(json.dumps([{"id": "network-owner", "title": "Network policy and proxy ownership (moonshot)",
                                                  "package_names": [row["package_name"] for row in result["members"]],
                                                  "package_ids": sorted({row["package_id"] for row in result["members"]}),
                                                  "status": result["status"],
                                                  "note": "Graph preview only: 45 exact package IDs/target units collapse to one visible owner; shared TLS/HTTP/Tokio crates and duplicate package versions stay outside. The isolated archive attempt saved zero allocated bytes; runtime behavior remains unverified."}], indent=2) + "\n")
    print(json.dumps({k: result[k] for k in ["active_units", "exclusive_units", "shared_units", "folded_units", "status"]}, indent=2))
    if args.link:
        print(json.dumps({k: result[k] for k in ["link_seconds", "same_allocated_sections", "same_undefined_symbols", "native_inputs_unchanged", "baseline_elf", "folded_elf"]}, indent=2))


if __name__ == "__main__":
    main()
