import argparse
import json
from pathlib import Path


def reject_constant(value):
    raise ValueError(f"invalid JSON constant {value}")


def catalog_names(path, key, *, optional=False):
    try:
        with path.open(encoding="utf-8") as source:
            catalog = json.load(source, parse_constant=reject_constant)
    except FileNotFoundError:
        if optional:
            return []
        raise

    entries = catalog.get(key) if isinstance(catalog, dict) else None
    if not isinstance(entries, list):
        raise ValueError(f"expected an array named {key}")

    names = set()
    for entry in entries:
        name = entry.get("name") if key == "apps" and isinstance(entry, dict) else entry
        if not isinstance(name, str) or not name:
            raise ValueError(f"missing {key} entry name")
        name.encode("utf-8")
        if name in names:
            raise ValueError(f"duplicate {name}")
        names.add(name)
    return sorted(names)


def rust_string(value):
    escaped = []
    for char in value:
        if char in ('"', "\\"):
            escaped.append("\\" + char)
        elif " " <= char <= "~":
            escaped.append(char)
        else:
            escaped.append(f"\\u{{{ord(char):x}}}")
    return '"' + "".join(escaped) + '"'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("buildins", type=Path)
    parser.add_argument("blueprints", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--optional-buildins", action="store_true")
    args = parser.parse_args()

    catalogs = (
        (args.buildins, "buildins", "BUILDINS", args.optional_buildins),
        (args.blueprints, "apps", "BLUEPRINTS", False),
    )
    generated = []
    for path, key, constant, optional in catalogs:
        try:
            names = catalog_names(path, key, optional=optional)
        except (OSError, ValueError) as error:
            raise SystemExit(f"{path}: {error}") from error
        literals = ", ".join(map(rust_string, names))
        generated.append(f"pub const {constant}: &[&str] = &[{literals}];\n")
    args.output.write_text("".join(generated), encoding="utf-8")


if __name__ == "__main__":
    main()
