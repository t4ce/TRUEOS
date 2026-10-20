#!/usr/bin/env python3
"""Run the production retained-atlas service with mocked GPU/frame boundaries."""
from pathlib import Path
import subprocess, tempfile
ROOT=Path(__file__).resolve().parents[2]
MOCKS=r'''
extern crate alloc;
use std::sync::Mutex;
mod libm { pub fn ceilf(v:f32)->f32 {v.ceil()} pub fn sqrtf(v:f32)->f32 {v.sqrt()} pub fn roundf(v:f32)->f32 {v.round()} }
mod spin {
    pub struct Mutex<T>(std::sync::Mutex<T>);
    impl<T> Mutex<T> { pub const fn new(v:T)->Self {Self(std::sync::Mutex::new(v))}
        pub fn try_lock(&self)->Option<std::sync::MutexGuard<'_,T>> {self.0.try_lock().ok()}
    }
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
mod phys { pub fn phys_to_virt(p:usize)->usize {p} }
mod ui4 {
    #[derive(Clone,Copy)] pub struct FrameHandle;
    pub struct View {pub width:u32,pub height:u32,pub pitch:u32,pub virt:*mut u8}
    pub fn acquire_frame_buffer(_:FrameHandle)->Result<(),()> {Ok(())}
    pub fn writable_rgba_view(_:())->Result<View,()> {
        let mut s=crate::STATE.lock().unwrap();s.dest.resize(192*176*4,0);
        Ok(View {width:192,height:176,pitch:192*4,virt:s.dest.as_mut_ptr()})
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
        #[derive(Clone,Copy)] pub struct GpgpuMask8Surface {
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
                if d.clip_adjacency {assert_eq!(d.line_width_px,f[13]);assert_eq!(f[8],f[13]/2.0);}
                s.params.push(p);
            }
            if s.fault==2 {return Err("render-busy");}
            let scale=mask.width/96;
            // Synthetic coverage tests the actual copy/tint sink independently
            // of hardware rasterization, which is exercised on the rig.
            let slot=('A' as usize)-32;
            for y in 0..11*scale { for x in 0..6*scale {
                let px=(slot%16) as u32*6*scale+x;let py=(slot/16) as u32*11*scale+y;
                mask.data[(py*mask.width+px) as usize]=128;
            }}
            Ok(ResultFrame {release_fence:(s.fault!=4).then_some(1),gpu_timestamp:None,gpu_target_ready_timestamp:None})
        }
    }
}
'''
TESTS=r'''
use services::microfont_mask_service::*;
fn reset(fault:u8) {
    let old=services::microfont_mask_service::ATLAS.try_lock().unwrap().take();drop(old);
    *STATE.lock().unwrap()=State {fault,..State::default()};
}
fn expected_lines()->usize {(0x20u8..=0x7e).filter_map(|c|microfont::raster_line::glyph_strips(c as char)).map(|s|s.len()).sum()}
#[test] fn maps_all_three_ranges_without_collisions() {
    assert_eq!(SLOT_COUNT,255);assert_eq!(glyph_slot(' '),Some(0));assert_eq!(glyph_slot('~'),Some(94));
    assert_eq!(glyph_slot('─'),Some(95));assert_eq!(glyph_slot('\u{257f}'),Some(222));
    assert_eq!(glyph_slot('▀'),Some(223));assert_eq!(glyph_slot('\u{259f}'),Some(254));assert_eq!(glyph_slot('漢'),None);
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
    for width in [7.0,9.0,8.0] {with_warmed_atlas(AtlasConfig {line_width_px:width,..AtlasConfig::default()},|_|Ok(())).unwrap();}
    let s=STATE.lock().unwrap();assert_eq!(s.uploads,uploads);assert_eq!(s.vertices,vertices);
    assert_eq!((s.renders,s.allocations,s.releases),(4,1,0));
}
#[test] fn fixed_endpoint_metadata_is_never_uploaded_again() {
    reset(0);with_warmed_atlas(AtlasConfig::default(),|_|Ok(())).unwrap();
    let (written,before)={let s=STATE.lock().unwrap();(s.constant_writes.len(),s.params.clone())};
    with_warmed_atlas(AtlasConfig {line_width_px:9.0,..AtlasConfig::default()},|_|Ok(())).unwrap();
    let s=STATE.lock().unwrap();assert_eq!(s.constant_allocations,1);
    assert!(s.constant_writes[written..].iter().all(|&(offset,len)|len==4 && matches!(offset%64,32|36|48|52|56|60)));
    for (old,new) in before.iter().zip(&s.params) {
        assert_eq!(&old[..8],&new[..8]);assert_eq!(&old[10..12],&new[10..12]);
    }
}
#[test] fn size_replaces_mask_but_keeps_every_geometry_buffer() {
    reset(0);with_warmed_atlas(AtlasConfig::default(),|_|Ok(())).unwrap();
    let uploads=STATE.lock().unwrap().uploads;
    with_warmed_atlas(AtlasConfig {glyph_scale:8,line_width_px:4.0,..AtlasConfig::default()},|a| {assert_eq!((a.surface().width,a.surface().height),(768,1408));Ok(())}).unwrap();
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
#[test] fn physical_sizes_reuse_source_geometry_and_keep_native_width() {
    reset(0);
    let mut uploads = 0;
    for height in [19,38,57,76,94,113,132,151,19] {
        let config = AtlasConfig { glyph_height_px:Some(height), ..AtlasConfig::default() };
        with_warmed_atlas(config,|atlas| {
            assert_eq!(atlas.config().line_width_px,8.0);
            let (w,h) = atlas.config().cell_extent();
            assert_eq!(h,height+12); // six native pixels of overhang room per side
            assert_eq!(w,(height*6+10)/11+12);
            assert_eq!(atlas.glyph_rect('A').unwrap()[2..],[w,h]);

            assert_eq!((atlas.surface().width,atlas.surface().height),(16*w,16*h));
            Ok(())
        }).unwrap();
        let state=STATE.lock().unwrap();
        if uploads==0 {uploads=state.uploads;}
        assert_eq!(state.uploads,uploads);
        assert!(state.params.iter().all(|p| f32::from_bits(p[9])==height as f32));
        assert!(state.params.iter().all(|p| (f32::from_bits(p[12])*11.0-height as f32).abs()<0.0001));
        assert!(state.params.iter().all(|p| p[8]!=0)); // box/block path unchanged, excluded here
    }
}
#[test] fn busy_retry_keeps_uploaded_geometry() {
    reset(2);assert_eq!(with_warmed_atlas(AtlasConfig::default(),|_|Ok(())),Err("render-busy"));
    let uploads=STATE.lock().unwrap().uploads;STATE.lock().unwrap().fault=0;
    with_warmed_atlas(AtlasConfig::default(),|_|Ok(())).unwrap();
    let s=STATE.lock().unwrap();assert_eq!(s.uploads,uploads);assert_eq!((s.allocations,s.releases),(1,0));
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
    reset(0);for (scale,width) in [(0,8.0),(24,8.0),(16,f32::NAN),(4,8.0)] {
        assert!(with_warmed_atlas(AtlasConfig {glyph_scale:scale,line_width_px:width,..AtlasConfig::default()},|_|Ok(())).is_err());
    }
    assert_eq!(STATE.lock().unwrap().uploads,0);
    for width in [8.0,8.001] {with_warmed_atlas(AtlasConfig {line_width_px:width,..AtlasConfig::default()},|_|Ok(())).unwrap();}
    assert_eq!(STATE.lock().unwrap().renders,1);
}
#[test] fn colored_transparent_stamp_premultiplies_each_ink_once() {
    reset(0);let cfg=AtlasConfig::default();
    stamp_preview_colored(ui4::FrameHandle,cfg,&['A','A'],2,16,&[[255,0,0,255],[0,0,255,128]],[0;4]).unwrap();
    let s=STATE.lock().unwrap();
    assert_eq!(&s.dest[..4],&[128,0,0,128]);
    assert_eq!(&s.dest[96*4..96*4+4],&[0,0,64,64]);
    assert_eq!((s.renders,s.published),(1,1));
    assert!(s.dest.chunks_exact(4).all(|p|p[0]<=p[3]&&p[1]<=p[3]&&p[2]<=p[3]));
}
#[test] fn real_stamp_applies_color_opacity_and_blank_cells_without_redrawing() {
    reset(0);let cfg=AtlasConfig::default();
    stamp_preview(ui4::FrameHandle,cfg,&['A',' '],2,16,[200,100,50,128],[0;4]).unwrap();
    {let s=STATE.lock().unwrap();assert_eq!(&s.dest[..4],&[50,25,13,64]);assert_eq!(&s.dest[96*4..96*4+4],&[0;4]);}
    let uploads=STATE.lock().unwrap().uploads;
    stamp_preview(ui4::FrameHandle,cfg,&['A',' '],2,16,[0,255,0,255],[0;4]).unwrap();
    let s=STATE.lock().unwrap();assert_eq!(&s.dest[..4],&[0,128,0,128]);assert_eq!((s.renders,s.published,s.uploads),(1,2,uploads));
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
    for strip in &strips[1..] {
        assert_eq!((strip.start,strip.end),(Junction,Junction));
        let mut vertices=Vec::new();
        services::microfont_mask_service::append_strip_joins(&mut vertices,0,strip);
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
    subprocess.run(['python3',str(ROOT/'vendor/microfont/tools/generate_line_glyphs.py')],check=True)
    service=(ROOT/'src/r/services/microfont_mask_service.rs').read_text()
    # Tests access the private owner only to reset between simulated boots.
    service=service.replace('static ATLAS:', 'pub(crate) static ATLAS:').replace('fn append_join(', 'pub(crate) fn append_join(').replace('pub(super) fn append_strip_joins(', 'pub(crate) fn append_strip_joins(')
    service=service.replace('pub(super) fn corner_triangle(', 'pub(crate) fn corner_triangle(')
    preview=(ROOT/'src/r/services/microfont_line_preview.rs').read_text()
    helpers=preview[preview.index('fn glyph_height_pixels('):preview.index('type PaintResult')]
    helper_tests=r'''
const COLUMNS:usize=13;
#[test] fn tab_cycles_all_eight_sizes_and_wraps() {
    let mut step=1;
    for expected in [2,3,4,5,6,7,8,1] {step=size_after_tabs(step,1);assert_eq!(step,expected);}
    assert_eq!(size_after_tabs(3,16),3);
    assert_eq!(size_after_tabs(1,9),2);
}
#[test] fn physical_em_uses_gridpaper_conversion_and_keeps_wheel_layout_stable() {
    for step in 1..=8 {
        let expected=(1440*u32::from(step)*5+170)/340;
        assert_eq!(glyph_height_pixels(step),Ok(expected));
        let base=frame_extent(expected,8.0);
        assert_eq!(base,(13*((expected*6+10)/11+12),6*(expected+12)));
        for width in [7.0,7.25,8.0,8.25,9.0] {assert_eq!(frame_extent(expected,width),base);}
    }
}
'''
    mocks=MOCKS.replace('mod intel {','mod intel { pub fn physical_extent_pixels(w:u32,h:u32)->Option<(u32,u32)> {Some(((2560*w+300)/600,(1440*h+170)/340))}')
    source=mocks+'\nmod services { pub mod microfont_mask_service { use crate::{libm,spin};\n'+service+'\n} mod preview_size {\n'+helpers+helper_tests+'\n}}\n'+TESTS
    with tempfile.TemporaryDirectory(prefix='microfont-mask-test-') as tmp:
        tmp=Path(tmp);lib=tmp/'libmicrofont.rlib'
        subprocess.run(['rustc','--edition=2024','--crate-name','microfont','--crate-type','rlib','--cfg','feature="raster"',str(ROOT/'vendor/microfont/src/lib.rs'),'-o',str(lib)],check=True)
        (tmp/'test.rs').write_text(source.replace('//!','//').replace('<-0.5','< -0.5'))
        subprocess.run(['rustc','--edition=2024','--test','-A','dead_code','-A','unused_imports','--extern',f'microfont={lib}',str(tmp/'test.rs'),'-o',str(tmp/'tests')],check=True)
        subprocess.run([str(tmp/'tests'),'--test-threads=1'],check=True)
if __name__=='__main__':main()
