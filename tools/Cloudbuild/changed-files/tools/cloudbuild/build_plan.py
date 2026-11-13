#!/usr/bin/env python3
"""Resolve checkbox selections to a strict, identity-free TRUEOS build manifest.

The server owns source_commit. Requesters can choose only catalog entries and
the profile, never repository URLs, command lines, environment, or Cargo flags.
The config hash identifies the effective source/configuration, not a person or
a promise of bit-for-bit reproducibility across unpinned dependency/toolchain
inputs. An operator must pin and record those inputs on the build worker.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
CATALOG_PATH = Path(__file__).with_name("catalog.json")
SCHEMA = 2
MAX_SPEC_BYTES = 64 * 1024
_COMMIT = re.compile(r"(?:[0-9a-f]{40}|[0-9a-f]{64})\Z")
_REQUEST_FIELDS = {"preset", "features", "buildins", "profile"}
_MANIFEST_FIELDS = {
    "schema", "source_commit", "profile", "features", "buildins", "cargo_features",
    "config_hash", "warnings", "command",
}


def _object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON field: {key}")
        result[key] = value
    return result


def _read_json(path: Path) -> Any:
    with path.open("rb") as stream:
        data = stream.read(MAX_SPEC_BYTES + 1)
    if len(data) > MAX_SPEC_BYTES:
        raise ValueError("build specification is too large")
    return json.loads(data, object_pairs_hook=_object)


def catalog() -> dict[str, Any]:
    """Return a fresh catalog for both the UI and the authoritative resolver."""
    return _read_json(CATALOG_PATH)


def canonical_bytes(value: Any) -> bytes:
    """The single serialization used for configuration hashes."""
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=True, allow_nan=False).encode("utf-8")


def _source_commit(value: Any) -> str:
    if not isinstance(value, str) or not _COMMIT.fullmatch(value):
        raise ValueError("source_commit must be a full lowercase Git commit hash")
    return value


def _keys(value: Any, allowed: set[str], label: str) -> dict[str, Any]:
    if not isinstance(value, dict) or any(not isinstance(key, str) for key in value):
        raise ValueError(f"{label} must be a JSON object")
    extra = set(value) - allowed
    if extra:
        raise ValueError(f"unknown {label} field(s): {', '.join(sorted(extra))}")
    return value


def buildin_selection(value: Any) -> list[str]:
    """Validate an exact app set and return its canonical order.

    App IDs are existing buildins, not arbitrary Blueprint names or source
    paths. Empty is the ordinary kernel-only packaging choice.
    """
    known = {app["id"] for app in catalog()["buildins"]}
    if not isinstance(value, list) or any(not isinstance(name, str) for name in value):
        raise ValueError("buildins must be a list of app IDs")
    if len(value) != len(set(value)):
        raise ValueError("buildins must not contain duplicate app IDs")
    unknown = set(value) - known
    if unknown:
        raise ValueError(f"unknown buildin(s): {', '.join(sorted(unknown))}")
    return sorted(value)


def buildins_from_cli(value: str) -> list[str]:
    """Parse the local all/none/CSV shorthand through the same allowlist."""
    if value == "all":
        return sorted(app["id"] for app in catalog()["buildins"])
    if value == "none":
        return []
    return buildin_selection(value.split(","))


def resolve_request(request: dict[str, Any], source_commit: str) -> dict[str, Any]:
    """Resolve an optional preset plus boolean overrides into a complete plan.

    Unknown fields, non-booleans and missing dependencies fail closed. Enabling
    a dependent does not silently re-enable a subsystem the caller disabled.
    The default preset exactly mirrors Cargo's default features.
    """
    request = _keys(request, _REQUEST_FIELDS, "request")
    source_commit = _source_commit(source_commit)
    definition = catalog()
    feature_map = {feature["id"]: feature for feature in definition["features"]}
    presets = {preset["id"]: preset for preset in definition["presets"]}

    preset = request.get("preset", definition["default_preset"])
    if not isinstance(preset, str) or preset not in presets:
        raise ValueError(f"preset must be one of: {', '.join(presets)}")
    profile = request.get("profile", "release")
    if not isinstance(profile, str) or profile not in ("release", "dev"):
        raise ValueError("profile must be release or dev")
    overrides = _keys(request.get("features", {}), set(feature_map), "features")
    for name, enabled in overrides.items():
        if type(enabled) is not bool:
            raise ValueError(f"feature {name} must be a boolean")

    selected = {name: name in presets[preset]["features"] for name in feature_map}
    selected.update(overrides)
    if "buildins" in request:
        buildins = buildin_selection(request["buildins"])
        if "buildins" in overrides and overrides["buildins"] != bool(buildins):
            raise ValueError("features.buildins conflicts with the explicit buildins list")
    elif "buildins" in overrides:
        # Preserve the original all/none API shorthand. App choices are
        # packaging, independent of every kernel capability checkbox.
        buildins = sorted(app["id"] for app in definition["buildins"]) if selected["buildins"] else []
    else:
        buildins = buildin_selection(presets[preset]["buildins"])
    selected["buildins"] = bool(buildins)
    for name, enabled in selected.items():
        if enabled:
            missing = [key for key in feature_map[name]["requires"] if not selected[key]]
            if missing:
                raise ValueError(
                    f"{name} requires {', '.join(missing)}; enable the dependency "
                    f"or disable {name}"
                )

    enabled = sorted(name for name, value in selected.items() if value)
    cargo_features = sorted(feature_map[name]["cargo_feature"] for name in enabled)
    identity = {
        "schema": SCHEMA,
        "source_commit": source_commit,
        "profile": profile,
        "features": enabled,
        "buildins": buildins,
        "cargo_features": cargo_features,
    }
    warnings = [
        "USB, Intel iGPU, and Virgl selections control device initialization; "
        "shared driver code remains compiled."
    ]
    if not selected["buildins"]:
        warnings.append(
            "Kernel only: no bundled file explorer, app store, or text editor. "
            "The app catalog starts empty; selected kernel capabilities remain available."
        )
    elif len(buildins) < len(definition["buildins"]):
        if "apps" not in buildins:
            warnings.append(
                "Appstore is not bundled; adding applications requires another available entry point."
            )
        if "hull" not in buildins:
            warnings.append(
                "Hull is not bundled. Default startup will not download the omitted desktop app."
            )
    if not selected["usb"]:
        warnings.append("USB input, storage, MIDI, and USB network devices will be unavailable.")
    if not selected["igpu"]:
        warnings.append(
            "Disabling Intel graphics also skips the current kernel's automatic "
            "Intel HDA audio probe."
        )
    if not selected["igpu"] and not selected["virgl"]:
        warnings.append(
            "Both graphics backends are disabled. Only the existing boot-framebuffer "
            "logo fallback remains; this is not a validated headless desktop."
        )
    if selected["igpu"] and selected["virgl"]:
        warnings.append("A claimed supported Intel GPU takes precedence over the Virgl fallback.")
    if selected["speech"]:
        warnings.append("Experimental speech needs separately provisioned runtime model assets.")
    return {
        **identity,
        "config_hash": hashlib.sha256(canonical_bytes(identity)).hexdigest(),
        "warnings": warnings,
        "command": ["bash", "tools/trueos-boot/prepare.sh", "--" + profile],
    }


def validate_manifest(manifest: dict[str, Any],
                      source_commit: str | None = None) -> dict[str, Any]:
    """Recompute every derived field; optionally enforce the worker's checkout.

    Returning the recomputed plan ensures even execution arguments are selected
    by server code, never trusted from a submitted manifest.
    """
    manifest = _keys(manifest, _MANIFEST_FIELDS, "manifest")
    if set(manifest) != _MANIFEST_FIELDS:
        raise ValueError("manifest is incomplete; resolve a request before building")
    if type(manifest["schema"]) is not int or manifest["schema"] != SCHEMA:
        raise ValueError("unsupported build manifest schema")
    commit = _source_commit(manifest["source_commit"])
    if source_commit is not None and commit != _source_commit(source_commit):
        raise ValueError("build manifest source_commit does not match the worker checkout")
    features = manifest["features"]
    known = {item["id"] for item in catalog()["features"]}
    if (not isinstance(features, list)
            or any(not isinstance(item, str) for item in features)
            or any(item not in known for item in features)
            or features != sorted(set(features))):
        raise ValueError("manifest features must be a sorted, unique list of catalog IDs")
    buildins = buildin_selection(manifest["buildins"])
    if buildins != manifest["buildins"]:
        raise ValueError("manifest buildins must be a sorted, unique list of app IDs")
    expected = resolve_request({
        "features": {name: name in features for name in known},
        "buildins": buildins,
        "profile": manifest["profile"],
    }, commit)
    if manifest != expected:
        raise ValueError("manifest has altered derived fields or an invalid config_hash")
    return expected


def cargo_args(manifest: dict[str, Any]) -> list[str]:
    """Return a fixed argument shape using only validated feature names."""
    plan = validate_manifest(manifest)
    result = ["--no-default-features"]
    if plan["cargo_features"]:
        result.extend(["--features", ",".join(plan["cargo_features"])])
    return result


def checkout_commit() -> str:
    result = subprocess.run(
        ["git", "-C", str(ROOT), "rev-parse", "HEAD"],
        check=True, capture_output=True, text=True, timeout=10,
    )
    return _source_commit(result.stdout.strip())


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    inputs = parser.add_mutually_exclusive_group(required=True)
    inputs.add_argument("--catalog", action="store_true", help="print the selector catalog")
    inputs.add_argument("--request", type=Path, help="resolve a request against this checkout")
    inputs.add_argument("--spec", type=Path, help="validate a normalized build manifest")
    inputs.add_argument("--kernel-only", action="store_true", help="resolve the regular no-buildins preset")
    inputs.add_argument("--buildins", metavar="all|none|CSV", help="resolve an exact bundled app set with default kernel features")
    outputs = parser.add_mutually_exclusive_group()
    outputs.add_argument("--cargo-args", action="store_true", help="print one Cargo argument per line")
    outputs.add_argument("--buildins-mode", action="store_true", help="print enabled or none for the app payload")
    outputs.add_argument("--buildins-list", action="store_true", help="print the exact selected app IDs as CSV (empty for none)")
    parser.add_argument("--profile", choices=("release", "dev"), help="require this build profile")
    args = parser.parse_args()
    try:
        if args.catalog:
            if args.cargo_args or args.buildins_mode or args.buildins_list or args.profile:
                raise ValueError("--catalog cannot be combined with build options")
            print(json.dumps(catalog(), indent=2))
            return
        commit = checkout_commit()
        if args.spec:
            plan = validate_manifest(_read_json(args.spec), commit)
        elif args.kernel_only:
            plan = resolve_request({"preset": "kernel-only", "profile": args.profile or "release"}, commit)
        elif args.buildins is not None:
            plan = resolve_request({"buildins": buildins_from_cli(args.buildins),
                                    "profile": args.profile or "release"}, commit)
        else:
            plan = resolve_request(_read_json(args.request), commit)
        if args.profile is not None and plan["profile"] != args.profile:
            raise ValueError("manifest profile does not match prepare.sh profile")
        if args.buildins_mode:
            print("enabled" if plan["buildins"] else "none")
        elif args.buildins_list:
            print(",".join(plan["buildins"]))
        elif args.cargo_args:
            print("\n".join(cargo_args(plan)))
        else:
            print(json.dumps(plan, indent=2, sort_keys=True))
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(2, f"cloud build plan: {error}\n")


if __name__ == "__main__":
    main()
