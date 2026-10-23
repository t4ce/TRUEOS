#!/usr/bin/env python3
"""Host model executes production virgl blit packets and tests owned snapshots.

This checks wire commands and pixel/lease semantics, not host GPU execution.
"""
from pathlib import Path
import subprocess
import tempfile
from test_text_blit import SOURCE, ROOT

ADAPTER = r'''
macro_rules! log_important {($($t:tt)*)=>{}}
pub(crate) use log_important;
use std::{future::Future,task::{Context,Poll},sync::{Arc,Mutex,atomic::{AtomicUsize,Ordering}}};
static MODE:AtomicUsize=AtomicUsize::new(0);
static CREATED:AtomicUsize=AtomicUsize::new(0);
static COPIES:AtomicUsize=AtomicUsize::new(0);
static FILLS:AtomicUsize=AtomicUsize::new(0);
mod virtio_gpu_logo {
 use super::*;
 use std::collections::{BTreeMap,VecDeque};
 #[derive(Default)] pub struct VirtioGpuLogo { failed:bool, resources:BTreeMap<u32,(u32,u32,Vec<u8>)> }
 impl VirtioGpuLogo {
  fn create_3d(&mut self,id:u32,w:u32,h:u32,format:u32,_:u32,_:bool)->bool {
   assert_eq!(format,67);assert!(!self.resources.contains_key(&id));CREATED.fetch_add(1,Ordering::SeqCst);
   self.resources.insert(id,(w,h,vec![0;(w*h*4)as usize]));true
  }
  fn readback_rgba(&mut self,id:u32,_:u32,_:u32,_:u64)->Result<Vec<u8>,&'static str> {
   if MODE.load(Ordering::SeqCst)==2 {return Err("test-readback");}Ok(self.resources[&id].2.clone())
  }
  fn unref_resource(&mut self,id:u32)->bool {self.resources.remove(&id).is_some()}
 }
 pub mod requests {
  use super::*;
  type Job=Box<dyn FnOnce(&mut virgl::Compositor,&mut VirtioGpuLogo)+Send>;
  static JOBS:Mutex<VecDeque<Job>>=Mutex::new(VecDeque::new());
  pub async fn dispatch(operation:impl FnOnce(&mut virgl::Compositor,&mut VirtioGpuLogo)->Result<Vec<u8>,&'static str>+Send+'static)->Result<Vec<u8>,&'static str> {
   let output=Arc::new(Mutex::new(None));let slot=Arc::clone(&output);
   JOBS.lock().unwrap().push_back(Box::new(move |c,g|*slot.lock().unwrap()=Some(operation(c,g))));
   std::future::poll_fn(|_|match output.lock().unwrap().take() {Some(result)=>Poll::Ready(result),None=>Poll::Pending}).await
  }
  pub fn service() {
   let job=JOBS.lock().unwrap().pop_front();
   if let Some(job)=job {let mut gpu=VirtioGpuLogo::default();let mut c=virgl::Compositor{next_id:105,fence:0};job(&mut c,&mut gpu);assert!(gpu.resources.is_empty(),"temporary resources leak");}
  }
  pub fn pending()->usize {JOBS.lock().unwrap().len()}
 }
 mod virgl {
  use super::*;
  const TEXTURE_FORMAT_RGBA:u32=67;
  struct VirglCmdBuf {dwords:Vec<u32>}
  impl VirglCmdBuf {fn new()->Self {Self{dwords:vec![]}}fn push(&mut self,v:u32){self.dwords.push(v)}}
  fn virgl_cmd0(cmd:u8,obj:u8,len:u32)->u32 {u32::from(cmd)|(u32::from(obj)<<8)|(len<<16)}
  pub struct Compositor {pub next_id:u32,pub fence:u64}
  impl Compositor {
   fn upload(&mut self,g:&mut VirtioGpuLogo,id:u32,w:u32,h:u32,pitch:u32,pixels:&[u8])->bool {
    let target=&mut g.resources.get_mut(&id).unwrap().2;for y in 0..h as usize {target[y*w as usize*4..(y+1)*w as usize*4].copy_from_slice(&pixels[y*pitch as usize..y*pitch as usize+w as usize*4]);}true
   }
   fn submit(&mut self,g:&mut VirtioGpuLogo,commands:&VirglCmdBuf)->bool {
    if MODE.load(Ordering::SeqCst)==1 {return false;}
    let mut offset=0;
    while offset<commands.dwords.len() {
     let header=commands.dwords[offset];let len=(header>>16)as usize;let words=&commands.dwords[offset+1..offset+1+len];
     match header&0xff {
      17=>{assert_eq!(len,13);COPIES.fetch_add(1,Ordering::SeqCst);let (sw,_,source)=g.resources[&words[5]].clone();let (dw,_,destination)=g.resources.get_mut(&words[0]).unwrap();
       assert_eq!((words[1],words[4],words[6],words[9],words[12]),(0,0,0,0,1));
       for y in 0..words[11]as usize {let s=((words[8]as usize+y)*sw as usize+words[7]as usize)*4;let d=((words[3]as usize+y)* *dw as usize+words[2]as usize)*4;destination[d..d+words[10]as usize*4].copy_from_slice(&source[s..s+words[10]as usize*4]);}
      },
      47=>{assert_eq!(len,12);FILLS.fetch_add(1,Ordering::SeqCst);let(w,_,destination)=g.resources.get_mut(&words[0]).unwrap();
       assert_eq!((words[1],words[4],words[7],words[9],words[10],words[11]),(0,0,1,0,0,0));
       for y in words[3]..words[3]+words[6] {for x in words[2]..words[2]+words[5] {let d=((y* *w+x)*4)as usize;destination[d..d+4].copy_from_slice(&words[8].to_le_bytes());}}
      },
      command=>panic!("unexpected virgl command {command}; blits must not draw glyph triangles"),
     }
     offset+=len+1;
    }
    true
   }
  }
  #[path="__ROOT__/src/virtio_gpu_logo/virgl/blit.rs"]pub mod blit;
 }
 pub(crate) use virgl::blit::{copy_rgba,fill_rgba};
}
fn run<T>(future:impl Future<Output=T>)->T {
 let mut future=std::pin::pin!(future);let mut cx=Context::from_waker(std::task::Waker::noop());
 loop {match future.as_mut().poll(&mut cx) {Poll::Ready(value)=>return value,Poll::Pending=>virtio_gpu_logo::requests::service()}}
}
fn reset() {assert_eq!(virtio_gpu_logo::requests::pending(),0);for c in [&MODE,&CREATED,&COPIES,&FILLS] {c.store(0,Ordering::SeqCst);}}
'''
TESTS = r'''
#[test]fn copy_preserves_rgba_alpha_offsets_padding_and_multiple_sources() {
 reset();let mut a:Vec<u8>=(0..80*13).map(|x|(x*17)as u8).collect();let sa=surface(&mut a,17,13,80);
 let mut b:Vec<u8>=(0..64*11).map(|x|(x*31)as u8).collect();let sb=surface(&mut b,14,11,64);
 let mut output=vec![0x59;112*18];let d=surface(&mut output,23,18,112);let mut expected=output.clone();
 let copies:Vec<_>=(0..12).map(|i|GucBcs0RgbaCopy{source:if i%2==0 {sa}else{sb},source_x:i%7,source_y:i%5,destination_x:i%11,destination_y:i%12,width:7,height:5}).collect();
 for c in &copies {let source=if c.source.phys==sa.phys {&a}else{&b};for y in 0..c.height {let s=((c.source_y+y)*c.source.pitch_bytes+c.source_x*4)as usize;let d=((c.destination_y+y)*d.pitch_bytes+c.destination_x*4)as usize;expected[d..d+c.width as usize*4].copy_from_slice(&source[s..s+c.width as usize*4]);}}
 run(virtio_gpu_logo::copy_rgba(d,&copies)).unwrap();assert_eq!(output,expected);assert_eq!(COPIES.load(Ordering::SeqCst),12);assert_eq!(CREATED.load(Ordering::SeqCst),3);assert_eq!(FILLS.load(Ordering::SeqCst),0);
}
#[test]fn raw_texture_fills_preserve_fractional_alpha_and_order_without_drawing() {
 reset();let mut output=vec![0x59;112*18];let d=surface(&mut output,23,18,112);let mut expected=output.clone();
 let fills=[GucBcs0RgbaFill{x:1,y:2,width:9,height:6,color:0x80402010},GucBcs0RgbaFill{x:4,y:3,width:8,height:9,color:0},GucBcs0RgbaFill{x:2,y:4,width:8,height:3,color:0xffeeddcc}];
 for f in &fills {for y in f.y..f.y+f.height {for x in f.x..f.x+f.width {let at=(y*d.pitch_bytes+x*4)as usize;expected[at..at+4].copy_from_slice(&f.color.to_le_bytes());}}}
 run(virtio_gpu_logo::fill_rgba(d,&fills)).unwrap();assert_eq!(output,expected);assert_eq!(FILLS.load(Ordering::SeqCst),3);assert_eq!(COPIES.load(Ordering::SeqCst),0);
}
#[test]fn cancellation_keeps_owned_snapshots_and_never_touches_original_memory() {
 reset();let mut source=vec![0x37;64*8];let s=surface(&mut source,13,8,64);let mut output=vec![0x59;80*10];let d=surface(&mut output,17,10,80);
 let copies=[GucBcs0RgbaCopy{source:s,source_x:1,source_y:1,destination_x:2,destination_y:2,width:9,height:6}];
 {let mut future=Box::pin(virtio_gpu_logo::copy_rgba(d,&copies));assert!(future.as_mut().poll(&mut Context::from_waker(std::task::Waker::noop())).is_pending());drop(future);}
 source.fill(0xa5);output.fill(0xc1);drop(source);virtio_gpu_logo::requests::service();assert!(output.iter().all(|&v|v==0xc1));assert_eq!(COPIES.load(Ordering::SeqCst),1);
}
#[test]fn failed_submit_or_readback_does_not_publish_partial_destination() {
 for mode in [1,2] {reset();MODE.store(mode,Ordering::SeqCst);let mut output=vec![0x59;80*10];let d=surface(&mut output,17,10,80);let before=output.clone();
 assert!(run(virtio_gpu_logo::fill_rgba(d,&[GucBcs0RgbaFill{x:0,y:0,width:17,height:10,color:0}])).is_err());assert_eq!(output,before);}
}
#[test]fn invalid_bounds_and_alias_reject_before_enqueue() {
 reset();let mut output=vec![0x59;80*10];let d=surface(&mut output,17,10,80);let before=output.clone();
 assert!(run(virtio_gpu_logo::fill_rgba(d,&[GucBcs0RgbaFill{x:u32::MAX,y:0,width:1,height:1,color:0}])).is_err());
 assert!(run(virtio_gpu_logo::copy_rgba(d,&[GucBcs0RgbaCopy{source:d,source_x:0,source_y:0,destination_x:0,destination_y:0,width:1,height:1}])).is_err());
 assert_eq!(output,before);assert_eq!(CREATED.load(Ordering::SeqCst),0);assert_eq!(virtio_gpu_logo::requests::pending(),0);
}
'''

def main():
    source = SOURCE[:SOURCE.index('#[test]')]
    source = source.replace('#[derive(Clone,Copy)]pub struct GucBcs0RgbaSurface', '#[derive(Clone,Copy,PartialEq,Eq)]pub struct GucBcs0RgbaSurface')
    source = source.replace('pub struct DmaFlushRows;', 'pub fn dma_cache_flush_range(_: *const u8,_:usize) {}\npub struct DmaFlushRows;')
    source += ADAPTER + TESTS
    with tempfile.TemporaryDirectory(prefix='virgl-blit-') as directory:
        path = Path(directory)
        (path / 'tests.rs').write_text(source.replace('__ROOT__', str(ROOT)))
        subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'tests.rs'), '-o', str(path / 'tests')], check=True)
        subprocess.run([str(path / 'tests'), '--test-threads=1'], check=True)

if __name__ == '__main__':
    main()
