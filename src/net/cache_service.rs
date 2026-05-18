extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_executor::{SpawnError, Spawner};
use embassy_time::{Duration as EmbassyDuration, Timer};
use sha2::{Digest, Sha256};
use spin::Mutex;

const MAX_CACHE_ENTRIES: usize = 64;
const MAX_PENDING_REQUESTS: usize = 32;
const MAX_COMPLETIONS: usize = 32;

static SERVICE_STARTED: AtomicBool = AtomicBool::new(false);
static NEXT_REQUEST_ID: AtomicU32 = AtomicU32::new(1);
static CACHE: Mutex<BTreeMap<CacheKey, CacheEntry>> = Mutex::new(BTreeMap::new());
static REQUESTS: Mutex<BTreeMap<u32, CacheRequest>> = Mutex::new(BTreeMap::new());
static COMPLETIONS: Mutex<BTreeMap<u32, CacheCompletion>> = Mutex::new(BTreeMap::new());

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct CacheKey(pub u64);

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub digest: [u8; 32],
    pub bytes: Vec<u8>,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CacheError {
    DigestMismatch { actual: [u8; 32] },
    QueueFull,
    Missing,
}

#[derive(Debug)]
pub enum CacheCompletion {
    Stored {
        id: u32,
        key: CacheKey,
        len: usize,
        digest: [u8; 32],
    },
    Verified {
        id: u32,
        key: CacheKey,
        len: usize,
        digest: [u8; 32],
    },
    Hit {
        id: u32,
        key: CacheKey,
        bytes: Vec<u8>,
        digest: [u8; 32],
    },
    Failed {
        id: u32,
        key: CacheKey,
        error: CacheError,
    },
}

enum CacheRequest {
    Store {
        key: CacheKey,
        bytes: Vec<u8>,
        expected_sha256: Option<[u8; 32]>,
    },
    Verify {
        key: CacheKey,
        expected_sha256: [u8; 32],
    },
    Load {
        key: CacheKey,
    },
}

pub fn cache_key_for_bytes(bytes: &[u8]) -> CacheKey {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    CacheKey(hash)
}

pub fn sha256_digest(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub fn store_verified_now(
    key: CacheKey,
    bytes: Vec<u8>,
    expected_sha256: Option<[u8; 32]>,
) -> Result<[u8; 32], CacheError> {
    let digest = sha256_digest(&bytes);
    if let Some(expected) = expected_sha256 {
        if digest != expected {
            return Err(CacheError::DigestMismatch { actual: digest });
        }
    }

    let mut cache = CACHE.lock();
    if cache.len() >= MAX_CACHE_ENTRIES {
        if let Some(oldest_key) = cache.keys().next().copied() {
            cache.remove(&oldest_key);
        }
    }
    cache.insert(key, CacheEntry { digest, bytes });
    Ok(digest)
}

pub fn verify_cached_now(key: CacheKey, expected_sha256: [u8; 32]) -> Result<[u8; 32], CacheError> {
    let cache = CACHE.lock();
    let Some(entry) = cache.get(&key) else {
        return Err(CacheError::Missing);
    };
    if entry.digest != expected_sha256 {
        return Err(CacheError::DigestMismatch {
            actual: entry.digest,
        });
    }
    Ok(entry.digest)
}

pub fn load_cached_now(key: CacheKey) -> Option<CacheEntry> {
    CACHE.lock().get(&key).cloned()
}

pub fn submit_store(
    key: CacheKey,
    bytes: Vec<u8>,
    expected_sha256: Option<[u8; 32]>,
) -> Result<u32, CacheError> {
    submit(CacheRequest::Store {
        key,
        bytes,
        expected_sha256,
    })
}

pub fn submit_verify(key: CacheKey, expected_sha256: [u8; 32]) -> Result<u32, CacheError> {
    submit(CacheRequest::Verify {
        key,
        expected_sha256,
    })
}

pub fn submit_load(key: CacheKey) -> Result<u32, CacheError> {
    submit(CacheRequest::Load { key })
}

pub fn take_completion(id: u32) -> Option<CacheCompletion> {
    COMPLETIONS.lock().remove(&id)
}

pub async fn wait_completion(id: u32, timeout_ms: u64) -> Option<CacheCompletion> {
    if let Some(done) = take_completion(id) {
        return Some(done);
    }
    for _ in 0..timeout_ms {
        Timer::after(EmbassyDuration::from_millis(1)).await;
        if let Some(done) = take_completion(id) {
            return Some(done);
        }
    }
    None
}

pub fn ensure_service_started(spawner: Spawner) -> Result<bool, SpawnError> {
    if SERVICE_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(false);
    }

    match cache_service_task() {
        Ok(token) => {
            spawner.spawn(token);
            Ok(true)
        }
        Err(err) => {
            SERVICE_STARTED.store(false, Ordering::Release);
            Err(err)
        }
    }
}

#[embassy_executor::task]
pub async fn cache_service_task() {
    loop {
        while let Some((id, request)) = take_request() {
            let completion = match request {
                CacheRequest::Store {
                    key,
                    bytes,
                    expected_sha256,
                } => {
                    let len = bytes.len();
                    match store_verified_now(key, bytes, expected_sha256) {
                        Ok(digest) => CacheCompletion::Stored {
                            id,
                            key,
                            len,
                            digest,
                        },
                        Err(error) => CacheCompletion::Failed { id, key, error },
                    }
                }
                CacheRequest::Verify {
                    key,
                    expected_sha256,
                } => match verify_cached_now(key, expected_sha256) {
                    Ok(digest) => {
                        let len = CACHE
                            .lock()
                            .get(&key)
                            .map(|entry| entry.bytes.len())
                            .unwrap_or(0);
                        CacheCompletion::Verified {
                            id,
                            key,
                            len,
                            digest,
                        }
                    }
                    Err(error) => CacheCompletion::Failed { id, key, error },
                },
                CacheRequest::Load { key } => match load_cached_now(key) {
                    Some(entry) => CacheCompletion::Hit {
                        id,
                        key,
                        bytes: entry.bytes,
                        digest: entry.digest,
                    },
                    None => CacheCompletion::Failed {
                        id,
                        key,
                        error: CacheError::Missing,
                    },
                },
            };
            push_completion(id, completion);
        }

        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
}

fn submit(request: CacheRequest) -> Result<u32, CacheError> {
    let mut requests = REQUESTS.lock();
    if requests.len() >= MAX_PENDING_REQUESTS {
        return Err(CacheError::QueueFull);
    }
    let id = NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed).max(1);
    requests.insert(id, request);
    Ok(id)
}

fn take_request() -> Option<(u32, CacheRequest)> {
    let mut requests = REQUESTS.lock();
    let id = requests.keys().next().copied()?;
    requests.remove(&id).map(|request| (id, request))
}

fn push_completion(id: u32, completion: CacheCompletion) {
    let mut completions = COMPLETIONS.lock();
    if completions.len() >= MAX_COMPLETIONS {
        if let Some(oldest_id) = completions.keys().next().copied() {
            completions.remove(&oldest_id);
        }
    }
    completions.insert(id, completion);
}
