#!/usr/bin/env python3
"""Execute the production R8 proof with controlled GPU retirement outcomes.

The fixture replaces hardware submission/allocation, then records which backing
resources the actual diagnostic releases. Incomplete or failed submissions must
retain their mesh and push constants; retired draws must verify actual R8 bytes.
"""
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def harness() -> str:
    source = r'''
#![allow(dead_code, unused_variables)]
use std::sync::{Mutex, atomic::{AtomicBool, AtomicUsize, Ordering}};
#[macro_export] macro_rules! log_important { ($($token:tt)*) => {{}}; }
#[macro_export] macro_rules! log_error { ($($token:tt)*) => {{}}; }
static MODE:AtomicUsize=AtomicUsize::new(0);
static FAIL_CONSTANTS:AtomicBool=AtomicBool::new(false);
static MESH_RELEASES:AtomicUsize=AtomicUsize::new(0);
static CONSTANT_RELEASES:AtomicUsize=AtomicUsize::new(0);
static MASK_RELEASES:AtomicUsize=AtomicUsize::new(0);
static MEMORY:Mutex<[u8;1024]>=Mutex::new([0;1024]);
mod pci {pub fn config_read_u32(_:u8,_:u8,_:u8,offset:u8)->u32 {
 if offset==0 {0x9a49_8086} else {1}
}}
mod phys {pub fn phys_to_virt(address:usize)->usize {address}}
mod intel {
 pub struct Device {pub bus:u8,pub slot:u8,pub function:u8,pub device_id:u16,pub revision_id:u8}
 pub fn claimed_device()->Option<Device> {Some(Device{bus:0,slot:2,function:0,device_id:0x4680,revision_id:12})}
 pub struct DmaFlushRows;
 impl DmaFlushRows {pub fn new(_: *mut u8,_:usize,_:usize,_:usize)->Self {Self}}
 pub fn dma_flush_strided_row_spans(_: &[DmaFlushRows])->bool {true}
 pub mod render {
  use crate::{MODE,FAIL_CONSTANTS,MESH_RELEASES,CONSTANT_RELEASES,MASK_RELEASES,MEMORY,Ordering};
  struct ResidentTriangleMesh;
  struct ResidentRenderBuffer;
  impl ResidentRenderBuffer {
   fn write(&self,offset:usize,bytes:&[u8])->bool {offset+bytes.len()<=64}
   fn flush(&self) {}
   fn gpu_base(&self)->u64 {0x1000}
  }
  #[derive(Clone,Copy)] struct Surface {phys:u64,bytes:usize,pitch_bytes:u32}
  struct ResidentCoverageMask {retired:bool}
  impl ResidentCoverageMask {fn surface(&self)->Surface {Surface {
   phys:MEMORY.lock().unwrap().as_ptr() as u64,bytes:1024,pitch_bytes:64,
  }}}
  enum ResidentSceneFragmentContract {MicrofontMask(u64)}
  enum ResidentScenePrimitiveTopology {TriangleList}
  struct ResidentSceneDraw<'a> {
   mesh:&'a ResidentTriangleMesh,depth_flags:Option<()>,rgba:[u8;4],sampled_texture:Option<()>,
   fragment_contract:ResidentSceneFragmentContract,viewport_translation_px:[f32;2],
   topology:ResidentScenePrimitiveTopology,point_width_px:u32,line_width_px:f32,clip_adjacency:bool,
  }
  struct Release {phys:u64,bytes:usize}
  impl Release {
   fn matches(&self,phys:u64,bytes:usize)->bool {self.phys==phys && self.bytes==bytes}
   fn sequence(&self)->u64 {17}
  }
  struct Frame {frame_complete:bool,release_fence:Option<Release>,completed_draws:usize,gpu_poll_us:u64}
  impl Frame {fn completion_error(&self)->Option<&'static str> {
   (!self.frame_complete).then_some("resident-scene-geometry-incomplete")
  }}
  fn create_resident_triangle_mesh(_: &[[f32;3]],_: &[u32])->Result<ResidentTriangleMesh,&'static str> {Ok(ResidentTriangleMesh)}
  fn allocate_resident_render_buffer(_:usize)->Result<ResidentRenderBuffer,&'static str> {
   if FAIL_CONSTANTS.load(Ordering::Relaxed) {Err("constants-allocation")} else {Ok(ResidentRenderBuffer)}
  }
  fn create_resident_coverage_mask(_:u32,_:u32)->Result<ResidentCoverageMask,&'static str> {Ok(ResidentCoverageMask{retired:true})}
  fn render_resident_coverage_mask(_: &[ResidentSceneDraw<'_>],mask:&mut ResidentCoverageMask)->Result<Frame,&'static str> {
   let mode=MODE.load(Ordering::Relaxed);
   if mode==2 {return Err("render-error")}
   let completed=mode!=1;
   mask.retired=completed;
   if mode==0 {
    let mut pixels=MEMORY.lock().unwrap();
    for y in 4..12 {pixels[y*64+4..y*64+12].fill(255);}
   }
   let surface=mask.surface();
   Ok(Frame {frame_complete:completed,release_fence:completed.then_some(Release{phys:surface.phys,bytes:surface.bytes}),
    completed_draws:usize::from(completed),gpu_poll_us:21})
  }
  fn release_resident_coverage_mask(mask:ResidentCoverageMask)->bool {
   MASK_RELEASES.fetch_add(1,Ordering::Relaxed);mask.retired
  }
  fn release_resident_render_buffer(_: &ResidentRenderBuffer)->bool {
   CONSTANT_RELEASES.fetch_add(1,Ordering::Relaxed);true
  }
  fn release_resident_triangle_mesh(_: &ResidentTriangleMesh)->bool {
   MESH_RELEASES.fetch_add(1,Ordering::Relaxed);true
  }
'''
    source += '#[path=' + json.dumps(str(ROOT / 'src/intel/render/native_font_probe_contract.rs')) + '] mod native_font_probe_contract;\n'
    source += 'include!(' + json.dumps(str(ROOT / 'src/intel/render/native_font_probe.rs')) + ');\n'
    source += r'''
 }
}
fn reset(mode:usize) {
 MODE.store(mode,Ordering::Relaxed);FAIL_CONSTANTS.store(false,Ordering::Relaxed);
 MESH_RELEASES.store(0,Ordering::Relaxed);CONSTANT_RELEASES.store(0,Ordering::Relaxed);
 MASK_RELEASES.store(0,Ordering::Relaxed);MEMORY.lock().unwrap().fill(0);
}
fn releases()->(usize,usize,usize) {(
 MESH_RELEASES.load(Ordering::Relaxed),CONSTANT_RELEASES.load(Ordering::Relaxed),MASK_RELEASES.load(Ordering::Relaxed),
)}
#[test] fn retired_quad_verifies_pixels_identity_and_releases_all_resources() {
 reset(0);let proof=intel::render::probe_native_microfont_r8().unwrap();
 assert!(proof.passed());assert_eq!(proof.physical_device,0x9a49);
 assert_eq!(proof.effective_device,0x4680);assert_eq!(proof.release_sequence,17);
 assert_eq!(releases(),(1,1,1));
}
#[test] fn retired_blank_target_cannot_pass_even_with_complete_fence() {
 reset(3);let proof=intel::render::probe_native_microfont_r8().unwrap();
 assert!(!proof.passed());assert_eq!(proof.pixels.mismatched,64);
 assert_eq!(releases(),(1,1,1));
}
#[test] fn unretired_draw_retains_mesh_constants_and_quarantines_target() {
 reset(1);assert_eq!(intel::render::probe_native_microfont_r8().unwrap_err(),"resident-scene-geometry-incomplete");
 assert_eq!(releases(),(0,0,1));
}
#[test] fn submission_error_does_not_assume_inputs_retired() {
 reset(2);assert_eq!(intel::render::probe_native_microfont_r8().unwrap_err(),"render-error");
 assert_eq!(releases(),(0,0,1));
}
#[test] fn allocation_failure_before_submission_releases_prepared_mesh() {
 reset(0);FAIL_CONSTANTS.store(true,Ordering::Relaxed);
 assert_eq!(intel::render::probe_native_microfont_r8().unwrap_err(),"constants-allocation");
 assert_eq!(releases(),(1,0,0));
}
'''
    return source


def main() -> None:
    with tempfile.TemporaryDirectory(prefix='trueos-native-font-proof-') as temporary:
        source = Path(temporary) / 'probe.rs'
        binary = Path(temporary) / 'probe-tests'
        source.write_text(harness())
        subprocess.run(['rustc', '--edition=2024', '--test', str(source), '-o', str(binary)], check=True)
        subprocess.run([str(binary), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
