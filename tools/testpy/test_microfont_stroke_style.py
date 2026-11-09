#!/usr/bin/env python3
"""Run the production stroke cache/worker/readback path with a native-render adapter."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
ADAPTERS = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as libm;
pub fn roundf(v:f32)->f32{v.round()}
use alloc::vec::Vec;
use std::sync::atomic::{AtomicU32,Ordering};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T>{
    const fn new(v:T)->Self{Self(std::sync::Mutex::new(v))}
    fn lock(&self)->std::sync::MutexGuard<'_,T>{self.0.lock().unwrap()}
}
mod spin{pub(super) use crate::Mutex;}
type Job=Box<dyn FnOnce()+Send>;
static JOBS:Mutex<Vec<Job>>=Mutex::new(Vec::new());
static WIDTH:AtomicU32=AtomicU32::new(0);
mod r {pub mod blocking{
    pub fn try_spawn_blocking_job_with_purpose(job:crate::Job,_:&str)->Result<(),crate::Job>{
        crate::JOBS.lock().push(job);Ok(())
    }
}}
mod phys{pub fn phys_to_virt(p:usize)->usize{p}}
mod intel{
    pub struct DmaFlushRows;
    impl DmaFlushRows{pub fn new(_: *mut u8,_:usize,_:usize,_:usize)->Self{Self}}
    pub fn dma_flush_strided_row_spans(_: &[DmaFlushRows])->bool{true}
}
const ATLAS_COLUMNS:u32=20;
struct Base {em_height_px:u32,line_width_px:f32}
impl Base{fn glyph(&self,_:char)->Option<&'static [u8]>{Some(&[])}}
fn ready_atlas()->Option<&'static Base>{Some(&Base{em_height_px:44,line_width_px:2.75})}
fn glyph_slot(_:char)->Option<usize>{Some(0)}
#[derive(Default,Clone,Copy)]struct AtlasConfig{glyph_height_px:Option<u32>,line_width_px:f32}
impl AtlasConfig{
    fn normalized(self)->Result<Self,&'static str>{Ok(self)}
    fn gutter(&self)->u32{6}
    fn cell_extent(&self)->(u32,u32){let h=self.glyph_height_px.unwrap();((h*6+5)/11+12,h+12)}
}
struct Surface{phys:u64,pitch_bytes:u32}
struct CoverageAtlas{data:Vec<u8>,pitch:u32}
impl CoverageAtlas{fn surface(&self)->Surface{Surface{phys:self.data.as_ptr() as u64,pitch_bytes:self.pitch}}}
fn with_warmed_atlas<T>(config:AtlasConfig,consumer:impl FnOnce(&CoverageAtlas)->Result<T,&'static str>)
    ->Result<T,&'static str>{
    WIDTH.store(config.line_width_px.to_bits(),Ordering::Relaxed);
    let (w,h)=config.cell_extent();let pitch=w*ATLAS_COLUMNS;
    consumer(&CoverageAtlas{data:vec![(config.line_width_px*10.).round() as u8;(pitch*h) as usize],pitch})
}
'''
TESTS = r'''
#[test]fn native_width_worker_cache_and_fixed_texture_extent(){
    use style::glyph;
    assert_eq!(glyph('A',64,2.5).unwrap_err(),"microfont-style-pending");
    assert_eq!(glyph('A',64,2.5).unwrap_err(),"microfont-style-pending");
    assert_eq!(JOBS.lock().len(),1,"cold polls must not queue duplicate jobs");
    let jobs=std::mem::take(&mut *JOBS.lock());for job in jobs{job();}
    let mask=glyph('A',64,2.5).unwrap();
    assert_eq!(WIDTH.load(Ordering::Relaxed),6.5f32.to_bits(),"2.75 at 44px -> 4 at 64px, plus 2.5");
    assert_eq!(mask.len(),35*64,"no change to texture budget");
    assert!(mask.iter().all(|&p|p==65),"copy the native renderer's coverage");
    assert!(std::sync::Arc::ptr_eq(&mask,&glyph('A',64,2.5).unwrap()),"ready mask reused");
    assert_eq!(JOBS.lock().len(),0);
    assert!(glyph('A',64,1.0).is_err());
    assert_eq!(JOBS.lock().len(),1,"new stroke gets an independent cached mask");
    for (h,w) in [(10,2.5),(129,2.5),(64,-1.),(64,f32::NAN),(64,8.1)]{
        assert_eq!(glyph('A',h,w).unwrap_err(),"microfont-style-shape");
    }
}
'''


def main():
    code = (ROOT / "src/r/services/microfont_mask_service/style.rs").read_text()
    with tempfile.TemporaryDirectory(prefix="trueos-stroke-style-") as directory:
        rust = Path(directory) / "test.rs"
        binary = Path(directory) / "test"
        rust.write_text(ADAPTERS + "\nmod style {\n" + code + "\n}\n" + TESTS)
        subprocess.run(["rustc", "--edition=2024", "--test", str(rust), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
