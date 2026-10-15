//! Fixed-capacity pointer registry for C allocation tracking.
//!
//! This is optional allocation metadata: insertion may fail while the caller
//! retains the allocation and its authoritative allocator-owned tag. Limit
//! each search to one page's worth of entries so saturation cannot turn every
//! ordinary malloc/free into a scan of the complete table. Deleted entries
//! remain tombstones and are reused without moving another live allocation.
//! The registry itself never allocates or evicts a live record.

const PROBE_BYTES: usize = 4096;

pub struct AllocationRegistry<V, const N: usize> {
    entries: [Slot<V>; N],
}

struct Entry<V> {
    key: usize,
    value: V,
}

enum Slot<V> {
    Empty,
    Deleted,
    Occupied(Entry<V>),
}

impl<V, const N: usize> AllocationRegistry<V, N> {
    pub const fn new() -> Self {
        Self {
            entries: [const { Slot::Empty }; N],
        }
    }

    #[inline]
    fn bucket(key: usize) -> usize {
        // MurmurHash3's 64-bit finalizer mixes low alignment zeros and high
        // address bits before the power-of-two allocation capacity is masked.
        let mut x = key as u64;
        x ^= x >> 33;
        x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
        x ^= x >> 33;
        x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        x ^= x >> 33;
        if N.is_power_of_two() {
            (x as usize) & (N - 1)
        } else {
            (x as usize) % N
        }
    }

    #[inline]
    fn next(index: usize) -> usize {
        if index + 1 == N { 0 } else { index + 1 }
    }

    #[inline]
    fn probe_limit() -> usize {
        N.min((PROBE_BYTES / core::mem::size_of::<Slot<V>>()).max(1))
    }

    pub fn insert(&mut self, key: usize, value: V) -> Result<(), V> {
        if N == 0 {
            return Err(value);
        }
        let mut index = Self::bucket(key);
        let mut deleted = None;
        for _ in 0..Self::probe_limit() {
            match &mut self.entries[index] {
                Slot::Occupied(entry) if entry.key == key => {
                    entry.value = value;
                    return Ok(());
                }
                Slot::Empty => {
                    self.entries[deleted.unwrap_or(index)] = Slot::Occupied(Entry { key, value });
                    return Ok(());
                }
                Slot::Deleted => { deleted.get_or_insert(index); }
                Slot::Occupied(_) => {}
            }
            index = Self::next(index);
        }
        // A tombstone may precede an existing key. Search the complete bounded
        // chain before reusing it, rather than publishing a duplicate mapping.
        if let Some(index) = deleted {
            self.entries[index] = Slot::Occupied(Entry { key, value });
            return Ok(());
        }
        Err(value)
    }

    pub fn get(&self, key: usize) -> Option<&V> {
        let index = self.find_index(key)?;
        match &self.entries[index] {
            Slot::Occupied(entry) => Some(&entry.value),
            _ => unreachable!("find_index returned an unoccupied slot"),
        }
    }

    pub fn remove(&mut self, key: usize) -> Option<V> {
        let index = self.find_index(key)?;
        match core::mem::replace(&mut self.entries[index], Slot::Deleted) {
            Slot::Occupied(entry) => Some(entry.value),
            _ => unreachable!("find_index returned an unoccupied slot"),
        }
    }

    fn find_index(&self, key: usize) -> Option<usize> {
        if N == 0 {
            return None;
        }
        let mut index = Self::bucket(key);
        for _ in 0..Self::probe_limit() {
            match &self.entries[index] {
                Slot::Occupied(entry) if entry.key == key => return Some(index),
                Slot::Empty => return None,
                Slot::Deleted | Slot::Occupied(_) => index = Self::next(index),
            }
        }
        None
    }
}

impl<V, const N: usize> Default for AllocationRegistry<V, N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn len<V, const N: usize>(table: &AllocationRegistry<V, N>) -> usize {
        table.entries.iter().filter(|entry| matches!(entry, Slot::Occupied(_))).count()
    }

    fn collisions<const N: usize>(bucket: usize, count: usize) -> Vec<usize> {
        (0..100_000)
            .map(|n| n * 16)
            .filter(|key| AllocationRegistry::<u32, N>::bucket(*key) == bucket)
            .take(count)
            .collect()
    }

    #[test]
    fn collisions_and_wrapped_cluster_survive_deletion() {
        let keys = collisions::<8>(7, 5);
        assert_eq!(keys.len(), 5);
        let mut table = AllocationRegistry::<u32, 8>::new();
        for (value, key) in keys.iter().enumerate() {
            table.insert(*key, value as u32).unwrap();
        }
        assert_eq!(len(&table), 5);
        assert_eq!(table.remove(keys[1]), Some(1));
        assert_eq!(table.remove(keys[3]), Some(3));
        for (value, key) in keys.iter().enumerate() {
            assert_eq!(table.get(*key).copied(), (!matches!(value, 1 | 3)).then_some(value as u32));
        }
        table.insert(keys[1], 99).unwrap();
        assert_eq!(table.get(keys[1]), Some(&99));
    }

    #[test]
    fn full_registry_still_updates_existing_key_and_recovers_after_remove() {
        let mut table = AllocationRegistry::<u32, 4>::new();
        for key in 0..4 {
            table.insert(key, key as u32).unwrap();
        }
        assert_eq!(table.insert(4, 40), Err(40));
        table.insert(2, 22).unwrap();
        assert_eq!(len(&table), 4);
        assert_eq!(table.remove(2), Some(22));
        table.insert(4, 40).unwrap();
        assert_eq!(table.get(4), Some(&40));
        assert_eq!(len(&table), 4);
    }

    #[test]
    fn zero_capacity_and_noncopy_values() {
        let mut empty = AllocationRegistry::<String, 0>::new();
        assert_eq!(empty.insert(0, "value".into()), Err("value".into()));
        assert_eq!(empty.get(0), None);
        assert_eq!(empty.remove(0), None);
        let mut table = AllocationRegistry::<String, 2>::new();
        table.insert(0, "old".into()).unwrap();
        table.insert(0, "new".into()).unwrap();
        assert_eq!(table.remove(0), Some("new".into()));
    }

    #[test]
    fn bounded_wrapped_collision_window_reuses_tombstones_without_duplicate_keys() {
        const N: usize = 4096;
        type Table = AllocationRegistry<(usize, usize), N>;
        let limit = Table::probe_limit();
        assert_eq!(limit, 128);
        assert!(limit * core::mem::size_of::<Slot<(usize, usize)>>() <= PROBE_BYTES);
        let keys: Vec<_> = (1usize..)
            .map(|key| key * 16)
            .filter(|&key| Table::bucket(key) == N - 3)
            .take(limit + 1).collect();
        let mut table = Table::new();
        for (index, &key) in keys[..limit].iter().enumerate() {
            table.insert(key, (index, 16)).unwrap();
        }
        assert_eq!(table.insert(keys[limit], (limit, 16)), Err((limit, 16)));
        assert_eq!(table.get(keys[limit]), None);
        assert_eq!(table.remove(keys[limit]), None);
        assert_eq!(len(&table), limit, "probe exhaustion must not evict a live record");
        assert_eq!(table.remove(keys[1]), Some((1, 16)));
        table.insert(keys[limit - 1], (777, 32)).unwrap();
        assert_eq!(len(&table), limit - 1, "updating beyond a tombstone cannot duplicate a key");
        assert_eq!(table.remove(keys[limit - 1]), Some((777, 32)));
        assert_eq!(table.get(keys[limit - 1]), None);
        table.insert(keys[limit], (limit, 16)).unwrap();
        for (index, &key) in keys[..limit - 1].iter().enumerate() {
            assert_eq!(table.get(key).copied(), (index != 1).then_some((index, 16)));
        }
        assert_eq!(table.get(keys[limit]), Some(&(limit, 16)));
    }

    #[test]
    fn repeated_full_delete_and_reinsert_cycles_do_not_exhaust_tombstones() {
        const N: usize = 16;
        let mut table = AllocationRegistry::<usize, N>::new();
        for wave in 0..1000 {
            for key in 0..N { table.insert(key + wave * N, key).unwrap(); }
            assert_eq!(len(&table), N);
            for key in 0..N { assert_eq!(table.remove(key + wave * N), Some(key)); }
            assert_eq!(len(&table), 0);
            assert_eq!(table.get(usize::MAX), None);
        }
    }

    #[test]
    fn replacement_removal_and_rejection_preserve_value_ownership() {
        use std::rc::Rc;
        use std::cell::Cell;
        struct Value(Rc<Cell<usize>>);
        impl Drop for Value { fn drop(&mut self) { self.0.set(self.0.get() + 1); } }
        let drops = Rc::new(Cell::new(0));
        let mut table = AllocationRegistry::<Value, 1>::new();
        assert!(table.insert(1, Value(drops.clone())).is_ok());
        assert!(table.insert(1, Value(drops.clone())).is_ok());
        assert_eq!(drops.get(), 1, "replacement drops only the previous value");
        let rejected = table.insert(2, Value(drops.clone())).err().unwrap();
        assert_eq!(drops.get(), 1, "rejected value remains owned by caller");
        let removed = table.remove(1).unwrap();
        assert_eq!(drops.get(), 1, "removed value remains owned by caller");
        drop(removed); drop(rejected);
        assert_eq!(drops.get(), 3);
        assert!(table.insert(3, Value(drops.clone())).is_ok());
        drop(table);
        assert_eq!(drops.get(), 4);
    }

    #[test]
    fn larger_values_still_admit_one_entry_per_probe_window() {
        type Table = AllocationRegistry<[u8; PROBE_BYTES + 1], 2>;
        assert_eq!(Table::probe_limit(), 1);
        let key = 7;
        let collision = (key + 1..).find(|&other| Table::bucket(other) == Table::bucket(key)).unwrap();
        let mut table = Table::new();
        assert!(table.insert(key, [3; PROBE_BYTES + 1]).is_ok());
        assert!(table.insert(collision, [4; PROBE_BYTES + 1]).is_err());
        assert_eq!(table.get(key).unwrap()[0], 3);
        assert!(table.remove(key).is_some());
        assert!(table.insert(collision, [4; PROBE_BYTES + 1]).is_ok());
    }

    #[test]
    fn randomized_differential_with_repeated_churn() {
        let mut table = AllocationRegistry::<u64, 32>::new();
        let mut reference = HashMap::<usize, u64>::new();
        let mut random = 0x35e9_11a8_6721_c05du64;
        for _ in 0..200_000 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let key = ((random >> 20) as usize % 48) * 16;
            match random & 3 {
                0 | 1 => {
                    let value = random;
                    let expected = if reference.contains_key(&key) || reference.len() < 32 {
                        reference.insert(key, value);
                        Ok(())
                    } else {
                        Err(value)
                    };
                    assert_eq!(table.insert(key, value), expected);
                }
                2 => assert_eq!(table.remove(key), reference.remove(&key)),
                _ => assert_eq!(table.get(key).copied(), reference.get(&key).copied()),
            }
            assert_eq!(len(&table), reference.len());
            for (&key, &value) in &reference {
                assert_eq!(table.get(key), Some(&value));
            }
        }
    }

    #[test]
    fn bounded_search_randomized_differential_preserves_every_accepted_record() {
        const N: usize = 257;
        let mut table = AllocationRegistry::<u64, N>::new();
        assert!(AllocationRegistry::<u64, N>::probe_limit() < N);
        let mut reference = HashMap::<usize, u64>::new();
        let mut random = 0xa37e_8129_ebfc_504du64;
        let mut peak = 0;
        for iteration in 0..50_000 {
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let key = ((random >> 20) as usize % (N * 2)) * 16;
            match random & 3 {
                0 | 1 => match table.insert(key, random) {
                    Ok(()) => { reference.insert(key, random); }
                    Err(value) => {
                        assert_eq!(value, random);
                        assert!(!reference.contains_key(&key), "an existing record must remain updatable");
                    }
                },
                2 => assert_eq!(table.remove(key), reference.remove(&key)),
                _ => assert_eq!(table.get(key).copied(), reference.get(&key).copied()),
            }
            assert_eq!(len(&table), reference.len());
            peak = peak.max(reference.len());
            if iteration % 127 == 0 {
                for (&key, &value) in &reference { assert_eq!(table.get(key), Some(&value)); }
            }
        }
        assert!(peak > N * 3 / 4);
        for (key, value) in reference { assert_eq!(table.remove(key), Some(value)); }
        assert_eq!(len(&table), 0);
    }

    #[test]
    #[ignore = "run explicitly for a host lookup timing comparison"]
    fn lookup_benchmark() {
        use std::hint::black_box;
        use std::time::Instant;
        const N: usize = 4096;
        let mut table = AllocationRegistry::<usize, N>::new();
        let mut linear = [None; N];
        let mut accepted = 0;
        for i in 0..N {
            let key = (i + 1) * 4096;
            if table.insert(key, i).is_ok() {
                linear[i] = Some((key, i));
                accepted += 1;
            }
        }
        let keys: Vec<_> = (0..200_000).map(|i| ((i * 997) % N + 1) * 4096).collect();
        let started = Instant::now();
        for &key in &keys {
            black_box(table.get(black_box(key)));
        }
        let hashed = started.elapsed();
        let started = Instant::now();
        for &key in &keys {
            black_box(
                linear
                    .iter()
                    .find_map(|entry| entry.filter(|(candidate, _)| *candidate == key)),
            );
        }
        let scanned = started.elapsed();
        eprintln!("lookup 200k at {accepted} tracked entries: hash={hashed:?} linear={scanned:?}");
    }
}
