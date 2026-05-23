//! Storing tokens sent from servers in NEW_TOKEN frames and using them in subsequent connections

use std::{
    collections::{HashMap, VecDeque, hash_map},
    sync::{Arc, Mutex},
};

use bytes::Bytes;
use lru_slab::LruSlab;
use tracing::trace;

use crate::token::TokenStore;

/// `TokenStore` implementation that stores up to `N` tokens per server name for up to a
/// limited number of server names, in-memory
#[derive(Debug)]
pub struct TokenMemoryCache(Mutex<State>);

impl TokenMemoryCache {
    /// Construct empty
    pub fn new(max_server_names: u32, max_tokens_per_server: usize) -> Self {
        Self(Mutex::new(State::new(
            max_server_names,
            max_tokens_per_server,
        )))
    }
}

impl TokenStore for TokenMemoryCache {
    fn insert(&self, server_name: &str, token: Bytes) {
        trace!(%server_name, "storing token");
        self.0.lock().unwrap().store(server_name, token)
    }

    fn take(&self, server_name: &str) -> Option<Bytes> {
        let token = self.0.lock().unwrap().take(server_name);
        trace!(%server_name, found=%token.is_some(), "taking token");
        token
    }
}

/// Defaults to a maximum of 256 servers and 2 tokens per server
impl Default for TokenMemoryCache {
    fn default() -> Self {
        Self::new(256, 2)
    }
}

/// Lockable inner state of `TokenMemoryCache`
#[derive(Debug)]
struct State {
    max_server_names: u32,
    max_tokens_per_server: usize,
    // map from server name to index in lru
    lookup: HashMap<Arc<str>, u32>,
    lru: LruSlab<CacheEntry>,
}

impl State {
    fn new(max_server_names: u32, max_tokens_per_server: usize) -> Self {
        Self {
            max_server_names,
            max_tokens_per_server,
            lookup: HashMap::new(),
            lru: LruSlab::default(),
        }
    }

    fn store(&mut self, server_name: &str, token: Bytes) {
        if self.max_server_names == 0 {
            // the rest of this method assumes that we can always insert a new entry so long as
            // we're willing to evict a pre-existing entry. thus, an entry limit of 0 is an edge
            // case we must short-circuit on now.
            return;
        }
        if self.max_tokens_per_server == 0 {
            // similarly to above, the rest of this method assumes that we can always push a new
            // token to a queue so long as we're willing to evict a pre-existing token, so we
            // short-circuit on the edge case of a token limit of 0.
            return;
        }

        let server_name = Arc::<str>::from(server_name);
        match self.lookup.entry(server_name.clone()) {
            hash_map::Entry::Occupied(hmap_entry) => {
                // key already exists, push the new token to its token queue
                let tokens = &mut self.lru.get_mut(*hmap_entry.get()).tokens;
                if tokens.len() >= self.max_tokens_per_server {
                    debug_assert!(tokens.len() == self.max_tokens_per_server);
                    tokens.pop_front().unwrap();
                }
                tokens.push_back(token);
            }
            hash_map::Entry::Vacant(hmap_entry) => {
                // key does not yet exist, create a new one, evicting the oldest if necessary
                let removed_key = if self.lru.len() >= self.max_server_names {
                    // unwrap safety: max_server_names is > 0, so there's at least one entry, so
                    //                lru() is some
                    Some(self.lru.remove(self.lru.lru().unwrap()).server_name)
                } else {
                    None
                };

                hmap_entry.insert(self.lru.insert(CacheEntry::new(server_name, token)));

                // for borrowing reasons, we must defer removing the evicted hmap entry to here
                if let Some(removed_slot) = removed_key {
                    let removed = self.lookup.remove(&removed_slot);
                    debug_assert!(removed.is_some());
                }
            }
        };
    }

    fn take(&mut self, server_name: &str) -> Option<Bytes> {
        let slab_key = *self.lookup.get(server_name)?;

        // pop from entry's token queue
        let entry = self.lru.get_mut(slab_key);
        // unwrap safety: we never leave tokens empty
        let token = entry.tokens.pop_front().unwrap();

        if entry.tokens.is_empty() {
            // token stack emptied, remove entry
            self.lru.remove(slab_key);
            self.lookup.remove(server_name);
        }

        Some(token)
    }
}

/// Cache entry within `TokenMemoryCache`'s LRU slab
#[derive(Debug)]
struct CacheEntry {
    server_name: Arc<str>,
    // invariant: tokens is never empty
    tokens: VecDeque<Bytes>,
}

impl CacheEntry {
    /// Construct with a single token
    fn new(server_name: Arc<str>, token: Bytes) -> Self {
        let mut tokens = VecDeque::new();
        tokens.push_back(token);
        Self {
            server_name,
            tokens,
        }
    }
}
