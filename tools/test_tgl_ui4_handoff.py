#!/usr/bin/env python3
"""Host regression checks for the native-panel -> ordinary UI4 handoff.

Runs the real native address-layout module in a Rust harness with hardware
facts stubbed out. This is not a kernel build or a GPU execution test.
"""
from pathlib import Path
import json
import os
import re
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def function(source: str, name: str) -> str:
    match = re.search(r"(?m)^(?:pub(?:\([^\n]*?\))? )?(?:const )?fn " + re.escape(name) + r"\(", source)
    assert match, f"missing function: {name}"
    end = source.find("\n}", match.start())
    assert end >= 0, name
    return source[match.start():end + 2]


def main() -> None:
    display = (ROOT / "src/intel/display.rs").read_text()
    panel = (ROOT / "src/intel/tgl_native_panel.rs").read_text()
    intel = (ROOT / "src/intel/mod.rs").read_text()
    assert "mod native_ui4;" in display
    assert "pub(crate) const SPIRIT_RESEALED: bool = false;" in panel
    assert "SCANOUT_LATCHED.load(Ordering::Acquire)" in function(panel, "native_scanout_ready")
    assert re.search(
        r"if self::tgl_native_panel::init_once\(dev\)\s*\{\s*self::display::init_primary_boot_surface\(dev\);",
        function(intel, "init_once"),
    ), "native success must enter the real display bootstrap"
    assert "bootstrap_ui4_rgba8_plane_stack_once(dev, primary_surface)" in function(display, "init_primary_boot_surface")
    for name in ["primary_surface_gpu_for_pipe", "primary_surface_gpu_capacity",
                 "overlay_surface_gpu_for_index", "primary_swap_surface_gpu_for_index"]:
        assert "native_ui4::active(pipe)" in function(display, name), name
    for name in ["primary_compose_rcs_gpu_for_surface", "overlay_compose_rcs_gpu_for_surface"]:
        assert "native_ui4::active(surface.pipe)" in function(display, name), name
    assert "native_ui4::overlay_capacity(pipe)" in function(display, "ensure_overlay_surface_for_pipe")
    assert "native_ui4::primary_swap_capacity(pipe)" in function(display, "ensure_primary_swap_surface_for_pipe")
    primary = function(display, "compose_premultiplied_rgba_tiles_into_primary_gpgpu")
    assert "native_ui4::compose_capacity(surface.pipe, 0)" in primary
    assert "native_ui4::base_gpu(primary.pipe, primary.gpu)" in primary
    assert "native_ui4::compose_capacity(surface.pipe, surface.plane_slot)" in function(
        display, "compose_premultiplied_rgba_tiles_into_overlay_gpgpu")
    for name in ["OVERLAY_SWAP_GPU_STRIDE", "PRIMARY_SWAP_GPU_STRIDE", "COMPOSE_RCS_GPU_ALIAS_BYTES"]:
        assert re.search(r"const " + name + r": u64 = 0x0100_0000;", display), name
    # The probe retains its own allocation; UI4 must not remap/free it.
    assert "pub(super) const SURFACE_GPU: u64 = 0xE000_0000;" in panel
    assert "RETAINED_FRAME.call_once(|| frame);" in panel
    print("PASS: native-success handoff, unseal, allocation/compute wiring, desktop constants", flush=True)

    module_path = json.dumps(str(ROOT / "src/intel/display/native_ui4.rs"))
    harness = r'''
#![allow(dead_code)]
mod intel {
    pub mod tgl_native_panel {
        pub static READY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
        pub const SURFACE_GPU: u64 = 0xE000_0000;
        pub const SURFACE_GPU_CAPACITY: u64 = 0x0400_0000;
        pub fn native_scanout_ready() -> bool { READY.load(core::sync::atomic::Ordering::Acquire) }
    }
    pub mod gpgpu { pub const DIRECT_RCS_PPGTT_LIMIT_BYTES: u64 = 0x4000_0000; }
}
mod r { pub mod ui_surface { pub const UI_SURFACE_GPU_LIMIT: u64 = 0x2000_0000; } }
mod display {
    #[derive(Clone, Copy)] pub struct PipeInfo { pub slot: usize }
    const PRIMARY_SWAP_BUFFER_COUNT: usize = 2;
    const OVERLAY_SWAP_BUFFER_COUNT: usize = 2;
    const OVERLAY_UNIVERSAL_PLANE_COUNT: usize = 4;
    const DIRECT_RCS_OVERLAY_UNIVERSAL_PLANE_COUNT: usize = 3;
    const UI4_DIRECT_SCANOUT_GPU_BASE: u64 = 0x5000_0000;
    const UI4_DIRECT_SCANOUT_PLANE_COUNT: usize = 4;
    const UI4_DIRECT_SCANOUT_PLANE_STRIDE: u64 = 0x2000_0000;
    const PRIMARY_SWAP_GPU_STRIDE: u64 = 0x0100_0000;
    const OVERLAY_SWAP_GPU_STRIDE: u64 = 0x0100_0000;
    const COMPOSE_RCS_GPU_ALIAS_BYTES: u64 = 0x0100_0000;
    #[path = MODULE_PATH] mod native_ui4;
    #[test]
    fn native_layout_and_target_isolation() {
        use native_ui4 as n;
        let a = PipeInfo { slot: 0 };
        let mib = 1024u64 * 1024;
        assert!(!n::active(a));
        assert_eq!(n::base_gpu(a, 0x0200_0000), 0x0200_0000);
        assert_eq!(n::primary_swap_capacity(a), 16 * mib);
        assert_eq!(n::overlay_capacity(a), 16 * mib);
        assert_eq!(n::compose_capacity(a, 0), 16 * mib);
        crate::intel::tgl_native_panel::READY.store(true, core::sync::atomic::Ordering::Release);
        assert!(n::active(a));
        for slot in 1..4 {
            let pipe = PipeInfo { slot };
            assert!(!n::active(pipe));
            assert_eq!(n::primary_swap_capacity(pipe), 16 * mib);
            assert_eq!(n::overlay_capacity(pipe), 16 * mib);
            assert_eq!(n::compose_capacity(pipe, 0), 16 * mib);
            assert_eq!(n::base_gpu(pipe, 0x3900_0000), 0x3900_0000);
        }
        let tight = 3840u64 * 2160 * 4;
        let pitch = (3840u64 + 64) * 4;
        let guarded = pitch * (2160 + 64);
        assert!(tight > 16 * mib && tight <= n::overlay_capacity(a));
        assert!(pitch * 2160 > 32 * mib);
        assert!(guarded <= n::PRIMARY_BYTES && guarded <= n::primary_swap_capacity(a));
        assert!(guarded <= n::compose_capacity(a, 0));
        let mut ggtt = vec![(0xE000_0000, 64 * mib), (n::PRIMARY_GPU, n::PRIMARY_BYTES)];
        let mut ppgtt = vec![(n::base_gpu(a, 0), n::PRIMARY_BYTES)];
        for index in 0..2 {
            ggtt.push((n::primary_swap_gpu(index).unwrap(), n::PRIMARY_BYTES));
            ppgtt.push((n::compose_gpu(0, index).unwrap(), n::PRIMARY_BYTES));
            for slot in 1..=4 {
                ggtt.push((n::overlay_gpu(slot, index).unwrap(), 32 * mib));
                if slot <= 3 { ppgtt.push((n::compose_gpu(slot, index).unwrap(), 32 * mib)); }
                else { assert_eq!(n::compose_gpu(slot, index), None); }
            }
        }
        ggtt.sort_unstable();
        ppgtt.sort_unstable();
        for (ranges, floor, limit) in [(&ggtt, 0xD000_0000, 0x1_0000_0000),
                                       (&ppgtt, 0x2000_0000, 0x4000_0000)] {
            for &(start, bytes) in ranges {
                assert_eq!(start % 4096, 0);
                assert!(start >= floor && start + bytes <= limit);
            }
            for pair in ranges.windows(2) { assert!(pair[0].0 + pair[0].1 <= pair[1].0); }
        }
        assert_eq!(n::overlay_gpu(0, 0), None);
        assert_eq!(n::overlay_gpu(5, 0), None);
        assert_eq!(n::overlay_gpu(usize::MAX, 0), None);
        assert_eq!(n::primary_swap_gpu(2), None);
        assert_eq!(n::primary_swap_gpu(usize::MAX), None);
        assert_eq!(n::overlay_gpu(1, 2), None);
        assert_eq!(n::compose_gpu(0, 2), None);
        assert_eq!(n::compose_gpu(usize::MAX, 0), None);
        assert_eq!(n::base_gpu(a, 0xE400_0000), 0x3400_0000);
    }
}
'''.replace("MODULE_PATH", module_path)
    rustc = shutil.which("rustc")
    if not rustc:
        raise SystemExit("rustc is required: layout compilation was NOT run")
    env = dict(os.environ, RUSTUP_TOOLCHAIN="stable")
    with tempfile.TemporaryDirectory(prefix="trueos-tgl-ui4-") as temporary:
        work = Path(temporary)
        source = work / "layout_test.rs"
        binary = work / "layout_test"
        source.write_text(harness)
        subprocess.run([rustc, "--edition=2024", "--test", str(source), "-o", str(binary)], check=True, env=env)
        subprocess.run([str(binary), "--nocapture"], check=True, env=env)
    print("PASS: actual native layout module compiled; 4K capacities, VA separation and target isolation")


if __name__ == "__main__":
    main()
