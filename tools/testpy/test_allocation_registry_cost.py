#!/usr/bin/env python3
"""Count production C-allocation metadata visits at saturation.

Only a scalar probe counter is injected into the production search loops.
The source's ordinary tests also run. A 65,536-live-record fixture uses one
key per home bucket, so a full table is attainable without relying on random
hash placement. Timings include counter instrumentation and are host evidence,
not physical TRUEOS performance measurements.
"""

from pathlib import Path
import hashlib
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "src/r/allocation_registry.rs"

COUNTER = r"""
static REGISTRY_PROBES: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
mod saturation_cost_tests {
    use super::*;
    use std::{hint::black_box, sync::atomic::Ordering, time::Instant};

    fn heap_registry<V, const N: usize>() -> Box<AllocationRegistry<V, N>> {
        // Avoid constructing this 2 MiB table on a host test thread's stack.
        let mut storage = Box::<AllocationRegistry<V, N>>::new_uninit();
        let entries = unsafe {
            core::ptr::addr_of_mut!((*storage.as_mut_ptr()).entries).cast::<Slot<V>>()
        };
        for index in 0..N { unsafe { entries.add(index).write(Slot::Empty) }; }
        unsafe { storage.assume_init() }
    }

    #[test]
    fn full_production_sized_registry_bounds_untracked_operations_and_recycles() {
        const N: usize = 65536;
        type Table = AllocationRegistry<(usize, usize), N>;
        let mut table = heap_registry::<(usize, usize), N>();
        let mut keys = vec![None; N];
        let mut filled = 0;
        let mut sequence = 1usize;
        while filled < N {
            let key = sequence * 16;
            sequence += 1;
            let bucket = Table::bucket(key);
            if keys[bucket].is_none() { keys[bucket] = Some(key); filled += 1; }
        }
        for (index, key) in keys.iter().enumerate() {
            table.insert(key.unwrap(), (index, 16)).unwrap();
        }
        let missing = sequence * 16;
        let iterations = 100;
        REGISTRY_PROBES.store(0, Ordering::Relaxed);
        let started = Instant::now();
        for _ in 0..iterations {
            assert_eq!(table.insert(black_box(missing), (7, 16)), Err((7, 16)));
            assert_eq!(table.get(black_box(missing)), None);
            assert_eq!(table.remove(black_box(missing)), None);
        }
        let elapsed = started.elapsed().as_nanos();
        let visits = REGISTRY_PROBES.load(Ordering::Relaxed);
        let expected = iterations * 3 * Table::probe_limit();
        assert_eq!(Table::probe_limit(), 128);
        assert_eq!(visits, expected);
        assert_eq!(core::mem::size_of::<Slot<(usize, usize)>>(), 32);
        assert_eq!(core::mem::size_of::<AllocationRegistry<(usize, usize), N>>(), 2 * 1024 * 1024);
        println!("registry_cost capacity={N} live={filled} iterations={iterations} insert_get_remove_slot_visits={visits} instrumented_host_ns={elapsed} physical_timing=0");

        // Deleting a record in a full table is one lookup, without walking
        // the whole occupied cluster to shift other allocation records.
        let removed_key = keys[N / 2].unwrap();
        REGISTRY_PROBES.store(0, Ordering::Relaxed);
        assert_eq!(table.remove(removed_key), Some((N / 2, 16)));
        assert_eq!(REGISTRY_PROBES.load(Ordering::Relaxed), 1);
        assert!(table.insert(removed_key, (123, 32)).is_ok());
        assert_eq!(table.get(removed_key), Some(&(123, 32)));
        for (index, key) in keys.iter().enumerate() {
            let expected = if index == N / 2 { (123, 32) } else { (index, 16) };
            assert_eq!(table.get(key.unwrap()), Some(&expected));
        }

        // A table consisting entirely of tombstones remains reusable.
        for key in &keys { assert!(table.remove(key.unwrap()).is_some()); }
        for (index, key) in keys.iter().enumerate() {
            table.insert(key.unwrap(), (index + 1, 64)).unwrap();
        }
        for (index, key) in keys.iter().enumerate() {
            assert_eq!(table.get(key.unwrap()), Some(&(index + 1, 64)));
        }
    }
}
"""


def main():
    source = SOURCE.read_text()
    loop = "for _ in 0..Self::probe_limit() {"
    assert source.count(loop) == 2, "production search loops changed"
    instrumented = source.replace(
        loop,
        loop + " REGISTRY_PROBES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);",
    ) + COUNTER
    print("production_source_sha256=" + hashlib.sha256(source.encode()).hexdigest(), flush=True)
    with tempfile.TemporaryDirectory(prefix="allocation-registry-cost-") as directory:
        folder = Path(directory)
        rust_source = folder / "registry_cost.rs"
        executable = folder / "registry_cost"
        rust_source.write_text(instrumented)
        subprocess.run([
            "rustc", "--edition=2024", "--test", "-C", "opt-level=1",
            str(rust_source), "-o", str(executable),
        ], check=True)
        subprocess.run([str(executable), "--test-threads=1", "--nocapture"], check=True)


if __name__ == "__main__":
    main()
