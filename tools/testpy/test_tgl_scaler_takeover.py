#!/usr/bin/env python3
"""Exercise native scaler takeover against a staged/live MMIO model.

The production detach functions and register constants compile unchanged.
The model arms only on the documented PS_WIN_SZ addresses and transfers
staged state at a frame boundary; PS_CTRL readback alone cannot prove it.
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
mod panel {
use std::cell::RefCell;
use std::collections::BTreeMap;
#[derive(Clone, Copy)] struct Dev { target: bool }
const NATIVE: Dev = Dev { target: true };
const DESKTOP: Dev = Dev { target: false };
// Independent hardware addresses, also checked against the production constants.
const CTRL: [usize;2] = [0x68180, 0x68280];
const POS: [usize;2] = [0x68170, 0x68270];
const SIZE: [usize;2] = [0x68174, 0x68274];
#[derive(Default)] struct Hardware {
    staged: BTreeMap<usize,u32>, live: BTreeMap<usize,u32>,
    armed: [bool;2], writes: Vec<(usize,u32)>, reads: usize,
    frames: usize, handoff: bool, advance: bool, ignore_ctrl: bool,
}
thread_local! { static HW: RefCell<Hardware> = RefCell::new(Hardware::default()); }
fn is_target(dev: Dev) -> bool { dev.target }
fn native_scanout_ready() -> bool { HW.with(|h|h.borrow().handoff) }
fn mmio_read(_: Dev, address: usize) -> u32 {
    HW.with(|h| { let mut h=h.borrow_mut(); h.reads+=1; *h.staged.get(&address).unwrap_or(&0) })
}
fn mmio_write(_: Dev, address: usize, value: u32) {
    HW.with(|h| {
        let mut h=h.borrow_mut();
        let scaler=(0..2).find(|&i| [CTRL[i],POS[i],SIZE[i]].contains(&address))
            .expect("write outside scaler control/window registers");
        h.writes.push((address,value));
        if !(h.ignore_ctrl && address==CTRL[scaler]) { h.staged.insert(address,value); }
        // Every non-arming register disarms its pending update.
        h.armed[scaler]=address==SIZE[scaler];
    });
}
fn wait_frame(_: Dev) -> bool {
    HW.with(|h| {
        let mut h=h.borrow_mut(); h.frames+=1;
        if !h.advance { return false; }
        for i in 0..2 {
            if h.armed[i] {
                for address in [CTRL[i],POS[i],SIZE[i]] {
                    let value=*h.staged.get(&address).unwrap_or(&0);
                    h.live.insert(address,value);
                }
                h.armed[i]=false;
            }
        }
        true
    })
}
fn setup(bindings: [u32;2], staged_enabled: bool, live_enabled: bool) {
    HW.with(|h| {
        let mut value=Hardware { handoff:true, advance:true, ..Hardware::default() };
        for i in 0..2 {
            let control=bindings[i]<<25;
            value.staged.insert(CTRL[i],control|if staged_enabled {1<<31} else {0});
            value.live.insert(CTRL[i],control|if live_enabled {1<<31} else {0});
            for address in [POS[i],SIZE[i]] {
                value.staged.insert(address,0x01230234);
                value.live.insert(address,0x01230234);
            }
        }
        *h.borrow_mut()=value;
    });
}
'''
    for name in (
        "PIPE_SCALER_0_A_CTRL", "PIPE_SCALER_1_A_CTRL", "PIPE_SCALER_BINDING_MASK",
        "PIPE_SCALER_ENABLE", "PIPE_SCALER_WIN_POS_FROM_CTRL", "PIPE_SCALER_WIN_SIZE_FROM_CTRL",
    ):
        source += re.sub(r"pub\([^)]*\)\s+", "", constant("src/intel/display/regs.rs", name)) + "\n"
    for name in (
        "detach_scalers", "scalers_detached", "detach_primary_scalers",
        "primary_scalers_detached", "detach_ui4_scalers",
    ):
        source += item("src/intel/tgl_native_panel.rs", name)
    source += r'''
#[test]
fn primary_phase_preserves_overlay_scalers_and_arms_the_actual_window_register() {
    assert_eq!([PIPE_SCALER_0_A_CTRL,PIPE_SCALER_1_A_CTRL],CTRL);
    for primary in [0,1] {
        for foreign in 2..=7 {
            setup([primary,foreign],true,true);
            detach_primary_scalers(NATIVE);
            assert!(primary_scalers_detached(NATIVE));
            HW.with(|h| {
                let h=h.borrow();
                assert_ne!(h.live[&CTRL[0]]&(1<<31),0,"staged readback is insufficient");
                assert_eq!(h.writes,[(CTRL[0],0),(POS[0],0),(SIZE[0],0)]);
                assert_ne!(h.staged[&CTRL[1]]&(1<<31),0);
            });
            assert!(wait_frame(NATIVE));
            HW.with(|h| {
                let h=h.borrow();
                assert_eq!(h.live[&CTRL[0]],0);
                assert_ne!(h.live[&CTRL[1]]&(1<<31),0);
            });
        }
    }
}
#[test]
fn seven_plane_takeover_retires_every_inherited_binding_at_the_frame_boundary() {
    for first in 0..=7 {
        for second in 0..=7 {
            setup([first,second],true,true);
            assert!(detach_ui4_scalers(NATIVE));
            HW.with(|h| {
                let h=h.borrow();assert_eq!(h.frames,1);
                assert_eq!(h.writes,[(CTRL[0],0),(POS[0],0),(SIZE[0],0),
                    (CTRL[1],0),(POS[1],0),(SIZE[1],0)]);
                for i in 0..2 { assert_eq!(h.live[&CTRL[i]],0);assert_eq!(h.live[&SIZE[i]],0); }
            });
        }
    }
}
#[test]
fn zero_staged_enable_does_not_hide_an_unretired_live_scaler() {
    setup([0,7],false,true);
    assert!(detach_ui4_scalers(NATIVE));
    HW.with(|h| { let h=h.borrow();for address in CTRL {assert_eq!(h.live[&address],0);} });
}
#[test]
fn missing_frame_or_ignored_control_write_cannot_publish_success() {
    setup([1,6],true,true);
    HW.with(|h|h.borrow_mut().advance=false);
    assert!(!detach_ui4_scalers(NATIVE));
    HW.with(|h|assert_ne!(h.borrow().live[&CTRL[1]]&(1<<31),0));
    setup([1,6],true,true);
    HW.with(|h|h.borrow_mut().ignore_ctrl=true);
    assert!(!detach_ui4_scalers(NATIVE));
}
#[test]
fn desktop_or_unadopted_panel_never_reads_or_writes_scalers() {
    for (dev,handoff) in [(DESKTOP,true),(NATIVE,false)] {
        setup([0,7],true,true);HW.with(|h|h.borrow_mut().handoff=handoff);
        assert!(!detach_ui4_scalers(dev));
        HW.with(|h| {let h=h.borrow();assert_eq!(h.reads,0);assert!(h.writes.is_empty());assert_eq!(h.frames,0);});
    }
}
}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-tgl-scaler-") as directory:
        code = Path(directory) / "tests.rs"
        binary = Path(directory) / "tests"
        code.write_text(source)
        subprocess.run(["rustc", "--edition=2024", "--test", str(code), "-o", str(binary)], cwd=ROOT, check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
