#![allow(dead_code)]
extern crate alloc;
use sha2::{Digest, Sha256};
use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Waker};

mod block {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum Error {
        InvalidParam,
        Io,
        Corrupted,
    }
    #[derive(Clone, Copy)]
    pub struct DeviceHandle(pub u32);
    impl DeviceHandle {
        pub fn id(self) -> u32 {
            self.0
        }
        pub fn parent(self) -> Option<Self> {
            (self.0 == 9).then_some(Self(1))
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ContentTypeId(u32);
impl ContentTypeId {
    const NONE: Self = Self(0);
    const BLOB: Self = Self(1);
    const TEXT: Self = Self(2);
    fn is_registered(self) -> bool {
        (1..=2).contains(&self.0)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecordKey {
    Ffa,
    Protected,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NodeKind {
    File,
    Directory,
}
#[derive(Clone)]
struct Entry {
    entry_lba: u64,
    content_type: ContentTypeId,
}
#[derive(Clone)]
struct File {
    record: trueos_fs::FileRecordRef,
    bytes: Vec<u8>,
}
struct State {
    current: Option<File>,
    pending: Option<File>,
    action: u8,
    invalid_index: bool,
    ensures: usize,
    reads: usize,
    published: usize,
    cache: usize,
    watches: usize,
    read_error: bool,
    finish_error: bool,
    finish_pending: bool,
    short_read: bool,
}
static STATE: Mutex<State> = Mutex::new(State {
    current: None,
    pending: None,
    action: 0,
    invalid_index: false,
    ensures: 0,
    reads: 0,
    published: 0,
    cache: 0,
    watches: 0,
    read_error: false,
    finish_error: false,
    finish_pending: false,
    short_read: false,
});
static HELD: AtomicBool = AtomicBool::new(false);
mod disc {
    pub mod access {
        pub struct Activity;
        impl Activity {
            pub fn begin(_: crate::block::DeviceHandle) -> Result<Self, crate::block::Error> {
                Ok(Self)
            }
        }
    }
}
// This fixture exercises trusted host conditional-write races; caller policy
// and real parent inheritance are exercised by native_file_objects.rs.
mod authorized {
    #[derive(Clone, Copy)] pub enum Actor { Host }
    pub async fn check_path_under_lease(_: Actor, _: crate::block::DeviceHandle, _: &crate::trueos_fs::FsParams, _: &str) -> Result<(), crate::block::Error> {
        assert!(crate::HELD.load(crate::Ordering::SeqCst)); Ok(())
    }
}
async fn inherited_record_key_under_lease(_: block::DeviceHandle, _: &trueos_fs::FsParams, _: &str) -> Result<RecordKey, block::Error> {
    assert!(HELD.load(Ordering::SeqCst)); Ok(RecordKey::Ffa)
}
struct Placement {
    super_lba: u64,
    data_lba: u64,
    data_end_lba_exclusive: Option<u64>,
}
async fn placement_for_io_async(_: block::DeviceHandle) -> Result<Option<Placement>, block::Error> {
    Ok(Some(Placement {
        super_lba: 1,
        data_lba: 2,
        data_end_lba_exclusive: Some(100000),
    }))
}
async fn ensure_index_async(_: block::DeviceHandle, _: &Placement) -> Result<(), block::Error> {
    assert!(
        !HELD.load(Ordering::SeqCst),
        "index construction cannot hold write lease"
    );
    let mut state = STATE.lock().unwrap();
    state.ensures += 1;
    state.invalid_index = false;
    Ok(())
}
fn validate_indexed_file_target(_: u32, path: &str, legacy: bool) -> Result<bool, block::Error> {
    assert!(HELD.load(Ordering::SeqCst));
    assert!(!legacy);
    if path == "missing-parent/file" {
        return Err(block::Error::InvalidParam);
    }
    Ok(!STATE.lock().unwrap().invalid_index)
}
fn indexed_removal_entry(_: u32, _: &str) -> Result<Option<Entry>, block::Error> {
    assert!(HELD.load(Ordering::SeqCst));
    Ok(STATE.lock().unwrap().current.as_ref().map(|f| Entry {
        entry_lba: f.record.entry_lba,
        content_type: f.record.content_type,
    }))
}
mod write_gate {
    pub struct RootWriteLease;
    impl RootWriteLease {
        pub async fn acquire(_: u32) -> Self {
            assert!(!crate::HELD.swap(true, crate::Ordering::SeqCst));
            let mut state = crate::STATE.lock().unwrap();
            match std::mem::take(&mut state.action) {
                1 => state.current.as_mut().unwrap().bytes[0] ^= 1,
                2 => {
                    state.current.as_mut().unwrap().record.record_key = crate::RecordKey::Protected
                }
                3 => state.current = None,
                4 => state.invalid_index = true,
                _ => {}
            }
            Self
        }
    }
    impl Drop for RootWriteLease {
        fn drop(&mut self) {
            assert!(crate::HELD.swap(false, crate::Ordering::SeqCst));
        }
    }
}
struct KernelBlockIo;
impl KernelBlockIo {
    fn new(_: block::DeviceHandle) -> Self {
        Self
    }
}
mod trueos_fs {
    use super::*;
    #[derive(Clone, Copy)]
    pub struct FileRecordRef {
        pub file_id: u64,
        pub entry_lba: u64,
        pub data_lba: u64,
        pub data_len: u64,
        pub content_type: ContentTypeId,
        pub record_key: RecordKey,
    }
    pub struct Record {
        pub kind: NodeKind,
        pub file_id: u64,
        pub entry_lba: u64,
        pub data_lba: u64,
        pub data_len: u64,
        pub content_type: ContentTypeId,
        pub record_key: RecordKey,
    }
    pub struct FileWriteMetadata {
        pub file_id: u64,
        pub content_type: ContentTypeId,
        pub record_key: RecordKey,
    }
    pub struct FsParams {
        pub super_lba: u64,
        pub data_lba: u64,
        pub data_end_lba_exclusive: Option<u64>,
    }
    pub struct Stream {
        file: File,
    }
    pub async fn get_node_record_by_lba(
        _: &KernelBlockIo,
        _: &FsParams,
        lba: u64,
    ) -> Result<Option<Record>, block::Error> {
        assert!(HELD.load(Ordering::SeqCst));
        let state = STATE.lock().unwrap();
        let f = state.current.as_ref().unwrap();
        assert_eq!(lba, f.record.entry_lba);
        let r = f.record;
        Ok(Some(Record {
            kind: NodeKind::File,
            file_id: r.file_id,
            entry_lba: r.entry_lba,
            data_lba: r.data_lba,
            data_len: r.data_len,
            content_type: r.content_type,
            record_key: r.record_key,
        }))
    }
    pub async fn read_file_range_at(
        _: &KernelBlockIo,
        _: &FsParams,
        r: &FileRecordRef,
        offset: u64,
        out: &mut [u8],
    ) -> Result<Option<usize>, block::Error> {
        assert!(HELD.load(Ordering::SeqCst));
        assert!(out.len() <= 65536);
        let mut state = STATE.lock().unwrap();
        state.reads += 1;
        if state.read_error {
            return Err(block::Error::Io);
        }
        if state.short_read {
            return Ok(Some(0));
        }
        let f = state.current.as_ref().unwrap();
        assert_eq!(r.entry_lba, f.record.entry_lba);
        out.copy_from_slice(&f.bytes[offset as usize..offset as usize + out.len()]);
        Ok(Some(out.len()))
    }
    pub async fn begin_write_file_stream_prevalidated(
        _: &KernelBlockIo,
        _: &FsParams,
        _: &str,
        len: u64,
        m: FileWriteMetadata,
    ) -> Result<Option<Stream>, block::Error> {
        assert!(HELD.load(Ordering::SeqCst));
        Ok(Some(Stream {
            file: File {
                record: FileRecordRef {
                    file_id: if m.file_id == 0 { 99 } else { m.file_id },
                    entry_lba: 200,
                    data_lba: 201,
                    data_len: len,
                    content_type: m.content_type,
                    record_key: m.record_key,
                },
                bytes: Vec::new(),
            },
        }))
    }
    pub async fn write_file_stream_chunk(
        _: &KernelBlockIo,
        s: &mut Stream,
        bytes: &[u8],
    ) -> Result<(), block::Error> {
        assert!(HELD.load(Ordering::SeqCst));
        s.file.bytes.extend_from_slice(bytes);
        Ok(())
    }
    pub fn write_stream_record_ref(s: &Stream) -> FileRecordRef {
        s.file.record
    }
    pub async fn finish_write_file_stream(
        _: &KernelBlockIo,
        _: &FsParams,
        s: Stream,
    ) -> Result<(), block::Error> {
        assert!(HELD.load(Ordering::SeqCst));
        let pending = {
            let mut state = STATE.lock().unwrap();
            if state.finish_error {
                return Err(block::Error::Io);
            }
            state.pending = Some(s.file.clone());
            state.finish_pending
        };
        if pending {
            std::future::pending::<()>().await;
        }
        let mut state = STATE.lock().unwrap();
        assert_eq!(s.file.bytes.len() as u64, s.file.record.data_len);
        state.current = Some(s.file);
        state.published += 1;
        Ok(())
    }
}
fn map_engine_err(e: block::Error) -> block::Error {
    e
}
fn record_successful_content_commit(_: ContentTypeId, _: bool) {}
fn bump_root_cache_gen(_: u32) {
    assert!(HELD.load(Ordering::SeqCst));
    STATE.lock().unwrap().cache += 1;
}
fn file_record_cache_invalidate_path(_: u32, _: &str) {
    assert!(HELD.load(Ordering::SeqCst));
}
fn file_record_cache_insert(_: u32, _: &str, _: trueos_fs::FileRecordRef) {
    assert!(HELD.load(Ordering::SeqCst));
}
fn update_root_index_put(_: u32, _: &str, _: trueos_fs::FileRecordRef) -> bool {
    assert!(HELD.load(Ordering::SeqCst));
    true
}
fn invalidate_root_index(_: u32) {
    panic!("unexpected invalidation")
}
mod file_watch {
    pub fn changed(_: u32, _: &str) {
        assert!(crate::HELD.load(crate::Ordering::SeqCst));
        crate::STATE.lock().unwrap().watches += 1;
    }
}
mod content_type_boundary {
    pub fn verify_named_bytes(path: &str, bytes: &[u8]) -> Result<super::ContentTypeId, ()> {
        if path.ends_with(".txt") && std::str::from_utf8(bytes).is_ok() {
            Ok(super::ContentTypeId::TEXT)
        } else {
            Err(())
        }
    }
}
mod production {
    use super::*;
    // @PRODUCTION@
}
#[cfg(test)]
use production::{CheckedWriteError, file_in_typed_checked_async, reset_blob_checked_async};
const DISK: block::DeviceHandle = block::DeviceHandle(1);
fn complete<F: Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("unexpected pending"),
    }
}
fn reset(bytes: &[u8], kind: ContentTypeId, key: RecordKey) -> [u8; 32] {
    assert!(!HELD.load(Ordering::SeqCst));
    let mut s = STATE.lock().unwrap();
    s.current = Some(File {
        record: trueos_fs::FileRecordRef {
            file_id: 42,
            entry_lba: 10,
            data_lba: 11,
            data_len: bytes.len() as u64,
            content_type: kind,
            record_key: key,
        },
        bytes: bytes.to_vec(),
    });
    s.pending = None;
    s.action = 0;
    s.invalid_index = false;
    s.ensures = 0;
    s.reads = 0;
    s.published = 0;
    s.cache = 0;
    s.watches = 0;
    s.read_error = false;
    s.finish_error = false;
    s.finish_pending = false;
    s.short_read = false;
    Sha256::digest(bytes).into()
}
fn unchanged() {
    let s = STATE.lock().unwrap();
    assert_eq!((s.published, s.cache, s.watches), (0, 0, 0));
    assert!(!HELD.load(Ordering::SeqCst));
}

#[test]
fn reset_preserves_identity_and_key_and_publishes_empty_blob() {
    let digest = reset(b"old text", ContentTypeId::TEXT, RecordKey::Ffa);
    assert_eq!(
        complete(reset_blob_checked_async(DISK, "a.txt", digest)),
        Ok(true)
    );
    let s = STATE.lock().unwrap();
    let f = s.current.as_ref().unwrap();
    assert!(f.bytes.is_empty());
    assert_eq!(
        (f.record.file_id, f.record.record_key, f.record.content_type),
        (42, RecordKey::Ffa, ContentTypeId::BLOB)
    );
    assert_eq!((s.published, s.cache, s.watches), (1, 1, 1));
}
#[test]
fn reset_rejects_missing_modified_and_protected_targets_including_waiting_races() {
    for action in 0..=3 {
        let mut digest = reset(b"old text", ContentTypeId::TEXT, RecordKey::Ffa);
        STATE.lock().unwrap().action = action;
        if action == 0 {
            digest[0] ^= 1;
        }
        assert_eq!(
            complete(reset_blob_checked_async(DISK, "a.txt", digest)),
            Err(CheckedWriteError::Conflict)
        );
        unchanged();
    }
    let digest = reset(b"old", ContentTypeId::TEXT, RecordKey::Protected);
    assert_eq!(
        complete(reset_blob_checked_async(DISK, "a.txt", digest)),
        Err(CheckedWriteError::Conflict)
    );
    unchanged();
}
#[test]
fn hash_reads_are_bounded_and_index_retry_releases_lease() {
    let bytes = vec![b'x'; 131073];
    let digest = reset(&bytes, ContentTypeId::TEXT, RecordKey::Ffa);
    STATE.lock().unwrap().action = 4;
    assert_eq!(
        complete(reset_blob_checked_async(DISK, "a.txt", digest)),
        Ok(true)
    );
    let s = STATE.lock().unwrap();
    assert_eq!((s.ensures, s.reads), (2, 3));
}
#[test]
fn io_failure_or_zero_progress_preserves_file_without_cache_or_watch_publication() {
    for mode in 0..3 {
        let digest = reset(b"old", ContentTypeId::TEXT, RecordKey::Ffa);
        {
            let mut s = STATE.lock().unwrap();
            s.read_error = mode == 0;
            s.short_read = mode == 1;
            s.finish_error = mode == 2;
        }
        assert!(matches!(
            complete(reset_blob_checked_async(DISK, "a.txt", digest)),
            Err(CheckedWriteError::Device(_))
        ));
        unchanged();
        assert_eq!(
            STATE.lock().unwrap().current.as_ref().unwrap().bytes,
            b"old"
        );
    }
}
#[test]
fn cancellation_before_commit_releases_lease_and_keeps_old_file() {
    let digest = reset(b"old", ContentTypeId::TEXT, RecordKey::Ffa);
    STATE.lock().unwrap().finish_pending = true;
    let mut f = Box::pin(reset_blob_checked_async(DISK, "a.txt", digest));
    assert!(
        f.as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    assert!(HELD.load(Ordering::SeqCst));
    drop(f);
    unchanged();
    assert_eq!(
        STATE.lock().unwrap().current.as_ref().unwrap().bytes,
        b"old"
    );
}
#[test]
fn ordinary_checked_replace_retains_type_key_and_id_and_create_only_stays_atomic() {
    let digest = reset(b"old", ContentTypeId::TEXT, RecordKey::Protected);
    assert_eq!(
        complete(file_in_typed_checked_async(
            DISK,
            "a.txt",
            b"new",
            ContentTypeId::BLOB,
            false,
            Some(digest),
            false
        )),
        Ok(true)
    );
    {
        let s = STATE.lock().unwrap();
        let f = s.current.as_ref().unwrap();
        assert_eq!(
            (f.record.file_id, f.record.record_key, f.record.content_type),
            (42, RecordKey::Protected, ContentTypeId::TEXT)
        );
        assert_eq!(f.bytes, b"new");
    }
    reset(b"old", ContentTypeId::TEXT, RecordKey::Ffa);
    assert_eq!(
        complete(file_in_typed_checked_async(
            DISK,
            "a.txt",
            b"new",
            ContentTypeId::BLOB,
            true,
            None,
            false
        )),
        Err(CheckedWriteError::AlreadyExists)
    );
    unchanged();
}
#[test]
fn inference_still_requires_unchanged_blob_with_matching_preview_and_named_type() {
    let digest = reset(b"text", ContentTypeId::BLOB, RecordKey::Ffa);
    assert_eq!(
        complete(file_in_typed_checked_async(
            DISK,
            "a.txt",
            b"text",
            ContentTypeId::TEXT,
            false,
            Some(digest),
            true
        )),
        Ok(true)
    );
    assert_eq!(
        STATE
            .lock()
            .unwrap()
            .current
            .as_ref()
            .unwrap()
            .record
            .content_type,
        ContentTypeId::TEXT
    );
    for mode in 0..3 {
        let digest = reset(
            b"text",
            if mode == 0 {
                ContentTypeId::TEXT
            } else {
                ContentTypeId::BLOB
            },
            RecordKey::Ffa,
        );
        assert_eq!(
            complete(file_in_typed_checked_async(
                DISK,
                if mode == 1 { "a.bin" } else { "a.txt" },
                if mode == 2 { b"changed" } else { b"text" },
                ContentTypeId::TEXT,
                false,
                Some(digest),
                true
            )),
            Err(CheckedWriteError::Conflict)
        );
        unchanged();
    }
}
