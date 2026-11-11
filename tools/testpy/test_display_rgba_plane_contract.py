#!/usr/bin/env python3
"""Exercise production RGBA plane ownership, programming and live guards.

Only MMIO storage, plane addressing and unrelated output readiness are fixtures.
CTL construction, geometry/auxiliary programming, rearm decisions and contract
guards execute unchanged. NV12's separate production builders remain covered.
"""

from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item
from test_vgpu_batch_overlay import block


ROOT = Path(__file__).resolve().parents[2]
DISPLAY = "src/intel/display.rs"
REGS = "src/intel/display/regs.rs"
PROBES = "src/intel/display/display_probes.rs"


def harness() -> str:
    declarations = [item(DISPLAY, name) for name in (
        "PrimaryPlaneSourceFormat", "OverlayAlphaMode", "primary_plane_ctl_enabled",
        "primary_plane_ctl_enabled_for_format", "overlay_plane_ctl_enabled", "plane_color_ctl_alpha",
        "program_rgba8_plane_static_contract", "primary_plane_contract_regs_match",
        "overlay_plane_dynamic_flip_guard", "overlay_plane_needs_rearm",
        "plane_keymax_alpha", "plane_keymsk_alpha", "overlay_plane_constant_alpha_matches",
        "overlay_plane_constant_alpha_is_valid", "plane_watermarks_match", "plane_buf_cfg_value",
    )]
    declarations.extend(item(REGS, name) for name in (
        "plane_pos_reg_value", "plane_size_reg_value", "plane_stride_reg_value",
    ))
    declarations.extend(item(PROBES, name) for name in (
        "DirectNv12PlaneTiling", "direct_nv12_plane_ctl_enabled", "direct_nv12_y_plane_ctl_enabled",
    ))
    declarations.append(block((ROOT / PROBES).read_text(), "impl DirectNv12PlaneTiling"))
    pending = set(re.findall(r"\b[A-Z][A-Z0-9_]+\b", "\n".join(declarations)))
    constants = {}
    while pending:
        name = pending.pop()
        if name in constants:
            continue
        for path in (REGS, DISPLAY):
            try:
                declaration = constant(path, name)
                break
            except ValueError:
                declaration = None
        if declaration is None:
            continue
        constants[name] = declaration
        pending.update(set(re.findall(r"\b[A-Z][A-Z0-9_]+\b", declaration)) - constants.keys())
    return r'''
#![allow(dead_code)]
use std::{cell::RefCell,collections::BTreeMap};
#[derive(Default)] struct Hardware {registers:BTreeMap<usize,u32>,writes:Vec<(usize,u32)>}
thread_local! {static HW:RefCell<Hardware>=RefCell::new(Hardware::default());}
mod ui4 {pub const PRIMARY_PLANE_SLOT:usize=0;}
mod intel {
 #[derive(Clone,Copy)] pub struct Dev;
 pub fn mmio_read(_:Dev,address:usize)->u32 {
  crate::HW.with(|h|*h.borrow().registers.get(&address).unwrap_or(&0))
 }
 pub fn mmio_write(_:Dev,address:usize,value:u32) {
  crate::HW.with(|h|{let mut h=h.borrow_mut();h.registers.insert(address,value);h.writes.push((address,value));});
 }
 mod display {
''' + "\n".join(constants.values()) + "\n" + "\n".join(declarations) + r'''
#[derive(Clone,Copy)] struct PipeInfo {slot:usize}
const PIPE:PipeInfo=PipeInfo{slot:0};
#[derive(Clone,Copy)] struct PlaneId(usize);
impl PipeInfo {
 fn plane(self,slot:usize)->PlaneId {PlaneId(slot)}
 fn primary_plane(self)->PlaneId {self.plane(0)}
}
impl PlaneId {
 fn base(self)->usize {0x70180+self.0*0x100}
 fn ctl(self)->usize {self.base()+UNI_PLANE_CTL_OFF}
 fn stride(self)->usize {self.base()+UNI_PLANE_STRIDE_OFF}
}
#[derive(Clone,Copy)] struct PrimarySurface {width:u32,height:u32,pitch_bytes:u32,pipe:PipeInfo}
#[derive(Clone,Copy)] struct OverlaySurface {
 width:u32,height:u32,pitch_bytes:u32,pipe:PipeInfo,plane_slot:usize,gpu:u64,
}
mod screenlog {pub fn owns_plane(_:usize,_:usize)->bool {false}}
// The guard's output routing and readiness boundaries do not sanitize CTLs.
fn plane_slot_count(_:PipeInfo)->usize {7}
fn ui4_rgba8_plane_stack_ready(_:PipeInfo)->bool {true}
fn overlay_plane_base(pipe:PipeInfo,slot:usize)->usize {pipe.plane(slot).base()}
fn plane_dbuf_range(_:PipeInfo,slot:usize)->Option<(u16,u16)> {
 Some((slot as u16*100,slot as u16*100+99))
}
fn plane_watermark(_:PipeInfo,_:usize)->Option<u32> {Some(0x800200f9)}
fn read(address:usize)->u32 {crate::intel::mmio_read(super::Dev,address)}
fn write(address:usize,value:u32) {crate::intel::mmio_write(super::Dev,address,value)}
fn reset() {crate::HW.with(|h|*h.borrow_mut()=crate::Hardware::default());}
const UNWANTED:[u32;6]=[1<<23,1<<15,1<<4,1<<8,1<<1,1];
fn firmware_bits(pattern:usize)->u32 {
 UNWANTED.iter().enumerate().filter(|(index,_)|pattern&(1<<index)!=0)
  .fold(0,|bits,(_,bit)|bits|bit)
}
fn ready_registers()->(PrimarySurface,OverlaySurface) {
 reset();
 for slot in 0..7 {
  let pitch=if slot==0 {15616}else{15360};
  let ctl=program_rgba8_plane_static_contract(super::Dev,PIPE,slot,3840,2160,pitch).unwrap();
  let base=PIPE.plane(slot).base();write(base+UNI_PLANE_CTL_OFF,ctl);
  write(base+UNI_PLANE_WM_0_OFF,0x800200f9);
  let (start,end)=plane_dbuf_range(PIPE,slot).unwrap();
  write(base+UNI_PLANE_BUF_CFG_OFF,plane_buf_cfg_value(start,end));
  write(base+UNI_PLANE_SURF_OFF,0xec000000);write(base+UNI_PLANE_SURFLIVE_OFF,0xec000000);
 }
 (PrimarySurface{width:3840,height:2160,pitch_bytes:15616,pipe:PIPE},
  OverlaySurface{width:3840,height:2160,pitch_bytes:15360,pipe:PIPE,plane_slot:4,gpu:0xec000000})
}

#[test] fn canonical_rgb_words_and_unowned_documented_fields_remain_unchanged() {
 for (format,expected) in [(PrimaryPlaneSourceFormat::Xrgb8888,0x94000000),
  (PrimaryPlaneSourceFormat::Xbgr8888,0x94100000),
  (PrimaryPlaneSourceFormat::Rgba8888Premultiplied,0x94100000)] {
  for preserved in [0,8,1<<9,1<<13,1<<14,(1<<9)|(1<<13)|(1<<14)|8] {
   assert_eq!(primary_plane_ctl_enabled_for_format(expected|preserved,format),expected|preserved);
  }
 }
 assert_eq!(overlay_plane_ctl_enabled(0x94100008,OverlayAlphaMode::PremultipliedRgba),0x94100008);
 assert_eq!(overlay_plane_ctl_enabled(0x94000008,OverlayAlphaMode::Opaque),0x94000008);
}

#[test] fn all_inherited_fetch_mode_combinations_normalize_for_every_rgb_format() {
 for pattern in 0..64 {
  let dirty=firmware_bits(pattern);
  for (format,expected) in [(PrimaryPlaneSourceFormat::Xrgb8888,0x94000008),
   (PrimaryPlaneSourceFormat::Xbgr8888,0x94100008),
   (PrimaryPlaneSourceFormat::Rgba8888Premultiplied,0x94100008)] {
   assert_eq!(primary_plane_ctl_enabled_for_format(expected|dirty,format),expected,"pattern={pattern:#x}");
  }
  for (alpha,expected) in [(OverlayAlphaMode::Opaque,0x94000008),
   (OverlayAlphaMode::PremultipliedRgba,0x94100008)] {
   assert_eq!(overlay_plane_ctl_enabled(expected|dirty,alpha),expected,"pattern={pattern:#x}");
  }
 }
}

#[test] fn static_programmer_clears_auxiliary_state_and_uses_actual_surface_pitch() {
 for (slot,width,height,pitch,expected_stride,expected_size,expected_ctl) in [
  (0,3840,2160,15616,244,0x086f0eff,0x94000008),
  (4,3840,2160,15360,240,0x086f0eff,0x94100008),
  (6,1024,128,4096,64,0x007f03ff,0x94100008)] {
  for pattern in 0..64 {
   reset();let base=PIPE.plane(slot).base();
   write(base+UNI_PLANE_CTL_OFF,0x94000008|firmware_bits(pattern));
   for offset in [UNI_PLANE_AUX_DIST_OFF,UNI_PLANE_AUX_OFFSET_OFF,UNI_PLANE_CUS_CTL_OFF] {
    write(base+offset,0xdeadbeef);
   }
   let enabled=program_rgba8_plane_static_contract(super::Dev,PIPE,slot,width,height,pitch).unwrap();
   assert_eq!(enabled,expected_ctl);assert_eq!(read(base+UNI_PLANE_CTL_OFF),expected_ctl&!(1<<31));
   assert_eq!(read(base+UNI_PLANE_STRIDE_OFF),expected_stride);
   assert_eq!(read(base+UNI_PLANE_SIZE_OFF),expected_size);
   for offset in [UNI_PLANE_AUX_DIST_OFF,UNI_PLANE_AUX_OFFSET_OFF,UNI_PLANE_CUS_CTL_OFF] {
    assert_eq!(read(base+offset),0);
   }
   assert!(crate::HW.with(|h|h.borrow().writes.iter().all(|(address,_)|*address!=base+UNI_PLANE_SURF_OFF)),
    "static setup must not arm an interim surface");
  }
 }
}

#[test] fn live_guards_reject_every_dirty_fetch_mode_without_mutating_registers() {
 let (primary,_overlay)=ready_registers();let primary_base=PIPE.primary_plane().base();
 let overlay_base=PIPE.plane(4).base();let primary_ctl=read(primary_base);let overlay_ctl=read(overlay_base);
 assert!(primary_plane_contract_regs_match(super::Dev,primary));
 assert_eq!(overlay_plane_dynamic_flip_guard(super::Dev,PIPE,4,OverlayAlphaMode::PremultipliedRgba),Ok(()));
 for pattern in 1..64 {
  write(primary_base,primary_ctl|firmware_bits(pattern));
  write(overlay_base,overlay_ctl|firmware_bits(pattern));
  let writes=crate::HW.with(|h|h.borrow().writes.len());
  assert!(!primary_plane_contract_regs_match(super::Dev,primary),"primary pattern={pattern:#x}");
  assert_eq!(overlay_plane_dynamic_flip_guard(super::Dev,PIPE,4,OverlayAlphaMode::PremultipliedRgba),
   Err("plane-control"),"overlay pattern={pattern:#x}");
  assert_eq!(crate::HW.with(|h|h.borrow().writes.len()),writes,"guards are read-only");
 }
 write(primary_base,primary_ctl);write(overlay_base,overlay_ctl);
 write(overlay_base+UNI_PLANE_AUX_DIST_OFF,4096);
 assert_eq!(overlay_plane_dynamic_flip_guard(super::Dev,PIPE,4,OverlayAlphaMode::PremultipliedRgba),
  Err("plane-aux-surface"));
}

#[test] fn dirty_overlay_fetch_contract_requires_rearm_even_when_geometry_and_surface_match() {
 let (_,overlay)=ready_registers();let base=PIPE.plane(4).base();let canonical=read(base);
 assert!(!overlay_plane_needs_rearm(super::Dev,overlay,0,0,OverlayAlphaMode::PremultipliedRgba));
 for pattern in 1..64 {
  write(base,canonical|firmware_bits(pattern));
  assert!(overlay_plane_needs_rearm(super::Dev,overlay,0,0,OverlayAlphaMode::PremultipliedRgba),
   "pattern={pattern:#x}");
 }
 write(base,canonical);
 assert!(!overlay_plane_needs_rearm(super::Dev,overlay,0,0,OverlayAlphaMode::PremultipliedRgba));
}

#[test] fn nv12_keeps_its_separate_format_y_plane_and_tiling_contract() {
 for (tiling,bits) in [(DirectNv12PlaneTiling::Linear,0),(DirectNv12PlaneTiling::Y,4<<10),
  (DirectNv12PlaneTiling::Yf,5<<10)] {
  let expected=0x81000008|bits;
  assert_eq!(direct_nv12_plane_ctl_enabled(expected,tiling),expected);
  assert_eq!(direct_nv12_y_plane_ctl_enabled(expected,tiling),expected|(1<<19));
  assert_eq!(direct_nv12_plane_ctl_enabled(0,tiling)&(0x1f<<23),1<<24);
  assert_eq!(direct_nv12_y_plane_ctl_enabled(0,tiling)&(0x1f<<23),1<<24);
 }
}
 }
}
'''


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-display-rgba-contract-") as directory:
        source, executable = Path(directory) / "tests.rs", Path(directory) / "tests"
        source.write_text(harness())
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)], check=True)
        subprocess.run([str(executable)], check=True)


if __name__ == "__main__":
    main()
