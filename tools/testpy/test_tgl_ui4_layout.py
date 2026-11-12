#!/usr/bin/env python3
"""Run production TGL plane/address/DBUF tests without display hardware.

Only native-panel discovery and the existing desktop capacities are fixtures.
The complete layout module and native capacity selection compile unchanged.
Exercise both plane profiles while retaining the same native 4K resources.
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
            if native {
                let guarded_bytes = 15_616u64 * (2160 + 64);
                let overlay_bytes = 3840u64 * 2160 * 4;
                assert!(guarded_bytes > PRIMARY_SWAP_GPU_STRIDE);
                assert!(guarded_bytes <= native_ui4::primary_swap_capacity(pipe));
                assert!(overlay_bytes <= native_ui4::overlay_capacity(pipe));
            }
        }
    }
}

#[test]
fn five_plane_profile_keeps_interaction_above_three_application_leases() {
    assert!(!tgl_ui4_layout::EXPANDED_PLANES_ENABLED,
        "the comparison boot must use the five-plane profile");
    assert_eq!(tgl_ui4_layout::plane_count(false),5);
    assert_eq!(tgl_ui4_layout::application_mask(false),0x0f);
    for logical in 0..5 {
        assert_eq!(tgl_ui4_layout::profile_physical_slot(logical,false),Some(logical));
        assert_eq!(tgl_ui4_layout::profile_logical_slot(logical,false),Some(logical));
    }
    for inactive in [5,6,7,usize::MAX] {
        assert_eq!(tgl_ui4_layout::profile_physical_slot(inactive,false),None);
        assert_eq!(tgl_ui4_layout::profile_logical_slot(inactive,false),None);
    }
    assert_eq!(tgl_ui4_layout::application_mask(false) & (1 << 4),0,
        "the top interaction carrier is never an application lease");
}

#[test]
fn expanded_profile_retains_its_original_physical_order_and_application_mask() {
    assert_eq!(tgl_ui4_layout::plane_count(true),7);
    assert_eq!(tgl_ui4_layout::application_mask(true),0x2f);
    for (physical,logical) in [0,1,2,3,5,6,4].into_iter().enumerate() {
        assert_eq!(tgl_ui4_layout::profile_physical_slot(logical,true),Some(physical));
        assert_eq!(tgl_ui4_layout::profile_logical_slot(physical,true),Some(logical));
    }
    for invalid in [7,usize::MAX] {
        assert_eq!(tgl_ui4_layout::profile_physical_slot(invalid,true),None);
        assert_eq!(tgl_ui4_layout::profile_logical_slot(invalid,true),None);
    }
    assert_eq!(tgl_ui4_layout::application_mask(true) & ((1 << 4) | (1 << 6)),0,
        "interaction and the unowned slot are not application leases");
}

#[test]
fn both_profiles_preserve_4k_watermarks_and_disjoint_cursor_safe_dbuf_ranges() {
    for expanded in [false,true] {
        let mut ranges = Vec::new();
        let mut physical_mask = 0u8;
        let count = tgl_ui4_layout::plane_count(expanded);
        for logical in 0..count {
            let physical = tgl_ui4_layout::profile_physical_slot(logical,expanded).unwrap();
            if !tgl_ui4_layout::profile_physical_plane_active(physical,expanded) {
                assert_eq!(logical,6);
                assert_eq!(tgl_ui4_layout::dbuf_range(logical),None);
                assert_eq!(tgl_ui4_layout::watermark(logical),None);
                continue;
            }
            assert_eq!(physical_mask & (1 << physical),0);
            physical_mask |= 1 << physical;
            let (start,end) = tgl_ui4_layout::dbuf_range(logical).unwrap();
            assert!(start <= end && end < 2048);
            for (cursor_start,cursor_end) in [(1008,1023),(2032,2047)] {
                assert!(end < cursor_start || start > cursor_end);
            }
            let wm = tgl_ui4_layout::watermark(logical).unwrap();
            let blocks = wm & 0x7ff;
            assert_ne!(wm & (1 << 31),0);
            assert_eq!((wm >> 14) & 0x1f,8);
            assert_eq!(blocks,249);
            assert!(blocks + blocks.div_ceil(10) < u32::from(end - start + 1));
            if !expanded {
                assert_eq!(end - start + 1,275);
            }
            ranges.push((start,end));
        }
        assert_eq!(physical_mask,if expanded { 0x5f } else { 0x1f });
        assert!(ranges.iter().any(|&(start,_)| start >= 1024),
            "both profiles still need DBUF slice 2 at native width");
        ranges.sort_unstable();
        for pair in ranges.windows(2) {
            assert!(pair[0].1 < pair[1].0);
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
