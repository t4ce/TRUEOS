extern crate std;

use super::*;
use std::vec::Vec;

struct Heap {
    storage: NonNull<u8>,
    layout: Layout,
    list: FreeList,
}

impl Heap {
    fn new(bytes: usize) -> Self {
        let layout = Layout::from_size_align(bytes, 4096).unwrap();
        let storage = NonNull::new(unsafe { std::alloc::alloc(layout) }).unwrap();
        let mut list = FreeList::new();
        list.install_heap(storage.as_ptr() as usize, 0, bytes);
        unsafe { list.init_once() };
        Self {
            storage,
            layout,
            list,
        }
    }

    /// Independent address-ordered first-fit oracle, without search hints.
    fn first_fit(&self, layout: Layout) -> (*mut u8, u64) {
        let mut current = self.list.head;
        let mut visits = 0;
        while let Some(node) = current {
            visits += 1;
            let block = unsafe { node.as_ref() };
            let start = node.as_ptr() as usize;
            if let Some(payload) = aligned_payload(start, layout)
                && let Some(used) = payload
                    .checked_add(layout.size())
                    .and_then(|end| end.checked_sub(start))
                && align_up(used, align_of::<FreeBlock>()) <= block.size
            {
                return (payload as *mut u8, visits);
            }
            current = block.next;
        }
        (null_mut(), visits)
    }

    fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        unsafe { self.list.init_once() };
        let layout = Layout::from_size_align(size, align).unwrap();
        let expected = self.first_fit(layout).0;
        let actual = unsafe { self.list.alloc(AllocDomain::Host, layout) };
        assert_eq!(actual, expected, "first-fit changed for {layout:?}");
        actual
    }

    fn free(&mut self, ptr: *mut u8) {
        unsafe { self.list.dealloc(AllocDomain::Host, ptr) };
    }

    fn validate(&mut self) {
        let mut nodes = Vec::new();
        let mut current = self.list.head;
        while let Some(node) = current {
            assert!(unsafe { self.list.checked_free_block_end(node) }.is_some());
            nodes.push(node);
            current = unsafe { node.as_ref().next };
        }
        for hint in &self.list.alloc_search {
            if let Some(before) = hint.before {
                let index = nodes
                    .iter()
                    .position(|node| *node == before)
                    .expect("search hint points outside the free list");
                for node in &nodes[..=index] {
                    assert!(
                        unsafe { node.as_ref().size } < hint.minimum,
                        "search hint skips a possible first fit"
                    );
                }
            }
        }
        for band in 0..FREE_ADDRESS_FENCES {
            let expected = nodes
                .iter()
                .copied()
                .take_while(|node| self.list.address_band(node.as_ptr() as usize) < band)
                .last();
            assert_eq!(self.list.address_fences[band], expected);
        }
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        unsafe { std::alloc::dealloc(self.storage.as_ptr(), self.layout) };
    }
}

#[test]
fn fragmented_prefix_is_not_rescanned_for_each_allocation() {
    const HOLES: usize = 10_000;
    const CALLS: usize = 128;
    let mut heap = Heap::new(64 * 1024 * 1024);
    let mut holes = Vec::new();
    for _ in 0..HOLES {
        holes.push(heap.alloc(32, 8));
        assert!(!heap.alloc(32, 8).is_null()); // Retained separators.
    }
    for ptr in holes {
        heap.free(ptr);
    }
    heap.list.reset_work_counts();
    let mut reference_visits = 0;
    for _ in 0..CALLS {
        reference_visits += heap.first_fit(Layout::from_size_align(1024, 8).unwrap()).1;
        let ptr = heap.alloc(1024, 8);
        assert!(!ptr.is_null());
        heap.free(ptr);
    }
    heap.validate();
    assert_eq!(heap.list.alloc_calls, CALLS as u64);
    assert_eq!(reference_visits, (HOLES + 1) as u64 * CALLS as u64);
    assert!(heap.list.alloc_nodes <= (HOLES + CALLS) as u64);
    assert!(heap.list.alloc_nodes * 100 < reference_visits);
    std::println!(
        "fragmented-prefix allocations={} reference_nodes={} actual_nodes={} nodes_per_alloc={:.2}",
        CALLS,
        reference_visits,
        heap.list.alloc_nodes,
        heap.list.alloc_nodes as f64 / CALLS as f64
    );
}

#[test]
fn freeing_an_earlier_fit_rewinds_the_prefix() {
    let mut heap = Heap::new(1024 * 1024);
    let small = heap.alloc(32, 8);
    let _first_guard = heap.alloc(32, 8);
    let earlier = heap.alloc(2048, 8);
    let _guard = heap.alloc(32, 8);
    heap.free(small);
    let later = heap.alloc(4096, 8);
    heap.free(later);
    heap.free(earlier);
    assert_eq!(heap.alloc(4096, 8), later);
    assert_eq!(heap.alloc(1024, 8), earlier);
    heap.validate();
}

#[test]
fn coalescing_in_a_rejected_prefix_restores_the_earliest_fit() {
    for reverse in [false, true] {
        let mut heap = Heap::new(1024 * 1024);
        let a = heap.alloc(256, 8);
        let b = heap.alloc(256, 8);
        let _guard = heap.alloc(32, 8);
        heap.free(if reverse { b } else { a });
        let later = heap.alloc(512, 8);
        heap.free(later);
        heap.free(if reverse { a } else { b });
        heap.validate();
        assert_eq!(heap.alloc(512, 8), a);
        heap.validate();
    }
}

#[test]
fn allocating_a_prefix_anchor_cannot_leave_a_stale_hint() {
    let mut heap = Heap::new(1024 * 1024);
    let first = heap.alloc(256, 8);
    let _guard = heap.alloc(32, 8);
    heap.free(first);
    assert!(!heap.alloc(512, 8).is_null());
    let replacement = heap.alloc(64, 8);
    assert_eq!(replacement, first);
    unsafe { core::ptr::write_bytes(replacement, 0xA5, 64) };
    heap.validate();
    assert!(!heap.alloc(512, 8).is_null());
    heap.validate();
}

#[test]
fn alignment_rejection_is_not_cached_as_a_size_rejection() {
    let mut heap = Heap::new(1024 * 1024);
    let first = heap.alloc(16, 8);
    let _guard = heap.alloc(32, 8);
    heap.free(first);
    let aligned = heap.alloc(1, 4096);
    assert!(!aligned.is_null());
    assert_ne!(aligned, first);
    assert_eq!(heap.alloc(1, 8), first);
    heap.validate();
}

#[test]
fn failed_search_hint_recovers_after_free_and_hash_collision() {
    let mut heap = Heap::new(16 * 1024);
    let mut allocations = Vec::new();
    loop {
        let ptr = heap.alloc(64, 8);
        if ptr.is_null() {
            break;
        }
        allocations.push(ptr);
    }
    assert!(heap.alloc(512, 8).is_null());
    assert!(heap.alloc(512, 8).is_null());
    let slot = FreeList::alloc_search_index(560);
    let collision = (1024..8192)
        .step_by(8)
        .find(|size| FreeList::alloc_search_index(size + 48) == slot)
        .unwrap();
    assert!(heap.alloc(collision, 8).is_null());
    for ptr in allocations.iter().take(6) {
        heap.free(*ptr);
    }
    assert_eq!(heap.alloc(512, 8), allocations[0]);
    heap.validate();
}

#[test]
fn rebuild_and_heap_replacement_drop_derived_search_hints() {
    let mut heap = Heap::new(1024 * 1024);
    let first = heap.alloc(16, 8);
    let _guard = heap.alloc(32, 8);
    heap.free(first);
    assert!(!heap.alloc(512, 8).is_null());
    assert!(
        heap.list
            .alloc_search
            .iter()
            .any(|hint| hint.before.is_some())
    );
    unsafe { heap.list.rebuild_address_fences() };
    assert!(
        heap.list
            .alloc_search
            .iter()
            .all(|hint| hint.before.is_none())
    );
    heap.validate();
    assert!(!heap.alloc(512, 8).is_null());
    let start = heap.storage.as_ptr() as usize + 512 * 1024;
    heap.list.install_heap(start, 0, 512 * 1024);
    assert!(
        heap.list
            .alloc_search
            .iter()
            .all(|hint| hint.before.is_none())
    );
    assert_eq!(heap.alloc(512, 8) as usize, start + minimum_block_size());
    heap.validate();
}

#[test]
fn randomized_churn_matches_first_fit_and_preserves_payloads() {
    let mut heap = Heap::new(4 * 1024 * 1024);
    let mut live: Vec<(*mut u8, usize, u8)> = Vec::new();
    let mut random = 0x4F93_175B_27C8_AE01u64;
    for step in 0..25_000 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        if !live.is_empty() && (random % 3 == 0 || live.len() >= 512) {
            let index = (random >> 12) as usize % live.len();
            let (ptr, size, value) = live.swap_remove(index);
            assert!(
                unsafe { core::slice::from_raw_parts(ptr, size) }
                    .iter()
                    .all(|byte| *byte == value)
            );
            heap.free(ptr);
        } else {
            let size = ((random >> 24) as usize % 97) * 16 + 1;
            let align = 1usize << ((random >> 40) as usize % 13);
            let ptr = heap.alloc(size, align);
            if !ptr.is_null() {
                let value = (random >> 56) as u8;
                unsafe { core::ptr::write_bytes(ptr, value, size) };
                live.push((ptr, size, value));
            }
        }
        if step % 127 == 0 {
            heap.validate();
        }
    }
    for (ptr, size, value) in live {
        assert!(
            unsafe { core::slice::from_raw_parts(ptr, size) }
                .iter()
                .all(|byte| *byte == value)
        );
        heap.free(ptr);
    }
    heap.validate();
    let head = unsafe { heap.list.head.unwrap().as_ref() };
    assert_eq!(head.size, heap.layout.size());
    assert!(head.next.is_none());
}
