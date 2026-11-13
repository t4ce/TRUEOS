#!/usr/bin/env python3
"""Validated manual Actions builds and their unsigned provenance bundle.

The CI path builds an operator-approved configuration. It does not verify a
customer challenge or issue a license; the cloud queue owns that separate flow.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
from typing import Any, Mapping

import build_plan
from buildins import preparation_record


ROOT = build_plan.ROOT
EXPECTED_RUSTC_COMMIT = "af3d95584dbddcae597890340995509a7fb47a50"
DEPENDENCIES = {
    "buildins": "TRUEOS-Buildins",
    "blueprints": "TRUEOS-Blueprints",
    "toolchain": "TRUEOS-Rust-Toolchain-nightly-2026-07-10",
}


def dependencies_for(manifest: Mapping[str, Any]) -> dict[str, str]:
    """Return only the source repositories consumed by this validated plan."""
    return {
        name: repository for name, repository in DEPENDENCIES.items()
        if name != "buildins" or manifest["buildins"]
    }


def validate_inputs(environ: Mapping[str, str], checkout: str) -> tuple[dict, dict]:
    """Read dispatcher data as data, bound its size, and pin every source."""
    request_id = environ.get("CLOUDBUILD_REQUEST_ID", "")
    if request_id and re.fullmatch(r"[a-f0-9]{32}", request_id) is None:
        raise ValueError("request_id must be an opaque 32-character hexadecimal ID")
    expected = environ.get("GITHUB_SHA", "")
    if checkout != expected:
        raise ValueError("checked-out TRUEOS commit does not match GITHUB_SHA")
    raw = environ.get("CLOUDBUILD_MANIFEST", "")
    if not raw or len(raw.encode("utf-8")) > build_plan.MAX_SPEC_BYTES:
        raise ValueError("manifest JSON is empty or too large")
    manifest = build_plan.validate_manifest(
        json.loads(raw, object_pairs_hook=build_plan._object), checkout
    )
    active_dependencies = dependencies_for(manifest)
    pins = {}
    for name in DEPENDENCIES:
        value = environ.get(f"CLOUDBUILD_{name.upper()}_COMMIT", "")
        if name not in active_dependencies and not value:
            continue
        if re.fullmatch(r"[0-9a-f]{40}", value) is None:
            unused = " (or empty when buildins are disabled)" if name not in active_dependencies else ""
            raise ValueError(f"{name}_commit must be a full lowercase 40-character Git SHA{unused}")
        if name in active_dependencies:
            pins[name] = value
    return manifest, pins


def write_inputs(environ: Mapping[str, str]) -> None:
    manifest, pins = validate_inputs(environ, build_plan.checkout_commit())
    directory = ROOT / "tgt/cloudbuild"
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "build-manifest.json").write_text(
        json.dumps(manifest, sort_keys=True, indent=2) + "\n", encoding="utf-8"
    )
    (directory / "dependency-pins.json").write_text(
        json.dumps(pins, sort_keys=True, indent=2) + "\n", encoding="utf-8"
    )
    # All values have been constrained to a hex SHA or an allowlisted profile.
    # No user-controlled string can inject a second Actions output line.
    with Path(environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as output:
        for name, commit in pins.items():
            output.write(f"{name}_commit={commit}\n")
        output.write(f"buildins_enabled={'true' if manifest['buildins'] else 'false'}\n")
        output.write(f"profile={manifest['profile']}\n")
        output.write(f"config_hash={manifest['config_hash']}\n")
    print(f"Validated configuration {manifest['config_hash']} at {manifest['source_commit']}")


def _command(argv: list[str], directory: Path) -> str:
    return subprocess.run(
        argv, cwd=directory, check=True, text=True, capture_output=True, timeout=30
    ).stdout.strip()


def _sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_sources() -> dict[str, Any]:
    """Recheck checked-out dependencies before invoking their build scripts."""
    directory = ROOT / "tgt/cloudbuild"
    manifest = build_plan.validate_manifest(
        build_plan._read_json(directory / "build-manifest.json"), build_plan.checkout_commit()
    )
    pins = build_plan._read_json(directory / "dependency-pins.json")
    active_dependencies = dependencies_for(manifest)
    if not isinstance(pins, dict) or set(pins) != set(active_dependencies):
        raise ValueError("dependency pins are incomplete or altered")
    sources = {"trueos": {"repository": "t4ce/TRUEOS", "commit": manifest["source_commit"]}}
    for name, repository in active_dependencies.items():
        commit = _command(["git", "rev-parse", "HEAD"], ROOT.parent / repository)
        if commit != pins[name] or re.fullmatch(r"[0-9a-f]{40}", commit) is None:
            raise ValueError(f"checked-out {name} does not match its validated immutable SHA")
        sources[name] = {"repository": f"t4ce/{repository}", "commit": commit}
    return {"manifest": manifest, "sources": sources}


def collect_artifact(environ: Mapping[str, str]) -> Path:
    checked = verify_sources()
    manifest = checked["manifest"]
    rustc = _command(["rustc", "-vV"], ROOT)
    if f"commit-hash: {EXPECTED_RUSTC_COMMIT}" not in rustc.splitlines():
        raise ValueError("the active Rust compiler is not the required archived TRUEOS compiler")
    cargo = _command(["cargo", "-V"], ROOT)
    source = ROOT / "tgt" / ("rigreal.tar.lz4" if manifest["profile"] == "release" else "rigdev.tar.lz4")
    if not source.resolve().is_relative_to(ROOT / "tgt"):
        raise ValueError("build archive points outside the build directory")
    with source.open("rb") as stream:
        if stream.read(4) != b"\x04\x22\x4d\x18":
            raise ValueError("prepared archive is not an LZ4 frame")
    lock = ROOT / "Cargo.lock"
    if not lock.is_file():
        raise ValueError("the build did not produce the required Cargo.lock")
    preparation = preparation_record(
        ROOT / "tgt/buildins" / manifest["profile"], manifest["profile"], manifest["buildins"]
    )

    # A fresh runner has no previous artifact. Fail rather than mixing output
    # from a failed or repeated collection with the current generation.
    artifact = ROOT / "tgt/cloudbuild/artifact"
    artifact.mkdir(parents=True, exist_ok=False)
    shutil.copyfile(source, artifact / "TrueOS.tar.lz4")
    shutil.copyfile(lock, artifact / "Cargo.lock")
    (artifact / "build-manifest.json").write_text(
        json.dumps(manifest, sort_keys=True, indent=2) + "\n", encoding="utf-8"
    )
    lock_paths = [
        lock,
        ROOT / "tools/trueos-boot/Cargo.lock",
        ROOT.parent / DEPENDENCIES["blueprints"] / "Cargo.lock",
    ]
    if manifest["buildins"]:
        lock_paths.append(ROOT.parent / DEPENDENCIES["buildins"] / "Cargo.lock")
    provenance = {
        "schema": 1,
        "kind": "trueos-manual-cloudbuild",
        "created_at": datetime.now(timezone.utc).isoformat(),
        "config_hash": manifest["config_hash"],
        "sources": checked["sources"],
        "submodules": _command(["git", "submodule", "status", "--recursive"], ROOT).splitlines(),
        "profile": manifest["profile"],
        "buildins_enabled": bool(manifest["buildins"]),
        "buildins": manifest["buildins"],
        "buildins_preparation": preparation,
        "cargo_features": manifest["cargo_features"],
        "rustc_verbose_version": rustc,
        "cargo_version": cargo,
        "lockfiles_sha256": {
            str(path.relative_to(ROOT.parent)): _sha256(path)
            for path in lock_paths if path.is_file()
        },
        "artifact": {"file": "TrueOS.tar.lz4", "sha256": _sha256(artifact / "TrueOS.tar.lz4")},
        "workflow": {
            "repository": environ.get("GITHUB_REPOSITORY", ""),
            "run_id": environ.get("GITHUB_RUN_ID", ""),
            "run_attempt": environ.get("GITHUB_RUN_ATTEMPT", ""),
            "workflow_ref": environ.get("GITHUB_WORKFLOW_REF", ""),
            "runner_image": environ.get("ImageOS", ""),
            "runner_image_version": environ.get("ImageVersion", ""),
        },
        "reproducibility_note": (
            "config_hash covers TRUEOS source and effective configuration only; it is not "
            "a complete reproducible cache key. This record also captures actual sibling "
            "revisions, compiler identity and lockfile hashes. Dependency resolution, "
            "host packages and other build inputs are not proven hermetic."
        ),
        "license_note": "Unsigned build provenance. This workflow does not issue a customer license.",
    }
    (artifact / "provenance.json").write_text(
        json.dumps(provenance, sort_keys=True, indent=2) + "\n", encoding="utf-8"
    )
    checksums = "".join(
        f"{_sha256(path)}  {path.name}\n" for path in sorted(artifact.iterdir()) if path.is_file()
    )
    (artifact / "SHA256SUMS").write_text(checksums, encoding="utf-8")
    return artifact


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("validate-inputs", "verify-sources", "collect-artifact"))
    args = parser.parse_args()
    try:
        if args.action == "validate-inputs":
            write_inputs(os.environ)
        elif args.action == "verify-sources":
            verify_sources()
            print("All source checkouts match their validated immutable revisions.")
        else:
            print(collect_artifact(os.environ))
    except (OSError, KeyError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(2, f"cloud build CI: {error}\n")


if __name__ == "__main__":
    main()
