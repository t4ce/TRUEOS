#!/usr/bin/env python3
"""Execute production atlas paint/cache ownership with mocked device boundaries."""
from pathlib import Path
import json
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, item, constant

MOCKS = r'''
#![allow(dead_code)]
extern crate alloc;
use std::sync::{LazyLock, Mutex};
#[derive(Clone,Copy,Default,PartialEq,Debug)]
enum Fault { #[default] None, Stale, Upload, Allocate, Busy, Complete, Fence,
    ReleaseMesh, EvictMask, Acquire, View, Copy, CpuMark, OpaqueMark, Publish }
#[derive(Default)]
struct State {
    fault: Fault, next: usize, tasks: usize, uploads: usize, allocations: usize,
    draws: usize, mesh_releases: usize, mesh_drops: usize, mask_releases: usize,
    mask_frees: usize, quarantines: usize, acquires: usize, copies: usize,
    cpu_marks: usize, opaque_marks: usize, publishes: usize, cancels: usize,
    events: Vec<&'static str>, logs: Vec<String>,
}
static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| Mutex::new(State::default()));
fn mutate<T>(f: impl FnOnce(&mut State)->T)->T { f(&mut STATE.lock().unwrap()) }
fn fault()->Fault { mutate(|s|s.fault) }
fn next()->usize { mutate(|s|{s.next+=1;s.next}) }
#[macro_export]
macro_rules! log_important {
    (target:$target:literal; $($args:tt)*) => { crate::mutate(|s|s.logs.push(format!($($args)*))) };
}
mod time { pub fn tsc_hz()->u64 { 1_000_000_000 } }
mod ui4 {
    #[derive(Clone,Copy,Debug)] pub struct FrameHandle(pub usize);
    impl FrameHandle { pub fn raw(self)->usize { self.0 } }
    #[derive(Clone,Copy)] pub struct Lease(pub usize);
    #[derive(Clone,Copy)] pub struct View;
    pub fn acquire_frame_buffer(frame:FrameHandle)->Result<Lease,()> {
        crate::mutate(|s|{s.acquires+=1;s.events.push("acquire");});
        if crate::fault()==crate::Fault::Acquire { Err(()) } else { Ok(Lease(frame.0)) }
    }
    pub fn writable_rgba_view(_:Lease)->Result<View,()> {
        if crate::fault()==crate::Fault::View {Err(())} else {Ok(View)}
    }
    pub fn mark_frame_buffer_cpu_authored(_:Lease)->Result<(),()> {
        crate::mutate(|s|{s.cpu_marks+=1;s.events.push("cpu-mark");});
        if crate::fault()==crate::Fault::CpuMark {Err(())} else {Ok(())}
    }
    pub fn mark_frame_buffer_fully_opaque(_:Lease)->Result<(),()> {
        crate::mutate(|s|{s.opaque_marks+=1;s.events.push("opaque-mark");});
        if crate::fault()==crate::Fault::OpaqueMark {Err(())} else {Ok(())}
    }
    pub fn publish_frame_buffer(_:Lease)->Result<(),()> {
        crate::mutate(|s|{s.publishes+=1;s.events.push("publish");});
        if crate::fault()==crate::Fault::Publish {Err(())} else {Ok(())}
    }
    pub fn cancel_frame_buffer(_:Lease)->Result<(),()> {
        crate::mutate(|s|{s.cancels+=1;s.events.push("cancel");}); Ok(())
    }
}
mod intel {
    pub mod shader {
        pub fn microfont_dista_seeds_current()->bool { crate::fault()!=crate::Fault::Stale }
        pub fn microfont_dista_artifact()->(u16,&'static str) {(0x4680,"host-test")}
    }
    pub mod render {
        use crate::{Fault,fault,mutate};
        pub struct Mesh;
        impl Drop for Mesh { fn drop(&mut self){mutate(|s|s.mesh_drops+=1);} }
        pub fn create_resident_triangle_mesh(_: &[[f32;3]], _: &[u32])->Result<Mesh,&'static str> {
            mutate(|s|{s.uploads+=1;s.events.push("upload");});
            if fault()==Fault::Upload {Err("mesh-upload")} else {Ok(Mesh)}
        }
        pub fn release_resident_triangle_mesh(_: &Mesh)->bool {
            mutate(|s|{s.mesh_releases+=1;s.events.push("mesh-release");}); fault()!=Fault::ReleaseMesh
        }
        #[derive(Clone,Copy,Debug)]
        pub struct Surface { pub width:u32,pub height:u32,pub bytes:usize,pub retired:bool,pub id:usize }
        pub struct ResidentDistaMask { surface:Surface }
        impl ResidentDistaMask { pub fn surface(&self)->Surface {self.surface} }
        impl Drop for ResidentDistaMask {
            fn drop(&mut self){mutate(|s|if self.surface.retired {s.mask_frees+=1;} else {s.quarantines+=1;});}
        }
        pub fn create_resident_dista_mask(width:u32,height:u32)->Result<ResidentDistaMask,&'static str> {
            mutate(|s|{s.allocations+=1;s.events.push("mask-allocate");});
            if fault()==Fault::Allocate {Err("mask-allocate")} else {
                Ok(ResidentDistaMask {surface:Surface {width,height,bytes:width as usize*height as usize,
                    retired:true,id:crate::next()}})
            }
        }
        pub fn release_resident_dista_mask(mask:ResidentDistaMask)->bool {
            mutate(|s|{s.mask_releases+=1;s.events.push("mask-release");});
            if fault()==Fault::EvictMask {core::mem::forget(mask);false} else {drop(mask);true}
        }
        pub struct FrameResult {
            pub gpu_timestamp:Option<u64>,pub gpu_target_ready_timestamp:Option<u64>,
            pub release_fence:Option<u64>,error:Option<&'static str>,
        }
        impl FrameResult {pub fn completion_error(&self)->Option<&'static str>{self.error}}
        pub fn render_resident_dista_mask(_: &Mesh,mask:&mut ResidentDistaMask)->Result<FrameResult,&'static str> {
            mutate(|s|{s.draws+=1;s.events.push("gpu-draw");});
            if fault()==Fault::Busy {return Err("render-busy");}
            mask.surface.retired=fault()!=Fault::Complete;
            Ok(FrameResult {gpu_timestamp:Some(10),gpu_target_ready_timestamp:Some(20),
                release_fence:if fault()==Fault::Fence {None}else{Some(1)},
                error:if fault()==Fault::Complete {Some("geometry-incomplete")}else{None}})
        }
    }
}
'''

SERVICES = r'''
pub mod microfont_raster_preview { pub type PaintResult=Result<usize,(&'static str,bool)>; }
pub mod microfont_dista_tasks {
    pub struct Tasks {pub descriptors:Vec<[f32;3]>,pub indices:Vec<u32>}
    pub fn build_atlas_segments(segments:usize,shape:microfont::FontShape,direct:bool)->Tasks {
        assert!(shape==microfont::FontShape::Meso && direct);
        crate::mutate(|s|{s.tasks+=1;s.events.push("tasks");});
        Tasks {descriptors:vec![[0.;3];4],indices:vec![0;segments*3]}
    }
}
pub mod microfont_dista_copy {
    pub struct Stats {pub output_pixels:usize,pub source_samples:usize,pub partial_coverage_pixels:usize}
    pub unsafe fn copy_retired_mask_to_rgba(source:crate::intel::render::Surface,_:crate::ui4::View,
        sample_scale:u32)->Result<Stats,&'static str> {
        assert!(source.retired); assert!(matches!(sample_scale,1|2));
        crate::mutate(|s|{s.copies+=1;s.events.push("cpu-copy");});
        if crate::fault()==crate::Fault::Copy {Err("copy-invalid")} else {
            Ok(Stats {output_pixels:source.width as usize*source.height as usize/(sample_scale*sample_scale) as usize,
                source_samples:source.width as usize*source.height as usize,partial_coverage_pixels:0})
        }
    }
}
'''

TESTS = r'''
#[cfg(test)]
mod tests {
    use super::*;
    fn reset(f:crate::Fault){crate::mutate(|s|*s=crate::State {fault:f,..crate::State::default()});}
    fn frame(n:usize)->FrameHandle {FrameHandle(n)}
    #[test]
    fn first_view_generates_once_then_copies_once_in_retirement_order() {
        reset(crate::Fault::None);let mut cache=AtlasCache::default();
        assert_eq!(paint_atlas(frame(1),Config::default(),&mut cache),Ok(16));
        assert_eq!(cache.entries.len(),1);
        crate::mutate(|s|{
            assert_eq!((s.tasks,s.uploads,s.allocations,s.draws,s.copies,s.publishes),(1,1,1,1,1,1));
            assert_eq!((s.mesh_releases,s.mesh_drops,s.mask_releases,s.mask_frees,s.cancels),(1,1,0,0,0));
            let position=|event|s.events.iter().position(|e|*e==event).unwrap();
            assert!(position("gpu-draw")<position("mesh-release"));
            assert!(position("mesh-release")<position("acquire"));
            assert!(position("acquire")<position("cpu-copy"));
            assert!(position("cpu-copy")<position("cpu-mark"));
            assert!(position("opaque-mark")<position("publish"));
            assert!(s.logs.last().unwrap().contains("cache_hit=false gpu_generations=1 cpu_copies=1"));
        });
        drop(cache);crate::mutate(|s|assert_eq!(s.mask_frees,1));
    }
    #[test]
    fn cache_hit_and_different_views_with_identical_source_do_zero_gpu_work() {
        reset(crate::Fault::None);let mut cache=AtlasCache::default();
        let mut big=Config::default();big.apply_key('4');
        let mut small_aa=big;small_aa.apply_key('3');small_aa.apply_key('a');
        assert_eq!(big.source_key(),small_aa.source_key());
        for (n,c) in [(1,big),(2,big),(3,small_aa)] {assert!(paint_atlas(frame(n),c,&mut cache).is_ok());}
        crate::mutate(|s|{
            assert_eq!((s.tasks,s.uploads,s.allocations,s.draws),(1,1,1,1));
            assert_eq!((s.copies,s.publishes,s.acquires),(3,3,3));
            assert_eq!(s.logs.iter().filter(|line|line.contains("cache_hit=true gpu_generations=0 cpu_copies=1")).count(),2);
        });
        assert_eq!(cache.entries.len(),1);
    }
    #[test]
    fn pre_submission_failures_never_acquire_or_copy_frame() {
        for (fault,error,releases,mask_releases) in [
            (crate::Fault::Stale,"seed-or-shape-bake-stale",0,0),
            (crate::Fault::Upload,"mesh-upload",0,0),
            (crate::Fault::Allocate,"mask-allocate",1,0),
            (crate::Fault::Busy,"render-busy",1,1),
        ] {
            reset(fault);let mut cache=AtlasCache::default();
            assert_eq!(paint_atlas(frame(1),Config::default(),&mut cache),Err((error,false)));
            crate::mutate(|s|{
                assert_eq!((s.acquires,s.copies,s.publishes,s.cancels),(0,0,0,0));
                assert_eq!((s.mesh_releases,s.mask_releases),(releases,mask_releases));
            });assert!(cache.entries.is_empty());
        }
    }
    #[test]
    fn uncertain_gpu_completion_pins_sources_and_leaves_rgba_frame_untouched() {
        reset(crate::Fault::Complete);let mut cache=AtlasCache::default();
        assert_eq!(paint_atlas(frame(1),Config::default(),&mut cache),Err(("geometry-incomplete",false)));
        assert!(cache.entries.is_empty());
        crate::mutate(|s|{
            assert_eq!((s.acquires,s.copies,s.publishes,s.cancels),(0,0,0,0));
            assert_eq!((s.mesh_releases,s.mesh_drops,s.mask_releases,s.mask_frees,s.quarantines),(0,0,0,0,1));
        });
        reset(crate::Fault::Fence);
        assert_eq!(paint_atlas(frame(1),Config::default(),&mut cache),Err(("mask-release-missing",false)));
        crate::mutate(|s|{
            assert_eq!((s.acquires,s.copies,s.mesh_releases,s.mesh_drops,s.mask_frees),(0,0,0,0,0));
        });
    }
    #[test]
    fn frame_backpressure_retains_completed_source_and_retry_only_copies() {
        reset(crate::Fault::Acquire);let mut cache=AtlasCache::default();
        assert_eq!(paint_atlas(frame(1),Config::default(),&mut cache),Err(("frame-busy",false)));
        assert_eq!(cache.entries.len(),1);
        crate::mutate(|s|s.fault=crate::Fault::None);
        assert!(paint_atlas(frame(1),Config::default(),&mut cache).is_ok());
        crate::mutate(|s|assert_eq!((s.draws,s.acquires,s.copies,s.publishes),(1,2,1,1)));
    }
    #[test]
    fn unpublished_destination_is_cancelled_on_copy_metadata_or_publication_failure() {
        for (fault,error,copies,publishes) in [
            (crate::Fault::View,"frame-view",0,0),
            (crate::Fault::Copy,"copy-invalid",1,0),
            (crate::Fault::CpuMark,"frame-cpu-authored",1,0),
            (crate::Fault::OpaqueMark,"frame-opaque",1,0),
            (crate::Fault::Publish,"frame-publish",1,1),
        ] {
            reset(fault);let mut cache=AtlasCache::default();
            assert_eq!(paint_atlas(frame(9),Config::default(),&mut cache),Err((error,false)));
            assert_eq!(cache.entries.len(),1);
            crate::mutate(|s|assert_eq!((s.acquires,s.copies,s.publishes,s.cancels),(1,copies,publishes,1)));
            crate::mutate(|s|s.fault=crate::Fault::None);
            assert!(paint_atlas(frame(10),Config::default(),&mut cache).is_ok());
            crate::mutate(|s|assert_eq!(s.draws,1));
        }
    }
    #[test]
    fn cache_evicts_oldest_retired_source_and_keeps_most_recent_hit() {
        reset(crate::Fault::None);let mut cache=AtlasCache::default();
        let mut config=Config::default();let mut configs=Vec::new();
        for _ in 0..MAX_CACHED_MASKS {configs.push(config);assert!(paint_atlas(frame(1),config,&mut cache).is_ok());
            if !config.aa {config.apply_key('a');}else{config.apply_key('a');config.apply_key('q');}}
        assert_eq!(cache.entries.len(),MAX_CACHED_MASKS);
        assert!(paint_atlas(frame(2),configs[0],&mut cache).is_ok());
        assert!(paint_atlas(frame(3),config,&mut cache).is_ok());
        assert_eq!(cache.entries.len(),MAX_CACHED_MASKS);
        assert!(cache.entries.iter().any(|entry|entry.key==configs[0].source_key()));
        assert!(!cache.entries.iter().any(|entry|entry.key==configs[1].source_key()));
        crate::mutate(|s|assert_eq!((s.draws,s.copies,s.mask_releases,s.mask_frees),(7,8,1,1)));
    }
}
'''


def main():
    path = "src/r/services/microfont_triangle_preview.rs"
    # AtlasCache is deliberately a compact one-line production declaration;
    # the shared item reader expects closing braces on their own lines.
    source = (ROOT / path).read_text()
    cache = re.search(r"^#\[derive\(Default\)\]\nstruct AtlasCache \{[^\n]*\}$", source, re.MULTILINE)
    if cache is None:
        raise ValueError("production AtlasCache declaration changed")
    production = "\n".join([constant(path, "MAX_CACHED_MASKS"),cache.group()]
        + [item(path, name) for name in ["CachedMask", "stamp", "elapsed_ns", "paint_atlas"]])
    with tempfile.TemporaryDirectory(prefix="trueos-dista-preview-tests-") as temporary:
        directory = Path(temporary)
        (directory / "src").mkdir()
        source = (MOCKS + "\nmod services {\n" + SERVICES
            + f'#[path={json.dumps(str(ROOT / "src/r/services/microfont_dista_view.rs"))}] mod microfont_dista_view;\n'
            + "mod preview { use alloc::vec::Vec; use crate::{intel::render,ui4::{self,FrameHandle}};\n"
            + "use super::{microfont_dista_tasks as tasks,microfont_dista_copy,microfont_dista_view::Config,microfont_raster_preview::PaintResult};\n"
            + production + TESTS + "\n}\n}\n")
        (directory / "src/lib.rs").write_text(source)
        (directory / "Cargo.toml").write_text(
            '[package]\nname="trueos-dista-preview-tests"\nversion="0.0.0"\nedition="2024"\n'
            '[dependencies]\nlibm="0.2"\n'
            f'microfont={{path={json.dumps(str(ROOT / "vendor/microfont"))},default-features=false}}\n')
        subprocess.run(["cargo", "test", "--offline", "--manifest-path", str(directory / "Cargo.toml"),
                        "--target", "x86_64-unknown-linux-gnu", "--quiet", "--", "--test-threads=1"],
                       cwd=directory, check=True)


if __name__ == "__main__":
    main()
