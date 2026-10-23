#!/usr/bin/env python3
"""Check the font's boot lifetime and archive wrapper without booting hardware."""

import importlib.util
import io
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location("microfont_pack", ROOT / "tools/microfont-pack/pack.py")
PACK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACK)

HARNESS = r'''
extern crate alloc;
extern crate self as microfont;
extern crate self as spin;

use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
pub struct Once<T>(std::sync::OnceLock<T>);
impl<T> Once<T> {
    pub const fn new() -> Self { Self(std::sync::OnceLock::new()) }
    pub fn call_once(&self, initialize: impl FnOnce() -> T) -> &T {
        self.0.get_or_init(initialize)
    }
}
#[macro_export] macro_rules! log_info { ($($args:tt)*) => {}; }

pub const EMBEDDED_ARCHIVE: &[u8] = b"archive fixture";
pub const UNCOMPRESSED_BUNDLE_BYTES: usize = 4;
static MODE: AtomicU8 = AtomicU8::new(0);
static DECODES: AtomicUsize = AtomicUsize::new(0);
static INSTALLS: AtomicUsize = AtomicUsize::new(0);
static INSTALLED: std::sync::OnceLock<&'static [u8]> = std::sync::OnceLock::new();
pub fn install_bundle(bytes: &'static [u8]) -> Result<(), &'static str> {
    INSTALLS.fetch_add(1, Ordering::SeqCst);
    assert_eq!(bytes, &[1, 2, 3, 4]);
    if MODE.load(Ordering::SeqCst) == 3 { return Err("install rejected"); }
    INSTALLED.set(bytes).unwrap();
    Ok(())
}
mod r {
    pub mod tar {
        pub fn decode_named_single_lz4(input: &[u8], limit: usize, name: &str) -> Result<Vec<u8>, ()> {
            use super::super::*;
            assert_eq!(input, EMBEDDED_ARCHIVE);
            assert_eq!(limit, UNCOMPRESSED_BUNDLE_BYTES);
            assert_eq!(name, "microfont.bin");
            DECODES.fetch_add(1, Ordering::SeqCst);
            match MODE.load(Ordering::SeqCst) {
                1 => Err(()),
                2 => Ok(vec![1, 2, 3]),
                _ => Ok(vec![1, 2, 3, 4]),
            }
        }
    }
    #[path = MODULE_PATH] pub mod microfont;
}
fn main() {
    let mode: u8 = std::env::args().nth(1).unwrap().parse().unwrap();
    MODE.store(mode, Ordering::SeqCst);
    let first = r::microfont::init();
    let second = r::microfont::init();
    assert_eq!(first, second);
    if cfg!(target_os = "trueos") {
        assert_eq!(DECODES.load(Ordering::SeqCst), 1);
        assert_eq!(INSTALLS.load(Ordering::SeqCst), usize::from(mode == 0 || mode == 3));
        assert_eq!(first.is_ok(), mode == 0);
        if mode == 0 {
            assert_eq!(*INSTALLED.get().unwrap(), &[1, 2, 3, 4]);
        }
    } else {
        assert!(first.is_ok());
        assert_eq!(DECODES.load(Ordering::SeqCst), 0);
        assert_eq!(INSTALLS.load(Ordering::SeqCst), 0);
    }
}
'''


class MicrofontBoot(unittest.TestCase):
    def test_heap_precedes_font_and_font_precedes_apps_display_and_smp(self):
        boot = (ROOT / "src/main.rs").read_text()
        heap = boot.index("phys::try_install_heap_arena_candidates(allocators::install_heap_arena)")
        font = boot.index('r::microfont::init().expect("initialize embedded MicroFont bundle")')
        self.assertLess(heap, font)
        for next_stage in ("app_db::init_bsp()", "percpu::init_bsp()", "intel::init_once()"):
            self.assertLess(font, boot.index(next_stage))

    def test_trueos_installs_once_and_rejects_decode_size_and_install_failures(self):
        self.run_harness(True, range(4))

    def test_portable_target_does_not_decode_or_reinstall(self):
        self.run_harness(False, [0])

    def run_harness(self, trueos, modes):
        with tempfile.TemporaryDirectory(prefix="microfont-boot-test-") as tmp:
            folder = Path(tmp)
            source = folder / "main.rs"
            source.write_text(HARNESS.replace("MODULE_PATH", '"' + str(ROOT / "src/r/microfont.rs") + '"'))
            command = ["rustc", "--edition=2024", "-Adead_code", str(source), "-o", str(folder / "check")]
            if trueos:
                command.extend(["-Aexplicit_builtin_cfgs_in_flags", "--cfg", 'target_os="trueos"'])
            subprocess.run(command, check=True)
            for mode in modes:
                subprocess.run([str(folder / "check"), str(mode)], check=True)

    def test_tar_lz4_preserves_one_portable_file_and_is_reproducible(self):
        self.assertLessEqual(len(PACK.tar_payload(b"a")), 1 + 8192)
        with tempfile.TemporaryDirectory(prefix="microfont-wrapper-test-") as tmp:
            folder = Path(tmp)
            source = folder / "portable.bin"
            payload = (b"MicroFont fixture\0" + bytes(range(256))) * 17
            source.write_bytes(payload)
            output = folder / "microfont.tar.lz4"
            PACK.pack(source, output)
            first = output.read_bytes()
            modified = output.stat().st_mtime_ns
            PACK.pack(source, output)
            self.assertEqual(output.read_bytes(), first)
            self.assertEqual(output.stat().st_mtime_ns, modified)
            decoded = subprocess.run(["lz4", "-q", "-d", "-c", str(output)],
                                     check=True, capture_output=True).stdout
            with tarfile.open(fileobj=io.BytesIO(decoded), mode="r:") as archive:
                members = archive.getmembers()
                self.assertEqual(len(members), 1)
                member = members[0]
                self.assertEqual(member.name, "microfont.bin")
                self.assertEqual((member.uid, member.gid, member.mtime), (0, 0, 0))
                self.assertEqual(archive.extractfile(member).read(), payload)
        with self.assertRaises(ValueError):
            PACK.tar_payload(b"")


if __name__ == "__main__":
    unittest.main()
