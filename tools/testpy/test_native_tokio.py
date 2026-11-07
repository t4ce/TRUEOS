#!/usr/bin/env python3
"""Validate the real native Tokio core, plus its existing std compatibility mode.

Eighteen native behavior cases run in a host process with instrumented kernel
ABI stubs, not on TRUEOS carriers. Ten separate full-feature std cases exercise
real host threads, TCP, filesystem and subprocesses, both with std-compat and
legacy default-features=false. Actual no-Unix/core+alloc dev and release objects
exercise current/multithread schedulers, channels, timers, JoinSet and duplex IO
through the SDK allocator/panic/entry contract. Native network/fs/process/signal
features remain outside this port. No builtin set or boot publication is changed.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = Path(__file__).with_name("native_tokio")
BUILDINS = ROOT.parent / "TRUEOS-Buildins"
TOKIO = BUILDINS / "vendor/tokio-1.54.1"
SDK = BUILDINS / "vendor/trueos"
TARGET = BUILDINS / "tools/trueos-object-native.json"
TOOLCHAIN = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
NATIVE_FEATURES = ["native", "rt", "rt-multi-thread", "sync", "time", "io-util", "macros"]


def info(path):
    data = path.read_bytes()
    return {"path": str(path), "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def environment(target):
    env = dict(os.environ)
    for key in list(env):
        if key.startswith(("CARGO_ENCODED_", "CARGO_TARGET_", "CARGO_PROFILE_", "CARGO_CFG_", "CARGO_FEATURE_")) or key in {
            "RUSTFLAGS", "RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_TARGET",
            "CARGO_BUILD_RUSTFLAGS", "CARGO_MAKEFLAGS", "MAKEFLAGS", "OUT_DIR", "CARGO_MANIFEST_DIR", "CARGO_MANIFEST_PATH",
        }:
            env.pop(key, None)
    env["CARGO_TARGET_DIR"] = str(target)
    env["CARGO_ENCODED_RUSTFLAGS"] = '--check-cfg=cfg(target_os,values("trueos","zkvm"))'
    env["RUSTC_BOOTSTRAP_SYNTHETIC_TARGET"] = "1"
    return env


def manifest(directory, body):
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "Cargo.toml").write_text(body)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, help="Preserve logs, source hashes and receipt here")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="trueos-native-tokio-") as directory:
        work = Path(directory)
        output = args.output.resolve() if args.output else work / "evidence"
        output.mkdir(parents=True, exist_ok=True)
        cargo = ["rustup", "run", TOOLCHAIN, "cargo"]
        checks = []

        def run(label, command, target):
            log = output / (label + ".log")
            with log.open("w") as stream:
                result = subprocess.run(command, cwd=work, env=environment(target), stdout=stream,
                                        stderr=subprocess.STDOUT, timeout=240)
            checks.append({"label": label, "command": command, "exit": result.returncode, "log": str(log)})
            if result.returncode:
                raise RuntimeError(f"{label} failed\n{log.read_text()[-16000:]}")

        tokio = json.dumps(str(TOKIO))
        runtime = json.dumps(str(ROOT / "crates/trueos-runtime"))
        common = '[package]\nname="tokio-native-behavior"\nversion="0.0.0"\nedition="2024"\n[workspace]\n'
        native = work / "behavior"
        manifest(native, common + f'''[[bin]]
name="tokio-native-behavior"
path={json.dumps(str(FIXTURES / 'native_behavior.rs'))}
[dependencies]
tokio={{path={tokio},default-features=false,features={json.dumps(NATIVE_FEATURES)}}}
trueos-runtime={{path={runtime}}}
[profile.dev]
panic="abort"
''')
        run("native-behavior", cargo + ["run", "--offline", "--manifest-path", str(native / "Cargo.toml"),
            "--target", "x86_64-unknown-linux-gnu"], work / "host-native-target")
        assert "18 native-runtime behavior cases passed" in (output / "native-behavior.log").read_text()

        compat = work / "compat"
        for name, features in [("std-compat", ["std-compat", "full", "test-util"]),
                               ("legacy-std", ["full", "test-util"])]:
            manifest(compat, f'''[package]
name="tokio-compat-behavior"
version="0.0.0"
edition="2024"
[workspace]
[[bin]]
name="tokio-compat-behavior"
path={json.dumps(str(FIXTURES / 'std_behavior.rs'))}
[dependencies]
tokio={{path={tokio},default-features=false,features={json.dumps(features)}}}
[patch.crates-io]
mio={{path={json.dumps(str(ROOT / 'vendor/mio-1.2.0'))}}}
''')
            run(name, cargo + ["run", "--offline", "--manifest-path", str(compat / "Cargo.toml"),
                "--target", "x86_64-unknown-linux-gnu"], work / "host-compat-target")
            assert "10 std-compat behavior cases passed" in (output / (name + ".log")).read_text()

        probe = work / "probe"
        manifest(probe, f'''[package]
name="tokio-native-probe"
version="0.0.0"
edition="2024"
[workspace]
[lib]
path={json.dumps(str(FIXTURES / 'native_probe.rs'))}
[[bin]]
name="tokio-native-linked"
path={json.dumps(str(FIXTURES / 'native_entry.rs'))}
[dependencies]
tokio={{path={tokio},default-features=false,features={json.dumps(NATIVE_FEATURES)}}}
trueos={{path={json.dumps(str(SDK))},default-features=false,features=["default-global-allocator","default-panic-handler"]}}
[profile.dev]
panic="abort"
''')
        native_target = work / "native-target"
        target_flags = ["--target", str(TARGET), "-Zbuild-std=core,compiler_builtins,alloc",
                        "-Zbuild-std-features=compiler-builtins-mem", "-Zjson-target-spec"]
        for label, flags in [("native-object", []), ("native-object-release", ["--release"])]:
            run(label, cargo + ["build", "--offline", "--manifest-path", str(probe / "Cargo.toml"),
                "--bin", "tokio-native-linked", *flags, *target_flags], native_target)
        features_probe = work / "features"
        manifest(features_probe, f'''[package]
name="native-tokio-feature-check"
version="0.0.0"
edition="2024"
[workspace]
[lib]
path="lib.rs"
[dependencies]
tokio={{path={tokio},default-features=false}}
''')
        (features_probe / "lib.rs").write_text("#![no_std]\npub use tokio::io::{AsyncRead,AsyncWrite};\n")
        for index, features in enumerate([["native"], ["native", "sync"], ["native", "rt", "time"], NATIVE_FEATURES]):
            run(f"release-features-{index}", cargo + ["check", "--offline", "--release", "--manifest-path",
                str(features_probe / "Cargo.toml"), "--features", ",".join("tokio/" + f for f in features),
                *target_flags], native_target)

        module = native_target / TARGET.stem / "release/tokio-native-linked.o"
        symbols = sorted({line.split()[-1] for line in subprocess.check_output(["nm", "-u", str(module)], text=True).splitlines() if line.strip()})
        cfg = subprocess.check_output(["rustup", "run", TOOLCHAIN, "rustc", "-Zunstable-options", "--print", "cfg", "--target", str(TARGET)], cwd=work, text=True).splitlines()
        assert "unix" not in cfg and 'target_env=""' in cfg and not any(x.startswith("target_family=") for x in cfg)
        forbidden_libs = []
        for pattern in ["libstd-*.rlib", "liblibc-*.rlib", "libsocket2-*.rlib"]:
            forbidden_libs += list((native_target / TARGET.stem).rglob(pattern))
        assert not forbidden_libs, forbidden_libs
        import audit_platform_contracts as contracts
        direct = {}
        for source in [ROOT / "src/unix_compat.rs", ROOT / "src/hv/blueprint/blueprint.rs"]:
            direct.update(contracts.resolver_arms(source))
        abi = ROOT / "crates/trueos-v/src/bp_abi.rs"
        declared = set(re.findall(r"\bfn\s+(\w+)\(", abi.read_text()))
        # build.rs generates each declared-and-defined CABI resolver arm. A
        # prototype alone cannot establish a live import binding.
        definitions = {}
        needed = set(symbols) - direct.keys()
        for path in (ROOT / "src").rglob("*.rs"):
            source = path.read_text()
            for name in needed:
                found = re.search(r'pub(?:\([^)]*\))?\s+(?:unsafe\s+)?extern\s+"C"\s+fn\s+' + re.escape(name) + r'\s*\(', source)
                if found:
                    definitions[name] = {"source": str(path), "line": source.count("\n", 0, found.start()) + 1,
                                         "generator": str(ROOT / "build.rs"), "declaration": str(abi)}
        unknown = sorted(name for name in symbols if name not in direct and not (name in declared and name in definitions))
        assert not unknown, unknown
        legacy = set().union(*(values for family, values in contracts.FAMILIES.items() if family not in {
            "rust_system_layout_allocation", "native_execution_tls", "native_generation_wait", "native_named_thread_join",
        }))
        assert not (set(symbols) & legacy), sorted(set(symbols) & legacy)
        assert {"sys_alloc_aligned", "sys_dealloc"} <= set(symbols)
        receipt = {"scope": __doc__, "toolchain": TOOLCHAIN, "checks": checks,
                   "native_behavior_cases": 18, "std_compat_behavior_cases": 10, "legacy_std_behavior_cases": 10,
                   "target": info(TARGET), "target_cfg": cfg, "object": info(module),
                   "native_features": NATIVE_FEATURES, "build_std": ["core", "compiler_builtins", "alloc"],
                   "imports": symbols, "unknown_source_contracts": unknown,
                   "import_contracts": {name: direct.get(name, definitions.get(name)) for name in symbols},
                   "fixtures": [info(path) for path in sorted(FIXTURES.glob("*.rs"))]}
        (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
        print(f"PASS: 18 native ABI cases; 10 std + 10 legacy std cases; 2 no-Unix linked objects; 4 release feature closures; {len(symbols)} native imports resolved")
        if args.output:
            print(output / "receipt.json")


if __name__ == "__main__":
    main()
