#!/usr/bin/env python3
"""Exercise actual upstream std synchronization over the TRUEOS wait adapter."""
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


class NativeStdSync(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not shutil.which("rustup"):
            raise unittest.SkipTest("pinned rust-src is needed")
        cls.installed = Path(subprocess.check_output(
            ["rustup", "run", TOOLCHAIN, "rustc", "--print", "sysroot"], text=True).strip())
        cls.library = cls.installed / "lib/rustlib/src/rust/library"
        if not cls.library.is_dir():
            raise unittest.SkipTest("pinned rust-src is needed")
        spec = importlib.util.spec_from_file_location("std_installer", ROOT / "tools/testpy/apply_trueos_rust_std_thread_backend.py")
        cls.installer = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.installer)

    def test_installer_selects_atomic_algorithms_and_is_idempotent(self):
        with tempfile.TemporaryDirectory(prefix="trueos-std-sync-install-") as directory:
            rust_root = Path(directory)
            shutil.copytree(self.library / "std/src/sys", rust_root / "library/std/src/sys")
            first = self.installer.native_sync(rust_root)
            for path, source in first.items():
                path.write_text(source)
            self.assertEqual(first, self.installer.native_sync(rust_root))
            for name in ("mutex", "condvar", "thread_parking", "once", "rwlock"):
                source = first[rust_root / f"library/std/src/sys/sync/{name}/mod.rs"]
                self.assertLess(source.index('target_os = "trueos"'), source.index("mod futex;"))
            backend = rust_root / "library/std/src/sys/pal/unix/futex_trueos.rs"
            backend.write_text(backend.read_text() + "// local edit\n")
            with self.assertRaisesRegex(SystemExit, "refusing to overwrite"):
                self.installer.native_sync(rust_root)

    def test_production_algorithms_with_generation_platform(self):
        sys = self.library / "std/src/sys"
        adapter = ROOT / "tools/rust-std/trueos_futex.rs"
        # Insert an accessor into the production adapter in the temporary test
        # copy, leaving its algorithm byte-for-byte unchanged.
        with tempfile.TemporaryDirectory(prefix="trueos-std-sync-tests-") as directory:
            directory = Path(directory)
            adapter_copy = directory / "futex.rs"
            adapter_copy.write_text(adapter.read_text() + "\n"
                "pub fn timeout_for_test(timeout: Option<Duration>) -> u64 { timeout_millis(timeout) }\n")
            modules = "mod sys {\n#[path = " + json.dumps(str(adapter_copy)) + "] pub mod futex;\npub mod sync {\n"
            for name in ("mutex", "condvar", "thread_parking", "once", "rwlock"):
                path = sys / "sync" / name / "futex.rs"
                modules += "#[path = " + json.dumps(str(path)) + "] pub mod " + name + ";\n"
            modules += "pub use {mutex::Mutex, condvar::Condvar, thread_parking::Parker, rwlock::RwLock};\n}\n}\n"
            harness = directory / "sync_tests.rs"
            harness.write_text((ROOT / "tools/rust-std/trueos_sync_tests.rs").read_text()
                               .replace("// @PRODUCTION_MODULES@", modules))
            binary = directory / "sync_tests"
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
