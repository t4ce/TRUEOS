#!/usr/bin/env python3
"""Exercise the production allocator against an independent interval model.

The allocator is extracted verbatim; only kernel diagnostics/domain providers
are replaced. Host memory backs the arena. No kernel or physical rig is used.
The fragmented fixture replays the observed 46,920-node geometry and compares
the prior full-list live check to the indexed production check at address
quantiles. Elapsed times are host measurements, not a TRUEOS speedup claim.
"""
from pathlib import Path
import subprocess
import tempfile
import textwrap
from test_posix_renameat import function

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "src/allocators.rs"


def structure(source, name):
    start = source.index("struct " + name + " {")
    return source[start:source.index("\n}", start) + 2]


def harness():
    source = SOURCE.read_text()
    start = source.index("impl FreeList {")
    end = source.index("\nstruct Allocator;", start)
    production = "\n".join([
        "#[repr(C)]\n" + structure(source, "FreeBlock"),
        "#[repr(C)]\n#[derive(Clone,Copy)]\n" + structure(source, "AllocTag"),
        structure(source, "FreeList"),
        source[start:end],
        *[function(SOURCE, name) for name in ("trace_alloc_block", "trace_alloc_compare")],
        source[source.index("const fn minimum_block_size()"):source.index("\n}", source.index("const fn minimum_block_size()")) + 2],
        *[function(SOURCE, name) for name in ("aligned_payload", "align_up")],
    ])
    fence_line = next(line for line in source.splitlines() if line.startswith("const FREE_ADDRESS_FENCES:"))
    # Replay the previous tracing policy against the same indexed allocator;
    # this isolates trace cost from first-fit and index maintenance cost.
    alloc_start = source.index("    unsafe fn alloc(&mut self")
    alloc_end = source.index("\n    }\n", alloc_start) + len("\n    }\n")
    prior = textwrap.dedent(source[alloc_start:alloc_end]).replace("unsafe fn alloc(", "unsafe fn old_traced_alloc(", 1)
    selected = """            trace_alloc_block(trace_enabled, block, block_start, payload_start, aligned_used);
            trace_alloc_compare(trace_enabled);"""
    # function() dedents production methods by four spaces.
    selected = selected[4:].replace("\n            ", "\n        ")
    if selected not in prior:
        raise ValueError("selected trace boundary changed")
    prior = prior.replace(selected, "", 1)
    prior = prior.replace("        if aligned_used > block.size {", selected + "\n\n        if aligned_used > block.size {", 1)
    production += "\nimpl FreeList {\n" + prior + "\n}\n"
    # Count actual metadata reads, including the one predecessor examined
    # before entering the band, without changing branches or list geometry.
    marker = production.index("unsafe fn checked_free_block_end(")
    production = production[:marker] + production[marker:].replace(
        "let free = unsafe { node.as_ref() };",
        "VISITS.fetch_add(1,Ordering::Relaxed); let free = unsafe { node.as_ref() };", 1)
    return SUPPORT + fence_line + "\n" + production + TESTS


SUPPORT = r'''
#![allow(dead_code,unused_variables,unsafe_op_in_unsafe_fn)]
use std::{alloc::{Layout,alloc_zeroed,dealloc},ptr::{NonNull,null_mut},mem::{align_of,size_of},sync::atomic::{AtomicUsize,AtomicU32,Ordering},hint::black_box,time::Instant};
static VISITS:AtomicUsize=AtomicUsize::new(0);
static ALLOC_TRACE_STAGE:AtomicU32=AtomicU32::new(0);
static ALLOC_TRACE_BLOCK_PTR:AtomicUsize=AtomicUsize::new(0);
static ALLOC_TRACE_BLOCK_SIZE:AtomicUsize=AtomicUsize::new(0);
static ALLOC_TRACE_BLOCK_NEXT:AtomicUsize=AtomicUsize::new(0);
static ALLOC_TRACE_PAYLOAD:AtomicUsize=AtomicUsize::new(0);
static ALLOC_TRACE_ALIGNED_USED:AtomicUsize=AtomicUsize::new(0);
const ALLOC_TRACE_STAGE_ENTRY:u32=1;const ALLOC_TRACE_STAGE_BLOCK:u32=2;const ALLOC_TRACE_STAGE_COMPARE:u32=3;const ALLOC_TRACE_STAGE_SUCCESS:u32=4;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]enum AllocDomain{Host,HvGuest(u8)}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]enum HeapSourceKind{Unconfigured,Arena}
struct HvGuestDmaPin{vm_id:u8,ptr:usize,bytes:usize,block_start:usize,block_size:usize}
mod hv{pub fn current_guest_execution_context_vm_id()->Option<u8>{None}}
fn alloc_domain_tag(d:AllocDomain)->u8{match d{AllocDomain::Host=>0,AllocDomain::HvGuest(id)=>id+1}}
fn alloc_domain_from_tag(t:&AllocTag)->Option<AllocDomain>{match t.domain{0=>Some(AllocDomain::Host),id if id<65=>Some(AllocDomain::HvGuest(id-1)),_=>None}}
fn log_alloc_domain_mismatch(_: *mut u8,_:Option<AllocDomain>,_:Option<u8>){}
fn trace_alloc_entry(_:bool,_:Layout,_:Option<NonNull<FreeBlock>>){ALLOC_TRACE_STAGE.store(ALLOC_TRACE_STAGE_ENTRY,Ordering::Release)}
fn trace_alloc_invalid_ptr(_:bool,_:usize){}
fn trace_alloc_success(_:bool){ALLOC_TRACE_STAGE.store(ALLOC_TRACE_STAGE_SUCCESS,Ordering::Release)}
#[macro_export]macro_rules! log{($($args:tt)*)=>{{let _=format_args!($($args)*);}};}
struct Arena{ptr:NonNull<u8>,layout:Layout,heap:FreeList}
impl Arena{fn new(bytes:usize)->Self{let layout=Layout::from_size_align(bytes,4096).unwrap();let ptr=NonNull::new(unsafe{alloc_zeroed(layout)}).unwrap();let mut heap=FreeList::new();heap.install_heap(ptr.as_ptr() as usize,0,bytes);Self{ptr,layout,heap}}}
impl Drop for Arena{fn drop(&mut self){unsafe{dealloc(self.ptr.as_ptr(),self.layout)}}}
fn intervals(heap:&FreeList)->Vec<(usize,usize)>{let mut ranges=vec![];let mut current=heap.head;while let Some(node)=current{assert!(ranges.len()<100_000);let b=unsafe{node.as_ref()};ranges.push((node.as_ptr() as usize,b.size));current=b.next;}ranges}
fn check_index(heap:&FreeList){let ranges=intervals(heap);let mut expected=[None;FREE_ADDRESS_FENCES];for &(start,_)in &ranges{for k in heap.address_band(start)+1..FREE_ADDRESS_FENCES{expected[k]=NonNull::new(start as *mut FreeBlock)}}assert_eq!(heap.address_fences,expected);for pair in ranges.windows(2){assert!(pair[0].0+pair[0].1<pair[1].0,"adjacent nodes must coalesce");}}
unsafe fn old_live(heap:&mut FreeList,start:usize,end:usize)->bool{let mut current=heap.head;let mut remaining=heap.heap_len/minimum_block_size()+1;while let Some(node)=current{if remaining==0||!heap.is_plausible_free_block_ptr(node.as_ptr() as usize){return false}remaining-=1;VISITS.fetch_add(1,Ordering::Relaxed);let b=unsafe{node.as_ref()};let Some(free_end)=(node.as_ptr() as usize).checked_add(b.size)else{return false};if start<free_end&&(node.as_ptr() as usize)<end{return false}current=b.next;}true}
fn next_random(state:&mut u64)->usize{*state^=*state<<13;*state^=*state>>7;*state^=*state<<17;*state as usize}
'''

TESTS = r'''
#[test]fn mixed_alloc_free_realloc_queries_match_interval_model(){
 let mut arena=Arena::new(8*1024*1024);let mut rng=0x123456789abcdefu64;let domain=AllocDomain::HvGuest(1);let mut live=Vec::<(usize,usize,usize,usize)>::new();
 for iteration in 0..25_000{
  let random=next_random(&mut rng);
  if live.is_empty()||(random%100<58&&live.len()<1400){
   let size=1+next_random(&mut rng)%8192;let align=1<<(next_random(&mut rng)%10);let ptr=unsafe{arena.heap.alloc(domain,Layout::from_size_align(size,align).unwrap())};
   if !ptr.is_null(){assert_eq!(ptr as usize%align,0);let tag=unsafe{ptr.sub(size_of::<AllocTag>()).cast::<AllocTag>().read()};for &(start,end,_,_)in &live{assert!(tag.block_start+tag.block_size<=start||end<=tag.block_start);}unsafe{std::ptr::write_bytes(ptr,iteration as u8,size)};live.push((tag.block_start,tag.block_start+tag.block_size,ptr as usize,size));}
  }else{
   let i=next_random(&mut rng)%live.len();let(start,end,ptr,size)=live.swap_remove(i);assert_eq!(unsafe{arena.heap.allocation_usable_size(domain,ptr as *const u8)},Some(end-ptr));
   let before=intervals(&arena.heap);unsafe{arena.heap.dealloc(AllocDomain::Host,ptr as *mut u8)};assert_eq!(intervals(&arena.heap),before,"wrong domain must not free");
   unsafe{arena.heap.dealloc(domain,ptr as *mut u8)};assert_eq!(unsafe{arena.heap.allocation_usable_size(domain,ptr as *const u8)},None);
   let after=intervals(&arena.heap);unsafe{arena.heap.dealloc(domain,ptr as *mut u8)};assert_eq!(intervals(&arena.heap),after,"double free must not mutate list");
  }
  check_index(&arena.heap);
  if iteration%101==0{for &(start,end,_,_)in &live{assert!(unsafe{arena.heap.allocation_block_is_live(start,end)});}}
 }
 for(_,_,ptr,_)in live{unsafe{arena.heap.dealloc(domain,ptr as *mut u8)}}check_index(&arena.heap);assert_eq!(intervals(&arena.heap),vec![(arena.ptr.as_ptr() as usize,arena.layout.size())]);
}
#[test]fn forged_interior_and_free_tags_cannot_reclaim_pages(){
 let mut arena=Arena::new(1024*1024);let domain=AllocDomain::HvGuest(3);let p=unsafe{arena.heap.alloc(domain,Layout::from_size_align(32768,4096).unwrap())};let tag=unsafe{p.sub(size_of::<AllocTag>()).cast::<AllocTag>().read()};
 let interior=unsafe{p.add(4096)};unsafe{interior.sub(size_of::<AllocTag>()).cast::<AllocTag>().write(tag)};let before=intervals(&arena.heap);unsafe{arena.heap.dealloc(domain,interior)};assert_eq!(intervals(&arena.heap),before);
 unsafe{arena.heap.dealloc(domain,p)};check_index(&arena.heap);let before=intervals(&arena.heap);assert_eq!(unsafe{arena.heap.pin_dma_range(domain,p as usize,4096)},None);unsafe{arena.heap.dealloc(domain,p)};assert_eq!(intervals(&arena.heap),before);
}
#[test]fn dma_pins_retain_pages_until_matching_unpin(){
 let mut arena=Arena::new(1024*1024);let domain=AllocDomain::HvGuest(2);let p=unsafe{arena.heap.alloc(domain,Layout::from_size_align(16384,4096).unwrap())};let(start,size)=unsafe{arena.heap.pin_dma_range(domain,p as usize,8192)}.unwrap();let before=intervals(&arena.heap);
 unsafe{arena.heap.dealloc(domain,p)};assert_eq!(intervals(&arena.heap),before);assert_eq!(unsafe{arena.heap.pin_dma_range(AllocDomain::HvGuest(3),p as usize,4096)},None);assert_eq!(unsafe{arena.heap.pin_dma_range(domain,p as usize,usize::MAX)},None);
 let pin=HvGuestDmaPin{vm_id:2,ptr:p as usize,bytes:8192,block_start:start,block_size:size};assert!(unsafe{arena.heap.unpin_dma_range(&pin)});assert!(!unsafe{arena.heap.unpin_dma_range(&pin)});unsafe{arena.heap.dealloc(domain,p)};check_index(&arena.heap);assert_eq!(intervals(&arena.heap).len(),1);
}
#[test]fn index_rebuild_restore_and_reset_preserve_list_and_layout(){
 assert_eq!(size_of::<FreeBlock>(),16);assert_eq!(size_of::<AllocTag>(),32);assert!(size_of::<FreeList>()+64<4096);
 let mut arena=Arena::new(1024*1024);let mut live=vec![];for _ in 0..512{live.push(unsafe{arena.heap.alloc(AllocDomain::Host,Layout::from_size_align(512,8).unwrap())});}for i in (0..512).step_by(2){unsafe{arena.heap.dealloc(AllocDomain::Host,live[i])}}let expected=arena.heap.address_fences;
 arena.heap.address_fences.fill(None);unsafe{arena.heap.rebuild_address_fences()};assert_eq!(arena.heap.address_fences,expected);check_index(&arena.heap);arena.heap.install_heap(arena.ptr.as_ptr() as usize,0,arena.layout.size());assert!(arena.heap.address_fences.iter().all(Option::is_none));assert!(arena.heap.head.is_none());
}
#[test]fn malformed_visited_nodes_fail_closed_without_dereferencing_invalid_links(){
 for mode in 0..4{let mut arena=Arena::new(1024*1024);let p=unsafe{arena.heap.alloc(AllocDomain::Host,Layout::from_size_align(64,8).unwrap())};let tag=unsafe{p.sub(size_of::<AllocTag>()).cast::<AllocTag>().read()};let node=arena.heap.head.unwrap();unsafe{match mode{0=>(*node.as_ptr()).size=usize::MAX,1=>(*node.as_ptr()).next=Some(node),2=>(*node.as_ptr()).next=NonNull::new(7 as *mut FreeBlock),_=>(*node.as_ptr()).size=1}}assert!(!unsafe{arena.heap.allocation_block_is_live(tag.block_start,tag.block_start+tag.block_size)});}
}
#[test]fn reviewer_boundary_geometries_preserve_every_fence(){
 for bytes in [64usize,65,128,129,255,256,257,511,512,513,4095,4096,4097,65521,1048561]{
  let mut arena=Arena::new(bytes);let mut live=Vec::new();
  for i in 0..2000{let p=unsafe{arena.heap.alloc(AllocDomain::Host,Layout::from_size_align(1+(i%113),8).unwrap())};if p.is_null(){break}live.push(p);check_index(&arena.heap);}
  while !live.is_empty(){let middle=live.len()/2;let p=live.remove(middle);unsafe{arena.heap.dealloc(AllocDomain::Host,p)};check_index(&arena.heap);}
  assert_eq!(intervals(&arena.heap),vec![(arena.ptr.as_ptr() as usize,bytes)]);
 }
}
#[test]fn reviewer_corrupt_fence_pointers_fail_closed(){
 let mut arena=Arena::new(1048576);let p=unsafe{arena.heap.alloc(AllocDomain::Host,Layout::from_size_align(65536,8).unwrap())};let tag=unsafe{p.sub(size_of::<AllocTag>()).cast::<AllocTag>().read()};let query=tag.block_start+32768;let band=arena.heap.address_band(query);assert!(band>0);
 for pointer in [7usize,arena.ptr.as_ptr() as usize-4096,arena.ptr.as_ptr() as usize+1048576,query]{arena.heap.address_fences[band]=NonNull::new(pointer as *mut FreeBlock);assert!(!unsafe{arena.heap.allocation_block_is_live(query,query+64)});}
}
#[test]fn fragmented_quantiles_measure_actual_indexed_searches(){
 const N:usize=46_920;const FREE:usize=503_319_776;const LARGE:usize=454_506_320;
 for page_stride in [false,true]{let fragments=N-1;let bytes=FREE-LARGE;let small=(bytes/fragments)&!15;let extra=(bytes-small*fragments)/16;let mut offsets=vec![];let mut cursor=0;let mut rng=0x123456789abcdefu64;
  for i in 0..fragments{let size=small+usize::from(i<extra)*16;let noise=next_random(&mut rng);let stride=if page_stride{4096*(1+(noise&3))}else{align_up(size+32+noise%1296,16)};offsets.push((cursor,size,cursor+stride));cursor+=stride;}offsets.push((cursor,LARGE,cursor+LARGE));let mut arena=Arena::new(align_up(cursor+LARGE,4096));let start=arena.ptr.as_ptr() as usize;
  for(i,&(offset,size,_))in offsets.iter().enumerate(){let next=offsets.get(i+1).map(|&(n,_,_)|unsafe{NonNull::new_unchecked((start+n) as *mut FreeBlock)});unsafe{((start+offset) as *mut FreeBlock).write(FreeBlock{size,next})}}arena.heap.head=NonNull::new(start as *mut FreeBlock);arena.heap.initialized=true;unsafe{arena.heap.rebuild_address_fences()};check_index(&arena.heap);
  for q in [0,25,50,75,99]{let i=(fragments-1)*q/100;let(o,size,end)=offsets[i];let a=start+o+size;let b=start+end;assert!(b-a>=minimum_block_size());
   VISITS.store(0,Ordering::Relaxed);let began=Instant::now();for _ in 0..200{assert!(unsafe{old_live(&mut arena.heap,black_box(a),black_box(b))});}let old_ns=began.elapsed().as_nanos();let old_visits=VISITS.load(Ordering::Relaxed);
   VISITS.store(0,Ordering::Relaxed);let began=Instant::now();for _ in 0..200{assert!(unsafe{arena.heap.allocation_block_is_live(black_box(a),black_box(b))});}let new_ns=began.elapsed().as_nanos();let new_visits=VISITS.load(Ordering::Relaxed);assert!(new_visits<old_visits/20);
   println!("allocator_index fixture={} free_nodes={N} quantile={q} calls=200 old_node_visits={old_visits} indexed_node_visits={new_visits} old_host_ns={old_ns} indexed_host_ns={new_ns}",if page_stride{"page_stride"}else{"irregular"});
  }
 }
}
#[test]fn first_fit_trace_cost_is_bounded_to_selected_or_last_failed_candidate(){
 const N:usize=46_920;const STRIDE:usize=128;let mut arena=Arena::new(N*STRIDE+1024*1024);let start=arena.ptr.as_ptr() as usize;
 for i in 0..=N{let offset=i*STRIDE;let size=if i==N{arena.layout.size()-offset}else{64};let next=if i==N{None}else{NonNull::new((start+offset+STRIDE) as *mut FreeBlock)};unsafe{((start+offset) as *mut FreeBlock).write(FreeBlock{size,next})}}
 arena.heap.head=NonNull::new(start as *mut FreeBlock);arena.heap.initialized=true;unsafe{arena.heap.rebuild_address_fences()};let layout=Layout::from_size_align(4096,8).unwrap();
 let began=Instant::now();for _ in 0..100{let p=unsafe{arena.heap.old_traced_alloc(AllocDomain::Host,layout)};assert_eq!(p as usize,start+N*STRIDE+48);unsafe{arena.heap.dealloc(AllocDomain::Host,p)}}let old_ns=began.elapsed().as_nanos();
 let began=Instant::now();for _ in 0..100{let p=unsafe{arena.heap.alloc(AllocDomain::Host,layout)};assert_eq!(p as usize,start+N*STRIDE+48);assert_eq!(ALLOC_TRACE_STAGE.load(Ordering::Acquire),ALLOC_TRACE_STAGE_SUCCESS);assert_eq!(ALLOC_TRACE_BLOCK_PTR.load(Ordering::Acquire),start+N*STRIDE);unsafe{arena.heap.dealloc(AllocDomain::Host,p)}}let new_ns=began.elapsed().as_nanos();
 println!("allocator_trace free_nodes={} allocation_calls=100 old_candidate_atomic_stores={} bounded_candidate_atomic_stores=700 old_host_ns={old_ns} bounded_host_ns={new_ns}",N+1,7*(N+1)*100);
 let impossible=Layout::from_size_align(arena.layout.size(),8).unwrap();assert!(unsafe{arena.heap.alloc(AllocDomain::Host,impossible)}.is_null());assert_eq!(ALLOC_TRACE_STAGE.load(Ordering::Acquire),ALLOC_TRACE_STAGE_COMPARE);assert_eq!(ALLOC_TRACE_BLOCK_PTR.load(Ordering::Acquire),start+N*STRIDE);check_index(&arena.heap);
}
'''


def main():
    with tempfile.TemporaryDirectory(prefix="allocator-index-") as directory:
        source = Path(directory) / "test.rs"
        binary = Path(directory) / "test"
        source.write_text(harness())
        subprocess.run(["rustc", "--edition=2024", "-O", "--test", str(source), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--nocapture", "--test-threads=1"], check=True, timeout=120)


if __name__ == "__main__":
    main()
