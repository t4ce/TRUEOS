#!/usr/bin/env python3
"""Exercise complete native Mio sources against an instrumented kernel boundary.

The 31 behavioral cases run in a host process with native kernel ABI stubs;
they do not send packets through TRUEOS. Two separate real-loopback cases use
Mio's std-compat backend. A real core+alloc, no-Unix target also links a -r probe
through the native SDK allocator/panic/entry path. No builtin set, AppDB, boot
publication, rig process, or physical reset is changed.
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
FIXTURES = Path(__file__).with_name("native_mio")
TOOLCHAIN = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
MIO = ROOT / "vendor/mio-1.2.0"
SDK = ROOT.parent / "TRUEOS-Buildins/vendor/trueos"
TARGET = ROOT.parent / "TRUEOS-Buildins/tools/trueos-object-native.json"


def info(path):
    data = path.read_bytes()
    return {"path": str(path), "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}


def environment(target, native_host=False):
    env = dict(os.environ)
    for key in list(env):
        if key.startswith(("CARGO_ENCODED_", "CARGO_TARGET_", "CARGO_PROFILE_", "CARGO_CFG_", "CARGO_FEATURE_")) or key in {
            "RUSTFLAGS", "RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_TARGET",
            "CARGO_BUILD_RUSTFLAGS", "CARGO_MAKEFLAGS", "MAKEFLAGS", "OUT_DIR", "CARGO_MANIFEST_DIR", "CARGO_MANIFEST_PATH",
        }:
            env.pop(key, None)
    flags = ['--check-cfg=cfg(target_os,values("trueos","zkvm"))']
    if native_host:
        # Test only: native source/dependencies with the prebuilt host std test
        # harness. Actual no-Unix/no_std compilation is checked independently.
        flags[:0] = ["--cfg", 'target_os="trueos"', "-Aexplicit_builtin_cfgs_in_flags"]
    env["CARGO_TARGET_DIR"] = str(target)
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
    env["RUSTC_BOOTSTRAP_SYNTHETIC_TARGET"] = "1"
    return env


def manifest(directory, text):
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "Cargo.toml").write_text(text)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, help="Preserve logs, source hashes and receipt in this directory")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="trueos-native-mio-") as directory:
        work = Path(directory)
        output = args.output.resolve() if args.output else work / "evidence"
        output.mkdir(parents=True, exist_ok=True)
        results = []
        cargo = ["rustup", "run", TOOLCHAIN, "cargo"]

        def run(label, command, env):
            log = output / (label + ".log")
            with log.open("w") as stream:
                result = subprocess.run(command, cwd=work, env=env, stdout=stream,
                                        stderr=subprocess.STDOUT, timeout=180)
            results.append({"label": label, "command": command, "exit": result.returncode, "log": str(log)})
            if result.returncode:
                raise RuntimeError(f"{label} failed\n{log.read_text()[-10000:]}")

        io = json.dumps(str(ROOT / "crates/trueos-io"))
        # Cargo's host target does not select native target dependencies. This
        # manifest supplies exactly those dependencies; [lib] points directly to
        # every real production Mio source, with no source replacements.
        native = work / "host-native"
        manifest(native / "mio", f'''[package]\nname="mio"\nversion="1.2.0"\nedition="2024"\n[lib]\npath={json.dumps(str(MIO / 'src/lib.rs'))}\n[features]\ndefault=[]\nstd-compat=["dep:libc"]\nnative-net=[]\nlog=["dep:log"]\nnet=[]\nos-poll=[]\nos-ext=["os-poll"]\n[dependencies]\ncore3={{version="0.1.2",default-features=false,features=["alloc"]}}\ntrueos-io={{path={io}}}\nspin="0.10"\nlibc={{version="0.2.183",optional=true}}\nlog={{version="0.4.8",optional=true}}\n[lints.rust]\nunexpected_cfgs={{level="warn",check-cfg=['cfg(target_os,values("trueos","zkvm"))','cfg(mio_unsupported_force_poll_poll)','cfg(mio_unsupported_force_waker_pipe)']}}\n''')
        manifest(native, f'''[package]\nname="native-mio-behavior"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\nmio={{path="mio",default-features=false,features=["net","os-poll"]}}\ntrueos-io={{path={io}}}\n[[test]]\nname="native_tcp"\npath={json.dumps(str(FIXTURES / 'native_tcp.rs'))}\n''')
        run("native-behavior", cargo + ["test", "--offline", "--target", "x86_64-unknown-linux-gnu",
            "--manifest-path", str(native / "Cargo.toml"), "--test", "native_tcp", "--", "--test-threads=1"],
            environment(work / "host-target", native_host=True))

        compat = work / "host-compat"
        manifest(compat, f'''[package]\nname="native-mio-host-compat"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\nmio={{path={json.dumps(str(MIO))},default-features=false,features=["std-compat","net","os-poll"]}}\n[[test]]\nname="host_tcp"\npath={json.dumps(str(FIXTURES / 'host_tcp.rs'))}\n''')
        run("host-compat", cargo + ["test", "--offline", "--target", "x86_64-unknown-linux-gnu",
            "--manifest-path", str(compat / "Cargo.toml"), "--test", "host_tcp", "--", "--test-threads=1"],
            environment(work / "compat-target"))

        probe = work / "native-probe"
        manifest(probe, f'''[package]\nname="mio-native-compile-probe"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[lib]\npath={json.dumps(str(FIXTURES / 'native_probe.rs'))}\n[[bin]]\nname="mio-native-linked-probe"\npath={json.dumps(str(FIXTURES / 'native_entry.rs'))}\n[dependencies]\nmio={{path={json.dumps(str(MIO))},default-features=false,features=["os-poll","net"]}}\ntrueos-io={{path={io}}}\ntrueos={{path={json.dumps(str(SDK))},default-features=false,features=["default-global-allocator","default-panic-handler"]}}\n[profile.dev]\npanic="abort"\n''')
        target_dir = work / "native-target"
        run("native-object", cargo + ["build", "--offline", "--manifest-path", str(probe / "Cargo.toml"),
            "--bin", "mio-native-linked-probe", "--target", str(TARGET),
            "-Zbuild-std=core,compiler_builtins,alloc", "-Zbuild-std-features=compiler-builtins-mem", "-Zjson-target-spec"],
            environment(target_dir))
        native_flags = ["--target", str(TARGET), "-Zbuild-std=core,compiler_builtins,alloc",
                        "-Zbuild-std-features=compiler-builtins-mem", "-Zjson-target-spec"]
        run("native-object-release", cargo + ["build", "--offline", "--release",
            "--manifest-path", str(probe / "Cargo.toml"), "--bin", "mio-native-linked-probe", *native_flags],
            environment(target_dir))
        # Feature-off and optimized native configurations must typecheck too;
        # dev-only association IDs previously hid a release dead_code failure.
        feature_probe = work / "feature-probe"
        manifest(feature_probe, f'''[package]\nname="native-mio-feature-probe"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[lib]\npath="lib.rs"\n[dependencies]\nmio={{path={json.dumps(str(MIO))},default-features=false}}\n''')
        (feature_probe / "lib.rs").write_text("#![no_std]\npub use mio::{Events, Poll, Token};\n")
        for index, features in enumerate(["", "mio/net", "mio/os-poll", "mio/net,mio/os-poll"]):
            command = cargo + ["check", "--offline", "--release", "--manifest-path",
                               str(feature_probe / "Cargo.toml"), *native_flags]
            if features:
                command += ["--features", features]
            run(f"native-release-features-{index}", command, environment(target_dir))
        module = target_dir / TARGET.stem / "release/mio-native-linked-probe.o"
        symbols = sorted({line.split()[-1] for line in subprocess.check_output(["nm", "-u", str(module)], text=True).splitlines() if line.strip()})
        cfg = subprocess.check_output(["rustup", "run", TOOLCHAIN, "rustc", "-Zunstable-options", "--print", "cfg", "--target", str(TARGET)], cwd=work, text=True).splitlines()
        assert "unix" not in cfg and 'target_env=""' in cfg and not any(x.startswith("target_family=") for x in cfg)
        assert not list((target_dir / TARGET.stem / "debug/deps").glob("libstd-*.rlib"))
        # Current source contract inventory, independent of a prior kernel build
        # or published AppDB. Deployment binding is a separate integration gate.
        import audit_platform_contracts as contracts
        known = {}
        for source in (ROOT / "src/unix_compat.rs", ROOT / "src/hv/blueprint/blueprint.rs"):
            known.update(contracts.resolver_arms(source))
        cabi = set(re.findall(r"\bfn\s+(\w+)\(", (ROOT / "crates/trueos-v/src/bp_abi.rs").read_text()))
        unknown = sorted(set(symbols) - known.keys() - cabi)
        assert not unknown, unknown
        native_count = int(re.search(r"test result: ok\. (\d+) passed", (output / "native-behavior.log").read_text())[1])
        compat_count = int(re.search(r"test result: ok\. (\d+) passed", (output / "host-compat.log").read_text())[1])
        assert native_count == 31 and compat_count == 2
        receipt = {"scope": __doc__, "toolchain": TOOLCHAIN, "checks": results,
                   "native_tests": native_count, "host_loopback_tests": compat_count,
                   "fixtures": [info(p) for p in sorted(FIXTURES.glob("*.rs"))],
                   "target": info(TARGET), "target_cfg": cfg, "object": info(module),
                   "undefined": symbols, "unknown_source_contracts": unknown,
                   "limits": ["Native behavioral kernel ABI is stubbed; physical runtime and performance need integration verification.",
                              "Native object lives in a temporary directory; its hash/import receipt is retained.",
                              "Source contract membership is not proof that a deployed kernel contains the current resolver."]}
        (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
        print(f"Native Mio: {native_count} boundary tests, {compat_count} host loopback tests; native dev+release objects and 4 release feature combinations; {len(symbols)} release imports, 0 unknown source contracts.")
        if args.output:
            print(output / "receipt.json")


if __name__ == "__main__":
    main()
