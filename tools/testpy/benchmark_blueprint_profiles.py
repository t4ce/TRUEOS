#!/usr/bin/env python3
"""Build all build-ins at opt-level 3 and s; compare tar/LZ4 size and CPU cost."""
import argparse
import ctypes as c
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import statistics
import struct
import subprocess
import tarfile
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
SETTINGS = {
    "CODEGEN_UNITS": "1", "DEBUG": "full", "LTO": "true",
    "PANIC": "abort", "SPLIT_DEBUGINFO": "packed", "STRIP": "symbols",
    "INCREMENTAL": "true",
}


def audit_compiler(log, app, level):
    invocations = [json.loads(line) for line in log.read_text().splitlines()]
    roots = [args for args in invocations if "--crate-name" in args
             and args[args.index("--crate-name") + 1] == app.replace("-", "_")]
    if not roots:
        raise RuntimeError(f"no compiler invocation captured for {app}")
    args = roots[-1]
    codegen = [args[i + 1] for i, value in enumerate(args[:-1]) if value == "-C"]
    codegen += [value[2:] for value in args if value.startswith("-C") and value != "-C"]
    required = [f"opt-level={level}", "codegen-units=1", "debuginfo=2",
                "lto", "panic=abort", "split-debuginfo=packed", "strip=symbols"]
    missing = [value for value in required if value not in codegen]
    if not any(value.startswith("incremental=") for value in codegen):
        missing.append("incremental=true")
    if missing:
        raise RuntimeError(f"compiler settings not applied: {missing}")
    return codegen


def pin_measurement_cpu():
    allowed = os.sched_getaffinity(0)
    def frequency(cpu):
        path = Path(f"/sys/devices/system/cpu/cpu{cpu}/cpufreq/cpuinfo_max_freq")
        return int(path.read_text()) if path.exists() else 0
    selected = max(allowed, key=lambda cpu: (frequency(cpu), -cpu))
    os.sched_setaffinity(0, {selected})
    return selected


class FrameInfo(c.Structure):
    _fields_ = [("block_size", c.c_int), ("block_mode", c.c_int),
                ("content_checksum", c.c_int), ("frame_type", c.c_int),
                ("content_size", c.c_ulonglong), ("dictionary", c.c_uint),
                ("block_checksum", c.c_int)]


class Preferences(c.Structure):
    _fields_ = [("frame", FrameInfo), ("level", c.c_int),
                ("auto_flush", c.c_uint), ("favor_decode", c.c_uint),
                ("reserved", c.c_uint * 3)]


class Lz4:
    def __init__(self):
        self.lib = c.CDLL("liblz4.so.1")
        self.lib.LZ4F_isError.argtypes = [c.c_size_t]
        self.lib.LZ4F_getErrorName.argtypes = [c.c_size_t]
        self.lib.LZ4F_getErrorName.restype = c.c_char_p
        self.lib.LZ4F_compressFrameBound.argtypes = [c.c_size_t, c.POINTER(Preferences)]
        self.lib.LZ4F_compressFrameBound.restype = c.c_size_t
        self.lib.LZ4F_compressFrame.argtypes = [c.c_void_p, c.c_size_t, c.c_void_p,
                                              c.c_size_t, c.POINTER(Preferences)]
        self.lib.LZ4F_compressFrame.restype = c.c_size_t
        self.lib.LZ4F_createDecompressionContext.argtypes = [c.POINTER(c.c_void_p), c.c_uint]
        self.lib.LZ4F_createDecompressionContext.restype = c.c_size_t
        self.lib.LZ4F_decompress.argtypes = [c.c_void_p, c.c_void_p, c.POINTER(c.c_size_t),
                                           c.c_void_p, c.POINTER(c.c_size_t), c.c_void_p]
        self.lib.LZ4F_decompress.restype = c.c_size_t
        self.lib.LZ4F_freeDecompressionContext.argtypes = [c.c_void_p]

    def checked(self, result):
        if self.lib.LZ4F_isError(result):
            raise RuntimeError(self.lib.LZ4F_getErrorName(result).decode())
        return result

    def decode(self, frame, capacity):
        ctx = c.c_void_p()
        self.checked(self.lib.LZ4F_createDecompressionContext(c.byref(ctx), 100))
        try:
            output = c.create_string_buffer(capacity)
            src = c.c_size_t(len(frame)); dst = c.c_size_t(capacity)
            start = time.perf_counter_ns()
            remaining = self.checked(self.lib.LZ4F_decompress(
                ctx, output, c.byref(dst), frame, c.byref(src), None))
            elapsed = (time.perf_counter_ns() - start) / 1e6
            if remaining or src.value != len(frame):
                raise RuntimeError("incomplete frame decode")
            return output.raw[:dst.value], elapsed
        finally:
            self.lib.LZ4F_freeDecompressionContext(ctx)

    def encode_samples(self, tar, count=7):
        prefs = Preferences()
        prefs.frame = FrameInfo(7, 1, 1, 0, len(tar), 0, 0)
        prefs.level = 12
        capacity = self.checked(self.lib.LZ4F_compressFrameBound(len(tar), c.byref(prefs)))
        output = c.create_string_buffer(capacity)
        samples = []
        for _ in range(count + 1):
            start = time.perf_counter_ns()
            size = self.checked(self.lib.LZ4F_compressFrame(
                output, capacity, tar, len(tar), c.byref(prefs)))
            samples.append((time.perf_counter_ns() - start) / 1e6)
        encoded = output.raw[:size]
        assert self.decode(encoded, len(tar))[0] == tar
        return statistics.median(samples[1:])


def verify_trueos_decoder(frame, payload):
    decoder = ROOT / "bld/lz4-host-tests/release/trueos-lz4-archive-test"
    with tempfile.TemporaryDirectory(prefix="blueprint-profile-decode-") as td:
        source = Path(td) / "payload.lz4"; restored = Path(td) / "module.elf"
        source.write_bytes(frame)
        subprocess.run([str(decoder), "profile-check", str(source), str(restored)], check=True)
        assert restored.read_bytes() == payload
        timing = subprocess.check_output([str(decoder), "benchmark-decode", str(source)], text=True)
        return {"trueos_cpu_decompression_ms": float(timing.strip().split("=")[1]),
                "trueos_cpu_verified": True}


def measure(bp, lz4):
    data = bp.read_bytes()
    if data[:4] != b"TRBP":
        raise RuntimeError("invalid Blueprint magic")
    version, flags, entry, packed, raw = struct.unpack("<HHQII", data[4:24])
    if flags & 255 != 3 or len(data) != packed + 24:
        raise RuntimeError("expected a complete tar/LZ4 Blueprint")
    frame = data[24:]
    tar, _ = lz4.decode(frame, raw + 8192)
    with tarfile.open(fileobj=io.BytesIO(tar)) as archive:
        files = [member for member in archive if member.isfile()]
        if len(files) != 1:
            raise RuntimeError("Blueprint bundle must have one regular member")
        payload = archive.extractfile(files[0]).read()
    if len(payload) != raw or payload[:4] != b"\x7fELF":
        raise RuntimeError("decoded ELF size/magic mismatch")
    # Verify the CLI decoder independently, outside the timing loop.
    cli = subprocess.run(["lz4", "-dc"], input=frame, capture_output=True, check=True)
    assert cli.stdout == tar
    samples = [lz4.decode(frame, len(tar))[1] for _ in range(101)]
    result = {
        "raw_elf_bytes": raw, "tar_bytes": len(tar), "lz4_payload_bytes": packed,
        "restored_elf_bytes": len(payload),
        "bp_bytes": len(data), "compression_ms": lz4.encode_samples(tar),
        "decompression_ms": statistics.median(samples[1:]),
        "raw_sha256": hashlib.sha256(payload).hexdigest(), "roundtrip": True,
    }
    result.update(verify_trueos_decoder(frame, payload))
    return result


def report(output, apps, rows):
    (output / "results.json").write_text(json.dumps(rows, indent=2) + "\n")
    fields = ["app", "profile", "opt_level", "raw_elf_bytes", "tar_bytes",
              "lz4_payload_bytes", "bp_bytes", "restored_elf_bytes", "compression_ms",
              "decompression_ms", "trueos_cpu_decompression_ms", "build_seconds",
              "roundtrip", "trueos_cpu_verified", "raw_sha256", "error"]
    with (output / "results.csv").open("w", newline="") as stream:
        writer = csv.DictWriter(stream, fieldnames=fields, extrasaction="ignore")
        writer.writeheader(); writer.writerows(rows)
    lines = ["# Blueprint release profile comparison", "",
             "Shared settings: codegen-units=1, debug=full, lto=true, panic=abort,",
             "split-debuginfo=packed, strip=symbols, incremental=true.",
             "Panic remains abort as specified. The pinned compiler rejects the obsolete",
             "panic_immediate_abort build-std feature; it is not enabled.", "",
             "Compression: standard tar/LZ4, level 12, independent blocks.",
             "Times are warm in-memory Linux CPU medians using liblz4:",
             "7 encoding samples and 100 decoding samples; no file I/O or tar extraction.",
             "Final timing measurements are pinned to one CPU core.",
             "ELF sizes are loader-compatible payloads after the packager removes debug data.",
             "TRUEOS CPU timings use the production frame decoder compiled for Linux, including allocation.",
             "These are not application-runtime or native-iGPU speed measurements.", "",
             "| Build-in | Profile | Raw ELF KiB | tar KiB | LZ4 KiB | Saved | Encode ms | liblz4 decode ms | TRUEOS CPU decode ms | Verified |",
             "|---|---|---:|---:|---:|---:|---:|---:|---:|---|"]
    for app in apps:
        for profile in ["speed", "size"]:
            row = next((r for r in rows if r["app"] == app and r["profile"] == profile), None)
            if row is None:
                lines.append(f"| {app} | {profile} | pending | | | | | | | |")
            elif "error" in row:
                lines.append(f"| {app} | {profile} | FAILED | | | | | | | See log |")
            else:
                raw = row["raw_elf_bytes"]; packed = row["lz4_payload_bytes"]
                lines.append(f"| {app} | {profile} | {raw/1024:.2f} | {row['tar_bytes']/1024:.2f} | "
                             f"{packed/1024:.2f} | {(1-packed/raw)*100:.1f}% | "
                             f"{row['compression_ms']:.3f} | {row['decompression_ms']:.3f} | "
                             f"{row.get('trueos_cpu_decompression_ms', float('nan')):.3f} | Yes |")
    (output / "report.md").write_text("\n".join(lines) + "\n")
    comparison = ["# Speed versus size optimization", "",
                  "Both variants use the same requested release settings; only opt-level changes.",
                  "Payload sizes exclude the 24-byte Blueprint header. Units: KiB.", "",
                  "| Build-in | Raw 3 | LZ4 3 | Raw s | LZ4 s | Compressed change s vs 3 |",
                  "|---|---:|---:|---:|---:|---:|"]
    pairs = []
    for app in apps:
        pair = {row["profile"]: row for row in rows if row["app"] == app and "error" not in row}
        if len(pair) != 2:
            continue
        speed = pair["speed"]; size = pair["size"]; pairs.append((speed, size))
        delta = (size["lz4_payload_bytes"] / speed["lz4_payload_bytes"] - 1) * 100
        comparison.append(f"| {app} | {speed['raw_elf_bytes']/1024:.2f} | "
                          f"{speed['lz4_payload_bytes']/1024:.2f} | {size['raw_elf_bytes']/1024:.2f} | "
                          f"{size['lz4_payload_bytes']/1024:.2f} | {delta:+.2f}% |")
    if len(pairs) == len(apps):
        totals = [sum(pair[index][field] for pair in pairs)
                  for index, field in [(0, "raw_elf_bytes"), (0, "lz4_payload_bytes"),
                                       (1, "raw_elf_bytes"), (1, "lz4_payload_bytes")]]
        comparison.append(f"| **Total** | **{totals[0]/1024:.2f}** | **{totals[1]/1024:.2f}** | "
                          f"**{totals[2]/1024:.2f}** | **{totals[3]/1024:.2f}** | "
                          f"**{(totals[3]/totals[1]-1)*100:+.2f}%** |")
    comparison += ["", "After decompression, every ELF must match its original bytes exactly.",
                   "Application runtime and native iGPU decode performance are not measured here."]
    (output / "comparison.md").write_text("\n".join(comparison) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--blueprints", type=Path, default=ROOT.parent / "TRUEOS-Blueprints")
    parser.add_argument("--output", type=Path, default=ROOT / "bld/blueprint-profile-comparison")
    args = parser.parse_args()
    repo = args.blueprints.resolve(); output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    with (output / "decoder-build.log").open("w") as stream:
        subprocess.run(["python3", str(ROOT / "tools/testpy/test_lz4_archive.py")],
                       stdout=stream, stderr=subprocess.STDOUT, check=True)
    apps = json.loads((repo / "buildins.json").read_text())["buildins"]
    binary = repo / "target/x86_64-unknown-linux-gnu/debug/trueos-blueprint"
    if not binary.exists():
        raise RuntimeError("build the host trueos-blueprint packager first")
    wrapper = output / "rustc-wrapper.py"
    wrapper.write_text("#!/usr/bin/env python3\nimport os,sys,json\np=os.environ.get('TRUEOS_PROFILE_RUSTC_LOG')\n"
                       "if p:\n fd=os.open(p,os.O_WRONLY|os.O_CREAT|os.O_APPEND,0o644)\n"
                       " os.write(fd,(json.dumps(sys.argv[1:])+'\\n').encode());os.close(fd)\n"
                       "os.execv(sys.argv[1],sys.argv[1:])\n")
    wrapper.chmod(0o755)
    rows = []; lz4 = Lz4()
    for profile, level in [("speed", "3"), ("size", "s")]:
        folder = output / profile; folder.mkdir(exist_ok=True)
        env = os.environ.copy()
        for key in list(env):
            if key.startswith("CARGO_PROFILE_") or key in ["CARGO_ENCODED_RUSTFLAGS", "RUSTFLAGS", "CARGO_INCREMENTAL"]:
                del env[key]
        for key, value in {**SETTINGS, "OPT_LEVEL": level}.items():
            env[f"CARGO_PROFILE_RELEASE_{key}"] = value
        env.update(TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH="1",
                   TRUEOS_BLUEPRINT_CARGO_CACHE_DIR=str(folder / "cache"),
                   CARGO_ENCODED_RUSTFLAGS="-C\x1fsplit-debuginfo=packed",
                   RUSTC_WRAPPER=str(wrapper), CARGO_BUILD_JOBS="8")
        for app in apps:
            row = {"app": app, "profile": profile, "opt_level": level}
            log = folder / f"{app}.log"
            env["TRUEOS_PROFILE_RUSTC_LOG"] = str(folder / f"{app}.rustc.jsonl")
            print(f"Building {app}: opt-level={level}", flush=True)
            start = time.monotonic()
            with log.open("w") as stream:
                result = subprocess.run([str(binary), app, "--release"], cwd=repo, env=env,
                                        stdout=stream, stderr=subprocess.STDOUT)
            row["build_seconds"] = time.monotonic() - start
            if result.returncode:
                row["error"] = f"build exited {result.returncode}; see {log}"
            else:
                artifact = folder / f"{app}.bp"
                shutil.copy2(repo / "dist" / artifact.name, artifact)
                try:
                    row["compiler_settings"] = audit_compiler(
                        Path(env["TRUEOS_PROFILE_RUSTC_LOG"]), app, level)
                    row.update(measure(artifact, lz4))
                    print(f"  ELF={row['raw_elf_bytes']} LZ4={row['lz4_payload_bytes']} verified", flush=True)
                except Exception as error:
                    row["error"] = str(error)
            rows.append(row); report(output, apps, rows)
    # Measure after the compiler has stopped so build contention does not
    # distort the profile comparison.
    cpu = pin_measurement_cpu()
    print(f"Final timing pass pinned to CPU {cpu}", flush=True)
    for row in rows:
        if "error" not in row:
            row["measurement_cpu"] = cpu
            row.update(measure(output / row["profile"] / f"{row['app']}.bp", lz4))
    report(output, apps, rows)
    print(f"Report: {output / 'report.md'}", flush=True)


if __name__ == "__main__":
    main()
