#!/usr/bin/env python3
"""Validate passive first-fit work counters on the production allocator."""
from pathlib import Path
import subprocess
import tempfile

from test_allocator_address_index import SOURCE, harness, structure
from test_posix_renameat import function


TESTS = r'''
unsafe impl Send for FreeList {}
struct TestMutex(std::sync::Mutex<FreeList>);
impl TestMutex {
 fn try_lock(&self)->Option<std::sync::MutexGuard<'_,FreeList>>{self.0.try_lock().ok()}
}
struct TestPage{allocator:TestMutex}
static TEST_PAGE:std::sync::LazyLock<TestPage>=std::sync::LazyLock::new(||TestPage{allocator:TestMutex(std::sync::Mutex::new(FreeList::new()))});
fn hv_guest_allocator_page(vm:u8)->Option<&'static TestPage>{(vm==0).then(||&*TEST_PAGE)}

#[test]fn work_counts_every_first_fit_candidate_once_without_changing_choice(){
 const N:usize=123;const STRIDE:usize=128;
 let mut arena=Arena::new(N*STRIDE+65536);let start=arena.ptr.as_ptr()as usize;
 for i in 0..=N{let offset=i*STRIDE;let size=if i==N{arena.layout.size()-offset}else{64};let next=if i==N{None}else{NonNull::new((start+offset+STRIDE)as*mut FreeBlock)};unsafe{((start+offset)as*mut FreeBlock).write(FreeBlock{size,next})}}
 arena.heap.head=NonNull::new(start as*mut FreeBlock);arena.heap.initialized=true;unsafe{arena.heap.rebuild_address_fences()};
 let p=unsafe{arena.heap.alloc(AllocDomain::Host,Layout::from_size_align(4096,8).unwrap())};
 assert_eq!(p as usize,start+N*STRIDE+48);assert_eq!((arena.heap.alloc_calls,arena.heap.alloc_nodes,arena.heap.alloc_max_nodes,arena.heap.alloc_failures),(1,124,124,0));
 let impossible=Layout::from_size_align(arena.layout.size(),8).unwrap();assert!(unsafe{arena.heap.alloc(AllocDomain::Host,impossible)}.is_null());
 assert_eq!((arena.heap.alloc_calls,arena.heap.alloc_nodes,arena.heap.alloc_max_nodes,arena.heap.alloc_failures),(2,248,124,1));
 check_index(&arena.heap);
}

#[test]fn failed_and_invalid_first_fit_attempts_commit_once_and_clear_hint(){
 let mut arena=Arena::new(65536);arena.heap.initialized=true;arena.heap.head=None;arena.heap.free_finger=NonNull::new(7 as *mut FreeBlock);
 assert!(unsafe{arena.heap.alloc(AllocDomain::Host,Layout::from_size_align(64,8).unwrap())}.is_null());
 assert_eq!((arena.heap.alloc_calls,arena.heap.alloc_nodes,arena.heap.alloc_failures),(1,0,1));assert!(arena.heap.free_finger.is_none());
 arena.heap.head=NonNull::new(7 as *mut FreeBlock);arena.heap.free_finger=arena.heap.head;
 assert!(unsafe{arena.heap.alloc(AllocDomain::Host,Layout::from_size_align(64,8).unwrap())}.is_null());
 assert_eq!((arena.heap.alloc_calls,arena.heap.alloc_nodes,arena.heap.alloc_max_nodes,arena.heap.alloc_failures),(2,1,1,2));assert!(arena.heap.free_finger.is_none());
}

#[test]fn cumulative_work_saturates_and_derived_state_resets_on_install_or_rebuild(){
 let mut arena=Arena::new(65536);let domain=AllocDomain::Host;let layout=Layout::from_size_align(64,8).unwrap();
 arena.heap.alloc_calls=u64::MAX;arena.heap.alloc_nodes=u64::MAX;arena.heap.alloc_max_nodes=u64::MAX;arena.heap.alloc_failures=u64::MAX;
 assert!(!unsafe{arena.heap.alloc(domain,layout)}.is_null());assert_eq!((arena.heap.alloc_calls,arena.heap.alloc_nodes,arena.heap.alloc_max_nodes,arena.heap.alloc_failures),(u64::MAX,u64::MAX,u64::MAX,u64::MAX));
 assert!(unsafe{arena.heap.alloc(domain,Layout::from_size_align(131072,8).unwrap())}.is_null());assert_eq!(arena.heap.alloc_failures,u64::MAX);
 unsafe{arena.heap.rebuild_address_fences()};assert_eq!((arena.heap.alloc_calls,arena.heap.alloc_nodes,arena.heap.alloc_max_nodes,arena.heap.alloc_failures),(0,0,0,0));
 assert!(!unsafe{arena.heap.alloc(domain,layout)}.is_null());arena.heap.install_heap(arena.ptr.as_ptr()as usize,0,arena.layout.size());
 assert_eq!((arena.heap.alloc_calls,arena.heap.alloc_nodes,arena.heap.alloc_max_nodes,arena.heap.alloc_failures),(0,0,0,0));assert!(size_of::<FreeList>()+64<4096);
}

#[test]fn passive_snapshot_never_waits_initializes_or_walks_heap(){
 assert!(hv_guest_allocator_work_snapshot(1).is_none());assert!(hv_guest_allocator_work_snapshot(0).is_none());
 let mut guard=TEST_PAGE.allocator.0.lock().unwrap();assert!(hv_guest_allocator_work_snapshot(0).is_none());
 // An invalid list head would fault if this observer tried to traverse it.
 guard.initialized=true;guard.head=NonNull::new(7 as *mut FreeBlock);guard.alloc_calls=9;guard.alloc_nodes=1234;guard.alloc_max_nodes=700;guard.alloc_failures=2;drop(guard);
 let snapshot=hv_guest_allocator_work_snapshot(0).unwrap();assert_eq!((snapshot.alloc_calls,snapshot.alloc_nodes,snapshot.alloc_max_nodes,snapshot.alloc_failures),(9,1234,700,2));
 *TEST_PAGE.allocator.0.lock().unwrap()=FreeList::new();
}
'''


def main():
    source = SOURCE.read_text()
    code = harness() + "\n#[derive(Clone,Copy,Debug)]\n" + structure(source, "AllocatorWorkSnapshot")
    code += "\n" + function(SOURCE, "hv_guest_allocator_work_snapshot") + TESTS
    with tempfile.TemporaryDirectory(prefix="allocator-work-") as directory:
        path = Path(directory) / "test.rs"
        binary = Path(directory) / "test"
        path.write_text(code)
        subprocess.run(["rustc", "--edition=2024", "-O", "--test", str(path), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True, timeout=120)


if __name__ == "__main__":
    main()
