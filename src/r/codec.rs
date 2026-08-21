extern crate alloc;

use alloc::collections::{BTreeSet, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use trueos_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

const CODEC_IDLE_MS: u64 = 25;
const REQUEST_CAP: usize = 32;
const OPERATION_CAP: usize = 64;

const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
const MAX_SOURCE_FILE_BYTES: usize = 16 * 1024 * 1024;
const MAX_SOURCE_TOTAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 4_096;
const MAX_ARCHIVE_DICTIONARY_BYTES: usize = 16 * 1024 * 1024;
const MAX_ARCHIVE_PATH_BYTES: usize = 1_024;
const MAX_ARCHIVE_PATH_DEPTH: usize = 64;

pub const OPERATION_PENDING: i32 = 0;
pub const OPERATION_READY: i32 = 1;
pub const OPERATION_NOT_FOUND: i32 = -1;
pub const OPERATION_FAILED: i32 = -2;

static NEXT_OPERATION_ID: AtomicU32 = AtomicU32::new(1);
static REQUESTS: Mutex<VecDeque<CodecRequest>> = Mutex::new(VecDeque::new());
static OPERATIONS: Mutex<Vec<OperationRecord>> = Mutex::new(Vec::new());

#[derive(Clone)]
enum CodecRequest {
    SevenZPackPath {
        owner: u32,
        id: u32,
        source_path: String,
        archive_path: String,
    },
    SevenZUnpackPath {
        owner: u32,
        id: u32,
        archive_path: String,
        output_path: String,
    },
}

impl CodecRequest {
    fn operation_key(&self) -> (u32, u32) {
        match self {
            Self::SevenZPackPath { owner, id, .. } | Self::SevenZUnpackPath { owner, id, .. } => {
                (*owner, *id)
            }
        }
    }
}

#[derive(Clone)]
enum OperationState {
    Queued,
    Running,
    Complete(Result<CodecReport, CodecError>),
}

#[derive(Clone)]
struct OperationRecord {
    owner: u32,
    id: u32,
    state: OperationState,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CodecReport {
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub file_count: u32,
}

#[derive(Clone, Debug)]
pub enum CodecError {
    NoRoot,
    BadPath,
    NotFound,
    NotReady,
    QueueFull,
    LimitExceeded,
    PathConflict,
    ReadFailed,
    WriteFailed,
    Archive(crate::z7::SevenZError),
    Fs(crate::disc::block::Error),
}

impl core::fmt::Display for CodecError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoRoot => f.write_str("no TRUEOSFS root"),
            Self::BadPath => f.write_str("bad path"),
            Self::NotFound => f.write_str("not found"),
            Self::NotReady => f.write_str("operation not ready"),
            Self::QueueFull => f.write_str("codec queue full"),
            Self::LimitExceeded => f.write_str("codec resource limit exceeded"),
            Self::PathConflict => f.write_str("archive path conflict"),
            Self::ReadFailed => f.write_str("read failed"),
            Self::WriteFailed => f.write_str("write failed"),
            Self::Archive(err) => write!(f, "archive: {:?}", err),
            Self::Fs(err) => write!(f, "fs: {:?}", err),
        }
    }
}

impl From<crate::disc::block::Error> for CodecError {
    fn from(value: crate::disc::block::Error) -> Self {
        Self::Fs(value)
    }
}

impl From<crate::z7::SevenZError> for CodecError {
    fn from(value: crate::z7::SevenZError) -> Self {
        Self::Archive(value)
    }
}

fn next_operation_id(operations: &[OperationRecord]) -> Result<u32, CodecError> {
    for _ in 0..=OPERATION_CAP {
        // The C ABI returns start results as i32, so keep successful handles
        // strictly positive even after the atomic sequence crosses bit 31.
        let id = NEXT_OPERATION_ID.fetch_add(1, Ordering::Relaxed) & i32::MAX as u32;
        if id != 0 && operations.iter().all(|operation| operation.id != id) {
            return Ok(id);
        }
    }
    Err(CodecError::QueueFull)
}

fn enqueue_operation(
    owner: u32,
    request: impl FnOnce(u32) -> CodecRequest,
) -> Result<u32, CodecError> {
    let mut requests = REQUESTS.lock();
    if requests.len() >= REQUEST_CAP {
        return Err(CodecError::QueueFull);
    }

    let mut operations = OPERATIONS.lock();
    if operations.len() >= OPERATION_CAP {
        return Err(CodecError::QueueFull);
    }
    let id = next_operation_id(operations.as_slice())?;
    operations.push(OperationRecord {
        owner,
        id,
        state: OperationState::Queued,
    });
    requests.push_back(request(id));
    Ok(id)
}

fn mark_operation_running(owner: u32, id: u32) -> bool {
    let mut operations = OPERATIONS.lock();
    let Some(operation) = operations
        .iter_mut()
        .find(|operation| operation.owner == owner && operation.id == id)
    else {
        return false;
    };
    if !matches!(operation.state, OperationState::Queued) {
        return false;
    }
    operation.state = OperationState::Running;
    true
}

fn complete_operation(owner: u32, id: u32, result: Result<CodecReport, CodecError>) {
    let mut operations = OPERATIONS.lock();
    if let Some(operation) = operations
        .iter_mut()
        .find(|operation| operation.owner == owner && operation.id == id)
    {
        operation.state = OperationState::Complete(result);
    }
}

pub fn enqueue_7z_pack(
    owner: u32,
    source_path: String,
    archive_path: String,
) -> Result<u32, CodecError> {
    let source_path = normalize_path(source_path.as_str(), false)?;
    let archive_path = normalize_path(archive_path.as_str(), false)?;
    if source_path == archive_path {
        return Err(CodecError::BadPath);
    }
    enqueue_operation(owner, |id| CodecRequest::SevenZPackPath {
        owner,
        id,
        source_path,
        archive_path,
    })
}

pub fn enqueue_7z_unpack(
    owner: u32,
    archive_path: String,
    output_path: String,
) -> Result<u32, CodecError> {
    let archive_path = normalize_path(archive_path.as_str(), false)?;
    let output_path = normalize_path(output_path.as_str(), false)?;
    if archive_path == output_path {
        return Err(CodecError::BadPath);
    }
    enqueue_operation(owner, |id| CodecRequest::SevenZUnpackPath {
        owner,
        id,
        archive_path,
        output_path,
    })
}

pub fn operation_status(owner: u32, id: u32) -> i32 {
    let operations = OPERATIONS.lock();
    let Some(operation) = operations
        .iter()
        .find(|operation| operation.owner == owner && operation.id == id)
    else {
        return OPERATION_NOT_FOUND;
    };
    match &operation.state {
        OperationState::Queued | OperationState::Running => OPERATION_PENDING,
        OperationState::Complete(Ok(_)) => OPERATION_READY,
        OperationState::Complete(Err(_)) => OPERATION_FAILED,
    }
}

/// Return a retained result. The caller must explicitly discard the operation.
pub fn operation_report(owner: u32, id: u32) -> Result<CodecReport, CodecError> {
    let operations = OPERATIONS.lock();
    let operation = operations
        .iter()
        .find(|operation| operation.owner == owner && operation.id == id)
        .ok_or(CodecError::NotFound)?;
    match &operation.state {
        OperationState::Queued | OperationState::Running => Err(CodecError::NotReady),
        OperationState::Complete(result) => result.clone(),
    }
}

pub fn discard_operation(owner: u32, id: u32) -> i32 {
    let mut requests = REQUESTS.lock();
    let mut operations = OPERATIONS.lock();
    let Some(index) = operations
        .iter()
        .position(|operation| operation.owner == owner && operation.id == id)
    else {
        return OPERATION_NOT_FOUND;
    };
    operations.swap_remove(index);
    requests.retain(|request| request.operation_key() != (owner, id));
    0
}

fn normalize_path(path: &str, allow_empty: bool) -> Result<String, CodecError> {
    crate::r::path::FsPath::parse(path, allow_empty)
        .map(|path| path.to_relative_string())
        .map_err(|_| CodecError::BadPath)
}

fn validate_archive_entry_name(name: &str) -> Result<String, CodecError> {
    if name.is_empty()
        || name.len() > MAX_ARCHIVE_PATH_BYTES
        || name.starts_with('/')
        || name.starts_with('\\')
        || name.contains('\\')
        || name.split('/').count() > MAX_ARCHIVE_PATH_DEPTH
    {
        return Err(CodecError::BadPath);
    }
    let normalized = normalize_path(name, false)?;
    if normalized != name {
        return Err(CodecError::BadPath);
    }
    Ok(normalized)
}

fn output_path_for_archive_entry(
    output_root: &str,
    entry_name: &str,
) -> Result<String, CodecError> {
    let entry = validate_archive_entry_name(entry_name)?;
    let mut out = String::from(output_root);
    if !out.is_empty() {
        out.push('/');
    }
    out.push_str(entry.as_str());
    normalize_path(out.as_str(), false)
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn parent_path(path: &str) -> Option<&str> {
    path.rsplit_once('/')
        .map(|(parent, _)| parent)
        .filter(|parent| !parent.is_empty())
}

fn dequeue_request() -> Option<CodecRequest> {
    REQUESTS.lock().pop_front()
}

struct OwnedSourceEntry {
    name: String,
    bytes: Vec<u8>,
}

fn checked_file_size(size: u64, total: &mut usize) -> Result<usize, CodecError> {
    let size = usize::try_from(size).map_err(|_| CodecError::LimitExceeded)?;
    if size > MAX_SOURCE_FILE_BYTES {
        return Err(CodecError::LimitExceeded);
    }
    *total = total.checked_add(size).ok_or(CodecError::LimitExceeded)?;
    if *total > MAX_SOURCE_TOTAL_BYTES {
        return Err(CodecError::LimitExceeded);
    }
    Ok(size)
}

async fn read_source_file(
    disk: crate::disc::block::DeviceHandle,
    path: &str,
    archive_name: String,
    total: &mut usize,
) -> Result<OwnedSourceEntry, CodecError> {
    let info = crate::r::fs::trueosfs::file_info_async(disk, path)
        .await?
        .ok_or(CodecError::NotFound)?;
    let expected = checked_file_size(info.data_len, total)?;
    let bytes = crate::r::fs::trueosfs::file_out_async(disk, path)
        .await?
        .ok_or(CodecError::ReadFailed)?;
    if bytes.len() != expected {
        return Err(CodecError::ReadFailed);
    }
    Ok(OwnedSourceEntry {
        name: archive_name,
        bytes,
    })
}

async fn collect_source_entries(
    disk: crate::disc::block::DeviceHandle,
    source_path: &str,
    excluded_path: &str,
) -> Result<Vec<OwnedSourceEntry>, CodecError> {
    let mut total = 0usize;
    if crate::r::fs::trueosfs::file_info_async(disk, source_path)
        .await?
        .is_some()
    {
        return Ok(alloc::vec![
            read_source_file(
                disk,
                source_path,
                validate_archive_entry_name(basename(source_path))?,
                &mut total,
            )
            .await?
        ]);
    }
    if !crate::r::fs::trueosfs::dir_has_children_async(disk, source_path).await? {
        return Err(CodecError::NotFound);
    }

    let mut directories = VecDeque::new();
    directories.push_back(String::from(source_path));
    let mut discovered_directories = 1usize;
    let mut entries = Vec::new();
    while let Some(directory) = directories.pop_front() {
        let listing = crate::r::fs::trueosfs::list_dir_async(disk, directory.as_str())
            .await?
            .ok_or(CodecError::NoRoot)?;
        for child in listing.lines() {
            if child == "..." {
                return Err(CodecError::LimitExceeded);
            }
            if child.is_empty() || child == ".keep" {
                continue;
            }
            let mut full_path = directory.clone();
            full_path.push('/');
            full_path.push_str(child);
            let full_path = normalize_path(full_path.as_str(), false)?;
            if full_path == excluded_path {
                continue;
            }
            let relative = full_path
                .strip_prefix(source_path)
                .and_then(|path| path.strip_prefix('/'))
                .ok_or(CodecError::BadPath)?;
            let relative = validate_archive_entry_name(relative)?;
            if crate::r::fs::trueosfs::file_info_async(disk, full_path.as_str())
                .await?
                .is_some()
            {
                if entries.len() >= MAX_ARCHIVE_ENTRIES {
                    return Err(CodecError::LimitExceeded);
                }
                entries
                    .push(read_source_file(disk, full_path.as_str(), relative, &mut total).await?);
            } else if crate::r::fs::trueosfs::dir_has_children_async(disk, full_path.as_str())
                .await?
            {
                discovered_directories = discovered_directories
                    .checked_add(1)
                    .ok_or(CodecError::LimitExceeded)?;
                if discovered_directories > MAX_ARCHIVE_ENTRIES {
                    return Err(CodecError::LimitExceeded);
                }
                directories.push_back(full_path);
            }
        }
    }
    if entries.is_empty() {
        return Err(CodecError::NotFound);
    }
    entries.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(entries)
}

async fn pack_path_job(source_path: &str, archive_path: &str) -> Result<CodecReport, CodecError> {
    let disk = crate::r::fs::trueosfs::primary_root_handle().ok_or(CodecError::NoRoot)?;
    let entries = collect_source_entries(disk, source_path, archive_path).await?;
    let source_bytes = entries.iter().try_fold(0u64, |total, entry| {
        total
            .checked_add(entry.bytes.len() as u64)
            .ok_or(CodecError::LimitExceeded)
    })?;
    let sources: Vec<crate::z7::SevenZSourceEntry<'_>> = entries
        .iter()
        .map(|entry| crate::z7::SevenZSourceEntry {
            name: entry.name.as_str(),
            bytes: entry.bytes.as_slice(),
        })
        .collect();
    let archive = crate::z7::compress_files_to_vec(sources.as_slice())?;
    if archive.len() > MAX_ARCHIVE_BYTES {
        return Err(CodecError::LimitExceeded);
    }
    if let Some(parent) = parent_path(archive_path)
        && !crate::r::fs::trueosfs::dir_create_all_async(disk, parent).await?
    {
        return Err(CodecError::WriteFailed);
    }
    if !crate::r::fs::trueosfs::file_write_all_async(disk, archive_path, archive.as_slice()).await?
    {
        return Err(CodecError::WriteFailed);
    }
    Ok(CodecReport {
        input_bytes: source_bytes,
        output_bytes: archive.len() as u64,
        file_count: u32::try_from(entries.len()).map_err(|_| CodecError::LimitExceeded)?,
    })
}

fn validate_entry_set(entries: &[crate::z7::SevenZEntry]) -> Result<Vec<String>, CodecError> {
    let mut path_set = BTreeSet::new();
    let mut paths = Vec::new();
    paths
        .try_reserve_exact(entries.len())
        .map_err(|_| CodecError::LimitExceeded)?;
    for entry in entries {
        let path = validate_archive_entry_name(entry.name.as_str())?;
        if !path_set.insert(path.clone()) {
            return Err(CodecError::PathConflict);
        }
        paths.push(path);
    }
    for path in &path_set {
        let mut prefix = String::new();
        for component in path.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
            if prefix != *path && path_set.contains(prefix.as_str()) {
                return Err(CodecError::PathConflict);
            }
        }
    }
    Ok(paths)
}

async fn unpack_path_job(archive_path: &str, output_path: &str) -> Result<CodecReport, CodecError> {
    let disk = crate::r::fs::trueosfs::primary_root_handle().ok_or(CodecError::NoRoot)?;
    let info = crate::r::fs::trueosfs::file_info_async(disk, archive_path)
        .await?
        .ok_or(CodecError::NotFound)?;
    if info.data_len > MAX_ARCHIVE_BYTES as u64 {
        return Err(CodecError::LimitExceeded);
    }
    let archive = crate::r::fs::trueosfs::file_out_async(disk, archive_path)
        .await?
        .ok_or(CodecError::ReadFailed)?;
    if archive.len() > MAX_ARCHIVE_BYTES {
        return Err(CodecError::LimitExceeded);
    }
    let entries = crate::z7::extract_all_to_vec_bounded(
        archive.as_slice(),
        MAX_ARCHIVE_ENTRIES,
        MAX_SOURCE_FILE_BYTES,
        MAX_SOURCE_TOTAL_BYTES,
        MAX_ARCHIVE_DICTIONARY_BYTES,
    )?;
    let validated = validate_entry_set(entries.as_slice())?;

    let mut destinations = Vec::new();
    destinations
        .try_reserve_exact(entries.len())
        .map_err(|_| CodecError::LimitExceeded)?;
    for path in &validated {
        destinations.push(output_path_for_archive_entry(output_path, path.as_str())?);
    }
    if destinations
        .iter()
        .any(|destination| destination == archive_path)
    {
        return Err(CodecError::PathConflict);
    }

    if !crate::r::fs::trueosfs::dir_create_all_async(disk, output_path).await? {
        return Err(CodecError::WriteFailed);
    }

    let mut output_bytes = 0u64;
    for (entry, destination) in entries.iter().zip(destinations.iter()) {
        if let Some(parent) = parent_path(destination.as_str())
            && !crate::r::fs::trueosfs::dir_create_all_async(disk, parent).await?
        {
            return Err(CodecError::WriteFailed);
        }
        if !crate::r::fs::trueosfs::file_write_all_async(
            disk,
            destination.as_str(),
            entry.bytes.as_slice(),
        )
        .await?
        {
            return Err(CodecError::WriteFailed);
        }
        output_bytes = output_bytes
            .checked_add(entry.bytes.len() as u64)
            .ok_or(CodecError::LimitExceeded)?;
    }

    Ok(CodecReport {
        input_bytes: archive.len() as u64,
        output_bytes,
        file_count: u32::try_from(entries.len()).map_err(|_| CodecError::LimitExceeded)?,
    })
}

async fn execute_request(request: CodecRequest) {
    let (owner, id) = request.operation_key();
    if !mark_operation_running(owner, id) {
        return;
    }
    let result = match request {
        CodecRequest::SevenZPackPath {
            source_path,
            archive_path,
            ..
        } => pack_path_job(source_path.as_str(), archive_path.as_str()).await,
        CodecRequest::SevenZUnpackPath {
            archive_path,
            output_path,
            ..
        } => unpack_path_job(archive_path.as_str(), output_path.as_str()).await,
    };
    complete_operation(owner, id, result);
}

#[trueos_executor::task(pool_size = 3)]
pub async fn codec_worker_task(worker_id: usize, worker_slot: u32, core_kind: u8) {
    crate::log_info!(
        target: "service";
        "codec: worker={} online archive=7z pool=3 worker_slot={} core_kind={}\n",
        worker_id,
        worker_slot,
        core_kind
    );
    loop {
        match dequeue_request() {
            Some(request) => execute_request(request).await,
            None => Timer::after(EmbassyDuration::from_millis(CODEC_IDLE_MS)).await,
        }
    }
}
