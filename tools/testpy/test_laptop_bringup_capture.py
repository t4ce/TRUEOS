#!/usr/bin/env python3
"""Exercise production log routing and boot-prefix retention without hardware."""
import os
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

TESTS = r'''
#[cfg(test)] mod tests {
    use super::*;
    use log_os_core::{GlobalLogDispatch, LogArea as A, LogLevel as L};
    #[test] fn all_bringup_trace_survives_competing_profiles_and_both_sinks() {
        assert!(flags::LAPTOP_BRINGUP_DIAG_PROFILE_ENABLED);
        policy::apply_profile(0);
        for target in ["crab_usb::backend::kmod", "TRUEOS::usb3::heal_protocol",
                       "TRUEOS::intel::display", "render", "intel/gpgpu", "ui4"] {
            let area = log_os_core::target_log_area(target);
            assert!(TRUEOS_LOG_ROUTER.enabled(area, L::Trace), "{target}");
            log_os_core::log_with_area_level(&TRUEOS_LOG_ROUTER, area, L::Trace,
                format_args!("{target}\n"));
        }
        assert_eq!(TCP_RECORDS.lock().unwrap().len(), 6);
        assert_eq!(*TCP_RECORDS.lock().unwrap(), *UART_RECORDS.lock().unwrap());
        for area in A::ALL {
            assert!(TRUEOS_LOG_ROUTER.enabled(area, L::Error));
            assert!(TRUEOS_LOG_ROUTER.enabled(area, L::Warn));
        }
        assert!(!TRUEOS_LOG_ROUTER.enabled(A::Hv, L::Trace));
        assert!(!TRUEOS_LOG_ROUTER.enabled(A::Net, L::Trace));
    }
}
'''

RING_TESTS = r'''
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn prefix_and_tail_are_contiguous_without_overflow_or_duplicates() {
        let mut ring = TcpLogRing::new();
        ring.write_bytes(b"early boot");
        let mut out = ring.drain_bytes(7);
        ring.write_bytes(b" plus later frames and more data until prefix boundary");
        while ring.tcp_cursor < ring.written { out.extend(ring.drain_bytes(9)); }
        assert_eq!(out, b"early boot plus later frames and more data until prefix boundary");
        assert!(ring.drain_bytes(9).is_empty());
    }
    #[test] fn early_boot_survives_wrap_and_loss_is_explicit_and_exact() {
        let mut ring = TcpLogRing::new();
        let data: Vec<u8> = (0..1024).map(|n| (n % 251) as u8).collect();
        for chunk in data.chunks(13) { ring.write_bytes(chunk); }
        let mut out = Vec::new();
        while ring.tcp_cursor < ring.written { out.extend(ring.drain_bytes(17)); }
        let gap = b"\n[boot] [warn] logtotcp: capture-gap dropped_bytes=736 from=32 to=768\n";
        assert_eq!(&out[..32], &data[..32]);
        assert_eq!(&out[32..32+gap.len()], gap);
        assert_eq!(&out[32+gap.len()..], &data[768..]);
    }
    #[test] fn oversized_write_and_independent_screen_reader_keep_correct_tail() {
        let mut ring = TcpLogRing::new();
        ring.write_bytes(&vec![b'a'; 600]);
        ring.write_bytes(b"end");
        let mut cursor = 0;
        let mut screen = [0; 256];
        let (take, lost) = ring.copy_since(&mut cursor, &mut screen);
        assert_eq!((take, lost), (256, 347));
        assert_eq!(&screen[253..], b"end");
        assert_eq!(ring.tcp_cursor, 0);
        assert_eq!(ring.drain_bytes(32), vec![b'a'; 32]);
    }
}
'''


def main():
    source = (ROOT / "src/log_os.rs").read_text()
    flags = source[source.index("pub(crate) mod flags {"):source.index("\nstatic LOG_WRITE_LOCK")]
    adapters = source[source.index("struct TcpLogSink;"):source.index("#[macro_export]")]
    ring = source[source.index("    struct TcpLogRing {"):source.index("    static RING:")]
    host = r'''
#![allow(dead_code)]
extern crate alloc;
use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};
static EMULATOR_UART_LOGGING: AtomicBool = AtomicBool::new(true);
static TCP_RECORDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
static UART_RECORDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
fn write_with_tags(_: flags::LogArea, _: Option<&str>, args: fmt::Arguments<'_>) {
    TCP_RECORDS.lock().unwrap().push(format!("{args}"));
}
fn write_uart_with_tags(_: flags::LogArea, _: Option<&str>, args: fmt::Arguments<'_>) {
    UART_RECORDS.lock().unwrap().push(format!("{args}"));
}
'''
    with tempfile.TemporaryDirectory(prefix="laptop-capture-") as temporary:
        project = Path(temporary)
        (project / "Cargo.toml").write_text(
            '[package]\nname="laptop-capture-test"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[lib]\npath="lib.rs"\n[dependencies]\nspin="0.10"\n'
            f'log_os_core={{package="log-os",path="{ROOT / "vendor/log-os"}"}}\n')
        (project / "lib.rs").write_text(
            host + f'#[path="{ROOT}/src/log_policy.rs"] mod policy;\n'
            + flags + adapters + TESTS
            + '\nmod ring { use core::cmp::min; const MAX_BYTES:usize=256; const BOOT_PREFIX_BYTES:usize=32;\n'
            + ring + RING_TESTS + '\n}\n')
        channel = re.search(r'channel\s*=\s*"([^"]+)"',
                            (ROOT / "rust-toolchain.toml").read_text()).group(1)
        env = dict(os.environ, RUSTUP_TOOLCHAIN=channel)
        env.pop("RUSTFLAGS", None)
        env.pop("CARGO_ENCODED_RUSTFLAGS", None)
        subprocess.run(["cargo", "test", "--offline", "--manifest-path", str(project / "Cargo.toml"),
                        "--", "--test-threads=1"], cwd=project, env=env, check=True)


if __name__ == "__main__":
    main()
