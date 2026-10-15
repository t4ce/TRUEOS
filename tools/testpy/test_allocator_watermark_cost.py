#!/usr/bin/env python3
"""Benchmark actual allocator watermark handlers on fixed fragmented metadata.

No kernel allocator is installed or changed. Production heap-stat traversal and
new watermark handler are extracted; the old handler differs only by removing
its new admission guard. Kernel clocks, heap lookup, host-domain entry and log
sink are fixtures. Both handlers replay the same deterministic 200ms input-clock
trace; elapsed host time is a measurement of this fixture, not a TRUEOS speedup.
"""
from pathlib import Path
import subprocess
import tempfile
from test_posix_renameat import function

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'src/allocators.rs'


def structure(source, name):
    start = source.index('struct ' + name + ' {')
    return source[start:source.index('\n}', start) + 2]


def harness():
    source = SOURCE.read_text()
    handler = function(SOURCE, 'log_hv_guest_alloc_watermark')
    marker = '    with_host_alloc_domain_strong('
    start = handler.index(marker)
    old = handler[:handler.index('\n') + 1] + handler[start:]
    old = old.replace('fn log_hv_guest_alloc_watermark(', 'fn old_log_hv_guest_alloc_watermark(', 1)
    defs = '\n'.join([
        structure(source, 'FreeBlock'), structure(source, 'FreeList'),
        '#[repr(C)]\n' + structure(source, 'AllocTag'),
        '#[derive(Clone,Copy,Debug)]\n' + structure(source, 'HeapStats'),
        function(SOURCE, 'heap_stats_from_guard'), function(SOURCE, 'align_up'),
        handler, old,
    ])
    fence_line = next(line for line in source.splitlines() if line.startswith('const FREE_ADDRESS_FENCES:'))
    return SUPPORT.replace('@ROOT@', str(ROOT)) + fence_line + '\n' + defs + TESTS


SUPPORT = r'''
#![allow(dead_code, unused_variables, unsafe_op_in_unsafe_fn)]
use std::{alloc::{Layout,alloc_zeroed,dealloc},ptr::NonNull,mem::{align_of,size_of},hint::black_box,sync::{Mutex,atomic::{AtomicU32,AtomicU64,AtomicUsize,Ordering}},time::Instant};
#[path="@ROOT@/src/allocators/watermark.rs"]mod watermark;
use watermark::SampleGate;
#[derive(Clone,Copy,Debug)]enum HeapSourceKind{Unconfigured,Arena}
unsafe impl Send for FreeList{}
impl FreeList {
 unsafe fn init_once(&mut self){panic!("fixture must initialize metadata before traversal")}
 fn ensure_heap_backing(&mut self)->(usize,usize){(self.heap_virt_start,self.heap_len)}
}
const fn minimum_block_size()->usize{size_of::<FreeBlock>()+size_of::<AllocTag>()}
const HV_GUEST_ALLOC_BUCKET_SHIFT:usize=24;
const HV_GUEST_ALLOC_BUCKET_INIT:u32=u32::MAX;
static HV_GUEST_ALLOC_FREE_BUCKET_BY_VM:[AtomicU32;1]=[AtomicU32::new(HV_GUEST_ALLOC_BUCKET_INIT)];
static HV_GUEST_ALLOC_SAMPLE_BY_VM:[SampleGate;1]=[SampleGate::new()];
static HEAP:Mutex<Option<FreeList>>=Mutex::new(None);
static CLOCK_MS:AtomicU64=AtomicU64::new(0);
static SAMPLES:AtomicUsize=AtomicUsize::new(0);
static VISITS:AtomicUsize=AtomicUsize::new(0);
static DOMAIN_ENTRIES:AtomicUsize=AtomicUsize::new(0);
mod r{pub mod input_timing{pub fn now_ms()->u64{crate::CLOCK_MS.load(std::sync::atomic::Ordering::Relaxed)}}}
fn with_host_alloc_domain_strong<T>(f:impl FnOnce()->T)->T{DOMAIN_ENTRIES.fetch_add(1,Ordering::Relaxed);f()}
fn hv_guest_heap_stats(_:u8)->HeapStats{
 let stats=heap_stats_from_guard(HEAP.lock().unwrap().as_mut().unwrap());
 SAMPLES.fetch_add(1,Ordering::Relaxed);VISITS.fetch_add(stats.free_blocks,Ordering::Relaxed);black_box(stats)
}
#[derive(Default)]struct Trace{layout_size:usize,layout_align:usize,caller_rip:usize,caller_rip_1:usize,caller_rip_2:usize}
fn last_alloc_trace()->Trace{Trace::default()}
#[macro_export]macro_rules! log_info{(target:$target:expr; $($args:tt)*)=>{{let _=format_args!($($args)*);}};}
struct Arena{ptr:NonNull<u8>,layout:Layout,first_gap:(usize,usize)}
impl Drop for Arena{fn drop(&mut self){*HEAP.lock().unwrap()=None;unsafe{dealloc(self.ptr.as_ptr(),self.layout)}}}
fn install(page_stride:bool)->Arena{
 const NODES:usize=46_920;const FREE:usize=503_319_776;const LARGE:usize=454_506_320;
 let fragments=NODES-1;let fragment_bytes=FREE-LARGE;
 let small=(fragment_bytes/fragments)&!15;let extras=(fragment_bytes-small*fragments)/16;
 assert_eq!(small*fragments+extras*16,fragment_bytes);
 let mut offsets=Vec::with_capacity(NODES);let mut cursor=0;let mut rng=0x123456789abcdefu64;let mut gap=(0,0);
 for i in 0..fragments{
  let size=small+usize::from(i<extras)*16;offsets.push((cursor,size));
  rng^=rng<<13;rng^=rng>>7;rng^=rng<<17;
  let spacing=if page_stride{4096*(1+(rng as usize&3))}else{align_up(size+32+(rng as usize%1296),16)};
  if i==0{gap=(cursor+size,cursor+spacing)}cursor+=spacing;
 }
 offsets.push((cursor,LARGE));let bytes=align_up(cursor+LARGE,4096);
 let layout=Layout::from_size_align(bytes,4096).unwrap();let ptr=NonNull::new(unsafe{alloc_zeroed(layout)}).expect("metadata arena allocation");
 let start=ptr.as_ptr() as usize;
 for(i,(offset,size))in offsets.iter().copied().enumerate(){
  let next=offsets.get(i+1).map(|(next,_)|unsafe{NonNull::new_unchecked(ptr.as_ptr().add(*next).cast::<FreeBlock>())});
  unsafe{ptr.as_ptr().add(offset).cast::<FreeBlock>().write(FreeBlock{size,next})};
 }
 *HEAP.lock().unwrap()=Some(FreeList{head:Some(ptr.cast()),address_fences:[None;FREE_ADDRESS_FENCES],initialized:true,heap_virt_start:start,heap_len:bytes,heap_phys_start:0,heap_source:HeapSourceKind::Arena});
 let stats=hv_guest_heap_stats(0);assert_eq!((stats.free_blocks,stats.free_bytes,stats.largest_free_block),(NODES,FREE,LARGE));
 Arena{ptr,layout,first_gap:(start+gap.0,start+gap.1)}
}
fn reset_counts(){SAMPLES.store(0,Ordering::Relaxed);VISITS.store(0,Ordering::Relaxed);DOMAIN_ENTRIES.store(0,Ordering::Relaxed);HV_GUEST_ALLOC_FREE_BUCKET_BY_VM[0].store(HV_GUEST_ALLOC_BUCKET_INIT,Ordering::Relaxed)}
fn measure(label:&str,base:u64,handler:fn(u8,Layout,*mut u8,&str))->(usize,usize){
 reset_counts();let layout=Layout::from_size_align(64,8).unwrap();let start=Instant::now();
 for offset in 0..200{CLOCK_MS.store(black_box(base+offset),Ordering::Relaxed);handler(black_box(0),black_box(layout),std::ptr::null_mut(),"global")}
 let ns=start.elapsed().as_nanos();let samples=SAMPLES.load(Ordering::Relaxed);let visits=VISITS.load(Ordering::Relaxed);let domains=DOMAIN_ENTRIES.load(Ordering::Relaxed);
 println!("watermark_fixture case={label} allocation_calls=200 logical_clock_span_ms=199 samples={samples} node_visits={visits} host_domain_entries={domains} host_elapsed_ns={ns}");
 (samples,visits)
}
'''

TESTS = r'''
#[test]fn production_handler_admission_bounds_actual_fragmented_stats_walks(){
 for(index,page_stride)in [false,true].into_iter().enumerate(){
  let arena=install(page_stride);let shape=if page_stride{"page_stride_46920"}else{"irregular_46920"};let base=index as u64*10_000;
  assert!(arena.first_gap.1-arena.first_gap.0>=minimum_block_size());
  println!("watermark_fixture geometry={shape} free_nodes=46920 free_bytes=503319776 largest_free_bytes=454506320 reserved_arena_bytes={} metadata_payload_only=true",arena.layout.size());
  let old=measure(&format!("{shape}_old"),base,old_log_hv_guest_alloc_watermark);
  let new=measure(&format!("{shape}_gated"),base,log_hv_guest_alloc_watermark);
  assert_eq!(old,(200,200*46_920));assert_eq!(new,(1,46_920));assert_eq!(DOMAIN_ENTRIES.load(Ordering::Relaxed),1);
  CLOCK_MS.store(base+1000,Ordering::Relaxed);log_hv_guest_alloc_watermark(0,Layout::from_size_align(64,8).unwrap(),std::ptr::null_mut(),"global");
  assert_eq!(SAMPLES.load(Ordering::Relaxed),2);
  CLOCK_MS.store(base+1001,Ordering::Relaxed);log_hv_guest_alloc_watermark(0,Layout::from_size_align(1024*1024,8).unwrap(),std::ptr::null_mut(),"global");
  assert_eq!(SAMPLES.load(Ordering::Relaxed),3,"large allocations retain immediate fullstats");
  drop(arena);
 }
}
'''


def main():
    with tempfile.TemporaryDirectory(prefix='allocator-watermark-') as directory:
        rust = Path(directory) / 'test.rs'
        binary = Path(directory) / 'test'
        rust.write_text(harness())
        subprocess.run(['rustc', '--edition=2024', '-O', '--test', str(rust), '-o', str(binary)], check=True)
        subprocess.run([str(binary), '--nocapture', '--test-threads=1'], check=True, timeout=60)


if __name__ == '__main__':
    main()
