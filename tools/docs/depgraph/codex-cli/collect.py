#!/usr/bin/env python3
"""Capture the already-staged cargo bp codex-cli graph without compiling/publishing."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time


HERE = Path(__file__).resolve().parent
REPOS = HERE.parents[4]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repos", type=Path, default=REPOS)
    parser.add_argument("--rlink", type=Path, help="existing root .rlink; default is the captured pack's root")
    args = parser.parse_args()
    repos = args.repos.resolve()
    toolchain = repos / "TRUEOS-Rust-Toolchain-nightly-2026-07-10"
    cli = repos / "codex/codex-rs/cli"
    manifest = cli / "target/trueos-blueprint/work/codex/source-overlay-app/Cargo.toml"
    target = cli / "target/trueos-blueprint/x86_64-unknown-trueos.json"
    raw = HERE / "raw"
    config = raw / "pack-config.toml"
    for path in [manifest, target, config, toolchain / "bin/cargo", toolchain / "bin/rustc"]:
        if not path.is_file():
            parser.error(f"required existing pack evidence is missing: {path}")
    # Use the captured pack patches, not the upstream workspace's patch set.
    env = os.environ.copy()
    env["RUSTC"] = str(toolchain / "bin/rustc")
    env["RUSTC_BOOTSTRAP_SYNTHETIC_TARGET"] = "1"
    flags = ['--check-cfg=cfg(target_os,values("trueos","zkvm"))', "-A", "warnings", "-C", "panic=abort"]
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
    shared = ["--locked", "--offline", "-Z", "json-target-spec", "--manifest-path", str(manifest),
              "--no-default-features", "--features", "trueos-blueprint,trueos/default-global-allocator",
              "--config", str(config)]
    commands = {
        "metadata": ["metadata", "--format-version", "1", "--filter-platform", str(target), *shared],
        "tree": ["tree", "--prefix", "depth", "--edges", "normal,build", "--package", "codex-cli", "--target", str(target), *shared],
        "unit-graph": ["build", "--unit-graph", "-Z", "unstable-options", "-Z",
                       "build-std=core,compiler_builtins,alloc,std,panic_abort", "--bin", "codex", "--release",
                       "--target", str(target), *shared],
    }
    outputs = {"metadata": "metadata.json", "tree": "codex-depth-tree.txt", "unit-graph": "unit-graph.json"}
    evidence = [manifest, manifest.with_name("Cargo.lock"), target, config]
    before = {str(path): digest(path) for path in evidence}
    capture = {"captured_at_unix": time.time(), "manifest": str(manifest), "target": str(target),
               "rustflags": flags, "inputs_sha256": before, "commands": {},
               "scope": "Existing staged pack only. Unit graph plans a build; it does not compile or publish."}
    for name, tail in commands.items():
        command = [str(toolchain / "bin/cargo"), *tail]
        with (raw / outputs[name]).open("wb") as out, (raw / f"{name}.stderr").open("wb") as err:
            result = subprocess.run(command, cwd=manifest.parent, env=env, stdout=out, stderr=err)
        capture["commands"][name] = {"argv": command, "exit_code": result.returncode,
                                      "output_sha256": digest(raw / outputs[name])}
        if result.returncode:
            raise SystemExit(f"{name} failed; read {raw / (name + '.stderr')}")
    capture["inputs_unchanged"] = before == {str(path): digest(path) for path in evidence}
    capture["rustc_identity"] = subprocess.check_output([str(toolchain / "bin/rustc"), "-vV"], text=True)
    inventory_file = raw / "rlink-closure.json"
    old_inventory = json.loads(inventory_file.read_text()) if inventory_file.exists() else {}
    rlink = args.rlink or Path(old_inventory.get("root_path", ""))
    if not rlink.is_file():
        parser.error("selected native root is missing; supply --rlink for the actual existing pack cache")
    rlink = rlink.resolve()
    tokens = [token.decode("ascii") for token in re.findall(rb"[!-~]+", rlink.read_bytes())]
    archives, root_objects, seen = [], [], set()
    for token in tokens:
        if ".rlib" in token:
            candidate = token[:token.index(".rlib") + 5]
            path = Path(candidate[candidate.index("/"):]) if "/" in candidate else rlink.parent / candidate[candidate.rfind("lib"):]
            if path.is_file() and path not in seen:
                seen.add(path)
                archives.append({"path": str(path), "crate_stem": path.name.removeprefix("lib").rsplit("-", 1)[0], "archive_bytes": path.stat().st_size})
        if ".rcgu.o" in token and "/" in token:
            candidate = token[token.index("/"):token.index(".rcgu.o") + 7]
            path = Path(candidate)
            if path.is_file() and path.name.startswith(rlink.stem) and str(path) not in root_objects:
                root_objects.append(str(path))
    if not archives or not root_objects:
        raise SystemExit("could not recover the native closure from the selected .rlink")
    inventory = {"authority": "Packer-compatible printable-token extraction from the selected root .rlink",
                 "root_path": str(rlink), "root_mtime_utc": datetime.fromtimestamp(rlink.stat().st_mtime, timezone.utc).isoformat(),
                 "root_objects": root_objects, "archives": archives, "archive_count": len(archives),
                 "archive_bytes_total": sum(a["archive_bytes"] for a in archives),
                 "caution": "Archive input bytes include metadata and dead code; they are not retained payload contributions."}
    inventory_file.write_text(json.dumps(inventory, indent=2) + "\n")
    capture["native_root_sha256"] = digest(rlink)
    capture["native_inventory_sha256"] = digest(inventory_file)
    work = manifest.parent.parent
    capture["existing_artifacts"] = {str(path): {"bytes": path.stat().st_size, "mtime_unix": path.stat().st_mtime}
                                      for path in [work / "module.o", work / "module.stripped.o", work / "module.stripped.tar.lz4", repos / "TRUEOS-Blueprints/dist/codex.bp"] if path.is_file()}
    (raw / "capture.json").write_text(json.dumps(capture, indent=2) + "\n")
    if not capture["inputs_unchanged"]:
        raise SystemExit("staged inputs changed during capture")
    print("Captured locked, offline metadata, depth tree and build unit graph; staged inputs unchanged.")


if __name__ == "__main__":
    main()
