#!/usr/bin/env python3
"""Check the real clip ABI, VM boundary, and sink together on the host."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    harness = r'''
#![allow(dead_code)]
extern crate alloc;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(t:T)->Self { Self(std::sync::Mutex::new(t)) }
    fn lock(&self)->std::sync::MutexGuard<'_,T> { self.0.lock().unwrap() }
}
#[path="@SINK@"] mod sink;
pub(crate) static SINK:Mutex<sink::Sink>=Mutex::new(sink::Sink::new());
#[path="@HANDOFF@"] mod handoff;
mod audio_playback {
    pub(crate) use crate::sink::{ProducerOwner, validate_append_size, validate_size};
    pub(crate) use crate::{SINK, monotonic_millis};
}
static MODE:AtomicU8=AtomicU8::new(0);
static START:AtomicUsize=AtomicUsize::new(0);
static END:AtomicUsize=AtomicUsize::new(0);
static GENERATION:AtomicUsize=AtomicUsize::new(1);
pub(crate) fn monotonic_millis()->u64 { 0 }
mod hv {
    pub const TRUEOS_VM_ID_LIMIT: usize = 64;
    pub fn current_hull_guest_context_vm_id()->Option<u8> {
        (super::MODE.load(super::Ordering::SeqCst)==1).then_some(7)
    }
    pub fn current_guest_execution_context_vm_id()->Option<u8> {
        (super::MODE.load(super::Ordering::SeqCst)==2).then_some(7)
    }
    pub fn vm_run_generation(vm:u8)->Option<u64> {
        (vm==7).then(|| super::GENERATION.load(super::Ordering::SeqCst) as u64)
    }
}
mod allocators {
    pub fn with_host_alloc_domain_strong<T>(f:impl FnOnce()->T)->T { f() }
    pub fn hv_guest_heap_bounds(vm:u8)->Option<(usize,usize)> {
        (vm==7).then(|| (super::START.load(super::Ordering::SeqCst),super::END.load(super::Ordering::SeqCst)))
    }
}
mod native_abi {
    pub fn abi_read_bytes<'a>(p:*const u8,n:usize)->Option<&'a[u8]> {
        if n==0 {Some(&[])} else if p.is_null() {None} else {Some(unsafe {std::slice::from_raw_parts(p,n)})}
    }
}
mod trueos_vm { pub mod vmcall {
    pub const OP_BP_HTTP_AUDIO_CLIP_SUBMIT_V1:u32=0x259;
    pub const OP_BP_HTTP_AUDIO_STREAM_V1:u32=0x25b;
    pub const STATUS_OK:u32=0;
    pub fn call(op:u32,address:u64,bytes:u64)->(u32,u64) {
        assert_eq!(op,OP_BP_HTTP_AUDIO_CLIP_SUBMIT_V1);
        (STATUS_OK,crate::handoff::submit_guest(7,address,bytes) as i64 as u64)
    }
    pub fn call_with_payload(op:u32,action:u64,token:u64,bytes:&[u8],_:&mut [u8])->(u32,u64) {
        assert_eq!(op,OP_BP_HTTP_AUDIO_STREAM_V1);
        (STATUS_OK,crate::handoff::stream_guest(7,action,token,bytes) as u64)
    }
} }
#[test]
fn pointer_boundaries_copy_lifetime_and_native_vm_paths() {
    let bytes=vec![1u8,2,3,4,5,6,7,8];
    let ptr=bytes.as_ptr() as usize;
    START.store(ptr,Ordering::SeqCst); END.store(ptr+bytes.len(),Ordering::SeqCst);
    assert_eq!(handoff::submit_guest(8,ptr as u64,8),-14);
    assert_eq!(handoff::submit_guest(7,0,8),-14);
    assert_eq!(handoff::submit_guest(7,(ptr-1) as u64,8),-14);
    assert_eq!(handoff::submit_guest(7,(ptr+4) as u64,8),-14);
    assert_eq!(handoff::submit_guest(7,(ptr+8) as u64,4),-14);
    assert_eq!(handoff::submit_guest(7,u64::MAX-1,4),-14);
    assert_eq!(handoff::submit_guest(7,ptr as u64,2),-22);
    assert_eq!(handoff::submit_guest(7,ptr as u64,(sink::MAX_CLIP_BYTES+4) as u64),-27);
    for mode in [0,1,2] {
        MODE.store(mode,Ordering::SeqCst);
        assert_eq!(unsafe {handoff::trueos_cabi_http_audio_clip_submit_v1(bytes.as_ptr(),8)},0);
        let expected_owner = if mode == 0 {
            sink::ProducerOwner::Host
        } else {
            sink::ProducerOwner::Vm { id: 7, generation: GENERATION.load(Ordering::SeqCst) as u64 }
        };
        assert_eq!(SINK.lock().route_sources()[0].unwrap().owner, expected_owner);
        let mut out=[0;8]; SINK.lock().render(&mut out); assert_eq!(out,bytes.as_slice());
    }
    MODE.store(0,Ordering::SeqCst);
    // Both submissions outlive their caller and occupy separate playback slots.
    let mut silent=[1;8]; SINK.lock().render(&mut silent); assert_eq!(silent,[0;8]);
    {
        let clip=vec![123u8,0,56,254,21,3,13,252];
        assert_eq!(unsafe {handoff::trueos_cabi_http_audio_clip_submit_v1(clip.as_ptr(),clip.len())},0);
    }
    let competing=[9u8,0,9,0];
    assert_eq!(unsafe {handoff::trueos_cabi_http_audio_clip_submit_v1(competing.as_ptr(),competing.len())},0);
    let mut out=[0;8]; let mut other=[0;8];
    let reports=SINK.lock().render_sources(&mut out,&mut other);
    assert_eq!(out,[123,0,56,254,21,3,13,252]);
    assert_eq!(other,[9,0,9,0,0,0,0,0]);
    assert_eq!((reports[0].bytes,reports[1].bytes),(8,4));
    SINK.lock().render(&mut out); assert_eq!(out,[0;8]);
}
#[test]
fn progressive_handoff_uses_bounded_guest_payload_and_generation() {
    *SINK.lock()=sink::Sink::new();
    let mut pcm=vec![1u8,2,3,4,5,6,7,8];
    let ptr=pcm.as_ptr() as usize;
    START.store(ptr,Ordering::SeqCst); END.store(ptr+pcm.len(),Ordering::SeqCst);
    let stream=handoff::trueos_cabi_http_audio_stream_v1;
    let null=core::ptr::null();
    MODE.store(1,Ordering::SeqCst);
    let id=unsafe { stream(0,0,null,0) };
    assert!(id>0);
    let id=id as u64;
    assert_eq!(unsafe {stream(4,id,null,0)},1);
    assert_eq!(unsafe {stream(4,id,pcm.as_ptr(),4)},-22);
    let other=unsafe {stream(0,0,null,0)};
    assert!(other>0);
    assert_eq!(unsafe {stream(0,0,null,0)},-16);
    assert_eq!(unsafe {stream(3,other as u64,null,0)},0);
    assert_eq!(unsafe {stream(1,id,null,4)},-14);
    assert_eq!(unsafe {stream(1,id,pcm.as_ptr(),2)},-22);
    assert_eq!(unsafe {stream(1,id,pcm.as_ptr(),sink::MAX_APPEND_BYTES+4)},-27);
    assert_eq!(unsafe {stream(1,id,pcm.as_ptr(),12)},-14);
    assert_eq!(unsafe {stream(1,id,pcm.as_ptr(),8)},0);
    pcm.fill(0);
    assert!(SINK.lock().snapshot().is_none());
    GENERATION.store(2,Ordering::SeqCst);
    assert_eq!(unsafe {stream(2,id,null,0)},-13);
    GENERATION.store(1,Ordering::SeqCst);
    assert_eq!(unsafe {stream(2,id,null,0)},0);
    assert_eq!(unsafe {stream(4,id,null,0)},1);
    let mut output=[0;8];
    assert_eq!(SINK.lock().render(&mut output)[0].first,Some((id,0)));
    assert_eq!(output,[1,2,3,4,5,6,7,8]);
    assert_eq!(unsafe {stream(4,id,null,0)},0);
    assert_eq!(unsafe {stream(4,999,null,0)},0);
    assert_eq!(SINK.lock().snapshot().unwrap().id,id);
    let next=unsafe {stream(0,0,null,0)};
    assert!(next>id as i64);
    handoff::release_vm(7,1);
    assert_eq!(unsafe {stream(2,next as u64,null,0)},-2);
    MODE.store(0,Ordering::SeqCst);
}

#[test]
fn analog_monitor_begin_uses_the_same_abi_across_execution_modes() {
    let frame = [5, 0, 9, 0];
    let pcm = frame.repeat(48_000 * 32 / 1000);
    let ptr = pcm.as_ptr() as usize;
    START.store(ptr, Ordering::SeqCst);
    END.store(ptr + pcm.len(), Ordering::SeqCst);
    let stream = handoff::trueos_cabi_http_audio_stream_v1;
    let null = core::ptr::null();
    for mode in [0, 1, 2] {
        let mut sink = SINK.lock();
        *sink = sink::Sink::new();
        sink.set_local_playback(true);
        sink.set_http_rendering(true);
        drop(sink);
        MODE.store(mode, Ordering::SeqCst);

        assert_eq!(unsafe { stream(5, 1, null, 0) }, -22);
        assert_eq!(unsafe { stream(5, 0, pcm.as_ptr(), 4) }, -22);
        let id = unsafe { stream(5, 0, null, 0) };
        assert!(id > 0);
        assert_eq!(unsafe { stream(1, id as u64, pcm.as_ptr(), pcm.len()) }, 0);

        let mut jack = [[0; 2]; 1];
        let mut first = [0; 4];
        let mut second = [0; 4];
        let mut sink = SINK.lock();
        sink.render_local(&mut jack, 0);
        assert_eq!(jack, [[5, 9]]);
        assert_eq!(sink.render_sources(&mut first, &mut second)[0].bytes, 0);
    }
    *SINK.lock() = sink::Sink::new();
    MODE.store(0, Ordering::SeqCst);
}
'''.replace('@SINK@', str(ROOT / 'src/r/services/audio_playback/sink.rs')).replace('@HANDOFF@',str(ROOT / 'src/r/services/audio_http/handoff.rs'))
    # Refer to the local VM stub from the otherwise unmodified production module.
    handoff = (ROOT / 'src/r/services/audio_http/handoff.rs').read_text()
    with tempfile.TemporaryDirectory(prefix='trueos-clip-handoff-') as work:
        directory=Path(work)
        fixture=directory/'handoff.rs'
        fixture.write_text(handoff.replace('use crate::r::services::audio_playback::{',
                                           'use crate::trueos_vm;\nuse crate::audio_playback::{'))
        harness=harness.replace(str(ROOT / 'src/r/services/audio_http/handoff.rs'),str(fixture))
        source=directory/'tests.rs'; source.write_text(harness)
        binary=directory/'tests'
        subprocess.run(['rustup','run','nightly-2026-07-10','rustc','--edition=2024','--test',str(source),
                        '--cfg','target_os="trueos"','-Aexplicit_builtin_cfgs_in_flags','-o',str(binary)],check=True)
        subprocess.run([str(binary),'--test-threads=1'],check=True)


if __name__ == '__main__':
    main()
