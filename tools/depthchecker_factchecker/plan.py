#!/usr/bin/env python3
"""Choose real emitted dependency entries; fail on absent or ambiguous symbols."""
import argparse
import json
from pathlib import Path
from elfmap import ModuleMap


def build_plan(mapped, graph):
    def choose(*parts, suffix=None):
        names = {row["name"] for row in mapped.function_rows if all(p in row["name"] for p in parts)
                 and (suffix is None or row["name"].endswith(suffix))}
        if len(names) != 1:
            raise ValueError(f"Selector {parts} has {len(names)} emitted candidates: {sorted(names)}")
        return names.pop()

    def package(crate):
        matches = {u["pkg_id"] for u in graph["units"] if u["target"]["name"] == crate
                   and u["platform"] and u["mode"] == "build"}
        if len(matches) != 1:
            raise ValueError(f"Crate {crate} has ambiguous active package identity: {matches}")
        return matches.pop()

    def target(symbol, crate, reason):
        return {"symbol": symbol, "package_id": package(crate), "reason": reason}

    text_leaf = target(choose("core3str7pattern", "StrSearcher3new"), "core",
                       "Real terminal emitted core call beneath the text parser's find clone; next_match is inlined in that clone.")
    text_find = target(choose("core3str", "findRe", "codex_utils_text"), "core",
                       "Monomorphized core str::find in the text archive; keeps the compiler's actual call shape visible.")
    url_path = target(choose("3url6parser", "Parser10parse_path", suffix="10parse_path"), "url",
                      "File URI parsing branch: no DNS or IDNA host branch is requested by this fixture.")
    url_leaf = target(choose("3url6parser", "parse_path12push_pending"), "url",
                      "Deeper emitted URL path routine observed in the real parse_path relocation chain.")
    json_leaf = target(choose("serde_json4read", "SliceRead14skip_to_escape.llvm."), "serde_json",
                       "Deep emitted byte-scanning routine beneath protocol JSON deserialization.")
    def kernel(symbol, reason):
        return {"symbol": symbol, "scope": "kernel", "reason": reason}
    rows = [
        {"id": "text_stream", "index": 1, "targets": [text_find, text_leaf]},
        {"id": "path_resolution", "index": 2, "targets": [url_path, url_leaf]},
        {"id": "protocol_permissions", "index": 3, "targets": [json_leaf]},
        {"id": "transport_zstd", "index": 4, "targets": [
            target("ZSTD_compressStream2", "zstd_sys", "Actual native C entry underneath Codex Request::into_prepared."),
            target("ZSTD_decompressStream", "zstd_sys", "Actual native C decode entry used to fact-check compressed bytes.") ]},
        {"id": "cabi_clock", "index": 5, "targets": [kernel("trueos_time_monotonic_nanos", "Imported TRUEOS clock ABI implementation.")]},
        {"id": "native_thread", "index": 6, "allow_cross_thread": True, "targets": [
            text_leaf,
            kernel("fsync", "Same real file roundtrip on the native continuation."),
            kernel("trueos_thread_context_swap", "Actual native filesystem continuation parking leaf with probe ancestry.") ]},
        {"id": "filesystem", "index": 7, "targets": [
            kernel("fsync", "Regular file writeback ABI boundary."),
            kernel("trueos_kernel_sync_fs_read_file", "Hull filesystem bridge reached from std File::open; uses VMCALL, not native context_swap."),
            kernel("trueos_cabi_async_fs_metadata_start_v2", "Hull metadata is read through the host CABI, avoiding its private unmounted kfs globals.") ]},
    ]
    return {"version": 1, "stack_bytes": 262144, "max_frames": 128, "scenarios": rows,
            "scope": "Selected real emitted endpoints and ABI boundaries per deterministic path. Cargo leaves, machine frames and async causal chains have different depths. These samples do not claim exhaustive call tracing."}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("debug-elf", "packed-elf", "unit-graph", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    mapped = ModuleMap(args.debug_elf, args.packed_elf)
    try:
        plan = build_plan(mapped, json.loads(args.unit_graph.read_text()))
    finally:
        mapped.close()
    args.output.write_text(json.dumps(plan, indent=2) + "\n")


if __name__ == "__main__":
    main()
