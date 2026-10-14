#!/usr/bin/env python3
"""Validate the integrated Codex BP and prove ordered folds preserve the full ELF."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import shlex
import struct
import subprocess
import tarfile
import tempfile


HERE = Path(__file__).resolve().parent
GRAPH = HERE.parent
REPOS = GRAPH.parents[4]
WORK = REPOS / "codex/codex-rs/cli/target/trueos-blueprint/work/codex"
TOOLBIN = REPOS / "TRUEOS-Rust-Toolchain-nightly-2026-07-10/lib/rustlib/x86_64-unknown-linux-gnu/bin"


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skip-direct-link", action="store_true")
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location("codex_elf_evidence", GRAPH / "experiments/coalesce/run.py")
    evidence = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(evidence)
    bp = REPOS / "TRUEOS-Blueprints/dist/codex.bp"
    stripped = WORK / "module.stripped.o"
    elf = evidence.elf_summary(stripped)
    assert elf["entry"] and elf["entry"]["executable_section"]
    assert elf["relocation_writes_in_bounds"]
    assert elf["section_count"] < 30, "Codex section-coalesce policy was not applied"
    # The child inherits the file descriptor position. Disable Python's
    # read-ahead so consuming the header leaves it exactly at the LZ4 frame.
    with bp.open("rb", buffering=0) as stream:
        magic, version, flags, entry_hint, compressed_length, raw_length = struct.unpack("<4sHHQII", stream.read(24))
        assert magic == b"TRBP" and version == 1
        assert compressed_length + 24 == bp.stat().st_size
        assert raw_length == stripped.stat().st_size
        assert entry_hint == elf["entry"]["section"] << 32 | elf["entry"]["value"]
        process = subprocess.Popen(["lz4", "-dc"], stdin=stream, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        archive = tarfile.open(fileobj=process.stdout, mode="r|")
        payload_hash = hashlib.sha256()
        members = []
        for member in archive:
            assert member.name == "module.elf" and member.isfile()
            assert member.size == raw_length
            members.append(member.name)
            source = archive.extractfile(member)
            while chunk := source.read(1024 * 1024):
                payload_hash.update(chunk)
        archive.close()
        stderr = process.stderr.read().decode(errors="replace")
        assert process.wait() == 0, stderr
        assert members == ["module.elf"]
        assert payload_hash.hexdigest() == digest(stripped)
    fold_path = WORK / "native-folds.json"
    fold = json.loads(fold_path.read_text())
    selected = [row["path"] for row in fold["archives"]]
    assert [row["input_index"] for row in fold["archives"]] == list(range(len(selected)))
    for run in fold["response_runs"]:
        expected = selected[run["first_input"]:run["past_last_input"]]
        observed = shlex.split(Path(run["response_file"]).read_text())
        assert observed == expected
        assert all(row["owned"] for row in fold["archives"][run["first_input"]:run["past_last_input"]])
    owned = [row for row in fold["archives"] if row["owned"]]
    assert len(owned) == sum(run["past_last_input"] - run["first_input"] for run in fold["response_runs"])
    stems = [Path(path).name for path in selected]
    assert sum(name.startswith("libpanic_abort-") for name in stems) == 1
    assert not any(name.startswith("libpanic_unwind-") for name in stems)
    inputs = [WORK / "app.rooted.o", WORK / "rel-coalesce.ld", *map(Path, selected)]
    before = {str(path): digest(path) for path in inputs}
    direct = None
    if not args.skip_direct_link:
        with tempfile.TemporaryDirectory(prefix="codex-direct-link-", dir=HERE) as directory:
            output = Path(directory) / "direct.o"
            command = [str(TOOLBIN / "rust-lld"), "-flavor", "gnu", "-r", "--gc-sections", "--discard-all", "-T", str(WORK / "rel-coalesce.ld"),
                       "--undefined", "_start", "-o", str(output), str(WORK / "app.rooted.o"), "--start-group", *selected, "--end-group"]
            completed = subprocess.run(command, capture_output=True, text=True)
            assert completed.returncode == 0, completed.stderr
            direct = {"exit_code": completed.returncode, "module_sha256": digest(output), "folded_module_sha256": digest(WORK / "module.o")}
            direct["byte_identical"] = direct["module_sha256"] == direct["folded_module_sha256"]
            assert direct["byte_identical"], "response folding changed the full native ELF"
    assert before == {str(path): digest(path) for path in inputs}
    result = {"blueprint": {"path": str(bp), "sha256": digest(bp), "bytes": bp.stat().st_size, "version": version, "flags": flags,
                             "entry_hint": f"{entry_hint:016x}", "compressed_bytes": compressed_length, "raw_bytes": raw_length,
                             "decompressed_payload_matches_elf": True},
              "elf": elf, "native_fold": {"selected_archives": len(selected), "owned_archives": len(owned),
                                           "response_runs": len(fold["response_runs"]), "exclusive_units": fold["exclusive_units"],
                                           "archive_order_preserved": True, "input_hashes_unchanged": True, "direct_link": direct},
              "scope": "Actual local package static validation and full native linker comparison; no rig launch or publication."}
    (HERE / "pack-validation.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps({"bp_bytes": result["blueprint"]["bytes"], "elf_sections": elf["section_count"],
                      "native_archives": len(selected), "direct_link_byte_identical": direct["byte_identical"] if direct else None}))


if __name__ == "__main__":
    main()
