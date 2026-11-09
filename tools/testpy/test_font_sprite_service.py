#!/usr/bin/env python3
"""Host-check the production glyph-resource API with a fake retained mask/storage.

No physical GPU, font rasterizer or guest VM is needed. The real request/cache/
ownership/status code runs against narrow adapters for the shared services.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

ADAPTERS = r'''
#![allow(dead_code, private_interfaces, unused_imports)]
extern crate alloc;
extern crate self as v;
extern crate self as libm;
pub fn roundf(x:f32)->f32{x.round()}
pub mod bp_abi {
    #[derive(Debug)] pub struct TrueosUi4FontSpriteStatusV1 {
        pub state:u32, pub sprite_id:u32, pub width:u32, pub height:u32,
        pub origin_x:i32, pub origin_y:i32,
    }
}
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
static READY:AtomicBool=AtomicBool::new(false);
static SAMPLES:AtomicUsize=AtomicUsize::new(0);
static STROKE:std::sync::atomic::AtomicU32=std::sync::atomic::AtomicU32::new(0);
mod r { pub mod services { pub mod shell3_font_service {
    pub fn with_styled_cell_coverage(_:char,w:u32,h:u32,ink:[u8;4],extra:f32,
        mut pixel:impl FnMut(u32,u32,u8,[u8;4])) -> Result<(), &'static str> {
        if !crate::READY.load(crate::Ordering::Relaxed) {return Err("shell3-font-not-ready");}
        crate::SAMPLES.fetch_add(1,crate::Ordering::Relaxed);
        crate::STROKE.store(extra.to_bits(),crate::Ordering::Relaxed);
        for y in 0..h {for x in 0..w {pixel(x,y,if x==1 {128} else {0},ink);}}
        Ok(())
    }
}}}
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
    fn try_lock(&self)->Option<std::sync::MutexGuard<'_,T>> {self.0.try_lock().ok()}
}
#[derive(Clone,Copy,PartialEq)] enum WindowOwner {Vm(u8), Other}
const ERROR_INVALID:i32=-1; const ERROR_BUSY:i32=-7; const ERROR_CONTEXT:i32=-2;
const ERROR_NOT_FOUND:i32=-3; const ERROR_UI4:i32=-6;
fn blueprint_owner()->Option<WindowOwner>{None}
struct Images {incarnation:u64}
struct Surface {font_sprites:Vec<font_sprite_api::Sprite>, sprites:Vec<(u32,Vec<u8>)>, hull_images:Images}
static SURFACES:Mutex<Surface>=Mutex::new(Surface{
    font_sprites:Vec::new(),sprites:Vec::new(),hull_images:Images{incarnation:1}});
fn surface_mut(s:&mut Surface,owner:WindowOwner,window:u32)->Option<&mut Surface>{
    (owner==WindowOwner::Vm(1) && window==10).then_some(s)
}
mod hull_image_api {
    use super::*;
    pub fn authenticate_window(owner:u32,window:u32)->Result<u64,i32>{
        if owner==1 && window==10 {Ok(1)} else {Err(ERROR_NOT_FOUND)}
    }
    pub fn prepare_rgba(_:u32,_:u32,p:&[u8])->Result<Vec<u8>,i32>{Ok(p.to_vec())}
    pub fn install_prepared(_:u32,_:u32,_:u64,_:u32,p:&mut Vec<u8>)->Result<u32,i32>{
        let mut s=SURFACES.lock();let id=0xc0000000+s.sprites.len() as u32;
        s.sprites.push((id,p.clone()));Ok(id)
    }
}
'''

TESTS = r'''
fn payload(font:u32,ch:u32,size:f32,color:u32)->Vec<u8>{
    [font,ch,size.to_bits(),color].into_iter().flat_map(u32::to_le_bytes).collect()
}
#[test] fn mask_service_cache_wire_and_owner_contract(){
    use font_sprite_api::{request,status};
    let owner=WindowOwner::Vm(1);
    let black=payload(1,'A' as u32,64.,0xff000000);
    assert_eq!(request(owner,10,&black),Err(ERROR_BUSY));
    assert_eq!(request(owner,10,&[]),Err(ERROR_INVALID));
    for bad in [payload(2,65,64.,0),payload(1,0xd800,64.,0),
        payload(1,65,f32::NAN,0),payload(1,65,257.,0)]{
        assert_eq!(request(owner,10,&bad),Err(ERROR_INVALID));
    }
    assert_eq!(request(WindowOwner::Vm(2),10,&black),Err(ERROR_NOT_FOUND));
    assert_eq!(request(owner,11,&black),Err(ERROR_NOT_FOUND));
    READY.store(true,Ordering::Relaxed);
    let id=request(owner,10,&black).unwrap();
    assert_eq!(request(owner,10,&black).unwrap(),id);
    assert_eq!(SAMPLES.load(Ordering::Relaxed),1,"reuse masks/textures");
    let info=status(owner,10,id).unwrap();
    assert_eq!((info.state,info.width,info.height),(2,35,64));
    assert!(status(WindowOwner::Vm(2),10,id).is_err());
    assert!(status(owner,11,id).is_err());
    assert!(status(owner,10,u64::MAX).is_err());
    {
        let s=SURFACES.lock();let p=&s.sprites[0].1;
        assert_eq!(&p[0..4],&[0,0,0,0]);
        assert_eq!(&p[4..8],&[0,0,0,128],"exact mask alpha, black ink");
    }
    let mut styled=black.clone(); styled.extend_from_slice(&2.5f32.to_le_bytes());
    let bold=request(owner,10,&styled).unwrap();
    assert_ne!(bold,id,"stroke is part of the resource cache key");
    assert_eq!(request(owner,10,&styled).unwrap(),bold);
    let bold_info=status(owner,10,bold).unwrap();
    assert_eq!((bold_info.width,bold_info.height),(35,64),"texture budget is unchanged");
    assert_eq!(STROKE.load(Ordering::Relaxed),2.5f32.to_bits());
    for bad in [f32::NAN,-1.,8.1] {
        let mut invalid=black.clone(); invalid.extend_from_slice(&bad.to_le_bytes());
        assert_eq!(request(owner,10,&invalid),Err(ERROR_INVALID));
    }
    let translucent=payload(1,'A' as u32,64.,0x80000000);
    let second=request(owner,10,&translucent).unwrap();
    assert_ne!(id,second);
    assert_eq!(&SURFACES.lock().sprites[2].1[4..8],&[0,0,0,64]);
    SURFACES.lock().sprites.clear();
    assert!(status(owner,10,id).is_err(),"stale resource cannot be sampled");
    request(owner,10,&black).unwrap();
    assert_eq!(SAMPLES.load(Ordering::Relaxed),4,"retired texture is rematerialized");
}
'''


def main():
    source = (ROOT / "src/ui4/blueprint_ui4/font_sprite_api.rs").read_text()
    # Exercise the actual service implementation; VM wrappers compile in the kernel.
    source = source[:source.index('pub unsafe extern "C"')]
    with tempfile.TemporaryDirectory(prefix="trueos-font-sprites-") as directory:
        rust = Path(directory) / "test.rs"
        binary = Path(directory) / "test"
        rust.write_text(ADAPTERS + "\nmod font_sprite_api {\n" + source + "\n}\n" + TESTS)
        subprocess.run(["rustc", "--edition=2024", "--test", str(rust), "-o", str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
