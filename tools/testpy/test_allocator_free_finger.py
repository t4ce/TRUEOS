#!/usr/bin/env python3
"""Exercise the production allocator's cached insertion predecessor.

Reuse the real allocator and its independent interval-model tests. Added
cases cover cache lifetime, invalid pointers, coalescing, provenance and
large concentrated destruction batches. The control clears only the optional
hint before every free; it retains the identical fence/validation algorithm.
Counter-instrumented host timings do not establish physical TRUEOS timings.
"""

from pathlib import Path
import subprocess
import tempfile

from test_allocator_address_index import harness


TESTS = r'''
#[test]fn cached_predecessor_tracks_the_actual_coalescing_survivor(){
 let mut arena=Arena::new(1048576);let domain=AllocDomain::HvGuest(2);
 let a=unsafe{arena.heap.alloc(domain,Layout::from_size_align(128,16).unwrap())};
 let b=unsafe{arena.heap.alloc(domain,Layout::from_size_align(128,16).unwrap())};
 let c=unsafe{arena.heap.alloc(domain,Layout::from_size_align(128,16).unwrap())};
 let tag_a=unsafe{a.sub(size_of::<AllocTag>()).cast::<AllocTag>().read()};
 unsafe{arena.heap.dealloc(domain,a)};
 assert_eq!(arena.heap.free_finger.unwrap().as_ptr()as usize,tag_a.block_start);
 unsafe{arena.heap.dealloc(domain,b)};
 assert_eq!(arena.heap.free_finger.unwrap().as_ptr()as usize,tag_a.block_start,"previous-node merge must not leave a dangling hint");
 let cache=arena.heap.free_finger;let before=intervals(&arena.heap);
 unsafe{arena.heap.dealloc(AllocDomain::HvGuest(3),c)};
 assert_eq!(intervals(&arena.heap),before,"wrong-domain free must remain rejected");
 assert_eq!(arena.heap.free_finger,cache);
 unsafe{arena.heap.dealloc(domain,c)};
 assert_eq!(arena.heap.free_finger,arena.heap.head);
 check_index(&arena.heap);assert_eq!(intervals(&arena.heap).len(),1);
}

#[test]fn every_allocation_invalidates_the_hint_before_first_fit_mutation(){
 let mut arena=Arena::new(1048576);let domain=AllocDomain::HvGuest(2);
 let a=unsafe{arena.heap.alloc(domain,Layout::from_size_align(128,16).unwrap())};
 let b=unsafe{arena.heap.alloc(domain,Layout::from_size_align(128,16).unwrap())};
 unsafe{arena.heap.dealloc(domain,a)};
 assert!(arena.heap.free_finger.is_some());
 let replacement=unsafe{arena.heap.alloc(domain,Layout::from_size_align(128,16).unwrap())};
 assert_eq!(replacement,a,"first-fit choice must remain unchanged");
 assert!(arena.heap.free_finger.is_none());
 unsafe{arena.heap.dealloc(domain,b)};
 unsafe{arena.heap.dealloc(domain,replacement)};
 check_index(&arena.heap);assert_eq!(intervals(&arena.heap).len(),1);
 // Invalid hints also disappear before an allocation fails for lack of space.
 arena.heap.free_finger=NonNull::new(7 as*mut FreeBlock);
 assert!(unsafe{arena.heap.alloc(domain,Layout::from_size_align(2*1048576,16).unwrap())}.is_null());
 assert!(arena.heap.free_finger.is_none());
}

#[test]fn cached_pointer_validation_and_existing_fence_validation_remain_fail_closed(){
 let mut arena=Arena::new(1048576);let domain=AllocDomain::HvGuest(2);
 let a=unsafe{arena.heap.alloc(domain,Layout::from_size_align(128,16).unwrap())};
 let b=unsafe{arena.heap.alloc(domain,Layout::from_size_align(128,16).unwrap())};
 let tag=unsafe{b.sub(size_of::<AllocTag>()).cast::<AllocTag>().read()};
 unsafe{arena.heap.dealloc(domain,a)};
 let valid=arena.heap.free_finger;
 assert!(unsafe{arena.heap.allocation_block_is_live(tag.block_start,tag.block_start+tag.block_size)});
 for pointer in [7usize,arena.ptr.as_ptr()as usize-16,valid.unwrap().as_ptr()as usize+1]{
  arena.heap.free_finger=NonNull::new(pointer as*mut FreeBlock);
  assert!(!unsafe{arena.heap.allocation_block_is_live(tag.block_start,tag.block_start+tag.block_size)});
 }
 arena.heap.free_finger=valid;
 let node=valid.unwrap();let next=unsafe{node.as_ref().next};
 unsafe{(*node.as_ptr()).next=Some(node)};
 assert!(!unsafe{arena.heap.allocation_block_is_live(tag.block_start,tag.block_start+tag.block_size)});
 unsafe{(*node.as_ptr()).next=next};
 let before=intervals(&arena.heap);unsafe{arena.heap.dealloc(domain,a)};
 assert_eq!(intervals(&arena.heap),before,"a cached node cannot authorize a double free");
 unsafe{arena.heap.dealloc(domain,b)};check_index(&arena.heap);

 // A valid hint must not hide a corrupt original address fence.
 let mut arena=Arena::new(65536);
 let a=unsafe{arena.heap.alloc(domain,Layout::from_size_align(1024,16).unwrap())};
 let b=unsafe{arena.heap.alloc(domain,Layout::from_size_align(1024,16).unwrap())};
 let tag=unsafe{b.sub(size_of::<AllocTag>()).cast::<AllocTag>().read()};
 unsafe{arena.heap.dealloc(domain,a)};
 let band=arena.heap.address_band(tag.block_start);assert!(band>0);
 let previous=arena.heap.address_fences[band];arena.heap.address_fences[band]=NonNull::new(7 as*mut FreeBlock);
 assert!(!unsafe{arena.heap.allocation_block_is_live(tag.block_start,tag.block_start+tag.block_size)});
 arena.heap.address_fences[band]=previous;unsafe{arena.heap.dealloc(domain,b)};check_index(&arena.heap);
}

#[test]fn heap_rebuild_and_replacement_discard_stale_cached_nodes(){
 let mut arena=Arena::new(1048576);let domain=AllocDomain::HvGuest(2);
 let a=unsafe{arena.heap.alloc(domain,Layout::from_size_align(128,16).unwrap())};
 unsafe{arena.heap.dealloc(domain,a)};assert!(arena.heap.free_finger.is_some());
 unsafe{arena.heap.rebuild_address_fences()};assert!(arena.heap.free_finger.is_none());check_index(&arena.heap);
 arena.heap.free_finger=NonNull::new(7 as*mut FreeBlock);
 arena.heap.install_heap(arena.ptr.as_ptr()as usize,0,arena.layout.size());assert!(arena.heap.free_finger.is_none());
 let a=unsafe{arena.heap.alloc(domain,Layout::from_size_align(128,16).unwrap())};
 unsafe{arena.heap.dealloc(domain,a)};check_index(&arena.heap);assert_eq!(intervals(&arena.heap).len(),1);
}

#[test]fn concentrated_batched_destruction_reduces_repeated_prefix_walks(){
 const N:usize=70_000;
 let mut counts=[[0usize;3];2];
 for control in 0..2{for order in 0..3{
  let mut arena=Arena::new(128*1024*1024);let domain=AllocDomain::HvGuest(2);let mut live=Vec::with_capacity(N);
  for i in 0..N{let p=unsafe{arena.heap.alloc(domain,Layout::from_size_align(16+i%5*16,16).unwrap())};assert!(!p.is_null());live.push(p);}
  let mut indices:Vec<_>=(0..N).step_by(2).collect();
  if order==1{indices.reverse()}
  if order==2{let mut rng=0x517cfbed4213a671u64;for i in(1..indices.len()).rev(){let j=(next_random(&mut rng)as usize)%(i+1);indices.swap(i,j)}}
  VISITS.store(0,Ordering::Relaxed);let started=Instant::now();
  for &i in &indices{if control==0{arena.heap.free_finger=None}unsafe{arena.heap.dealloc(domain,live[i])}}
  let elapsed=started.elapsed().as_nanos();let visits=VISITS.load(Ordering::Relaxed);counts[control][order]=visits;
  println!("allocator_free_finger control={control} order={order} calls={} node_visits={visits} instrumented_host_ns={elapsed} physical_timing=0",indices.len());check_index(&arena.heap);
  for i in(1..N).step_by(2){unsafe{arena.heap.dealloc(domain,live[i])}}
  check_index(&arena.heap);assert_eq!(intervals(&arena.heap).len(),1);
 }}
 assert!(counts[1][0]<=3*(N/2));assert!(counts[1][0]<counts[0][0]/400);
 assert_eq!(counts[1][1],counts[0][1]);assert!(counts[1][2]<=counts[0][2]);
}
'''


def main():
    with tempfile.TemporaryDirectory(prefix="allocator-free-finger-") as directory:
        source = Path(directory) / "free_finger.rs"
        executable = Path(directory) / "free_finger"
        source.write_text(harness() + TESTS)
        subprocess.run([
            "rustc", "--edition=2024", "-C", "opt-level=1", "--test",
            str(source), "-o", str(executable),
        ], check=True)
        subprocess.run([str(executable), "--nocapture", "--test-threads=1"], check=True, timeout=120)


if __name__ == "__main__":
    main()
