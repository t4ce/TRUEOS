use alloc::vec::Vec;

/// A minimal B+tree mapping `K -> V`.
///
/// Design notes:
/// - Values live only in leaf nodes.
/// - Internal nodes store separator keys and child pointers.
/// - Leaves are linked for ordered iteration.
/// - Deletion is intentionally not implemented yet (early-dev friendly).
///
/// `M` is the maximum number of children for internal nodes.
/// - Max keys per internal node: `M - 1`
/// - Max keys per leaf node: `M - 1`
///
/// Recommended: `M >= 4`.
pub struct BPlusTree<K, V, const M: usize> {
    nodes: Vec<Node<K, V, M>>,
    root: Option<NodeId>,
    len: usize,
}

type NodeId = usize;

enum Node<K, V, const M: usize> {
    Internal {
        keys: Vec<K>,
        children: Vec<NodeId>,
    },
    Leaf {
        keys: Vec<K>,
        values: Vec<V>,
        next: Option<NodeId>,
    },
}

impl<K: Ord + Clone, V, const M: usize> BPlusTree<K, V, M> {
    pub const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            root: None,
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns a reference to the value for `key` if present.
    pub fn get(&self, key: &K) -> Option<&V> {
        let mut id = self.root?;
        loop {
            match &self.nodes[id] {
                Node::Internal { keys, children } => {
                    let idx = upper_bound(keys, key);
                    id = children.get(idx).copied()?;
                }
                Node::Leaf { keys, values, .. } => match lower_bound(keys, key) {
                    Ok(pos) => return values.get(pos),
                    Err(_) => return None,
                },
            }
        }
    }

    /// Inserts `key -> value`.
    ///
    /// Returns the old value if the key already existed.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        debug_assert!(M >= 3, "BPlusTree requires M >= 3");

        let root = match self.root {
            Some(r) => r,
            None => {
                let leaf = self.alloc_leaf();
                self.root = Some(leaf);
                // insert into empty leaf
                if let Node::Leaf { keys, values, .. } = &mut self.nodes[leaf] {
                    keys.push(key);
                    values.push(value);
                }
                self.len = 1;
                return None;
            }
        };

        // Path of internal nodes: (node_id, child_index_taken)
        let mut path: Vec<(NodeId, usize)> = Vec::new();

        // Descend to leaf.
        let mut id = root;
        loop {
            match &self.nodes[id] {
                Node::Internal { keys, children } => {
                    let idx = upper_bound(keys, &key);
                    path.push((id, idx));
                    id = children[idx];
                }
                Node::Leaf { .. } => break,
            }
        }

        // Insert into leaf.
        let mut promoted: Option<(K, NodeId)> = None;
        {
            let leaf_id = id;
            let Node::Leaf { keys, values, .. } = &mut self.nodes[leaf_id] else {
                unreachable!();
            };

            match lower_bound(keys, &key) {
                Ok(pos) => {
                    let old = core::mem::replace(&mut values[pos], value);
                    return Some(old);
                }
                Err(pos) => {
                    keys.insert(pos, key);
                    values.insert(pos, value);
                    self.len = self.len.saturating_add(1);
                }
            }

            if keys.len() > (M - 1) {
                promoted = Some(self.split_leaf(leaf_id));
            }
        }

        // Propagate splits up.
        while let Some((promo_key, right_id)) = promoted {
            match path.pop() {
                None => {
                    // Leaf/internal was root; create new root.
                    let new_root = self.alloc_internal();
                    self.nodes[new_root] = Node::Internal {
                        keys: alloc::vec![promo_key],
                        children: alloc::vec![self.root.unwrap(), right_id],
                    };
                    self.root = Some(new_root);
                    promoted = None;
                }
                Some((parent_id, child_idx)) => {
                    let Node::Internal { keys, children } = &mut self.nodes[parent_id] else {
                        unreachable!();
                    };

                    // Insert separator key and new right child after the child we descended into.
                    keys.insert(child_idx, promo_key);
                    children.insert(child_idx + 1, right_id);

                    if keys.len() > (M - 1) {
                        promoted = Some(self.split_internal(parent_id));
                    } else {
                        promoted = None;
                    }
                }
            }
        }

        None
    }

    /// Iterates over keys/values in sorted key order.
    pub fn iter(&self) -> Iter<'_, K, V, M> {
        let (leaf, pos) = self.leftmost_leaf();
        Iter {
            tree: self,
            leaf,
            pos,
        }
    }

    fn leftmost_leaf(&self) -> (Option<NodeId>, usize) {
        let mut id = match self.root {
            Some(r) => r,
            None => return (None, 0),
        };
        loop {
            match &self.nodes[id] {
                Node::Internal { children, .. } => id = children[0],
                Node::Leaf { .. } => return (Some(id), 0),
            }
        }
    }

    fn alloc_leaf(&mut self) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(Node::Leaf {
            keys: Vec::new(),
            values: Vec::new(),
            next: None,
        });
        id
    }

    fn alloc_internal(&mut self) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(Node::Internal {
            keys: Vec::new(),
            children: Vec::new(),
        });
        id
    }

    fn split_leaf(&mut self, leaf_id: NodeId) -> (K, NodeId) {
        // Split leaf into (left=existing leaf, right=new leaf).
        let (right_keys, right_values, old_next, promote_key);
        {
            let Node::Leaf { keys, values, next } = &mut self.nodes[leaf_id] else {
                unreachable!();
            };

            let split_at = keys.len() / 2;
            let mut rk = keys.split_off(split_at);
            let mut rv = values.split_off(split_at);

            promote_key = rk[0].clone();
            old_next = *next;

            right_keys = core::mem::take(&mut rk);
            right_values = core::mem::take(&mut rv);
        }

        let right_id = self.alloc_leaf();
        self.nodes[right_id] = Node::Leaf {
            keys: right_keys,
            values: right_values,
            next: old_next,
        };

        // Link left -> right
        let Node::Leaf { next, .. } = &mut self.nodes[leaf_id] else {
            unreachable!();
        };
        *next = Some(right_id);

        (promote_key, right_id)
    }

    fn split_internal(&mut self, node_id: NodeId) -> (K, NodeId) {
        // Split internal node and promote the middle key.
        let (promote, right_keys, right_children);
        {
            let Node::Internal { keys, children } = &mut self.nodes[node_id] else {
                unreachable!();
            };

            // After insertion, internal node has up to M keys and M+1 children.
            let mid = keys.len() / 2;
            promote = keys[mid].clone();

            // Right side gets keys after mid.
            let mut rk = keys.split_off(mid + 1);
            // Remove promoted key from left keys.
            keys.pop();

            // Children: left keeps 0..=mid, right gets mid+1..
            let rc = children.split_off(mid + 1);

            right_keys = core::mem::take(&mut rk);
            right_children = rc;
        }

        let right_id = self.alloc_internal();
        self.nodes[right_id] = Node::Internal {
            keys: right_keys,
            children: right_children,
        };

        (promote, right_id)
    }
}

impl<K: Ord + Clone, V, const M: usize> Default for BPlusTree<K, V, M> {
    fn default() -> Self {
        Self::new()
    }
}

/// Ordered iterator over a B+tree.
pub struct Iter<'a, K, V, const M: usize> {
    tree: &'a BPlusTree<K, V, M>,
    leaf: Option<NodeId>,
    pos: usize,
}

impl<'a, K, V, const M: usize> Iterator for Iter<'a, K, V, M> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        let leaf_id = self.leaf?;
        match &self.tree.nodes[leaf_id] {
            Node::Leaf { keys, values, next } => {
                if self.pos < keys.len() {
                    let k = &keys[self.pos];
                    let v = &values[self.pos];
                    self.pos += 1;
                    Some((k, v))
                } else {
                    self.leaf = *next;
                    self.pos = 0;
                    self.next()
                }
            }
            Node::Internal { .. } => None,
        }
    }
}

fn upper_bound<K: Ord>(keys: &[K], key: &K) -> usize {
    // first index i where keys[i] > key
    let mut lo = 0usize;
    let mut hi = keys.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if &keys[mid] <= key {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

fn lower_bound<K: Ord>(keys: &[K], key: &K) -> Result<usize, usize> {
    // Ok(pos) if found, Err(insertion_pos) if not
    let mut lo = 0usize;
    let mut hi = keys.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if &keys[mid] < key {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo < keys.len() && &keys[lo] == key {
        Ok(lo)
    } else {
        Err(lo)
    }
}

#[cfg(test)]
mod tests {
    use super::BPlusTree;

    #[test]
    fn insert_get_smoke() {
        let mut t: BPlusTree<u64, &'static str, 4> = BPlusTree::new();
        assert!(t.is_empty());

        assert_eq!(t.insert(10, "a"), None);
        assert_eq!(t.insert(20, "b"), None);
        assert_eq!(t.insert(15, "c"), None);

        assert_eq!(t.get(&10), Some(&"a"));
        assert_eq!(t.get(&15), Some(&"c"));
        assert_eq!(t.get(&20), Some(&"b"));
        assert_eq!(t.get(&99), None);
    }

    #[test]
    fn insert_replace() {
        let mut t: BPlusTree<u64, u32, 4> = BPlusTree::new();
        assert_eq!(t.insert(1, 10), None);
        assert_eq!(t.insert(1, 11), Some(10));
        assert_eq!(t.get(&1), Some(&11));
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn split_leaf_and_iter_order() {
        // M=4 -> max keys per node = 3, so inserting 10 keys forces multiple splits.
        let mut t: BPlusTree<u64, u32, 4> = BPlusTree::new();
        for i in 0..10u64 {
            t.insert(i, (i as u32) * 2);
        }

        for i in 0..10u64 {
            assert_eq!(t.get(&i), Some(&((i as u32) * 2)));
        }

        let collected: alloc::vec::Vec<(u64, u32)> = t.iter().map(|(k, v)| (*k, *v)).collect();
        assert_eq!(collected.len(), 10);
        for (idx, (k, v)) in collected.into_iter().enumerate() {
            assert_eq!(k, idx as u64);
            assert_eq!(v, (idx as u32) * 2);
        }
    }

    #[test]
    fn vec_u8_keys_are_lexicographic() {
        let mut t: BPlusTree<alloc::vec::Vec<u8>, u32, 4> = BPlusTree::new();
        t.insert(b"b".to_vec(), 2);
        t.insert(b"aa".to_vec(), 1);
        t.insert(b"a".to_vec(), 0);

        assert_eq!(t.get(&b"a".to_vec()), Some(&0));
        assert_eq!(t.get(&b"aa".to_vec()), Some(&1));
        assert_eq!(t.get(&b"b".to_vec()), Some(&2));

        let keys: alloc::vec::Vec<alloc::vec::Vec<u8>> = t.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(keys, alloc::vec![b"a".to_vec(), b"aa".to_vec(), b"b".to_vec()]);
    }
}
