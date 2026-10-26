#!/usr/bin/env python3
"""Production shared font service: retention, RGBA pages and fallback pixels.

The R8 producer and physical DMA allocations are mocked. The production service
performs preparation, coverage sampling, premultiplication and page caching.
"""
from pathlib import Path
import subprocess
import tempfile

from _microfont_build import build_microfont
ROOT = Path(__file__).resolve().parents[2]
MOCKS = r'''
#![allow(dead_code, unused_variables)]
extern crate alloc;
extern crate self as spin;
extern crate self as embassy_sync;
extern crate self as trueos_time;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    pub fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
pub struct Once<T>(std::sync::OnceLock<T>);
impl<T> Once<T> {
    pub const fn new()->Self {Self(std::sync::OnceLock::new())}
    pub fn get(&self)->Option<&T> {self.0.get()}
    pub fn call_once(&self,f:impl FnOnce()->T)->&T {self.0.get_or_init(f)}
}
pub mod signal {
    pub struct Signal<M,T> {value:std::sync::Mutex<Option<T>>,marker:std::marker::PhantomData<M>}
    impl<M,T> Signal<M,T> {
        pub fn new()->Self {Self {value:std::sync::Mutex::new(None),marker:std::marker::PhantomData}}
        pub fn signal(&self,value:T) {*self.value.lock().unwrap()=Some(value);}
        pub async fn wait(&self)->T {self.value.lock().unwrap().take().unwrap()}
    }
}
pub struct Duration;
impl Duration {pub fn from_millis(_:u64)->Self {Self}}
pub struct Timer;
impl Timer {pub async fn after(_:Duration) {}}
mod wait {pub struct EmbassySpinRawMutex;}
macro_rules! log_important {($($t:tt)*)=>{}}
pub(crate) use log_important;
mod phys {pub fn phys_to_virt(value:usize)->usize {value}}
static BUILDS:AtomicUsize=AtomicUsize::new(0);
static ALLOCATIONS:AtomicUsize=AtomicUsize::new(0);
static FLUSH_FAILS:AtomicBool=AtomicBool::new(false);
static ALLOCATION_FAILS:AtomicBool=AtomicBool::new(false);
mod ui4 {
    pub fn output_dimensions()->Option<(u32,u32)> {Some((1920,1080))}
    pub fn physical_extent_pixels_fraction(w:u32,h:u32,d:u32)->Option<(u32,u32)> {
        assert_eq!((w,h,d),(30,30,4));Some((21,21))
    }
    pub fn physical_extent_pixels(w:u32,h:u32)->Option<(u32,u32)> {
        assert_eq!(w,h);Some((w*14/5,h*14/5))
    }
}
mod intel {
    #[derive(Clone,Copy,Debug,PartialEq,Eq)]
    pub struct GucBcs0RgbaSurface {pub phys:u64,pub gpu:u64,pub bytes:usize,pub width:u32,pub height:u32,pub pitch_bytes:u32}
    #[derive(Clone,Copy,Debug,PartialEq,Eq)]
    pub struct GucBcs0RgbaCopy {pub source:GucBcs0RgbaSurface,pub source_x:u32,pub source_y:u32,pub destination_x:u32,pub destination_y:u32,pub width:u32,pub height:u32}
    pub struct DmaFlushRows;
    impl DmaFlushRows {pub fn new(_: *mut u8,_:usize,_:usize,_:usize)->Self {Self}}
    pub fn dma_flush_strided_row_spans(_:&[DmaFlushRows])->bool {!crate::FLUSH_FAILS.load(crate::Ordering::SeqCst)}
    pub mod gpgpu {
        pub struct GpgpuOwnedRgba8Surface {data:Vec<u8>,width:u32,height:u32,pitch:u32}
        impl GpgpuOwnedRgba8Surface {
            pub fn surface(&self)->super::GucBcs0RgbaSurface {
                super::GucBcs0RgbaSurface {phys:self.data.as_ptr() as u64,gpu:self.data.as_ptr() as u64,
                    bytes:self.data.len(),width:self.width,height:self.height,pitch_bytes:self.pitch}
            }
        }
        pub fn allocate_font_instance_rgba8_surface(width:u32,height:u32)->Option<GpgpuOwnedRgba8Surface> {
            if crate::ALLOCATION_FAILS.load(crate::Ordering::SeqCst) {return None;}
            crate::ALLOCATIONS.fetch_add(1,crate::Ordering::SeqCst);
            let pitch=(width*4).div_ceil(64)*64;
            Some(GpgpuOwnedRgba8Surface {data:vec![0xA5;pitch as usize*height as usize],width,height,pitch})
        }
    }
}
mod r {
    pub mod blocking {
        pub fn try_spawn_blocking_job_with_purpose(job:Box<dyn FnOnce()+Send>,purpose:&str)
            ->Result<(),Box<dyn FnOnce()+Send>> {assert!(["shell3-font","grid-font"].contains(&purpose));job();Ok(())}
    }
    pub mod services {
        pub mod microfont_mask_service {
            pub struct OwnedAlphaAtlas {pub em_height_px:u32,pub line_width_px:f32,
                pub glyph_width:u32,pub glyph_height:u32,pub gutter_px:u32,zero:Vec<u8>,half:Vec<u8>,full:Vec<u8>,horizontal:Vec<u8>,left_heavy:Vec<u8>,junction:Vec<u8>,question:Vec<u8>}
            impl OwnedAlphaAtlas {
                pub fn glyph(&self,ch:char)->Option<&[u8]> {
                    match ch {' '|'\u{2800}'=>Some(&self.zero),'A'=>Some(&self.half),'?'=>Some(&self.question),'─'=>Some(&self.horizontal),'╾'=>Some(&self.left_heavy),'┤'=>Some(&self.junction),'█'|'⣿'=>Some(&self.full),_=>None}
                }
                pub fn glyphs(&self)->impl Iterator<Item=(char,&[u8])> {
                    [' ','A','─','█'].into_iter().map(|ch|(ch,self.glyph(ch).unwrap()))
                }
            }
            pub fn build_shell3_alpha_atlas(height:u32)->Result<&'static OwnedAlphaAtlas,&'static str> {
                assert!([14,21].contains(&height));crate::BUILDS.fetch_add(1,crate::Ordering::SeqCst);
                let glyph_width=((height*6).div_ceil(11)+12) as usize;let glyph_height=(height+12) as usize;let len=glyph_width*glyph_height;
                let raster = |ch| {
                    let bits=microfont::glyph_cell_pixels(ch);
                    (0..len).map(|i| {
                        let x=i%glyph_width;let y=i/glyph_width;
                        if x<6||y<6 {return 0;}
                        let sx=((2*(x-6)+1)*11)/(2*height as usize);
                        let sy=((2*(y-6)+1)*11)/(2*height as usize);
                        if sx<6&&sy<11&&bits&(1u128<<(65-sy*6-sx))!=0 {255}else{0}
                    }).collect::<Vec<u8>>()
                };
                Ok(Box::leak(Box::new(OwnedAlphaAtlas {em_height_px:height,line_width_px:2.75,glyph_width:glyph_width as u32,
                    glyph_height:glyph_height as u32,gutter_px:6,zero:vec![0;len],half:vec![128;len],full:vec![255;len],horizontal:raster('─'),left_heavy:raster('╾'),junction:raster('┤'),question:(0..len).map(|i|if i%glyph_width<glyph_width/2 {255}else{0}).collect()})))
            }
        }
        #[path="__ROOT__/src/r/services/shell3_font_service.rs"]pub mod shell3_font_service;
    }
}
'''
TESTS = r'''
use r::services::shell3_font_service::{prepare,glyph,solid_color};
fn run<F:std::future::Future>(future:F)->F::Output {
    let waker=std::task::Waker::noop();let mut cx=std::task::Context::from_waker(waker);
    let mut future=std::pin::pin!(future);
    loop {if let std::task::Poll::Ready(out)=future.as_mut().poll(&mut cx) {return out;}}
}
fn ready() {run(prepare()).unwrap();}
fn pixels(copy:intel::GucBcs0RgbaCopy)->Vec<[u8;4]> {
    (0..copy.height).flat_map(|y|(0..copy.width).map(move|x| {
        let at=copy.source.phys as usize+(copy.source_y+y)as usize*copy.source.pitch_bytes as usize+(copy.source_x+x)as usize*4;
        unsafe {core::ptr::read_unaligned(at as *const [u8;4])}
    })).collect()
}
#[test]fn z_grid_five_mm_base_does_not_crop_the_new_shell3_seven_point_five_mm_atlas() {
    ready();
    let data=run(r::services::shell3_font_service::grid_glyph(1,'─')).unwrap();
    let width=u32::from_le_bytes(data[..4].try_into().unwrap());
    let height=u32::from_le_bytes(data[4..8].try_into().unwrap());
    assert_eq!((width,height),(8,14));
    let painted:Vec<_>=(0..height).filter(|&y|data[8+(y*width*4) as usize]<255).collect();
    assert!(!painted.is_empty());
    for scale in 1..=8 {run(r::services::shell3_font_service::grid_glyph(scale,'─')).unwrap();}
    assert_eq!(BUILDS.load(Ordering::SeqCst),1,"all Gridpape sizes reuse the shared atlas");
}
#[test]fn preparation_produces_one_retained_alpha_atlas_for_all_consumers() {
    ready();ready();ready();
    assert_eq!(BUILDS.load(Ordering::SeqCst),1);
    for ch in ['A','─','█','q','§'] {glyph(ch,12,22,[255;4],[0,0,0,255],false).unwrap();}
    assert_eq!(BUILDS.load(Ordering::SeqCst),1,"styles and fallback glyphs cannot submit new atlas work");
}
#[test]fn fractional_coverage_is_tinted_once_and_premultiplied() {
    ready();
    let copy=glyph('A',12,22,[200,100,50,255],[20,40,130,255],false).unwrap();
    assert!(pixels(copy).iter().all(|&p|p==[110,70,90,255]));
    let translucent=glyph('A',12,22,[200,100,50,128],[0;4],false).unwrap();
    assert!(pixels(translucent).iter().all(|&p|p==[50,25,13,64]));
    assert!(pixels(translucent).iter().all(|p|p[..3].iter().all(|&c|c<=p[3])));
}
#[test]fn blank_cells_backgrounds_and_underlines_are_preserved() {
    ready();
    let background=[10,20,30,255];let ink=[200,210,220,255];
    let blank=glyph(' ',12,22,ink,background,false).unwrap();
    assert!(pixels(blank).iter().all(|&p|p==background));
    let underline=glyph(' ',12,22,ink,background,true).unwrap();let actual=pixels(underline);
    assert!(actual[..12*20].iter().all(|&p|p==background));
    assert!(actual[12*20..].iter().all(|&p|p==ink));
    let translucent_background=glyph(' ',12,22,ink,[100,50,20,128],false).unwrap();
    assert!(pixels(translucent_background).iter().all(|&p|p==[50,25,10,128]));
}
#[test]fn quad_cells_fill_both_edges_so_neighbour_cells_meet() {
    ready();for ch in ['█','⣿'] {
        let copy=glyph(ch,12,22,[30,60,90,255],[0;4],false).unwrap();
        assert!(pixels(copy).iter().all(|&p|p==[30,60,90,255]));
        assert_eq!((copy.width,copy.height),(12,22));
    }
}
#[test]fn all_horizontal_box_connections_fill_the_cell_without_character_exceptions() {
    ready();for (width,height) in [(6,11),(12,22),(18,33)] {
        let ink=[255;4];let bg=[0,0,0,255];
        let rule=pixels(glyph('─',width,height,ink,bg,false).unwrap());
        let heavy=pixels(glyph('╾',width,height,ink,bg,false).unwrap());
        let junction=pixels(glyph('┤',width,height,ink,bg,false).unwrap());
        let row=(height/2*width) as usize;
        assert!(rule[row..row+width as usize].iter().all(|p|p[0]>0));
        assert!(heavy[row..row+width as usize].iter().all(|p|p[0]>0));
        assert!(junction[row][0]>0,"junction must touch the preceding cell");
        assert_eq!(junction[row+width as usize-1],bg,"do not extend the junction through its blank half");
        assert_eq!(heavy[row+width as usize-1],junction[row]);
    }
}
#[test]fn missing_native_glyphs_use_the_native_question_mark_in_red() {
    ready();for (width,height) in [(6,11),(12,22)] {
        let reference=pixels(glyph('?',width,height,[255,0,0,255],[10,20,30,255],false).unwrap());
        assert!(reference.iter().any(|p|*p==[255,0,0,255]));
        assert!(reference.iter().any(|p|*p==[10,20,30,255]));
        for ch in ['q','§','é','漢','\u{e000}'] {
            let actual=pixels(glyph(ch,width,height,[0,255,0,255],[10,20,30,255],false).unwrap());
            assert_eq!(actual,reference,"missing {ch} must use red native ?");
        }
        let translucent=pixels(glyph('漢',width,height,[0,255,0,128],[0;4],false).unwrap());
        let expected=pixels(glyph('?',width,height,[255,0,0,128],[0;4],false).unwrap());
        assert_eq!(translucent,expected);
    }
}
#[test]fn cache_keys_keep_style_and_dimensions_without_modifying_old_cells() {
    ready();let base=glyph('A',12,22,[255;4],[0;4],false).unwrap();let old=pixels(base);
    assert_eq!(glyph('A',12,22,[255;4],[0;4],false).unwrap(),base);
    let color=glyph('A',12,22,[1,2,3,255],[0;4],false).unwrap();
    let background=glyph('A',12,22,[255;4],[0,0,0,255],false).unwrap();
    let underline=glyph('A',12,22,[255;4],[0;4],true).unwrap();
    let size=glyph('A',6,11,[255;4],[0;4],false).unwrap();
    for other in [color,background,underline,size] {assert_ne!(other,base);}
    assert_eq!(pixels(base),old);
    assert_eq!((base.width,base.height,base.destination_x,base.destination_y),(12,22,0,0));
}
#[test]fn pages_pack_sixty_four_cells_and_retain_every_source_allocation() {
    ready();let before=ALLOCATIONS.load(Ordering::SeqCst);let mut copies=Vec::new();
    for i in 0..65 {
        copies.push(glyph(char::from_u32(0x1000+i).unwrap(),17,23,[255;4],[0,0,0,255],false).unwrap());
    }
    assert_eq!(ALLOCATIONS.load(Ordering::SeqCst)-before,2);
    assert!(copies[..64].iter().all(|c|c.source.phys==copies[0].source.phys));
    assert_ne!(copies[64].source.phys,copies[0].source.phys);
    let slots=copies[..64].iter().map(|c|(c.source_x,c.source_y)).collect::<std::collections::BTreeSet<_>>();
    assert_eq!(slots.len(),64);assert_eq!(copies[63].source_x,7*17);assert_eq!(copies[63].source_y,7*23);
    let old=pixels(copies[0]);
    assert_eq!(glyph('\u{1000}',17,23,[255;4],[0,0,0,255],false).unwrap(),copies[0]);
    assert_eq!(ALLOCATIONS.load(Ordering::SeqCst)-before,2);assert_eq!(pixels(copies[0]),old);
}
#[test]fn allocation_and_publish_failures_can_retry_without_caching_partial_cells() {
    ready();ALLOCATION_FAILS.store(true,Ordering::SeqCst);
    assert_eq!(glyph('A',19,25,[1,2,3,255],[0;4],false),Err("shell3-font-cell-allocation"));
    ALLOCATION_FAILS.store(false,Ordering::SeqCst);FLUSH_FAILS.store(true,Ordering::SeqCst);
    assert_eq!(glyph('A',19,25,[1,2,3,255],[0;4],false),Err("shell3-font-cell-publish"));
    FLUSH_FAILS.store(false,Ordering::SeqCst);
    let retried=glyph('A',19,25,[1,2,3,255],[0;4],false).unwrap();
    assert_eq!((retried.source_x,retried.source_y),(0,0));
    assert_eq!(glyph('A',19,25,[1,2,3,255],[0;4],false).unwrap(),retried);
}
#[test]fn invalid_cell_extents_never_allocate_pages() {
    ready();let before=ALLOCATIONS.load(Ordering::SeqCst);
    for (w,h) in [(0,22),(12,0),(257,22),(12,257)] {
        assert_eq!(glyph('A',w,h,[255;4],[0;4],false),Err("shell3-font-cell-size"));
    }
    assert_eq!(ALLOCATIONS.load(Ordering::SeqCst),before);
}
#[test]fn uniform_metadata_matches_cached_pixels_and_survives_clipping() {
    ready();
    for (ch,fg,bg) in [(' ',[255;4],[10,20,30,255]),
                       ('A',[200,100,50,255],[20,40,130,255]),
                       ('█',[30,60,90,255],[0;4]),
                       (' ',[255;4],[100,50,20,128])] {
        let cell=glyph(ch,12,22,fg,bg,false).unwrap();
        let actual=pixels(cell);let color=u32::from_le_bytes(actual[0]);
        assert!(actual.iter().all(|pixel|*pixel==actual[0]));
        assert_eq!(solid_color(&cell),Some(color));
        let mut clipped=cell;clipped.width=3;clipped.height=5;
        clipped.destination_x=71;clipped.destination_y=29;
        assert_eq!(solid_color(&clipped),Some(color));
        let cached=glyph(ch,12,22,fg,bg,false).unwrap();
        assert_eq!(cached,cell);assert_eq!(solid_color(&cached),Some(color));
    }
}
#[test]fn mixed_cells_and_unknown_source_rectangles_do_not_become_solid_fills() {
    ready();
    for (ch,underline) in [('q',false),(' ',true),('A',true)] {
        let cell=glyph(ch,12,22,[255;4],[0,0,0,255],underline).unwrap();
        let actual=pixels(cell);
        assert!(actual.iter().any(|pixel|*pixel!=actual[0]));
        assert_eq!(solid_color(&cell),None);
        let mut cropped=cell;cropped.width=1;cropped.height=1;
        assert_eq!(solid_color(&cropped),None,"mixed full cells remain copies even if a cropped pixel is uniform");
        assert_eq!(solid_color(&glyph(ch,12,22,[255;4],[0,0,0,255],underline).unwrap()),None);
    }
    let mut unknown=glyph(' ',12,22,[255;4],[0,0,0,255],false).unwrap();
    unknown.source.gpu=0;
    assert_eq!(solid_color(&unknown),None);
}
'''


def main():
    from test_microfont_coverage_batches import block
    sampler = block((ROOT/'src/r/services/microfont_mask_service.rs').read_text(), 'pub(crate) fn sample(')
    source = (MOCKS + TESTS).replace('impl OwnedAlphaAtlas {', 'impl OwnedAlphaAtlas {\n' + sampler)
    with tempfile.TemporaryDirectory(prefix='shell3-font-service-') as temporary:
        path = Path(temporary)
        build_microfont(path / 'libmicrofont.rlib')
        (path / 'tests.rs').write_text(source.replace('__ROOT__', str(ROOT)))
        subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'tests.rs'),
                        '--extern', f'microfont={path / "libmicrofont.rlib"}', '-o', str(path / 'tests')], check=True)
        subprocess.run([str(path / 'tests'), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
