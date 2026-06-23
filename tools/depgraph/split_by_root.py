#!/usr/bin/env python3
from __future__ import annotations

import re
import subprocess
import json
from html import escape
from collections import defaultdict
from pathlib import Path


TREE_PATH = Path("tools/depgraph/trueos-depth-tree.txt")
FULL_DOT_PATH = Path("tools/depgraph/trueos-depth-tree.dot")
FULL_SVG_PATH = Path("tools/depgraph/trueos-depth-tree.svg")
OUT_DIR = Path("tools/depgraph/by-root")
HTML_INDEX_PATH = Path("tools/depgraph/index.html")
REPO_ROOT = "/home/t4ce/REPOS/TRUEOS"
LEFT_WING_CRATES = {
    "core3",
    "mio",
    "regex-automata",
    "socket2",
    "tower",
    "trueos-esp",
    "trueos-io",
    "trueos-lsd",
    "trueos-vm",
    "v",
    "zune-core",
    "zune-jpeg",
}


def canonical_label(label: str) -> str:
    return re.sub(r" \(\*\)$", "", label.strip())


def dot_escape(value: str) -> str:
    return value.replace("\\", "\\\\").replace("\\\\n", "\\n").replace('"', '\\"')


def relative_source(source: str) -> str:
    if source == REPO_ROOT:
        return "/"
    if source.startswith(f"{REPO_ROOT}/"):
        return "/" + source.removeprefix(f"{REPO_ROOT}/")
    if source.startswith(("http://", "https://")) and "#" in source:
        return "#" + source.rsplit("#", 1)[1]
    return source


def display_lines(label: str) -> list[tuple[str, bool]]:
    match = re.match(r"^([^ ]+) v([^ ]+)(.*)$", label)
    if not match:
        return [(relative_source(label), True)]

    name, version, rest = match.groups()
    lines = [(name, True), (version, False)]
    for part in re.findall(r"\(([^()]*)\)", rest):
        lines.append((relative_source(part), False))
    return lines


def display_label(label: str) -> str:
    return r"\n".join(text for text, _ in display_lines(label))


def crate_name(label: str) -> str:
    match = re.match(r"^([^ ]+) v[^ ]+", label)
    return match.group(1) if match else label


def compact_html_label(label: str) -> str:
    lines = display_lines(label)
    name = escape(lines[0][0])
    version = escape(lines[1][0]) if len(lines) > 1 else ""
    rendered = f"<B>{name}</B>" if not version else f"<B>{name}</B> {version}"
    extra = [escape(text) for text, _ in lines[2:]]
    if extra:
        rendered += "<BR/>" + "<BR/>".join(extra)
    return f'<FONT POINT-SIZE="8">{rendered}</FONT>'


def label_text_html(label: str) -> list[str]:
    lines = []
    for text, bold in display_lines(label):
        escaped = escape(text)
        lines.append(f"<B>{escaped}</B>" if bold else escaped)
    return lines


class EmbeddedEntry:
    def __init__(
        self,
        label: str,
        port: str | None = None,
        children: list["EmbeddedEntry"] | None = None,
    ) -> None:
        self.label = label
        self.port = port
        self.children = children or []


def embedded_entry_key(entry: EmbeddedEntry) -> str:
    return entry.label


def normalize_embedded_entries(
    embedded_ends: list[str | tuple[str, str | None] | EmbeddedEntry] | None,
) -> list[EmbeddedEntry]:
    if not embedded_ends:
        return []
    entries: list[EmbeddedEntry] = []
    for end in embedded_ends:
        if isinstance(end, EmbeddedEntry):
            entries.append(end)
        elif isinstance(end, tuple):
            entries.append(EmbeddedEntry(end[0], end[1]))
        else:
            entries.append(EmbeddedEntry(end))
    return sorted(entries, key=embedded_entry_key)


def embedded_path_count(entry: EmbeddedEntry) -> int:
    return 1 + sum(embedded_path_count(child) for child in entry.children)


def direct_embedded_path_index(entries: list[EmbeddedEntry], label: str) -> int | None:
    index = 0
    for entry in entries:
        if entry.label == label:
            return index
        index += embedded_path_count(entry)
    return None


def embedded_bubble_html(entry: EmbeddedEntry) -> str:
    port_attr = f' PORT="{escape(entry.port)}"' if entry.port else ""
    child_rows = "".join(
        f"<TR>{embedded_bubble_html(child)}</TR>"
        for child in normalize_embedded_entries(entry.children)
    )
    return (
        f"<TD{port_attr}>"
        "<TABLE BORDER=\"1\" CELLBORDER=\"0\" CELLSPACING=\"0\" CELLPADDING=\"4\" "
        "COLOR=\"#9aa8b8\" STYLE=\"ROUNDED\">"
        f"<TR><TD>{compact_html_label(entry.label)}</TD></TR>"
        f"{child_rows}"
        "</TABLE>"
        "</TD>"
    )


def html_label(
    label: str,
    embedded_ends: list[str | tuple[str, str | None] | EmbeddedEntry] | None = None,
) -> str:
    lines = label_text_html(label)
    embedded_entries = normalize_embedded_entries(embedded_ends)
    if embedded_entries:
        main_rows = "\n".join(f"<TR><TD>{line}</TD></TR>" for line in lines)
        end_rows = ""
        for entry in embedded_entries:
            end_rows += f"<TR>{embedded_bubble_html(entry)}</TR>"
        return (
            "<<TABLE BORDER=\"0\" CELLBORDER=\"0\" CELLSPACING=\"0\" CELLPADDING=\"0\">"
            f"{main_rows}"
            f"{end_rows}"
            "</TABLE>>"
        )
    return "<" + "<BR/>".join(lines) + ">"


def slug_for(label: str) -> str:
    label = canonical_label(label)
    match = re.match(r"^([^ ]+) v([^ ]+)", label)
    if match:
        base = f"{match.group(1)}-v{match.group(2)}"
    else:
        base = label
    base = re.sub(r"[^A-Za-z0-9._+-]+", "-", base).strip("-")
    return base or "node"


def port_for(label: str) -> str:
    return "p_" + re.sub(r"[^A-Za-z0-9_]+", "_", slug_for(label))


def node_style(label: str) -> tuple[str, str]:
    if "(/home/t4ce/REPOS/TRUEOS/crates/" in label or "(/home/t4ce/REPOS/TRUEOS/kernel/" in label:
        return "#e7f7e7", "#3f8f46"
    if "(/home/t4ce/REPOS/TRUEOS/vendor/" in label:
        return "#fff0d5", "#c47a1a"
    if "https://github.com/t4ce/trait-ffi" in label:
        return "#f0e6ff", "#8656c9"
    return "#f7f9fb", "#8b9bb0"


def read_tree() -> tuple[str, list[str], dict[str, set[str]], set[tuple[str, str]]]:
    root = ""
    root_children: list[str] = []
    stack: dict[int, str] = {}
    adjacency: dict[str, set[str]] = defaultdict(set)
    edges: set[tuple[str, str]] = set()

    for raw in TREE_PATH.read_text().splitlines():
        match = re.match(r"^(\d+)(.*)$", raw)
        if not match:
            continue
        depth = int(match.group(1))
        label = canonical_label(match.group(2))
        if not label:
            continue

        if depth == 0:
            root = label
        elif depth == 1 and label not in root_children:
            root_children.append(label)

        stack[depth] = label
        for stale_depth in [d for d in stack if d > depth]:
            del stack[stale_depth]

        if depth > 0 and (depth - 1) in stack:
            parent = stack[depth - 1]
            adjacency[parent].add(label)
            edges.add((parent, label))

    return root, root_children, adjacency, edges


def reachable(start: str, adjacency: dict[str, set[str]]) -> set[str]:
    seen: set[str] = set()
    stack = [start]
    while stack:
        node = stack.pop()
        if node in seen:
            continue
        seen.add(node)
        stack.extend(sorted(adjacency.get(node, ()), reverse=True))
    return seen


def assign_owners(root_children: list[str], adjacency: dict[str, set[str]]) -> dict[str, str]:
    roots = set(root_children)
    owner = {root: root for root in root_children}
    for root in root_children:
        for node in sorted(reachable(root, adjacency)):
            if node in roots:
                continue
            owner.setdefault(node, root)
    return owner


def unique_filenames(root_children: list[str]) -> dict[str, str]:
    used: dict[str, int] = {}
    names: dict[str, str] = {}
    for root in root_children:
        base = slug_for(root)
        count = used.get(base, 0)
        used[base] = count + 1
        filename = f"{base}.svg" if count == 0 else f"{base}-{count + 1}.svg"
        names[root] = filename
    return names


def render_dot(
    image_root: str,
    owned_nodes: set[str],
    filenames: dict[str, str],
    owner: dict[str, str],
    edges: set[tuple[str, str]],
    incoming: list[tuple[str, str]],
    outgoing: list[tuple[str, str]],
) -> str:
    node_ids: dict[str, str] = {}
    connector_ids: dict[tuple[str, str, str], str] = {}
    lines: list[str] = [
        f'digraph "{dot_escape(slug_for(image_root))}" {{',
        '  graph [rankdir=LR, bgcolor="white", overlap=false, splines=true, nodesep=0.35, ranksep=0.55];',
        '  node [shape=box, style="rounded,filled", fontname="Inter,Arial", fontsize=10, margin="0.08,0.05", color="#8b9bb0", fillcolor="#f7f9fb", fontcolor="#172033"];',
        '  edge [color="#9aa8b8", arrowsize=0.55, penwidth=0.9];',
    ]

    def node_id(label: str) -> str:
        if label not in node_ids:
            node_ids[label] = f"n{len(node_ids)}"
        return node_ids[label]

    internal_edges = {
        (parent, child)
        for parent, child in edges
        if parent in owned_nodes and child in owned_nodes
    }
    internal_in_count: dict[str, int] = defaultdict(int)
    internal_out_count: dict[str, int] = defaultdict(int)
    internal_parent_by_leaf: dict[str, str] = {}
    internal_parents: dict[str, set[str]] = defaultdict(set)
    for parent, child in internal_edges:
        internal_in_count[child] += 1
        internal_out_count[parent] += 1
        internal_parent_by_leaf[child] = parent
        internal_parents[child].add(parent)

    connector_in_count: dict[str, int] = defaultdict(int)
    connector_out_count: dict[str, int] = defaultdict(int)
    for _parent, child in incoming:
        connector_in_count[child] += 1
    for parent, _child in outgoing:
        connector_out_count[parent] += 1

    embedded_by_parent: dict[str, list[str]] = defaultdict(list)
    collapsed_leaves = {
        node
        for node in owned_nodes
        if node != image_root
        and internal_in_count[node] == 1
        and internal_out_count[node] == 0
        and connector_in_count[node] == 0
        and connector_out_count[node] == 0
    }
    for leaf in collapsed_leaves:
        embedded_by_parent[internal_parent_by_leaf[leaf]].append(leaf)
    visible_nodes = owned_nodes - collapsed_leaves
    two_input_leaves = {
        node
        for node in visible_nodes
        if node != image_root
        and internal_in_count[node] == 2
        and internal_out_count[node] == 0
        and connector_in_count[node] == 0
        and connector_out_count[node] == 0
    }

    for label in sorted(visible_nodes, key=lambda x: (x != image_root, x)):
        fill, color = node_style(label)
        if label == image_root:
            fill, color = "#dff3ff", "#1b78a6"
        embedded_ends = sorted(embedded_by_parent.get(label, ()))
        lines.append(
            f'  {node_id(label)} [label={html_label(label, embedded_ends)}, fillcolor="{fill}", color="{color}"];'
        )

    for parent, child in sorted(internal_edges):
        if parent in collapsed_leaves or child in collapsed_leaves:
            continue
        attrs = ' [constraint=false, weight=4]' if child in two_input_leaves else ""
        lines.append(f"  {node_id(parent)} -> {node_id(child)}{attrs};")

    def connector_label(direction: str, filename: str, labels: list[str]) -> str:
        unique_labels = []
        seen = set()
        for label in labels:
            if label not in seen:
                unique_labels.append(label)
                seen.add(label)
        visible = unique_labels[:8]
        suffix = [] if len(unique_labels) <= 8 else [f"... {len(unique_labels) - 8} more"]
        lines = [escape(f"{direction} {filename}")]
        for label in visible:
            lines.extend(
                f"<B>{escape(text)}</B>" if bold else escape(text)
                for text, bold in display_lines(label)
            )
        lines.extend(escape(line) for line in suffix)
        return "<" + "<BR/>".join(lines) + ">"

    outgoing_by_target: dict[str, list[tuple[str, str]]] = defaultdict(list)
    for parent, child in outgoing:
        outgoing_by_target[owner[child]].append((parent, child))

    for target_root, crossings in sorted(outgoing_by_target.items(), key=lambda x: filenames[x[0]]):
        key = ("out", image_root, target_root)
        cid = connector_ids.setdefault(key, f"out{len(connector_ids)}")
        label = connector_label("to", filenames[target_root], [child for _, child in sorted(crossings)])
        lines.append(
            f'  {cid} [label={label}, shape=note, style="filled", fillcolor="#fff8e8", color="#d19922", fontcolor="#614000"];'
        )
        for parent in sorted({parent for parent, _ in crossings if parent not in collapsed_leaves}):
            lines.append(f"  {node_id(parent)} -> {cid} [style=dashed, color=\"#d19922\"];")

    incoming_by_source: dict[str, list[tuple[str, str]]] = defaultdict(list)
    for parent, child in incoming:
        incoming_by_source[owner[parent]].append((parent, child))

    for source_root, crossings in sorted(incoming_by_source.items(), key=lambda x: filenames[x[0]]):
        key = ("in", source_root, image_root)
        cid = connector_ids.setdefault(key, f"in{len(connector_ids)}")
        label = connector_label("from", filenames[source_root], [child for _, child in sorted(crossings)])
        lines.append(
            f'  {cid} [label={label}, shape=note, style="filled", fillcolor="#eef6ff", color="#4b8fc5", fontcolor="#123d5c"];'
        )
        for child in sorted({child for _, child in crossings if child not in collapsed_leaves}):
            lines.append(f"  {cid} -> {node_id(child)} [style=dashed, color=\"#4b8fc5\"];")

    for leaf in sorted(two_input_leaves):
        parent_a, parent_b = sorted(internal_parents[leaf])
        lines.append("  {")
        lines.append("    rank=same;")
        lines.append(f"    {node_id(parent_a)}; {node_id(leaf)}; {node_id(parent_b)};")
        lines.append(
            f"    {node_id(parent_a)} -> {node_id(leaf)} -> {node_id(parent_b)} "
            '[style=invis, weight=120];'
        )
        lines.append("  }")

    lines.append(f"  // collapsed leaf ends: {len(collapsed_leaves)}")
    lines.append(f"  // centered two-input leaves: {len(two_input_leaves)}")
    lines.append("}")
    return "\n".join(lines) + "\n"


def full_collapse_info(
    root: str, edges: set[tuple[str, str]]
) -> tuple[
    set[str],
    dict[str, int],
    dict[str, int],
    dict[str, set[str]],
    set[str],
    dict[str, list[EmbeddedEntry]],
    set[str],
]:
    all_nodes = {root}
    incoming_count: dict[str, int] = defaultdict(int)
    outgoing_count: dict[str, int] = defaultdict(int)
    incoming_parents: dict[str, set[str]] = defaultdict(set)
    children_by_parent: dict[str, set[str]] = defaultdict(set)
    for parent, child in edges:
        all_nodes.add(parent)
        all_nodes.add(child)
        incoming_count[child] += 1
        outgoing_count[parent] += 1
        incoming_parents[child].add(parent)
        children_by_parent[parent].add(child)

    shared_input_leaves = {
        node
        for node in all_nodes
        if node != root
        and incoming_count[node] == 2
        and outgoing_count[node] == 0
    }
    collapsed_leaves = set(shared_input_leaves)
    changed = True
    while changed:
        changed = False
        for node in sorted(all_nodes):
            if node == root or node in collapsed_leaves or incoming_count[node] != 1:
                continue
            parent = next(iter(incoming_parents[node]))
            if parent == root and outgoing_count[node] != 0:
                continue
            if all(child in collapsed_leaves for child in children_by_parent.get(node, ())):
                collapsed_leaves.add(node)
                changed = True

    def embedded_entry(label: str) -> EmbeddedEntry:
        children = [
            embedded_entry(child)
            for child in sorted(children_by_parent.get(label, ()))
            if child in collapsed_leaves
        ]
        port = port_for(label) if label in shared_input_leaves else None
        return EmbeddedEntry(label, port, children)

    embedded_by_parent: dict[str, list[EmbeddedEntry]] = defaultdict(list)
    for node in sorted(collapsed_leaves):
        if incoming_count[node] == 1:
            parent = next(iter(incoming_parents[node]))
            if parent not in collapsed_leaves:
                embedded_by_parent[parent].append(embedded_entry(node))
        elif node in shared_input_leaves:
            for parent in sorted(incoming_parents[node]):
                if parent not in collapsed_leaves:
                    embedded_by_parent[parent].append(embedded_entry(node))
    for parent, entries in embedded_by_parent.items():
        embedded_by_parent[parent] = normalize_embedded_entries(entries)

    return (
        all_nodes,
        incoming_count,
        outgoing_count,
        incoming_parents,
        collapsed_leaves,
        embedded_by_parent,
        shared_input_leaves,
    )


def render_full_dot(root: str, edges: set[tuple[str, str]]) -> str:
    node_ids: dict[str, str] = {}
    (
        all_nodes,
        incoming_count,
        outgoing_count,
        incoming_parents,
        collapsed_leaves,
        embedded_by_parent,
        shared_input_leaves,
    ) = full_collapse_info(root, edges)

    left_input_leaves = {
        node
        for node in all_nodes
        if node != root
        and node not in shared_input_leaves
        and node not in collapsed_leaves
        and incoming_count[node] == 2
        and outgoing_count[node] == 0
        and root in incoming_parents[node]
        and crate_name(node) in LEFT_WING_CRATES
    }
    visible_nodes = all_nodes - collapsed_leaves
    left_wing_nodes = {
        node for node in visible_nodes if crate_name(node) in LEFT_WING_CRATES
    }

    def node_id(label: str) -> str:
        if label not in node_ids:
            node_ids[label] = f"n{len(node_ids)}"
        return node_ids[label]

    lines = [
        "digraph trueos_depth_graph {",
        '  graph [rankdir=LR, bgcolor="white", overlap=false, splines=true, nodesep=0.35, ranksep=1.004];',
        '  node [shape=box, style="rounded,filled", fontname="Inter,Arial", fontsize=10, margin="0.08,0.05", color="#8b9bb0", fillcolor="#f7f9fb", fontcolor="#172033"];',
        '  edge [color="#9aa8b8", arrowsize=0.55, penwidth=0.9];',
    ]

    for label in sorted(visible_nodes, key=lambda x: (x != root, x)):
        fill, color = node_style(label)
        if label == root:
            fill, color = "#dff3ff", "#1b78a6"
        embedded_ends = embedded_by_parent.get(label, [])
        lines.append(
            f'  {node_id(label)} [label={html_label(label, embedded_ends)}, fillcolor="{fill}", color="{color}"];'
        )
    for parent, child in sorted(edges):
        if parent in collapsed_leaves or child in collapsed_leaves:
            continue
        if child in left_wing_nodes:
            lines.append(
                f"  {node_id(parent)} -> {node_id(child)} "
                "[constraint=false, tailport=w, headport=e];"
            )
        else:
            lines.append(f"  {node_id(parent)} -> {node_id(child)};")

    for leaf in sorted(left_input_leaves):
        for parent in sorted(incoming_parents[leaf]):
            if parent in collapsed_leaves or parent in left_wing_nodes:
                continue
            lines.append(
                f"  {node_id(leaf)} -> {node_id(parent)} "
                '[style=invis, weight=90, minlen=2];'
            )

    for index in range(1, 4):
        lines.append(
            f'  left_space_{index} [label="", shape=point, width=0.01, height=0.01, '
            'style=invis];'
        )
    lines.append(
        f"  left_space_3 -> left_space_2 -> left_space_1 -> {node_id(root)} "
        '[style=invis, weight=1000];'
    )
    if left_wing_nodes:
        rank_members = "; ".join(["left_space_2", *(node_id(node) for node in sorted(left_wing_nodes))])
        lines.append("  {")
        lines.append("    rank=same;")
        lines.append(f"    {rank_members};")
        lines.append("  }")
        for node in sorted(left_wing_nodes):
            lines.append(
                f"  {node_id(node)} -> {node_id(root)} "
                '[style=invis, weight=300, minlen=2];'
            )

    lines.append(f"  // collapsed leaf ends: {len(collapsed_leaves)}")
    lines.append(f"  // shared two-input leaf ends: {len(shared_input_leaves)}")
    lines.append(f"  // left-wing root-shared leaf ends: {len(left_input_leaves)}")
    lines.append(f"  // pinned left-wing crates: {len(left_wing_nodes)}")
    lines.append("}")
    return "\n".join(lines) + "\n"


def full_graph_layout(
    root: str, edges: set[tuple[str, str]]
) -> tuple[dict[str, str], set[str], dict[str, list[EmbeddedEntry]], set[str], dict[str, set[str]]]:
    (
        all_nodes,
        _incoming_count,
        _outgoing_count,
        incoming_parents,
        collapsed_leaves,
        embedded_by_parent,
        shared_input_leaves,
    ) = full_collapse_info(root, edges)
    visible_nodes = all_nodes - collapsed_leaves
    node_ids: dict[str, str] = {}
    for label in sorted(visible_nodes, key=lambda x: (x != root, x)):
        node_ids[label] = f"n{len(node_ids)}"
    return node_ids, collapsed_leaves, embedded_by_parent, shared_input_leaves, incoming_parents


def svg_path_bbox(path_d: str) -> tuple[float, float, float, float]:
    nums = [float(n) for n in re.findall(r"-?\d+(?:\.\d+)?", path_d)]
    points = list(zip(nums[::2], nums[1::2]))
    xs = [x for x, _ in points]
    ys = [y for _, y in points]
    return min(xs), min(ys), max(xs), max(ys)


def transform_bbox(
    bbox: tuple[float, float, float, float], tx: float, ty: float
) -> tuple[float, float, float, float]:
    min_x, min_y, max_x, max_y = bbox
    return min_x + tx, min_y + ty, max_x - min_x, max_y - min_y


def display_size(width: float, height: float) -> tuple[float, float]:
    return width, height


def full_svg_bboxes() -> tuple[
    float,
    float,
    dict[str, tuple[float, float, float, float]],
    dict[str, list[tuple[float, float, float, float]]],
]:
    svg = FULL_SVG_PATH.read_text()
    viewbox = re.search(r'viewBox="[^"]*?[^0-9.]([0-9.]+) ([0-9.]+)"', svg)
    if not viewbox:
        raise RuntimeError(f"could not parse viewBox from {FULL_SVG_PATH}")
    width, height = float(viewbox.group(1)), float(viewbox.group(2))

    transform = re.search(r'<g id="graph0"[^>]*transform="[^"]*translate\(([0-9.-]+) ([0-9.-]+)\)', svg)
    if not transform:
        raise RuntimeError(f"could not parse graph transform from {FULL_SVG_PATH}")
    tx, ty = float(transform.group(1)), float(transform.group(2))

    bboxes: dict[str, tuple[float, float, float, float]] = {}
    inner_bboxes: dict[str, list[tuple[float, float, float, float]]] = {}
    for group in re.finditer(r'<g id="node\d+" class="node">(.*?)</g>', svg, re.S):
        body = group.group(1)
        title = re.search(r"<title>(n\d+)</title>", body)
        if not title:
            continue
        node_id = title.group(1)
        paths = re.findall(r'<path\b[^>]*\bd="([^"]+)"', body)
        if not paths:
            continue
        bboxes[node_id] = transform_bbox(svg_path_bbox(paths[0]), tx, ty)
        inner_bboxes[node_id] = [transform_bbox(svg_path_bbox(path), tx, ty) for path in paths[1:]]

    return width, height, bboxes, inner_bboxes


def ownership_regions(
    root: str, root_children: list[str], edges: set[tuple[str, str]]
) -> list[dict[str, object]]:
    adjacency: dict[str, set[str]] = defaultdict(set)
    for parent, child in edges:
        adjacency[parent].add(child)
    node_ids, _collapsed_leaves, _embedded_by_parent, _shared_input_leaves, _incoming_parents = (
        full_graph_layout(root, edges)
    )
    width, height, node_bboxes, _inner_bboxes = full_svg_bboxes()

    def region_for(prefix: str, label: str, class_name: str) -> dict[str, object] | None:
        root_label = next((child for child in root_children if child.startswith(prefix)), None)
        if not root_label:
            return None
        owned_nodes = {root_label, *adjacency.get(root_label, set())}
        boxes = []
        for node in owned_nodes:
            node_id = node_ids.get(node)
            if node_id and node_id in node_bboxes:
                boxes.append(node_bboxes[node_id])
        if not boxes:
            return None
        min_x = min(box[0] for box in boxes)
        min_y = min(box[1] for box in boxes)
        max_x = max(box[0] + box[2] for box in boxes)
        max_y = max(box[1] + box[3] for box in boxes)
        pad = 24
        return {
            "label": label,
            "class_name": class_name,
            "x": round(max(0, min_x - pad), 2),
            "y": round(max(0, min_y - pad), 2),
            "w": round(min(width, max_x + pad) - max(0, min_x - pad), 2),
            "h": round(min(height, max_y + pad) - max(0, min_y - pad), 2),
        }

    return [
        region
        for region in [
            region_for("crab-usb ", "crab-usb direct dependency region", "ownership-region-crab-usb"),
            region_for(
                "rustls-rustcrypto ",
                "rustls-rustcrypto direct dependency region",
                "ownership-region-rustcrypto",
            ),
        ]
        if region
    ]


def connection_points(
    a: tuple[float, float, float, float],
    b: tuple[float, float, float, float],
) -> tuple[float, float, float, float] | None:
    ax, ay, aw, ah = a
    bx, by, bw, bh = b
    acx, acy = ax + aw / 2, ay + ah / 2
    bcx, bcy = bx + bw / 2, by + bh / 2
    dx, dy = bcx - acx, bcy - acy
    if abs(dx) < 0.001 and abs(dy) < 0.001:
        return None

    def boundary(cx: float, cy: float, w: float, h: float, sx: float, sy: float) -> tuple[float, float]:
        x_scale = (w / 2) / abs(sx) if abs(sx) >= 0.001 else float("inf")
        y_scale = (h / 2) / abs(sy) if abs(sy) >= 0.001 else float("inf")
        scale = min(x_scale, y_scale)
        return cx + sx * scale, cy + sy * scale

    x1, y1 = boundary(acx, acy, aw, ah, dx, dy)
    x2, y2 = boundary(bcx, bcy, bw, bh, -dx, -dy)
    return x1, y1, x2, y2


def shared_leaf_links(root: str, edges: set[tuple[str, str]]) -> list[dict[str, object]]:
    node_ids, _collapsed_leaves, embedded_by_parent, shared_input_leaves, incoming_parents = (
        full_graph_layout(root, edges)
    )
    _width, _height, _node_bboxes, inner_bboxes = full_svg_bboxes()
    links: list[dict[str, object]] = []

    for leaf in sorted(shared_input_leaves):
        parents = sorted(incoming_parents[leaf])
        if len(parents) != 2:
            continue
        bubble_boxes = []
        for parent in parents:
            node_id = node_ids.get(parent)
            if not node_id:
                break
            entries = embedded_by_parent.get(parent, [])
            index = direct_embedded_path_index(entries, leaf)
            if index is None:
                break
            boxes = inner_bboxes.get(node_id, [])
            if index >= len(boxes):
                break
            bubble_boxes.append(boxes[index])
        if len(bubble_boxes) != 2:
            continue
        points = connection_points(bubble_boxes[0], bubble_boxes[1])
        if not points:
            continue
        links.append(
            {
                "label": display_label(leaf).replace(r"\n", " / "),
                "x1": round(points[0], 2),
                "y1": round(points[1], 2),
                "x2": round(points[2], 2),
                "y2": round(points[3], 2),
            }
        )
    return links


def terminal_leaf_borders(root: str, edges: set[tuple[str, str]]) -> list[dict[str, object]]:
    incoming_count: dict[str, int] = defaultdict(int)
    outgoing_count: dict[str, int] = defaultdict(int)
    for parent, child in edges:
        incoming_count[child] += 1
        outgoing_count[parent] += 1

    node_ids, _collapsed_leaves, embedded_by_parent, _shared_input_leaves, _incoming_parents = (
        full_graph_layout(root, edges)
    )
    _width, _height, node_bboxes, inner_bboxes = full_svg_bboxes()
    borders: list[dict[str, object]] = []
    for label, node_id in sorted(node_ids.items(), key=lambda item: item[0]):
        if label == root:
            continue
        if incoming_count[label] == 0 or outgoing_count[label] != 0:
            continue
        if embedded_by_parent.get(label) or inner_bboxes.get(node_id):
            continue
        if node_id not in node_bboxes:
            continue
        x, y, w, h = node_bboxes[node_id]
        _fill, color = node_style(label)
        pad = 3.0
        borders.append(
            {
                "label": display_label(label).replace(r"\n", " / "),
                "color": color,
                "x": round(max(0, x - pad), 2),
                "y": round(max(0, y - pad), 2),
                "w": round(w + pad * 2, 2),
                "h": round(h + pad * 2, 2),
                "r": round(min(18.0, h / 2 + pad), 2),
            }
        )
    return borders


def inject_full_svg_regions(root: str, root_children: list[str], edges: set[tuple[str, str]]) -> None:
    svg = FULL_SVG_PATH.read_text()
    transform = re.search(
        r'<g id="graph0"[^>]*transform="[^"]*translate\(([0-9.-]+) ([0-9.-]+)\)',
        svg,
    )
    if not transform:
        raise RuntimeError(f"could not parse graph transform from {FULL_SVG_PATH}")
    tx, ty = float(transform.group(1)), float(transform.group(2))

    svg = re.sub(
        r'<g id="shared-leaf-links" class="shared-leaf-links">.*?</g>\n',
        "",
        svg,
        flags=re.S,
    )
    svg = re.sub(
        r'<g id="terminal-leaf-borders" class="terminal-leaf-borders">.*?</g>\n',
        "",
        svg,
        flags=re.S,
    )
    region_lines = ['<g id="ownership-regions" class="ownership-regions">']
    for region in ownership_regions(root, root_children, edges):
        x = float(region["x"]) - tx
        y = float(region["y"]) - ty
        dash = "4 9" if "rustcrypto" in str(region["class_name"]) else "12 10"
        region_lines.extend(
            [
                f'<!-- {escape(str(region["label"]))} -->',
                f'<g class="{escape(str(region["class_name"]))}">',
                f'<title>{escape(str(region["label"]))}</title>',
                (
                    f'<rect x="{x:.2f}" y="{y:.2f}" width="{float(region["w"]):.2f}" '
                    f'height="{float(region["h"]):.2f}" rx="26" ry="26" '
                    'fill="black" fill-opacity="0.045" '
                    'stroke="black" stroke-opacity="0.33" stroke-width="3" '
                    f'stroke-dasharray="{dash}"/>'
                ),
                "</g>",
            ]
        )
    region_lines.append("</g>")
    region_markup = "\n".join(region_lines) + "\n"

    svg = re.sub(
        r'<g id="ownership-regions" class="ownership-regions">.*?</g>\n',
        "",
        svg,
        flags=re.S,
    )
    svg = re.sub(
        r'(<polygon fill="white" stroke="none" points="[^"]+"/>\n)',
        r"\1" + region_markup,
        svg,
        count=1,
    )

    border_lines = ['<g id="terminal-leaf-borders" class="terminal-leaf-borders" pointer-events="none">']
    for border in terminal_leaf_borders(root, edges):
        x = float(border["x"]) - tx
        y = float(border["y"]) - ty
        border_lines.extend(
            [
                f'<!-- incoming-only leaf {escape(str(border["label"]))} -->',
                f'<title>incoming-only leaf {escape(str(border["label"]))}</title>',
                (
                    f'<rect x="{x:.2f}" y="{y:.2f}" width="{float(border["w"]):.2f}" '
                    f'height="{float(border["h"]):.2f}" rx="{float(border["r"]):.2f}" '
                    f'ry="{float(border["r"]):.2f}" fill="none" '
                    f'stroke="{escape(str(border["color"]))}" stroke-opacity="0.78" '
                    'stroke-width="1.25"/>'
                ),
            ]
        )
    border_lines.append("</g>")
    border_markup = "\n".join(border_lines) + "\n"

    link_lines = [
        '<g id="shared-leaf-links" class="shared-leaf-links" pointer-events="none">',
        '<defs>',
        (
            '<marker id="shared-leaf-arrow" viewBox="0 0 7 6" refX="6.2" refY="3" '
            'markerWidth="7" markerHeight="6" orient="auto-start-reverse">'
            '<path d="M 0 0 L 7 3 L 0 6 z" fill="#111111"/>'
            "</marker>"
        ),
        "</defs>",
    ]
    for link in shared_leaf_links(root, edges):
        x1 = float(link["x1"]) - tx
        y1 = float(link["y1"]) - ty
        x2 = float(link["x2"]) - tx
        y2 = float(link["y2"]) - ty
        link_lines.extend(
            [
                f'<!-- shared leaf {escape(str(link["label"]))} -->',
                "<g>",
                f'<title>shared leaf {escape(str(link["label"]))}</title>',
                (
                    f'<line x1="{x1:.2f}" y1="{y1:.2f}" x2="{x2:.2f}" y2="{y2:.2f}" '
                    'stroke="#111111" stroke-opacity="0.82" stroke-width="1.6" '
                    'marker-start="url(#shared-leaf-arrow)" marker-end="url(#shared-leaf-arrow)"/>'
                ),
                "</g>",
            ]
        )
    link_lines.append("</g>")
    link_markup = "\n".join(link_lines) + "\n"

    graph_end = svg.rfind("</g>\n</svg>")
    if graph_end == -1:
        raise RuntimeError(f"could not find graph end in {FULL_SVG_PATH}")
    svg = svg[:graph_end] + border_markup + link_markup + svg[graph_end:]

    viewbox = re.search(r'viewBox="0\.00 0\.00 ([0-9.]+) ([0-9.]+)"', svg)
    if viewbox:
        display_width, display_height = display_size(float(viewbox.group(1)), float(viewbox.group(2)))
        svg = re.sub(
            r'<svg width="[^"]+" height="[^"]+"',
            f'<svg width="{display_width:.0f}pt" height="{display_height:.0f}pt"',
            svg,
            count=1,
        )
    FULL_SVG_PATH.write_text(svg)


def render_html_index(root: str, root_children: list[str], edges: set[tuple[str, str]]) -> str:
    filenames = unique_filenames(root_children)
    node_ids, collapsed_leaves, embedded_by_parent, _shared_input_leaves, _incoming_parents = (
        full_graph_layout(root, edges)
    )
    width, height, node_bboxes, inner_bboxes = full_svg_bboxes()
    display_width, display_height = display_size(width, height)
    root_inner_bboxes = inner_bboxes.get("n0", [])
    root_inner_by_label = {}
    root_entries = embedded_by_parent.get(root, [])
    for child in root_children:
        if child not in collapsed_leaves:
            continue
        index = direct_embedded_path_index(root_entries, child)
        if index is not None and index < len(root_inner_bboxes):
            root_inner_by_label[child] = root_inner_bboxes[index]

    hotspots = []
    for child in root_children:
        if child in node_ids:
            bbox = node_bboxes.get(node_ids[child])
        else:
            bbox = root_inner_by_label.get(child)
        if not bbox:
            continue
        x, y, w, h = bbox
        pad = 5
        hotspots.append(
            {
                "label": display_label(child).replace(r"\n", " / "),
                "href": f"by-root/{filenames[child]}",
                "x": round(max(0, x - pad), 2),
                "y": round(max(0, y - pad), 2),
                "w": round(w + pad * 2, 2),
                "h": round(h + pad * 2, 2),
            }
        )

    rects = "\n".join(
        f'''      <a class="root-hotspot" href="{escape(hotspot["href"])}" aria-label="{escape(hotspot["label"])}">
        <title>{escape(hotspot["label"])}</title>
        <rect x="{hotspot["x"]}" y="{hotspot["y"]}" width="{hotspot["w"]}" height="{hotspot["h"]}" rx="9" ry="9"></rect>
      </a>'''
        for hotspot in hotspots
    )
    return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>TRUEOS Dependency Graph</title>
  <style>
    :root {{
      color-scheme: light;
      --ink: #172033;
      --hot: #1b78a6;
      --paper: #f6f8fb;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      color: var(--ink);
      background: var(--paper);
      font: 14px/1.4 Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }}
    .viewport {{
      overflow: auto;
      padding: 18px;
      min-height: 100vh;
    }}
    #graph {{
      display: block;
      width: {display_width}px;
      max-width: none;
      height: auto;
      background: white;
      box-shadow: 0 1px 3px rgba(15, 23, 42, 0.14);
      transform-origin: top left;
    }}
    @keyframes rootPulse {{
      0%, 100% {{ fill: rgba(27, 120, 166, 0.06); }}
      50% {{ fill: rgba(27, 120, 166, 0.25); }}
    }}
    .root-hotspot rect {{
      fill: rgba(27, 120, 166, 0.06);
      stroke: transparent;
      stroke-width: 3;
      pointer-events: all;
      animation: rootPulse 3.2s ease-in-out infinite;
      transition: fill 120ms ease, stroke 120ms ease;
    }}
    .root-hotspot:hover rect,
    .root-hotspot:focus rect {{
      animation: none;
      fill: rgba(27, 120, 166, 0.25);
      stroke: var(--hot);
    }}
    dialog {{
      width: 95vw;
      height: 95vh;
      max-width: 95vw;
      max-height: 95vh;
      padding: 0;
      border: 0;
      border-radius: 8px;
      background: white;
      box-shadow: 0 24px 80px rgba(15, 23, 42, 0.32);
      overflow: hidden;
    }}
    dialog::backdrop {{
      background: rgba(15, 23, 42, 0.32);
    }}
    #close-dialog {{
      position: absolute;
      top: 8px;
      right: 8px;
      z-index: 1;
      display: grid;
      place-items: center;
      width: 32px;
      height: 32px;
      border: 1px solid #b9c5d3;
      border-radius: 6px;
      background: rgba(255, 255, 255, 0.92);
      color: var(--ink);
      font-size: 22px;
      line-height: 1;
      cursor: pointer;
    }}
    #dialog-frame {{
      width: 100%;
      height: 100%;
      border: 0;
      background: white;
    }}
  </style>
</head>
<body>
  <div class="viewport">
    <svg id="graph" viewBox="0 0 {width} {height}" width="{display_width}" height="{display_height}" xmlns="http://www.w3.org/2000/svg">
      <image href="trueos-depth-tree.svg" x="0" y="0" width="{width}" height="{height}"></image>
{rects}
    </svg>
  </div>
  <dialog id="root-dialog">
    <button id="close-dialog" type="button" aria-label="Close">&times;</button>
    <iframe id="dialog-frame" title="Root dependency graph"></iframe>
  </dialog>
  <script>
    const dialog = document.getElementById('root-dialog');
    const frame = document.getElementById('dialog-frame');
    const closeButton = document.getElementById('close-dialog');

    document.querySelectorAll('.root-hotspot').forEach((link) => {{
      link.addEventListener('click', (event) => {{
        event.preventDefault();
        frame.src = link.getAttribute('href');
        dialog.showModal();
      }});
    }});

    closeButton.addEventListener('click', () => {{
      dialog.close();
    }});

    dialog.addEventListener('close', () => {{
      frame.removeAttribute('src');
    }});
  </script>
</body>
</html>
"""


def main() -> None:
    root, root_children, adjacency, edges = read_tree()
    if not root or not root_children:
        raise SystemExit(f"could not parse root children from {TREE_PATH}")

    FULL_DOT_PATH.write_text(render_full_dot(root, edges))
    subprocess.run(["dot", "-Tsvg", str(FULL_DOT_PATH), "-o", str(FULL_SVG_PATH)], check=True)
    inject_full_svg_regions(root, root_children, edges)
    HTML_INDEX_PATH.write_text(render_html_index(root, root_children, edges))

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    for old in OUT_DIR.glob("*.dot"):
        old.unlink()
    for old in OUT_DIR.glob("*.svg"):
        old.unlink()

    owner = assign_owners(root_children, adjacency)
    filenames = unique_filenames(root_children)
    all_roots = set(root_children)

    rows: list[tuple[str, str, int, int, int]] = []
    for image_root in root_children:
        owned_nodes = {node for node, node_owner in owner.items() if node_owner == image_root}
        owned_nodes.add(image_root)

        incoming: list[tuple[str, str]] = []
        outgoing: list[tuple[str, str]] = []
        for parent, child in edges:
            if parent == root:
                continue
            parent_owner = owner.get(parent)
            child_owner = owner.get(child)
            if not parent_owner or not child_owner or parent_owner == child_owner:
                continue
            if parent_owner == image_root and parent in owned_nodes:
                outgoing.append((parent, child))
            if child_owner == image_root and child in owned_nodes:
                incoming.append((parent, child))

        dot = render_dot(image_root, owned_nodes, filenames, owner, edges, incoming, outgoing)
        dot_path = OUT_DIR / filenames[image_root].replace(".svg", ".dot")
        svg_path = OUT_DIR / filenames[image_root]
        dot_path.write_text(dot)
        subprocess.run(["dot", "-Tsvg", str(dot_path), "-o", str(svg_path)], check=True)
        input_groups = len({owner[parent] for parent, _ in incoming})
        output_groups = len({owner[child] for _, child in outgoing})
        rows.append((image_root, filenames[image_root], len(owned_nodes), input_groups, output_groups))

    index = [
        "# TRUEOS dependency graph split by root dependency",
        "",
        f"Source: `{TREE_PATH}`",
        "",
        "Each SVG expands one direct dependency of the TRUEOS root. Blue note nodes are incoming cross-image edges; yellow note nodes are outgoing cross-image edges.",
        "",
        "| Root dependency | SVG | Owned nodes | Input images | Output images |",
        "| --- | --- | ---: | ---: | ---: |",
    ]
    for root_label, filename, node_count, input_count, output_count in rows:
        rendered_root = display_label(root_label).replace(r"\n", "<br>")
        index.append(
            f"| {rendered_root} | [`{filename}`]({filename}) | {node_count} | {input_count} | {output_count} |"
        )
    index.append("")
    index.append(f"Total direct TRUEOS roots: {len(all_roots)}")
    index.append(f"Total owned nodes excluding TRUEOS root: {len(owner)}")
    (OUT_DIR / "README.md").write_text("\n".join(index) + "\n")

    print(f"wrote {len(rows)} split graphs to {OUT_DIR}")
    print(f"owned nodes excluding TRUEOS root: {len(owner)}")


if __name__ == "__main__":
    main()
