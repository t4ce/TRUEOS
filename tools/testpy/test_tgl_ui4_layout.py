#!/usr/bin/env python3
"""Run production TGL plane/address/DBUF tests without display hardware.

Only native-panel discovery and the existing desktop capacities are fixtures.
The complete layout module and native capacity selection compile unchanged.
"""

import json
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    source = r'''
#![allow(dead_code)]
#[derive(Clone, Copy)] struct PipeInfo { slot: usize }
const PRIMARY_SWAP_GPU_STRIDE: u64 = 0x0100_0000;
const OVERLAY_SWAP_GPU_STRIDE: u64 = 0x0100_0000;
mod r { pub mod ui_surface { pub const UI_SURFACE_GPU_LIMIT: u64 = 0x2000_0000; } }
mod intel {
    pub mod gpgpu { pub const DIRECT_RCS_PPGTT_LIMIT_BYTES: u64 = 1 << 30; }
    pub mod tgl_native_panel {
        use std::sync::atomic::{AtomicBool, Ordering};
        pub static READY: AtomicBool = AtomicBool::new(false);
        pub fn native_scanout_ready() -> bool { READY.load(Ordering::Relaxed) }
    }
}
#[path = @LAYOUT@] mod tgl_ui4_layout;
#[path = @NATIVE@] mod native_ui4;

#[test]
fn native_capacity_and_source_selection_changes_only_the_adopted_pipe() {
    use std::sync::atomic::Ordering;
    let desktop_gpu = 0x0200_0000;
    for adopted in [false, true] {
        intel::tgl_native_panel::READY.store(adopted, Ordering::Relaxed);
        for slot in 0..4 {
            let pipe = PipeInfo { slot };
            let native = adopted && slot == 0;
            assert_eq!(native_ui4::active(pipe), native);
            assert_eq!(native_ui4::primary_swap_capacity(pipe),
                if native { 0x0400_0000 } else { PRIMARY_SWAP_GPU_STRIDE });
            assert_eq!(native_ui4::overlay_capacity(pipe),
                if native { 0x0200_0000 } else { OVERLAY_SWAP_GPU_STRIDE });
            assert_eq!(native_ui4::base_gpu(pipe, desktop_gpu),
                if native { 0x3800_0000 } else { desktop_gpu });
        }
    }
}
'''.replace("@LAYOUT@", json.dumps(str(ROOT / "src/intel/display/tgl_ui4_layout.rs"))).replace(
        "@NATIVE@", json.dumps(str(ROOT / "src/intel/display/native_ui4.rs"))
    )
    with tempfile.TemporaryDirectory(prefix="trueos-tgl-ui4-layout-") as directory:
        code = Path(directory) / "tests.rs"
        binary = Path(directory) / "tests"
        code.write_text(source)
        subprocess.run([
            "rustc", "--edition=2024", "--target=x86_64-unknown-linux-gnu", "--test",
            "-C", "linker=" + str(ROOT / "tools/rust-host-ld"), str(code), "-o", str(binary),
        ], cwd=ROOT, check=True)
        subprocess.run([str(binary)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
