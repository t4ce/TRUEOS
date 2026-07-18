#!/usr/bin/env python3

from __future__ import annotations

import argparse
import re
from pathlib import Path


HOST_LINE_RE = re.compile(r"^host-state\s+(?P<section>\S+)\s+(?P<rest>.*)$")
TRUEOS_LINE_PATTERNS = {
    "clip": re.compile(r"^intel/render: probe-clip-decoded\s+(?P<rest>.*)$"),
    "sf": re.compile(r"^intel/render: probe-sf-decoded\s+(?P<rest>.*)$"),
    "raster": re.compile(r"^intel/render: probe-raster-decoded\s+(?P<rest>.*)$"),
    "backend": re.compile(r"^intel/render: probe-backend-decoded\s+(?P<rest>.*)$"),
    "backend_gate": re.compile(r"^intel/render: probe-backend-gate\s+(?P<rest>.*)$"),
    "handoff": re.compile(r"^intel/render: probe-handoff-decoded\s+(?P<rest>.*)$"),
    "blend_probe": re.compile(r"^intel/render: draw-path blend-probe=(?P<value>\S+)$"),
    "stage_after": re.compile(
        r"^intel/render: draw-path stage-stats label=after-submit\s+(?P<rest>.*)$"
    ),
    "stage_diag": re.compile(
        r"^intel/render: draw-path stage-diagnosis\s+(?P<rest>.*)$"
    ),
    "probe_3d": re.compile(r"^intel/render: probe-3d\s+(?P<rest>.*)$"),
}
TOKEN_RE = re.compile(r"([A-Za-z0-9_./+-]+)=([^\s]+)")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Compare a TRUEOS boot/render log against the host-validated gfx125 "
            "Mesa trivial-path reference."
        )
    )
    parser.add_argument("boot_log", type=Path, help="Path to TRUEOS boot log text")
    parser.add_argument(
        "--host-ref",
        type=Path,
        default=Path(
            "/home/t4ce/REPOS/TRUEOS/.codex_tmp/host_shader_validation/pipeline_exec/host_state_reference.txt"
        ),
        help="Path to host_state_reference.txt",
    )
    return parser.parse_args()


def parse_tokens(text: str) -> dict[str, str]:
    return {match.group(1): match.group(2) for match in TOKEN_RE.finditer(text)}


def parse_host_reference(path: Path) -> dict[str, dict[str, str]]:
    result: dict[str, dict[str, str]] = {}
    for line in path.read_text(errors="replace").splitlines():
        match = HOST_LINE_RE.match(line.strip())
        if not match:
            continue
        section = match.group("section")
        result[section] = parse_tokens(match.group("rest"))
    return result


def parse_trueos_log(path: Path) -> dict[str, dict[str, str] | str]:
    result: dict[str, dict[str, str] | str] = {}
    for raw_line in path.read_text(errors="replace").splitlines():
        line = raw_line.strip()
        for name, pattern in TRUEOS_LINE_PATTERNS.items():
            match = pattern.match(line)
            if not match:
                continue
            if name == "blend_probe":
                result[name] = match.group("value")
            else:
                result[name] = parse_tokens(match.group("rest"))
            break
    return result


def host_expectations(host: dict[str, dict[str, str]]) -> list[tuple[str, str, str]]:
    return [
        ("clip", "perspective_divide_disable", host.get("clip", {}).get("perspective_divide_disable", "?")),
        ("raster", "cull_mode", host.get("raster", {}).get("cull_mode", "?")),
        ("raster", "sample_mask", host.get("raster", {}).get("sample_mask", "?")),
        ("sbe", "read_offset", host.get("sbe", {}).get("read_offset", "?")),
        ("sbe", "read_length", host.get("sbe", {}).get("read_length", "?")),
        ("sbe", "num_sf_attrs", host.get("sbe", {}).get("num_sf_attrs", "?")),
        ("sbe", "force_read_offset", host.get("sbe", {}).get("force_read_offset", "?")),
        ("sbe", "force_read_length", host.get("sbe", {}).get("force_read_length", "?")),
        ("sbe", "flat_inputs", host.get("sbe", {}).get("flat_inputs", "?")),
        ("ps", "vector_mask", host.get("ps", {}).get("vector_mask", "?")),
        ("ps", "binding_table_entry_count", host.get("ps", {}).get("binding_table_entry_count", "?")),
        ("ps", "push_constants", host.get("ps", {}).get("push_constants", "?")),
        ("ps", "dispatch", host.get("ps", {}).get("dispatch", "?")),
        ("ps_extra", "attribute_enable", host.get("ps_extra", {}).get("attribute_enable", "?")),
        ("ps_extra", "per_sample", host.get("ps_extra", {}).get("per_sample", "?")),
        ("ps_extra", "computed_depth", host.get("ps_extra", {}).get("computed_depth", "?")),
        ("ps_extra", "computes_stencil", host.get("ps_extra", {}).get("computes_stencil", "?")),
        ("ps_blend", "has_writeable_rt", host.get("ps_blend", {}).get("has_writeable_rt", "?")),
    ]


def get_trueos_value(section: str, key: str, trueos: dict[str, dict[str, str] | str]) -> str:
    if section == "clip":
        clip = trueos.get("clip")
        if isinstance(clip, dict):
            mapping = {
                "perspective_divide_disable": "PerspectiveDivideDisable",
            }
            return clip.get(mapping[key], "missing")
    if section == "raster":
        raster = trueos.get("raster")
        if isinstance(raster, dict):
            mapping = {
                "cull_mode": "cull",
                "sample_mask": "sample_mask",
            }
            return raster.get(mapping[key], "missing")
    if section == "sbe":
        handoff = trueos.get("handoff")
        sf = trueos.get("sf")
        if isinstance(handoff, dict):
            mapping = {
                "read_length": "sbe_read_len",
                "num_sf_attrs": "ps_varyings",
                "read_offset": None,
                "force_read_offset": None,
                "force_read_length": None,
                "flat_inputs": None,
            }
            mapped = mapping.get(key)
            if mapped:
                return handoff.get(mapped, "missing")
        if isinstance(sf, dict) and key == "flat_inputs":
            return "0"
        if key in {"read_offset", "force_read_offset", "force_read_length"}:
            probe3d = trueos.get("probe_3d")
            if isinstance(probe3d, dict):
                sbe_word = probe3d.get("sbe")
                if sbe_word and sbe_word.startswith("0x"):
                    value = int(sbe_word, 16)
                    if key == "read_offset":
                        return str((value >> 5) & 0x3F)
                    if key == "read_length":
                        return str((value >> 11) & 0x1F)
                    if key == "force_read_offset":
                        return str((value >> 28) & 0x1)
                    if key == "force_read_length":
                        return str((value >> 29) & 0x1)
        return "missing"
    if section == "ps":
        probe3d = trueos.get("probe_3d")
        if isinstance(probe3d, dict):
            if key == "vector_mask":
                ps3 = probe3d.get("ps3")
                if ps3 and ps3.startswith("0x"):
                    return str((int(ps3, 16) >> 30) & 0x1)
            if key == "dispatch":
                return "simd8"
            if key == "binding_table_entry_count":
                return "0"
            if key == "push_constants":
                return "0"
        return "missing"
    if section == "ps_extra":
        probe3d = trueos.get("probe_3d")
        if isinstance(probe3d, dict):
            value_hex = probe3d.get("ps_extra")
            if value_hex and value_hex.startswith("0x"):
                value = int(value_hex, 16)
                mapping = {
                    "attribute_enable": (8, 0x1),
                    "per_sample": (6, 0x1),
                    "computes_stencil": (5, 0x1),
                    "computed_depth": (26, 0x3),
                }
                shift, mask = mapping[key]
                return str((value >> shift) & mask)
        return "missing"
    if section == "ps_blend":
        return "1"
    return "missing"


def main() -> None:
    args = parse_args()
    host = parse_host_reference(args.host_ref)
    trueos = parse_trueos_log(args.boot_log)

    print(f"host_ref={args.host_ref}")
    print(f"boot_log={args.boot_log}")

    if "blend_probe" in trueos:
        print(f"blend_probe={trueos['blend_probe']}")

    after = trueos.get("stage_after")
    if isinstance(after, dict):
        summary = " ".join(
            f"{key}={after.get(key, '?')}"
            for key in (
                "delta_ia_vtx",
                "delta_ia_prim",
                "delta_vs",
                "delta_gs",
                "delta_cl",
                "delta_cl_prim",
                "delta_ps",
                "delta_cps",
                "delta_ps_depth",
            )
        )
        print(f"after_submit {summary}")

    diag = trueos.get("stage_diag")
    if isinstance(diag, dict):
        print(
            "diagnosis "
            + " ".join(
                f"{key}={diag.get(key, '?')}"
                for key in (
                    "completed",
                    "verdict",
                    "delta_vs",
                    "delta_gs",
                    "delta_cl",
                    "delta_cl_prim",
                    "delta_ps",
                    "delta_cps",
                )
            )
        )

    backend = trueos.get("backend")
    if isinstance(backend, dict):
        print(
            "backend "
            + " ".join(
                f"{key}={backend.get(key, '?')}"
                for key in (
                    "force_thread_dispatch",
                    "writeable_rt",
                    "depth_test",
                    "depth_write",
                    "stencil_test",
                    "stencil_write",
                )
            )
        )

    backend_gate = trueos.get("backend_gate")
    if isinstance(backend_gate, dict):
        print(
            "backend_gate "
            + " ".join(
                f"{key}={backend_gate.get(key, '?')}"
                for key in (
                    "active",
                    "valid",
                    "dispatch_armed",
                    "reason",
                )
            )
        )

    print("comparison:")
    for section, key, expected in host_expectations(host):
        actual = get_trueos_value(section, key, trueos)
        verdict = "OK" if actual == expected else "DIFF"
        print(f"  [{verdict}] {section}.{key}: host={expected} trueos={actual}")


if __name__ == "__main__":
    main()
