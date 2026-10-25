#!/usr/bin/env python3
"""Run production WD scene reflection against a recorded MMIO register bank."""
from pathlib import Path
import re
import subprocess
import tempfile

import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
MIRROR = "src/intel/display/mirror_map_dp_engine.rs"
DISPLAY = "src/intel/display.rs"
REGS = "src/intel/display/regs.rs"


def main():
    extract.ROOT = ROOT
    source = r'''#![allow(dead_code)]
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU8, Ordering};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
static UI4_PLANE_SURFACE_FLIP_BATCH: Mutex<()> = Mutex::new(());
static BANK: Mutex<BTreeMap<usize,u32>> = Mutex::new(BTreeMap::new());
static WRITES: Mutex<Vec<(usize,u32)>> = Mutex::new(Vec::new());
static MODE: AtomicU8 = AtomicU8::new(0);
mod intel {
    #[derive(Clone,Copy)] pub struct Dev;
    pub fn mmio_read(_: Dev, reg: usize) -> u32 {
        // Application source reads must hold the same lock as submission.
        if (0x70180..0x70880).contains(&reg) || (0x68160..0x68300).contains(&reg) {
            assert!(super::UI4_PLANE_SURFACE_FLIP_BATCH.0.try_lock().is_err());
        }
        *super::BANK.lock().get(&reg).unwrap_or(&0)
    }
    pub fn mmio_write(_: Dev, reg: usize, value: u32) {
        assert!(super::UI4_PLANE_SURFACE_FLIP_BATCH.0.try_lock().is_ok(),
            "release source lock before programming the independent mirror");
        assert!(!(0x70000..0x71000).contains(&reg), "never write Pipe A");
        assert!(!(0x68160..0x68300).contains(&reg), "never write source scalers");
        super::WRITES.lock().push((reg,value)); super::BANK.lock().insert(reg,value);
    }
}
#[derive(Clone,Copy)] struct PipeInfo { slot:usize }
#[derive(Clone,Copy)] struct Plane { base:usize }
impl PipeInfo { fn plane(self, slot:usize) -> Plane {
    Plane { base:UNI_PLANE_BASE+self.slot*UNI_PLANE_PIPE_STRIDE+slot*UNI_PLANE_SLOT_STRIDE }
} }
impl Plane {
    fn base(self)->usize {self.base}
    fn ctl(self)->usize {self.base+UNI_PLANE_CTL_OFF}
    fn surf(self)->usize {self.base+UNI_PLANE_SURF_OFF}
}
const PIPES:[PipeInfo;3]=[PipeInfo{slot:0},PipeInfo{slot:1},PipeInfo{slot:2}];
const UNIVERSAL_PLANE_SLOTS:usize=7;
fn mirror_cursor(_:intel::Dev) {}
fn program_plane_watermark_boot_safe(dev:intel::Dev, base:usize, _:bool) {
    intel::mmio_write(dev,base+UNI_PLANE_WM_0_OFF,0x8000_0001);
}
fn program_plane_buf_cfg(dev:intel::Dev,base:usize,start:u16,end:u16) {
    intel::mmio_write(dev,base+UNI_PLANE_BUF_CFG_OFF,(u32::from(end)<<16)|u32::from(start));
}
fn mirror_map_mode()->MirrorMapMode {MirrorMapMode::from_raw(MODE.load(Ordering::Relaxed))}
'''
    mirror_constants = (
        "SOURCE_PIPE_SLOT", "MIRROR_PIPE_SLOT", "MIRROR_PLANE_COUNT",
        "MIRROR_SCALER_COUNT", "MIRROR_DBUF_RANGES", "MIRROR_PLANE_REGISTERS",
        "CURSOR_A_BASE", "CURSOR_PIPE_STRIDE", "CURSOR_CTL_OFF", "CURSOR_BASE_OFF",
    )
    # Pull the complete register vocabulary from the production mirror/helper
    # declarations, rather than encoding a second address map in the test.
    names = set(re.findall(r'\b(?:UNI_PLANE_[A-Z0-9_]+|PLANE_DBUF_S2_SLOT_[0-4]_(?:START|END)|'
                           r'PIPE_SCALER_[A-Z0-9_]+|SKL_BOTTOM_COLOR_[A-Z_]+|PLANE_CTL_ENABLE)\b',
                           (ROOT / MIRROR).read_text()))
    names |= {
        "UNI_PLANE_BASE", "UNI_PLANE_PIPE_STRIDE", "UNI_PLANE_SLOT_STRIDE",
        "PIPE_SCALER_0_A_CTRL", "PIPE_SCALER_1_A_CTRL", "PIPE_SCALER_0_B_CTRL",
        "PIPE_SCALER_1_B_CTRL", "PIPE_SCALER_0_C_CTRL", "PIPE_SCALER_1_C_CTRL",
        "PIPE_SCALER_WIN_POS_FROM_CTRL", "PIPE_SCALER_WIN_SIZE_FROM_CTRL",
        "PIPE_SCALER_HPHASE_FROM_CTRL", "PIPE_SCALER_VPHASE_FROM_CTRL",
        "UNI_PLANE_WM_0_OFF", "UNI_PLANE_BUF_CFG_OFF", "UNI_PLANE_SURF_OFF",
        "UNI_PLANE_SURFLIVE_OFF",
    }
    declarations = {}
    while names:
        name = names.pop()
        if name in declarations:
            continue
        path = "src/intel/mod.rs" if name == "SPIRIT_CURSOR_DBUF_S1_START" else REGS
        declaration = extract.constant(path, name)
        declarations[name] = declaration
        names.update(set(re.findall(r'\b[A-Z][A-Z0-9_]+\b', declaration.split('=', 1)[1]))
                     - declarations.keys())
    for declaration in declarations.values():
        source += re.sub(r'pub\([^)]*\)\s+', '', declaration).replace('crate::intel::', '') + '\n'
    for name in mirror_constants:
        source += extract.constant(MIRROR, name) + '\n'
    source += extract.item(MIRROR, "MirrorMapMode")
    source += re.search(r'^impl MirrorMapMode \{.*?^}\n',
                        (ROOT / MIRROR).read_text(), re.M | re.S).group()
    for name in ("PipeScalerRegisters", "pipe_scaler_registers", "pipe_scaler_binding"):
        source += extract.item(DISPLAY, name) + '\n'
    for name in ("MirrorPlaneState", "MirrorScalerState", "MirrorScene", "read_mirror_scene",
                 "program_mirror_scaler", "mirror_live_scene", "initialize_mirror_planes",
                 "disable_mirror_planes"):
        source += extract.item(MIRROR, name) + '\n'
    source += r'''
fn put(reg:usize,value:u32) {BANK.lock().insert(reg,value);}
fn get(reg:usize)->u32 {*BANK.lock().get(&reg).unwrap_or(&0)}
fn reset() {BANK.lock().clear();WRITES.lock().clear();MODE.store(0,Ordering::Relaxed);}
fn plane(slot:usize) {
    let p=PIPES[0].plane(slot).base();
    put(p+UNI_PLANE_CTL_OFF,PLANE_CTL_ENABLE|0x0400_0000);
    put(p+UNI_PLANE_STRIDE_OFF,0x100+slot as u32);
    put(p+UNI_PLANE_SIZE_OFF,(564<<16)|564);
    put(p+UNI_PLANE_KEYMSK_OFF,0x8000_0000);
    put(p+UNI_PLANE_KEYMAX_OFF,(100+slot as u32)<<24);
    put(p+UNI_PLANE_INPUT_CSC_COEFF_OFF,0x1234_0000|slot as u32);
    put(p+UNI_PLANE_SURFLIVE_OFF,0x1000_0000+slot as u32*0x100000);
    put(p+UNI_PLANE_SURF_OFF,0x1000_0000+slot as u32*0x100000);
}
fn scaler(id:usize,slot:usize,x:u32,y:u32) {
    let r=pipe_scaler_registers(0,id).unwrap();
    put(r.control,PIPE_SCALER_ENABLE|pipe_scaler_binding(slot));
    put(r.window_pos,(x<<16)|y);put(r.window_size,(600<<16)|600);
    put(r.hphase,0x4001_4001);put(r.vphase,0x2001_2001);
}
#[test]
fn scaled_window_and_second_scaler_keep_their_real_destination() {
    reset();for slot in 0..5 {plane(slot);}
    // PLANE_POS is zero while PS_WIN_POS carries placement during animation.
    scaler(0,1,980,410);scaler(1,2,1500,650);
    mirror_live_scene(intel::Dev);
    for (id,slot,x,y) in [(0,1,980,410),(1,2,1500,650)] {
        let r=pipe_scaler_registers(2,id).unwrap();
        assert_eq!(get(r.control),PIPE_SCALER_ENABLE|pipe_scaler_binding(slot));
        assert_eq!(get(r.window_pos),(x<<16)|y);
        assert_eq!(get(r.window_size),(600<<16)|600);
        assert_eq!(get(r.hphase),0x4001_4001);assert_eq!(get(r.vphase),0x2001_2001);
        let p=PIPES[2].plane(slot).base();
        assert_eq!(get(p+UNI_PLANE_POS_OFF),0);
        assert_eq!(get(p+UNI_PLANE_KEYMAX_OFF),(100+slot as u32)<<24);
        assert_eq!(get(p+UNI_PLANE_INPUT_CSC_COEFF_OFF),0x1234_0000|slot as u32);
        assert_eq!(get(p+UNI_PLANE_SURF_OFF),0x1000_0000+slot as u32*0x100000);
    }
    let writes=WRITES.lock();
    let first_surf=writes.iter().position(|(reg,_)|
        (0..5).any(|slot|*reg==PIPES[2].plane(slot).surf())).unwrap();
    for id in 0..2 {
        let r=pipe_scaler_registers(2,id).unwrap();
        let control=writes.iter().position(|(reg,_)|*reg==r.control).unwrap();
        let pos=writes.iter().position(|(reg,_)|*reg==r.window_pos).unwrap();
        let arm=writes.iter().position(|(reg,_)|*reg==r.window_size).unwrap();
        assert!(control<pos && pos<arm && arm<first_surf);
    }
}
#[test]
fn plane_permutations_remap_scaler_bindings_without_moving_the_window() {
    for mode in [MirrorMapMode::Identity,MirrorMapMode::Swap1And3,MirrorMapMode::Swap2And3] {
        reset();MODE.store(mode as u8,Ordering::Relaxed);
        for slot in 0..5 {plane(slot);}
        scaler(0,3,1200,700);scaler(1,2,500,400);
        mirror_live_scene(intel::Dev);
        for (id,src,x,y) in [(0,3,1200,700),(1,2,500,400)] {
            let dst=mode.destination_for_source(src).unwrap();
            let r=pipe_scaler_registers(2,id).unwrap();
            assert_eq!(get(r.control),PIPE_SCALER_ENABLE|pipe_scaler_binding(dst));
            assert_eq!(get(r.window_pos),(x<<16)|y);
            assert_eq!(get(PIPES[2].plane(dst).surf()),0x1000_0000+src as u32*0x100000);
        }
    }
}
#[test]
fn animation_end_and_capture_teardown_clear_both_stale_scalers() {
    reset();plane(1);plane(2);scaler(0,1,980,410);scaler(1,2,1500,650);
    mirror_live_scene(intel::Dev);
    for id in 0..2 {put(pipe_scaler_registers(0,id).unwrap().control,0);}
    put(PIPES[0].plane(1).base()+UNI_PLANE_POS_OFF,(410<<16)|980);
    mirror_live_scene(intel::Dev);
    for id in 0..2 {
        let r=pipe_scaler_registers(2,id).unwrap();
        assert_eq!(get(r.control),0);assert_eq!(get(r.window_size),0);
    }
    assert_eq!(get(PIPES[2].plane(1).base()+UNI_PLANE_POS_OFF),(410<<16)|980);
    scaler(0,1,980,410);scaler(1,2,1500,650);mirror_live_scene(intel::Dev);
    disable_mirror_planes(intel::Dev);
    for id in 0..2 {
        let r=pipe_scaler_registers(2,id).unwrap();
        assert_eq!(get(r.control),0);assert_eq!(get(r.window_size),0);
    }
}
#[test]
fn selection_reassignment_keeps_geometry_and_surface_in_the_same_commit() {
    reset();for slot in 0..5 {plane(slot);}
    let src=PIPES[0].plane(1).base();
    let dst=PIPES[2].plane(1).base();
    // A focus change repurposes the old direct slot as a full-output composed
    // plane at the origin. Its previous window is still in SURFLIVE until
    // vblank. Mirroring that window with the new geometry creates the jump.
    put(src+UNI_PLANE_POS_OFF,0);
    put(src+UNI_PLANE_SIZE_OFF,(1439<<16)|2559);
    put(src+UNI_PLANE_SURF_OFF,0x3000_0000);
    mirror_live_scene(intel::Dev);
    assert_eq!(get(dst+UNI_PLANE_SURF_OFF),0x3000_0000,
        "new origin geometry must use its ready surface, never the old window in SURFLIVE");
    assert_eq!(get(dst+UNI_PLANE_SIZE_OFF),(1439<<16)|2559);
    // Once the physical pipe catches up, the same contract remains valid.
    put(src+UNI_PLANE_SURFLIVE_OFF,0x3000_0000);
    mirror_live_scene(intel::Dev);
    assert_eq!(get(dst+UNI_PLANE_SURF_OFF),0x3000_0000);
    assert_eq!(get(dst+UNI_PLANE_SIZE_OFF),(1439<<16)|2559);
}
#[test]
fn static_fetch_configuration_is_not_reprogrammed_on_each_capture() {
    reset();initialize_mirror_planes(intel::Dev);WRITES.lock().clear();
    plane(1);scaler(0,1,980,410);mirror_live_scene(intel::Dev);
    for slot in 0..5 {
        let p=PIPES[2].plane(slot).base();
        assert!(!WRITES.lock().iter().any(|(reg,_)|
            *reg==p+UNI_PLANE_WM_0_OFF || *reg==p+UNI_PLANE_BUF_CFG_OFF));
    }
}
#[test]
fn pipe_scaling_is_not_mistaken_for_plane_zero_binding() {
    reset();scaler(0,0,0,0);
    let src=pipe_scaler_registers(0,0).unwrap();put(src.control,PIPE_SCALER_ENABLE);
    let scene=read_mirror_scene(intel::Dev,MirrorMapMode::Swap1And3);
    assert_eq!(scene.scalers[0].control,PIPE_SCALER_ENABLE);
    // An unmirrored hardware plane must not leave a stale destination binding.
    put(src.control,PIPE_SCALER_ENABLE|pipe_scaler_binding(6));
    assert_eq!(read_mirror_scene(intel::Dev,MirrorMapMode::Identity).scalers[0].control,0);
}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-wd-mirror-") as tmp:
        path = Path(tmp)
        (path / "tests.rs").write_text(source)
        subprocess.run(["rustc", "--edition=2024", "--test", str(path / "tests.rs"),
                        "-o", str(path / "tests")], check=True, cwd=ROOT)
        subprocess.run([str(path / "tests"), "--test-threads=1"], check=True, cwd=ROOT)


if __name__ == "__main__":
    main()
