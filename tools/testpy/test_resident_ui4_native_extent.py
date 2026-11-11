#!/usr/bin/env python3
"""Exercise production native frame admission, aliases, and depth lifetimes."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item

ROOT = Path(__file__).resolve().parents[2]
C = "src/intel/render/constants.rs"
P = "src/intel/render/primary.rs"
PC = "src/intel/render/picasso_carrier.rs"
source = r'''
#![allow(dead_code)]
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
#[macro_export] macro_rules! log_info {($($tt:tt)*)=>{()}}
#[macro_export] macro_rules! log_warn {($($tt:tt)*)=>{()}}
#[macro_export] macro_rules! log_error {($($tt:tt)*)=>{()}}
static NATIVE:AtomicBool=AtomicBool::new(false);
static FAIL_MAP:AtomicBool=AtomicBool::new(false);
static FAIL_UNMAP:AtomicBool=AtomicBool::new(false);
static ALLOCS:Mutex<Vec<(usize,usize)>>=Mutex::new(Vec::new());
static FREES:Mutex<Vec<(usize,usize)>>=Mutex::new(Vec::new());
static MAPS:Mutex<Vec<(u64,u64,usize)>>=Mutex::new(Vec::new());
static UNMAPS:Mutex<Vec<(u64,usize)>>=Mutex::new(Vec::new());
mod intel {
    pub const WARM_ALIGN:usize=4096;
    pub fn align_up(v:usize,a:usize)->Option<usize> {v.checked_add(a-1).map(|v|v&!(a-1))}
    pub mod tgl_native_panel {pub fn native_scanout_ready()->bool {
        crate::NATIVE.load(crate::Ordering::Acquire)
    }}
    pub mod gpgpu {
        #[derive(Clone,Copy)]pub struct GpgpuRgba8Surface {
            pub phys:u64,pub bytes:usize,pub width:u32,pub height:u32,pub pitch_bytes:u32
        }
        impl GpgpuRgba8Surface {pub fn is_valid(self)->bool {
            self.phys%4096==0 && self.width!=0 && self.height!=0 &&
            self.pitch_bytes>=self.width*4 && self.bytes>=self.pitch_bytes as usize*self.height as usize
        }}
    }
}
mod dma {
    pub fn alloc_ppgtt(bytes:usize,_align:usize)->Option<(u64,*mut u8)> {
        // These functions only validate allocation identity and bounds; no
        // fixture pointer is dereferenced, so allocating 32 MiB is unnecessary.
        let id=0x1000_0000+crate::ALLOCS.lock().len()*0x0400_0000;
        crate::ALLOCS.lock().push((id,bytes));Some((id as u64,id as *mut u8))
    }
    pub fn dealloc(ptr:*mut u8,bytes:usize) {crate::FREES.lock().push((ptr as usize,bytes));}
}
mod gpu {pub mod physical {
    pub fn physical_device()->Option<&'static dyn crate::PhysicalGpuDevice> {Some(&crate::PHYSICAL)}
}}
fn map_render_ppgtt_range(gpu:u64,phys:u64,bytes:usize)->bool {
    MAPS.lock().push((gpu,phys,bytes));!FAIL_MAP.load(Ordering::Acquire)
}
fn map_render_ppgtt_scanout_range(gpu:u64,phys:u64,bytes:usize)->bool {
    map_render_ppgtt_range(gpu,phys,bytes)
}
fn unmap_render_ppgtt_range(gpu:u64,bytes:usize)->bool {
    UNMAPS.lock().push((gpu,bytes));!FAIL_UNMAP.load(Ordering::Acquire)
}
fn device_is_gfx12(d:u16)->bool {matches!(d,0x9a49|0x4680|0xa780)}
fn device_is_gfx125(d:u16)->bool {d==0xa780}
const COMPARE_FUNCTION_LEQUAL:u8=4;
#[derive(Debug)]struct TriangleDepthConfig {
    gpu_addr:u64,hiz:Option<hiz::Layout>,hiz_clear:bool,pitch_bytes:u32,
    width:u32,height:u32,qpitch_rows_div4:u32,write_enabled:bool,compare_function:u8
}
'''
source += f'#[path="{ROOT / "src/intel/render/hiz.rs"}"] mod hiz;\n'
for name in (
    "RESIDENT_SCENE_TARGET_WIDTH", "RESIDENT_SCENE_TARGET_HEIGHT",
    "RESIDENT_SCENE_DIRECT_TARGET_WIDTH", "RESIDENT_SCENE_DIRECT_TARGET_HEIGHT",
    "WARM_STREAMOUT_BYTES", "RESIDENT_SCENE_DEPTH_TILE_WIDTH_BYTES",
    "RESIDENT_SCENE_DEPTH_TILE_HEIGHT_ROWS", "RESIDENT_SCENE_DEPTH_MAIN_BYTES",
    "RESIDENT_SCENE_DEPTH_BYTES", "GPU_VA_RESIDENT_SCENE_DEPTH_BASE",
    "GPU_VA_RESIDENT_UI4_FRAME_BASE", "GPU_VA_RESIDENT_UI4_FRAME_STRIDE",
    "RESIDENT_UI4_DIRECT_MAPPING_COUNT", "GPU_VA_RESIDENT_UI4_FRAME_LIMIT",
    "GPU_VA_RESIDENT_UI4_LARGE_FRAME_BASE", "GPU_VA_RESIDENT_UI4_LARGE_FRAME_STRIDE",
    "GPU_VA_RESIDENT_UI4_LARGE_FRAME_LIMIT", "GPU_VA_RESIDENT_SCENE_LARGE_DEPTH_BASE",
    "RESIDENT_SCENE_LARGE_DEPTH_BYTES", "GPU_VA_PERSISTENT_SPILL_LIMIT",
):
    source += constant(C, name) + "\n"
for name in (
    "ResidentSceneDepthAllocation", "ResidentSceneDirectUi4Mapping",
    "ui4_frame_extent_limit", "resident_scene_target_extent_supported",
    "resident_scene_legacy_extent", "resident_scene_depth_layout",
    "resident_scene_direct_ui4_gpu_for_slot", "resident_scene_direct_ui4_mapping_slot",
    "resident_scene_direct_ui4_vacant_slot", "prepare_resident_scene_direct_ui4_target",
    "release_resident_scene_direct_ui4_target", "release_render0_resident_scene_direct_ui4_target",
    "prepare_resident_scene_depth",
    "resident_scene_direct_ui4_mapping_tests",
):
    source += item(P, name) + "\n"
source += item("src/intel/render/drawable_depth.rs", "drawable_depth_bytes")
source += r'''
unsafe impl Send for ResidentSceneDepthAllocation {}
static RESIDENT_SCENE_DEPTH:Mutex<Option<ResidentSceneDepthAllocation>>=Mutex::new(None);
static RESIDENT_SCENE_LARGE_DEPTH:Mutex<Option<ResidentSceneDepthAllocation>>=Mutex::new(None);
static RESIDENT_SCENE_DIRECT_UI4_TARGETS:Mutex<[Option<ResidentSceneDirectUi4Mapping>;RESIDENT_UI4_DIRECT_MAPPING_COUNT]>=Mutex::new([None;RESIDENT_UI4_DIRECT_MAPPING_COUNT]);

#[derive(Clone,Copy,PartialEq,Eq)]struct PicassoCarrierLease(u8);
#[derive(Clone,Copy,PartialEq,Eq)]struct PicassoCarrierId(u8);
impl PicassoCarrierId {const ALL:[Self;4]=[Self(1),Self(2),Self(3),Self(4)];}
impl PicassoCarrierLease {
    fn carrier(self)->PicassoCarrierId {PicassoCarrierId(self.0)}
    fn gpuvm(self)->u8 {self.0} fn root_phys(self)->u64 {0x1234000}
}
trait PhysicalGpuDevice:Sync {
    fn unmap_gpuvm(&self,vm:u8,gpu:u64,bytes:usize)->Result<(),()>;
    fn gpuvm_root_phys(&self,_vm:u8)->Result<u64,()> {Ok(0x1234000)}
    fn map_gpuvm_scanout(&self,_vm:u8,gpu:u64,phys:u64,bytes:usize)->Result<(),()> {
        // Acquiring the Render0 table here would deadlock if the frame-pool
        // hook kept it locked while operating on a Picasso GPUVM.
        assert!(crate::RESIDENT_SCENE_DIRECT_UI4_TARGETS.0.try_lock().is_ok());
        if map_render_ppgtt_range(gpu,phys,bytes) {Ok(())} else {Err(())}
    }
    fn map_gpuvm(&self,vm:u8,gpu:u64,phys:u64,bytes:usize)->Result<(),()> {
        self.map_gpuvm_scanout(vm,gpu,phys,bytes)
    }
}
struct Physical;
impl PhysicalGpuDevice for Physical {
    fn unmap_gpuvm(&self,_vm:u8,gpu:u64,bytes:usize)->Result<(),()> {
        assert!(crate::RESIDENT_SCENE_DIRECT_UI4_TARGETS.0.try_lock().is_ok());
        if unmap_render_ppgtt_range(gpu,bytes) {Ok(())} else {Err(())}
    }
}
static PHYSICAL:Physical=Physical;
struct CarrierState {
    lease:PicassoCarrierLease,quarantined:bool,frame_active:bool,mapping_in_flight:bool,
    scene_state:Option<PicassoCarrierAllocation>,scene_depth:Option<PicassoCarrierAllocation>,
    scene_large_depth:Option<PicassoCarrierAllocation>,mappings:Vec<PicassoCarrierMapping>,
}
struct CarrierSlot {state:Option<CarrierState>}
static CARRIER:Mutex<CarrierSlot>=Mutex::new(CarrierSlot{state:None});
static OTHER_CARRIERS:[Mutex<CarrierSlot>;3]=[
    Mutex::new(CarrierSlot{state:None}),Mutex::new(CarrierSlot{state:None}),Mutex::new(CarrierSlot{state:None})];
fn picasso_carrier_slot(carrier:PicassoCarrierId)->&'static Mutex<CarrierSlot> {
    if carrier.0==1 {&CARRIER} else {&OTHER_CARRIERS[carrier.0 as usize-2]}
}
struct PicassoCarrierMappingGuard {lease:PicassoCarrierLease}
impl Drop for PicassoCarrierMappingGuard {fn drop(&mut self) {
    if let Some(state)=picasso_carrier_slot(self.lease.carrier()).lock().state.as_mut() {
        state.mapping_in_flight=false;
    }
}}
fn allocate_picasso_scene_range(lease:PicassoCarrierLease,_physical:&'static dyn PhysicalGpuDevice,gpu:u64,bytes:usize)->Option<PicassoCarrierAllocation> {
    let(phys,virt)=dma::alloc_ppgtt(bytes,4096)?;
    if !map_render_ppgtt_range(gpu,phys,bytes) {dma::dealloc(virt,bytes);return None;}
    picasso_carrier_slot(lease.carrier()).lock().state.as_mut()?.mappings.push(PicassoCarrierMapping{gpu,phys,bytes,scanout:false});
    Some(PicassoCarrierAllocation{gpu,phys,virt,bytes})
}
fn quarantine_picasso_render1(lease:PicassoCarrierLease,_reason:&str) {
    picasso_carrier_slot(lease.carrier()).lock().state.as_mut().unwrap().quarantined=true;
}
static PICASSO_CARRIER_SUBMIT_PROOF_LOGGED_MASK:AtomicU32=AtomicU32::new(0);
fn picasso_carrier_submit_proof_bit(carrier:PicassoCarrierId)->u32 {1<<carrier.0}
'''
source += item(PC, "PicassoCarrierAllocation")
source += "unsafe impl Send for PicassoCarrierAllocation {}\n"
for name in ("PICASSO_UI4_ALIAS_VA_BASE", "PICASSO_UI4_ALIAS_VA_LIMIT"):
    source += constant(PC, name) + "\n"
for name in (
    "PicassoCarrierMapping", "aligned_carrier_bytes", "begin_picasso_render1_mapping",
    "picasso_ui4_alias_first_fit", "picasso_mapping_is_ui4_alias",
    "map_picasso_render1_range_inner", "map_picasso_render1_scanout_range",
    "prepare_picasso_render1_ui4_target", "release_picasso_ui4_target_aliases",
    "unmap_picasso_render1_range_inner", "prepare_picasso_render1_large_depth_storage",
    "teardown_picasso_render1",
):
    source += item(PC, name) + "\n"
source += r'''
fn reset() {
    NATIVE.store(false,Ordering::Release);FAIL_MAP.store(false,Ordering::Release);FAIL_UNMAP.store(false,Ordering::Release);
    ALLOCS.lock().clear();FREES.lock().clear();MAPS.lock().clear();UNMAPS.lock().clear();
    *RESIDENT_SCENE_DEPTH.lock()=None;*RESIDENT_SCENE_LARGE_DEPTH.lock()=None;
    *RESIDENT_SCENE_DIRECT_UI4_TARGETS.lock()=[None;RESIDENT_UI4_DIRECT_MAPPING_COUNT];
    for carrier in PicassoCarrierId::ALL {
        picasso_carrier_slot(carrier).lock().state=Some(CarrierState{
            lease:PicassoCarrierLease(carrier.0),quarantined:false,
            frame_active:false,mapping_in_flight:false,scene_state:None,scene_depth:None,
            scene_large_depth:None,mappings:Vec::new()});
    }
}
fn surface(phys:u64,w:u32,h:u32)->intel::gpgpu::GpgpuRgba8Surface {
    intel::gpgpu::GpgpuRgba8Surface{phys,width:w,height:h,pitch_bytes:w*4,bytes:w as usize*h as usize*4}
}
#[test]fn only_native_direct_frames_admit_the_larger_extent() {
    reset();assert_eq!(ui4_frame_extent_limit(),(2560,1440));
    assert!(!resident_scene_target_extent_supported(3840,2160,true));
    NATIVE.store(true,Ordering::Release);assert_eq!(ui4_frame_extent_limit(),(3840,2160));
    assert!(resident_scene_target_extent_supported(3840,2160,true));
    assert!(!resident_scene_target_extent_supported(3840,2160,false));
    for (w,h) in [(0,2160),(3840,0),(3841,2160),(3840,2161),(usize::MAX,1)] {
        assert!(!resident_scene_target_extent_supported(w,h,true));
    }
    assert_eq!(drawable_depth_bytes(3840,2160),Some(33_423_360));
    NATIVE.store(false,Ordering::Release);assert_eq!(drawable_depth_bytes(3840,2160),None);
}
#[test]fn thirty_large_aliases_preserve_legacy_and_stay_between_spill_and_depth() {
    reset();let bytes=3840*2160*4;
    for slot in 0..RESIDENT_UI4_DIRECT_MAPPING_COUNT {
        let small=resident_scene_direct_ui4_gpu_for_slot(slot,0x01000000).unwrap();
        let big=resident_scene_direct_ui4_gpu_for_slot(slot,bytes).unwrap();
        assert_eq!(small,0x60000000+slot as u64*0x01000000);
        assert_eq!(big,0x90000000+slot as u64*0x02000000);
        assert!(big>=GPU_VA_PERSISTENT_SPILL_LIMIT);
        assert!(big+0x02000000<=GPU_VA_RESIDENT_SCENE_LARGE_DEPTH_BASE);
    }
    for bytes in [0,0x02000001,usize::MAX] {assert!(resident_scene_direct_ui4_gpu_for_slot(0,bytes).is_none());}
    assert!(resident_scene_direct_ui4_gpu_for_slot(30,3840*2160*4).is_none());
}
#[test]fn mixed_aliases_keep_identity_and_release_the_selected_bank() {
    reset();let small=surface(0x10000000,2560,1440);let big=surface(0x20000000,3840,2160);
    assert_eq!(prepare_resident_scene_direct_ui4_target(small),Ok(0x60000000));
    assert_eq!(prepare_resident_scene_direct_ui4_target(big),Ok(0x92000000));
    assert_eq!(prepare_resident_scene_direct_ui4_target(big),Ok(0x92000000));
    assert_eq!(MAPS.lock().len(),2);
    assert!(!release_resident_scene_direct_ui4_target(big.phys,big.bytes+4096));
    assert!(UNMAPS.lock().is_empty());
    FAIL_UNMAP.store(true,Ordering::Release);assert!(!release_resident_scene_direct_ui4_target(big.phys,big.bytes));
    assert!(RESIDENT_SCENE_DIRECT_UI4_TARGETS.lock()[1].is_some());
    FAIL_UNMAP.store(false,Ordering::Release);assert!(release_resident_scene_direct_ui4_target(big.phys,big.bytes));
    assert_eq!(UNMAPS.lock().last().copied(),Some((0x92000000,big.bytes)));
    assert_eq!(prepare_resident_scene_direct_ui4_target(surface(0x30000000,2560,1440)),Ok(0x61000000));
}
#[test]fn native_depth_has_its_own_lazy_cache_and_keeps_legacy_hiz() {
    reset();let small=prepare_resident_scene_depth(0xa780,2560,1440).unwrap();
    assert_eq!(small.gpu_addr,0x02000000);assert!(small.hiz.is_some());
    let big=prepare_resident_scene_depth(0xa780,3840,2160).unwrap();
    assert_eq!((big.gpu_addr,big.pitch_bytes,big.qpitch_rows_div4),(0xCC000000,15360,544));
    assert!(big.hiz.is_none());assert_eq!(ALLOCS.lock().len(),2);
    assert_eq!(ALLOCS.lock()[1].1,32*1024*1024);
    assert!(prepare_resident_scene_depth(0x9a49,2880,1620).is_ok());
    assert!(prepare_resident_scene_depth(0xa780,3840,2160).is_ok());
    assert_eq!(ALLOCS.lock().len(),2);assert_eq!(MAPS.lock().len(),2);
    assert_eq!(prepare_resident_scene_depth(0xa780,3841,2160).unwrap_err(),"resident-scene-depth-shape");
    assert_eq!(prepare_resident_scene_depth(0xffff,3840,2160).unwrap_err(),"resident-scene-depth-device");
}
#[test]fn rejected_depth_map_does_not_publish_or_leak_its_allocation() {
    reset();FAIL_MAP.store(true,Ordering::Release);
    assert_eq!(prepare_resident_scene_depth(0x9a49,3840,2160).unwrap_err(),"resident-scene-depth-map");
    assert!(RESIDENT_SCENE_LARGE_DEPTH.lock().is_none());assert_eq!(*ALLOCS.lock(),*FREES.lock());
}
#[test]fn picasso_large_depth_must_be_mapped_before_frame_admission() {
    reset();let lease=PicassoCarrierLease(1);
    CARRIER.lock().state.as_mut().unwrap().frame_active=true;
    assert!(prepare_picasso_render1_large_depth_storage(lease,&PHYSICAL).is_none());
    assert!(ALLOCS.lock().is_empty());
    CARRIER.lock().state.as_mut().unwrap().frame_active=false;
    let first=prepare_picasso_render1_large_depth_storage(lease,&PHYSICAL).unwrap();
    assert_eq!((first.gpu,first.bytes),(0xCC000000,32*1024*1024));
    assert!(!CARRIER.lock().state.as_ref().unwrap().mapping_in_flight);
    CARRIER.lock().state.as_mut().unwrap().frame_active=true;
    let cached=prepare_picasso_render1_large_depth_storage(lease,&PHYSICAL).unwrap();
    assert_eq!(first.phys,cached.phys);assert_eq!(ALLOCS.lock().len(),1);
    assert!(!teardown_picasso_render1(lease,&PHYSICAL));assert!(FREES.lock().is_empty());
    CARRIER.lock().state.as_mut().unwrap().frame_active=false;
    assert!(teardown_picasso_render1(lease,&PHYSICAL));
    assert_eq!(*ALLOCS.lock(),*FREES.lock());assert!(CARRIER.lock().state.is_none());
}
#[test]fn failed_picasso_teardown_retains_native_depth_and_quarantines_owner() {
    reset();let lease=PicassoCarrierLease(1);
    prepare_picasso_render1_large_depth_storage(lease,&PHYSICAL).unwrap();
    FAIL_UNMAP.store(true,Ordering::Release);
    assert!(!teardown_picasso_render1(lease,&PHYSICAL));assert!(FREES.lock().is_empty());
    let slot=CARRIER.lock();let owner=slot.state.as_ref().unwrap();
    assert!(owner.quarantined && owner.scene_large_depth.is_some() && owner.mappings.len()==1);
}
fn destroy_pixels_if_released(phys:u64,bytes:usize,pixels:&mut Vec<u8>)->bool {
    if !release_resident_scene_direct_ui4_target(phys,bytes) {return false;}
    pixels.clear();true
}
#[test]fn repeated_native_triple_buffer_resizes_reuse_holes_and_keep_small_live_va() {
    reset();let lease=PicassoCarrierLease(1);let small_phys=0x08000000;
    let small=prepare_picasso_render1_ui4_target(lease,&PHYSICAL,small_phys,1024*1024).unwrap();
    assert_eq!(small,0x60000000);
    let bytes=3840*2160*4;let mut transferred=0usize;
    for cycle in 0..100u64 {
        let mut frames=Vec::new();
        for index in 0..3u64 {
            let phys=0x100000000+(cycle*3+index)*0x04000000;
            let gpu=prepare_picasso_render1_ui4_target(lease,&PHYSICAL,phys,bytes).unwrap();
            assert_eq!(gpu,0x60100000+index*bytes as u64);
            frames.push((phys,gpu));transferred+=bytes;
        }
        assert_eq!(CARRIER.lock().state.as_ref().unwrap().mappings.len(),4);
        for (phys,_) in frames {assert!(release_resident_scene_direct_ui4_target(phys,bytes));}
        assert_eq!(CARRIER.lock().state.as_ref().unwrap().mappings.len(),1);
        assert_eq!(prepare_picasso_render1_ui4_target(lease,&PHYSICAL,small_phys,1024*1024),Some(small));
    }
    assert!(transferred>32*1024*1024*256); // Many complete arena turnovers.
    assert!(FREES.lock().is_empty()); // The frame pool, not the driver, frees pixels.
}
#[test]fn full_alias_arena_reuses_an_interior_hole_without_moving_live_targets() {
    reset();let lease=PicassoCarrierLease(1);let bytes=3840*2160*4;
    let mut live=Vec::new();
    for index in 0..8u64 {
        let phys=0x100000000+index*0x04000000;
        live.push((phys,prepare_picasso_render1_ui4_target(lease,&PHYSICAL,phys,bytes).unwrap()));
    }
    assert!(prepare_picasso_render1_ui4_target(lease,&PHYSICAL,0x200000000,bytes).is_none());
    assert!(!CARRIER.lock().state.as_ref().unwrap().mapping_in_flight);
    let retired=live.remove(3);assert!(release_resident_scene_direct_ui4_target(retired.0,bytes));
    assert_eq!(prepare_picasso_render1_ui4_target(lease,&PHYSICAL,0x200000000,bytes),Some(retired.1));
    for (phys,gpu) in live {assert_eq!(prepare_picasso_render1_ui4_target(lease,&PHYSICAL,phys,bytes),Some(gpu));}
}
#[test]fn cross_carrier_busy_release_retains_pixels_then_retries_all_aliases() {
    reset();let phys=0x50000000;let bytes=3840*2160*4;let mut pixels=vec![0x5a;128];
    for carrier in PicassoCarrierId::ALL {
        prepare_picasso_render1_ui4_target(PicassoCarrierLease(carrier.0),&PHYSICAL,phys,bytes).unwrap();
    }
    // Import the same physical address as an unrelated resource and as a
    // scanout mapping outside the UI4 window: neither belongs to this hook.
    CARRIER.lock().state.as_mut().unwrap().mappings.extend([
        PicassoCarrierMapping{gpu:0x31000000,phys,bytes:4096,scanout:false},
        PicassoCarrierMapping{gpu:0x71000000,phys,bytes:4096,scanout:true},
    ]);
    picasso_carrier_slot(PicassoCarrierId(2)).lock().state.as_mut().unwrap().frame_active=true;
    assert!(!destroy_pixels_if_released(phys,bytes,&mut pixels));assert_eq!(pixels,vec![0x5a;128]);
    assert_eq!(CARRIER.lock().state.as_ref().unwrap().mappings.len(),2);
    for carrier in &PicassoCarrierId::ALL[1..] {
        assert_eq!(picasso_carrier_slot(*carrier).lock().state.as_ref().unwrap().mappings.len(),1);
    }
    picasso_carrier_slot(PicassoCarrierId(2)).lock().state.as_mut().unwrap().frame_active=false;
    assert!(destroy_pixels_if_released(phys,bytes,&mut pixels));assert!(pixels.is_empty());
    for carrier in &PicassoCarrierId::ALL[1..] {
        assert!(picasso_carrier_slot(*carrier).lock().state.as_ref().unwrap().mappings.is_empty());
    }
    assert_eq!(CARRIER.lock().state.as_ref().unwrap().mappings.len(),2);
}
#[test]fn uncertain_carrier_unmap_quarantines_and_retains_pixels_and_alias() {
    reset();let lease=PicassoCarrierLease(1);let phys=0x50000000;let bytes=3840*2160*4;
    let gpu=prepare_picasso_render1_ui4_target(lease,&PHYSICAL,phys,bytes).unwrap();
    let mut pixels=vec![0x69;128];
    assert!(!destroy_pixels_if_released(phys,bytes+4096,&mut pixels));
    assert!(UNMAPS.lock().is_empty());assert_eq!(pixels,vec![0x69;128]);
    CARRIER.lock().state.as_mut().unwrap().mapping_in_flight=true;
    assert!(!destroy_pixels_if_released(phys,bytes,&mut pixels));assert!(UNMAPS.lock().is_empty());
    CARRIER.lock().state.as_mut().unwrap().mapping_in_flight=false;
    FAIL_UNMAP.store(true,Ordering::Release);
    assert!(!destroy_pixels_if_released(phys,bytes,&mut pixels));assert_eq!(pixels,vec![0x69;128]);
    let slot=CARRIER.lock();let state=slot.state.as_ref().unwrap();
    assert!(state.quarantined && !state.mapping_in_flight);
    assert_eq!(state.mappings.len(),1);assert_eq!(state.mappings[0].gpu,gpu);drop(slot);
    FAIL_UNMAP.store(false,Ordering::Release);
    assert!(!destroy_pixels_if_released(phys,bytes,&mut pixels));assert_eq!(pixels,vec![0x69;128]);
    assert_eq!(UNMAPS.lock().len(),1);
}
#[test]fn failed_prepare_consumes_no_alias_and_cached_active_target_never_remaps() {
    reset();let lease=PicassoCarrierLease(1);let phys=0x50000000;let bytes=3840*2160*4;
    FAIL_MAP.store(true,Ordering::Release);
    assert!(prepare_picasso_render1_ui4_target(lease,&PHYSICAL,phys,bytes).is_none());
    assert!(CARRIER.lock().state.as_ref().unwrap().mappings.is_empty());
    assert!(!CARRIER.lock().state.as_ref().unwrap().mapping_in_flight);
    FAIL_MAP.store(false,Ordering::Release);
    assert_eq!(prepare_picasso_render1_ui4_target(lease,&PHYSICAL,phys,bytes),Some(0x60000000));
    let maps=MAPS.lock().len();CARRIER.lock().state.as_mut().unwrap().frame_active=true;
    assert_eq!(prepare_picasso_render1_ui4_target(lease,&PHYSICAL,phys,bytes),Some(0x60000000));
    assert!(prepare_picasso_render1_ui4_target(lease,&PHYSICAL,0x60000000,bytes).is_none());
    assert_eq!(MAPS.lock().len(),maps);
}
'''

with tempfile.TemporaryDirectory(prefix="trueos-native-frame-") as directory:
    path = Path(directory)
    (path / "tests.rs").write_text(source)
    subprocess.run(["rustc", "--edition=2024", "--test", str(path / "tests.rs"),
                    "-o", str(path / "tests")], check=True, cwd=ROOT)
    subprocess.run([str(path / "tests"), "--test-threads=1"], check=True)
