//! Runs the real local orchestration with deterministic I/O fault injection.
//! The mock stores files as bytes; it does not model TRUEOSFS disk layout.
#![cfg(test)]
extern crate alloc;
extern crate self as trueos_time;
pub use std::time::Duration;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicU64, Ordering},
    sync::{LazyLock, Mutex},
};
static CLOCK: AtomicU64 = AtomicU64::new(0);
pub struct Instant(u64);
impl Instant {
    pub fn now() -> Self {
        Self(CLOCK.fetch_add(100, Ordering::SeqCst))
    }
    pub fn as_millis(&self) -> u64 {
        self.0
    }
}
pub struct Timer;
impl Timer {
    pub async fn after(_: Duration) {}
}
mod tyche {
    pub fn fill_bytes(bytes: &mut [u8]) -> bool {
        bytes.fill(7);
        true
    }
}
#[derive(Default)]
struct Store {
    disks: BTreeMap<u32, Vec<u8>>,
    files: BTreeMap<(u32, String), Vec<u8>>,
    streams: BTreeMap<u32, (u32, String, Vec<u8>, usize)>,
    handles: Vec<(u32, Vec<u8>)>,
    mounted: BTreeSet<u32>,
    held: BTreeSet<u32>,
    discarded: BTreeSet<u32>,
    reprobed: BTreeSet<u32>,
    writes: usize,
    fail_first_write: bool,
    next_stream: u32,
}
static STORE: LazyLock<Mutex<Store>> = LazyLock::new(|| Mutex::new(Store::default()));
static TEST_LOCK: Mutex<()> = Mutex::new(());
mod disc {
    pub mod block {
        use crate::STORE;
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub struct DiscId(pub u32);
        impl DiscId {
            pub fn raw(self) -> u32 {
                self.0
            }
        }
        #[derive(Clone, Copy, Debug)]
        pub struct DeviceHandle(pub u32);
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum Error {
            InvalidParam,
            NotReady,
            Corrupted,
            Io,
        }
        pub type Result<T> = core::result::Result<T, Error>;
        pub struct Info {
            pub label: Option<String>,
        }
        impl DeviceHandle {
            pub fn id(self) -> DiscId {
                DiscId(self.0)
            }
            pub fn parent(self) -> Option<DiscId> {
                None
            }
            pub fn block_size(self) -> u32 {
                512
            }
            pub fn block_count(self) -> u64 {
                STORE.lock().unwrap().disks[&self.0].len() as u64 / 512
            }
            pub fn max_transfer_bytes(self) -> u64 {
                1024
            }
            pub fn supports_write(self) -> bool {
                true
            }
            pub fn info(self) -> Info {
                Info {
                    label: Some(format!("disk{}", self.0)),
                }
            }
            pub async fn flush(self) -> Result<()> {
                Ok(())
            }
        }
        pub struct BackupLease {
            pub disk: DeviceHandle,
        }
        impl BackupLease {
            pub fn try_acquire(disk: DeviceHandle) -> Result<Self> {
                if !STORE.lock().unwrap().held.insert(disk.0) {
                    return Err(Error::NotReady);
                }
                Ok(Self { disk })
            }
            pub async fn flush(&self) -> Result<()> {
                Ok(())
            }
            pub async fn read(&self, lba: u64, blocks: usize) -> Result<Vec<u8>> {
                let s = STORE.lock().unwrap();
                let start = lba as usize * 512;
                Ok(s.disks[&self.disk.0][start..start + blocks * 512].to_vec())
            }
        }
        impl Drop for BackupLease {
            fn drop(&mut self) {
                STORE.lock().unwrap().held.remove(&self.disk.0);
            }
        }
        pub struct RawRestoreLease(BackupLease);
        impl RawRestoreLease {
            pub fn try_acquire(disk: DeviceHandle) -> Result<Self> {
                Ok(Self(BackupLease::try_acquire(disk)?))
            }
            pub async fn flush(&self) -> Result<()> {
                Ok(())
            }
            pub async fn write(&self, lba: u64, data: &[u8]) -> Result<()> {
                let mut s = STORE.lock().unwrap();
                s.writes += 1;
                let start = lba as usize * 512;
                if s.fail_first_write && s.writes == 1 {
                    // Failed I/O may already have modified a prefix.
                    s.disks.get_mut(&self.0.disk.0).unwrap()[start] = data[0];
                    return Err(Error::Io);
                }
                s.disks.get_mut(&self.0.disk.0).unwrap()[start..start + data.len()]
                    .copy_from_slice(data);
                Ok(())
            }
        }
    }
    #[path = "@ENGINE@"]
    pub mod backup_local;
}
mod r {
    pub mod fs {
        pub mod trueosfs {
            use crate::{
                STORE,
                disc::block::{BackupLease, DeviceHandle, DiscId, Result},
            };
            #[derive(PartialEq)]
            pub enum NodeKind {
                File,
            }
            pub struct Entry {
                pub name: String,
                pub kind: NodeKind,
            }
            pub struct Listing {
                pub entries: Vec<Entry>,
            }
            pub struct Root {
                pub disk_id: DiscId,
            }
            pub struct FileInfo {
                pub data_len: u64,
            }
            #[derive(Clone, Copy)]
            pub struct FileReadHandle {
                index: usize,
                len: u64,
            }
            impl FileReadHandle {
                pub fn data_len(self) -> u64 {
                    self.len
                }
            }
            pub fn list_roots() -> Vec<Root> {
                STORE
                    .lock()
                    .unwrap()
                    .mounted
                    .iter()
                    .map(|&id| Root {
                        disk_id: DiscId(id),
                    })
                    .collect()
            }
            pub async fn available_backup_bytes_async(_: DeviceHandle) -> Result<Option<u64>> {
                Ok(Some(1 << 20))
            }
            pub async fn dir_create_all_async(_: DeviceHandle, _: &str) -> Result<bool> {
                Ok(true)
            }
            pub async fn list_dir_async(root: DeviceHandle, _: &str) -> Result<Option<Listing>> {
                Ok(Some(Listing {
                    entries: STORE
                        .lock()
                        .unwrap()
                        .files
                        .keys()
                        .filter(|(id, _)| *id == root.0)
                        .map(|(_, path)| Entry {
                            name: path.strip_prefix("backups/").unwrap().into(),
                            kind: NodeKind::File,
                        })
                        .collect(),
                }))
            }
            pub async fn file_info_async(
                root: DeviceHandle,
                path: &str,
            ) -> Result<Option<FileInfo>> {
                Ok(STORE
                    .lock()
                    .unwrap()
                    .files
                    .get(&(root.0, path.into()))
                    .map(|v| FileInfo {
                        data_len: v.len() as u64,
                    }))
            }
            pub async fn file_out_async(root: DeviceHandle, path: &str) -> Result<Option<Vec<u8>>> {
                Ok(STORE
                    .lock()
                    .unwrap()
                    .files
                    .get(&(root.0, path.into()))
                    .cloned())
            }
            pub async fn file_write_begin_async(
                root: DeviceHandle,
                path: &str,
                len: u64,
            ) -> Result<Option<u32>> {
                let mut s = STORE.lock().unwrap();
                s.next_stream += 1;
                let id = s.next_stream;
                s.streams
                    .insert(id, (root.0, path.into(), Vec::new(), len as usize));
                Ok(Some(id))
            }
            pub async fn file_write_chunk_async(id: u32, data: &[u8]) -> Result<()> {
                STORE
                    .lock()
                    .unwrap()
                    .streams
                    .get_mut(&id)
                    .unwrap()
                    .2
                    .extend_from_slice(data);
                Ok(())
            }
            pub async fn file_write_finish_async(id: u32) -> Result<()> {
                let mut s = STORE.lock().unwrap();
                let (root, path, bytes, len) = s.streams.remove(&id).unwrap();
                assert_eq!(bytes.len(), len);
                s.files.insert((root, path), bytes);
                Ok(())
            }
            pub async fn file_write_abort_async(id: u32) -> Result<()> {
                STORE.lock().unwrap().streams.remove(&id);
                Ok(())
            }
            pub async fn file_write_all_async(
                root: DeviceHandle,
                path: &str,
                data: &[u8],
            ) -> Result<bool> {
                STORE
                    .lock()
                    .unwrap()
                    .files
                    .insert((root.0, path.into()), data.to_vec());
                Ok(true)
            }
            pub async fn file_read_open_async(
                root: DeviceHandle,
                path: &str,
            ) -> Result<Option<FileReadHandle>> {
                let mut s = STORE.lock().unwrap();
                let Some(data) = s.files.get(&(root.0, path.into())).cloned() else {
                    return Ok(None);
                };
                let handle = FileReadHandle {
                    index: s.handles.len(),
                    len: data.len() as u64,
                };
                s.handles.push((root.0, data));
                Ok(Some(handle))
            }
            pub fn file_read_handle_is_current(_: FileReadHandle) -> bool {
                true
            }
            pub async fn file_read_handle_range_backup_lease_async(
                handle: FileReadHandle,
                lease: &BackupLease,
                offset: u64,
                out: &mut [u8],
            ) -> Result<Option<usize>> {
                let s = STORE.lock().unwrap();
                let (root, data) = &s.handles[handle.index];
                assert_eq!(*root, lease.disk.0);
                assert!(s.held.contains(root));
                let start = offset as usize;
                out.copy_from_slice(&data[start..start + out.len()]);
                Ok(Some(out.len()))
            }
            pub struct BackupMount {
                id: u32,
                restore: bool,
            }
            pub fn suspend_backup_mount(id: DiscId) -> BackupMount {
                let mut s = STORE.lock().unwrap();
                assert!(s.held.contains(&id.0));
                let restore = s.mounted.remove(&id.0);
                BackupMount { id: id.0, restore }
            }
            impl BackupMount {
                pub fn discard_after_raw_overwrite(&mut self) {
                    self.restore = false;
                    STORE.lock().unwrap().discarded.insert(self.id);
                }
            }
            impl Drop for BackupMount {
                fn drop(&mut self) {
                    let mut s = STORE.lock().unwrap();
                    assert!(s.held.contains(&self.id));
                    if self.restore {
                        s.mounted.insert(self.id);
                    }
                }
            }
            pub fn request_mount_root(root: DeviceHandle) {
                let mut s = STORE.lock().unwrap();
                assert!(!s.held.contains(&root.0));
                s.reprobed.insert(root.0);
            }
        }
    }
}
fn run<F: std::future::Future>(future: F) -> F::Output {
    use std::{
        pin::pin,
        sync::Arc,
        task::{Context, Poll, Wake, Waker},
    };
    struct Noop;
    impl Wake for Noop {
        fn wake(self: Arc<Self>) {}
    }
    let waker = Waker::from(Arc::new(Noop));
    let mut context = Context::from_waker(&waker);
    let mut future = pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(out) => return out,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}
fn setup() -> (disc::block::DeviceHandle, disc::block::DeviceHandle, disc::block::DeviceHandle) {
    let mut s = STORE.lock().unwrap();
    *s = Store::default();
    s.disks
        .insert(1, (0..4096).map(|i| (i % 251) as u8).collect());
    s.disks.insert(2, vec![0; 32768]);
    s.disks.insert(3, vec![0xCC; 4096]);
    s.mounted.extend([1, 2, 3]);
    (disc::block::DeviceHandle(1), disc::block::DeviceHandle(2), disc::block::DeviceHandle(3))
}
#[test]
fn create_catalog_restore_full_disk() {
    let _test = TEST_LOCK.lock().unwrap();
    let (source, root, target) = setup();
    let created =
        run(disc::backup_local::create_local_backup_async(source, root, &mut |_| true)).unwrap();
    let listed = run(disc::backup_local::list_local_backups_async(root)).unwrap();
    assert_eq!(listed, vec![created.clone()]);
    run(disc::backup_local::restore_local_backup_async(root, &listed[0], target, &mut |_| true))
        .unwrap();
    let s = STORE.lock().unwrap();
    assert_eq!(s.disks[&1], s.disks[&3]);
    assert!(s.held.is_empty());
    assert!(s.discarded.contains(&3));
    assert!(s.reprobed.contains(&3));
    assert!(s.mounted.contains(&2));
}
#[test]
fn corrupt_image_never_writes_target() {
    let _test = TEST_LOCK.lock().unwrap();
    let (source, root, target) = setup();
    let artifact =
        run(disc::backup_local::create_local_backup_async(source, root, &mut |_| true)).unwrap();
    STORE
        .lock()
        .unwrap()
        .files
        .get_mut(&(2, artifact.image_path.clone()))
        .unwrap()[10] ^= 1;
    assert!(
        run(disc::backup_local::restore_local_backup_async(root, &artifact, target, &mut |_| true))
            .is_err()
    );
    let s = STORE.lock().unwrap();
    assert_eq!(s.writes, 0);
    assert!(s.held.is_empty());
    assert!(s.mounted.contains(&2));
    assert!(s.mounted.contains(&3));
}
#[test]
fn failed_first_write_discards_old_mount_and_reprobes() {
    let _test = TEST_LOCK.lock().unwrap();
    let (source, root, target) = setup();
    let artifact =
        run(disc::backup_local::create_local_backup_async(source, root, &mut |_| true)).unwrap();
    STORE.lock().unwrap().fail_first_write = true;
    assert!(
        run(disc::backup_local::restore_local_backup_async(root, &artifact, target, &mut |_| true))
            .is_err()
    );
    let s = STORE.lock().unwrap();
    assert_eq!(s.writes, 1);
    assert!(s.discarded.contains(&3));
    assert!(s.reprobed.contains(&3));
    assert!(!s.mounted.contains(&3));
    assert!(s.mounted.contains(&2));
    assert!(s.held.is_empty());
}
#[test]
fn cancellation_before_writing_keeps_mounts_and_releases_claims() {
    use disc::backup_local::ProgressPhase;
    let _test = TEST_LOCK.lock().unwrap();
    let (source, root, target) = setup();
    let artifact =
        run(disc::backup_local::create_local_backup_async(source, root, &mut |_| true)).unwrap();
    assert!(
        run(disc::backup_local::restore_local_backup_async(root, &artifact, target, &mut |p| p
            .phase
            != ProgressPhase::Restoring))
        .is_err()
    );
    let s = STORE.lock().unwrap();
    assert_eq!(s.writes, 0);
    assert!(s.discarded.is_empty());
    assert!(s.held.is_empty());
    assert_eq!(s.mounted, BTreeSet::from([1, 2, 3]));
}
#[test]
fn replaced_manifest_cannot_change_the_confirmed_selection() {
    let _test = TEST_LOCK.lock().unwrap();
    let (source, root, target) = setup();
    let artifact =
        run(disc::backup_local::create_local_backup_async(source, root, &mut |_| true)).unwrap();
    STORE
        .lock()
        .unwrap()
        .files
        .get_mut(&(2, artifact.manifest_path.clone()))
        .unwrap()[64] ^= 1;
    assert!(
        run(disc::backup_local::restore_local_backup_async(root, &artifact, target, &mut |_| true))
            .is_err()
    );
    let s = STORE.lock().unwrap();
    assert_eq!(s.writes, 0);
    assert!(s.held.is_empty());
    assert!(s.discarded.is_empty());
}
#[test]
fn busy_source_leaves_no_stream_or_incomplete_catalog_entry() {
    let _test = TEST_LOCK.lock().unwrap();
    let (source, root, _) = setup();
    let busy = disc::block::BackupLease::try_acquire(source).unwrap();
    assert_eq!(
        run(disc::backup_local::create_local_backup_async(source, root, &mut |_| true)),
        Err(disc::backup_local::Error::Busy)
    );
    drop(busy);
    assert!(
        run(disc::backup_local::list_local_backups_async(root))
            .unwrap()
            .is_empty()
    );
    let s = STORE.lock().unwrap();
    assert!(s.streams.is_empty());
    assert!(s.held.is_empty());
    assert!(s.mounted.contains(&1));
}
