#!/usr/bin/env python3
"""Run the production retained-atlas service with mocked GPU/frame boundaries."""
from pathlib import Path
import subprocess, tempfile
from _microfont_build import build_microfont
ROOT=Path(__file__).resolve().parents[2]
MOCKS=r'''
struct ResetAlpha(std::sync::Mutex<Option<&'static services::microfont_mask_service::OwnedAlphaAtlas>>);
impl ResetAlpha {
 const fn new()->Self {Self(std::sync::Mutex::new(None))}
 fn get(&self)->Option<&'static services::microfont_mask_service::OwnedAlphaAtlas> {*self.0.lock().unwrap()}
 fn call_once(&self,f:impl FnOnce()->services::microfont_mask_service::OwnedAlphaAtlas)->&'static services::microfont_mask_service::OwnedAlphaAtlas {
  *self.0.lock().unwrap().get_or_insert_with(||Box::leak(Box::new(f())))
 }
 fn reset(&self) {*self.0.lock().unwrap()=None;}
}

extern crate alloc;
use std::sync::Mutex;
mod wait {
    pub struct WaitQueue(std::sync::atomic::AtomicU64);
    impl WaitQueue {
        pub const fn new()->Self {Self(std::sync::atomic::AtomicU64::new(0))}
        pub fn observe(&self)->u64 {self.0.load(std::sync::atomic::Ordering::Acquire)}
        pub fn notify_all(&self) {self.0.fetch_add(1,std::sync::atomic::Ordering::Release);}
        pub async fn wait_after(&self,old:u64) {
            std::future::poll_fn(|_|if self.observe()!=old {std::task::Poll::Ready(())}else{std::task::Poll::Pending}).await
        }
    }
}
mod libm { pub fn ceilf(v:f32)->f32 {v.ceil()} pub fn sqrtf(v:f32)->f32 {v.sqrt()} pub fn roundf(v:f32)->f32 {v.round()} }
mod spin {
    pub struct Once<T>(std::sync::OnceLock<T>);
    impl<T> Once<T> {
        pub const fn new()->Self {Self(std::sync::OnceLock::new())}
        pub fn get(&self)->Option<&T> {self.0.get()}
        pub fn call_once(&self,f:impl FnOnce()->T)->&T {self.0.get_or_init(f)}
    }
    pub struct Mutex<T>(std::sync::Mutex<T>);
    impl<T> Mutex<T> { pub const fn new(v:T)->Self {Self(std::sync::Mutex::new(v))}
        pub fn try_lock(&self)->Option<std::sync::MutexGuard<'_,T>> {self.0.try_lock().ok()}
        pub fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
    }
}
mod log_os {
    pub enum LogLevel {Once}
    pub fn log_with_target_level(_: &str, _: LogLevel, _: std::fmt::Arguments<'_>) {}
}
#[macro_export] macro_rules! log_important { ($($t:tt)*) => {}; }
#[macro_export] macro_rules! log_error { ($($t:tt)*) => {}; }
#[derive(Default)] struct State {
    uploads:usize,releases:usize,renders:usize,allocations:usize,mask_releases:usize,fault:u8,
    vertices:Vec<Vec<[f32;3]>>,params:Vec<[u32;16]>,dest:Vec<u8>,published:usize,constant_allocations:usize,constant_writes:Vec<(usize,usize)>,
}
static STATE:Mutex<State>=Mutex::new(State {uploads:0,releases:0,renders:0,allocations:0,
    mask_releases:0,fault:0,vertices:Vec::new(),params:Vec::new(),dest:Vec::new(),published:0,constant_allocations:0,constant_writes:Vec::new()});
fn fault()->u8 {STATE.lock().unwrap().fault}
mod phys {
    pub fn phys_to_virt(p:usize)->usize {p}
    pub fn virt_to_phys_checked<T>(p:*const T)->Option<u64> {Some(p as u64)}
}
mod virtio_gpu_logo {
    pub static EMULATOR:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
    pub struct NativeMaskVertex {pub position:[f32;4],pub geometry:[f32;4],pub style:[f32;4]}
    pub struct NativeMaskGeometry {
        pub lines:Vec<NativeMaskVertex>,pub points:Vec<NativeMaskVertex>,
        pub quads:Vec<NativeMaskVertex>,pub line_width_px:f32,
    }
    pub fn output_dimensions()->Option<(u32,u32)> {
        EMULATOR.load(std::sync::atomic::Ordering::Relaxed).then_some((1280,720))
    }
    pub fn render_coverage_mask(w:u32,h:u32,geometry:NativeMaskGeometry)->Result<Vec<u8>,&'static str> {
        assert!(output_dimensions().is_some());
        assert!(!geometry.lines.is_empty() || !geometry.quads.is_empty());
        crate::STATE.lock().unwrap().renders+=1;
        Ok(vec![128;w as usize*h as usize])
    }
}
mod graphics {pub mod primitives {
    #[derive(Clone,Copy,Debug,PartialEq)]
    pub struct Rgba8 {pub r:u8,pub g:u8,pub b:u8,pub a:u8}
    impl Rgba8 {pub fn new(r:u8,g:u8,b:u8,a:u8)->Self {Self{r,g,b,a}}}
}}
#[derive(Debug)] struct MenuRect {x:u32,y:u32,width:u32,height:u32,color:graphics::primitives::Rgba8}
mod r {pub mod services {pub use crate::services::microfont_mask_service;}}
mod shell3_font_service {
    pub const DEFAULT_UI4_FONT_SCALE:u32=2;
    pub fn cell_extent(_:u32)->Option<(u32,u32)> {panic!("this GPU harness supplies physical font metrics")}
}
mod ui4 {
    pub mod context_menu {pub const MENU_RENDER_LABEL_CHARS:usize=20;}
    pub fn physical_extent_pixels(w:u32,h:u32)->Option<(u32,u32)> {assert!(w==h && (h==50 || h==10));Some((w*213/50,h*213/50))}
    #[derive(Clone,Copy)] pub struct FrameHandle;
    pub struct View {pub width:u32,pub height:u32,pub pitch:u32,pub virt:*mut u8}
    pub fn acquire_frame_buffer(_:FrameHandle)->Result<(),()> {Ok(())}
    pub fn writable_rgba_view(_:())->Result<View,()> {
        let mut s=crate::STATE.lock().unwrap();s.dest.resize(96*88*4,0);
        Ok(View {width:96,height:88,pitch:96*4,virt:s.dest.as_mut_ptr()})
    }
    pub fn mark_frame_buffer_cpu_authored(_:())->Result<(),()> {Ok(())}
    pub fn mark_frame_buffer_fully_opaque(_:())->Result<(),()> {Ok(())}
    pub fn publish_frame_buffer(_:())->Result<(),()> {crate::STATE.lock().unwrap().published+=1;Ok(())}
    pub fn cancel_frame_buffer(_:())->Result<(),()> {Ok(())}
}
mod intel {
    pub struct DmaFlushRows;
    impl DmaFlushRows {pub fn new(_:*mut u8,_:usize,_:usize,_:usize)->Self {Self}}
    pub fn dma_flush_strided_row_spans(_:&[DmaFlushRows])->bool {true}
    pub mod gpgpu {
        #[derive(Clone,Copy,Default)] pub struct GpgpuMask8Surface {
            pub phys:u64,pub bytes:usize,pub width:u32,pub height:u32,pub pitch_bytes:u32,
        }
    }
    pub mod render {
        use super::gpgpu::GpgpuMask8Surface;
        #[derive(Clone,Copy,PartialEq,Debug)] pub enum ResidentScenePrimitiveTopology {TriangleList,LineStripAdj}
        pub struct ResidentRenderBuffer {data:std::cell::RefCell<Vec<u32>>}
        impl ResidentRenderBuffer {
            pub fn gpu_base(&self)->u64 {self.data.borrow().as_ptr() as u64}
            pub fn write(&self,offset:usize,bytes:&[u8])->bool {
                crate::STATE.lock().unwrap().constant_writes.push((offset,bytes.len()));
                let mut data=self.data.borrow_mut();
                if offset+bytes.len()>data.len()*4 {return false;}
                unsafe {std::ptr::copy_nonoverlapping(bytes.as_ptr(),(data.as_mut_ptr() as *mut u8).add(offset),bytes.len());}
                true
            }
            pub fn flush(&self) {}
        }
        pub fn allocate_resident_render_buffer(bytes:usize)->Result<ResidentRenderBuffer,&'static str> {
            crate::STATE.lock().unwrap().constant_allocations+=1;
            Ok(ResidentRenderBuffer {data:std::cell::RefCell::new(vec![0;bytes.div_ceil(4)])})
        }
        pub fn release_resident_render_buffer(_:&ResidentRenderBuffer)->bool {true}
        pub struct ResidentTriangleMesh;
        pub enum ResidentSceneFragmentContract {MicrofontMask(u64)}
        pub struct ResidentSceneDraw<'a> {
            pub mesh:&'a ResidentTriangleMesh,pub depth_flags:Option<u32>,pub rgba:[u8;4],
            pub sampled_texture:Option<()>,pub fragment_contract:ResidentSceneFragmentContract,
            pub viewport_translation_px:[f32;2],pub topology:ResidentScenePrimitiveTopology,
            pub point_width_px:u32,pub line_width_px:f32,pub clip_adjacency:bool,
        }
        pub fn create_resident_indexed_mesh(v:&[[f32;3]],i:&[u32],_:ResidentScenePrimitiveTopology)->Result<ResidentTriangleMesh,&'static str> {
            let mut s=crate::STATE.lock().unwrap();s.uploads+=1;
            if s.fault==1 && s.uploads==2 {return Err("upload");}
            assert!(!v.is_empty() && !i.is_empty());assert!(v.iter().flatten().all(|x|x.is_finite()));
            assert!(i.iter().all(|&x|(x as usize)<v.len()));
            s.vertices.push(v.to_vec());Ok(ResidentTriangleMesh)
        }
        pub fn release_resident_triangle_mesh(_:&ResidentTriangleMesh)->bool {crate::STATE.lock().unwrap().releases+=1;true}
        pub struct ResidentCoverageMask {data:Vec<u8>,width:u32,height:u32}
        impl ResidentCoverageMask {pub fn surface(&self)->GpgpuMask8Surface {
            GpgpuMask8Surface {phys:self.data.as_ptr() as u64,bytes:self.data.len(),width:self.width,
                height:self.height,pitch_bytes:self.width}
        }}
        pub fn create_resident_coverage_mask(w:u32,h:u32)->Result<ResidentCoverageMask,&'static str> {
            crate::STATE.lock().unwrap().allocations+=1;
            Ok(ResidentCoverageMask {data:vec![0;w as usize*h as usize],width:w,height:h})
        }
        pub fn release_resident_coverage_mask(_:ResidentCoverageMask)->bool {crate::STATE.lock().unwrap().mask_releases+=1;true}
        pub struct ResultFrame {pub release_fence:Option<u32>,pub gpu_timestamp:Option<u32>,pub gpu_target_ready_timestamp:Option<u32>}
        impl ResultFrame {pub fn completion_error(&self)->Option<&'static str> {(crate::fault()==3).then_some("retirement")}}
        pub fn render_resident_coverage_mask(draws:&[ResidentSceneDraw<'_>],mask:&mut ResidentCoverageMask)->Result<ResultFrame,&'static str> {
            let mut s=crate::STATE.lock().unwrap();s.renders+=1;s.params.clear();
            for d in draws {
                let ResidentSceneFragmentContract::MicrofontMask(address)=d.fragment_contract;
                let mut p=[0u32;16];
                unsafe {std::ptr::copy_nonoverlapping(address as *const u32,p.as_mut_ptr(),16);}
                let f=p.map(f32::from_bits);
                assert_eq!(d.rgba,[255;4]);assert!(d.depth_flags.is_none());assert!(d.sampled_texture.is_none());
                assert_eq!(f[14],1.0/mask.width as f32);assert_eq!(f[15],1.0/mask.height as f32);
                assert_eq!(d.clip_adjacency,d.topology==ResidentScenePrimitiveTopology::LineStripAdj);
                if d.clip_adjacency {
                    assert_eq!(f[8],d.line_width_px/2.0);
                    assert!(d.line_width_px==f[13] || d.line_width_px==(f[13]*std::f32::consts::SQRT_2*128.0).round()/128.0);
                }
                s.params.push(p);
            }
            if s.fault==2 {return Err("render-busy");}
            if s.fault==5 {return Err("scene-frame-draw-limit");}
            let columns=crate::services::microfont_mask_service::ATLAS_COLUMNS as usize;
            let scale=mask.width/(columns as u32*6);
            let height=f32::from_bits(s.params[0][9]) as u32;
            let half_width=f32::from_bits(s.params[0][13])*0.5;
            let source_scale=f32::from_bits(s.params[0][12]);
            let dot_overhang=half_width*std::f32::consts::SQRT_2-source_scale*0.5+0.5;
            let gutter=((half_width+1.0).max(dot_overhang).ceil() as u32).max(6);
            let (cell_width,cell_height)=if height==0 {(6*scale,11*scale)}
                else {((height*6+10)/11+2*gutter,height+2*gutter)};
            // Synthetic coverage tests the actual copy/tint sink independently
            // of hardware rasterization, which is exercised on the rig.
            let slot=if mask.width==cell_width {0}else{('A' as usize)-32};
            mask.data.fill(0);
            for y in 0..cell_height { for x in 0..cell_width {
                let px=(slot%columns) as u32*cell_width+x;let py=(slot/columns) as u32*cell_height+y;
                mask.data[(py*mask.width+px) as usize]=128;
            }}
            Ok(ResultFrame {release_fence:(s.fault!=4).then_some(1),gpu_timestamp:None,gpu_target_ready_timestamp:None})
        }
    }
}
'''
TESTS=r'''
use services::microfont_mask_service::*;
#[test] fn virgl_keeps_line_seeds_and_width_without_tessellating() {
    let config=AtlasConfig {glyph_height_px:Some(110),line_width_px:3.5,..AtlasConfig::default()};
    let (width,height)=config.cell_extent();
    let native=native_mask_geometry(config,width,height,Some('§'.into())).unwrap();
    let strips=microfont::raster_line::glyph_strips('§').unwrap();
    let expected:usize=strips.iter().map(|s|s.iter_points().collect::<Vec<_>>().windows(2)
        .filter(|pair|pair[0]!=pair[1]).count()*2).sum();
    assert_eq!(native.lines.len(),expected);assert_eq!(native.line_width_px,3.5);
    assert!(native.quads.is_empty());
    let mut at=0;
    for strip in strips {for segment in strip.iter_points().collect::<Vec<_>>().windows(2) {
        if segment[0]==segment[1] {continue;}
        for p in segment {
            let v=&native.lines[at];at+=1;
            let pixel=[(v.position[0]+1.0)*width as f32/2.0,(v.position[1]+1.0)*height as f32/2.0];
            let expected=[6.0+(p.x() as f32+0.5)*10.0,6.0+(p.y() as f32+0.5)*10.0];
            for k in 0..2 {assert!((pixel[k]-expected[k]).abs()<0.001);}
            assert_eq!(v.style,[1.75,1.0,0.0,0.0]);
        }
    }}
    assert!(native.points.iter().all(|v|[2.0,3.0,4.0].contains(&v.style[1])));
    assert!(native.points.is_empty()); // Current virgl diagnostic submits seed bodies only.
}
#[test] fn virgl_native_quads_preserve_all_canonical_box_block_braille_seeds() {
    let config=AtlasConfig::default();let (width,height)=config.extent();
    let native=native_mask_geometry(config,width,height,None).unwrap();
    let quads:usize=microfont::raster_quad::GLYPH_CHARACTERS.iter().filter_map(|&ch|
        microfont::raster_quad::glyph_quads_with_curve_quads(ch,rounded_quad_budget(config))).map(|q|q.len()).sum();
    assert_eq!(native.quads.len(),4*quads);
    assert!(native.quads.iter().all(|v|v.style==[0.0;4]));
    assert!(native.lines.len()>1000);
    assert!(native.lines.iter().chain(native.points.iter()).chain(native.quads.iter())
        .all(|v|v.position.iter().chain(v.geometry.iter()).chain(v.style.iter()).all(|x|x.is_finite())));
}
#[test] fn virgl_readback_reuses_retained_mask_and_never_allocates_intel_geometry() {
    reset(0);
    virtio_gpu_logo::EMULATOR.store(true,std::sync::atomic::Ordering::Relaxed);
    let config=AtlasConfig {glyph_height_px:Some(19),line_width_px:3.5,..AtlasConfig::default()};
    let surface=with_warmed_atlas(config,|atlas|Ok(atlas.surface())).unwrap();
    assert_eq!((surface.width,surface.height),config.extent());
    assert_eq!(surface.pitch_bytes,surface.width);
    assert_eq!(unsafe {std::ptr::read(surface.phys as *const u8)},128);
    with_warmed_atlas(config,|_|Ok(())).unwrap();
    let s=STATE.lock().unwrap();
    assert_eq!(s.renders,1);assert_eq!(s.uploads,0);assert_eq!(s.allocations,0);
    assert_eq!(s.constant_allocations,0);drop(s);
    let old=services::microfont_mask_service::ATLAS.try_lock().unwrap().take();drop(old);
    virtio_gpu_logo::EMULATOR.store(false,std::sync::atomic::Ordering::Relaxed);
}
fn reset(fault:u8) {
    // Complete the one-off boot production before measuring atlas reuse.
    warm_start_glyph().unwrap();
    services::microfont_mask_service::ALPHA_ATLAS.reset();
    let old=services::microfont_mask_service::ATLAS.try_lock().unwrap().take();drop(old);
    *STATE.lock().unwrap()=State {fault,..State::default()};
}
fn expected_lines()->usize {
    (' '..='~').chain(microfont::raster_line::GLYPH_CHARACTERS.iter().copied()
        .filter(|ch| !(' '..='~').contains(ch) && !('\u{2500}'..='\u{259f}').contains(ch)))
        .filter_map(microfont::raster_line::glyph_strips).map(|s|s.len()).sum()
}
#[test] fn start_homebutton_is_retained_once_as_a_single_physical_size_glyph() {
    reset(0);
    let cached=START_GLYPH.get().unwrap();
    assert_eq!((cached.em_height_px,cached.width,cached.height),(213,129,225));
    assert_eq!(cached.pixels.len(),129*225);
    assert!(cached.pixels.iter().any(|&p|p!=0));
    let pointer=cached.pixels.as_ptr();
    warm_start_glyph().unwrap();warm_start_glyph().unwrap();
    assert_eq!(START_GLYPH.get().unwrap().pixels.as_ptr(),pointer);
    assert_eq!(STATE.lock().unwrap().renders,0);
    with_warmed_atlas(AtlasConfig::default(),|_|Ok(())).unwrap();
    assert_eq!(START_GLYPH.get().unwrap().pixels.as_ptr(),pointer);
    assert_eq!(STATE.lock().unwrap().renders,1);
}
#[test] fn homebutton_renders_its_private_strokes_as_one_independent_mask() {
    reset(0);
    let home=microfont::raster_line::home_button_strips().unwrap();
    let mask=build_home_button_mask(320).unwrap();
    assert_eq!((mask.width,mask.height),(187,332));
    let state=STATE.lock().unwrap();
    assert_eq!(state.uploads,home.len()+1);
    assert!(state.params.iter().all(|p|f32::from_bits(p[13])==6.0));
    assert_eq!(state.params.iter().filter(|p|f32::from_bits(p[8])>0.0).count(),home.len());
    assert_eq!((state.renders,state.allocations,state.mask_releases),(1,1,1));
    for (vertices,strip) in state.vertices.iter().zip(home) {
        for (actual,point) in vertices[1..vertices.len()-1].iter().zip(strip.iter_points()) {
            assert_eq!(&actual[..2],&[point.x() as f32+0.5,point.y() as f32+0.5]);
        }
    }
}
#[test] fn private_homebutton_uses_the_same_source_on_virgl() {
    let config=AtlasConfig {glyph_height_px:Some(110),line_width_px:6.0,..AtlasConfig::default()};
    let (width,height)=config.cell_extent();
    let geometry=native_mask_geometry(config,width,height,Some(MaskGlyph::HomeButton)).unwrap();
    let count:usize=microfont::raster_line::home_button_strips().unwrap().iter()
        .map(|s|s.points.len()-1).sum();
    assert_eq!(geometry.lines.len(),count*2);
    assert!(geometry.points.is_empty() && geometry.quads.is_empty());
}
#[test] fn maps_all_three_ranges_without_collisions() {
    assert!(SLOT_COUNT>255);assert_eq!(glyph_slot(' '),Some(0));assert_eq!(glyph_slot('~'),Some(94));
    assert_eq!(glyph_slot('─'),Some(95));assert_eq!(glyph_slot('\u{257f}'),Some(222));
    assert_eq!(glyph_slot('▀'),Some(223));assert_eq!(glyph_slot('\u{259f}'),Some(254));assert_eq!(glyph_slot('漢'),None);
    let extras:Vec<_>=microfont::raster_line::GLYPH_CHARACTERS.iter().copied()
        .filter(|ch| !(' '..='~').contains(ch) && !('\u{2500}'..='\u{259f}').contains(ch))
        .map(|ch|glyph_slot(ch).unwrap()).collect();
    let quad_start=255+extras.len();
    assert_eq!(extras,(255..quad_start).collect::<Vec<_>>());
    let quad_extras:Vec<_>=microfont::raster_quad::GLYPH_CHARACTERS.iter().copied()
        .filter(|ch| !(' '..='~').contains(ch) && !('\u{2500}'..='\u{259f}').contains(ch)
            && !('\u{2800}'..='\u{28ff}').contains(ch))
        .map(|ch|glyph_slot(ch).unwrap()).collect();
    assert_eq!(quad_extras,(quad_start..BRAILLE_START).collect::<Vec<_>>());
    assert!(glyph_slot('§').unwrap()>=255);
    assert_eq!(glyph_slot('\u{2800}'),Some(BRAILLE_START));
    assert_eq!(glyph_slot('\u{28ff}'),Some(SLOT_COUNT-1));
    assert_eq!(ATLAS_ROWS,SLOT_COUNT.div_ceil(ATLAS_COLUMNS as usize) as u32);
}
#[test] fn endpoint_recipes_identify_indices_without_changing_seed_positions() {
    reset(0);with_warmed_atlas(AtlasConfig::default(),|_|Ok(())).unwrap();
    let state=STATE.lock().unwrap();
    for (vertices,params) in state.vertices.iter().zip(&state.params) {
        if f32::from_bits(params[8])<=0.0 {continue;}
        let last=vertices.len()-2;
        let dot=vertices.iter().all(|v|v[..2]==vertices[1][..2]);
        assert_eq!(vertices[1][2],if dot {1.25}else{0.25});
        assert_eq!(vertices[last][2],if dot {1.75}else{0.75});
        for point in &vertices[2..last] {assert_eq!(point[2],0.5);}
        for (index,offset) in [(1,0),(last,4)] {
            assert_eq!(vertices[index][0],f32::from_bits(params[offset]));
            assert_eq!(vertices[index][1],f32::from_bits(params[offset+1]));
        }
    }
}
#[test] fn coincident_dots_keep_centres_cap_tags_and_finite_opposite_frames() {
    use microfont::raster_line::{Boundary::{End,Junction},Point,Strip};
    static DOT:[Point;2]=[Point::new(2,2),Point::new(2,2)];
    for start in [End,Junction] {for end in [End,Junction] {
        let strip=Strip::new(&DOT,start,end);
        let p=endpoint_params(&strip,0).map(f32::from_bits);
        assert_eq!(&p[..8],&[2.5,2.5,1.0,0.0,2.5,2.5,-1.0,0.0]);
        assert_eq!(p[8],std::f32::consts::SQRT_2);
        assert_eq!(p[10],if start==End {1.0}else{0.0});
        assert_eq!(p[11],if end==End {1.0}else{0.0});
    }}
    for ch in ['ä','Ä'] {
        let strips=microfont::raster_line::glyph_strips(ch).unwrap();
        let dots:Vec<_>=strips.iter().filter(|s|s.points.iter().all(|p|*p==s.points[0])).collect();
        assert_eq!(dots.len(),2);
        for dot in dots {
            let p=endpoint_params(&dot,glyph_slot(ch).unwrap()).map(f32::from_bits);
            assert_eq!(&p[..2],&p[4..6]);
            assert_eq!(&p[2..4],&[1.0,0.0]);assert_eq!(&p[6..8],&[-1.0,0.0]);
            assert_eq!(&p[10..12],&[1.0,1.0]);
        }
    }
    // The current i dot is a short nonzero stroke with its own tangent.
    assert!(microfont::raster_line::glyph_strips('i').unwrap().iter()
        .all(|s|s.points.iter().any(|p|*p!=s.points[0])));
}
#[test] fn dot_diameter_tracks_sqrt_two_with_matching_native_and_analytic_widths() {
    reset(0);
    let mut uploads=0;
    for width in [3.5,7.0,8.0,9.0,32.0] {
        with_warmed_atlas(AtlasConfig {glyph_height_px:Some(19),line_width_px:width,
            ..AtlasConfig::default()},|_|Ok(())).unwrap();
        let state=STATE.lock().unwrap();
        if uploads==0 {uploads=state.uploads;}
        assert_eq!(state.uploads,uploads);
        let mut dots=0;
        for (vertices,params) in state.vertices.iter().zip(&state.params) {
            let p=params.map(f32::from_bits);
            if p[8]<=0.0 {continue;}
            let expected=if vertices[1][2]==1.25 {
                dots+=1;(width*std::f32::consts::SQRT_2*128.0).round()/128.0
            } else {width};
            assert_eq!(p[8]*2.0,expected);
            assert_eq!(p[13],width); // Cell layout and joins use the stroke width.
        }
        assert!(dots>0);
    }
}
#[test] fn physical_gutters_contain_the_larger_dots_at_the_cell_edge() {
    for height in [11,19,38,110] {for width in [3.5,8.0,9.0,16.0,32.0] {
        let config=AtlasConfig {glyph_height_px:Some(height),line_width_px:width,
            ..AtlasConfig::default()};
        let radius=(width*std::f32::consts::SQRT_2*128.0).round()/256.0;
        let gutter=(config.cell_extent().1-height)/2;
        assert!(gutter as f32+(height as f32/11.0)*0.5>=radius+0.5-0.004);
    }}
}
#[test] fn geometry_is_uploaded_once_and_same_config_never_redraws() {
    reset(0);for _ in 0..3 {with_warmed_atlas(AtlasConfig::default(),|_|Ok(())).unwrap();}
    let s=STATE.lock().unwrap();assert_eq!((s.renders,s.allocations,s.releases),(1,1,0));
    assert!(s.uploads>expected_lines());assert_eq!(s.params.iter().filter(|p|f32::from_bits(p[8])>0.0).count(),expected_lines());
    assert_eq!(s.params[0][8],0); // Combined box/block quad batch.
}
#[test] fn width_changes_only_constants_and_mask_no_geometry_or_allocation() {
    reset(0);with_warmed_atlas(AtlasConfig::default(),|_|Ok(())).unwrap();
    let (uploads,vertices)={let s=STATE.lock().unwrap();(s.uploads,s.vertices.clone())};
    for width in [7.0,6.0,8.0] {with_warmed_atlas(AtlasConfig {line_width_px:width,..AtlasConfig::default()},|_|Ok(())).unwrap();}
    let s=STATE.lock().unwrap();assert_eq!(s.uploads,uploads);assert_eq!(s.vertices,vertices);
    assert_eq!((s.renders,s.allocations,s.releases),(4,1,0));
}
#[test] fn fixed_endpoint_metadata_is_never_uploaded_again() {
    reset(0);with_warmed_atlas(AtlasConfig::default(),|_|Ok(())).unwrap();
    let (written,before)={let s=STATE.lock().unwrap();(s.constant_writes.len(),s.params.clone())};
    with_warmed_atlas(AtlasConfig {line_width_px:7.0,..AtlasConfig::default()},|_|Ok(())).unwrap();
    let s=STATE.lock().unwrap();assert_eq!(s.constant_allocations,1);
    assert!(s.constant_writes[written..].iter().all(|&(offset,len)|len==4 && matches!(offset%64,32|36|48|52|56|60)));
    for (old,new) in before.iter().zip(&s.params) {
        assert_eq!(&old[..8],&new[..8]);assert_eq!(&old[10..12],&new[10..12]);
    }
}
#[test] fn size_replaces_mask_but_keeps_every_geometry_buffer() {
    reset(0);with_warmed_atlas(AtlasConfig::default(),|_|Ok(())).unwrap();
    let uploads=STATE.lock().unwrap().uploads;
    with_warmed_atlas(AtlasConfig {glyph_scale:4,line_width_px:4.0,..AtlasConfig::default()},|a| {assert_eq!((a.surface().width,a.surface().height),(ATLAS_COLUMNS*24,ATLAS_ROWS*44));Ok(())}).unwrap();
    let s=STATE.lock().unwrap();assert_eq!(s.uploads,uploads);assert_eq!((s.renders,s.allocations,s.mask_releases),(2,2,1));
}
#[test] fn physical_cell_pitch_rounds_up_to_match_shader_packing() {
    for height in 11..=240 {
        let config=AtlasConfig {glyph_height_px:Some(height),..AtlasConfig::default()};
        let (width,cell_height)=config.cell_extent();
        let inner=width-12;
        assert!(inner*11>=height*6);
        assert!((inner-1)*11<height*6);
        assert_eq!(cell_height,height+12);
    }
}
#[test] fn rounded_corners_select_one_retained_variant_without_reuploads() {
    reset(0);
    let mut uploads=0;
    for height in [42,84,42] {
        let config=AtlasConfig {glyph_height_px:Some(height),..AtlasConfig::default()};
        with_warmed_atlas(config,|_|Ok(())).unwrap();
        let state=STATE.lock().unwrap();
        if uploads==0 {uploads=state.uploads;}
        assert_eq!(state.uploads,uploads);
        let corners:Vec<_>=state.params.iter().filter(|p|p[8]==0 && f32::from_bits(p[10])>=4.0).collect();
        assert_eq!(corners.len(),1);
        assert_eq!(f32::from_bits(corners[0][10]) as u8,if height==42 {4}else{8});
        assert_eq!(state.params.iter().filter(|p|p[8]==0 && p[10]==0).count(),1);
    }
}
#[test] fn rounded_native_geometry_matches_the_intel_variant_budget_and_cell_edges() {
    let config=AtlasConfig {glyph_height_px:Some(168),..AtlasConfig::default()};
    let (width,height)=config.cell_extent();
    assert_eq!(rounded_quad_budget(config),16);
    for ch in ['╭','╮','╯','╰'] {
        let native=native_mask_geometry(config,width,height,Some(ch.into())).unwrap();
        assert!(native.lines.is_empty() && native.points.is_empty());
        assert_eq!(native.quads.len(),4*(2+16));
        let expected=microfont::raster_quad::glyph_quads_with_curve_quads(ch,16).unwrap();
        for (vertices,quad) in native.quads.chunks_exact(4).zip(expected) {
            for (vertex,source) in vertices.iter().zip(quad.vertices) {
                let pixel=[(vertex.position[0]+1.0)*width as f32*0.5,(vertex.position[1]+1.0)*height as f32*0.5];
                for axis in 0..2 {assert!((pixel[axis]-6.0-source[axis]*(168.0/11.0)).abs()<0.001);}
            }
        }
    }
}
#[test] fn physical_sizes_reuse_source_geometry_and_keep_native_width() {
    reset(0);
    let mut uploads = 0;
    for height in [19,38,57,76,85,19] {
        let config = AtlasConfig { glyph_height_px:Some(height), ..AtlasConfig::default() };
        with_warmed_atlas(config,|atlas| {
            assert_eq!(atlas.config().line_width_px,8.0);
            let (w,h) = atlas.config().cell_extent();
            assert_eq!(h,height+12); // six native pixels of overhang room per side
            assert_eq!(w,(height*6+10)/11+12);
            assert_eq!(atlas.glyph_rect('A').unwrap()[2..],[w,h]);

            assert_eq!((atlas.surface().width,atlas.surface().height),(ATLAS_COLUMNS*w,ATLAS_ROWS*h));
            Ok(())
        }).unwrap();
        let state=STATE.lock().unwrap();
        if uploads==0 {uploads=state.uploads;}
        assert_eq!(state.uploads,uploads);
        assert!(state.params.iter().all(|p| f32::from_bits(p[9])==height as f32));
        assert!(state.params.iter().all(|p| (f32::from_bits(p[12])*11.0-height as f32).abs()<0.0001));
        assert_eq!(state.params.iter().filter(|p| p[8]==0).count(),2); // static quads + one adaptive corner variant
    }
}
#[test] fn shared_snapshot_is_produced_once_for_every_consumer() {
    reset(0);
    let fixed=build_shell3_alpha_atlas(19).unwrap();
    assert_eq!(fixed.em_height_px,19);assert_eq!(fixed.line_width_px,2.75);
    assert_eq!((fixed.glyph_width,fixed.glyph_height,fixed.gutter_px),(23,31,6));
    let a=fixed.glyph('A').unwrap().to_vec();
    assert!(a.iter().all(|&coverage|coverage==128));
    assert_eq!(fixed.glyph('§').unwrap().len(),23*31);
    assert_eq!(fixed.glyph('█').unwrap().len(),23*31);
    for code in 0x2800..=0x28ff {
        assert_eq!(fixed.glyph(char::from_u32(code).unwrap()).unwrap().len(),23*31);
    }
    assert!(fixed.glyph('\u{2800}').unwrap().iter().all(|&coverage|coverage==0));
    assert!(fixed.glyph('漢').is_none());
    assert!(fixed.glyph(' ').unwrap().iter().all(|&coverage|coverage==0));
    let uploads=STATE.lock().unwrap().uploads;
    {
        let state=STATE.lock().unwrap();
        assert_eq!(state.renders,1);
        assert_eq!(state.params[0][8],0); // box/block body shares this submission
        assert!(state.params.iter().all(|p|f32::from_bits(p[13])==2.75));
    }
    let again=build_shell3_alpha_atlas(19).unwrap();
    assert!(std::ptr::eq(fixed,again));
    assert_eq!(build_shell3_alpha_atlas(38).err(),Some("microfont-atlas-already-sized"));
    let mut text=vec![0;24*22];
    copy_text_mask(&mut text,24,22,0,0,"A ",12,22).unwrap();
    assert!(text.chunks_exact(24).all(|row|row[..12].iter().all(|&v|v==128)
        && row[12..].iter().all(|&v|v==0)));
    assert_eq!(fixed.glyph('A').unwrap(),a);
    assert_eq!(fixed.em_height_px,19);assert_eq!(fixed.line_width_px,2.75);
    let state=STATE.lock().unwrap();assert_eq!(state.uploads,uploads);assert_eq!(state.renders,1);
    assert!(fixed.glyphs().any(|(ch,_)|ch=='§'));
}
#[test] fn all_braille_patterns_upload_their_precomputed_dot_quads() {
    reset(0);with_warmed_atlas(AtlasConfig::default(),|_|Ok(())).unwrap();
    let state=STATE.lock().unwrap();
    for code in 0x2800..=0x28ff {
        let ch=char::from_u32(code).unwrap();let slot=glyph_slot(ch).unwrap();
        let actual=state.vertices[0].chunks_exact(4)
            .filter(|v|(-v[0][2]-1.0) as usize==slot).collect::<Vec<_>>();
        let expected=microfont::raster_quad::braille_quad_slice(ch).unwrap();
        assert_eq!(actual.len(),(code-0x2800).count_ones() as usize);
        for (vertices,quad) in actual.iter().zip(expected) {
            let origin=[(slot%ATLAS_COLUMNS as usize) as f32*6.0,(slot/ATLAS_COLUMNS as usize) as f32*11.0];
            assert_eq!([vertices[0][0]-origin[0],vertices[0][1]-origin[1]],quad.top_left.map(f32::from));
            assert_eq!([vertices[2][0]-origin[0],vertices[2][1]-origin[1]],quad.bottom_right.map(f32::from));
        }
    }
}
#[test] fn cp850_standalone_quads_have_slots_uploads_and_snapshot_reads() {
    reset(0);
    with_warmed_atlas(AtlasConfig::default(),|atlas| {
        for ch in ['▬','■'] {assert!(atlas.glyph_rect(ch).is_some());}
        Ok(())
    }).unwrap();
    let state=STATE.lock().unwrap();
    for ch in ['▬','■'] {
        assert!(microfont::raster_line::glyph_strips(ch).is_none());
        assert!(microfont::raster_quad::GLYPH_CHARACTERS.contains(&ch));
        let slot=glyph_slot(ch).unwrap();
        assert!(slot>=255 && slot<BRAILLE_START);
        let quads=microfont::raster_quad::glyph_quads(ch).unwrap();
        assert_eq!(quads.len(),1);
        let actual=state.vertices[0].chunks_exact(4)
            .filter(|v|(-v[0][2]-1.0) as usize==slot).collect::<Vec<_>>();
        assert_eq!(actual.len(),1);
        let origin=[(slot%ATLAS_COLUMNS as usize) as f32*6.0,(slot/ATLAS_COLUMNS as usize) as f32*11.0];
        for (point,expected) in actual[0].iter().zip(quads.get(0).unwrap().vertices) {
            assert_eq!([point[0]-origin[0],point[1]-origin[1]],expected);
        }
    }
    assert_ne!(glyph_slot('▬'),glyph_slot('■'));
    drop(state);
    let snapshot=build_shell3_alpha_atlas(19).unwrap();
    for ch in ['▬','■'] {
        assert_eq!(snapshot.glyph(ch).unwrap().len(),(snapshot.glyph_width*snapshot.glyph_height) as usize);
        assert_eq!(snapshot.glyphs().filter(|&(candidate,_)|candidate==ch).count(),1);
    }
}
#[test] fn quad_vertices_keep_explicit_owning_cell_at_exact_boundaries() {
    reset(0);with_warmed_atlas(AtlasConfig {glyph_height_px:Some(19),
        line_width_px:3.5,..AtlasConfig::default()},|_|Ok(())).unwrap();
    let state=STATE.lock().unwrap();
    for quad in state.vertices[0].chunks_exact(4) {
        let slot=(-quad[0][2]-1.0) as usize;
        assert!((BOX_START..SLOT_COUNT).contains(&slot));
        for point in quad {
            assert_eq!(point[2],quad[0][2]);
            let x=point[0]-(slot%ATLAS_COLUMNS as usize) as f32*6.0;
            let y=point[1]-(slot/ATLAS_COLUMNS as usize) as f32*11.0;
            assert!((0.0..=6.0).contains(&x));
            assert!((0.0..=11.0).contains(&y));
        }
    }
}
#[test] fn busy_retry_keeps_uploaded_geometry() {
    reset(2);assert_eq!(with_warmed_atlas(AtlasConfig::default(),|_|Ok(())),Err("render-busy"));
    let uploads=STATE.lock().unwrap().uploads;STATE.lock().unwrap().fault=0;
    with_warmed_atlas(AtlasConfig::default(),|_|Ok(())).unwrap();
    let s=STATE.lock().unwrap();assert_eq!(s.uploads,uploads);assert_eq!((s.allocations,s.releases),(1,0));
}
#[test] fn rejected_submission_does_not_quarantine_shared_atlas() {
    reset(5);
    assert_eq!(with_warmed_atlas(AtlasConfig::default(),|_|Ok(())),Err("scene-frame-draw-limit"));
    let uploads=STATE.lock().unwrap().uploads;
    STATE.lock().unwrap().fault=0;
    with_warmed_atlas(AtlasConfig::default(),|_|Ok(())).unwrap();
    let state=STATE.lock().unwrap();
    assert_eq!(state.uploads,uploads);
    assert_eq!((state.renders,state.allocations,state.releases),(2,1,0));
}
#[test] fn partial_startup_releases_only_successful_uploads() {
    reset(1);assert_eq!(with_warmed_atlas(AtlasConfig::default(),|_|Ok(())),Err("upload"));
    let s=STATE.lock().unwrap();assert_eq!((s.uploads,s.releases,s.renders),(2,1,0));
}
#[test] fn uncertain_completion_pins_geometry_and_rejects_reuse() {
    for fault in [3,4] {
        reset(fault);assert!(with_warmed_atlas(AtlasConfig::default(),|_|Ok(())).is_err());
        assert_eq!(with_warmed_atlas(AtlasConfig::default(),|_|Ok(())),Err("microfont-atlas-quarantined"));
        let s=STATE.lock().unwrap();assert_eq!((s.releases,s.mask_releases,s.renders),(0,0,1));
    }
}
#[test] fn configuration_validation_precedes_upload_and_quantizes_width_once() {
    reset(0);for (scale,width) in [(0,8.0),(24,8.0),(16,f32::NAN),(16,0.125),(4,8.0)] {
        assert!(with_warmed_atlas(AtlasConfig {glyph_scale:scale,line_width_px:width,..AtlasConfig::default()},|_|Ok(())).is_err());
    }
    assert_eq!(STATE.lock().unwrap().uploads,0);
    for width in [7.0,7.001] {with_warmed_atlas(AtlasConfig {line_width_px:width,..AtlasConfig::default()},|_|Ok(())).unwrap();}
    assert_eq!(STATE.lock().unwrap().renders,1);
    for width in [0.25,0.5,0.75] {
        with_warmed_atlas(AtlasConfig {line_width_px:width,..AtlasConfig::default()},|a| {
            assert_eq!(a.config().line_width_px,width); Ok(())
        }).unwrap();
    }
}
#[test] fn text_copy_waits_for_ready_tiles_and_clips_without_warming() {
    reset(0);
    let mut pixels=vec![7;26];
    assert_eq!(copy_text_mask(&mut pixels[..24],6,4,0,0,"A",6,11),Err("microfont-not-ready"));
    assert!(pixels.iter().all(|&v|v==7));
    assert_eq!(STATE.lock().unwrap().renders,0);
    build_shell3_alpha_atlas(19).unwrap();
    assert_eq!(copy_text_mask(&mut pixels,0,4,0,0,"A",6,11),Err("microfont-text-shape"));
    assert_eq!(copy_text_mask(&mut pixels[..3],6,4,0,0,"A",6,11),Err("microfont-text-shape"));
    copy_text_mask(&mut pixels[..24],6,4,4,2,"AAAA",6,11).unwrap();
    for y in 0..4 {for x in 0..6 {
        assert_eq!(pixels[y*6+x],if x>=4&&y>=2 {128}else{7});
    }}
    assert_eq!(&pixels[24..],&[7,7]);
    assert_eq!(STATE.lock().unwrap().renders,1);
}
#[test] fn ui4_menus_copy_r8_at_display_size_and_preserve_coverage_and_opacity() {
    reset(0);build_shell3_alpha_atlas(19).unwrap();
    use graphics::primitives::Rgba8;
    let mut rects=Vec::new();
    services::menu_text::push_microfont_menu_text(&mut rects,10,20,24,"A ",Rgba8::new(80,90,100,128));
    assert_eq!(rects.len(),22);
    for (y,r) in rects.iter().enumerate() {
        assert_eq!((r.x,r.y,r.width,r.height),(10,20+y as u32,12,1));
        assert_eq!(r.color,Rgba8::new(80,90,100,64));
    }
    rects.clear();
    services::menu_text::push_microfont_rainbow_menu_text(&mut rects,0,0,5,"AAAA");
    assert_eq!(rects.len(),22);
    assert!(rects.iter().all(|r|r.x==0&&r.width==5&&r.height==1&&r.color.a==128));
    assert_eq!(STATE.lock().unwrap().renders,1,"menus only copy retained tiles");
}
#[test] fn shrinking_r8_tiles_preserves_thin_strokes_and_partial_coverage() {
    // The old centre sample skipped source row 10 entirely at 21 -> 14 px.
    let mut pixels=vec![0;24*33*SLOT_COUNT];
    for x in 0..11 {pixels[(10+6)*24+x+6]=255;}
    let atlas=OwnedAlphaAtlas {em_height_px:21,line_width_px:2.75,glyph_width:24,
        glyph_height:33,gutter_px:6,pixels};
    let mask=atlas.glyph(' ').unwrap();
    let samples:Vec<_>=(0..14).map(|y|atlas.sample(mask,0,y,8,14)).collect();
    assert_eq!(&samples[6..8],&[85,85]);
    assert!(samples[..6].iter().chain(&samples[8..]).all(|&v|v==0));
}
#[test] fn connected_loops_get_one_closing_join_only() {
    use microfont::raster_line::{Point,Strip,Boundary::{End,Junction}};
    const POINTS:&[Point]=&[Point::new(0,2),Point::new(0,1),Point::new(1,1),Point::new(1,2),Point::new(0,2)];
    for (start,end,expected) in [(Junction,Junction,12),(End,End,9),(End,Junction,9),(Junction,End,9)] {
        let strip=Strip::new(POINTS,start,end);
        let mut vertices=Vec::new();
        services::microfont_mask_service::append_strip_joins(&mut vertices,0,&strip);
        assert_eq!(vertices.len(),expected);
        if expected==12 {assert_eq!(vertices[9][0] as u32,0|(2<<7));}
    }
}
#[test] fn percent_has_two_connected_circles_with_four_joins_each() {
    use microfont::raster_line::Boundary::Junction;
    let strips=microfont::raster_line::glyph_strips('%').unwrap();
    assert_eq!(strips.len(),3);
    for strip in strips.iter().skip(1) {
        assert_eq!((strip.start,strip.end),(Junction,Junction));
        let mut vertices=Vec::new();
        services::microfont_mask_service::append_strip_joins(&mut vertices,0,&strip);
        assert_eq!(vertices.len(),12);
    }
}
#[test] fn sharp_joins_use_bounded_round_sectors_only_above_270_degrees() {
    use microfont::raster_line::Point;
    for (triple,expected) in [
        ([Point::new(0,8),Point::new(2,0),Point::new(4,8)],12),
        ([Point::new(4,8),Point::new(2,0),Point::new(0,8)],12),
        ([Point::new(0,8),Point::new(0,0),Point::new(4,0)],3),
        ([Point::new(0,8),Point::new(0,2),Point::new(2,0)],3),
    ] {
        let mut vertices=Vec::new();services::microfont_mask_service::append_join(&mut vertices,0,&triple);
        assert_eq!(vertices.len(),expected);
        for triangle in vertices.chunks_exact(3) {
            if expected==12 {
                assert!(triangle.iter().all(|v|v[0] as u32 & 0x8000 !=0));
                assert!(triangle.iter().all(|v|v[1].abs()<=0.7072&&v[2].abs()<=0.7072));
                let a=triangle[1];let b=triangle[2];
                assert!(a[0]==b[0]);assert!(a[1]*b[2]-a[2]*b[1]<0.0);
            }
        }
    }
}
#[test] fn join_recipes_expand_to_the_same_corners_for_all_widths() {
    use microfont::raster_line::Point;
    let triple=[Point::new(0,8),Point::new(0,0),Point::new(4,0)];
    let mut vertices=Vec::new();services::microfont_mask_service::append_join(&mut vertices,0,&triple);
    assert_eq!(vertices.len(),3);assert!(vertices[0][1]<-0.5);
    for width in [7.0,8.0,9.0] {
        let points=triple.map(|p|[(p.x() as f32+0.5)*16.0,(p.y() as f32+0.5)*16.0]);
        let expected=services::microfont_mask_service::corner_triangle(points[0],points[1],points[2],width).unwrap();
        for (i,v) in vertices.iter().enumerate() {
            let code=v[0] as u32;let at=[((code&127) as f32+0.5)*16.0,((code>>7) as f32+0.5)*16.0];
            let p=if i==0 {at} else {[at[0]+v[1]*width,at[1]+v[2]*width]};
            for k in 0..2 {assert!((p[k]-expected[i][k]).abs()<0.0001);}
        }
    }
}
'''
def main():
    service=(ROOT/'src/r/services/microfont_mask_service.rs').read_text()
    # Tests access the private owner only to reset between simulated boots.
    service=service.replace('static ATLAS:', 'pub(crate) static ATLAS:').replace('fn append_join(', 'pub(crate) fn append_join(').replace('pub(super) fn append_strip_joins(', 'pub(crate) fn append_strip_joins(')
    service=service.replace('pub(super) fn corner_triangle(', 'pub(crate) fn corner_triangle(')
    service=service.replace('fn native_mask_geometry(', 'pub(crate) fn native_mask_geometry(')
    service=service.replace('fn rounded_quad_budget(', 'pub(crate) fn rounded_quad_budget(')
    service=service.replace('fn endpoint_params(', 'pub(crate) fn endpoint_params(')
    service=service.replace('    pixels: Vec<u8>,', '    pub(crate) pixels: Vec<u8>,')
    service=service.replace('static START_GLYPH:', 'pub(crate) static START_GLYPH:')
    service=service.replace('fn with_warmed_atlas<T>(', 'pub(crate) fn with_warmed_atlas<T>(')
    service=service.replace('static ALPHA_ATLAS: spin::Once<OwnedAlphaAtlas> = spin::Once::new();',
        'pub(crate) static ALPHA_ATLAS: crate::ResetAlpha = crate::ResetAlpha::new();')
    from test_microfont_coverage_batches import block
    slot4=(ROOT/'src/ui4/slot4_service.rs').read_text()
    menu_helpers='\n'.join(block(slot4, marker) for marker in (
        'fn push_microfont_menu_text(', 'fn push_microfont_rainbow_menu_text(',
        'fn menu_label_prefix', 'fn rainbow_color('))
    menu_helpers=menu_helpers.replace('fn push_microfont_', 'pub(crate) fn push_microfont_')
    menu_helpers=menu_helpers.replace('super::context_menu::', 'crate::ui4::context_menu::')
    menu_prelude = r'''
type Slot4Rects=Vec<crate::MenuRect>;
fn push_overlay_rect(out:&mut Slot4Rects,x:u32,y:u32,width:u32,height:u32,color:crate::graphics::primitives::Rgba8) {
    out.push(crate::MenuRect{x,y,width,height,color});
}
'''
    source=MOCKS+'\nmod services { use crate::shell3_font_service; pub mod microfont_mask_service { use crate::{libm,spin};\n'+service+'\n} pub mod menu_text {\n'+menu_prelude+menu_helpers+'\n}}\n'+TESTS
    with tempfile.TemporaryDirectory(prefix='microfont-mask-test-') as tmp:
        tmp=Path(tmp);lib=tmp/'libmicrofont.rlib'
        build_microfont(lib, features=('raster',))
        (tmp/'test.rs').write_text(source.replace('//!','//').replace('<-0.5','< -0.5'))
        subprocess.run(['rustc','--edition=2024','--test','-A','dead_code','-A','unused_imports','--extern',f'microfont={lib}',str(tmp/'test.rs'),'-o',str(tmp/'tests')],check=True)
        subprocess.run([str(tmp/'tests'),'--test-threads=1'],check=True)
if __name__=='__main__':main()
