#!/usr/bin/env python3
"""Exercise production live-update/VPID code with only hardware and VM storage mocked.

The native park trampoline is also compiled and executed on its unprivileged
abort path. VMX instructions and a full kernel handoff still require hardware.
"""
from pathlib import Path
import json
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


def item(source, declaration):
    start = source.index(declaration)
    brace = source.index("{", start)
    depth = 1
    end = brace + 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


class LiveUpdateVmxTests(unittest.TestCase):
    def test_retained_vm_and_abort_contracts(self):
        hv = (ROOT / "src/hv/mod.rs").read_text()
        live = (ROOT / "src/live_update.rs").read_text()
        fixture = (Path(__file__).parent / "live_update_vmx/harness.rs").read_text()
        hv_code = "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n" + item(hv, "pub(crate) enum LiveUpdateVmxDisposition")
        for declaration in ["pub(crate) fn prepare_current_cpu_for_live_update", "pub fn leave_vmx_root_for_current_cpu_contract"]:
            hv_code += "\n" + item(hv, declaration)
        live_code = "#[repr(C, align(64))]\n" + item(live, "struct TransitionControl")
        live_code += "\n#[derive(Debug)]\n" + item(live, "pub enum LiveUpdateError")
        for declaration in ["impl fmt::Display for LiveUpdateError", "struct ApTransitionError",
                            "impl ApTransitionError", "fn mark_ap_mask", "fn enter_ap_transition_if_requested",
                            "fn reset_ap_transition_diagnostics", "fn ap_transition_error"]:
            live_code += "\n" + item(live, declaration)
        live_code += "\n#[unsafe(naked)]\n" + item(live, "unsafe extern \"C\" fn trueos_live_update_ap_park_trampoline")
        with tempfile.TemporaryDirectory(prefix="trueos-live-update-vmx-") as directory:
            directory = Path(directory)
            # Include the complete production VPID implementation, adding only
            # fixture reset access for independent serial test cases.
            vpid = (ROOT / "src/hv/vpid.rs").read_text() + r'''
pub(crate) fn reset_fixture() {
    for lane in &LANE_VPID_STATE {
        lane.state.store(LANE_OFFLINE, Ordering::Release);
        lane.owner_tag.store(0, Ordering::Release);
        lane.vpid.store(0, Ordering::Release);
        lane.generation.store(0, Ordering::Release);
    }
}
'''
            vpid_path = directory / "vpid.rs"
            vpid_path.write_text(vpid)
            fixture = fixture.replace("@VPID_PATH@", json.dumps(str(vpid_path)))
            fixture = fixture.replace("@HV_CODE@", hv_code).replace("@LIVE_CODE@", live_code)
            source = directory / "tests.rs"
            source.write_text(fixture)
            binary = directory / "tests"
            subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)], check=True)
            subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    unittest.main()
