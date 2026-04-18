use core::cmp::Ordering;
use core::fmt::{self, Write};

use crate::bst_arena::{BstArena, NodeId};

/// An unbalanced binary search tree (the plain precursor to `AvlTree`).
///
/// Same arena-backed storage and API surface as `AvlTree`, but without
/// height tracking or rotations.  Insert/remove are O(n) worst-case
/// (degenerate linked list), O(log n) on average with random keys.
///
/// Use this when you want a simple ordered map without the constant-factor
/// overhead of AVL rebalancing, or as a baseline to compare against.
pub struct BstMap<K, V> {
    arena: BstArena<K, V, ()>,
}

impl<K, V> BstMap<K, V> {
    pub const fn new() -> Self {
        Self {
            arena: BstArena::new(),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.arena.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.arena.is_empty()
    }

    pub fn clear(&mut self) {
        self.arena.clear();
    }

    pub fn min(&self) -> Option<(&K, &V)> {
        self.arena.min()
    }

    pub fn max(&self) -> Option<(&K, &V)> {
        self.arena.max()
    }

    pub fn iter(&self) -> crate::bst_arena::Iter<'_, K, V, ()> {
        crate::bst_arena::Iter::new(&self.arena)
    }

    pub fn write_ascii_tree<W, F>(
        &self,
        out: &mut W,
        max_nodes: usize,
        mut render_key: F,
    ) -> fmt::Result
    where
        W: Write,
        F: FnMut(&K, &V, &mut W) -> fmt::Result,
    {
        self.arena.write_ascii_tree(out, max_nodes, |_id, n, w| {
            let label = if n.left.is_some() || n.right.is_some() {
                "N"
            } else {
                "L"
            };
            w.write_str(label)?;
            w.write_char(' ')?;
            render_key(&n.key, &n.value, w)
        })
    }
}

// ── Key-ordered operations ──

impl<K: Ord, V> BstMap<K, V> {
    pub fn get(&self, key: &K) -> Option<&V> {
        self.arena.get(key)
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.arena.get_mut(key)
    }

    pub fn contains(&self, key: &K) -> bool {
        self.arena.contains(key)
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let (new_root, old) = self.insert_at(self.arena.root, key, value);
        self.arena.root = Some(new_root);
        if old.is_none() {
            self.arena.len += 1;
        }
        old
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        let (new_root, val) = self.remove_at(self.arena.root, key)?;
        self.arena.root = new_root;
        self.arena.len -= 1;
        Some(val)
    }

    // ── recursive helpers ──

    fn insert_at(&mut self, node: Option<NodeId>, key: K, value: V) -> (NodeId, Option<V>) {
        let id = match node {
            None => return (self.arena.alloc(key, value, ()), None),
            Some(id) => id,
        };

        match key.cmp(&self.arena.node(id).key) {
            Ordering::Equal => {
                let old = core::mem::replace(&mut self.arena.node_mut(id).value, value);
                (id, Some(old))
            }
            Ordering::Less => {
                let left = self.arena.node(id).left;
                let (new_left, old) = self.insert_at(left, key, value);
                self.arena.node_mut(id).left = Some(new_left);
                (id, old)
            }
            Ordering::Greater => {
                let right = self.arena.node(id).right;
                let (new_right, old) = self.insert_at(right, key, value);
                self.arena.node_mut(id).right = Some(new_right);
                (id, old)
            }
        }
    }

    fn remove_min(&mut self, id: NodeId) -> (Option<NodeId>, K, V) {
        let left = self.arena.node(id).left;
        if left.is_none() {
            let right = self.arena.node(id).right;
            let n = self.arena.take(id);
            (right, n.key, n.value)
        } else {
            let (new_left, key, val) = self.remove_min(left.unwrap());
            self.arena.node_mut(id).left = new_left;
            (Some(id), key, val)
        }
    }

    fn remove_at(&mut self, node: Option<NodeId>, key: &K) -> Option<(Option<NodeId>, V)> {
        let id = node?;

        match key.cmp(&self.arena.node(id).key) {
            Ordering::Less => {
                let left = self.arena.node(id).left;
                let (new_left, val) = self.remove_at(left, key)?;
                self.arena.node_mut(id).left = new_left;
                Some((Some(id), val))
            }
            Ordering::Greater => {
                let right = self.arena.node(id).right;
                let (new_right, val) = self.remove_at(right, key)?;
                self.arena.node_mut(id).right = new_right;
                Some((Some(id), val))
            }
            Ordering::Equal => {
                let left = self.arena.node(id).left;
                let right = self.arena.node(id).right;

                match (left, right) {
                    (None, None) => {
                        let n = self.arena.take(id);
                        Some((None, n.value))
                    }
                    (Some(child), None) | (None, Some(child)) => {
                        let n = self.arena.take(id);
                        Some((Some(child), n.value))
                    }
                    (Some(_), Some(right_child)) => {
                        let (new_right, succ_key, succ_val) = self.remove_min(right_child);
                        let old_val =
                            core::mem::replace(&mut self.arena.node_mut(id).value, succ_val);
                        let _ = core::mem::replace(&mut self.arena.node_mut(id).key, succ_key);
                        self.arena.node_mut(id).right = new_right;
                        Some((Some(id), old_val))
                    }
                }
            }
        }
    }
}

impl<K, V> Default for BstMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::BstMap;
    use alloc::string::String;
    use alloc::vec::Vec;

    /// Verify BST ordering recursively.
    fn verify_bst<K: Ord, V>(tree: &BstMap<K, V>) -> bool {
        verify_subtree(tree, tree.arena.root, None, None)
    }

    fn verify_subtree<K: Ord, V>(
        tree: &BstMap<K, V>,
        node: Option<usize>,
        min: Option<&K>,
        max: Option<&K>,
    ) -> bool {
        let id = match node {
            None => return true,
            Some(id) => id,
        };
        let n = tree.arena.node(id);

        if let Some(lo) = min {
            if n.key <= *lo {
                return false;
            }
        }
        if let Some(hi) = max {
            if n.key >= *hi {
                return false;
            }
        }

        verify_subtree(tree, n.left, min, Some(&n.key))
            && verify_subtree(tree, n.right, Some(&n.key), max)
    }

    #[test]
    fn empty() {
        let t: BstMap<u64, u64> = BstMap::new();
        assert!(t.is_empty());
        assert_eq!(t.len(), 0);
        assert_eq!(t.get(&0), None);
        assert_eq!(t.min(), None);
        assert_eq!(t.max(), None);
        assert_eq!(t.iter().count(), 0);
        assert!(verify_bst(&t));
    }

    #[test]
    fn insert_get() {
        let mut t = BstMap::new();
        assert_eq!(t.insert(10u64, "a"), None);
        assert_eq!(t.insert(5, "b"), None);
        assert_eq!(t.insert(15, "c"), None);
        assert_eq!(t.len(), 3);
        assert_eq!(t.get(&10), Some(&"a"));
        assert_eq!(t.get(&5), Some(&"b"));
        assert_eq!(t.get(&15), Some(&"c"));
        assert!(t.contains(&10));
        assert!(!t.contains(&99));
        assert!(verify_bst(&t));
    }

    #[test]
    fn insert_replace() {
        let mut t = BstMap::new();
        assert_eq!(t.insert(1u64, 10u32), None);
        assert_eq!(t.insert(1, 20), Some(10));
        assert_eq!(t.get(&1), Some(&20));
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn iter_in_order() {
        let mut t = BstMap::new();
        for &k in &[5u64, 3, 7, 1, 4, 6, 8, 2] {
            t.insert(k, k * 100);
        }
        let collected: Vec<(u64, u64)> = t.iter().map(|(&k, &v)| (k, v)).collect();
        assert_eq!(
            collected,
            alloc::vec![
                (1, 100),
                (2, 200),
                (3, 300),
                (4, 400),
                (5, 500),
                (6, 600),
                (7, 700),
                (8, 800)
            ]
        );
    }

    #[test]
    fn min_max() {
        let mut t = BstMap::new();
        for k in [20u64, 10, 30, 5, 15, 25, 35] {
            t.insert(k, k);
        }
        assert_eq!(t.min(), Some((&5, &5)));
        assert_eq!(t.max(), Some((&35, &35)));
    }

    #[test]
    fn remove_leaf() {
        let mut t = BstMap::new();
        t.insert(10u64, "a");
        t.insert(5, "b");
        t.insert(15, "c");
        assert_eq!(t.remove(&5), Some("b"));
        assert_eq!(t.len(), 2);
        assert!(!t.contains(&5));
        assert!(verify_bst(&t));
    }

    #[test]
    fn remove_one_child() {
        let mut t = BstMap::new();
        t.insert(10u64, 10);
        t.insert(5, 5);
        t.insert(15, 15);
        t.insert(3, 3);
        assert_eq!(t.remove(&5), Some(5));
        assert_eq!(t.len(), 3);
        assert!(t.contains(&3));
        assert!(verify_bst(&t));
    }

    #[test]
    fn remove_two_children() {
        let mut t = BstMap::new();
        for k in [10u64, 5, 15, 3, 7, 12, 20] {
            t.insert(k, k);
        }
        assert_eq!(t.remove(&10), Some(10));
        assert_eq!(t.len(), 6);
        assert!(!t.contains(&10));
        for &k in &[5, 15, 3, 7, 12, 20] {
            assert!(t.contains(&k));
        }
        assert!(verify_bst(&t));
    }

    #[test]
    fn remove_root_single() {
        let mut t = BstMap::new();
        t.insert(1u64, "only");
        assert_eq!(t.remove(&1), Some("only"));
        assert!(t.is_empty());
    }

    #[test]
    fn remove_nonexistent() {
        let mut t = BstMap::new();
        t.insert(1u64, 1);
        assert_eq!(t.remove(&999), None);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn insert_remove_round_trip() {
        let mut t = BstMap::new();
        for i in 0..64u64 {
            t.insert(i, i);
        }
        assert_eq!(t.len(), 64);
        assert!(verify_bst(&t));

        for i in (0..64u64).step_by(2) {
            assert_eq!(t.remove(&i), Some(i));
        }
        assert_eq!(t.len(), 32);
        assert!(verify_bst(&t));

        let keys: Vec<u64> = t.iter().map(|(&k, _)| k).collect();
        let expected: Vec<u64> = (0..64).filter(|i| i % 2 != 0).collect();
        assert_eq!(keys, expected);
    }

    #[test]
    fn get_mut_updates() {
        let mut t = BstMap::new();
        t.insert(5u64, 100u64);
        if let Some(v) = t.get_mut(&5) {
            *v = 200;
        }
        assert_eq!(t.get(&5), Some(&200));
    }

    #[test]
    fn clear_resets() {
        let mut t = BstMap::new();
        for i in 0..20u64 {
            t.insert(i, i);
        }
        t.clear();
        assert!(t.is_empty());
        assert_eq!(t.iter().count(), 0);
    }

    #[test]
    fn slot_reuse() {
        let mut t: BstMap<u64, u64> = BstMap::new();
        for i in 0..10 {
            t.insert(i, i);
        }
        let slots_before = t.arena.slots.len();
        for i in 0..10 {
            t.remove(&i);
        }
        for i in 0..10 {
            t.insert(i, i);
        }
        assert_eq!(t.arena.slots.len(), slots_before);
        assert!(verify_bst(&t));
    }

    #[test]
    fn write_ascii_tree_smoke() {
        let mut t = BstMap::new();
        for k in [10u64, 5, 15, 3, 7, 12, 20] {
            t.insert(k, k);
        }
        let mut out = String::new();
        t.write_ascii_tree(&mut out, 32, |k, _v, w| write!(w, "{}", k))
            .unwrap();
        assert!(out.lines().count() >= 1);
    }
}
