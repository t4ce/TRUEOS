#!/usr/bin/env python3
"""Exercise the actual Edit render method across two-buffer publication races."""

from pathlib import Path
import argparse
import subprocess
import tempfile

from test_edit_life_native import function


ROOT = Path(__file__).resolve().parents[2]

FIXTURE = r'''
#![allow(dead_code, unused_variables)]
use std::sync::{Arc,Mutex,atomic::{AtomicBool,AtomicUsize,Ordering}};
#[macro_export] macro_rules! log_error { ($($tokens:tt)*) => {}; }
struct SpinMutex<T>(Mutex<T>);
impl<T> SpinMutex<T> {
    const fn new(value:T)->Self {Self(Mutex::new(value))}
    fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
#[derive(Clone,Copy,PartialEq)] struct WindowId(u32);
impl WindowId {fn raw(self)->u32 {self.0}}
type FrameHandle=u32;
const OWNER:()=();
struct DamageRect;
impl DamageRect {const FULL:Self=Self;}
#[derive(Clone,Copy)] struct PixelRect {x:u32,y:u32,width:u32,height:u32}
impl PixelRect {fn contains(self,x:u32,y:u32)->bool {x>=self.x&&y>=self.y&&x-self.x<self.width&&y-self.y<self.height}}
struct Page {width:u32,height:u32,viewport:PixelRect}
#[derive(Clone)] struct Registration {
    window:WindowId,source:FrameHandle,page:Arc<Page>,serial:u64,
    displayed:Option<FrameHandle>,paused:bool,failed:bool,
}
static PAGES:SpinMutex<Vec<Registration>>=SpinMutex::new(Vec::new());
static HOOK:AtomicUsize=AtomicUsize::new(0);
static GPU_FAILURE:AtomicUsize=AtomicUsize::new(0);
static FRAME_FAILURE:AtomicBool=AtomicBool::new(false);
static BROKER_FAILURE:AtomicBool=AtomicBool::new(false);
static BAD_EXTENT:AtomicBool=AtomicBool::new(false);
static CHECK_INPUT_LOCK:bool=true;
#[derive(Clone,Copy)] struct FrameWriteLease {index:usize}
#[derive(Clone,Copy)] struct FrameReadLease;
struct FrameView {phys:u64,gpu:u64,virt:*mut u8,byte_len:usize,width:u32,height:u32,pitch:u32}
struct Placement {width:u32,height:u32}
#[derive(Default)] struct Pool {
    front:usize,displayed:usize,acquired:[bool;2],readers:[usize;2],source_readers:usize,
    front_publishes:usize,broker_publishes:usize,cancels:usize,
}
static POOL:SpinMutex<Pool>=SpinMutex::new(Pool {
    front:0,displayed:0,acquired:[false;2],readers:[1,0],source_readers:0,
    front_publishes:0,broker_publishes:0,cancels:0,
});
fn interleave() {
    let hook=HOOK.swap(0,Ordering::SeqCst);
    if hook==0 {return;}
    // The actual renderer must not lock page input while GPU work runs.
    let mut pages=PAGES.0.try_lock().expect("input lock held during GPU/CPU authoring");
    let current=&mut pages[0];
    current.serial+=1;
    match hook {1=>{},2=>current.source=11,3=>current.paused=true,4=>current.failed=true,_=>panic!()}
}
@ACTIVE@
fn acquire_published_frame(_:FrameHandle)->Result<FrameReadLease,()> {
    POOL.lock().source_readers+=1;Ok(FrameReadLease)
}
fn release_published_frame(_:FrameReadLease)->Result<(),()> {
    POOL.lock().source_readers-=1;Ok(())
}
fn acquire_frame_buffer(_:FrameHandle)->Result<FrameWriteLease,()> {
    let mut pool=POOL.lock();
    let index=(0..2).find(|&i|i!=pool.front&&!pool.acquired[i]&&pool.readers[i]==0).ok_or(())?;
    pool.acquired[index]=true;Ok(FrameWriteLease{index})
}
fn cancel_frame_buffer(lease:FrameWriteLease)->Result<(),()> {
    let mut pool=POOL.lock();
    if !pool.acquired[lease.index] {return Err(());}
    pool.acquired[lease.index]=false;pool.cancels+=1;Ok(())
}
fn view()->FrameView {
    let pixels=Box::leak(Box::new([0u32;32]));
    FrameView{phys:0x1000,gpu:0x1000,virt:pixels.as_mut_ptr().cast(),byte_len:128,width:8,height:4,pitch:32}
}
fn published_rgba_view(_:FrameReadLease)->Result<FrameView,()> {Ok(view())}
fn writable_rgba_view(_:FrameWriteLease)->Result<FrameView,()> {Ok(view())}
fn mark_frame_buffer_cpu_authored(_:FrameWriteLease)->Result<(),()> {interleave();Ok(())}
fn gpgpu_rgba_surface(_:FrameWriteLease)->Result<intel::gpgpu::GpgpuRgba8Surface,()> {Ok(intel::gpgpu::GpgpuRgba8Surface)}
fn publish_frame_buffer(lease:FrameWriteLease)->Result<(),()> {
    if CHECK_INPUT_LOCK {assert!(PAGES.0.try_lock().is_err(),"front publication must hold input lock");}
    if FRAME_FAILURE.load(Ordering::SeqCst) {return Err(());}
    let mut pool=POOL.lock();
    assert!(pool.acquired[lease.index]);
    pool.front=lease.index;pool.acquired[lease.index]=false;pool.front_publishes+=1;Ok(())
}
fn publish_gpgpu_frame_buffer(lease:FrameWriteLease,_:u32)->Result<(),()> {publish_frame_buffer(lease)}
fn publish_window_frame(_:(),_:WindowId,_:DamageRect)->Result<u64,()> {
    if CHECK_INPUT_LOCK {assert!(PAGES.0.try_lock().is_err(),"broker publication must hold input lock");}
    if BROKER_FAILURE.load(Ordering::SeqCst) {return Err(());}
    let mut pool=POOL.lock();
    let old=pool.displayed;let new=pool.front;
    pool.readers[old]-=1;pool.readers[new]+=1;pool.displayed=new;pool.broker_publishes+=1;Ok(1)
}
fn window_resize_state(_:(),_:WindowId)->Result<(Placement,u64),()> {
    Ok((Placement{width:if BAD_EXTENT.load(Ordering::SeqCst){9}else{8},height:4},1))
}
fn commit_window_frame_replacement(owner:(),window:WindowId,_:FrameHandle,
    _:Placement,_:u64,damage:DamageRect)->Result<u64,()> {publish_window_frame(owner,window,damage)}
mod emulator_paint {pub fn begin(_:super::FrameWriteLease){}}
mod virtio_gpu_logo {pub fn output_dimensions()->Option<(u32,u32)> {None}}
mod intel {
    pub fn dma_flush(_: *const u8,_:usize){}
    pub mod gpgpu {
        pub struct GpgpuRgba8Surface;
        impl GpgpuRgba8Surface {
            pub fn new(_:u64,_:u64,_:usize,_:u32,_:u32,_:u32)->Option<Self> {Some(Self)}
        }
        pub struct Native;
        pub struct Outcome {pub ok:bool,pub submitted:bool,pub release:Option<u32>}
        impl Native {
            pub fn render(&mut self,_:GpgpuRgba8Surface,_:GpgpuRgba8Surface,_:bool,_:bool,_:u32)->Outcome {
                super::super::interleave();
                let failure=super::super::GPU_FAILURE.load(super::super::Ordering::SeqCst);
                Outcome{ok:failure==0,submitted:failure!=1,release:(failure==0).then_some(7)}
            }
        }
    }
}
mod effects {
use super::*;
struct Life;
impl Life {fn hex_pixel(&self,_:u32,_:u32)->[u8;4] {[0,0,0,153]}}
enum Backend {Reference(Life),Native(intel::gpgpu::Native)}
struct Effect {
    input:Registration,backend:Backend,frame:FrameHandle,reset:bool,emit:bool,steps:u32,
    quarantined:bool,disabled:bool,
}
impl Effect {
@RENDER@
}
fn setup(native:bool,displayed:bool)->Effect {
    HOOK.store(0,Ordering::SeqCst);GPU_FAILURE.store(0,Ordering::SeqCst);
    FRAME_FAILURE.store(false,Ordering::SeqCst);BROKER_FAILURE.store(false,Ordering::SeqCst);
    BAD_EXTENT.store(false,Ordering::SeqCst);
    *POOL.lock()=Pool{readers:[1,0],..Pool::default()};
    let input=Registration{window:WindowId(1),source:10,
        page:Arc::new(Page{width:8,height:4,viewport:PixelRect{x:1,y:1,width:6,height:2}}),
        serial:1,displayed:displayed.then_some(90),paused:false,failed:false};
    *PAGES.lock()=vec![input.clone()];
    Effect{input,backend:if native{Backend::Native(intel::gpgpu::Native)}else{Backend::Reference(Life)},
        frame:90,reset:true,emit:true,steps:1,quarantined:false,disabled:false}
}
#[test]
fn stale_input_after_authoring_never_flips_front_or_strands_a_reader() {
    for native in [false,true] {
        for hook in 1..=4 {
            let mut effect=setup(native,true);
            HOOK.store(hook,Ordering::SeqCst);
            assert!(effect.render().is_err());
            assert!(!effect.disabled&&!effect.quarantined,"stale input is not a GPU failure");
            {
                let pool=POOL.lock();
                assert_eq!(pool.front,0,"stale output must never become FramePool front");
                assert_eq!(pool.readers,[1,0]);assert_eq!(pool.acquired,[false,false]);
                assert_eq!(pool.front_publishes,0);assert_eq!(pool.broker_publishes,0);
                assert_eq!(pool.source_readers,0);assert_eq!(pool.cancels,1);
            }
            let write=acquire_frame_buffer(90).expect("cancelled stale buffer must be reusable");
            cancel_frame_buffer(write).unwrap();
            {
                let mut pages=PAGES.lock();
                // Active cursor/source updates require no additional input;
                // paused/failed mutations are made active only for this next
                // independent publication check.
                pages[0].paused=false;pages[0].failed=false;
                effect.input=pages[0].clone();
            }
            assert!(effect.render().is_ok(),"fresh input must publish without an unrelated click");
            assert_eq!(POOL.lock().front,1);assert_eq!(POOL.lock().broker_publishes,1);
            assert!(acquire_frame_buffer(90).is_ok(),"display handoff frees the old back buffer");
        }
    }
}
#[test]
fn replacement_extent_is_checked_before_front_publication() {
    let mut effect=setup(true,false);BAD_EXTENT.store(true,Ordering::SeqCst);
    assert!(effect.render().is_err());assert!(!effect.disabled&&!effect.quarantined);
    assert_eq!(POOL.lock().front_publishes,0);assert_eq!(POOL.lock().front,0);
    assert_eq!(POOL.lock().acquired,[false,false]);
}
#[test]
fn broker_failure_after_flip_latches_instead_of_busy_retrying() {
    for replacing in [false,true] {
        let mut effect=setup(true,!replacing);BROKER_FAILURE.store(true,Ordering::SeqCst);
        assert!(effect.render().is_err());assert!(effect.disabled&&!effect.quarantined);
        assert_eq!(POOL.lock().front,1);assert_eq!(POOL.lock().front_publishes,1);
        assert_eq!(POOL.lock().broker_publishes,0);assert_eq!(POOL.lock().source_readers,0);
        assert!(acquire_frame_buffer(90).is_err(),"this is precisely the consumed-lease case requiring stop");
    }
}
#[test]
fn gpu_uncertainty_still_retains_both_exact_leases() {
    let mut effect=setup(true,true);GPU_FAILURE.store(2,Ordering::SeqCst);
    assert!(effect.render().is_err());assert!(effect.disabled&&effect.quarantined);
    assert_eq!(POOL.lock().source_readers,1);assert_eq!(POOL.lock().acquired,[false,true]);
    assert_eq!(POOL.lock().front_publishes,0);assert_eq!(POOL.lock().cancels,0);
}
#[test]
fn clean_native_failure_cancels_and_native_publication_failure_latches() {
    let mut effect=setup(true,true);GPU_FAILURE.store(1,Ordering::SeqCst);
    assert!(effect.render().is_err());assert!(effect.disabled&&!effect.quarantined);
    assert_eq!(POOL.lock().source_readers,0);assert_eq!(POOL.lock().acquired,[false,false]);
    let mut effect=setup(true,true);FRAME_FAILURE.store(true,Ordering::SeqCst);
    assert!(effect.render().is_err());assert!(effect.disabled&&!effect.quarantined);
    assert_eq!(POOL.lock().front,0);assert_eq!(POOL.lock().source_readers,0);
    assert_eq!(POOL.lock().acquired,[false,false]);
}
#[test]
fn transient_busy_does_not_disable_or_consume_a_source_reader() {
    let mut effect=setup(true,true);POOL.lock().readers[1]=1;
    assert!(effect.render().is_err());assert!(!effect.disabled&&!effect.quarantined);
    assert_eq!(POOL.lock().source_readers,0);assert_eq!(POOL.lock().front_publishes,0);
}
}
'''


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before-source", type=Path, help="Optionally prove an earlier UI4 source fails the stale-front regression")
    args = parser.parse_args()
    source = (ROOT / "src/ui4/edit_life.rs").read_text()
    fixture = FIXTURE.replace("@ACTIVE@", function(source, "input_is_active"))
    fixture = fixture.replace("@RENDER@", function(source, "render"))
    with tempfile.TemporaryDirectory(prefix="trueos-edit-life-publication-") as temporary:
        path = Path(temporary) / "publication.rs"
        binary = Path(temporary) / "publication"
        path.write_text(fixture)
        subprocess.run(["rustc", "--edition=2024", "--test", str(path), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)
        if args.before_source:
            before_source = args.before_source.read_text()
            before = FIXTURE.replace("@ACTIVE@", function(before_source, "input_is_active"))
            before = before.replace("@RENDER@", function(before_source, "render"))
            # Let the earlier renderer change its front so the behavioral
            # assertion, rather than the stronger lock assertion, catches it.
            before = before.replace("static CHECK_INPUT_LOCK:bool=true;", "static CHECK_INPUT_LOCK:bool=false;")
            path.write_text(before)
            subprocess.run(["rustc", "--edition=2024", "--test", str(path), "-o", str(binary)], check=True)
            result = subprocess.run([str(binary), "--exact",
                "effects::stale_input_after_authoring_never_flips_front_or_strands_a_reader",
                "--test-threads=1"], text=True, capture_output=True)
            output = result.stdout + result.stderr
            if result.returncode == 0 or "stale output must never become FramePool front" not in output:
                raise AssertionError("Earlier source did not fail the intended stale-front assertion:\n" + output)
            print("Earlier source fails the stale-front assertion; current source passes all publication tests.")


if __name__ == "__main__":
    main()
