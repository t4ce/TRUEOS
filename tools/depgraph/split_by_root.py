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
NODE_MARGIN = "0.13,0.10"
INNER_BUBBLE_PADDING = 8
INNER_BUBBLE_SPACING = 6
NAME_POINT_SIZE = 13
VERSION_POINT_SIZE = 12
PATH_POINT_SIZE = 11
COMPACT_NAME_POINT_SIZE = 11
COMPACT_VERSION_POINT_SIZE = 10
COMPACT_PATH_POINT_SIZE = 9
GRAPH_EDGE_PEN_WIDTH = 7.2
SHARED_LEAF_LINK_STROKE_WIDTH = 12.8
TERMINAL_LEAF_BORDER_STROKE_WIDTH = GRAPH_EDGE_PEN_WIDTH
OWNERSHIP_REGION_STROKE_WIDTH = GRAPH_EDGE_PEN_WIDTH
NESTED_SHARED_BADGE_STROKE_WIDTH = GRAPH_EDGE_PEN_WIDTH
FULL_GRAPH_LABEL_SCALE = 4.5
FULL_GRAPH_ROOT_SCALE = FULL_GRAPH_LABEL_SCALE * 2
FULL_GRAPH_ROOT_SCALE_CRATES = {
    "crab-usb",
    "hyper",
    "mio",
    "rustls-rustcrypto",
    "socket2",
    "tower",
}
SHARED_LINK_VERTICAL_THRESHOLD = 24.0
ARCHITECTURE_BUCKET_PADDING = 32.0
# Spread the full LR graph wide enough that the layout itself uses the 16:9 canvas.
FULL_GRAPH_NODESEP = 0.62
FULL_GRAPH_RANKSEP = 55.0
FULL_GRAPH_ASPECT_RATIO = 16 / 9
SPLIT_GRAPH_NODESEP = 0.35
SPLIT_GRAPH_RANKSEP = 0.55
LEFT_WING_CRATES = {
    "core3",
    "memchr",
    "mio",
    "regex-automata",
    "serde",
    "serde_derive",
    "serde_json",
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
LEFT_SIDE_ATTACHED_PARENT_CRATES = {
    "hyper",
}
SKIP_ATTACHED_CHILDREN_OF_CRATES = {
    "rustls-rustcrypto",
}
ARCHITECTURE_IRRELEVANT_CRATES = {
    "log",
}


def left_wing_depths(
    left_wing_nodes: set[str],
    edges: set[tuple[str, str]],
) -> dict[str, int]:
    left_parents: dict[str, set[str]] = defaultdict(set)
    for parent, child in edges:
        if parent in left_wing_nodes and child in left_wing_nodes:
            left_parents[child].add(parent)

    depths: dict[str, int] = {}
    visiting: set[str] = set()

    def depth(node: str) -> int:
        if node in depths:
            return depths[node]
        if node in visiting:
            return 0
        visiting.add(node)
        parents = left_parents.get(node, set())
        depths[node] = 0 if not parents else max(depth(parent) + 1 for parent in parents)
        visiting.remove(node)
        return depths[node]

    return {node: depth(node) for node in left_wing_nodes}


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


def is_architecture_irrelevant(label: str) -> bool:
    return crate_name(label) in ARCHITECTURE_IRRELEVANT_CRATES


def scaled(value: int, scale: float) -> int:
    return max(1, round(value * scale))


def scaled_node_margin(scale: float) -> str:
    x_text, y_text = NODE_MARGIN.split(",", 1)
    return f"{float(x_text) * scale:.2f},{float(y_text) * scale:.2f}"


def compact_html_label(label: str, scale: float = 1.0) -> str:
    lines = display_lines(label)
    name = escape(lines[0][0])
    rendered = f'<FONT POINT-SIZE="{scaled(COMPACT_NAME_POINT_SIZE, scale)}"><B>{name}</B></FONT>'
    if len(lines) > 1:
        version = escape(lines[1][0])
        rendered += f' <FONT POINT-SIZE="{scaled(COMPACT_VERSION_POINT_SIZE, scale)}">{version}</FONT>'
    extra = [
        f'<FONT POINT-SIZE="{scaled(COMPACT_PATH_POINT_SIZE, scale)}">{escape(text)}</FONT>'
        for text, _ in lines[2:]
    ]
    if extra:
        rendered += "<BR/>" + "<BR/>".join(extra)
    return rendered


def label_text_html(
    label: str,
    scale: float = 1.0,
    include_paths: bool = True,
) -> list[str]:
    lines = []
    display = display_lines(label)
    if not include_paths:
        display = display[:2]
    for index, (text, bold) in enumerate(display):
        escaped = escape(text)
        if index == 0:
            lines.append(f'<FONT POINT-SIZE="{scaled(NAME_POINT_SIZE, scale)}"><B>{escaped}</B></FONT>')
        elif index == 1:
            lines.append(f'<FONT POINT-SIZE="{scaled(VERSION_POINT_SIZE, scale)}">{escaped}</FONT>')
        else:
            lines.append(f'<FONT POINT-SIZE="{scaled(PATH_POINT_SIZE, scale)}">{escaped}</FONT>')
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


def embedded_path_labels_postorder(entries: list[EmbeddedEntry]) -> list[str]:
    labels: list[str] = []
    for entry in entries:
        labels.extend(embedded_path_labels_postorder(normalize_embedded_entries(entry.children)))
        labels.append(entry.label)
    return labels


def embedded_path_index_postorder(entries: list[EmbeddedEntry], label: str) -> int | None:
    for index, entry_label in enumerate(embedded_path_labels_postorder(entries)):
        if entry_label == label:
            return index
    return None


def embedded_bubble_html(
    entry: EmbeddedEntry,
    scale: float = 1.0,
) -> str:
    port_attr = f' PORT="{escape(entry.port)}"' if entry.port else ""
    child_rows = embedded_rows_html(
        normalize_embedded_entries(entry.children), scale
    )
    child_row = f"<TR><TD>{child_rows}</TD></TR>" if child_rows else ""
    return (
        f"<TD{port_attr}>"
        f"<TABLE BORDER=\"1\" CELLBORDER=\"0\" CELLSPACING=\"0\" CELLPADDING=\"{scaled(INNER_BUBBLE_PADDING, scale)}\" "
        "COLOR=\"#9aa8b8\" STYLE=\"ROUNDED\">"
        f"<TR><TD>{compact_html_label(entry.label, scale)}</TD></TR>"
        f"{child_row}"
        "</TABLE>"
        "</TD>"
    )


def embedded_rows_html(
    entries: list[EmbeddedEntry],
    scale: float = 1.0,
) -> str:
    if not entries:
        return ""
    rows = "".join(
        f"<TR>{embedded_bubble_html(entry, scale)}</TR>"
        for entry in entries
    )
    return (
        f"<TABLE BORDER=\"0\" CELLBORDER=\"0\" CELLSPACING=\"{scaled(INNER_BUBBLE_SPACING, scale)}\" CELLPADDING=\"0\">"
        f"{rows}"
        "</TABLE>"
    )


def html_label(
    label: str,
    embedded_ends: list[str | tuple[str, str | None] | EmbeddedEntry] | None = None,
    scale: float = 1.0,
    include_paths: bool = True,
) -> str:
    lines = label_text_html(label, scale, include_paths)
    embedded_entries = normalize_embedded_entries(embedded_ends)
    if embedded_entries:
        main_rows = "\n".join(f"<TR><TD>{line}</TD></TR>" for line in lines)
        embedded_rows = embedded_rows_html(embedded_entries, scale)
        body = (
            "<TABLE BORDER=\"0\" CELLBORDER=\"0\" CELLSPACING=\"0\" CELLPADDING=\"0\">"
            f"{main_rows}"
            f"<TR><TD>{embedded_rows}</TD></TR>"
            "</TABLE>"
        )
    else:
        body = "<BR/>".join(lines)

    return f"<{body}>"


def architecture_irrelevant_bucket_label(labels: list[str], scale: float = 1.0) -> str:
    rows = [
        f'<TR><TD><FONT POINT-SIZE="{scaled(VERSION_POINT_SIZE, scale)}"><B>irrelevant to architecture</B></FONT></TD></TR>'
    ]
    for label in sorted(labels, key=crate_name):
        rows.append(f"<TR><TD>{compact_html_label(label, scale)}</TD></TR>")
    return (
        f'<<TABLE BORDER="1" CELLBORDER="0" CELLSPACING="0" CELLPADDING="{scaled(INNER_BUBBLE_PADDING, scale)}" '
        'COLOR="#b7bec9" STYLE="ROUNDED">'
        + "".join(rows)
        + "</TABLE>>"
    )


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


def nested_shared_leaf_parents(
    incoming_parents: dict[str, set[str]],
    children_by_parent: dict[str, set[str]],
    leaves: set[str],
) -> dict[str, tuple[str, str]]:
    nested: dict[str, tuple[str, str]] = {}
    for leaf in sorted(leaves):
        parents = sorted(incoming_parents.get(leaf, ()))
        if len(parents) != 2:
            continue
        parent_a, parent_b = parents
        if parent_b in children_by_parent.get(parent_a, ()):
            nested[leaf] = (parent_a, parent_b)
        elif parent_a in children_by_parent.get(parent_b, ()):
            nested[leaf] = (parent_b, parent_a)
    return nested


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
        f'  graph [rankdir=LR, bgcolor="white", overlap=false, splines=true, nodesep={SPLIT_GRAPH_NODESEP}, ranksep={SPLIT_GRAPH_RANKSEP}];',
        f'  node [shape=box, style="rounded,filled", fontname="Inter,Arial", fontsize=10, margin="{NODE_MARGIN}", color="#8b9bb0", fillcolor="#f7f9fb", fontcolor="#172033"];',
        f'  edge [color="#9aa8b8", arrowsize=0.55, penwidth={GRAPH_EDGE_PEN_WIDTH}];',
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
    children_by_parent: dict[str, set[str]] = defaultdict(set)
    for parent, child in internal_edges:
        internal_in_count[child] += 1
        internal_out_count[parent] += 1
        internal_parent_by_leaf[child] = parent
        internal_parents[child].add(parent)
        children_by_parent[parent].add(child)

    connector_in_count: dict[str, int] = defaultdict(int)
    connector_out_count: dict[str, int] = defaultdict(int)
    for _parent, child in incoming:
        connector_in_count[child] += 1
    for parent, _child in outgoing:
        connector_out_count[parent] += 1

    one_input_leaves = {
        node
        for node in owned_nodes
        if node != image_root
        and internal_in_count[node] == 1
        and internal_out_count[node] == 0
        and connector_in_count[node] == 0
        and connector_out_count[node] == 0
    }
    two_input_leaf_candidates = {
        node
        for node in owned_nodes
        if node != image_root
        and internal_in_count[node] == 2
        and internal_out_count[node] == 0
        and connector_in_count[node] == 0
        and connector_out_count[node] == 0
    }
    nested_two_input = nested_shared_leaf_parents(
        internal_parents, children_by_parent, two_input_leaf_candidates
    )
    nested_two_input_leaves = set(nested_two_input)
    nested_two_input_outer_parents = {outer for outer, _inner in nested_two_input.values()}
    two_input_leaves = two_input_leaf_candidates - nested_two_input_leaves
    collapsed_leaves = set(one_input_leaves) | nested_two_input_leaves

    changed = True
    while changed:
        changed = False
        for node in sorted(owned_nodes):
            if node == image_root or node in collapsed_leaves or internal_in_count[node] != 1:
                continue
            if node in nested_two_input_outer_parents:
                continue
            parent = internal_parent_by_leaf.get(node)
            if parent == image_root and internal_out_count[node] != 0:
                continue
            if connector_in_count[node] != 0 or connector_out_count[node] != 0:
                continue
            if all(child in collapsed_leaves for child in children_by_parent.get(node, ())):
                collapsed_leaves.add(node)
                changed = True

    def embedded_entry(label: str) -> EmbeddedEntry:
        children = [
            embedded_entry(child)
            for child in sorted(children_by_parent.get(label, ()))
            if child in collapsed_leaves
            and not (child in nested_two_input and nested_two_input[child][0] == label)
        ]
        return EmbeddedEntry(label, children=children)

    embedded_by_parent: dict[str, list[EmbeddedEntry]] = defaultdict(list)
    for node in sorted(collapsed_leaves):
        if internal_in_count[node] != 1:
            continue
        parent = internal_parent_by_leaf[node]
        if parent not in collapsed_leaves:
            embedded_by_parent[parent].append(embedded_entry(node))
    for parent, entries in embedded_by_parent.items():
        embedded_by_parent[parent] = normalize_embedded_entries(entries)

    visible_nodes = owned_nodes - collapsed_leaves

    for label in sorted(visible_nodes, key=lambda x: (x != image_root, x)):
        fill, color = node_style(label)
        if label == image_root:
            fill, color = "#dff3ff", "#1b78a6"
        embedded_ends = embedded_by_parent.get(label, [])
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
    lines.append(f"  // nested two-input leaves: {len(nested_two_input_leaves)}")
    lines.append("}")
    return "\n".join(lines) + "\n"


def split_graph_layout(
    image_root: str,
    owned_nodes: set[str],
    edges: set[tuple[str, str]],
    incoming: list[tuple[str, str]],
    outgoing: list[tuple[str, str]],
) -> tuple[dict[str, str], dict[str, list[EmbeddedEntry]], dict[str, tuple[str, str]]]:
    internal_edges = {
        (parent, child)
        for parent, child in edges
        if parent in owned_nodes and child in owned_nodes
    }
    internal_in_count: dict[str, int] = defaultdict(int)
    internal_out_count: dict[str, int] = defaultdict(int)
    internal_parent_by_leaf: dict[str, str] = {}
    internal_parents: dict[str, set[str]] = defaultdict(set)
    children_by_parent: dict[str, set[str]] = defaultdict(set)
    for parent, child in internal_edges:
        internal_in_count[child] += 1
        internal_out_count[parent] += 1
        internal_parent_by_leaf[child] = parent
        internal_parents[child].add(parent)
        children_by_parent[parent].add(child)

    connector_in_count: dict[str, int] = defaultdict(int)
    connector_out_count: dict[str, int] = defaultdict(int)
    for _parent, child in incoming:
        connector_in_count[child] += 1
    for parent, _child in outgoing:
        connector_out_count[parent] += 1

    one_input_leaves = {
        node
        for node in owned_nodes
        if node != image_root
        and internal_in_count[node] == 1
        and internal_out_count[node] == 0
        and connector_in_count[node] == 0
        and connector_out_count[node] == 0
    }
    two_input_leaf_candidates = {
        node
        for node in owned_nodes
        if node != image_root
        and internal_in_count[node] == 2
        and internal_out_count[node] == 0
        and connector_in_count[node] == 0
        and connector_out_count[node] == 0
    }
    nested_two_input = nested_shared_leaf_parents(
        internal_parents, children_by_parent, two_input_leaf_candidates
    )
    nested_two_input_outer_parents = {outer for outer, _inner in nested_two_input.values()}
    collapsed_leaves = set(one_input_leaves) | set(nested_two_input)

    changed = True
    while changed:
        changed = False
        for node in sorted(owned_nodes):
            if node == image_root or node in collapsed_leaves or internal_in_count[node] != 1:
                continue
            if node in nested_two_input_outer_parents:
                continue
            parent = internal_parent_by_leaf.get(node)
            if parent == image_root and internal_out_count[node] != 0:
                continue
            if connector_in_count[node] != 0 or connector_out_count[node] != 0:
                continue
            if all(child in collapsed_leaves for child in children_by_parent.get(node, ())):
                collapsed_leaves.add(node)
                changed = True

    def embedded_entry(label: str) -> EmbeddedEntry:
        children = [
            embedded_entry(child)
            for child in sorted(children_by_parent.get(label, ()))
            if child in collapsed_leaves
            and not (child in nested_two_input and nested_two_input[child][0] == label)
        ]
        return EmbeddedEntry(label, children=children)

    embedded_by_parent: dict[str, list[EmbeddedEntry]] = defaultdict(list)
    for node in sorted(collapsed_leaves):
        if internal_in_count[node] != 1:
            continue
        parent = internal_parent_by_leaf[node]
        if parent not in collapsed_leaves:
            embedded_by_parent[parent].append(embedded_entry(node))
    for parent, entries in embedded_by_parent.items():
        embedded_by_parent[parent] = normalize_embedded_entries(entries)

    visible_nodes = owned_nodes - collapsed_leaves
    node_ids = {
        label: f"n{index}"
        for index, label in enumerate(sorted(visible_nodes, key=lambda x: (x != image_root, x)))
    }
    return node_ids, embedded_by_parent, nested_two_input


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

    shared_input_leaf_candidates = {
        node
        for node in all_nodes
        if node != root
        and incoming_count[node] == 2
        and outgoing_count[node] == 0
    }
    nested_shared_leaves = nested_shared_leaf_parents(
        incoming_parents, children_by_parent, shared_input_leaf_candidates
    )
    nested_shared_outer_parents = {outer for outer, _inner in nested_shared_leaves.values()}
    shared_input_leaves = shared_input_leaf_candidates - set(nested_shared_leaves)
    collapsed_leaves = set(shared_input_leaves) | set(nested_shared_leaves)
    changed = True
    while changed:
        changed = False
        for node in sorted(all_nodes):
            if node == root or node in collapsed_leaves or incoming_count[node] != 1:
                continue
            if node in nested_shared_outer_parents:
                continue
            parent = next(iter(incoming_parents[node]))
            if parent == root and outgoing_count[node] != 0:
                continue
            if any(child in shared_input_leaves for child in children_by_parent.get(node, ())):
                continue
            if all(child in collapsed_leaves for child in children_by_parent.get(node, ())):
                collapsed_leaves.add(node)
                changed = True

    attached_nodes = {
        node
        for node in all_nodes
        if node != root
        and node not in collapsed_leaves
        and incoming_count[node] == 1
        and next(iter(incoming_parents[node])) not in collapsed_leaves
        and crate_name(next(iter(incoming_parents[node]))) not in SKIP_ATTACHED_CHILDREN_OF_CRATES
        and not any(child in collapsed_leaves for child in children_by_parent.get(node, ()))
    }

    def embedded_entry(label: str) -> EmbeddedEntry:
        children = [
            embedded_entry(child)
            for child in sorted(children_by_parent.get(label, ()))
            if child in collapsed_leaves
            and not (child in nested_shared_leaves and nested_shared_leaves[child][0] == label)
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
        attached_nodes,
    )


def render_full_dot(
    root: str,
    edges: set[tuple[str, str]],
    architecture_irrelevant: list[str] | None = None,
    advanced_parent_nodes: set[str] | None = None,
    advanced_parent_anchors: dict[str, str] | None = None,
) -> str:
    advanced_parent_nodes = advanced_parent_nodes or set()
    advanced_parent_anchors = advanced_parent_anchors or {}
    node_ids: dict[str, str] = {}
    (
        all_nodes,
        incoming_count,
        outgoing_count,
        incoming_parents,
        collapsed_leaves,
        embedded_by_parent,
        shared_input_leaves,
        attached_nodes,
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
    base_left_wing_depth_by_node = left_wing_depths(left_wing_nodes, edges)
    left_wing_depth_by_node = {
        node: max(0, depth - 1) if node in advanced_parent_nodes else depth
        for node, depth in base_left_wing_depth_by_node.items()
    }
    def node_id(label: str) -> str:
        if label not in node_ids:
            node_ids[label] = f"n{len(node_ids)}"
        return node_ids[label]

    lines = [
        "digraph trueos_depth_graph {",
        f'  graph [rankdir=LR, bgcolor="white", overlap=false, splines=true, nodesep={FULL_GRAPH_NODESEP}, ranksep={FULL_GRAPH_RANKSEP}];',
        f'  node [shape=box, style="rounded,filled", fontname="Inter,Arial", fontsize=10, margin="{NODE_MARGIN}", color="#8b9bb0", fillcolor="#f7f9fb", fontcolor="#172033"];',
        f'  edge [color="#9aa8b8", arrowsize=0.55, penwidth={GRAPH_EDGE_PEN_WIDTH}];',
    ]

    for label in sorted(visible_nodes, key=lambda x: (x != root, x)):
        fill, color = node_style(label)
        if label == root:
            fill, color = "#dff3ff", "#1b78a6"
        embedded_ends = embedded_by_parent.get(label, [])
        label_scale = (
            FULL_GRAPH_ROOT_SCALE
            if label == root or crate_name(label) in FULL_GRAPH_ROOT_SCALE_CRATES
            else FULL_GRAPH_LABEL_SCALE
        )
        margin_attr = f', margin="{scaled_node_margin(label_scale)}"'
        lines.append(
            f'  {node_id(label)} [label={html_label(label, embedded_ends, label_scale, label != root)}, fillcolor="{fill}", color="{color}"{margin_attr}];'
        )
    for parent, child in sorted(edges):
        if child in attached_nodes:
            lines.append(f"  {node_id(parent)} -> {node_id(child)} [style=invis, constraint=false];")
            continue
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

    for label in sorted(advanced_parent_nodes - left_wing_nodes):
        if label not in visible_nodes or label == root:
            continue
        anchor = advanced_parent_anchors.get(label)
        if anchor in visible_nodes and anchor != label:
            lines.append(
                f"  {node_id(anchor)} -> {node_id(label)} "
                "[style=invis, weight=900, minlen=2];"
            )
        else:
            lines.append(f"  {node_id(root)} -> {node_id(label)} [style=invis, weight=500, minlen=2];")

    if left_wing_nodes:
        max_left_depth = max(left_wing_depth_by_node.values(), default=0)
        for depth in range(max_left_depth + 1):
            lines.append(
                f'  left_column_{depth} [label="", shape=point, width=0.01, height=0.01, '
                'style=invis];'
            )
        column_chain = " -> ".join(
            [f"left_column_{depth}" for depth in range(max_left_depth, -1, -1)]
            + [node_id(root)]
        )
        lines.append(f"  {column_chain} [style=invis, weight=1000];")
        for depth in range(max_left_depth + 1):
            column_nodes = [
                node
                for node in sorted(left_wing_nodes)
                if left_wing_depth_by_node.get(node, 0) == depth
            ]
            rank_members = "; ".join(
                [f"left_column_{depth}", *(node_id(node) for node in column_nodes)]
            )
            lines.append("  {")
            lines.append("    rank=same;")
            lines.append(f"    {rank_members};")
            lines.append("  }")
        for parent, child in sorted(edges):
            if parent in left_wing_nodes and child in left_wing_nodes:
                lines.append(
                    f"  {node_id(child)} -> {node_id(parent)} "
                    '[style=invis, weight=300, minlen=1];'
                )

    if architecture_irrelevant:
        bucket_id = "architecture_irrelevant"
        lines.append(
            f'  {bucket_id} [label={architecture_irrelevant_bucket_label(architecture_irrelevant, FULL_GRAPH_LABEL_SCALE)}, '
            'shape=plain, fontcolor="#334155"];'
        )

    lines.append(f"  // collapsed leaf ends: {len(collapsed_leaves)}")
    lines.append(f"  // shared two-input leaf ends: {len(shared_input_leaves)}")
    lines.append(f"  // parent-attached one-input nodes: {len(attached_nodes)}")
    lines.append(f"  // left-wing root-shared leaf ends: {len(left_input_leaves)}")
    lines.append(f"  // pinned left-wing crates: {len(left_wing_nodes)}")
    lines.append(f"  // architecture-irrelevant bucket entries: {len(architecture_irrelevant or [])}")
    lines.append("}")
    return "\n".join(lines) + "\n"


def full_graph_layout(
    root: str, edges: set[tuple[str, str]]
) -> tuple[dict[str, str], set[str], dict[str, list[EmbeddedEntry]], set[str], dict[str, set[str]]]:
    (
        all_nodes,
        _incoming_count,
        outgoing_count,
        incoming_parents,
        collapsed_leaves,
        embedded_by_parent,
        shared_input_leaves,
        _attached_nodes,
    ) = full_collapse_info(root, edges)
    visible_nodes = all_nodes - collapsed_leaves
    node_ids: dict[str, str] = {}
    for label in sorted(visible_nodes, key=lambda x: (x != root, x)):
        node_ids[label] = f"n{len(node_ids)}"
    return node_ids, collapsed_leaves, embedded_by_parent, shared_input_leaves, incoming_parents


def full_nested_shared_leaf_parents(root: str, edges: set[tuple[str, str]]) -> dict[str, tuple[str, str]]:
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

    shared_input_leaf_candidates = {
        node
        for node in all_nodes
        if node != root
        and incoming_count[node] == 2
        and outgoing_count[node] == 0
    }
    return nested_shared_leaf_parents(
        incoming_parents, children_by_parent, shared_input_leaf_candidates
    )


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


def widen_full_svg_to_aspect() -> None:
    svg = FULL_SVG_PATH.read_text()
    viewbox = re.search(r'viewBox="0\.00 0\.00 ([0-9.]+) ([0-9.]+)"', svg)
    if not viewbox:
        raise RuntimeError(f"could not parse viewBox from {FULL_SVG_PATH}")
    width, height = float(viewbox.group(1)), float(viewbox.group(2))
    target_width = width
    target_height = height
    if width / height < FULL_GRAPH_ASPECT_RATIO:
        target_width = height * FULL_GRAPH_ASPECT_RATIO
    elif width / height > FULL_GRAPH_ASPECT_RATIO:
        target_height = width / FULL_GRAPH_ASPECT_RATIO
    if abs(target_width - width) <= 0.01 and abs(target_height - height) <= 0.01:
        return

    svg = re.sub(
        r'<svg width="[^"]+" height="[^"]+"',
        f'<svg width="{target_width:.0f}pt" height="{target_height:.0f}pt"',
        svg,
        count=1,
    )
    svg = re.sub(
        r'viewBox="0\.00 0\.00 [0-9.]+ [0-9.]+"',
        f'viewBox="0.00 0.00 {target_width:.2f} {target_height:.2f}"',
        svg,
        count=1,
    )

    def pad_background(match: re.Match[str]) -> str:
        points = match.group(2)
        pairs = re.findall(r"(-?[0-9.]+),(-?[0-9.]+)", points)
        if not pairs:
            return match.group(0)
        xs = [float(x) for x, _y in pairs]
        ys = [float(y) for _x, y in pairs]
        old_max_x = max(xs)
        old_max_y = max(ys)
        new_max_x = old_max_x + (target_width - width)
        new_max_y = old_max_y + (target_height - height)
        rewritten = []
        for x_text, y_text in pairs:
            x = float(x_text)
            y = float(y_text)
            if abs(x - old_max_x) < 0.01:
                x_text = f"{new_max_x:.2f}"
            if abs(y - old_max_y) < 0.01:
                y_text = f"{new_max_y:.2f}"
            rewritten.append(f"{x_text},{y_text}")
        return match.group(1) + " ".join(rewritten) + match.group(3)

    svg = re.sub(
        r'(<polygon fill="white" stroke="none" points=")([^"]+)(")',
        pad_background,
        svg,
        count=1,
    )
    FULL_SVG_PATH.write_text(svg)


def move_architecture_irrelevant_bucket_to_top_left() -> None:
    svg = FULL_SVG_PATH.read_text()
    transform = re.search(
        r'<g id="graph0"[^>]*transform="[^"]*translate\(([0-9.-]+) ([0-9.-]+)\)',
        svg,
    )
    if not transform:
        raise RuntimeError(f"could not parse graph transform from {FULL_SVG_PATH}")
    tx, ty = float(transform.group(1)), float(transform.group(2))

    group = re.search(
        r'(<g id="node\d+" class="node">[\s\n]*<title>architecture_irrelevant</title>)(.*?)(</g>)',
        svg,
        re.S,
    )
    if not group:
        return

    path_values = re.findall(r'<path\b[^>]*\bd="([^"]+)"', group.group(2))
    if not path_values:
        return
    boxes = [svg_path_bbox(path) for path in path_values]
    min_x = min(box[0] for box in boxes)
    min_y = min(box[1] for box in boxes)
    target_x = ARCHITECTURE_BUCKET_PADDING - tx
    target_y = ARCHITECTURE_BUCKET_PADDING - ty
    dx = target_x - min_x
    dy = target_y - min_y
    opening = group.group(1).replace(
        ' class="node"',
        f' class="node" transform="translate({dx:.2f} {dy:.2f})"',
        1,
    )
    replacement = f"{opening}{group.group(2)}{group.group(3)}"
    svg = svg[: group.start()] + replacement + svg[group.end() :]
    FULL_SVG_PATH.write_text(svg)


def extract_balanced_svg_group(svg: str, start: int) -> tuple[str, int]:
    depth = 0
    pos = start
    while True:
        next_open = svg.find("<g", pos)
        next_close = svg.find("</g>", pos)
        if next_close == -1:
            raise RuntimeError(f"could not find closing group in {FULL_SVG_PATH}")
        if next_open != -1 and next_open < next_close:
            depth += 1
            tag_end = svg.find(">", next_open)
            if tag_end == -1:
                raise RuntimeError(f"could not parse group in {FULL_SVG_PATH}")
            pos = tag_end + 1
            continue
        depth -= 1
        end = next_close + len("</g>")
        if end < len(svg) and svg[end] == "\n":
            end += 1
        if depth == 0:
            return svg[start:end], end
        pos = end


def layer_full_svg_edges_below_nodes() -> None:
    svg = FULL_SVG_PATH.read_text()

    edge_groups = re.findall(r'<g id="edge\d+" class="edge">.*?</g>\n?', svg, re.S)
    if edge_groups:
        svg = re.sub(r'<g id="edge\d+" class="edge">.*?</g>\n?', "", svg, flags=re.S)

    shared_group = ""
    shared_start = svg.find('<g id="shared-leaf-links"')
    if shared_start != -1:
        shared_group, shared_end = extract_balanced_svg_group(svg, shared_start)
        svg = svg[:shared_start] + svg[shared_end:]

    edge_markup = "".join(edge_groups)
    if not edge_markup and not shared_group:
        FULL_SVG_PATH.write_text(svg)
        return

    region_match = re.search(
        r'<g id="ownership-regions" class="ownership-regions">.*?</g>\n',
        svg,
        re.S,
    )
    if region_match:
        insert_at = region_match.end()
    else:
        background_match = re.search(
            r'<polygon fill="white" stroke="none" points="[^"]+"/>\n',
            svg,
        )
        if not background_match:
            raise RuntimeError(f"could not find SVG background in {FULL_SVG_PATH}")
        insert_at = background_match.end()

    svg = svg[:insert_at] + edge_markup + svg[insert_at:]

    if shared_group:
        graph_end = svg.rfind("</g>\n</svg>")
        if graph_end == -1:
            raise RuntimeError(f"could not find graph end in {FULL_SVG_PATH}")
        svg = svg[:graph_end] + shared_group + svg[graph_end:]
    FULL_SVG_PATH.write_text(svg)


def center_full_svg_horizontally() -> None:
    width, _height, _tx, _ty, node_bboxes, _inner_bboxes, _bodies = svg_layout(FULL_SVG_PATH)
    if not node_bboxes:
        return
    left = min(x for x, _y, _w, _h in node_bboxes.values())
    right = max(x + w for x, _y, w, _h in node_bboxes.values())
    target_left = (width - (right - left)) / 2
    dx = target_left - left
    if abs(dx) <= 0.01:
        return

    svg = FULL_SVG_PATH.read_text()
    transform = re.search(
        r'(<g id="graph0"[^>]*transform="[^"]*translate\()([0-9.-]+)( )([0-9.-]+)(\)[^"]*")',
        svg,
    )
    if not transform:
        raise RuntimeError(f"could not parse graph transform from {FULL_SVG_PATH}")
    next_tx = float(transform.group(2)) + dx
    svg = svg[: transform.start()] + (
        f"{transform.group(1)}{next_tx:.2f}{transform.group(3)}{transform.group(4)}{transform.group(5)}"
    ) + svg[transform.end() :]

    def keep_background_in_place(match: re.Match[str]) -> str:
        points = match.group(2)

        def shift_x(point: re.Match[str]) -> str:
            return f"{float(point.group(1)) - dx:.2f},{point.group(2)}"

        return match.group(1) + re.sub(
            r"(-?[0-9.]+),(-?[0-9.]+)",
            shift_x,
            points,
        ) + match.group(3)

    svg = re.sub(
        r'(<polygon fill="white" stroke="none" points=")([^"]+)(")',
        keep_background_in_place,
        svg,
        count=1,
    )
    FULL_SVG_PATH.write_text(svg)


def svg_layout(
    svg_path: Path,
) -> tuple[
    float,
    float,
    float,
    float,
    dict[str, tuple[float, float, float, float]],
    dict[str, list[tuple[float, float, float, float]]],
    dict[str, str],
]:
    svg = svg_path.read_text()
    viewbox = re.search(r'viewBox="[^"]*?[^0-9.]([0-9.]+) ([0-9.]+)"', svg)
    if not viewbox:
        raise RuntimeError(f"could not parse viewBox from {svg_path}")
    width, height = float(viewbox.group(1)), float(viewbox.group(2))

    transform = re.search(r'<g id="graph0"[^>]*transform="[^"]*translate\(([0-9.-]+) ([0-9.-]+)\)', svg)
    if not transform:
        raise RuntimeError(f"could not parse graph transform from {svg_path}")
    tx, ty = float(transform.group(1)), float(transform.group(2))

    bboxes: dict[str, tuple[float, float, float, float]] = {}
    inner_bboxes: dict[str, list[tuple[float, float, float, float]]] = {}
    bodies: dict[str, str] = {}
    node_group_pattern = (
        r'<g id="node\d+" class="node"'
        r'(?: transform="translate\(([0-9.-]+) ([0-9.-]+)\)")?>(.*?)</g>'
    )
    for group in re.finditer(node_group_pattern, svg, re.S):
        node_tx = float(group.group(1) or 0.0)
        node_ty = float(group.group(2) or 0.0)
        body = group.group(3)
        title = re.search(r"<title>(n\d+)</title>", body)
        if not title:
            continue
        node_id = title.group(1)
        bodies[node_id] = body
        paths = re.findall(r'<path\b[^>]*\bd="([^"]+)"', body)
        if not paths:
            continue
        bboxes[node_id] = transform_bbox(svg_path_bbox(paths[0]), tx + node_tx, ty + node_ty)
        inner_bboxes[node_id] = [
            transform_bbox(svg_path_bbox(path), tx + node_tx, ty + node_ty)
            for path in paths[1:]
        ]

    return width, height, tx, ty, bboxes, inner_bboxes, bodies


def full_svg_bboxes() -> tuple[
    float,
    float,
    dict[str, tuple[float, float, float, float]],
    dict[str, list[tuple[float, float, float, float]]],
]:
    width, height, _tx, _ty, bboxes, inner_bboxes, _bodies = svg_layout(FULL_SVG_PATH)
    return width, height, bboxes, inner_bboxes


def shifted_text_markup(markup: str, dx: float, dy: float) -> str:
    def shift_attr(match: re.Match[str]) -> str:
        name = match.group(1)
        value = float(match.group(2))
        shifted = value + (dx if name == "x" else dy)
        return f'{name}="{shifted:.2f}"'

    return re.sub(r'\b(x|y)="([0-9.-]+)"', shift_attr, markup)


def text_markup_in_bbox(
    body: str,
    bbox: tuple[float, float, float, float],
    tx: float,
    ty: float,
) -> list[str]:
    x, y, w, h = bbox
    matches: list[str] = []
    for text in re.finditer(r'<text\b[^>]*\bx="([0-9.-]+)"\s+y="([0-9.-]+)"[^>]*>.*?</text>', body):
        text_x = float(text.group(1)) + tx
        text_y = float(text.group(2)) + ty
        if x - 2 <= text_x <= x + w + 2 and y - 4 <= text_y <= y + h + 4:
            matches.append(text.group(0))
    return matches


def inject_nested_shared_badges(
    svg_path: Path,
    node_ids: dict[str, str],
    embedded_by_parent: dict[str, list[EmbeddedEntry]],
    nested_leaf_parents: dict[str, tuple[str, str]],
) -> None:
    if not nested_leaf_parents:
        return

    svg = svg_path.read_text()
    svg = re.sub(
        r'<g id="nested-shared-leaf-badges" class="nested-shared-leaf-badges".*?</g>\n',
        "",
        svg,
        flags=re.S,
    )
    _width, _height, tx, ty, _node_bboxes, inner_bboxes, node_bodies = svg_layout(svg_path)
    badge_lines = [
        '<g id="nested-shared-leaf-badges" class="nested-shared-leaf-badges" pointer-events="none">'
    ]

    for leaf, (outer_parent, inner_parent) in sorted(nested_leaf_parents.items()):
        node_id = node_ids.get(outer_parent)
        if not node_id:
            continue
        entries = embedded_by_parent.get(outer_parent, [])
        labels = embedded_path_labels_postorder(entries)
        leaf_index = embedded_path_index_postorder(entries, leaf)
        inner_index = embedded_path_index_postorder(entries, inner_parent)
        boxes = inner_bboxes.get(node_id, [])
        if leaf_index is None or inner_index is None:
            continue
        if leaf_index >= len(boxes) or inner_index >= len(boxes):
            continue

        leaf_box = boxes[leaf_index]
        inner_box = boxes[inner_index]
        old_x, old_y, old_w, old_h = leaf_box
        target_x = inner_box[0] + inner_box[2] / 2 - old_w / 2
        target_y = inner_box[1] + inner_box[3] - old_h / 2
        dx = target_x - old_x
        dy = target_y - old_y
        if abs(dx) < 0.01 and abs(dy) < 0.01:
            continue

        body = node_bodies.get(node_id, "")
        texts = text_markup_in_bbox(body, leaf_box, tx, ty)
        if not texts:
            continue

        label = display_label(leaf).replace(r"\n", " / ")
        inner_label = display_label(inner_parent).replace(r"\n", " / ")
        rx = min(18.0, old_h / 2)
        badge_lines.extend(
            [
                f'<!-- nested shared leaf badge {escape(label)} on {escape(inner_label)} -->',
                "<g>",
                f"<title>nested shared leaf {escape(label)}</title>",
                (
                    f'<rect x="{old_x - tx - 1.25:.2f}" y="{old_y - ty - 1.25:.2f}" '
                    f'width="{old_w + 2.5:.2f}" height="{old_h + 2.5:.2f}" '
                    'fill="#f7f9fb" stroke="none"/>'
                ),
                (
                    f'<rect x="{target_x - tx:.2f}" y="{target_y - ty:.2f}" '
                    f'width="{old_w:.2f}" height="{old_h:.2f}" '
                    f'rx="{rx:.2f}" ry="{rx:.2f}" fill="#f7f9fb" '
                    f'stroke="#9aa8b8" stroke-opacity="1" stroke-width="{NESTED_SHARED_BADGE_STROKE_WIDTH}"/>'
                ),
                *(shifted_text_markup(text, dx, dy) for text in texts),
                "</g>",
            ]
        )

    badge_lines.append("</g>")
    if len(badge_lines) == 2:
        return

    graph_end = svg.rfind("</g>\n</svg>")
    if graph_end == -1:
        raise RuntimeError(f"could not find graph end in {svg_path}")
    svg = svg[:graph_end] + "\n".join(badge_lines) + "\n" + svg[graph_end:]
    svg_path.write_text(svg)


def ownership_regions(
    root: str, root_children: list[str], edges: set[tuple[str, str]]
) -> list[dict[str, object]]:
    return []
    adjacency: dict[str, set[str]] = defaultdict(set)
    for parent, child in edges:
        adjacency[parent].add(child)
    root_child_set = set(root_children)
    node_ids, _collapsed_leaves, _embedded_by_parent, _shared_input_leaves, _incoming_parents = (
        full_graph_layout(root, edges)
    )
    width, height, node_bboxes, _inner_bboxes = full_svg_bboxes()

    def region_for(prefix: str, label: str, class_name: str) -> dict[str, object] | None:
        root_label = next((child for child in root_children if child.startswith(prefix)), None)
        if not root_label:
            return None
        direct_local_children = {
            child for child in adjacency.get(root_label, set()) if child not in root_child_set
        }
        owned_nodes = {root_label, *direct_local_children}
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


def vertically_aligned_shared_leaves(root: str, edges: set[tuple[str, str]]) -> set[str]:
    node_ids, _collapsed_leaves, embedded_by_parent, shared_input_leaves, incoming_parents = (
        full_graph_layout(root, edges)
    )
    _width, _height, _node_bboxes, inner_bboxes = full_svg_bboxes()
    vertical_leaves: set[str] = set()

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
        if points and abs(points[0] - points[2]) <= SHARED_LINK_VERTICAL_THRESHOLD:
            vertical_leaves.add(leaf)

    return vertical_leaves


def shared_leaf_parent_advances(
    root: str,
    edges: set[tuple[str, str]],
    shared_leaves: set[str],
) -> tuple[set[str], dict[str, str]]:
    if not shared_leaves:
        return set(), {}

    node_ids, _collapsed_leaves, embedded_by_parent, _shared_input_leaves, incoming_parents = (
        full_graph_layout(root, edges)
    )
    _width, _height, node_bboxes, inner_bboxes = full_svg_bboxes()
    advanced_parents: set[str] = set()
    advanced_anchors: dict[str, str] = {}
    for leaf in sorted(shared_leaves):
        candidates: list[tuple[float, str, str]] = []
        for parent in sorted(incoming_parents.get(leaf, set())):
            node_id = node_ids.get(parent)
            if not node_id:
                continue
            entries = embedded_by_parent.get(parent, [])
            index = direct_embedded_path_index(entries, leaf)
            boxes = inner_bboxes.get(node_id, [])
            if index is not None and index < len(boxes):
                x, y, w, h = boxes[index]
            elif node_id in node_bboxes:
                x, y, w, h = node_bboxes[node_id]
            else:
                continue
            candidates.append((y + h / 2, display_label(parent), parent))
        if candidates:
            ordered = sorted(candidates)
            _center_y, _display, parent = ordered[-1]
            advanced_parents.add(parent)
            if len(ordered) > 1:
                advanced_anchors[parent] = ordered[0][2]
    return advanced_parents, advanced_anchors


def shifted_svg_path_endpoint(
    path_d: str,
    tail_delta: tuple[float, float],
    head_delta: tuple[float, float],
) -> str:
    point_pattern = r"(-?[0-9]+(?:\.[0-9]+)?),(-?[0-9]+(?:\.[0-9]+)?)"
    points = list(re.finditer(point_pattern, path_d))
    if not points:
        return path_d

    shifts: dict[int, tuple[float, float]] = {}
    if abs(tail_delta[0]) > 0.01 or abs(tail_delta[1]) > 0.01:
        shifts[0] = tail_delta
        if len(points) > 1:
            shifts[1] = tail_delta
    if abs(head_delta[0]) > 0.01 or abs(head_delta[1]) > 0.01:
        shifts[len(points) - 1] = head_delta
        if len(points) > 1:
            shifts[len(points) - 2] = head_delta
    if not shifts:
        return path_d

    point_index = -1

    def shift_point(match: re.Match[str]) -> str:
        nonlocal point_index
        point_index += 1
        delta = shifts.get(point_index)
        if delta is None:
            return match.group(0)
        dx, dy = delta
        return f"{float(match.group(1)) + dx:.2f},{float(match.group(2)) + dy:.2f}"

    return re.sub(point_pattern, shift_point, path_d)


def shifted_svg_points(points: str, delta: tuple[float, float]) -> str:
    dx, dy = delta
    if abs(dx) <= 0.01 and abs(dy) <= 0.01:
        return points

    def shift_point(match: re.Match[str]) -> str:
        return f"{float(match.group(1)) + dx:.2f},{float(match.group(2)) + dy:.2f}"

    return re.sub(r"(-?[0-9]+(?:\.[0-9]+)?),(-?[0-9]+(?:\.[0-9]+)?)", shift_point, points)


def node_id_from_edge_title_endpoint(endpoint: str) -> str:
    return endpoint.split(":", 1)[0]


def downstream_nodes(start: str, edges: set[tuple[str, str]]) -> set[str]:
    children_by_parent: dict[str, set[str]] = defaultdict(set)
    for parent, child in edges:
        children_by_parent[parent].add(child)

    seen = {start}
    stack = [start]
    while stack:
        parent = stack.pop()
        for child in children_by_parent.get(parent, ()):
            if child in seen:
                continue
            seen.add(child)
            stack.append(child)
    return seen


def selected_arrow_has_rendered_edges(
    root: str,
    selected_root: str,
    selected_nodes: set[str],
    rendered_edges: set[tuple[str, str]],
) -> bool:
    if selected_root == root:
        return bool(rendered_edges)
    return any(
        parent in selected_nodes or (parent == root and child == selected_root)
        for parent, child in rendered_edges
    )


def inline_full_svg_markup(node_ids: dict[str, str]) -> str:
    svg = FULL_SVG_PATH.read_text()
    outer = re.search(r"<svg\b[^>]*>(.*)</svg>\s*$", svg, re.S)
    if not outer:
        raise RuntimeError(f"could not parse inline SVG body from {FULL_SVG_PATH}")

    label_by_node_id = {node_id: label for label, node_id in node_ids.items()}

    def annotate_edge(match: re.Match[str]) -> str:
        edge_group = match.group(0)
        title = re.search(r"<title>([^<]+?)&#45;&gt;([^<]+?)</title>", edge_group)
        if not title:
            return edge_group
        tail_node = node_id_from_edge_title_endpoint(title.group(1))
        head_node = node_id_from_edge_title_endpoint(title.group(2))
        tail_label = label_by_node_id.get(tail_node)
        head_label = label_by_node_id.get(head_node)
        if not tail_label or not head_label:
            return edge_group

        start = re.search(r'<g id="edge\d+" class="edge"', edge_group)
        if not start:
            return edge_group
        annotated_start = (
            start.group(0).replace('class="edge"', 'class="edge graph-edge"')
            + f' data-tail="{escape(tail_label, quote=True)}"'
            + f' data-head="{escape(head_label, quote=True)}"'
        )
        return edge_group[: start.start()] + annotated_start + edge_group[start.end() :]

    return re.sub(r'<g id="edge\d+" class="edge">.*?</g>\n?', annotate_edge, outer.group(1), flags=re.S)


def move_attached_nodes_to_parents(root: str, edges: set[tuple[str, str]]) -> None:
    (
        _all_nodes,
        _incoming_count,
        outgoing_count,
        incoming_parents,
        _collapsed_leaves,
        _embedded_by_parent,
        _shared_input_leaves,
        attached_nodes,
    ) = full_collapse_info(root, edges)
    if not attached_nodes:
        return

    node_ids, _collapsed_leaves, _embedded_by_parent, _shared_input_leaves, _incoming_parents = (
        full_graph_layout(root, edges)
    )
    _width, _height, tx, ty, node_bboxes, _inner_bboxes, _bodies = svg_layout(FULL_SVG_PATH)
    gap = float(scaled(INNER_BUBBLE_SPACING, FULL_GRAPH_ROOT_SCALE))
    node_delta: dict[str, tuple[float, float]] = {}
    attached_by_parent: dict[str, list[tuple[str, str, tuple[float, float, float, float]]]] = defaultdict(list)
    for label in attached_nodes:
        parent = next(iter(incoming_parents[label]))
        if (
            parent in node_ids
            and node_ids[parent] in node_bboxes
            and label in node_ids
            and node_ids[label] in node_bboxes
        ):
            attached_by_parent[parent].append((label, node_ids[label], node_bboxes[node_ids[label]]))

    def place_stack(
        stack: list[tuple[str, str, tuple[float, float, float, float]]],
        parent_box: tuple[float, float, float, float],
        side: str,
    ) -> None:
        if not stack:
            return
        parent_x, parent_y, parent_w, parent_h = parent_box
        columns: list[list[tuple[str, str, tuple[float, float, float, float]]]] = []
        column_heights: list[float] = []
        for item in sorted(stack, key=lambda item: (item[2][1], display_label(item[0]))):
            h = item[2][3]
            extra_h = h if not columns or not columns[-1] else h + gap
            if columns and column_heights[-1] + extra_h > parent_h:
                columns.append([item])
                column_heights.append(h)
            else:
                if not columns:
                    columns.append([])
                    column_heights.append(0.0)
                columns[-1].append(item)
                column_heights[-1] += extra_h

        previous_left = parent_x
        previous_right = parent_x + parent_w
        for column in columns:
            column_h = sum(box[3] for _label, _node_id, box in column) + gap * (len(column) - 1)
            next_y = parent_y + max(0.0, (parent_h - column_h) / 2)
            column_w = max(box[2] for _label, _node_id, box in column)
            if side == "left":
                target_column_x = previous_left - gap - column_w
                previous_left = target_column_x
            else:
                target_column_x = previous_right + gap
                previous_right = target_column_x + column_w
            for _label, node_id, box in column:
                x, _y, w, h = box
                target_x = (
                    target_column_x + (column_w - w)
                    if side == "left"
                    else target_column_x
                )
                node_delta[node_id] = (target_x - x, next_y - box[1])
                next_y += h + gap

    root_id = node_ids.get(root)
    root_attached = attached_by_parent.pop(root, [])
    top_attached: list[tuple[str, str, tuple[float, float, float, float]]] = []
    if root_id and root_id in node_bboxes and root_attached:
        root_box = node_bboxes[root_id]
        left_attached = [
            item for item in root_attached if crate_name(item[0]) in LEFT_WING_CRATES
        ]
        top_attached = [
            item
            for item in root_attached
            if crate_name(item[0]) not in LEFT_WING_CRATES and outgoing_count[item[0]] >= 8
        ]
        right_attached = [
            item
            for item in root_attached
            if crate_name(item[0]) not in LEFT_WING_CRATES and item not in top_attached
        ]

        place_stack(left_attached, root_box, "left")
        place_stack(right_attached, root_box, "right")

        if top_attached:
            root_x, root_y, root_w, _root_h = root_box
            ordered_top = sorted(top_attached, key=lambda item: (item[2][0], display_label(item[0])))
            total_w = sum(box[2] for _label, _node_id, box in ordered_top) + gap * (len(ordered_top) - 1)
            next_x = root_x + (root_w - total_w) / 2
            top_h = max(box[3] for _label, _node_id, box in ordered_top)
            target_y = root_y - gap - top_h
            for _label, node_id, box in ordered_top:
                x, y, w, h = box
                node_delta[node_id] = (next_x - x, target_y + (top_h - h) - y)
                next_x += w + gap

    def moved_node_box(label: str) -> tuple[float, float, float, float]:
        node_id = node_ids[label]
        x, y, w, h = node_bboxes[node_id]
        dx, dy = node_delta.get(node_id, (0.0, 0.0))
        return x + dx, y + dy, w, h

    placed_parents: set[str] = set()
    visiting_parents: set[str] = set()

    def place_attached_children(parent: str) -> None:
        if parent in placed_parents or parent in visiting_parents:
            return
        visiting_parents.add(parent)
        if parent in attached_nodes:
            upstream_parent = next(iter(incoming_parents[parent]))
            if upstream_parent in attached_by_parent:
                place_attached_children(upstream_parent)
        if parent in attached_by_parent and parent in node_ids:
            side = "left" if crate_name(parent) in LEFT_SIDE_ATTACHED_PARENT_CRATES else "right"
            place_stack(attached_by_parent[parent], moved_node_box(parent), side)
        visiting_parents.remove(parent)
        placed_parents.add(parent)

    for parent in sorted(attached_by_parent, key=display_label):
        place_attached_children(parent)

    top_node_ids = {node_id for _label, node_id, _box in top_attached}
    moved_bboxes = {
        node_id: (x + dx, y + dy, w, h)
        for node_id, (x, y, w, h) in node_bboxes.items()
        for dx, dy in [node_delta.get(node_id, (0.0, 0.0))]
    }

    def replace_svg_path_points(
        path_d: str,
        replacements: dict[int, tuple[float, float]],
    ) -> str:
        point_pattern = r"(-?[0-9]+(?:\.[0-9]+)?),(-?[0-9]+(?:\.[0-9]+)?)"
        point_index = -1

        def replace_point(match: re.Match[str]) -> str:
            nonlocal point_index
            point_index += 1
            point = replacements.get(point_index)
            if point is None:
                return match.group(0)
            return f"{point[0]:.2f},{point[1]:.2f}"

        return re.sub(point_pattern, replace_point, path_d)

    def reroute_top_attached_tail(path_d: str, tail_node: str, head_node: str) -> str:
        tail_box = moved_bboxes.get(tail_node)
        head_box = moved_bboxes.get(head_node)
        if not tail_box or not head_box:
            return path_d
        tail_x, tail_y, tail_w, tail_h = tail_box
        head_x, head_y, head_w, head_h = head_box
        tail_cx = tail_x + tail_w / 2
        head_cx = head_x + head_w / 2
        head_cy = head_y + head_h / 2
        side_threshold = tail_w * 0.30
        control = max(gap * 3, 160.0)
        if head_cx < tail_cx - side_threshold:
            anchor = (tail_x, tail_y + tail_h / 2)
            control_point = (anchor[0] - control, anchor[1])
        elif head_cx > tail_cx + side_threshold:
            anchor = (tail_x + tail_w, tail_y + tail_h / 2)
            control_point = (anchor[0] + control, anchor[1])
        else:
            anchor_x = min(max(head_cx, tail_x + tail_w * 0.20), tail_x + tail_w * 0.80)
            anchor = (anchor_x, tail_y)
            control_point = (anchor[0], anchor[1] - control)
        raw_anchor = (anchor[0] - tx, anchor[1] - ty)
        raw_control = (control_point[0] - tx, control_point[1] - ty)
        return replace_svg_path_points(path_d, {0: raw_anchor, 1: raw_control})

    svg = FULL_SVG_PATH.read_text()
    for node_id, (dx, dy) in sorted(node_delta.items()):
        pattern = rf'(<g id="node\d+" class="node")>(\s*<title>{re.escape(node_id)}</title>)'
        replacement = rf'\1 transform="translate({dx:.2f} {dy:.2f})">\2'
        svg = re.sub(pattern, replacement, svg, count=1)

    def move_edge_endpoint(match: re.Match[str]) -> str:
        edge_group = match.group(0)
        title = re.search(r"<title>([^<]+?)&#45;&gt;([^<]+?)</title>", edge_group)
        if not title:
            return edge_group
        tail_node = node_id_from_edge_title_endpoint(title.group(1))
        head_node = node_id_from_edge_title_endpoint(title.group(2))
        tail_delta = node_delta.get(tail_node, (0.0, 0.0))
        head_delta = node_delta.get(head_node, (0.0, 0.0))
        if (
            abs(tail_delta[0]) <= 0.01
            and abs(tail_delta[1]) <= 0.01
            and abs(head_delta[0]) <= 0.01
            and abs(head_delta[1]) <= 0.01
        ):
            return edge_group

        def move_path(path: re.Match[str]) -> str:
            shifted_tail_delta = (0.0, 0.0) if tail_node in top_node_ids else tail_delta
            path_d = shifted_svg_path_endpoint(path.group(2), shifted_tail_delta, head_delta)
            if tail_node in top_node_ids:
                path_d = reroute_top_attached_tail(path_d, tail_node, head_node)
            return f"{path.group(1)}{path_d}{path.group(3)}"

        edge_group = re.sub(r'(<path\b[^>]*\bd=")([^"]+)(")', move_path, edge_group, count=1)
        if abs(head_delta[0]) > 0.01 or abs(head_delta[1]) > 0.01:
            edge_group = re.sub(
                r'(<polygon\b[^>]*\bpoints=")([^"]+)(")',
                lambda polygon: (
                    f"{polygon.group(1)}"
                    f"{shifted_svg_points(polygon.group(2), head_delta)}"
                    f"{polygon.group(3)}"
                ),
                edge_group,
                count=1,
            )
        return edge_group

    svg = re.sub(r'<g id="edge\d+" class="edge">.*?</g>', move_edge_endpoint, svg, flags=re.S)
    lifted_top_nodes: list[str] = []
    for node_id in sorted(top_node_ids):
        pattern = (
            rf'(?:<!-- {re.escape(node_id)} -->\n)?'
            rf'<g id="node\d+" class="node"[^>]*>\s*<title>{re.escape(node_id)}</title>.*?</g>\n'
        )
        match = re.search(pattern, svg, re.S)
        if not match:
            continue
        lifted_top_nodes.append(match.group(0))
        svg = svg[: match.start()] + svg[match.end() :]
    if lifted_top_nodes:
        graph_end = svg.rfind("</g>\n</svg>")
        if graph_end == -1:
            raise RuntimeError(f"could not find graph end in {FULL_SVG_PATH}")
        svg = svg[:graph_end] + "".join(lifted_top_nodes) + svg[graph_end:]
    FULL_SVG_PATH.write_text(svg)


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
    regions = ownership_regions(root, root_children, edges)
    region_lines = ['<g id="ownership-regions" class="ownership-regions">']
    for region in regions:
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
                    f'stroke="black" stroke-opacity="0.33" stroke-width="{OWNERSHIP_REGION_STROKE_WIDTH}" '
                    f'stroke-dasharray="{dash}"/>'
                ),
                "</g>",
            ]
        )
    region_lines.append("</g>")
    region_markup = "\n".join(region_lines) + "\n" if regions else ""

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
                    f'stroke-width="{TERMINAL_LEAF_BORDER_STROKE_WIDTH}"/>'
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
                    f'stroke="#111111" stroke-opacity="0.82" stroke-width="{SHARED_LEAF_LINK_STROKE_WIDTH}" '
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
    inline_svg = inline_full_svg_markup(node_ids)
    root_node_id = node_ids.get(root)
    root_bbox = node_bboxes.get(root_node_id) if root_node_id else None
    if root_bbox:
        root_focus_x = root_bbox[0] + root_bbox[2] / 2
        root_focus_y = root_bbox[1] + root_bbox[3] / 2
    else:
        root_focus_x = width / 2
        root_focus_y = height / 2
    root_inner_bboxes = inner_bboxes.get("n0", [])
    root_inner_by_label = {}
    root_entries = embedded_by_parent.get(root, [])
    for child in root_children:
        if child not in collapsed_leaves:
            continue
        index = direct_embedded_path_index(root_entries, child)
        if index is not None and index < len(root_inner_bboxes):
            root_inner_by_label[child] = root_inner_bboxes[index]

    rendered_edges = {
        (parent, child)
        for parent, child in edges
        if parent not in collapsed_leaves and child not in collapsed_leaves
    }
    arrow_roots = {
        root: sorted({root, *(node for edge in rendered_edges for node in edge)})
    } if rendered_edges else {}
    for child in root_children:
        if is_architecture_irrelevant(child):
            continue
        selected_nodes = downstream_nodes(child, edges)
        if selected_arrow_has_rendered_edges(root, child, selected_nodes, rendered_edges):
            arrow_roots[child] = sorted(selected_nodes)

    arrow_targets = []
    if root_bbox and rendered_edges:
        x, y, w, h = root_bbox
        arrow_targets.append(
            {
                "node": root,
                "label": display_label(root).replace(r"\n", " / "),
                "is_root": True,
                "x": round(max(0, x), 2),
                "y": round(max(0, y), 2),
                "w": round(w, 2),
                "h": round(h, 2),
            }
        )
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
        hotspot = (
            {
                "node": child,
                "label": display_label(child).replace(r"\n", " / "),
                "href": f"by-root/{filenames[child]}",
                "is_root": False,
                "x": round(max(0, x - pad), 2),
                "y": round(max(0, y - pad), 2),
                "w": round(w + pad * 2, 2),
                "h": round(h + pad * 2, 2),
            }
        )
        hotspots.append(hotspot)
        if child in arrow_roots:
            arrow_targets.append(hotspot)

    rects = "\n".join(
        f'''      <a class="root-hotspot" href="{escape(hotspot["href"])}" aria-label="{escape(hotspot["label"])}">
        <title>{escape(hotspot["label"])}</title>
        <rect x="{hotspot["x"]}" y="{hotspot["y"]}" width="{hotspot["w"]}" height="{hotspot["h"]}" rx="9" ry="9"></rect>
      </a>'''
        for hotspot in hotspots
    )
    toggles = []
    for target in arrow_targets:
        size = min(152.0, max(68.0, min(float(target["w"]), float(target["h"])) * 0.44))
        margin = 10.0
        x = float(target["x"]) + margin
        y = float(target["y"]) + margin
        stroke = max(4.0, size * 0.11)
        mark = (
            f"M{x + size * 0.25:.2f},{y + size * 0.53:.2f} "
            f"L{x + size * 0.43:.2f},{y + size * 0.71:.2f} "
            f"L{x + size * 0.76:.2f},{y + size * 0.30:.2f}"
        )
        toggles.append(
            f'''      <g class="arrow-toggle{" arrow-toggle-root" if target["is_root"] else ""}" data-arrow-root="{escape(target["node"], quote=True)}" role="checkbox" aria-checked="false" tabindex="0">
        <title>show arrows for {escape(target["label"])}</title>
        <rect class="arrow-toggle-box" x="{x:.2f}" y="{y:.2f}" width="{size:.2f}" height="{size:.2f}" rx="{size * 0.18:.2f}" ry="{size * 0.18:.2f}"></rect>
        <path class="arrow-toggle-mark" d="{mark}" fill="none" stroke-linecap="round" stroke-linejoin="round" stroke-width="{stroke:.2f}"></path>
      </g>'''
        )
    toggle_markup = "\n".join(toggles)
    view_width = width
    view_height = height
    display_width, display_height = display_size(view_width, view_height)
    content_offset_x = 0.0
    content_offset_y = 0.0
    arrow_roots_json = json.dumps(arrow_roots, ensure_ascii=False).replace("</", "<\\/")
    root_json = json.dumps(root, ensure_ascii=False).replace("</", "<\\/")
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
    html,
    body {{
      width: 100%;
      height: 100%;
      overflow: hidden;
    }}
    body {{
      margin: 0;
      color: var(--ink);
      background: var(--paper);
      font: 14px/1.4 Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }}
    .viewport {{
      overflow: scroll;
      padding: 18px;
      width: 100vw;
      height: 100vh;
      cursor: grab;
      scrollbar-gutter: stable both-edges;
      user-select: none;
      overscroll-behavior: contain;
    }}
    .viewport.is-panning,
    .viewport.is-panning * {{
      cursor: grabbing;
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
    #graph .graph-edge {{
      opacity: 0;
      pointer-events: none;
      transition: opacity 140ms ease;
    }}
    #graph .graph-edge.is-visible-arrow {{
      opacity: 1;
    }}
    .control-panel {{
      position: fixed;
      right: 16px;
      bottom: 16px;
      z-index: 4;
      width: min(320px, calc(100vw - 32px));
      padding: 14px 16px 16px;
      border: 1px solid rgba(139, 155, 176, 0.42);
      border-radius: 8px;
      background: rgba(255, 255, 255, 0.94);
      box-shadow: 0 16px 44px rgba(15, 23, 42, 0.16);
      backdrop-filter: blur(10px);
      opacity: 0.25;
      transition: opacity 140ms ease;
    }}
    .control-panel:hover,
    .control-panel:focus-within {{
      opacity: 1;
    }}
    .control-title {{
      margin: 0 0 12px;
      font-size: 18px;
      line-height: 1.15;
      font-weight: 750;
      color: var(--ink);
    }}
    .zoom-control {{
      display: grid;
      gap: 6px;
    }}
    .zoom-control label {{
      font-size: 12px;
      font-weight: 700;
      color: #475569;
    }}
    #zoom-slider {{
      width: 100%;
      accent-color: var(--hot);
    }}
    @keyframes rootPulse {{
      0%, 100% {{ fill: rgba(27, 120, 166, 0.06); }}
      50% {{ fill: rgba(27, 120, 166, 0.50); }}
    }}
    .root-hotspot rect {{
      fill: rgba(27, 120, 166, 0.06);
      stroke: transparent;
      stroke-width: {GRAPH_EDGE_PEN_WIDTH};
      pointer-events: all;
      animation: rootPulse 3.2s ease-in-out infinite;
      transition: fill 120ms ease, stroke 120ms ease;
    }}
    .root-hotspot:hover rect,
    .root-hotspot:focus rect {{
      animation: none;
      fill: rgba(27, 120, 166, 0.50);
      stroke: var(--hot);
    }}
    .arrow-toggle {{
      cursor: pointer;
      outline: none;
      opacity: 0.25;
      pointer-events: all;
      transition: opacity 140ms ease;
    }}
    .arrow-toggle:hover,
    .arrow-toggle:focus,
    .arrow-toggle.is-arrow-selected {{
      opacity: 1;
    }}
    .arrow-toggle-box {{
      fill: rgba(255, 255, 255, 0.92);
      stroke: var(--hot);
      stroke-opacity: 0.72;
      stroke-width: {GRAPH_EDGE_PEN_WIDTH};
      transition: fill 120ms ease, stroke-opacity 120ms ease;
    }}
    .arrow-toggle-mark {{
      opacity: 0;
      stroke: white;
      transition: opacity 120ms ease;
    }}
    .arrow-toggle:hover .arrow-toggle-box,
    .arrow-toggle:focus .arrow-toggle-box {{
      fill: rgba(27, 120, 166, 0.22);
      stroke-opacity: 1;
    }}
    .arrow-toggle.is-arrow-selected .arrow-toggle-box {{
      fill: rgba(27, 120, 166, 0.86);
      stroke-opacity: 1;
    }}
    .arrow-toggle.is-arrow-selected .arrow-toggle-mark {{
      opacity: 1;
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
    #dialog-image {{
      width: 100%;
      height: 100%;
      display: block;
      object-fit: contain;
      object-position: center center;
      background: white;
    }}
  </style>
</head>
<body>
  <section class="control-panel" aria-label="Graph controls">
    <h1 class="control-title">TrueOS § Depth Graph</h1>
    <div class="zoom-control">
      <label for="zoom-slider">Zoom</label>
      <input id="zoom-slider" type="range" min="0.01" max="4" step="0.001" value="1">
    </div>
  </section>
  <div class="viewport">
    <svg id="graph" viewBox="0 0 {view_width:.2f} {view_height:.2f}" width="{display_width}" height="{display_height}" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
      <g transform="translate({content_offset_x:.2f} {content_offset_y:.2f})">
{inline_svg}
{rects}
{toggle_markup}
      </g>
    </svg>
  </div>
  <dialog id="root-dialog">
    <button id="close-dialog" type="button" aria-label="Close">&times;</button>
    <img id="dialog-image" alt="Root dependency graph">
  </dialog>
  <script>
    const viewport = document.querySelector('.viewport');
    const graph = document.getElementById('graph');
    const dialog = document.getElementById('root-dialog');
    const dialogImage = document.getElementById('dialog-image');
    const closeButton = document.getElementById('close-dialog');
    const zoomSlider = document.getElementById('zoom-slider');
    const dragThreshold = 4;
    const maxGraphScale = 0.8;
    const graphBaseWidth = Number.parseFloat(graph.getAttribute('width'));
    const graphBaseHeight = Number.parseFloat(graph.getAttribute('height'));
    const rootFocusX = {root_focus_x:.2f};
    const rootFocusY = {root_focus_y:.2f};
    const rootArrowLabel = {root_json};
    const arrowRoots = {arrow_roots_json};
    const graphEdges = Array.from(graph.querySelectorAll('.graph-edge'));
    const rootHotspots = Array.from(document.querySelectorAll('.root-hotspot'));
    const arrowToggles = Array.from(graph.querySelectorAll('.arrow-toggle'));
    const arrowToggleRoots = arrowToggles.map((toggle) => toggle.dataset.arrowRoot);
    const childArrowToggleRoots = arrowToggleRoots.filter((root) => root !== rootArrowLabel);
    const arrowRootNodeSets = new Map(Object.entries(arrowRoots).map(
      ([root, nodes]) => [root, new Set(nodes)],
    ));
    let graphScale = 1;
    let pointerId = null;
    let dragStartX = 0;
    let dragStartY = 0;
    let dragStartLeft = 0;
    let dragStartTop = 0;
    let didDrag = false;
    let checkedArrowRoots = new Set();

    function clamp(value, min, max) {{
      return Math.min(Math.max(value, min), max);
    }}

    function toggleScope(nextRoot) {{
      if (nextRoot === rootArrowLabel) {{
        return new Set([rootArrowLabel, ...childArrowToggleRoots]);
      }}

      const selectedNodes = new Set(arrowRoots[nextRoot] || [nextRoot]);
      return new Set(childArrowToggleRoots.filter((root) => selectedNodes.has(root)));
    }}

    function edgeBelongsToArrowRoot(edge, root) {{
      if (root === rootArrowLabel) {{
        return true;
      }}

      const nodes = arrowRootNodeSets.get(root) || new Set([root]);
      return (
        edge.dataset.tail === rootArrowLabel && edge.dataset.head === root
      ) || (
        edge.dataset.tail !== rootArrowLabel
          && nodes.has(edge.dataset.tail)
          && nodes.has(edge.dataset.head)
      );
    }}

    function updateArrowSelection() {{
      const selectedRoots = Array.from(checkedArrowRoots);
      graphEdges.forEach((edge) => {{
        const isSelectedEdge = selectedRoots.some((root) => (
          edgeBelongsToArrowRoot(edge, root)
        ));
        edge.classList.toggle('is-visible-arrow', Boolean(isSelectedEdge));
      }});

      const allChildrenChecked = childArrowToggleRoots.length > 0
        && childArrowToggleRoots.every((root) => checkedArrowRoots.has(root));
      arrowToggles.forEach((toggle) => {{
        const isSelected = toggle.dataset.arrowRoot === rootArrowLabel
          ? checkedArrowRoots.has(rootArrowLabel) || allChildrenChecked
          : checkedArrowRoots.has(toggle.dataset.arrowRoot);
        toggle.classList.toggle(
          'is-arrow-selected',
          isSelected,
        );
        toggle.setAttribute('aria-checked', isSelected ? 'true' : 'false');
      }});
    }}

    function toggleArrowRoot(nextRoot) {{
      const scope = toggleScope(nextRoot);
      const shouldClear = nextRoot === rootArrowLabel
        ? checkedArrowRoots.has(rootArrowLabel) || childArrowToggleRoots.length > 0
          && childArrowToggleRoots.every((root) => checkedArrowRoots.has(root))
        : scope.size > 0 && Array.from(scope).every((root) => checkedArrowRoots.has(root));

      if (nextRoot !== rootArrowLabel) {{
        checkedArrowRoots.delete(rootArrowLabel);
      }}

      scope.forEach((root) => {{
        if (shouldClear) {{
          checkedArrowRoots.delete(root);
        }} else {{
          checkedArrowRoots.add(root);
        }}
      }});
      updateArrowSelection();
    }}

    function minGraphScale() {{
      const style = getComputedStyle(viewport);
      const paddingX = (Number.parseFloat(style.paddingLeft) || 0)
        + (Number.parseFloat(style.paddingRight) || 0);
      const paddingY = (Number.parseFloat(style.paddingTop) || 0)
        + (Number.parseFloat(style.paddingBottom) || 0);
      const fitWidth = Math.max(0.01, (viewport.clientWidth - paddingX) / graphBaseWidth);
      const fitHeight = Math.max(0.01, (viewport.clientHeight - paddingY) / graphBaseHeight);
      return Math.min(maxGraphScale, Math.max(fitWidth, fitHeight));
    }}

    function setGraphScale(nextScale) {{
      graphScale = clamp(nextScale, minGraphScale(), maxGraphScale);
      graph.style.width = `${{graphBaseWidth * graphScale}}px`;
      graph.style.height = `${{graphBaseHeight * graphScale}}px`;
      zoomSlider.min = minGraphScale().toFixed(4);
      zoomSlider.max = String(maxGraphScale);
      zoomSlider.value = graphScale.toFixed(4);
    }}

    function setGraphScaleAroundViewportPoint(nextScale, pointerX, pointerY) {{
      const style = getComputedStyle(viewport);
      const paddingLeft = Number.parseFloat(style.paddingLeft) || 0;
      const paddingTop = Number.parseFloat(style.paddingTop) || 0;
      const graphX = (viewport.scrollLeft + pointerX - paddingLeft) / graphScale;
      const graphY = (viewport.scrollTop + pointerY - paddingTop) / graphScale;

      setGraphScale(nextScale);
      viewport.scrollLeft = paddingLeft + graphX * graphScale - pointerX;
      viewport.scrollTop = paddingTop + graphY * graphScale - pointerY;
    }}

    function centerGraphPoint(graphX, graphY) {{
      const style = getComputedStyle(viewport);
      const paddingLeft = Number.parseFloat(style.paddingLeft) || 0;
      const paddingTop = Number.parseFloat(style.paddingTop) || 0;
      viewport.scrollLeft = paddingLeft + graphX * graphScale - viewport.clientWidth / 2;
      viewport.scrollTop = paddingTop + graphY * graphScale - viewport.clientHeight / 2;
    }}

    function startupTargetScale() {{
      const minScale = minGraphScale();
      return minScale + (maxGraphScale - minScale) * 0.25;
    }}

    function animateStartupZoom() {{
      const startScale = minGraphScale();
      const targetScale = startupTargetScale();
      const duration = 3000;
      const startTime = performance.now();

      setGraphScale(startScale);
      centerGraphPoint(rootFocusX, rootFocusY);

      function frame(now) {{
        const t = clamp((now - startTime) / duration, 0, 1);
        const eased = 1 - Math.pow(1 - t, 3);
        setGraphScale(startScale + (targetScale - startScale) * eased);
        centerGraphPoint(rootFocusX, rootFocusY);
        if (t < 1) {{
          requestAnimationFrame(frame);
        }}
      }}

      requestAnimationFrame(frame);
    }}

    function normalizeWheelDelta(event) {{
      const multiplier = event.deltaMode === WheelEvent.DOM_DELTA_LINE
        ? 16
        : event.deltaMode === WheelEvent.DOM_DELTA_PAGE
          ? viewport.clientHeight
          : 1;

      return {{
        x: event.deltaX * multiplier,
        y: event.deltaY * multiplier,
      }};
    }}

    viewport.addEventListener('pointerdown', (event) => {{
      if (event.button !== 0) {{
        return;
      }}
      if (event.target.closest('.root-hotspot, .arrow-toggle')) {{
        return;
      }}

      pointerId = event.pointerId;
      dragStartX = event.clientX;
      dragStartY = event.clientY;
      dragStartLeft = viewport.scrollLeft;
      dragStartTop = viewport.scrollTop;
      didDrag = false;
      viewport.setPointerCapture(pointerId);
      viewport.classList.add('is-panning');
    }});

    viewport.addEventListener('pointermove', (event) => {{
      if (event.pointerId !== pointerId) {{
        return;
      }}

      const deltaX = event.clientX - dragStartX;
      const deltaY = event.clientY - dragStartY;
      if (Math.abs(deltaX) > dragThreshold || Math.abs(deltaY) > dragThreshold) {{
        didDrag = true;
      }}

      if (didDrag) {{
        event.preventDefault();
        viewport.scrollLeft = dragStartLeft - deltaX;
        viewport.scrollTop = dragStartTop - deltaY;
      }}
    }});

    function endDrag(event) {{
      if (event.pointerId !== pointerId) {{
        return;
      }}

      viewport.releasePointerCapture(pointerId);
      viewport.classList.remove('is-panning');
      pointerId = null;
    }}

    viewport.addEventListener('pointerup', endDrag);
    viewport.addEventListener('pointercancel', endDrag);

    viewport.addEventListener('click', (event) => {{
      if (!didDrag) {{
        return;
      }}

      event.preventDefault();
      event.stopPropagation();
      didDrag = false;
    }}, true);

    viewport.addEventListener('wheel', (event) => {{
      event.preventDefault();

      const delta = normalizeWheelDelta(event);
      if (!event.ctrlKey && !event.metaKey) {{
        viewport.scrollLeft += event.shiftKey && delta.x === 0 ? delta.y : delta.x;
        viewport.scrollTop += event.shiftKey && delta.x === 0 ? 0 : delta.y;
        return;
      }}

      const rect = viewport.getBoundingClientRect();
      const pointerX = event.clientX - rect.left;
      const pointerY = event.clientY - rect.top;

      setGraphScaleAroundViewportPoint(graphScale * Math.exp(-delta.y * 0.001), pointerX, pointerY);
    }}, {{ passive: false }});

    zoomSlider.addEventListener('input', () => {{
      setGraphScaleAroundViewportPoint(
        Number.parseFloat(zoomSlider.value),
        viewport.clientWidth / 2,
        viewport.clientHeight / 2,
      );
    }});

    window.addEventListener('resize', () => {{
      setGraphScale(graphScale);
    }});

    animateStartupZoom();

    arrowToggles.forEach((toggle) => {{
      toggle.addEventListener('click', (event) => {{
        event.preventDefault();
        event.stopPropagation();
        toggleArrowRoot(toggle.dataset.arrowRoot);
      }});

      toggle.addEventListener('keydown', (event) => {{
        if (event.key !== 'Enter' && event.key !== ' ') {{
          return;
        }}

        event.preventDefault();
        event.stopPropagation();
        toggleArrowRoot(toggle.dataset.arrowRoot);
      }});
    }});

    rootHotspots.forEach((link) => {{
      link.addEventListener('click', (event) => {{
        event.preventDefault();
        dialogImage.src = link.getAttribute('href');
        dialogImage.alt = link.getAttribute('aria-label') || 'Root dependency graph';
        dialog.showModal();
      }});
    }});

    closeButton.addEventListener('click', () => {{
      dialog.close();
    }});

    dialog.addEventListener('close', () => {{
      dialogImage.removeAttribute('src');
    }});
  </script>
</body>
</html>
"""


def main() -> None:
    root, root_children, adjacency, edges = read_tree()
    if not root or not root_children:
        raise SystemExit(f"could not parse root children from {TREE_PATH}")

    architecture_irrelevant = [child for child in root_children if is_architecture_irrelevant(child)]
    graph_root_children = [child for child in root_children if not is_architecture_irrelevant(child)]
    graph_edges = {
        (parent, child)
        for parent, child in edges
        if not is_architecture_irrelevant(parent) and not is_architecture_irrelevant(child)
    }
    graph_adjacency: dict[str, set[str]] = defaultdict(set)
    for parent, child in graph_edges:
        graph_adjacency[parent].add(child)

    FULL_DOT_PATH.write_text(render_full_dot(root, graph_edges, architecture_irrelevant))
    subprocess.run(["dot", "-Tsvg", str(FULL_DOT_PATH), "-o", str(FULL_SVG_PATH)], check=True)
    advanced_shared_leaves = vertically_aligned_shared_leaves(root, graph_edges)
    advanced_parent_nodes, advanced_parent_anchors = shared_leaf_parent_advances(
        root, graph_edges, advanced_shared_leaves
    )
    FULL_DOT_PATH.write_text(
        render_full_dot(
            root,
            graph_edges,
            architecture_irrelevant,
            advanced_parent_nodes,
            advanced_parent_anchors,
        )
    )
    subprocess.run(["dot", "-Tsvg", str(FULL_DOT_PATH), "-o", str(FULL_SVG_PATH)], check=True)
    move_attached_nodes_to_parents(root, graph_edges)
    inject_full_svg_regions(root, graph_root_children, graph_edges)
    full_node_ids, _collapsed_leaves, full_embedded_by_parent, _shared_input_leaves, _incoming_parents = (
        full_graph_layout(root, graph_edges)
    )
    inject_nested_shared_badges(
        FULL_SVG_PATH,
        full_node_ids,
        full_embedded_by_parent,
        full_nested_shared_leaf_parents(root, graph_edges),
    )
    widen_full_svg_to_aspect()
    move_architecture_irrelevant_bucket_to_top_left()
    layer_full_svg_edges_below_nodes()
    center_full_svg_horizontally()
    HTML_INDEX_PATH.write_text(render_html_index(root, graph_root_children, graph_edges))

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    for old in OUT_DIR.glob("*.dot"):
        old.unlink()
    for old in OUT_DIR.glob("*.svg"):
        old.unlink()

    owner = assign_owners(graph_root_children, graph_adjacency)
    filenames = unique_filenames(graph_root_children)
    all_roots = set(graph_root_children)

    rows: list[tuple[str, str, int, int, int]] = []
    for image_root in graph_root_children:
        owned_nodes = {node for node, node_owner in owner.items() if node_owner == image_root}
        owned_nodes.add(image_root)

        incoming: list[tuple[str, str]] = []
        outgoing: list[tuple[str, str]] = []
        for parent, child in graph_edges:
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

        dot = render_dot(image_root, owned_nodes, filenames, owner, graph_edges, incoming, outgoing)
        dot_path = OUT_DIR / filenames[image_root].replace(".svg", ".dot")
        svg_path = OUT_DIR / filenames[image_root]
        dot_path.write_text(dot)
        subprocess.run(["dot", "-Tsvg", str(dot_path), "-o", str(svg_path)], check=True)
        split_node_ids, split_embedded_by_parent, split_nested_two_input = split_graph_layout(
            image_root,
            owned_nodes,
            graph_edges,
            incoming,
            outgoing,
        )
        inject_nested_shared_badges(
            svg_path,
            split_node_ids,
            split_embedded_by_parent,
            split_nested_two_input,
        )
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
