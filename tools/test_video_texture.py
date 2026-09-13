#!/usr/bin/env python3
"""Host tests of the production video API, ring protocol and textured draws."""
from pathlib import Path
import subprocess,tempfile
from test_clip_position3_uv_texture import ROOT,item
from test_retained_material import harness_source

def run(source):
    with tempfile.TemporaryDirectory(prefix='video-texture-tests-') as d:
        root=Path(d);(root/'test.rs').write_text(source)
        subprocess.run(['rustc','--edition=2024','--test',str(root/'test.rs'),'-o',str(root/'tests')],check=True)
        subprocess.run([str(root/'tests'),'--test-threads=1'],check=True)

api=ROOT/'crates/trueos-v/src/vmedia_video.rs'
run('''#![allow(dead_code)]
extern crate alloc;
mod vgpu { #[derive(Clone,Copy)] pub struct Device; impl Device { pub fn raw(self)->u64 {7} } }
mod bp_abi { pub use crate::command_mock as trueos_cabi_vmedia_video_command_v1; }
mod vmedia {
 #[derive(Debug,PartialEq)] pub struct TextureId(pub u64);
 #[path="'''+str(api)+'''"] pub mod video;
}
use vmedia::video::*;
use std::sync::Mutex;
static CALLS: Mutex<Vec<(u32,u64,u64,usize)>>=Mutex::new(Vec::new());
static FAILURE: Mutex<Option<u32>>=Mutex::new(None);
static POLLS: Mutex<Vec<i32>>=Mutex::new(Vec::new());
pub unsafe fn command_mock(op:u32,a:u64,b:u64,_input:*const u8,len:usize,out:*mut u8,_out_len:usize)->i32 {
 CALLS.lock().unwrap().push((op,a,b,len));
 if *FAILURE.lock().unwrap()==Some(op) { return -9; }
 match op { 0=>42, 3=>{
   let rc=POLLS.lock().unwrap().pop().unwrap_or(0);
   if rc==1 { let mut data=Vec::new();data.extend(99u64.to_le_bytes());data.extend(10u64.to_le_bytes());data.extend(48u32.to_le_bytes());data.extend(48u32.to_le_bytes());unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(),out,24); } }
   rc
 }, _=>0 }
}
fn reset(){CALLS.lock().unwrap().clear();*FAILURE.lock().unwrap()=None;POLLS.lock().unwrap().clear();}
#[test] fn uploads_only_encoded_chunks_and_commits_once(){
 reset();let video=Video::open(vgpu::Device,&vec![1;7000],true).unwrap();drop(video);
 assert_eq!(*CALLS.lock().unwrap(),vec![(0,7,7000,1),(1,42,0,3072),(1,42,3072,3072),(1,42,6144,856),(2,42,0,0),(5,42,0,0)]);
}
#[test] fn upload_and_commit_failures_close_the_reserved_stream(){
 for op in [1,2] {reset();*FAILURE.lock().unwrap()=Some(op);assert!(Video::open(vgpu::Device,&[1],false).is_err());assert_eq!(CALLS.lock().unwrap().last().unwrap().0,5);}
}
#[test] fn frame_lease_survives_video_handle_and_releases_before_stream_close(){
 reset();let mut video=Video::open(vgpu::Device,&[1],true).unwrap();POLLS.lock().unwrap().push(1);
 let VideoPoll::Frame(frame)=video.poll().unwrap() else {panic!()};
 assert_eq!(frame.texture_id(),vmedia::TextureId(99));assert_eq!(frame.extent(),[48,48]);assert_eq!(frame.sequence(),10);
 drop(video);assert!(!CALLS.lock().unwrap().iter().any(|c|c.0==5));drop(frame);
 let calls=CALLS.lock().unwrap();assert_eq!(calls[calls.len()-2],(4,42,10,0));assert_eq!(calls.last().unwrap().0,5);
}
#[test] fn pending_end_and_errors_never_fabricate_frame_leases(){
 reset();let mut video=Video::open(vgpu::Device,&[1],false).unwrap();*POLLS.lock().unwrap()=vec![-2,2,0];
 assert!(matches!(video.poll(),Ok(VideoPoll::Pending)));assert!(matches!(video.poll(),Ok(VideoPoll::End)));assert!(matches!(video.poll(),Err(-2)));drop(video);
 assert!(!CALLS.lock().unwrap().iter().any(|c|c.0==4));
}
''')
source=harness_source()
kind=item('crates/trueos-v/src/vgpu.rs','RetainedTexturedFrameV1')
assert kind==item('../TRUEOS-Blueprints/crates/trueos-v/src/vgpu.rs','RetainedTexturedFrameV1')
source=source.replace('pub mod vgpu {','pub mod vgpu {\n'+kind,1)
source+=item('src/gpu/vgpu.rs','retained_textured_descriptor_valid')
source+=item('src/intel/render/resources.rs','picasso_retained_draw_templates')
source+='''
#[test] fn four_texture_draws_share_one_mesh_and_may_repeat_video_ids(){
 use vgpu::*;let mut s=RetainedTexturedFrameV1::default();s.frame.seed_count=4;
 s.textures=[11,22,33,11];s.ranges=[RetainedDrawRange{first_index:0,index_count:6};4];
 assert!(retained_textured_descriptor_valid(&s));
 let mut seeds=s.frame.seeds;for (i,seed) in seeds.iter_mut().enumerate(){seed.draw_group=i as u32;}
 let draws=picasso_retained_draw_templates(6,&seeds,&s.ranges).unwrap();
 for (i,d) in draws.iter().enumerate(){assert_eq!(*d,[6,0,0,i as u32,1,0]);}
 assert_eq!(std::mem::size_of::<RetainedTexturedFrameV1>(),880);
 assert_eq!(std::mem::offset_of!(RetainedTexturedFrameV1,ranges),816);
 for count in [0,5,u32::MAX] {let mut bad=s;bad.frame.seed_count=count;assert!(!retained_textured_descriptor_valid(&bad));}
 let mut bad=s;bad.textures[1]=0;assert!(!retained_textured_descriptor_valid(&bad));
 let mut bad=s;bad.ranges[0].first_index=u32::MAX;assert!(!retained_textured_descriptor_valid(&bad));
 let mut bad=s;bad.frame.material.textures[0]=7;assert!(!retained_textured_descriptor_valid(&bad));
}
'''
run(source)
# Exercise the real stream command/ring implementation with GPU execution
# replaced at the hardware boundary. Shared-slot admission below is production.
service=(ROOT/'src/r/services/video_service.rs').read_text().replace('#[trueos_executor::task(pool_size = 3)]','')
shared_begin=item('src/ui4/video_frame.rs','begin_texture_video_player')
stubs=r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as spin;
extern crate self as trueos_time;
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T>{pub const fn new(t:T)->Self{Self(std::sync::Mutex::new(t))} pub fn lock(&self)->std::sync::MutexGuard<'_,T>{self.0.lock().unwrap()}}
pub struct Duration;impl Duration{pub fn from_millis(_:u64)->Self{Self}}
pub struct Timer;impl Timer{pub async fn after(_:Duration){}}
#[macro_export] macro_rules! log_error{($($x:tt)*)=>{}}
#[macro_export] macro_rules! log_warn{($($x:tt)*)=>{}}
mod gpu {pub mod vgpu {
 #[derive(Clone,Copy,Debug,PartialEq)]pub enum Principal{HullGuest(u16),HostRuntime}
 #[derive(Clone,Copy)]pub struct DeviceHandle(u64);impl DeviceHandle{pub fn from_raw(v:u64)->Self{Self(v)}}
 #[derive(Clone,Copy)]pub struct RetainedTextureHandle(u64);impl RetainedTextureHandle{pub fn raw(self)->u64{self.0}}
 #[derive(Clone,Copy)]pub enum VgpuError{Busy,NotComplete,InvalidHandle}impl VgpuError{pub fn errno(self)->i32{-9}}
 pub fn device_info(_:Principal,d:DeviceHandle)->Result<(),VgpuError>{if d.0==7{Ok(())}else{Err(VgpuError::InvalidHandle)}}
 static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(100);
 pub fn create_retained_texture(_:Principal,_:DeviceHandle,_:u32,_:u32,_:u32,_:&[u8])->Result<RetainedTextureHandle,VgpuError>{Ok(RetainedTextureHandle(NEXT.fetch_add(1,std::sync::atomic::Ordering::Relaxed)))}
 pub fn destroy_retained_texture(_:Principal,_:DeviceHandle,_:RetainedTextureHandle)->Result<(),VgpuError>{Ok(())}
 pub struct Write;impl Write{pub fn surface(&self)->crate::intel::gpgpu::GpgpuRgba8Surface{crate::intel::gpgpu::GpgpuRgba8Surface}}
 pub fn acquire_retained_texture_write(_:Principal,_:DeviceHandle,_:RetainedTextureHandle)->Result<Write,VgpuError>{Ok(Write)}
}}
mod intel {
 pub mod media{pub mod hw_vid{pub async fn run_memory_texture_video_playback(_:crate::ui4::VideoPlaybackSession,_:Vec<u8>)->Result<(),&'static str>{Ok(())}}}
 pub mod gpgpu{
 #[derive(Clone,Copy)]pub struct GpgpuRgba8Surface;
 #[derive(Clone,Copy)]pub struct GpgpuNv12Tile64Surface;impl GpgpuNv12Tile64Surface{pub fn new(_:u64,_:u64,_:usize,_:u32,_:u32,_:u32,_:u32)->Option<Self>{Some(Self)}}
 pub enum Ui4CompositorSubmitError{Busy}
 pub enum Ui4VideoFrameCompletion{Complete{},Pending,Failed}
 pub fn queue_ui4_video_frame_nv12_tile64_to_rgba8(_:GpgpuNv12Tile64Surface,_:GpgpuRgba8Surface,_:u32,_:u32,_:u32,_:u32,_:u32,_:u32,_:bool,_:u8)->Result<u32,Ui4CompositorSubmitError>{Ok(1)}
 pub fn poll_ui4_video_frame_submission(_:u32,_:GpgpuRgba8Surface)->Ui4VideoFrameCompletion{Ui4VideoFrameCompletion::Complete{}}
 }
}
mod ui4 {
 use std::sync::atomic::{AtomicBool,AtomicU64,AtomicU32,Ordering};
 #[derive(Clone,Copy,Debug,PartialEq)]pub struct VideoPlaybackSession{pub slot:usize,generation:u64}
 struct State{occupied:AtomicBool,generation:AtomicU64,cancelled:AtomicBool,paused:AtomicBool,texture_target:AtomicU32}
 impl State{const fn new()->Self{Self{occupied:AtomicBool::new(false),generation:AtomicU64::new(0),cancelled:AtomicBool::new(false),paused:AtomicBool::new(false),texture_target:AtomicU32::new(0)}}}
 static VIDEO_SESSIONS:[State;3]=[const{State::new()};3];
 impl VideoPlaybackSession{
 pub fn is_cancelled(self)->bool{VIDEO_SESSIONS[self.slot].cancelled.load(Ordering::Acquire)}
 pub fn cancel(self){VIDEO_SESSIONS[self.slot].cancelled.store(true,Ordering::Release);}
 pub fn finish(self,_:&str){VIDEO_SESSIONS[self.slot].occupied.store(false,Ordering::Release);}
 }
 pub fn video_frame_extent_admitted(w:u32,h:u32)->bool{w>0&&h>0&&w*h<=1920*1080}
 #[derive(Clone,Copy)]pub struct DecodedNv12Source{pub phys:u64,pub gpu:u64,pub byte_len:usize,pub width:u32,pub height:u32,pub visible_width:u32,pub visible_height:u32,pub pitch_bytes:usize,pub uv_offset:usize,pub video_full_range:bool,pub matrix_coefficients:u8}
'''+shared_begin+r'''
}
fn block_on<F:std::future::Future>(future:F)->F::Output{
 use std::task::{Context,Poll,Waker};let mut f=std::pin::pin!(future);let mut cx=Context::from_waker(Waker::noop());
 loop{if let Poll::Ready(v)=f.as_mut().poll(&mut cx){return v}}
}
mod service {
'''
tests=r'''
#[test] fn protocol_enforces_owner_order_and_shared_slot_cap(){
 let owner=0x80000001;let mut ids=Vec::new();
 // Reserve one slot as a shell player would; only two remain for Blueprint.
 let shell=crate::ui4::begin_texture_video_player(999).unwrap();
 for _ in 0..2{let id=command(owner,0,7,4,&[1],&mut []);assert!(id>0);ids.push(id);}
 assert_eq!(command(owner,0,7,4,&[1],&mut []),BUSY);
 let id=ids[0] as u64;
 assert_eq!(command(owner+1,1,id,0,&[1],&mut []),-1);
 assert_eq!(command(owner,1,id,1,&[1],&mut []),INVALID);
 assert_eq!(command(owner,2,id,0,&[],&mut []),INVALID);
 assert_eq!(command(owner,1,id,0,&[1,2,3,4],&mut []),0);
 assert_eq!(command(owner,1,id,4,&[5],&mut []),INVALID);
 assert_eq!(command(owner,2,id,0,&[],&mut []),0);
 assert_eq!(command(owner,2,id,0,&[],&mut []),INVALID);
 release_owner(owner);reap();assert!(STREAMS.lock().is_empty());shell.finish("test");
}
#[test] fn ring_backpressure_preserves_order_and_owner_cleanup_drains(){
 let owner=0x80000002;let id=command(owner,0,7,1,&[0],&mut []) as u32;assert!(id>0);
 let session=STREAMS.lock()[0].session;
 let source=DecodedNv12Source{phys:4096,gpu:4096,byte_len:4096,width:48,height:48,visible_width:48,visible_height:48,pitch_bytes:64,uv_offset:3072,video_full_range:true,matrix_coefficients:1};
 for _ in 0..3{assert!(crate::block_on(convert(id,session,source)));}
 assert!(!conversion_ready(id)); // no conversion lane is occupied waiting for us
 let mut data=[0;24];
 for sequence in 1..=3{
   assert_eq!(command(owner,3,id as u64,0,&[],&mut data),1);
   assert_eq!(u64::from_le_bytes(data[8..16].try_into().unwrap()),sequence);
 }
 assert_eq!(command(owner,3,id as u64,0,&[],&mut data),0);
 assert_eq!(command(owner,4,id as u64,99,&[],&mut []),INVALID);
 assert_eq!(command(owner,4,id as u64,1,&[],&mut []),0);
 assert!(conversion_ready(id));assert!(crate::block_on(convert(id,session,source)));
 assert_eq!(command(owner,3,id as u64,0,&[],&mut data),1);
 assert_eq!(u64::from_le_bytes(data[8..16].try_into().unwrap()),4);
 assert_eq!(command(owner,5,id as u64,0,&[],&mut []),0);
 assert!(conversion_ready(id));reap();assert_eq!(STREAMS.lock().len(),1); // outstanding readers
 release_owner(owner);reap();assert!(STREAMS.lock().is_empty());
}
#[test] fn malformed_open_does_not_reserve_a_slot(){
 for (device,length,flags) in [(0,1,0),(7,0,0),(7,MAX_ENCODED as u64+1,0),(7,1,2)]{
 assert!(command(1,0,device,length,&[flags],&mut [])<0);
 }
 assert!(STREAMS.lock().is_empty());
}
}
'''
run(stubs+service+tests)
# Device teardown must observe both producer and staged render-reader pins.
lease=item('src/gpu/vgpu.rs','device_has_operation_leases')
run('''use std::sync::Arc;
#[derive(Default)]struct Record{in_flight:u32,writing:bool,resident:Arc<()>}
struct Slot{record:Option<Record>}
#[derive(Default)]struct VirtualDevice{picasso_setup_in_flight:bool,buffers:Vec<Slot>,surfaces:Vec<Slot>,queues:Vec<Slot>,retained_meshes:Vec<Slot>,retained_textures:Vec<Slot>}
'''+lease+'''
#[test] fn texture_writers_and_readers_block_device_teardown(){
 let mut d=VirtualDevice::default();d.retained_textures.push(Slot{record:Some(Record::default())});
 assert!(!device_has_operation_leases(&d));
 d.retained_textures[0].record.as_mut().unwrap().writing=true;assert!(device_has_operation_leases(&d));
 d.retained_textures[0].record.as_mut().unwrap().writing=false;
 let reader=d.retained_textures[0].record.as_ref().unwrap().resident.clone();assert!(device_has_operation_leases(&d));
 drop(reader);assert!(!device_has_operation_leases(&d));
}
''')
# Validate the actual demo submission rather than a separately authored sample.
from test_clip_position3_uv_texture import constant
source=harness_source().replace('pub mod vgpu {','pub mod vgpu {\n'+kind,1)
source+=constant('../TRUEOS-Picasso-Example/src/main.rs','HEAD_INSTANCE_COUNT')
source+=constant('../TRUEOS-Picasso-Example/src/main.rs','HEAD_PLANE_OFFSET')
source+=constant('../TRUEOS-Picasso-Example/src/main.rs','HEAD_WORLD_TRANSLATIONS')
source+='mod video_demo {use crate::vgpu::*;\n'+item('../TRUEOS-Picasso-Example/src/video_demo.rs','submission')+item('../TRUEOS-Picasso-Example/src/video_demo.rs','tests')+'}\n'
run(source)
