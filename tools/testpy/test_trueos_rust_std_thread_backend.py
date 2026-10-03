#!/usr/bin/env python3
"""Installer regression fixtures; never writes the installed Rust toolchain."""
from pathlib import Path
import hashlib
import os
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from apply_trueos_rust_std_thread_backend import (
    install, rust_root, UNIX_SELECTOR, TRUEOS_SELECTOR, TRUEOS_TLS_SELECTOR,
)

TLS_FIXTURE = '''cfg_select! {
    any(
        target_os = "uefi",
        target_os = "trueos",
        target_os = "zkvm",
    ) => {
        mod no_threads;
        pub use no_threads::{EagerStorage, LazyStorage, thread_local_inner};
        pub(crate) use no_threads::{LocalPointer, local_pointer};
    }
    target_thread_local => {
        mod native;
    }
    _ => {
        mod os;
    }
}
'''
CURRENT_FIXTURE = '''pub(super) fn set_current(thread: Thread) -> Result<(), Thread> {
    #[cfg(any(target_os = "trueos", target_os = "zkvm"))]
    {
        // TRUEOS carrier lanes may host multiple logical std threads over time.
        rebind(thread);
        return Ok(());
    }
    #[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
    {
        initialize(thread)
    }
}
'''


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.thread = self.root / "library/std/src/sys/thread"
        self.thread.mkdir(parents=True)
        self.selector = self.thread / "mod.rs"
        self.selector.write_text("cfg_select! {\n" + UNIX_SELECTOR + "        mod unix;\n    }\n}\n")
        self.unix = self.root / "library/std/src/os/unix/mod.rs"
        self.unix.parent.mkdir(parents=True)
        self.unix.write_text("pub mod thread;\npub mod prelude {\n    pub use super::thread::JoinHandleExt;\n}\n")
        self.tls = self.root / "library/std/src/sys/thread_local/mod.rs"
        self.tls.parent.mkdir(parents=True)
        self.tls.write_text(TLS_FIXTURE)
        self.current = self.root / "library/std/src/thread/current.rs"
        self.current.parent.mkdir(parents=True)
        self.current.write_text(CURRENT_FIXTURE)

    def snapshot(self):
        return {path.relative_to(self.root): path.read_bytes() for path in self.root.rglob("*") if path.is_file()}

    def test_clean_repeated_and_check_only(self):
        self.assertEqual(rust_root(self.root / "library"), self.root)
        install(self.root)
        original = self.snapshot()
        install(self.root)
        install(self.root, check=True)
        self.assertEqual(original, self.snapshot())
        source = self.selector.read_text()
        self.assertLess(source.index(TRUEOS_SELECTOR), source.index(UNIX_SELECTOR))
        self.assertEqual(source.count(TRUEOS_SELECTOR), 1)
        self.assertEqual(self.unix.read_text().count('#[cfg(not(target_os = "trueos"))]'), 2)
        self.assertTrue(self.tls.read_text().startswith("cfg_select! {\n" + TRUEOS_TLS_SELECTOR))
        self.assertNotIn('        target_os = "trueos",', self.tls.read_text())
        self.assertIn('#[cfg(target_os = "zkvm")]', self.current.read_text())
        self.assertIn('#[cfg(not(target_os = "zkvm"))]', self.current.read_text())
        self.assertNotIn('target_os = "trueos"', self.current.read_text())

    def test_check_only_does_not_install(self):
        original = self.snapshot()
        with self.assertRaises(SystemExit): install(self.root, check=True)
        self.assertEqual(original, self.snapshot())

    def test_missing_anchor_does_not_partially_install(self):
        self.unix.write_text("pub mod thread;\n")
        original = self.snapshot()
        with self.assertRaises(SystemExit): install(self.root)
        self.assertEqual(original, self.snapshot())

    def test_conflicting_backend_does_not_overwrite(self):
        (self.thread / "trueos.rs").write_text("// local backend\n")
        original = self.snapshot()
        with self.assertRaises(SystemExit): install(self.root)
        self.assertEqual(original, self.snapshot())

    def test_recognized_backend_upgrade_and_check_only(self):
        previous = b"// previously reviewed canonical backend\n"
        backend = self.thread / "trueos.rs"
        backend.write_bytes(previous)
        original = self.snapshot()
        with patch("apply_trueos_rust_std_thread_backend.KNOWN_BACKEND_SHA256",
                   {hashlib.sha256(previous).hexdigest()}):
            with self.assertRaises(SystemExit): install(self.root, check=True)
            self.assertEqual(original, self.snapshot())
            install(self.root)
        self.assertNotEqual(backend.read_bytes(), previous)
        install(self.root, check=True)

    def test_upgrade_still_preflights_other_files(self):
        previous = b"// previously reviewed canonical backend\n"
        (self.thread / "trueos.rs").write_bytes(previous)
        self.unix.write_text("pub mod thread;\n")
        original = self.snapshot()
        with patch("apply_trueos_rust_std_thread_backend.KNOWN_BACKEND_SHA256",
                   {hashlib.sha256(previous).hexdigest()}):
            with self.assertRaises(SystemExit): install(self.root)
        self.assertEqual(original, self.snapshot())

    def test_locally_edited_recognized_backend_is_preserved(self):
        previous = b"// previously reviewed canonical backend\n"
        (self.thread / "trueos.rs").write_bytes(previous + b"// local edit\n")
        original = self.snapshot()
        with patch("apply_trueos_rust_std_thread_backend.KNOWN_BACKEND_SHA256",
                   {hashlib.sha256(previous).hexdigest()}):
            with self.assertRaises(SystemExit): install(self.root)
        self.assertEqual(original, self.snapshot())

    def test_conflicting_selector_does_not_install(self):
        self.selector.write_text('cfg_select! {\n    target_os = "trueos" => { mod another; }\n' + UNIX_SELECTOR + '}\n}\n')
        original = self.snapshot()
        with self.assertRaises(SystemExit): install(self.root)
        self.assertEqual(original, self.snapshot())

    def test_repeated_unix_anchor_rejected(self):
        self.selector.write_text(UNIX_SELECTOR * 2)
        original = self.snapshot()
        with self.assertRaises(SystemExit): install(self.root)
        self.assertEqual(original, self.snapshot())

    def test_recover_after_backend_only_was_installed(self):
        install(self.root)
        self.selector.write_text("cfg_select! {\n" + UNIX_SELECTOR + "        mod unix;\n    }\n}\n")
        install(self.root)
        install(self.root, check=True)

    def test_upstream_tls_without_legacy_entry(self):
        self.tls.write_text(TLS_FIXTURE.replace('        target_os = "trueos",\n', ""))
        self.current.write_text("pub(super) fn set_current() { initialize(); }\n")
        install(self.root)
        install(self.root, check=True)

    def test_later_tls_selectors_stay_unchanged(self):
        guard = '\nmod guard { cfg_select! { target_os = "trueos", => {} } }\n'
        self.tls.write_text(TLS_FIXTURE + guard)
        install(self.root)
        self.assertTrue(self.tls.read_text().endswith(guard))

    def test_conflicting_tls_backend_does_not_partially_install(self):
        self.tls.write_text('cfg_select! {\n    target_os = "trueos" => { mod native; }\n}\n')
        original = self.snapshot()
        with self.assertRaises(SystemExit): install(self.root)
        self.assertEqual(original, self.snapshot())

    def test_invalid_tls_anchor_does_not_partially_install(self):
        self.tls.write_text(TLS_FIXTURE.replace("    target_thread_local => {", "    another_selector => {"))
        original = self.snapshot()
        with self.assertRaises(SystemExit): install(self.root)
        self.assertEqual(original, self.snapshot())

    def test_invalid_current_thread_hook_does_not_partially_install(self):
        self.current.write_text(CURRENT_FIXTURE.replace('#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]', ""))
        original = self.snapshot()
        with self.assertRaises(SystemExit): install(self.root)
        self.assertEqual(original, self.snapshot())


class RuntimeBackendTests(unittest.TestCase):
    def test_lifecycle_against_native_cabi_fixture(self):
        rust_std = Path(__file__).resolve().parents[1] / "rust-std"
        environment = dict(os.environ, TRUEOS_STD_THREAD_BACKEND=str(rust_std / "trueos_thread.rs"))
        with tempfile.TemporaryDirectory(prefix="trueos-std-thread-") as directory:
            binary = Path(directory) / "tests"
            subprocess.run(["rustc", "--edition=2024", "--test",
                            str(rust_std / "tests/thread_backend.rs"), "-o", str(binary)],
                           env=environment, check=True)
            subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    unittest.main()
