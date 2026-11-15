#!/usr/bin/env python3
"""Check the real clip ABI/VM boundary and TTSTT collector together on the host."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
BLUEPRINTS = ROOT.parent / 'TRUEOS-Blueprints'


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
static SINK:Mutex<sink::Sink>=Mutex::new(sink::Sink::new());
#[path="@HANDOFF@"] mod handoff;
#[path="@PLAYBACK@"] mod playback;
mod ttstt_service { pub type AudioSink=Box<dyn FnMut(&[i16])->Result<(), &'static str>+Send>; }
static MODE:AtomicU8=AtomicU8::new(0);
static START:AtomicUsize=AtomicUsize::new(0);
static END:AtomicUsize=AtomicUsize::new(0);
mod hv {
    pub fn current_hull_guest_context_vm_id()->Option<u8> {
        (super::MODE.load(super::Ordering::SeqCst)==1).then_some(7)
    }
    pub fn current_guest_execution_context_vm_id()->Option<u8> {
        (super::MODE.load(super::Ordering::SeqCst)==2).then_some(7)
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
        if p.is_null() {None} else {Some(unsafe {std::slice::from_raw_parts(p,n)})}
    }
}
mod trueos_vm { pub mod vmcall {
    pub const OP_BP_HTTP_AUDIO_CLIP_SUBMIT_V1:u32=0x259;
    pub const STATUS_OK:u32=0;
    pub fn call(op:u32,address:u64,bytes:u64)->(u32,u64) {
        assert_eq!(op,OP_BP_HTTP_AUDIO_CLIP_SUBMIT_V1);
        (STATUS_OK,crate::handoff::submit_guest(7,address,bytes) as i64 as u64)
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
        let mut out=[0;8]; SINK.lock().render(&mut out); assert_eq!(out,bytes.as_slice());
    }
    MODE.store(0,Ordering::SeqCst);
    // Real TTSTT collector -> actual exported handoff -> owned sink -> HTTP PCM.
    let speech=playback::Playback::open().unwrap();
    speech.sink()(&[123,-456]).unwrap();
    speech.sink()(&[789,-1011]).unwrap();
    let mut silent=[1;8]; SINK.lock().render(&mut silent); assert_eq!(silent,[0;8]);
    speech.handoff().unwrap(); drop(speech);
    let competing=playback::Playback::open().unwrap();
    competing.sink()(&[9,9]).unwrap(); competing.handoff().unwrap(); drop(competing);
    let mut out=[0;8]; SINK.lock().render(&mut out);
    assert_eq!(out,[123,0,56,254,21,3,13,252]);
    SINK.lock().render(&mut out); assert_eq!(out,[0;8]);
}
'''.replace('@SINK@', str(ROOT / 'src/r/services/audio_http/sink.rs')).replace('@HANDOFF@',str(ROOT / 'src/r/services/audio_http/handoff.rs')).replace('@PLAYBACK@',str(BLUEPRINTS / 'apps/ttstt/src/playback.rs'))
    # Refer to the local VM stub from the otherwise unmodified production module.
    handoff = (ROOT / 'src/r/services/audio_http/handoff.rs').read_text()
    with tempfile.TemporaryDirectory(prefix='trueos-clip-handoff-') as work:
        directory=Path(work)
        fixture=directory/'handoff.rs'
        fixture.write_text(handoff.replace('use super::{SINK, sink};','use super::{SINK, sink};\nuse crate::trueos_vm;'))
        harness=harness.replace(str(ROOT / 'src/r/services/audio_http/handoff.rs'),str(fixture))
        source=directory/'tests.rs'; source.write_text(harness)
        binary=directory/'tests'
        subprocess.run(['rustup','run','nightly-2026-07-10','rustc','--edition=2024','--test',str(source),
                        '--cfg','target_os="trueos"','-Aexplicit_builtin_cfgs_in_flags','-o',str(binary)],check=True)
        subprocess.run([str(binary)],check=True)


if __name__ == '__main__':
    main()
