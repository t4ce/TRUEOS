#!/usr/bin/env python3
"""Host-test production policies, TCP/UART routing, and the native log editor.

Only hardware/executor plumbing is removed; policy, input, painting and stream
code are the production sources. Does not boot or deploy a kernel.
"""
from pathlib import Path
import json
import os
import runpy
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
HOST = runpy.run_path(str(ROOT / "tools/testpy/test_tgl_gpu_log_profile.py"))["HOST"]
TESTS = r'''
#[cfg(test)] mod runtime_tests {
    use super::*;
    use log_os_core::{GlobalLogDispatch, LogArea as A, LogLevel as L, LogLevelFilter as F,
        LogLevelPolicy as P, LogLevelSet as S};
    #[test] fn build_defaults_seed_and_restore_every_area() {
        policy::apply_profile(0);
        for area in A::ALL {
            assert_eq!(flags::area_log_policy(area), flags::build_area_log_policy(area));
            policy::set_area_log_policy(area, P::only(S::NONE));
        }
        assert_eq!(policy::profile_name(), "Custom");
        policy::apply_profile(0);
        for area in A::ALL { assert_eq!(flags::area_log_policy(area), flags::build_area_log_policy(area)); }
    }
    #[test] fn both_real_sink_adapters_obey_every_live_policy_including_user_and_ai() {
        TCP_RECORDS.lock().unwrap().clear(); UART_RECORDS.lock().unwrap().clear();
        let mut policies = vec![P::only(S::ALL), P::only(S::NONE), P::only(S::USER.union(S::AI))];
        for &level in F::ALL { policies.push(P::up(level)); policies.push(P::down(level)); }
        let mut expected = 0;
        for area in A::ALL {
            for &p in &policies {
                policy::set_area_log_policy(area, p);
                assert_eq!(flags::area_log_policy(area), p);
                for &level in L::ALL {
                    let accepted = log_os_core::level_enabled(p, level);
                    assert_eq!(TRUEOS_LOG_ROUTER.enabled(area, level), accepted);
                    log_os_core::log_with_area_level(&TRUEOS_LOG_ROUTER, area, level, format_args!("record\n"));
                    expected += usize::from(accepted);
                    assert_eq!(TCP_RECORDS.lock().unwrap().len(), expected);
                    assert_eq!(UART_RECORDS.lock().unwrap().len(), expected);
                }
            }
        }
        assert_eq!(*TCP_RECORDS.lock().unwrap(), *UART_RECORDS.lock().unwrap());
        policy::apply_profile(0);
    }
    #[test] fn profiles_replace_previous_area_edits_and_keep_unrelated_areas_quiet() {
        for (index, profile) in policy::Profile::ALL.iter().enumerate() {
            policy::set_area_log_policy(A::Usb, P::only(S::NONE));
            policy::apply_profile(index);
            assert_eq!(policy::profile_name(), profile.name());
            for area in A::ALL { assert_eq!(flags::area_log_policy(area), profile.policy(area)); }
        }
        assert_eq!(policy::Profile::EarlyBoot.policy(A::Boot), P::up(F::Trace));
        assert_eq!(policy::Profile::UsbFull.policy(A::Usb), P::up(F::Trace));
        assert_eq!(policy::Profile::UsbFull.policy(A::Render), P::up(F::Warn));
        policy::apply_profile(0);
    }
}
'''


def main():
    source = (ROOT / "src/log_os.rs").read_text()
    flags = source[source.index("pub(crate) mod flags {"):source.index("\nstatic LOG_WRITE_LOCK")]
    adapters = source[source.index("struct TcpLogSink;"):source.index("#[macro_export]")]
    helper = (ROOT / "src/shell3/helper.rs").read_text()
    helper = helper[helper.index("#[derive(Clone, Copy, Debug, PartialEq, Eq)]"):helper.index("#[derive(Default)]\npub(super) struct Screen")]
    editor = (ROOT / "src/shell3/system/log.rs").read_text()
    editor = editor[:editor.index("pub(super) fn start")] + editor[editor.index("#[derive(Clone, Copy, PartialEq)]"):]
    editor = editor[:editor.index("#[trueos_executor::task")]
    with tempfile.TemporaryDirectory(prefix="live-log-policy-") as tmp:
        work = Path(tmp)
        (work / "Cargo.toml").write_text('[package]\nname="live-log-policy-test"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[lib]\npath="lib.rs"\n[dependencies]\nspin="0.10"\nlog_os_core={package="log-os",path=' + json.dumps(str(ROOT.parent / "TRUEOS-Blueprints/crates/log-os")) + '}\ntrueos_terminal={package="trueos-terminal",path=' + json.dumps(str(ROOT / "crates/trueos-terminal")) + '}\n')
        (work / "editor.rs").write_text(editor.replace("use trueos_time::{Duration, Timer};", ""))
        support = f'''
extern crate alloc;
#[path="{ROOT}/src/log_policy.rs"] pub(crate) mod policy;
mod log_os {{pub(crate) use crate::policy;}}
struct MatrixTarget;
mod tui {{pub struct Frontend;}}
mod helper {{use alloc::vec::Vec; {helper} }}
#[path="{work}/editor.rs"] mod editor;
'''
        (work / "lib.rs").write_text(HOST + flags + adapters + support + TESTS)
        env = dict(os.environ, RUSTUP_TOOLCHAIN="stable", CARGO_TARGET_DIR=str(work / "target"))
        subprocess.run(["cargo", "test", "--offline", "--manifest-path", str(work / "Cargo.toml"), "--", "--test-threads=1"], cwd=work, env=env, check=True)


if __name__ == "__main__":
    main()
