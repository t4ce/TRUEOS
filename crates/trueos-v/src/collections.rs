pub use alloc::collections::{BTreeMap, BTreeSet};
pub use hashbrown::{HashMap, HashSet, hash_map};

#[cfg(test)]
mod tests {
    use super::{HashMap, HashSet};

    #[derive(Hash, Eq, PartialEq)]
    struct HashOnly(u8);

    #[test]
    fn hash_collections_do_not_require_ord() {
        let mut map = HashMap::new();
        map.insert(HashOnly(1), "map");
        assert_eq!(map.get(&HashOnly(1)), Some(&"map"));

        let mut set = HashSet::new();
        set.insert(HashOnly(2));
        assert!(set.contains(&HashOnly(2)));
    }
}
