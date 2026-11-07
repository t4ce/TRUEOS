use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Mutex, MutexGuard};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Waker};

struct Lock<T>(Mutex<T>);
impl<T> Lock<T> {
    const fn new(value: T) -> Self { Self(Mutex::new(value)) }
    fn lock(&self) -> MutexGuard<'_, T> { self.0.lock().unwrap() }
}
mod block {
    pub type DiscId = u32;
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub enum Error { InvalidParam, Io, NotReady, Corrupted }
    #[derive(Clone, Copy)] pub struct DeviceHandle(pub u32);
    impl DeviceHandle {
        pub fn id(self) -> u32 { self.0 }
        pub fn parent(self) -> Option<Self> { (self.0 == 9).then_some(Self(1)) }
    }
}
mod disc { pub mod access {
    pub struct Activity;
    impl Activity {
        pub fn begin(_: crate::block::DeviceHandle) -> Result<Self, crate::block::Error> {
            if crate::STATE.lock().activity_error { return Err(crate::block::Error::Io); }
            crate::ACTIVE.fetch_add(1, crate::Ordering::SeqCst); Ok(Self)
        }
    }
    impl Drop for Activity { fn drop(&mut self) { assert_eq!(crate::ACTIVE.fetch_sub(1, crate::Ordering::SeqCst), 1); } }
} }
#[derive(Clone, Copy, Debug, Eq, PartialEq)] enum NodeKind { File, Directory }
#[derive(Clone, Copy, Debug, Eq, PartialEq)] struct IndexRef { kind: trueos_fs::LogKind, entry_lba: u64, content_type: u32 }
type TrueosFsIndex = BTreeMap<Vec<u8>, IndexRef>;
struct RootMount { disk_id: u32, index: Option<TrueosFsIndex>, writes_since_checkpoint: u64 }
static ROOTS: Lock<Vec<RootMount>> = Lock::new(Vec::new());
#[derive(Default)]
struct State {
    live: TrueosFsIndex, records: BTreeMap<u64, trueos_fs::Record>,
    action: u8, delete_mode: u8, placement_mode: u8, read_mode: u8,
    activity_error: bool, update_failure: bool, ensures: usize, reads: usize,
    calls: Vec<(&'static str, String)>, published: Vec<String>, cache: Vec<String>, watch: Vec<String>, invalidated: bool,
}
static STATE: Lock<State> = Lock::new(State {
    live: BTreeMap::new(), records: BTreeMap::new(), action: 0, delete_mode: 0, placement_mode: 0, read_mode: 0,
    activity_error: false, update_failure: false, ensures: 0, reads: 0,
    calls: Vec::new(), published: Vec::new(), cache: Vec::new(), watch: Vec::new(), invalidated: false,
});
static HELD: AtomicUsize = AtomicUsize::new(0);
static ACTIVE: AtomicUsize = AtomicUsize::new(0);
struct Placement { super_lba: u64, data_lba: u64, data_end_lba_exclusive: Option<u64> }
async fn placement_for_io_async(_: block::DeviceHandle) -> Result<Option<Placement>, block::Error> {
    match STATE.lock().placement_mode {
        1 => Ok(None), 2 => Err(block::Error::Io),
        _ => Ok(Some(Placement { super_lba: 1, data_lba: 2, data_end_lba_exclusive: Some(100) })),
    }
}
async fn ensure_index_async(_: block::DeviceHandle, _: &Placement) -> Result<(), block::Error> {
    assert_eq!(HELD.load(Ordering::SeqCst), 0, "index rebuild must precede root write lease");
    let mut state = STATE.lock(); state.ensures += 1;
    let mut roots = ROOTS.lock();
    if roots[0].index.is_none() { roots[0].index = Some(state.live.clone()); }
    Ok(())
}
fn insert(path: &str, kind: NodeKind, lba: u64) {
    let entry = IndexRef { kind: if kind == NodeKind::File { trueos_fs::LogKind::Put } else { trueos_fs::LogKind::Directory }, entry_lba: lba, content_type: 42 };
    let mut state = STATE.lock(); state.live.insert(path.as_bytes().to_vec(), entry);
    state.records.insert(lba, trueos_fs::Record { kind, entry_lba: lba, content_type: 42 });
    ROOTS.lock()[0].index.as_mut().unwrap().insert(path.as_bytes().to_vec(), entry);
}
mod write_gate {
    pub struct RootWriteLease;
    impl RootWriteLease {
        pub async fn acquire(_: u32) -> Self {
            assert_eq!(crate::HELD.fetch_add(1, crate::Ordering::SeqCst), 0);
            let action = std::mem::take(&mut crate::STATE.lock().action);
            // A competing writer committed while this requester waited for its lease.
            // Apply that state before the production lookup resumes under exclusivity.
            match action {
                1 => crate::insert("dir/file", crate::NodeKind::Directory, 11),
                2 => crate::ROOTS.lock()[0].index = None,
                3 => crate::insert("dir/child", crate::NodeKind::File, 12),
                4 => { crate::ROOTS.lock()[0].index.as_mut().unwrap().remove(b"dir/file".as_slice()); crate::STATE.lock().live.remove(b"dir/file".as_slice()); }
                5 => crate::insert("dir", crate::NodeKind::File, 13),
                _ => {}
            }
            Self
        }
    }
    impl Drop for RootWriteLease { fn drop(&mut self) { assert_eq!(crate::HELD.fetch_sub(1, crate::Ordering::SeqCst), 1); } }
}
struct KernelBlockIo(block::DeviceHandle);
impl KernelBlockIo { fn new(disk: block::DeviceHandle) -> Self { Self(disk) } }
mod trueos_fs {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub enum LogKind { Directory, Put, Delete }
    #[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct Record { pub kind: crate::NodeKind, pub entry_lba: u64, pub content_type: u32 }
    pub struct FsParams { pub super_lba: u64, pub data_lba: u64, pub data_end_lba_exclusive: Option<u64> }
    fn boundary(io: &crate::KernelBlockIo, params: &FsParams) {
        assert_eq!(io.0.id(), 1); assert_eq!(crate::HELD.load(crate::Ordering::SeqCst), 1);
        assert_eq!((params.super_lba, params.data_lba, params.data_end_lba_exclusive), (1, 2, Some(100)));
    }
    pub async fn get_node_record_by_lba(io: &crate::KernelBlockIo, params: &FsParams, lba: u64) -> Result<Option<Record>, crate::block::Error> {
        boundary(io, params);
        let mut state = crate::STATE.lock(); state.reads += 1;
        match state.read_mode { 1 => Err(crate::block::Error::Io), 2 => Ok(None), _ => Ok(state.records.get(&lba).copied()) }
    }
    async fn delete(io: &crate::KernelBlockIo, params: &FsParams, path: &str, record: &Record, recursive: bool, prevalidated_directory: bool) -> Result<bool, crate::block::Error> {
        boundary(io, params);
        let (mode, update_failure) = {
            let mut state = crate::STATE.lock();
            assert_eq!(state.records.get(&record.entry_lba), Some(record));
            state.calls.push((if recursive { "tree" } else if prevalidated_directory { "directory-prevalidated" } else { "node" }, path.into()));
            if prevalidated_directory { assert_eq!(record.kind, crate::NodeKind::Directory); }
            (state.delete_mode, state.update_failure)
        };
        // This controlled pending point precedes publication; dropping it must
        // release the exclusive lease and Activity without pretending rollback.
        if mode == 3 { std::future::pending::<()>().await; }
        if mode == 1 { return Err(crate::block::Error::Io); }
        if mode == 2 { return Ok(false); }
        let mut state = crate::STATE.lock();
        if !recursive && record.kind == crate::NodeKind::Directory {
            let prefix = crate::normalized_dir_prefix(path);
            assert!(!state.live.keys().any(|p| p.starts_with(prefix.as_bytes())), "leaf deletion must never remove children");
        }
        if recursive {
            let prefix = crate::normalized_dir_prefix(path);
            state.live.retain(|p, _| p != path.as_bytes() && !p.starts_with(prefix.as_bytes()));
        } else { state.live.remove(path.as_bytes()); }
        state.published.push(path.into());
        if update_failure { crate::ROOTS.lock()[0].index = None; }
        Ok(true)
    }
    pub async fn delete_node_at_record(io: &crate::KernelBlockIo, params: &FsParams, path: &str, record: &Record) -> Result<bool, crate::block::Error> { delete(io, params, path, record, false, false).await }
    pub async fn delete_tree_at_record(io: &crate::KernelBlockIo, params: &FsParams, path: &str, record: &Record) -> Result<bool, crate::block::Error> { delete(io, params, path, record, true, false).await }
    pub async fn delete_empty_directory_at_record_prevalidated(io: &crate::KernelBlockIo, params: &FsParams, path: &str, record: &Record) -> Result<bool, crate::block::Error> { delete(io, params, path, record, false, true).await }
}
fn map_engine_err(error: block::Error) -> block::Error { error }
fn bump_root_cache_gen(_: u32) { assert_eq!(HELD.load(Ordering::SeqCst), 1); STATE.lock().cache.push("generation".into()); }
fn file_record_cache_invalidate_prefix(_: u32, prefix: &str) { assert_eq!(HELD.load(Ordering::SeqCst), 1); STATE.lock().cache.push(format!("prefix:{prefix}")); }
fn file_record_cache_invalidate_path(_: u32, path: &str) { assert_eq!(HELD.load(Ordering::SeqCst), 1); STATE.lock().cache.push(format!("path:{path}")); }
fn invalidate_root_index(_: u32) { STATE.lock().invalidated = true; ROOTS.lock()[0].index = None; }
mod file_watch { pub fn changed(_: u32, path: &str) { assert_eq!(crate::HELD.load(crate::Ordering::SeqCst), 1); crate::STATE.lock().watch.push(path.into()); } }
mod production {
    use super::*;
    // @PRODUCTION@
    pub(super) fn prefix_for_fixture(path: &str) -> String { normalized_dir_prefix(path) }
    pub(super) fn empty_for_fixture(id: block::DiscId, path: &str) -> Result<bool, block::Error> { indexed_directory_is_empty(id, path) }
}
use production::{RemoveNodeError, remove_leaf_async, remove_recursive_async};
fn normalized_dir_prefix(path: &str) -> String { production::prefix_for_fixture(path) }
fn indexed_directory_is_empty(id: block::DiscId, path: &str) -> Result<bool, block::Error> { production::empty_for_fixture(id, path) }
fn reset() {
    assert_eq!(HELD.load(Ordering::SeqCst), 0); assert_eq!(ACTIVE.load(Ordering::SeqCst), 0);
    *STATE.lock() = State::default();
    *ROOTS.lock() = vec![RootMount { disk_id: 1, index: Some(BTreeMap::new()), writes_since_checkpoint: 0 }];
    insert("dir", NodeKind::Directory, 1);
}
fn complete<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) { Poll::Ready(value) => value, Poll::Pending => panic!("unexpected pending") }
}
fn remove(path: &str, kind: NodeKind) -> Result<bool, RemoveNodeError> { complete(remove_leaf_async(block::DeviceHandle(1), path, kind)) }
fn unchanged() { let state = STATE.lock(); assert!(state.published.is_empty()); assert!(state.cache.is_empty()); assert!(state.watch.is_empty()); assert_eq!(HELD.load(Ordering::SeqCst), 0); assert_eq!(ACTIVE.load(Ordering::SeqCst), 0); }

#[test] fn removes_file_and_invalidates_caches_once_under_lease() {
    reset(); insert("dir/file", NodeKind::File, 2); assert_eq!(remove("dir/file", NodeKind::File), Ok(true));
    let state = STATE.lock(); assert_eq!(state.calls, [("node", "dir/file".into())]); assert_eq!(state.cache, ["generation", "prefix:dir/file/", "path:dir/file"]); assert_eq!(state.watch, ["dir/file"]);
    assert!(!ROOTS.lock()[0].index.as_ref().unwrap().contains_key(b"dir/file".as_slice())); assert_eq!(ROOTS.lock()[0].writes_since_checkpoint, 1);
}
#[test] fn empty_directory_uses_prevalidated_single_node_tombstone_not_recursive() { reset(); assert_eq!(remove("dir", NodeKind::Directory), Ok(true)); assert_eq!(STATE.lock().calls, [("directory-prevalidated", "dir".into())]); assert!(ROOTS.lock()[0].index.as_ref().unwrap().is_empty()); }
#[test] fn nonempty_directory_is_rejected_without_storage_write() { reset(); insert("dir/child", NodeKind::File, 2); assert_eq!(remove("dir", NodeKind::Directory), Err(RemoveNodeError::NotEmpty)); assert!(STATE.lock().calls.is_empty()); unchanged(); }
#[test] fn descendants_beyond_immediate_child_still_make_directory_nonempty() { reset(); insert("dir/missing-parent/deep", NodeKind::File, 2); assert_eq!(remove("dir", NodeKind::Directory), Err(RemoveNodeError::NotEmpty)); unchanged(); }
#[test] fn child_arriving_during_lease_wait_is_rejected() { reset(); STATE.lock().action = 3; assert_eq!(remove("dir", NodeKind::Directory), Err(RemoveNodeError::NotEmpty)); assert!(STATE.lock().calls.is_empty()); unchanged(); }
#[test] fn file_to_directory_replacement_during_lease_wait_is_rejected() { reset(); insert("dir/file", NodeKind::File, 2); STATE.lock().action = 1; assert_eq!(remove("dir/file", NodeKind::File), Err(RemoveNodeError::KindMismatch)); assert!(STATE.lock().calls.is_empty()); unchanged(); }
#[test] fn directory_to_file_replacement_during_lease_wait_is_rejected() { reset(); STATE.lock().action = 5; assert_eq!(remove("dir", NodeKind::Directory), Err(RemoveNodeError::KindMismatch)); unchanged(); }
#[test] fn missing_and_disappearing_nodes_do_not_publish() { reset(); assert_eq!(remove("missing", NodeKind::File), Ok(false)); unchanged(); insert("dir/file", NodeKind::File, 2); STATE.lock().action = 4; assert_eq!(remove("dir/file", NodeKind::File), Ok(false)); unchanged(); }
#[test] fn prefix_siblings_are_retained_and_do_not_make_leaf_nonempty() { reset(); for (path, lba) in [("dir-other/child", 2), ("dir0", 3), ("dira/file", 4)] { insert(path, NodeKind::File, lba); } assert_eq!(remove("dir", NodeKind::Directory), Ok(true)); assert_eq!(ROOTS.lock()[0].index.as_ref().unwrap().len(), 3); }
#[test] fn invalidated_index_rebuilds_outside_publication_lease() { reset(); STATE.lock().action = 2; assert_eq!(remove("dir", NodeKind::Directory), Ok(true)); assert_eq!(STATE.lock().ensures, 2); assert_eq!(STATE.lock().published, ["dir"]); }
#[test] fn invalid_root_and_partition_handles_are_rejected_before_io() { for path in ["", "/"] { reset(); assert_eq!(remove(path, NodeKind::Directory), Err(RemoveNodeError::Device(block::Error::InvalidParam))); assert_eq!(STATE.lock().ensures, 0); unchanged(); } reset(); assert_eq!(complete(remove_leaf_async(block::DeviceHandle(9), "dir", NodeKind::Directory)), Err(RemoveNodeError::Device(block::Error::InvalidParam))); assert_eq!(STATE.lock().ensures, 0); unchanged(); }
#[test] fn absent_filesystem_and_placement_errors_do_not_write() { for (mode, expected) in [(1, Ok(false)), (2, Err(RemoveNodeError::Device(block::Error::Io)))] { reset(); STATE.lock().placement_mode = mode; assert_eq!(remove("dir", NodeKind::Directory), expected); assert_eq!(STATE.lock().ensures, 0); unchanged(); } }
#[test] fn activity_error_is_propagated_without_io() { reset(); STATE.lock().activity_error = true; assert_eq!(remove("dir", NodeKind::Directory), Err(RemoveNodeError::Device(block::Error::Io))); assert_eq!(STATE.lock().ensures, 0); unchanged(); }
#[test] fn storage_failure_and_full_volume_never_publish_or_change_index() { for (mode, expected) in [(1, Err(RemoveNodeError::Device(block::Error::Io))), (2, Err(RemoveNodeError::NotCommitted))] { reset(); STATE.lock().delete_mode = mode; assert_eq!(remove("dir", NodeKind::Directory), expected); assert!(ROOTS.lock()[0].index.as_ref().unwrap().contains_key(b"dir".as_slice())); unchanged(); } }
#[test] fn missing_or_corrupt_physical_record_never_publishes() { for mode in [1, 2, 3, 4, 5] { reset(); match mode { 1 => STATE.lock().read_mode = 1, 2 => STATE.lock().read_mode = 2, 3 => STATE.lock().records.get_mut(&1).unwrap().kind = NodeKind::File, 4 => STATE.lock().records.get_mut(&1).unwrap().content_type = 99, _ => ROOTS.lock()[0].index.as_mut().unwrap().get_mut(b"dir".as_slice()).unwrap().kind = trueos_fs::LogKind::Delete } let error = if mode == 1 { block::Error::Io } else { block::Error::Corrupted }; assert_eq!(remove("dir", NodeKind::Directory), Err(RemoveNodeError::Device(error))); assert!(STATE.lock().calls.is_empty()); unchanged(); } }
#[test] fn cancelled_pending_storage_releases_lease_and_activity_without_publication() { reset(); STATE.lock().delete_mode = 3; let mut future = Box::pin(remove_leaf_async(block::DeviceHandle(1), "dir", NodeKind::Directory)); assert!(future.as_mut().poll(&mut Context::from_waker(Waker::noop())).is_pending()); assert_eq!(HELD.load(Ordering::SeqCst), 1); assert_eq!(ACTIVE.load(Ordering::SeqCst), 1); drop(future); assert!(ROOTS.lock()[0].index.as_ref().unwrap().contains_key(b"dir".as_slice())); unchanged(); }
#[test] fn successful_storage_with_lost_index_invalidates_it_instead_of_lying() { reset(); STATE.lock().update_failure = true; assert_eq!(remove("dir", NodeKind::Directory), Ok(true)); assert!(STATE.lock().invalidated); assert!(ROOTS.lock()[0].index.is_none()); assert_eq!(STATE.lock().watch, ["dir"]); }
#[test] fn existing_recursive_directory_behavior_is_preserved() { reset(); insert("dir/child", NodeKind::File, 2); insert("dir/sub", NodeKind::Directory, 3); insert("dir/sub/leaf", NodeKind::File, 4); insert("dir-other/keep", NodeKind::File, 5); assert_eq!(complete(remove_recursive_async(block::DeviceHandle(1), "dir")), Ok(true)); assert_eq!(STATE.lock().calls, [("tree", "dir".into())]); assert_eq!(ROOTS.lock()[0].index.as_ref().unwrap().keys().map(|k|String::from_utf8(k.clone()).unwrap()).collect::<Vec<_>>(), ["dir-other/keep"]); }
#[test] fn recursive_file_still_uses_node_deletion_and_false_is_not_new_leaf_error() { reset(); insert("dir/file", NodeKind::File, 2); assert_eq!(complete(remove_recursive_async(block::DeviceHandle(1), "dir/file")), Ok(true)); assert_eq!(STATE.lock().calls, [("node", "dir/file".into())]); reset(); STATE.lock().delete_mode = 2; assert_eq!(complete(remove_recursive_async(block::DeviceHandle(1), "dir")), Ok(false)); unchanged(); }
#[test] fn empty_check_reports_missing_index_and_mount_explicitly() { reset(); ROOTS.lock()[0].index = None; assert_eq!(indexed_directory_is_empty(1, "dir"), Err(block::Error::NotReady)); assert_eq!(indexed_directory_is_empty(2, "dir"), Err(block::Error::NotReady)); }
