#!/usr/bin/env python3
"""Exercise production native TLS with unchanged upstream std Storage."""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
TOOLCHAIN = "nightly-2026-07-10"


class NativeStdTls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not shutil.which("rustup"):
            raise unittest.SkipTest("pinned rust-src is needed")
        cls.installed = Path(subprocess.check_output(
            ["rustup", "run", TOOLCHAIN, "rustc", "--print", "sysroot"], text=True).strip())
        cls.library = cls.installed / "lib/rustlib/src/rust/library"
        spec = importlib.util.spec_from_file_location("std_installer", ROOT / "tools/testpy/apply_trueos_rust_std_thread_backend.py")
        cls.installer = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.installer)

    def test_installer_keeps_std_storage_and_selects_static_keys_idempotently(self):
        with tempfile.TemporaryDirectory(prefix="trueos-std-tls-install-") as directory:
            rust_root = Path(directory)
            path = rust_root / "library/std/src/sys/thread_local/key/trueos.rs"
            path.parent.mkdir(parents=True)
            source = (self.library / "std/src/sys/thread_local/mod.rs").read_text()
            path, reference, result = self.installer.static_tls_keys(rust_root, source)
            path.write_text(reference)
            again = self.installer.static_tls_keys(rust_root, result)
            self.assertEqual((path, reference, result), again)
            self.assertIn('pub(super) use trueos::{Key, LazyKey, get, set};', result)

    def test_complete_shared_sysroot_install_is_idempotent_and_rejects_local_backend_edits(self):
        prepare = ROOT.parent / "TRUEOS-Buildins/tools/prepare_sysroot.py"
        if not prepare.is_file():
            self.skipTest("shared sysroot installer is unavailable")
        with tempfile.TemporaryDirectory(prefix="trueos-complete-std-install-") as directory:
            directory = Path(directory)
            result = directory / "selection.json"
            subprocess.run(["python3", prepare, "--output", directory, "--kernel", ROOT,
                            "--rustc", self.installed / "bin/rustc", "--result", result],
                           check=True, capture_output=True, text=True, timeout=60)
            sysroot = Path(json.loads(result.read_text())["sysroot"])
            rust_root = sysroot / "lib/rustlib/src/rust"
            planned = self.installer.installation(rust_root)
            before = {path: path.read_bytes() for path in planned}
            self.installer.install(rust_root, check=True)
            self.installer.install(rust_root)
            self.assertEqual(before, {path: path.read_bytes() for path in planned})
            guard = rust_root / "library/std/src/sys/thread_local/guard/trueos.rs"
            guard.write_text(guard.read_text() + "// private edit\n")
            with self.assertRaisesRegex(SystemExit, "refusing to overwrite local changes"):
                self.installer.install(rust_root, check=True)

    def test_production_state_key_adapter_storage_and_cleanup_guard(self):
        tls = self.library / "std/src/sys/thread_local"
        with tempfile.TemporaryDirectory(prefix="trueos-std-tls-tests-") as directory:
            directory = Path(directory)
            # Adapt import paths only: use the host allocator and host spin lock
            # binding around the complete production native State and APIs.
            native = (ROOT / "src/r/native_tls.rs").read_text()
            native = native.replace("use alloc::collections::BTreeMap;", "use std::collections::BTreeMap;")
            native = native.replace("use alloc::vec::Vec;", "use std::vec::Vec;")
            native = native.replace("use spin::Mutex;", "use crate::SpinMutex as Mutex;")
            native_path = directory / "native_tls.rs"
            native_path.write_text(native)
            # Stability annotations are std-only; retain the complete Storage
            # implementation and its macros, removing that annotation only.
            storage_path = directory / "storage.rs"
            storage_path.write_text((tls / "os.rs").read_text().replace(
                '#[unstable(feature = "thread_local_internals", issue = "none")]\n', ''))
            modules = "mod r {\n#[path = " + json.dumps(str(native_path)) + "] pub mod native_tls;\n"
            modules += '''mod threads {
                pub fn current_id() -> Option<usize> { Some(crate::EXECUTION.get()) }
                pub fn current_vm_id() -> Option<u8> { crate::REALM.get().checked_sub(1).map(|v| v as u8) }
            }}
            mod sys { pub mod thread_local {
                fn abort_on_dtor_unwind(f: impl FnOnce()) { f(); }
                pub mod key {
            '''
            modules += "#[path = " + json.dumps(str(ROOT / "tools/rust-std/trueos_tls_key.rs")) + "] mod native;\npub use native::{Key, LazyKey, get, set};\n}\n"
            modules += "#[path = " + json.dumps(str(storage_path)) + "] mod os;\npub use os::{Storage, value_align};\npub(crate) use os::LocalPointer;\n"
            modules += "#[path = " + json.dumps(str(ROOT / "tools/rust-std/trueos_tls_guard.rs")) + "] pub mod guard;\n}}\n"
            shim = (ROOT / "src/std_abi_shim.rs").read_text()
            lifecycle = shim.split("fn run_execution_tls_destructors(owner: usize) {", 1)[1].split("\n#[cfg(test)]", 1)[0]
            modules += "fn run_execution_tls_destructors(owner: usize) {" + lifecycle
            harness = directory / "tls_tests.rs"
            harness.write_text((ROOT / "tools/rust-std/trueos_tls_tests.rs").read_text()
                               .replace("// @PRODUCTION_MODULES@", modules))
            binary = directory / "tls_tests"
            env = dict(os.environ)
            env["LD_LIBRARY_PATH"] = str(self.installed / "lib") + ":" + env.get("LD_LIBRARY_PATH", "")
            for optimization in ("0", "3"):
                subprocess.run([self.installed / "bin/rustc", "--edition=2024", "--test", harness,
                                "-C", f"opt-level={optimization}", "-o", binary],
                               env=env, check=True, timeout=60)
                for _ in range(3):
                    subprocess.run([binary, "--test-threads=1"], env=env, check=True, timeout=60)


if __name__ == "__main__":
    unittest.main()
