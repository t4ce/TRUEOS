#!/usr/bin/env python3
"""Apply the reviewed source-fold maps to Cargo, Rust consumers and Bazel labels."""
import argparse
import json
from pathlib import Path
import re
import tomllib


def dependency_body(values):
    result = {"workspace": True}
    for value in values:
        if not value.get("workspace"):
            raise ValueError(f"unexpected non-workspace dependency: {value}")
        if set(value) - {"workspace", "optional", "features", "default-features"}:
            raise ValueError(f"unhandled dependency options: {value}")
    if all(value.get("optional", False) for value in values):
        result["optional"] = True
    features = sorted({feature for value in values for feature in value.get("features", [])})
    if features:
        result["features"] = features
    if any(value.get("default-features", True) for value in values):
        if any("default-features" in value for value in values):
            result["default-features"] = True
    else:
        result["default-features"] = False
    return "{ " + ", ".join(f"{key} = {json.dumps(value)}" for key, value in result.items()) + " }"


def migrate_manifest(text, mapping):
    chunks = re.split(r"(?m)(?=^\[)", text)
    for i, chunk in enumerate(chunks):
        first = chunk.splitlines()[0] if chunk else ""
        if "dependencies]" not in first or first.startswith("[workspace."):
            continue
        lines = chunk.splitlines(keepends=True)
        groups = {}
        for position, line in enumerate(lines):
            match = re.match(r"^([\w-]+)\s*=\s*(\{.*\})\s*$", line)
            if not match:
                continue
            key, body = match.groups()
            if key in mapping:
                new = mapping[key]["key"]
            elif key in {item["key"] for item in mapping.values()}:
                new = key
            else:
                continue
            groups.setdefault(new, []).append((position, tomllib.loads("entry = " + body)["entry"]))
        for new, entries in groups.items():
            lines[entries[0][0]] = new + " = " + dependency_body([value for _, value in entries]) + "\n"
            for position, _ in entries[1:]:
                lines[position] = ""
        chunks[i] = "".join(lines)
    text = "".join(chunks)
    # Feature references require the same dependency migration.
    for old, new in mapping.items():
        text = text.replace('"dep:' + old + '"', '"dep:' + new["key"] + '"')
        text = re.sub(r'"' + re.escape(old) + r'([?/])', '"' + new["key"] + r'\1', text)
    return text


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("plans", type=Path, nargs="+")
    parser.add_argument("--repo", type=Path, default=Path("/home/t4ce/Repos/codex"))
    args = parser.parse_args()
    repo = args.repo.resolve()
    root = repo / "codex-rs"
    plans = [json.loads(path.read_text()) for path in args.plans]
    cargo = {key: value for plan in plans for key, value in plan["cargo_dependency_mapping"].items()}
    rust = {key: value for plan in plans for key, value in plan["rust_path_mapping"].items()}
    removed = [member for plan in plans for member in plan["removed_members"]]
    added = [member for plan in plans for member in plan["new_members"]]
    central = root / "Cargo.toml"
    text = central.read_text()
    for member in removed:
        text = text.replace(f'    "{member}",\n', "")
    marker = "\n]\nresolver ="
    assert marker in text
    for member in added:
        if f'    "{member}",\n' not in text:
            text = text.replace(marker, f'\n    "{member}",' + marker, 1)
    for key in cargo:
        text = re.sub(r"(?m)^" + re.escape(key) + r"\s*=.*\n", "", text)
    dependencies = {entry["key"]: entry["path"] for entry in cargo.values()}
    for key, path in sorted(dependencies.items()):
        if not re.search(r"(?m)^" + re.escape(key) + r"\s*=", text):
            text = text.replace("[workspace.dependencies]\n", "[workspace.dependencies]\n" + f'{key} = {{ path = "{path}" }}\n', 1)
    central.write_text(text)
    changed = [str(central.relative_to(repo))]
    candidates = [Path(path) for path in __import__("subprocess").check_output(
        ["rg", "--files", "-g", "!target", "-g", "!vendor", "-g", "!node_modules", "-g", "!.git", str(repo)], text=True).splitlines()]
    for path in candidates:
        if path == central or path.suffix not in {".rs", ".toml", ".bzl", ".bazel"}:
            continue
        before = path.read_text()
        after = before
        if path.name == "Cargo.toml":
            after = migrate_manifest(after, cargo)
        elif path.suffix == ".rs":
            for old, new in rust.items():
                after = re.sub(r"\b" + re.escape(old) + r"\b", new, after)
        elif path.suffix in {".bzl", ".bazel"}:
            # Only explicit local dependency labels; the new package's crate
            # name/module definitions were created by its source migration.
            for old_member in removed:
                old_packages = [key for key, value in cargo.items()
                                if value["path"] == old_member]
                # Most old paths differ from the destination; infer from
                # package/member lists using the original workspace snapshot.
                snapshot = Path(__file__).parent / "before/codex-Cargo.toml"
                prior = tomllib.loads(snapshot.read_text())["workspace"]["dependencies"]
                old_packages = [key for key in cargo if prior.get(key, {}).get("path") == old_member]
                if old_packages:
                    new_member = cargo[old_packages[0]]["path"]
                    old_label = "//codex-rs/" + old_member
                    new_label = "//codex-rs/" + new_member
                    after = re.sub(re.escape(old_label) + r'(?=["\':])', new_label, after)
        if after != before:
            path.write_text(after)
            changed.append(str(path.relative_to(repo)))
    report = {"plan_files": [str(path) for path in args.plans], "changed_files": changed,
              "cargo_dependency_mapping": cargo, "rust_path_mapping": rust}
    output = Path(__file__).parent / "consumer-migration-results.json"
    output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Migrated {len(changed)} files, {len(cargo)} Cargo dependencies and {len(rust)} Rust crate paths.")


if __name__ == "__main__":
    main()
