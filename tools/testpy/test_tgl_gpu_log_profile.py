#!/usr/bin/env python3
"""Host-test the production flags and TCP/UART acceptance, not the kernel.

Uses the same sibling log-os dependency as TRUEOS. Network, UART hardware,
and the executor are not started. No repository files are rewritten.
"""
from pathlib import Path
import json
import os
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SWITCH = "TGL_GPU_DIAG_PROFILE_ENABLED"

TESTS = r'''
#[cfg(test)]
mod tests {
    use super::*;
    use log_os_core::{GlobalLogDispatch, LogArea as A, LogLevel as L};
    const AREAS: [A; 16] = [A::Global, A::Boot, A::Service, A::Net, A::Usb,
        A::Storage, A::Gfx, A::Gpgpu, A::Render, A::Hda, A::Hv, A::Apps,
        A::ExecutorRealm, A::ExecutorCache, A::IntelMediaNgin, A::Blueprint];
    const LEVELS: [L; 7] = [L::Error, L::Important, L::Warn, L::Once,
        L::Info, L::Debug, L::Trace];
    // Baseline at introduction, not recomputed from the implementation under test.
    const BASELINE: [L; 16] = [L::Warn, L::Warn, L::Warn, L::Info, L::Info,
        L::Info, L::Warn, L::Once, L::Info, L::Warn, L::Trace, L::Trace,
        L::Info, L::Warn, L::Info, L::Trace];
    fn cutoff(index: usize, area: A) -> L {
        if !flags::TGL_GPU_DIAG_PROFILE_ENABLED { return BASELINE[index]; }
        match area {
            A::Gfx | A::Gpgpu => L::Info,
            A::Render | A::Boot | A::Service => L::Once,
            _ => L::Warn,
        }
    }
    #[test]
    fn complete_area_level_matrix() {
        for (index, area) in AREAS.into_iter().enumerate() {
            for level in LEVELS {
                assert_eq!(flags::area_log_enabled(area, level), level <= cutoff(index, area),
                    "area={area:?} level={level:?}");
            }
        }
    }
    #[test]
    fn no_area_loses_errors_important_or_warnings() {
        for area in AREAS {
            for level in [L::Error, L::Important, L::Warn] {
                assert!(flags::area_log_enabled(area, level), "{area:?} {level:?}");
            }
        }
    }
    #[test]
    fn real_callsite_areas_keep_gpu_evidence_and_reject_the_photographed_flood() {
        for (target, expected_area) in [
            ("intel", A::Gfx), ("intel/guc", A::Gfx),
            ("TRUEOS::intel::tgl_native_panel", A::Gfx),
            ("TRUEOS::intel::display", A::Gfx),
            ("intel/gpgpu", A::Gpgpu),
            ("TRUEOS::intel::gpgpu::operations::spirit_vfx", A::Gpgpu),
            ("render", A::Render), ("intel/render", A::Render),
            ("crab_usb::backend::kmod", A::Usb), ("hv", A::Hv),
            ("ui4/start-button", A::Global), ("intel/media-encode", A::IntelMediaNgin),
        ] {
            assert_eq!(log_os_core::target_log_area(target), expected_area, "{target}");
            if flags::TGL_GPU_DIAG_PROFILE_ENABLED {
                assert_eq!(flags::area_log_enabled(expected_area, L::Info),
                    matches!(expected_area, A::Gfx | A::Gpgpu), "{target}");
                assert!(flags::area_log_enabled(expected_area, L::Error), "{target}");
            }
        }
    }
    #[test]
    fn actual_tcp_and_uart_adapters_agree_through_the_actual_router() {
        TCP_RECORDS.lock().unwrap().clear();
        UART_RECORDS.lock().unwrap().clear();
        let mut expected = 0;
        for (index, area) in AREAS.into_iter().enumerate() {
            for level in LEVELS {
                let accepted = level <= cutoff(index, area);
                assert_eq!(TRUEOS_LOG_ROUTER.enabled(area, level), accepted);
                log_os_core::log_with_area_level(&TRUEOS_LOG_ROUTER, area, level,
                    format_args!("area={area:?} level={level:?}\n"));
                expected += usize::from(accepted);
                assert_eq!(TCP_RECORDS.lock().unwrap().len(), expected);
                assert_eq!(UART_RECORDS.lock().unwrap().len(), expected);
            }
        }
        assert_eq!(*TCP_RECORDS.lock().unwrap(), *UART_RECORDS.lock().unwrap());
    }
}
'''

HOST = r'''
#![allow(dead_code)]
use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};
static EMULATOR_UART_LOGGING: AtomicBool = AtomicBool::new(true);
static TCP_RECORDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
static UART_RECORDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
// Replace only the physical writes; adapters and shared router remain real.
fn write_with_tags(area: flags::LogArea, purpose: Option<&str>, args: fmt::Arguments<'_>) {
    TCP_RECORDS.lock().unwrap().push(format!("[{}] [{}] {}", area.tag(), purpose.unwrap_or(""), args));
}
fn write_uart_with_tags(area: flags::LogArea, purpose: Option<&str>, args: fmt::Arguments<'_>) {
    UART_RECORDS.lock().unwrap().push(format!("[{}] [{}] {}", area.tag(), purpose.unwrap_or(""), args));
}
'''


def main() -> None:
    source = (ROOT / "src/log_os.rs").read_text()
    flags_start = source.index("pub(crate) mod flags {")
    flags_end = source.index("\nstatic LOG_WRITE_LOCK", flags_start)
    flags = source[flags_start:flags_end]
    adapters = source[source.index("struct TcpLogSink;"):source.index("#[macro_export]")]
    assert "flags::area_log_policy(area)" in adapters
    assert "flags::area_log_enabled(area, level)" in adapters
    assert "log-profile=tgl-gpu-first-v1" in source
    pattern = r"(pub\(crate\) const " + SWITCH + r": bool = )(true|false);"
    assert len(re.findall(pattern, flags)) == 1
    dependency = ROOT.parent / "TRUEOS-Blueprints/crates/log-os"
    if not (dependency / "Cargo.toml").is_file():
        raise SystemExit(f"missing kernel dependency: {dependency}")
    if not shutil.which("cargo"):
        raise SystemExit("cargo is required; no compiled tests were run")
    enabled = re.sub(pattern, r"\g<1>true;", flags)
    disabled = re.sub(pattern, r"\g<1>false;", flags)
    competing = re.sub(r"(pub\(crate\) const [A-Z0-9_]+_DIAG_PROFILE_ENABLED: bool = )(?:true|false);",
                       r"\g<1>true;", enabled)
    with tempfile.TemporaryDirectory(prefix="trueos-gpu-log-profile-") as directory:
        work = Path(directory)
        (work / "Cargo.toml").write_text(
            '[package]\nname="trueos-gpu-log-profile-test"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[lib]\npath="lib.rs"\n[dependencies]\nspin="0.10"\n'
            'log_os_core={package="log-os",path=' + json.dumps(str(dependency)) + '}\n')
        env = dict(os.environ, RUSTUP_TOOLCHAIN="stable", CARGO_TARGET_DIR=str(work / "target"))
        for label, policy in [("enabled", enabled), ("disabled/baseline", disabled),
                              ("enabled/all competing profiles", competing)]:
            (work / "lib.rs").write_text(HOST + policy + adapters + TESTS)
            print(f"Testing {label}", flush=True)
            subprocess.run(["cargo", "test", "--lib", "--manifest-path", str(work / "Cargo.toml"),
                            "--", "--test-threads=1"], cwd=work, env=env, check=True)
    print("PASS: 3 profile configurations, 12 compiled policy/real-sink tests; not a kernel build")


if __name__ == "__main__":
    main()
