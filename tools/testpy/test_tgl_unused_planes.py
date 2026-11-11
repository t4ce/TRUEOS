#!/usr/bin/env python3
"""Exercise unused-plane retirement against staged and live display state.

Compile the production helper unchanged. Only SURF writes arm a plane, and
only a successful frame boundary transfers its staged state to scanout.
"""

from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item

ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    source = r'''
#![allow(dead_code)]
mod display {
    pub fn laptop_expanded_planes_enabled() -> bool { crate::panel::expanded() }
}
mod panel {
use std::cell::RefCell;
use std::collections::BTreeMap;
#[derive(Clone, Copy)] pub(super) struct Dev { target: bool }
const NATIVE: Dev = Dev { target: true };
const DESKTOP: Dev = Dev { target: false };
// Independent physical plane 6/7 addresses; logical remapping is not used.
const CTRL: [usize;2] = [0x70680, 0x70780];
const SURF: [usize;2] = [0x7069C, 0x7079C];
const ENABLE: u32 = 1 << 31;
const SURFACES: [u32;2] = [0xE400_0000,0xEC00_0000];
#[derive(Default)] struct Hardware {
    staged: BTreeMap<usize,u32>, live: BTreeMap<usize,u32>,
    armed: [bool;2], writes: Vec<(usize,u32)>, reads: Vec<usize>,
    frames: usize, handoff: bool, expanded: bool,
    successful_frames: usize, ignore_ctrl: bool,
}
thread_local! { static HW: RefCell<Hardware> = RefCell::new(Hardware::default()); }
fn is_target(dev: Dev) -> bool { dev.target }
fn native_scanout_ready() -> bool { HW.with(|h|h.borrow().handoff) }
pub(super) fn expanded() -> bool { HW.with(|h|h.borrow().expanded) }
fn mmio_read(_: Dev, address: usize) -> u32 {
    HW.with(|h| {
        let mut h=h.borrow_mut();
        h.reads.push(address);
        *h.staged.get(&address).expect("read outside unused physical planes")
    })
}
fn mmio_write(_: Dev, address: usize, value: u32) {
    HW.with(|h| {
        let mut h=h.borrow_mut();
        let plane=(0..2).find(|&i| [CTRL[i],SURF[i]].contains(&address))
            .expect("write outside unused physical planes");
        h.writes.push((address,value));
        if !(h.ignore_ctrl && address==CTRL[plane]) { h.staged.insert(address,value); }
        h.armed[plane]=address==SURF[plane];
    })
}
fn wait_frame(_: Dev) -> bool {
    HW.with(|h| {
        let mut h=h.borrow_mut(); h.frames+=1;
        if h.frames > h.successful_frames { return false; }
        for i in 0..2 {
            if h.armed[i] {
                for address in [CTRL[i],SURF[i]] {
                    let value=h.staged[&address];
                    h.live.insert(address,value);
                }
                h.armed[i]=false;
            }
        }
        true
    })
}
fn setup(staged_enabled: [bool;2], live_enabled: [bool;2]) {
    HW.with(|h| {
        let mut value=Hardware { handoff:true, successful_frames:2, ..Hardware::default() };
        for i in 0..2 {
            let control=0x1410_0008 | (i as u32)<<14;
            value.staged.insert(CTRL[i],control|if staged_enabled[i] {ENABLE} else {0});
            value.live.insert(CTRL[i],control|if live_enabled[i] {ENABLE} else {0});
            value.staged.insert(SURF[i],SURFACES[i]);
            value.live.insert(SURF[i],SURFACES[i]);
        }
        *h.borrow_mut()=value;
    });
}
'''
    for name in (
        "UNI_PLANE_BASE", "UNI_PLANE_SLOT_STRIDE", "UNI_PLANE_CTL_OFF",
        "UNI_PLANE_SURF_OFF", "PLANE_CTL_ENABLE",
    ):
        source += re.sub(r"pub\([^)]*\)\s+", "", constant("src/intel/display/regs.rs", name)) + "\n"
    source += item("src/intel/tgl_native_panel.rs", "disable_unused_ui4_planes")
    source += r'''
#[test]
fn retirement_arms_only_physical_planes_six_and_seven_with_their_existing_surfaces() {
    assert_eq!([UNI_PLANE_BASE+5*UNI_PLANE_SLOT_STRIDE,UNI_PLANE_BASE+6*UNI_PLANE_SLOT_STRIDE],CTRL);
    assert_eq!([CTRL[0]+UNI_PLANE_SURF_OFF,CTRL[1]+UNI_PLANE_SURF_OFF],SURF);
    setup([true,true],[true,true]);
    assert!(disable_unused_ui4_planes(NATIVE));
    HW.with(|h| {
        let h=h.borrow();
        assert_eq!(h.frames,2);
        assert_eq!(h.writes,[(CTRL[0],0x1410_0008),(SURF[0],SURFACES[0]),
                            (CTRL[1],0x1410_4008),(SURF[1],SURFACES[1])]);
        for i in 0..2 {
            assert_eq!(h.live[&CTRL[i]]&ENABLE,0);
            assert_eq!(h.live[&CTRL[i]],h.staged[&CTRL[i]]);
            assert_eq!(h.live[&SURF[i]],SURFACES[i]);
        }
    });
}
#[test]
fn disabled_staged_controls_still_retire_independently_enabled_live_planes() {
    for mask in 0..16 {
        setup([mask&1!=0,mask&2!=0],[mask&4!=0,mask&8!=0]);
        assert!(disable_unused_ui4_planes(NATIVE));
        HW.with(|h| {
            let h=h.borrow();
            assert_eq!(h.frames,2);
            assert_eq!(h.writes.len(),4,"every staged state still requires an arm");
            for i in 0..2 { assert_eq!(h.live[&CTRL[i]]&ENABLE,0); }
        });
    }
}
#[test]
fn missing_either_frame_cannot_report_success() {
    for frames in [0,1] {
        setup([true,true],[true,true]);
        HW.with(|h|h.borrow_mut().successful_frames=frames);
        assert!(!disable_unused_ui4_planes(NATIVE));
        HW.with(|h|assert_eq!(h.borrow().frames,frames+1));
    }
}
#[test]
fn ignored_control_write_cannot_report_success() {
    setup([true,true],[true,true]);
    HW.with(|h|h.borrow_mut().ignore_ctrl=true);
    assert!(!disable_unused_ui4_planes(NATIVE));
    HW.with(|h| {
        let h=h.borrow();
        assert_eq!(h.frames,2);
        for address in CTRL { assert_ne!(h.live[&address]&ENABLE,0); }
    });
}
#[test]
fn desktop_and_unadopted_panel_do_not_touch_display_registers() {
    for (dev,handoff) in [(DESKTOP,true),(NATIVE,false)] {
        setup([true,true],[true,true]);
        HW.with(|h|h.borrow_mut().handoff=handoff);
        assert!(!disable_unused_ui4_planes(dev));
        HW.with(|h| {
            let h=h.borrow();
            assert!(h.reads.is_empty());assert!(h.writes.is_empty());assert_eq!(h.frames,0);
        });
    }
}
#[test]
fn expanded_profile_preserves_the_extra_planes_without_mmio() {
    setup([true,true],[true,true]);
    HW.with(|h|h.borrow_mut().expanded=true);
    assert!(disable_unused_ui4_planes(NATIVE));
    HW.with(|h| {
        let h=h.borrow();
        assert!(h.reads.is_empty());assert!(h.writes.is_empty());assert_eq!(h.frames,0);
        for address in CTRL { assert_ne!(h.live[&address]&ENABLE,0); }
    });
}
}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-tgl-unused-planes-") as directory:
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
