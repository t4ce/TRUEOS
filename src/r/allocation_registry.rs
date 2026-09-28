//! Fixed-capacity pointer registry for C allocation tracking.
//!
//! Linear probing keeps lookup and insertion local to a pointer's hash
//! bucket. Removal shifts the following cluster backward, so repeated
//! free/alloc cycles never accumulate tombstones. The registry itself never
//! allocates; the caller supplies its entire capacity at compile time.

pub struct AllocationRegistry<V, const N: usize> {
    entries: [Option<Entry<V>>; N],
}

struct Entry<V> {
    key: usize,
    value: V,
}

impl<V, const N: usize> AllocationRegistry<V, N> {
    pub const fn new() -> Self {
        Self {
            entries: [const { None }; N],
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
    fn distance(from: usize, to: usize) -> usize {
        if to >= from { to - from } else { N - from + to }
    }

    pub fn insert(&mut self, key: usize, value: V) -> Result<(), V> {
        if N == 0 {
            return Err(value);
        }
        let mut index = Self::bucket(key);
        for _ in 0..N {
            match self.entries[index].as_mut() {
                Some(entry) if entry.key == key => {
                    entry.value = value;
                    return Ok(());
                }
                None => {
                    self.entries[index] = Some(Entry { key, value });
                    return Ok(());
                }
                Some(_) => index = Self::next(index),
            }
        }
        Err(value)
    }

    pub fn get(&self, key: usize) -> Option<&V> {
        let index = self.find_index(key)?;
        self.entries[index].as_ref().map(|entry| &entry.value)
    }

    pub fn remove(&mut self, key: usize) -> Option<V> {
        let index = self.find_index(key)?;
        let value = self.entries[index].take()?.value;
        let mut hole = index;
        let mut scan = Self::next(hole);
        while let Some(entry) = self.entries[scan].as_ref() {
            let home = Self::bucket(entry.key);
            if Self::distance(home, hole) < Self::distance(home, scan) {
                self.entries[hole] = self.entries[scan].take();
                hole = scan;
            }
            scan = Self::next(scan);
        }
        Some(value)
    }

    fn find_index(&self, key: usize) -> Option<usize> {
        if N == 0 {
            return None;
        }
        let mut index = Self::bucket(key);
        for _ in 0..N {
            match self.entries[index].as_ref() {
                Some(entry) if entry.key == key => return Some(index),
                None => return None,
                Some(_) => index = Self::next(index),
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
        table.entries.iter().filter(|entry| entry.is_some()).count()
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
    #[ignore = "run explicitly for a host lookup timing comparison"]
    fn lookup_benchmark() {
        use std::hint::black_box;
        use std::time::Instant;
        const N: usize = 4096;
        let mut table = AllocationRegistry::<usize, N>::new();
        let mut linear = [None; N];
        for i in 0..N {
            let key = (i + 1) * 4096;
            table.insert(key, i).unwrap();
            linear[i] = Some((key, i));
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
        eprintln!("lookup 200k at 4096 entries: hash={hashed:?} linear={scanned:?}");
    }
}
