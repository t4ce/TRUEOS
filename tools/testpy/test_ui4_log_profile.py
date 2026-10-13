#!/usr/bin/env python3
"""Compile the production policy and real TCP/UART adapters for UI4/TGL combinations."""
from pathlib import Path
import json, os, re, runpy, subprocess, tempfile
ROOT = Path(__file__).resolve().parents[2]
HOST = runpy.run_path(str(ROOT / "tools/testpy/test_tgl_gpu_log_profile.py"))["HOST"]
TESTS = r''' 
#[cfg(test)] mod tests {
    use super::*;
    use log_os_core::{GlobalLogDispatch, LogArea as A, LogLevel as L};
    #[test] fn ui4_and_vgpu_trace_survive_tgl_and_hv_profiles() {
        for target in ["ui4", "ui4/input", "ui4/slot0", "vgpu"] {
            assert_eq!(log_os_core::target_log_area(target), A::Global);
        }
        for &level in L::ALL {
            let expected = flags::UI4_DIAG_PROFILE_ENABLED || level <= L::Warn;
            assert_eq!(flags::area_log_enabled(A::Global, level), expected);
            assert_eq!(TRUEOS_LOG_ROUTER.enabled(A::Global, level), expected);
        }
    }
    #[test] fn both_capture_sinks_receive_admitted_ui4_diagnostics() {
        TCP_RECORDS.lock().unwrap().clear();
        UART_RECORDS.lock().unwrap().clear();
        log_os_core::log_with_area_level(&TRUEOS_LOG_ROUTER, A::Global, L::Trace,
            format_args!("ui4 ghost diagnostic\n"));
        assert_eq!(TCP_RECORDS.lock().unwrap().len(), usize::from(flags::UI4_DIAG_PROFILE_ENABLED));
        assert_eq!(*TCP_RECORDS.lock().unwrap(), *UART_RECORDS.lock().unwrap());
    }
}
'''
def main():
    source = (ROOT / "src/log_os.rs").read_text()
    flags = source[source.index("pub(crate) mod flags {"):source.index("\nstatic LOG_WRITE_LOCK")]
    adapters = source[source.index("struct TcpLogSink;"):source.index("#[macro_export]")]
    # Isolate the independently selected policies; leave tested production code intact.
    flags = re.sub(r"(pub\(crate\) const [A-Z0-9_]+_DIAG_PROFILE_ENABLED: bool = )(?:true|false);", r"\g<1>false;", flags)
    dep = ROOT.parent / "TRUEOS-Blueprints/crates/log-os"
    with tempfile.TemporaryDirectory(prefix="ui4-log-policy-") as tmp:
        work = Path(tmp)
        (work / "Cargo.toml").write_text('[package]\nname="ui4-log-policy-test"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[lib]\npath="lib.rs"\n[dependencies]\nspin="0.10"\nlog_os_core={package="log-os",path=' + json.dumps(str(dep)) + '}\n')
        env = dict(os.environ, RUSTUP_TOOLCHAIN="stable", CARGO_TARGET_DIR=str(work / "target"))
        for ui4,tgl,hv in [(False,True,False),(True,True,False),(True,False,False),(True,True,True)]:
            policy = flags
            for name,value in [("UI4_DIAG_PROFILE_ENABLED",ui4),("TGL_GPU_DIAG_PROFILE_ENABLED",tgl),("BLUEPRINT_HV_DEBUG_PROFILE_ENABLED",hv)]:
                policy = re.sub(rf"(const {name}: bool = )(true|false);", rf"\g<1>{str(value).lower()};", policy)
            live_policy = f'#[path="{ROOT}/src/log_policy.rs"] mod policy;\n'
            (work / "lib.rs").write_text(HOST + policy + adapters + live_policy + TESTS)
            print(f"UI4={ui4} TGL={tgl} HV={hv}", flush=True)
            subprocess.run(["cargo","test","--lib","--manifest-path",str(work / "Cargo.toml"),"--","--test-threads=1"], cwd=work,env=env,check=True)
if __name__ == "__main__": main()
