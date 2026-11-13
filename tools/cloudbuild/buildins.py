#!/usr/bin/env python3
"""Prepare an exact selection of the existing TRUEOS buildins.

The sibling builder's --app option is diagnostic: it does not create a complete
AppDB. This adapter reuses its existing compiler, object packaging and ABI checks,
then seeds and verifies a database for the selected set. It never rewrites the
source buildins.json or promotes another Blueprint to a buildin.
"""
from __future__ import annotations

import argparse
import fcntl
import hashlib
import importlib.util
import inspect
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time
import tomllib
import shutil

from build_plan import CATALOG_PATH, ROOT, catalog


READY_SCHEMA = "trueos-selected-buildins/v2"
COMPILER_COMMIT = "af3d95584dbddcae597890340995509a7fb47a50"
MAX_JSON_BYTES = 64 * 1024
STAGED_IGNORED = {".git", "target", "tgt", "__pycache__", "Cargo.lock"}
# Refuse an incompatible builder instead of silently producing an unchecked
# payload after an upstream interface change. The operator pins its source.
REQUIRED_API = {
    "settings": ("name",),
    "tool_environment": ("output", "kernel", "needs_std"),
    "patches": ("kernel",),
    "input_roots": ("kernel",),
    "dependency_path": ("relative", "original", "staged", "app_root"),
    "sync_tree": ("source", "target", "transformed"),
    "build_app": ("name", "output", "profile", "cargo", "binaries", "env", "overlays"),
    "verify_abi": ("kernel", "output", "names"),
    "prune_obsolete_packages": ("output", "names"),
    "write_changed": ("path", "data", "durable"),
    "lz4_frame": ("archive",),
    "checkpoint": ("output", "name", "phase"),
    "run": ("args", "kwargs"),
}


def _object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON field: {key}")
        result[key] = value
    return result


def read_json(path: Path):
    with path.open("rb") as stream:
        data = stream.read(MAX_JSON_BYTES + 1)
    if len(data) > MAX_JSON_BYTES:
        raise ValueError(f"buildin metadata is too large: {path}")
    return json.loads(data, object_pairs_hook=_object)


def selection_names(source: Path, selection: str) -> list[str]:
    document = read_json(source / "buildins.json")
    names = document.get("buildins") if isinstance(document, dict) else None
    if (not isinstance(document, dict) or set(document) != {"buildins"}
            or not isinstance(names, list) or not names
            or any(not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", name) for name in names)
            or len(set(names)) != len(names)):
        raise ValueError("source buildins.json must contain one unique, nonempty buildins list")
    known = {item["id"] for item in catalog()["buildins"]}
    if set(names) != known:
        raise ValueError("the pinned TRUEOS-Buildins manifest differs from the selector catalog; "
                         "review the catalog and source pin together")
    selected = sorted(known) if selection == "all" else selection.split(",")
    if (not selected or any(name not in known for name in selected)
            or len(set(selected)) != len(selected)):
        raise ValueError("selection must contain unique catalog buildin IDs; "
                         "an empty selection must skip the buildins stage")
    return sorted(selected)


def load_builder(source: Path):
    path = source / "tools/build.py"
    spec = importlib.util.spec_from_file_location("_trueos_buildins_source", path)
    if spec is None or spec.loader is None:
        raise ValueError("cannot load the pinned TRUEOS-Buildins builder")
    module = importlib.util.module_from_spec(spec)
    # Source checkouts stay clean. The sibling module imports elf_layout from
    # its own tools directory; no copied or modified source tree is needed.
    sys.path.insert(0, str(path.parent))
    previous = sys.dont_write_bytecode
    sys.dont_write_bytecode = True
    try:
        spec.loader.exec_module(module)
    finally:
        sys.dont_write_bytecode = previous
        sys.path.pop(0)
    if (getattr(module, "ROOT", None) != source
            or getattr(module, "COMMIT", None) != COMPILER_COMMIT
            or not isinstance(getattr(module, "IGNORED", None), (set, frozenset))):
        raise ValueError("incompatible TRUEOS-Buildins source root, toolchain or input contract")
    for name, parameters in REQUIRED_API.items():
        function = getattr(module, name, None)
        if not callable(function) or tuple(inspect.signature(function).parameters) != parameters:
            raise ValueError(f"incompatible TRUEOS-Buildins builder API: {name}; review the adapter")
    return module


def external_dependency_roots(builder, names: list[str]) -> list[Path]:
    """Follow path dependencies of selected apps, without opening other apps."""
    seen, external, pending = set(), set(), []
    for name in names:
        app = builder.ROOT / name
        pending.extend((manifest, app) for manifest in app.rglob("Cargo.toml")
                       if not any(part in builder.IGNORED for part in manifest.relative_to(app).parts))
    while pending:
        manifest, app = pending.pop()
        manifest = manifest.resolve()
        if manifest in seen or not manifest.is_file():
            continue
        seen.add(manifest)
        document = tomllib.loads(manifest.read_text())

        def visit(value):
            if not isinstance(value, dict):
                return
            for key, child in value.items():
                if key in ("dependencies", "dev-dependencies", "build-dependencies") and isinstance(child, dict):
                    for dependency in child.values():
                        if isinstance(dependency, dict) and isinstance(dependency.get("path"), str):
                            relative = dependency["path"]
                            if relative.endswith("/api"):
                                continue  # The shared SDK input root covers this.
                            destination = builder.dependency_path(relative, manifest, manifest, app).resolve()
                            if destination.is_relative_to((builder.ROOT / "vendor").resolve()):
                                continue  # Only this source tree's vendor root is already covered.
                            if not destination.is_relative_to(app.resolve()):
                                external.add(destination)
                                pending.append((destination / "Cargo.toml", app))
                else:
                    visit(child)

        visit(document)
    return sorted(external)


def reference_inputs(output: Path, kernel: Path) -> list[Path]:
    manifest = read_json(output / "dependencies/trueos-ide/api-prm/manifest.json")
    sources = manifest.get("source", {}).get("files")
    if not isinstance(sources, dict) or not sources:
        raise ValueError("generated API reference has no source fingerprint")
    roots = [kernel / "tools/build_api_prm.py", kernel / "tools/api-prm"]
    for name in sources:
        if (not isinstance(name, str) or not name or Path(name).is_absolute()
                or ".." in Path(name).parts):
            raise ValueError("generated API reference contains an invalid source path")
        roots.append(kernel / name)
    return roots


def dependency_paths(document) -> list[str]:
    paths = []
    if isinstance(document, dict):
        for key, value in document.items():
            if key in ("dependencies", "dev-dependencies", "build-dependencies") and isinstance(value, dict):
                paths += [spec["path"] for spec in value.values() if isinstance(spec, dict) and isinstance(spec.get("path"), str)]
            else:
                paths += dependency_paths(value)
    return paths


def external_manifest(original: Path) -> bytes:
    """Keep a copied crate's path dependencies pointing to immutable originals."""
    source = original.read_text()
    expected = sorted(str((original.parent / path).resolve()) for path in dependency_paths(tomllib.loads(source)))
    lines, table = [], ""
    for line in source.splitlines():
        if line.strip().startswith("["):
            table = line.strip().strip("[]")
        dependency = (table.endswith("dependencies") or ".dependencies." in table
                      or table.startswith(("dependencies.", "dev-dependencies.", "build-dependencies.")))
        if dependency:
            def absolute(match):
                path = tomllib.loads("path = " + match[1])["path"]
                return "path = " + json.dumps(str((original.parent / path).resolve()))
            line = re.sub(r"\bpath\s*=\s*(\"(?:[^\"\\]|\\.)*\"|'[^']*')", absolute, line)
        lines.append(line)
    transformed = "\n".join(lines) + "\n"
    if sorted(dependency_paths(tomllib.loads(transformed))) != expected:
        raise ValueError(f"cannot safely rewrite staged dependency paths in {original}")
    return transformed.encode()


def run_reference_generator(kernel: Path, output: Path, *, check: bool = False) -> None:
    command = [sys.executable, str(kernel / "tools/build_api_prm.py"),
               "--output", str(output / "dependencies/trueos-ide/api-prm")]
    if check:
        command.append("--check")
    result = subprocess.run(command, cwd=kernel, env={**os.environ, "TRUEOS_API_SOURCE_ROOT": str(kernel)},
                            capture_output=check, text=True)
    if result.returncode:
        detail = (result.stderr or result.stdout or "")[-4000:] if check else ""
        raise ValueError("staged Edit API reference is stale or failed generation" + (": " + detail.strip() if detail else ""))


def stage_editor_dependency(builder, output: Path, kernel: Path, names: list[str]) -> dict[Path, Path]:
    if "edit" not in names:
        return {}
    candidates = [path for path in external_dependency_roots(builder, names)
                  if path.name == "trueos-ide" and (path / "Cargo.toml").is_file()
                  and tomllib.loads((path / "Cargo.toml").read_text()).get("package", {}).get("name") == "trueos-ide"]
    if len(candidates) != 1:
        raise ValueError("Edit must resolve exactly one trueos-ide source crate; review the buildin adapter")
    original = candidates[0].resolve()
    staged = output / "dependencies/trueos-ide"
    if ((output / "dependencies").is_symlink() or not staged.resolve().is_relative_to(output.resolve())
            or staged.is_symlink() or any(path.is_symlink() for path in staged.rglob("*"))):
        raise ValueError("staged trueos-ide must contain private files, not source symlinks")
    staged.mkdir(parents=True, exist_ok=True)
    transformed = {path: external_manifest(path) for path in original.rglob("Cargo.toml")
                   if not any(part in builder.IGNORED or part == "api-prm" for part in path.relative_to(original).parts)}
    # Keep cached generated references in this output directory. Copying the
    # tracked reference from the source checkout would replace them on each run.
    for old in staged.iterdir():
        if old.name not in builder.IGNORED | {"api-prm"} and not (original / old.name).exists():
            shutil.rmtree(old) if old.is_dir() else old.unlink()
    for path in original.iterdir():
        if path.name in builder.IGNORED or path.name == "api-prm":
            continue
        if path.is_dir():
            builder.sync_tree(path, staged / path.name, transformed)
        else:
            builder.write_changed(staged / path.name, transformed.get(path, path.read_bytes()))
    run_reference_generator(kernel, output)
    return {original: staged}


def generated_state(output: Path, names: list[str]) -> dict:
    if "edit" not in names:
        return {}
    staged = output / "dependencies/trueos-ide"
    if (not staged.is_dir() or staged.is_symlink() or (output / "dependencies").is_symlink()
            or not staged.resolve().is_relative_to(output.resolve())):
        raise ValueError("the selected Edit build is missing its private dependency tree")
    digest = hashlib.sha256()
    for path in sorted(staged.rglob("*")):
        relative = path.relative_to(staged)
        if any(part in STAGED_IGNORED for part in relative.parts):
            continue
        if path.is_symlink():
            raise ValueError("staged trueos-ide must not contain source symlinks")
        if path.is_file():
            digest.update(relative.as_posix().encode() + b"\0" + hashlib.sha256(path.read_bytes()).digest())
    return {"trueos-ide": {"tree_sha256": digest.hexdigest(),
                           "api_manifest_sha256": file_digest(staged / "api-prm/manifest.json")}}


def input_roots(builder, kernel: Path, names: list[str], output: Path) -> list[Path]:
    known = {item["id"] for item in catalog()["buildins"]}
    excluded = known - set(names)
    roots = []
    for path in builder.input_roots(kernel):
        path = Path(path)
        try:
            relative = path.relative_to(builder.ROOT)
        except ValueError:
            relative = None
        if relative is not None and not relative.parts:
            raise ValueError("incompatible builder input roots: an entire app tree cannot be filtered safely")
        if relative is not None and relative.parts[0] in excluded:
            continue
        roots.append(path)
    roots += external_dependency_roots(builder, names)
    roots += list(builder.patches(kernel).values())
    roots += [Path(__file__).resolve(), CATALOG_PATH]
    roots += [kernel / "src/unix_compat.rs"]
    if "edit" in names:
        roots += reference_inputs(output, kernel)
    return sorted(set(roots))


def source_digest(builder, kernel: Path, names: list[str], output: Path) -> str:
    digest = hashlib.sha256(json.dumps({"schema": READY_SCHEMA, "buildins": names}, sort_keys=True).encode())
    for root in input_roots(builder, kernel, names, output):
        # Include missing roots and empty directories too: disappearance of an
        # input must not accidentally retain the previous freshness identity.
        kind = b"directory" if root.is_dir() else b"file" if root.is_file() else b"missing"
        digest.update(str(root).encode() + b"\0" + kind + b"\0")
        for path in sorted(root.rglob("*") if root.is_dir() else [root]):
            if path.is_file() and not any(part in builder.IGNORED for part in path.relative_to(root.parent).parts):
                digest.update(str(path).encode() + b"\0" + hashlib.sha256(path.read_bytes()).digest())
    return digest.hexdigest()


def manifest_bytes(names: list[str]) -> bytes:
    return (json.dumps({"buildins": names}, indent=2) + "\n").encode()


def file_digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate_native_selection(builder, names: list[str]) -> None:
    """Audit the exact request, never filter it into a different app selection."""
    audited = getattr(builder, "NATIVE_CORE_ALLOC_BUILTINS", None)
    if not isinstance(audited, (set, frozenset)):
        raise ValueError("the pinned builder has no audited native buildin set; review the source pin")
    for name in names:
        if name not in audited:
            raise ValueError(f"{name}: selected buildin is not in the audited native set")
        if builder.settings(name)[2]:
            raise ValueError(f"{name}: native-only selection now requires std")


def verify_native_imports(kernel: Path, output: Path, names: list[str]) -> None:
    """Apply the upstream native-only import audit after compiling selected apps."""
    source = (kernel / "src/unix_compat.rs").read_text()
    start, end = "pub(crate) fn is_unix_import", "pub(crate) fn resolve_import"
    if source.count(start) != 1 or source.count(end) != 1:
        raise ValueError("native Unix import classifier changed; review the buildin adapter")
    classifier, separator, _ = source.split(start, 1)[1].partition(end)
    unix = set(re.findall(r'"([^"\n]+)"', classifier))
    if not separator or not unix:
        raise ValueError("native Unix import classifier is missing or empty")
    for name in names:
        result = subprocess.run(["nm", "-u", output / "objects" / name / "module.stripped.o"],
                                check=True, capture_output=True, text=True)
        imports = {line.split()[-1] for line in result.stdout.splitlines() if line.strip()}
        if forbidden := sorted(imports & unix):
            raise ValueError(f"{name}: native-only image imports Unix compatibility: {forbidden}")


def preparation_record(output: Path, profile: str, names: list[str]) -> dict | None:
    """Bind completed provenance to one bounded readiness snapshot, without executing sources."""
    if not names:
        return None
    with (output / "ready.json").open("rb") as stream:
        data = stream.read(MAX_JSON_BYTES + 1)
    if len(data) > MAX_JSON_BYTES:
        raise ValueError("buildin readiness metadata is too large")
    ready = json.loads(data, object_pairs_hook=_object)
    if (not isinstance(ready, dict) or ready.get("schema") != READY_SCHEMA
            or set(ready) - {"native_only"} != {"schema", "profile", "buildins", "sources", "packages", "database", "generated"}
            or ("native_only" in ready and ready["native_only"] is not True)
            or ready["profile"] != profile or ready["buildins"] != names or (output / ".building").exists()):
        raise ValueError("buildin readiness does not match the completed build")
    if not isinstance(ready["sources"], str) or not re.fullmatch(r"[0-9a-f]{64}", ready["sources"]):
        raise ValueError("buildin readiness has an invalid source fingerprint")
    for key, expected in (("packages", set(names)), ("database", {"app.db", "app.db.lz4"})):
        hashes = ready[key]
        if (not isinstance(hashes, dict) or set(hashes) != expected
                or any(not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{64}", value) for value in hashes.values())):
            raise ValueError(f"buildin readiness has invalid {key} fingerprints")
    generated = ready["generated"]
    if not isinstance(generated, dict) or set(generated) != ({"trueos-ide"} if "edit" in names else set()):
        raise ValueError("buildin readiness has invalid generated input metadata")
    for record in generated.values():
        if (not isinstance(record, dict) or set(record) != {"tree_sha256", "api_manifest_sha256"}
                or any(not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{64}", value) for value in record.values())):
            raise ValueError("buildin readiness has invalid generated input fingerprints")
    return {"ready_sha256": hashlib.sha256(data).hexdigest(), "generated": generated}


def check_ready(builder, output: Path, kernel: Path, profile: str, names: list[str],
                native_only: bool = False) -> None:
    if native_only:
        validate_native_selection(builder, names)
    ready = read_json(output / "ready.json")
    if (not isinstance(ready, dict)
            or set(ready) - {"native_only"} != {"schema", "profile", "buildins", "sources", "packages", "database", "generated"}
            or ("native_only" in ready and ready["native_only"] is not True)
            or ready.get("schema") != READY_SCHEMA):
        raise ValueError("prepared buildins use a different selection format; prepare this build again")
    if ready.get("native_only", False) != native_only:
        raise ValueError("prepared buildins do not match the requested native-only validation mode")
    if (output / ".building").exists() or ready["profile"] != profile or ready["buildins"] != names:
        raise ValueError("prepared buildins do not match the requested selection or the last build failed")
    if (not isinstance(ready["packages"], dict) or set(ready["packages"]) != set(names)
            or not isinstance(ready["database"], dict) or set(ready["database"]) != {"app.db", "app.db.lz4"}
            or (output / "buildins.json").read_bytes() != manifest_bytes(names)
            or {path.stem for path in (output / "dist").glob("*.bp")} != set(names)):
        raise ValueError("prepared buildin packages or manifest differ from the requested selection")
    if "edit" in names:
        run_reference_generator(kernel, output, check=True)
    if ready["generated"] != generated_state(output, names):
        raise ValueError("generated buildin inputs changed since the selected build")
    if ready["sources"] != source_digest(builder, kernel, names, output):
        raise ValueError("selected buildin inputs are stale; prepare the kernel and buildins together")
    for name, expected in ready["packages"].items():
        if file_digest(output / "dist" / f"{name}.bp") != expected:
            raise ValueError(f"prepared package changed since the selected build: {name}")
    for name, expected in ready["database"].items():
        if file_digest(output / name) != expected:
            raise ValueError(f"prepared database changed since the selected build: {name}")
    for path in [*input_roots(builder, kernel, names, output), output / "ready.json", output / "buildins.json",
                 *(output / "dist" / f"{name}.bp" for name in names), output / "app.db", output / "app.db.lz4"]:
        print(f"cargo:rerun-if-changed={path}")
    if "edit" in names:
        print(f"cargo:rerun-if-changed={output / 'dependencies/trueos-ide'}")


def build_database(builder, output: Path, kernel: Path, cargo: Path, env: dict,
                   names: list[str]) -> dict[str, str]:
    packages = {name: file_digest(output / "dist" / f"{name}.bp") for name in names}
    tool = kernel / "tools/appdb-seed"
    kernel_version = tomllib.loads((kernel / "Cargo.toml").read_text())["dependencies"]["redb"]["version"]
    host_version = tomllib.loads((tool / "Cargo.toml").read_text())["dependencies"]["redb"]["version"]
    if kernel_version != host_version or not host_version.startswith("="):
        raise ValueError("kernel and app.db builder must pin the same exact redb version")
    inputs = hashlib.sha256(json.dumps({"schema": READY_SCHEMA, "packages": packages}, sort_keys=True).encode())
    inputs.update(manifest_bytes(names))
    inputs.update(Path(__file__).read_bytes())
    inputs.update((builder.ROOT / "tools/build.py").read_bytes())
    for path in sorted(tool.rglob("*")):
        if path.is_file() and not any(part in builder.IGNORED for part in path.relative_to(tool).parts):
            inputs.update(str(path).encode() + b"\0" + path.read_bytes())
    bundle_manifest = kernel.parent / "TRUEOS-Blueprints/appbundle.json"
    bundle_crate = kernel.parent / "TRUEOS-Blueprints/crates/trueos-appbundle"
    for path in [bundle_manifest, *sorted(bundle_crate.rglob("*"))]:
        if path.is_file() and not any(part in builder.IGNORED for part in path.parts):
            inputs.update(str(path).encode() + b"\0" + path.read_bytes())
    inputs.update(os.environ.get("SOURCE_DATE_EPOCH", "").encode())
    signature = inputs.hexdigest()
    state = output / "database.json"
    if state.is_file():
        previous = read_json(state)
        if (isinstance(previous, dict) and previous.get("inputs") == signature
                and isinstance(previous.get("database"), dict)
                and set(previous["database"]) == {"app.db", "app.db.lz4"}
                and all((output / name).is_file() and file_digest(output / name) == digest
                        for name, digest in previous["database"].items())):
            print("buildins: fresh selected app.db image", flush=True)
            return previous["database"]
    host_env = dict(env)
    host_env.pop("CARGO_ENCODED_RUSTFLAGS", None)
    host_env.pop("RUSTC_BOOTSTRAP_SYNTHETIC_TARGET", None)
    host_env["RUSTC"] = str(cargo.parent / "rustc")
    host_env["CARGO_TARGET_DIR"] = str(output / "host")
    updated = time.strftime("%Y-%m-%d %H:%MZ", time.gmtime(int(os.environ.get("SOURCE_DATE_EPOCH", time.time()))))
    with tempfile.TemporaryDirectory(prefix="trueos-appdb-host-") as directory:
        builder.run([cargo, "run", "--quiet", "--release", "--target", "x86_64-unknown-linux-gnu",
                     "--manifest-path", tool / "Cargo.toml", "--", output / "buildins.json",
                     output / "dist", output / "app.db", updated, bundle_manifest], cwd=directory, env=host_env)
    builder.write_changed(output / "app.db.lz4", builder.lz4_frame((output / "app.db").read_bytes()), durable=True)
    database = {name: file_digest(output / name) for name in ("app.db", "app.db.lz4")}
    builder.write_changed(state, (json.dumps({"inputs": signature, "database": database}, indent=2) + "\n").encode(), durable=True)
    return database


def build_selected(builder, output: Path, kernel: Path, profile: str, names: list[str],
                   native_only: bool = False) -> None:
    if native_only:
        validate_native_selection(builder, names)
    # Validation and std/sysroot selection visit only selected applications.
    needs_std = any(builder.settings(name)[2] for name in names)
    (output / ".building").touch()
    redirects = stage_editor_dependency(builder, output, kernel, names)
    before = source_digest(builder, kernel, names, output)
    generated = generated_state(output, names)
    cargo, binaries, env = builder.tool_environment(output, kernel, needs_std)
    env["TRUEOS_API_SOURCE_ROOT"] = str(kernel)
    overlays = builder.patches(kernel)
    original_dependency_path = builder.dependency_path

    def redirected_dependency_path(relative, original, staged, app_root):
        destination = original_dependency_path(relative, original, staged, app_root)
        return redirects.get(destination.resolve(), destination)

    # The upstream manifest staging calls this resolver. Scope the redirect to
    # compilation only; source traversal/freshness always sees immutable inputs.
    builder.dependency_path = redirected_dependency_path
    try:
        for name in names:
            builder.build_app(name, output, profile, cargo, binaries, env, overlays)
    finally:
        builder.dependency_path = original_dependency_path
    builder.verify_abi(kernel, output, names)
    if native_only:
        verify_native_imports(kernel, output, names)
    builder.prune_obsolete_packages(output, names)
    builder.write_changed(output / "buildins.json", manifest_bytes(names), durable=True)
    builder.checkpoint(output, "app.db", "selected image start")
    database = build_database(builder, output, kernel, cargo, env, names)
    builder.checkpoint(output, "app.db", "selected image complete")
    if source_digest(builder, kernel, names, output) != before or generated_state(output, names) != generated:
        raise ValueError("selected buildin inputs changed during the build; run the build again")
    ready = {"schema": READY_SCHEMA, "profile": profile, "buildins": names,
             "sources": before, "database": database, "generated": generated,
             "packages": {name: file_digest(output / "dist" / f"{name}.bp") for name in names}}
    if native_only:
        ready["native_only"] = True
    builder.write_changed(output / "ready.json", (json.dumps(ready, indent=2) + "\n").encode(), durable=True)
    (output / ".building").unlink()
    print(f"buildins: selected set of {len(names)} apps ready for the kernel ({', '.join(names)})", flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--kernel", type=Path, default=ROOT)
    parser.add_argument("--source", type=Path, default=ROOT.parent / "TRUEOS-Buildins")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--profile", choices=("dev", "release"), default="dev")
    parser.add_argument("--selection", default="all", help="comma-separated existing buildin IDs, or all")
    parser.add_argument("--check", action="store_true", help="verify the exact prepared selection without compiling")
    parser.add_argument("--native-only", action="store_true", help="require the exact selected apps to use the audited core/alloc target and no Unix imports")
    args = parser.parse_args()
    try:
        source, output, kernel = args.source.resolve(), args.output.resolve(), args.kernel.resolve()
        names = selection_names(source, args.selection)
        builder = load_builder(source)
        output.mkdir(parents=True, exist_ok=True)
        with (output / "build.lock").open("a") as lock:
            fcntl.flock(lock, fcntl.LOCK_SH if args.check else fcntl.LOCK_EX)
            if args.check:
                check_ready(builder, output, kernel, args.profile, names, args.native_only)
            else:
                build_selected(builder, output, kernel, args.profile, names, args.native_only)
    except (OSError, ValueError, KeyError, TypeError, ImportError, subprocess.SubprocessError) as error:
        parser.exit(2, f"buildins: {error}\n")


if __name__ == "__main__":
    main()
