#!/usr/bin/env python3
"""Check emulator paint ownership and text-cache behavior using production code."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
production = (ROOT / 'src/ui4/emulator_paint.rs').read_text().replace('//!', '//')
harness = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as spin;
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(t:T)->Self {Self(std::sync::Mutex::new(t))}
    pub fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
#[derive(Clone,Copy,PartialEq,Eq)] struct FrameHandle(u64);
#[derive(Clone,Copy)] struct FrameWriteLease {frame:FrameHandle,buffer_index:u8}
#[derive(Clone,Copy)] struct FrameReadLease {frame:FrameHandle,buffer_index:u8}
mod intel {pub mod gpu_font {
    #[derive(Clone,Copy)] pub struct GpuFontFace;
    impl GpuFontFace {
        pub fn resolve_optional(self)->Self {self}
        pub fn registry_name(self)->&'static str {"test"}
    }
    pub fn ensure_font_face_available(_:GpuFontFace)->Result<(),()> {Ok(())}
}}
mod r {pub mod services {pub mod font_kernel_service {
    pub struct RetainedFontRun {pub text:String,pub position:[f32;2],pub font_pixels:f32}
}}}
mod graphics {pub mod font {
    pub static CALLS:std::sync::atomic::AtomicUsize=std::sync::atomic::AtomicUsize::new(0);
    pub struct Summary {pub tessellate_failures:usize,pub outline_glyphs:usize,pub status:&'static str}
    pub struct Mesh {pub summary:Summary,pub indices:Vec<u32>,pub vertices:Vec<[f32;2]>}
    pub fn tessellate_text_mesh(_: &str,_:&str,px:f32)->Mesh {
        CALLS.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
        Mesh {summary:Summary{tessellate_failures:0,outline_glyphs:1,status:"ok"},
            indices:vec![0,1,2],vertices:vec![[0.,0.],[px,0.],[0.,px]]}
    }
}}
mod paint {
PRODUCTION
}
fn lease(frame:u64,index:u8)->FrameWriteLease {FrameWriteLease{frame:FrameHandle(frame),buffer_index:index}}
fn read(l:FrameWriteLease)->FrameReadLease {FrameReadLease{frame:l.frame,buffer_index:l.buffer_index}}
fn solid(n:usize)->paint::Paint {paint::Paint {
    vertices:std::sync::Arc::new(vec![paint::Vertex{position:[0.,0.],uv:[0.,0.]};n]),
    image:None,color:0xffffffff,source_over:true,
}}
#[test]
fn reusing_back_buffer_preserves_front_and_other_frame_generations() {
    let front=lease(1,0);let back=lease(1,1);let replacement=lease(2,0);
    for l in [front,back,replacement] {assert!(paint::append(l,vec![solid(3)]));}
    paint::begin(back);
    assert_eq!(paint::snapshot(read(front)).len(),1);
    assert!(paint::snapshot(read(back)).is_empty());
    paint::forget(front.frame);
    assert!(paint::snapshot(read(front)).is_empty());
    assert_eq!(paint::snapshot(read(replacement)).len(),1);
    paint::forget(replacement.frame);
}
#[test]
fn quota_rejection_preserves_previously_accepted_paint() {
    let l=lease(3,0);
    assert!(paint::append(l,vec![solid(3)]));
    assert!(!paint::append(l,vec![solid(1_000_000)]));
    assert_eq!(paint::snapshot(read(l))[0].vertices.len(),3);
    paint::forget(l.frame);
}
#[test]
fn cached_mesh_is_reused_but_position_color_and_size_remain_per_call() {
    use r::services::font_kernel_service::RetainedFontRun as Run;
    use std::sync::atomic::Ordering::SeqCst;
    let font=intel::gpu_font::GpuFontFace;
    let run=|position,px| Run{text:"cache reuse".into(),position,font_pixels:px};
    let before=graphics::font::CALLS.load(SeqCst);
    let first=paint::text(font,&[run([1.,2.],12.)],0xffffffff).unwrap();
    let second=paint::text(font,&[run([8.,9.],12.)],0xff0000ff).unwrap();
    assert_eq!(graphics::font::CALLS.load(SeqCst),before+1);
    assert_eq!(first[0].vertices[1].position,[13.,2.]);
    assert_eq!(second[0].vertices[1].position,[20.,9.]);
    assert_eq!(second[0].color,0xff0000ff);
    paint::text(font,&[run([0.,0.],24.)],0).unwrap();
    assert_eq!(graphics::font::CALLS.load(SeqCst),before+2);
}
'''.replace('PRODUCTION', production)
with tempfile.TemporaryDirectory(prefix='trueos-virgl-paint-') as tmp:
    source = Path(tmp) / 'paint.rs'
    binary = Path(tmp) / 'paint-tests'
    source.write_text(harness)
    subprocess.run(['rustc', '--edition=2024', '--test', str(source), '-o', str(binary)], check=True)
    subprocess.run([str(binary), '--test-threads=1'], check=True)
