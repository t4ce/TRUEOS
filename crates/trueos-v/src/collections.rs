pub use alloc::collections::{BTreeMap, BTreeSet};

pub type HashMap<K, V> = BTreeMap<K, V>;
pub type HashSet<T> = BTreeSet<T>;
