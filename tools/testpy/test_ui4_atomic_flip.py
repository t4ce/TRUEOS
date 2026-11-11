#!/usr/bin/env python3
"""Exercise the production native display atomic guard with staged MMIO.

Every write can cross a simulated frame boundary. Armed plane/scaler updates
must remain staged while DOUBLE_BUFFER_CTL holds the stall, then latch together
after the real guard releases it. No alternate guard implementation is used.
"""

from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant

ROOT = Path(__file__).resolve().parents[2]
MODULE = ROOT / "src/intel/display/atomic_flip.rs"
REGISTERS = "src/intel/display/regs.rs"


def production_constants() -> str:
    register_source = (ROOT / REGISTERS).read_text()
    available = set(re.findall(r"^(?:pub(?:\([^)]*\))?\s+)?const\s+(\w+)",
                               register_source, re.MULTILINE))
    needed = (set(re.findall(r"\b[A-Z][A-Z0-9_]+\b", MODULE.read_text())) & available) - {"PIPES"}
    declarations: dict[str, str] = {}
    while needed:
        name = needed.pop()
        if name in declarations:
            continue
        declaration = constant(REGISTERS, name)
        declarations[name] = re.sub(r"pub\([^)]*\)\s+", "", declaration)
        needed.update((set(re.findall(r"\b[A-Z][A-Z0-9_]+\b", declaration)) & available)
                      - declarations.keys())
    return "\n".join(declarations.values())


def harness_source() -> str:
    source = r'''
#![allow(dead_code)]
use std::cell::RefCell;
use std::collections::BTreeMap;

// Independent register addresses and bit definitions from the hardware contract.
const GLOBAL: usize = 0x44500;
const GLOBAL_STALL: u32 = 1;
const FRAME_COUNT: usize = 0x70040;
const SCALER_CTL: [usize; 2] = [0x68180, 0x68280];
const SCALER_SIZE: [usize; 2] = [0x68174, 0x68274];
const SCALER_ALLOW: u32 = 1 << 9;
const PLANE_POS: [usize; 2] = [0x7018C, 0x7028C];
const PLANE_SURF: [usize; 2] = [0x7019C, 0x7029C];
const PLANE_ALPHA: usize = 0x701A0;
const ORIGINAL_GLOBAL: u32 = 0xA5A5_0010;
const ORIGINAL_SCALERS: [u32; 2] = [0x8200_0020, 0x8E00_0205];

#[derive(Default)]
struct Hardware {
    native: bool,
    staged: BTreeMap<usize, u32>,
    live: BTreeMap<usize, u32>,
    reads: Vec<usize>,
    writes: Vec<(usize, u32)>,
    plane_armed: [bool; 2],
    scaler_armed: [bool; 2],
    plane_latch_frame: [Option<u64>; 2],
    scaler_latch_frame: [Option<u64>; 2],
    frame: u64,
    ignore_stall: bool,
    auto_tick: bool,
}

impl Hardware {
    fn tick(&mut self) {
        self.frame += 1;
        if self.staged[&GLOBAL] & GLOBAL_STALL != 0 { return; }
        for i in 0..2 {
            if self.plane_armed[i] {
                for address in [PLANE_POS[i], PLANE_SURF[i]] {
                    self.live.insert(address, self.staged[&address]);
                }
                if i == 0 { self.live.insert(PLANE_ALPHA, self.staged[&PLANE_ALPHA]); }
                self.plane_latch_frame[i] = Some(self.frame);
                self.plane_armed[i] = false;
            }
            if self.scaler_armed[i] {
                for address in [SCALER_CTL[i], SCALER_SIZE[i]] {
                    self.live.insert(address, self.staged[&address]);
                }
                self.scaler_latch_frame[i] = Some(self.frame);
                self.scaler_armed[i] = false;
            }
        }
    }
}

thread_local! { static HW: RefCell<Hardware> = RefCell::new(Hardware::default()); }

mod intel {
    #[derive(Clone, Copy)]
    pub struct Dev;
    pub fn mmio_read(_: Dev, address: usize) -> u32 {
        super::HW.with(|h| {
            let mut h = h.borrow_mut(); h.reads.push(address);
            if address == super::FRAME_COUNT { return h.frame as u32; }
            *h.staged.get(&address).unwrap_or(&0)
        })
    }
    pub fn mmio_write(_: Dev, address: usize, value: u32) {
        super::HW.with(|h| {
            let mut h = h.borrow_mut();
            h.writes.push((address, value));
            if address == super::GLOBAL && value & super::GLOBAL_STALL != 0 {
                for control in super::SCALER_CTL {
                    assert_ne!(h.staged[&control] & super::SCALER_ALLOW, 0,
                        "both scaler permissions must precede the global stall");
                }
            }
            if !(h.ignore_stall && address == super::GLOBAL
                 && value & super::GLOBAL_STALL != 0) {
                h.staged.insert(address, value);
            }
            for i in 0..2 {
                if address == super::PLANE_SURF[i] { h.plane_armed[i] = true; }
                if address == super::SCALER_SIZE[i] { h.scaler_armed[i] = true; }
            }
            if h.auto_tick { h.tick(); }
        });
    }
}

mod display {
    use super::*;
    #[derive(Clone, Copy)] struct PipeInfo;
    const PIPES: [PipeInfo; 4] = [PipeInfo; 4];
    fn native_ui4_for_pipe(_: PipeInfo) -> bool { HW.with(|h| h.borrow().native) }
'''
    source += production_constants()
    source += f'\n#[path = "{MODULE.as_posix()}"] mod atomic_flip;\n'
    source += r'''
    fn begin() -> Result<Option<atomic_flip::Guard>, &'static str> {
        atomic_flip::begin(crate::intel::Dev)
    }
fn setup(native: bool) {
    HW.with(|h| {
        let mut value = Hardware { native, auto_tick: true, ..Hardware::default() };
        value.staged.insert(GLOBAL, ORIGINAL_GLOBAL);
        value.staged.insert(PLANE_ALPHA, 0xFF);
        value.live.insert(PLANE_ALPHA, 0xFF);
        for i in 0..2 {
            value.staged.insert(SCALER_CTL[i], ORIGINAL_SCALERS[i]);
            value.staged.insert(SCALER_SIZE[i], 0x0010_0020);
            value.live.insert(SCALER_CTL[i], ORIGINAL_SCALERS[i]);
            value.live.insert(SCALER_SIZE[i], 0x0010_0020);
            value.staged.insert(PLANE_POS[i], 0);
            value.staged.insert(PLANE_SURF[i], 0x1000 + i as u32 * 0x1000);
            value.live.insert(PLANE_POS[i], 0);
            value.live.insert(PLANE_SURF[i], 0x1000 + i as u32 * 0x1000);
        }
        *h.borrow_mut() = value;
    });
}

#[test]
fn non_native_outputs_do_not_read_or_write_mmio() {
    setup(false);
    assert!(display::begin().unwrap().is_none());
    HW.with(|h| { let h = h.borrow(); assert!(h.reads.is_empty()); assert!(h.writes.is_empty()); });
}

#[test]
fn existing_stall_is_never_stolen_or_released() {
    setup(true);
    HW.with(|h| { h.borrow_mut().staged.insert(GLOBAL, ORIGINAL_GLOBAL | GLOBAL_STALL); });
    assert!(display::begin().is_err());
    HW.with(|h| {
        let h = h.borrow(); assert!(h.writes.is_empty());
        assert_eq!(h.staged[&GLOBAL], ORIGINAL_GLOBAL | GLOBAL_STALL);
        for i in 0..2 { assert_eq!(h.staged[&SCALER_CTL[i]], ORIGINAL_SCALERS[i]); }
    });
}

#[test]
fn scaler_permissions_preserve_other_bits_and_precede_the_global_stall() {
    setup(true);
    let guard = display::begin().unwrap().expect("native guard");
    HW.with(|h| {
        let h = h.borrow();
        for i in 0..2 {
            assert_eq!(h.staged[&SCALER_CTL[i]], ORIGINAL_SCALERS[i] | SCALER_ALLOW);
        }
        let global_write = h.writes.iter().position(|&(a, _)| a == GLOBAL).unwrap();
        assert!(h.writes[..global_write].iter().any(|&(a, _)| a == SCALER_CTL[0]));
        assert_eq!(h.staged[&GLOBAL], ORIGINAL_GLOBAL | GLOBAL_STALL);
    });
    drop(guard);
    HW.with(|h| { let h = h.borrow(); assert_eq!(h.staged[&GLOBAL], ORIGINAL_GLOBAL); });
}

#[test]
fn failed_stall_readback_restores_the_prior_global_word() {
    setup(true);
    HW.with(|h| h.borrow_mut().ignore_stall = true);
    assert!(display::begin().is_err());
    HW.with(|h| {
        let h = h.borrow();
        assert_eq!(h.staged[&GLOBAL], ORIGINAL_GLOBAL);
        let writes: Vec<_> = h.writes.iter().copied().filter(|&(a, _)| a == GLOBAL).collect();
        assert_eq!(writes, [(GLOBAL, ORIGINAL_GLOBAL | GLOBAL_STALL), (GLOBAL, ORIGINAL_GLOBAL)]);
    });
}

#[test]
fn normal_drop_posts_the_restoration_and_keeps_unowned_bits() {
    setup(true);
    let guard = display::begin().unwrap().unwrap();
    drop(guard);
    HW.with(|h| {
        let h = h.borrow();
        assert_eq!(h.writes.last(), Some(&(GLOBAL, ORIGINAL_GLOBAL)));
        assert_eq!(h.reads.last(), Some(&GLOBAL), "restoration requires a posting read");
        assert_eq!(h.staged[&GLOBAL] & !GLOBAL_STALL, ORIGINAL_GLOBAL);
    });
}

#[test]
fn early_return_releases_the_guard() {
    fn interrupted() -> Result<(), &'static str> {
        let _guard = display::begin()?;
        crate::intel::mmio_write(crate::intel::Dev, PLANE_POS[0], 0x0070_0080);
        Err("later-validation-failed")
    }
    setup(true);
    assert_eq!(interrupted(), Err("later-validation-failed"));
    HW.with(|h| { let h = h.borrow(); assert_eq!(h.staged[&GLOBAL], ORIGINAL_GLOBAL); });
}

#[test]
fn armed_planes_and_scaler_latch_together_after_guard_release() {
    setup(true);
    let guard = display::begin().unwrap().unwrap();
    for i in 0..2 {
        crate::intel::mmio_write(crate::intel::Dev, PLANE_POS[i], 0x0200_0300 + i as u32);
    }
    crate::intel::mmio_write(crate::intel::Dev, SCALER_CTL[0], 0x8400_0200);
    crate::intel::mmio_write(crate::intel::Dev, SCALER_SIZE[0], 0x0400_0500);
    for i in 0..2 {
        crate::intel::mmio_write(crate::intel::Dev, PLANE_SURF[i], 0x4000 + i as u32 * 0x1000);
    }
    HW.with(|h| {
        let h = h.borrow();
        assert_eq!(h.plane_latch_frame, [None, None]);
        assert_eq!(h.scaler_latch_frame, [None, None]);
        for i in 0..2 { assert_eq!(h.live[&PLANE_SURF[i]], 0x1000 + i as u32 * 0x1000); }
    });
    drop(guard);
    HW.with(|h| {
        let h = h.borrow();
        assert_eq!(h.plane_latch_frame[0], h.plane_latch_frame[1]);
        assert_eq!(h.plane_latch_frame[0], h.scaler_latch_frame[0]);
        assert!(h.plane_latch_frame[0].is_some());
        for i in 0..2 { assert_eq!(h.live[&PLANE_SURF[i]], 0x4000 + i as u32 * 0x1000); }
        assert_eq!(h.live[&SCALER_SIZE[0]], 0x0400_0500);
    });
}

#[test]
fn release_records_its_frame_baseline_after_restoring_the_global_hold() {
    setup(true);
    let guard = display::begin().unwrap().unwrap();
    let frame_while_held = HW.with(|h| h.borrow().frame as u32);
    let baseline = guard.release();
    HW.with(|h| {
        let h = h.borrow();
        assert_eq!(h.staged[&GLOBAL], ORIGINAL_GLOBAL);
        // The restoration write itself crosses a modeled frame boundary.
        // A baseline captured before Drop would therefore be one frame old.
        assert_eq!(baseline, frame_while_held.wrapping_add(1));
        assert_eq!(baseline, h.frame as u32);
        assert_eq!(&h.reads[h.reads.len() - 2..], &[GLOBAL, FRAME_COUNT]);
    });
    assert!(!atomic_flip::frame_advanced(Some(baseline), baseline));
    assert!(atomic_flip::frame_advanced(Some(baseline), baseline.wrapping_add(1)));
    assert!(atomic_flip::frame_advanced(None, baseline));
}

#[test]
fn unchanged_surface_and_new_staged_alpha_cannot_retire_until_frame_wraps() {
    setup(true);
    HW.with(|h| {
        let mut h = h.borrow_mut();
        h.auto_tick = false;
        h.frame = u32::MAX as u64;
        h.staged.insert(PLANE_ALPHA, 0xFF);
        h.live.insert(PLANE_ALPHA, 0xFF);
    });
    let guard = display::begin().unwrap().unwrap();
    crate::intel::mmio_write(crate::intel::Dev, PLANE_ALPHA, 0x44);
    crate::intel::mmio_write(crate::intel::Dev, PLANE_SURF[0], 0x1000);
    let baseline = guard.release();
    assert_eq!(baseline, u32::MAX);
    // SURFLIVE is already equal for a same-address update, while alpha reads
    // the newly staged value. Those observations alone would falsely retire.
    let old_surface_and_staged_alpha_match = HW.with(|h| {
        let h = h.borrow();
        assert_eq!(h.live[&PLANE_ALPHA], 0xFF, "opacity has not retired");
        h.live[&PLANE_SURF[0]] == 0x1000 && h.staged[&PLANE_ALPHA] == 0x44
    });
    assert!(old_surface_and_staged_alpha_match);
    assert!(!(old_surface_and_staged_alpha_match
        && atomic_flip::frame_advanced(Some(baseline),
            crate::intel::mmio_read(crate::intel::Dev, FRAME_COUNT))));
    HW.with(|h| h.borrow_mut().tick());
    let current = crate::intel::mmio_read(crate::intel::Dev, FRAME_COUNT);
    assert_eq!(current, 0, "frame counter wrap is a real advance");
    HW.with(|h| assert_eq!(h.borrow().live[&PLANE_ALPHA], 0x44));
    assert!(old_surface_and_staged_alpha_match
        && atomic_flip::frame_advanced(Some(baseline), current));
}
}
'''
    return source


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-ui4-atomic-") as directory:
        code = Path(directory) / "tests.rs"
        binary = Path(directory) / "tests"
        code.write_text(harness_source())
        subprocess.run(["rustc", "--edition=2024", "--test", str(code), "-o", str(binary)],
                       cwd=ROOT, check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
