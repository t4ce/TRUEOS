#!/usr/bin/env python3
"""Host-test Hull's production wire decoder and GPU-to-CPU lease transition."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import item, constant

BRIDGE = 'src/ui4/blueprint_ui4/hull_text_api.rs'
POOL = 'src/ui4/frame_pool.rs'
HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
use alloc::{string::String,vec::Vec};
const ERROR_INVALID:i32=-1;
const ERROR_UI4:i32=-6;
#[derive(Debug)] struct HullTextRun {text:String,x:f32,baseline_y:f32,size_px:f32,advance_px:f32,rgba:u32}
mod hull_font_service {pub fn natural_advance_px(size:f32)->f32 {size*6.0/11.0}}
ITEMS
fn wire(text:&[u8])->Vec<u8> {
    let mut p=1u32.to_le_bytes().to_vec();
    for f in [12.5f32,42.0,24.0,123.0] {p.extend(f.to_le_bytes());}
    p.extend(0x44332211u32.to_le_bytes());p.extend((text.len() as u32).to_le_bytes());
    p.extend(0u32.to_le_bytes());p.extend(text);p
}
#[test] fn decoder_preserves_svg_geometry_color_and_utf8() {
    let r=decode_wire(&wire("café ?".as_bytes())).unwrap().remove(0);
    assert_eq!(r.text,"café ?");assert_eq!((r.x,r.baseline_y,r.size_px,r.advance_px),(12.5,42.,24.,123.));
    assert_eq!(r.rgba,0x44332211);
}
#[test] fn decoder_rejects_truncation_trailing_bytes_and_invalid_utf8() {
    let p=wire(b"text");
    for length in 0..p.len() {assert!(decode_wire(&p[..length]).is_err(),"{length}");}
    let mut trailing=p.clone();trailing.push(0);assert!(decode_wire(&trailing).is_err());
    assert!(decode_wire(&wire(&[0xff])).is_err());
}
#[test] fn decoder_retains_column_origin_of_split_natural_advance_runs() {
    let mut p=wire(b"next");p[16..20].copy_from_slice(&0f32.to_le_bytes());
    p[28..32].copy_from_slice(&12u32.to_le_bytes());
    let r=decode_wire(&p).unwrap().remove(0);
    assert!((r.x-(12.5+hull_font_service::natural_advance_px(24.)*12.)).abs()<0.001);
    assert_eq!(r.advance_px,0.);
}
#[test] fn decoder_bounds_total_run_count_and_total_text_bytes() {
    assert!(decode_wire(&((MAX_RUNS+1) as u32).to_le_bytes()).is_err());
    assert!(decode_wire(&wire(&vec![b'a';MAX_RUN_TEXT_BYTES+1])).is_err());
    let run=wire(&vec![b'a';MAX_RUN_TEXT_BYTES]);
    let mut p=17u32.to_le_bytes().to_vec();
    for _ in 0..17 {p.extend_from_slice(&run[4..]);}
    assert!(decode_wire(&p).is_err());
    assert!(decode_wire(&0u32.to_le_bytes()).unwrap().is_empty());
}
#[test] fn decoder_rejects_nonfinite_geometry_and_invalid_sizes() {
    for (offset,values) in [(4,vec![f32::NAN,f32::INFINITY,1_000_001.]),
        (8,vec![f32::NAN,f32::NEG_INFINITY,-1_000_001.]),
        (12,vec![0.,-1.,1441.,f32::NAN]),(16,vec![-1.,f32::INFINITY])] {
        for f in values {let mut p=wire(b"t");p[offset..offset+4].copy_from_slice(&f.to_le_bytes());
            assert!(decode_wire(&p).is_err(),"{offset} {f}");}
    }
}
#[test] fn paint_flush_region_respects_pitch_and_exact_last_visible_byte() {
    let r=paint_flush_region(13,7,64,436,Some((2,3,11,4))).unwrap().unwrap();
    assert_eq!((r.offset,r.row_bytes,r.rows),(200,44,4));
    assert!(paint_flush_region(13,7,64,435,Some((2,3,11,4))).is_err());
    let pixel=paint_flush_region(1,1,4,4,Some((0,0,1,1))).unwrap().unwrap();
    assert_eq!((pixel.offset,pixel.row_bytes,pixel.rows),(0,4,1));
}
#[test] fn paint_flush_region_rejects_outside_empty_and_overflowing_rectangles() {
    for bounds in [(2,3,12,4),(2,3,11,5),(0,0,0,1),(0,0,1,0),
        (u32::MAX,0,2,1),(0,u32::MAX,1,2)] {
        assert!(paint_flush_region(13,7,64,448,Some(bounds)).is_err());
    }
    assert!(paint_flush_region(13,7,51,448,Some((0,0,1,1))).is_err());
    assert!(paint_flush_region(13,7,64,448,None).unwrap().is_none());
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)] enum FramePoolError {InvalidLease,ProducerReleaseRequired}
#[derive(Clone,Copy)] struct FrameWriteLease {frame:u32,buffer_index:u8,token:u64}
#[derive(Clone,Copy,Debug,PartialEq,Eq)] struct FrameGpuRelease {phys:u64,bytes:usize}
impl FrameGpuRelease {fn matches(self,phys:u64,bytes:usize)->bool {self.phys==phys&&self.bytes==bytes}}
struct TestFrame {gpu_full_overwrite_required:bool,gpu_authored:[bool;1],gpu_release:[Option<FrameGpuRelease>;1],surfaces:[Option<u32>;1]}
struct Pool {frame:TestFrame}
impl Pool {fn checked_mut(&mut self,frame:u32)->Result<&mut TestFrame,FramePoolError> {
    if frame==7 {Ok(&mut self.frame)} else {Err(FramePoolError::InvalidLease)}
}}
struct Lock(std::sync::Mutex<Pool>);
impl Lock {fn lock(&self)->std::sync::MutexGuard<'_,Pool> {self.0.lock().unwrap()}}
static FRAME_POOL:Lock=Lock(std::sync::Mutex::new(Pool {frame:TestFrame {
    gpu_full_overwrite_required:false,gpu_authored:[false],gpu_release:[None],surfaces:[Some(1)]}}));
fn checked_lease(_: &TestFrame,lease:FrameWriteLease)->Result<(),FramePoolError> {
    if lease.buffer_index==0&&lease.token==8 {Ok(())} else {Err(FramePoolError::InvalidLease)}
}
mod ui_surface {pub struct Access {pub phys:u64,pub byte_len:usize}
    pub fn rgba_access(surface:u32)->Option<Access> {(surface==1).then_some(Access {phys:4096,byte_len:8192})}}
OVERLAY
#[test] fn overlay_requires_exact_retired_allocation_and_current_lease() {
    let lease=FrameWriteLease {frame:7,buffer_index:0,token:8};
    let proof=FrameGpuRelease {phys:4096,bytes:8192};
    FRAME_POOL.lock().frame.gpu_authored[0]=true;
    assert_eq!(mark_frame_buffer_cpu_overlay(lease,None),Err(FramePoolError::ProducerReleaseRequired));
    for bad in [FrameGpuRelease {phys:8192,..proof},FrameGpuRelease {bytes:4096,..proof}] {
        assert_eq!(mark_frame_buffer_cpu_overlay(lease,Some(bad)),Err(FramePoolError::InvalidLease));
        assert!(FRAME_POOL.lock().frame.gpu_authored[0]);
    }
    assert_eq!(mark_frame_buffer_cpu_overlay(FrameWriteLease {token:9,..lease},Some(proof)),Err(FramePoolError::InvalidLease));
    assert_eq!(mark_frame_buffer_cpu_overlay(lease,Some(proof)),Ok(()));
    assert!(!FRAME_POOL.lock().frame.gpu_authored[0]);
    assert_eq!(mark_frame_buffer_cpu_overlay(lease,None),Ok(()));
    assert_eq!(mark_frame_buffer_cpu_overlay(lease,Some(proof)),Err(FramePoolError::ProducerReleaseRequired));
    FRAME_POOL.lock().frame.gpu_full_overwrite_required=true;
    assert_eq!(mark_frame_buffer_cpu_overlay(lease,None),Err(FramePoolError::ProducerReleaseRequired));
}
'''


def main():
    declarations = '\n'.join(constant(BRIDGE, name) for name in (
        'MAX_RUNS', 'MAX_RUN_TEXT_BYTES', 'MAX_TEXT_BYTES', 'WIRE_RUN_BYTES'))
    declarations += '\n' + '\n'.join(item(BRIDGE, name) for name in (
        'valid_geometry', 'decode_wire', 'PaintFlushRegion', 'paint_flush_region'))
    source = HARNESS.replace('ITEMS', declarations).replace('OVERLAY', item(POOL, 'mark_frame_buffer_cpu_overlay'))
    with tempfile.TemporaryDirectory(prefix='hull-text-bridge-') as directory:
        path = Path(directory) / 'tests.rs'
        binary = Path(directory) / 'tests'
        path.write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', str(path), '-o', str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    main()
