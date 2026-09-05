#!/usr/bin/env python3
"""Installer regression fixtures; never writes the installed Rust toolchain."""
from pathlib import Path
import tempfile
import unittest
from apply_trueos_rust_std_thread_backend import install, rust_root, UNIX_SELECTOR, TRUEOS_SELECTOR


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


if __name__ == "__main__":
    unittest.main()
