/// Fixed-capacity key/value table for guest-reachable kernel state.
///
/// Use this for small registries that must not allocate internal nodes or retain
/// host-heap pointers across Hull guest execution.
pub struct FixedKeyMap<K, V, const N: usize> {
    entries: [Option<FixedKeyMapEntry<K, V>>; N],
}

struct FixedKeyMapEntry<K, V> {
    key: K,
    value: V,
}

impl<K, V, const N: usize> FixedKeyMap<K, V, N> {
    pub const fn new() -> Self {
        Self {
            entries: [const { None }; N],
        }
    }
}

impl<K: Copy + Eq, V, const N: usize> FixedKeyMap<K, V, N> {
    pub fn insert(&mut self, key: K, value: V) -> Result<(), V> {
        if let Some(index) = self.find_index(key) {
            self.entries[index] = Some(FixedKeyMapEntry { key, value });
            return Ok(());
        }

        let Some(index) = self.first_empty_index() else {
            return Err(value);
        };
        self.entries[index] = Some(FixedKeyMapEntry { key, value });
        Ok(())
    }

    pub fn get(&self, key: K) -> Option<&V> {
        let index = self.find_index(key)?;
        self.entries[index].as_ref().map(|entry| &entry.value)
    }

    pub fn get_mut(&mut self, key: K) -> Option<&mut V> {
        let index = self.find_index(key)?;
        self.entries[index].as_mut().map(|entry| &mut entry.value)
    }

    pub fn get_or_insert_with(&mut self, key: K, make_value: impl FnOnce() -> V) -> Option<&mut V> {
        if let Some(index) = self.find_index(key) {
            return self.entries[index].as_mut().map(|entry| &mut entry.value);
        }

        let index = self.first_empty_index()?;
        self.entries[index] = Some(FixedKeyMapEntry {
            key,
            value: make_value(),
        });
        self.entries[index].as_mut().map(|entry| &mut entry.value)
    }

    pub fn remove(&mut self, key: K) -> Option<V> {
        let index = self.find_index(key)?;
        self.entries[index].take().map(|entry| entry.value)
    }

    fn find_index(&self, key: K) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.as_ref().is_some_and(|entry| entry.key == key))
    }

    fn first_empty_index(&self) -> Option<usize> {
        self.entries.iter().position(Option::is_none)
    }
}

impl<K, V, const N: usize> Default for FixedKeyMap<K, V, N> {
    fn default() -> Self {
        Self::new()
    }
}
