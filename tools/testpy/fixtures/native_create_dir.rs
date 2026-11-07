use std::collections::BTreeMap;
use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::task::{Context, Poll, Waker};

struct Lock<T>(Mutex<T>);
impl<T> Lock<T> {
    const fn new(value: T) -> Self {
        Self(Mutex::new(value))
    }
    fn lock(&self) -> MutexGuard<'_, T> {
        self.0.lock().unwrap()
    }
}
mod block {
    pub type DiscId = u32;
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum Error {
        InvalidParam,
        Io,
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
#[derive(Clone, Copy)]
struct IndexRef {
    kind: trueos_fs::LogKind,
}
struct RootMount {
    disk_id: u32,
    index: Option<BTreeMap<Vec<u8>, IndexRef>>,
}
static ROOTS: Lock<Vec<RootMount>> = Lock::new(Vec::new());
#[derive(Default)]
struct State {
    action: u8,
    append_mode: u8,
    appends: usize,
    ensures: usize,
    published: Vec<String>,
    invalidated: bool,
}
static STATE: Lock<State> = Lock::new(State {
    action: 0,
    append_mode: 0,
    appends: 0,
    ensures: 0,
    published: Vec::new(),
    invalidated: false,
});
static HELD: AtomicUsize = AtomicUsize::new(0);
struct Placement {
    super_lba: u64,
    data_lba: u64,
    data_end_lba_exclusive: Option<u64>,
}
async fn placement_for_io_async(_: block::DeviceHandle) -> Result<Option<Placement>, block::Error> {
    Ok(Some(Placement {
        super_lba: 1,
        data_lba: 2,
        data_end_lba_exclusive: Some(100),
    }))
}
async fn ensure_index_async(_: block::DeviceHandle, _: &Placement) -> Result<(), block::Error> {
    assert_eq!(
        HELD.load(Ordering::SeqCst),
        0,
        "index rebuild must run outside publication lease"
    );
    STATE.lock().ensures += 1;
    let mut roots = ROOTS.lock();
    if roots[0].index.is_none() {
        roots[0].index = Some(BTreeMap::from([(
            b"parent".to_vec(),
            IndexRef {
                kind: trueos_fs::LogKind::Directory,
            },
        )]));
    }
    Ok(())
}
mod write_gate {
    pub struct RootWriteLease;
    impl RootWriteLease {
        pub async fn acquire(_: u32) -> Self {
            assert_eq!(crate::HELD.fetch_add(1, crate::Ordering::SeqCst), 0);
            let action = std::mem::take(&mut crate::STATE.lock().action);
            match action {
                1 => {
                    crate::ROOTS.lock()[0].index.as_mut().unwrap().insert(
                        b"parent/new".to_vec(),
                        crate::IndexRef {
                            kind: crate::trueos_fs::LogKind::Put,
                        },
                    );
                }
                2 => {
                    crate::ROOTS.lock()[0].index = None;
                }
                3 => {
                    crate::ROOTS.lock()[0]
                        .index
                        .as_mut()
                        .unwrap()
                        .remove(b"parent".as_slice());
                }
                _ => {}
            }
            Self
        }
    }
    impl Drop for RootWriteLease {
        fn drop(&mut self) {
            assert_eq!(crate::HELD.fetch_sub(1, crate::Ordering::SeqCst), 1);
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
    #[derive(Clone, Copy, PartialEq)]
    pub enum LogKind {
        Directory,
        Put,
    }
    pub struct FsParams {
        pub super_lba: u64,
        pub data_lba: u64,
        pub data_end_lba_exclusive: Option<u64>,
    }
    pub struct Record;
    pub async fn append_directory_prevalidated(
        _: &crate::KernelBlockIo,
        p: &FsParams,
        path: &str,
    ) -> Result<Option<Record>, crate::block::Error> {
        assert_eq!(
            (p.super_lba, p.data_lba, p.data_end_lba_exclusive),
            (1, 2, Some(100))
        );
        assert_eq!(crate::HELD.load(crate::Ordering::SeqCst), 1);
        let mode = crate::STATE.lock().append_mode;
        if mode == 3 {
            std::future::pending::<()>().await;
        }
        let mut s = crate::STATE.lock();
        s.appends += 1;
        match mode {
            1 => Err(crate::block::Error::Io),
            2 => Ok(None),
            _ => {
                s.published.push(path.into());
                Ok(Some(Record))
            }
        }
    }
}
fn map_engine_err(e: block::Error) -> block::Error {
    e
}
fn bump_root_cache_gen(_: u32) {
    assert_eq!(HELD.load(Ordering::SeqCst), 1);
}
fn update_root_index_node(_: u32, path: &str, _: trueos_fs::Record) -> bool {
    ROOTS.lock()[0].index.as_mut().unwrap().insert(
        path.as_bytes().to_vec(),
        IndexRef {
            kind: trueos_fs::LogKind::Directory,
        },
    );
    true
}
fn invalidate_root_index(_: u32) {
    STATE.lock().invalidated = true;
    ROOTS.lock()[0].index = None;
}

// @PRODUCTION@

fn reset() {
    assert_eq!(HELD.load(Ordering::SeqCst), 0);
    *STATE.lock() = State::default();
    *ROOTS.lock() = vec![RootMount {
        disk_id: 1,
        index: Some(BTreeMap::from([(
            b"parent".to_vec(),
            IndexRef {
                kind: trueos_fs::LogKind::Directory,
            },
        )])),
    }];
}
fn complete<F: Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("unexpected pending"),
    }
}
fn create(path: &str) -> Result<bool, CreateDirectoryError> {
    complete(create_directory_new_async(block::DeviceHandle(1), path))
}
#[test]
fn creates_only_requested_leaf_under_existing_parent() {
    reset();
    assert_eq!(create("parent/new"), Ok(true));
    assert_eq!(STATE.lock().published, ["parent/new"]);
    assert_eq!(HELD.load(Ordering::SeqCst), 0);
    assert_eq!(
        create("parent/new"),
        Err(CreateDirectoryError::AlreadyExists)
    );
    assert_eq!(STATE.lock().appends, 1);
}
#[test]
fn missing_parent_is_not_created() {
    reset();
    assert_eq!(
        create("absent/new"),
        Err(CreateDirectoryError::MissingParent)
    );
    assert!(STATE.lock().published.is_empty());
    assert_eq!(STATE.lock().appends, 0);
}
#[test]
fn file_parent_is_rejected_without_write() {
    reset();
    ROOTS.lock()[0].index.as_mut().unwrap().insert(
        b"parent".to_vec(),
        IndexRef {
            kind: trueos_fs::LogKind::Put,
        },
    );
    assert_eq!(
        create("parent/new"),
        Err(CreateDirectoryError::ParentNotDirectory)
    );
    assert_eq!(STATE.lock().appends, 0);
}
#[test]
fn root_and_file_destination_are_already_exists() {
    reset();
    assert_eq!(create(""), Err(CreateDirectoryError::AlreadyExists));
    assert_eq!(create("/"), Err(CreateDirectoryError::AlreadyExists));
    ROOTS.lock()[0].index.as_mut().unwrap().insert(
        b"file".to_vec(),
        IndexRef {
            kind: trueos_fs::LogKind::Put,
        },
    );
    assert_eq!(create("file"), Err(CreateDirectoryError::AlreadyExists));
    assert_eq!(STATE.lock().appends, 0);
}
#[test]
fn occupied_destination_race_is_checked_after_lease() {
    reset();
    STATE.lock().action = 1;
    assert_eq!(
        create("parent/new"),
        Err(CreateDirectoryError::AlreadyExists)
    );
    assert_eq!(STATE.lock().appends, 0);
    assert_eq!(HELD.load(Ordering::SeqCst), 0);
}
#[test]
fn disappearing_parent_race_is_checked_after_lease() {
    reset();
    STATE.lock().action = 3;
    assert_eq!(
        create("parent/new"),
        Err(CreateDirectoryError::MissingParent)
    );
    assert_eq!(STATE.lock().appends, 0);
}
#[test]
fn invalidated_index_is_rebuilt_outside_lease() {
    reset();
    STATE.lock().action = 2;
    assert_eq!(create("parent/new"), Ok(true));
    assert_eq!(STATE.lock().ensures, 2);
    assert_eq!(STATE.lock().appends, 1);
}
#[test]
fn storage_failure_and_full_volume_do_not_publish_index() {
    for (mode, result) in [
        (1, Err(CreateDirectoryError::Device(block::Error::Io))),
        (2, Ok(false)),
    ] {
        reset();
        STATE.lock().append_mode = mode;
        assert_eq!(create("parent/new"), result);
        assert!(
            !ROOTS.lock()[0]
                .index
                .as_ref()
                .unwrap()
                .contains_key(b"parent/new".as_slice())
        );
        assert!(STATE.lock().published.is_empty());
        assert_eq!(HELD.load(Ordering::SeqCst), 0);
    }
}
#[test]
fn cancelled_pending_io_releases_publication_lease() {
    reset();
    STATE.lock().append_mode = 3;
    let mut f = Box::pin(create_directory_new_async(
        block::DeviceHandle(1),
        "parent/new",
    ));
    assert!(
        f.as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    assert_eq!(HELD.load(Ordering::SeqCst), 1);
    drop(f);
    assert_eq!(HELD.load(Ordering::SeqCst), 0);
    assert!(STATE.lock().published.is_empty());
}
#[test]
fn partition_handle_is_rejected_without_io() {
    reset();
    assert_eq!(
        complete(create_directory_new_async(block::DeviceHandle(9), "new")),
        Err(CreateDirectoryError::Device(block::Error::InvalidParam))
    );
    assert_eq!(STATE.lock().ensures, 0);
}
