#!/usr/bin/env python3
"""Run the production GPU-then-CPU release order against owned-buffer mocks."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item

ROOT = Path(__file__).resolve().parents[2]


def main():
    helper = item("src/spirit/mod.rs", "release_vfx_producers")
    source = r'''#![allow(dead_code)]
use std::sync::{Mutex,atomic::{AtomicBool,Ordering}};
static EVENTS:Mutex<Vec<&'static str>>=Mutex::new(Vec::new());
static GPU_DONE:AtomicBool=AtomicBool::new(false);
static CPU_DONE:AtomicBool=AtomicBool::new(false);
static CPU_REQUIRED:AtomicBool=AtomicBool::new(false);
static FAIL_WRITE:AtomicBool=AtomicBool::new(false);
static PIXELS:Mutex<[u8;8]>=Mutex::new([41;8]);
#[derive(Clone,Copy)]struct SpiritFrameLease;
#[derive(Clone,Copy)]struct SpiritSurfaceLayout;
#[derive(Clone,Copy)]struct Stamp{valid:bool}
impl Stamp {
 fn apply(self,pixels:&mut[u8],_:SpiritSurfaceLayout)->bool {
  assert!(GPU_DONE.load(Ordering::SeqCst),"stamp must follow completed GPU ownership");
  assert!(!CPU_DONE.load(Ordering::SeqCst),"stamp cannot write a released surface");
  assert_eq!(pixels[0],41,"stamp reads the completed GPU pixels");
  EVENTS.lock().unwrap().push("stamp");pixels[0]=93;self.valid
 }
}
struct GpuInflight{lease:SpiritFrameLease,cpu_required:bool,stamp:Option<Stamp>}
#[derive(Debug,Eq,PartialEq)]enum SpiritSubmitError{InvalidCommand,InvalidGpuRelease,StaleLease}
mod intel{pub mod gpgpu{pub struct GpgpuRgba8ReleaseFence{pub valid:bool}}}
fn release_gpgpu(_:SpiritFrameLease,release:intel::gpgpu::GpgpuRgba8ReleaseFence)->Result<bool,SpiritSubmitError>{
 if !release.valid{return Err(SpiritSubmitError::InvalidGpuRelease);}
 GPU_DONE.store(true,Ordering::SeqCst);EVENTS.lock().unwrap().push("gpu-release");
 assert!(!CPU_DONE.load(Ordering::SeqCst));Ok(true)
}
fn write_cpu<R>(_:SpiritFrameLease,write:impl FnOnce(&mut[u8],SpiritSurfaceLayout)->R)->Result<R,SpiritSubmitError>{
 assert!(GPU_DONE.load(Ordering::SeqCst));assert!(CPU_REQUIRED.load(Ordering::SeqCst));
 assert!(!ready(),"both-producer surface cannot present before CPU release");
 if FAIL_WRITE.load(Ordering::SeqCst){return Err(SpiritSubmitError::StaleLease);}
 EVENTS.lock().unwrap().push("cpu-invalidate");
 Ok(write(&mut *PIXELS.lock().unwrap(),SpiritSurfaceLayout))
}
fn release_cpu(_:SpiritFrameLease)->Result<(),SpiritSubmitError>{
 assert!(GPU_DONE.load(Ordering::SeqCst));assert!(CPU_REQUIRED.load(Ordering::SeqCst));
 EVENTS.lock().unwrap().push("cpu-flush-release");CPU_DONE.store(true,Ordering::SeqCst);Ok(())
}
fn ready()->bool{GPU_DONE.load(Ordering::SeqCst)&&(!CPU_REQUIRED.load(Ordering::SeqCst)||CPU_DONE.load(Ordering::SeqCst))}
@HELPER@
fn setup(cpu_required:bool,stamp:Option<Stamp>)->GpuInflight{
 EVENTS.lock().unwrap().clear();GPU_DONE.store(false,Ordering::SeqCst);
 CPU_DONE.store(false,Ordering::SeqCst);CPU_REQUIRED.store(cpu_required,Ordering::SeqCst);
 FAIL_WRITE.store(false,Ordering::SeqCst);*PIXELS.lock().unwrap()=[41;8];
 GpuInflight{lease:SpiritFrameLease,cpu_required,stamp}
}
fn release()->intel::gpgpu::GpgpuRgba8ReleaseFence{intel::gpgpu::GpgpuRgba8ReleaseFence{valid:true}}
#[test]fn gpu_then_cpu_stamp_is_flushed_before_surface_becomes_ready(){
 let frame=setup(true,Some(Stamp{valid:true}));assert!(!ready());
 assert_eq!(release_vfx_producers(&frame,release()),Ok(true));assert!(ready());
 assert_eq!(*EVENTS.lock().unwrap(),["gpu-release","cpu-invalidate","stamp","cpu-flush-release"]);
 assert_eq!(PIXELS.lock().unwrap()[0],93);
}
#[test]fn ordinary_frames_keep_their_gpu_only_path(){
 let frame=setup(false,None);assert_eq!(release_vfx_producers(&frame,release()),Ok(true));
 assert!(ready());assert_eq!(*EVENTS.lock().unwrap(),["gpu-release"]);
 assert_eq!(*PIXELS.lock().unwrap(),[41;8]);
}
#[test]fn arrival_canceled_before_source_still_releases_reserved_cpu_bit(){
 let frame=setup(true,None);assert_eq!(release_vfx_producers(&frame,release()),Ok(true));assert!(ready());
 assert_eq!(*EVENTS.lock().unwrap(),["gpu-release","cpu-flush-release"]);
}
#[test]fn unproven_gpu_completion_cannot_touch_cpu_pixels(){
 let frame=setup(true,Some(Stamp{valid:true}));
 assert_eq!(release_vfx_producers(&frame,intel::gpgpu::GpgpuRgba8ReleaseFence{valid:false}),Err(SpiritSubmitError::InvalidGpuRelease));
 assert!(!ready());assert!(EVENTS.lock().unwrap().is_empty());assert_eq!(*PIXELS.lock().unwrap(),[41;8]);
}
#[test]fn failed_stamp_cannot_release_surface_for_presentation(){
 let frame=setup(true,Some(Stamp{valid:false}));
 assert_eq!(release_vfx_producers(&frame,release()),Err(SpiritSubmitError::InvalidCommand));assert!(!ready());
 assert!(!CPU_DONE.load(Ordering::SeqCst));
}
#[test]fn stale_cpu_lease_cannot_stamp_or_release_surface(){
 let frame=setup(true,Some(Stamp{valid:true}));FAIL_WRITE.store(true,Ordering::SeqCst);
 assert_eq!(release_vfx_producers(&frame,release()),Err(SpiritSubmitError::StaleLease));assert!(!ready());
 assert_eq!(*EVENTS.lock().unwrap(),["gpu-release"]);assert_eq!(*PIXELS.lock().unwrap(),[41;8]);
}
'''.replace("@HELPER@", helper)
    with tempfile.TemporaryDirectory(prefix="spirit-vfx-cpu-stamp-") as directory:
        path = Path(directory)
        test = path / "test.rs"
        binary = path / "test"
        test.write_text(source)
        subprocess.run(["rustc", "--edition=2024", "--test", str(test), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
