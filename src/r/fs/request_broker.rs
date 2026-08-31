//! Blocking-call compatibility bridge for the BSP-owned async filesystem.
//!
//! Architecture invariant: TRUEOSFS and its USB/block futures are non-`Send`
//! and must be created and polled by the BSP executor. A synchronous caller
//! must therefore run on a dedicated background AP service lane, submit only
//! owned request data here, and park until the BSP completes its request.
//!
//! Do not replace this with `spawn_and_wait_local`, a boxed cross-core future,
//! or manual `Executor::poll()`. The old version recursively polled the BSP
//! executor from inside one of its own tasks, deadlocking filesystem access and
//! apparently unrelated first-frame GPGPU/video work.

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use spin::Mutex;

use crate::disc::block;
use crate::r::io::kfs::{FsError, FsNodeKind, FsStat, Result as FsResult};
use crate::wait::CompletionCell;

const BROKER_QUEUE_CAP: usize = 256;

static BROKER_ONLINE: AtomicBool = AtomicBool::new(false);
static BROKER_REQUESTS: Mutex<VecDeque<Request>> = Mutex::new(VecDeque::new());
static BROKER_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();
static BROKER_REQUEST_SEQ: AtomicU64 = AtomicU64::new(1);

type Completion<T> = Arc<CompletionCell<FsResult<T>>>;

enum Request {
    ReadFile {
        id: u64,
        disk: block::DeviceHandle,
        path: String,
        completion: Completion<Vec<u8>>,
    },
    ReadFileLen {
        id: u64,
        disk: block::DeviceHandle,
        path: String,
        completion: Completion<usize>,
    },
    ReadFileRange {
        id: u64,
        disk: block::DeviceHandle,
        path: String,
        offset: u64,
        cap: usize,
        completion: Completion<Vec<u8>>,
    },
    Stat {
        id: u64,
        disk: block::DeviceHandle,
        path: String,
        completion: Completion<FsStat>,
    },
    TypedStat {
        id: u64,
        disk: block::DeviceHandle,
        path: String,
        completion: Completion<(FsStat, crate::r::fs::trueosfs::ContentTypeId)>,
    },
    WriteFileBegin {
        id: u64,
        disk: block::DeviceHandle,
        path: String,
        total_len: u64,
        completion: Completion<u32>,
    },
    WriteFileBeginTyped {
        id: u64,
        disk: block::DeviceHandle,
        path: String,
        total_len: u64,
        content_type: crate::r::fs::trueosfs::ContentTypeId,
        completion: Completion<u32>,
    },
    CreateDirAll {
        id: u64,
        disk: block::DeviceHandle,
        path: String,
        completion: Completion<()>,
    },
    WriteFileChunk {
        id: u64,
        handle: u32,
        data: Vec<u8>,
        completion: Completion<()>,
    },
    WriteFileFinish {
        id: u64,
        handle: u32,
        completion: Completion<()>,
    },
    WriteFileAbort {
        id: u64,
        handle: u32,
        completion: Completion<()>,
    },
    HtmlTree {
        id: u64,
        disk: block::DeviceHandle,
        max_entries: usize,
        completion: Completion<String>,
    },
    JsonAll {
        id: u64,
        disk: block::DeviceHandle,
        max_entries: usize,
        completion: Completion<String>,
    },
    ListDir {
        id: u64,
        disk: block::DeviceHandle,
        path: String,
        completion: Completion<String>,
    },
    TypedListDir {
        id: u64,
        disk: block::DeviceHandle,
        path: String,
        completion: Completion<super::trueosfs::DirListing>,
    },
    Remove {
        id: u64,
        disk: block::DeviceHandle,
        path: String,
        completion: Completion<()>,
    },
    Exists {
        id: u64,
        disk: block::DeviceHandle,
        path: String,
        completion: Completion<bool>,
    },
}

impl Request {
    fn metadata(&self) -> (u64, &'static str) {
        match self {
            Self::ReadFile { id, .. } => (*id, "read-file"),
            Self::ReadFileLen { id, .. } => (*id, "read-file-len"),
            Self::ReadFileRange { id, .. } => (*id, "read-file-range"),
            Self::Stat { id, .. } => (*id, "stat"),
            Self::TypedStat { id, .. } => (*id, "typed-stat"),
            Self::WriteFileBegin { id, .. } => (*id, "write-file-begin"),
            Self::WriteFileBeginTyped { id, .. } => (*id, "write-file-begin-typed"),
            Self::CreateDirAll { id, .. } => (*id, "create-dir-all"),
            Self::WriteFileChunk { id, .. } => (*id, "write-file-chunk"),
            Self::WriteFileFinish { id, .. } => (*id, "write-file-finish"),
            Self::WriteFileAbort { id, .. } => (*id, "write-file-abort"),
            Self::HtmlTree { id, .. } => (*id, "html-tree"),
            Self::JsonAll { id, .. } => (*id, "json-all"),
            Self::ListDir { id, .. } => (*id, "list-dir"),
            Self::TypedListDir { id, .. } => (*id, "typed-list-dir"),
            Self::Remove { id, .. } => (*id, "remove"),
            Self::Exists { id, .. } => (*id, "exists"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockingRequestError {
    BrokerOffline,
    WrongExecutionRealm,
    QueueFull,
}

fn validate_caller() -> core::result::Result<(), BlockingRequestError> {
    if !BROKER_ONLINE.load(Ordering::Acquire) {
        return Err(BlockingRequestError::BrokerOffline);
    }

    let cpu_slot = crate::percpu::this_cpu().cpu_index();
    if !crate::workers::is_background_worker_slot(cpu_slot) {
        return Err(BlockingRequestError::WrongExecutionRealm);
    }
    Ok(())
}

/// Submit owned filesystem request data to the BSP and park the dedicated AP
/// service lane until the BSP completes it. No executor is polled recursively.
fn submit<T>(
    build: impl FnOnce(u64, Completion<T>) -> Request,
) -> core::result::Result<FsResult<T>, BlockingRequestError> {
    validate_caller()?;

    let id = BROKER_REQUEST_SEQ.fetch_add(1, Ordering::Relaxed);
    let completion = Arc::new(CompletionCell::new());
    let request = build(id, completion.clone());
    let (id, operation) = request.metadata();
    let queue_depth = {
        let mut requests = BROKER_REQUESTS.lock();
        if requests.len() >= BROKER_QUEUE_CAP {
            return Err(BlockingRequestError::QueueFull);
        }
        requests.push_back(request);
        requests.len()
    };

    crate::log_trace!(target: "filesystem";
        "trueosfs-request-broker: submitted id={} op={} caller_cpu={} queue_depth={}\n",
        id,
        operation,
        crate::percpu::this_cpu().cpu_index(),
        queue_depth,
    );

    BROKER_WAIT.notify_one();
    crate::remote_work_wake::wake_cpu_for_remote_work(0);
    Ok(completion.join_blocking_parked())
}

pub fn read_file(
    disk: block::DeviceHandle,
    path: String,
) -> core::result::Result<FsResult<Vec<u8>>, BlockingRequestError> {
    submit(|id, completion| Request::ReadFile {
        id,
        disk,
        path,
        completion,
    })
}

pub fn read_file_len(
    disk: block::DeviceHandle,
    path: String,
) -> core::result::Result<FsResult<usize>, BlockingRequestError> {
    submit(|id, completion| Request::ReadFileLen {
        id,
        disk,
        path,
        completion,
    })
}

pub fn read_file_range(
    disk: block::DeviceHandle,
    path: String,
    offset: u64,
    cap: usize,
) -> core::result::Result<FsResult<Vec<u8>>, BlockingRequestError> {
    submit(|id, completion| Request::ReadFileRange {
        id,
        disk,
        path,
        offset,
        cap,
        completion,
    })
}

pub fn stat(
    disk: block::DeviceHandle,
    path: String,
) -> core::result::Result<FsResult<FsStat>, BlockingRequestError> {
    submit(|id, completion| Request::Stat {
        id,
        disk,
        path,
        completion,
    })
}

pub fn typed_stat(
    disk: block::DeviceHandle,
    path: String,
) -> core::result::Result<
    FsResult<(FsStat, crate::r::fs::trueosfs::ContentTypeId)>,
    BlockingRequestError,
> {
    submit(|id, completion| Request::TypedStat {
        id,
        disk,
        path,
        completion,
    })
}

pub fn typed_list_dir(
    disk: block::DeviceHandle,
    path: String,
) -> core::result::Result<FsResult<super::trueosfs::DirListing>, BlockingRequestError> {
    submit(|id, completion| Request::TypedListDir {
        id,
        disk,
        path,
        completion,
    })
}

pub fn write_file_begin(
    disk: block::DeviceHandle,
    path: String,
    total_len: u64,
) -> core::result::Result<FsResult<u32>, BlockingRequestError> {
    submit(|id, completion| Request::WriteFileBegin {
        id,
        disk,
        path,
        total_len,
        completion,
    })
}

pub fn write_file_begin_typed(
    disk: block::DeviceHandle,
    path: String,
    total_len: u64,
    content_type: crate::r::fs::trueosfs::ContentTypeId,
) -> core::result::Result<FsResult<u32>, BlockingRequestError> {
    submit(|id, completion| Request::WriteFileBeginTyped {
        id,
        disk,
        path,
        total_len,
        content_type,
        completion,
    })
}

pub fn create_dir_all(
    disk: block::DeviceHandle,
    path: String,
) -> core::result::Result<FsResult<()>, BlockingRequestError> {
    submit(|id, completion| Request::CreateDirAll {
        id,
        disk,
        path,
        completion,
    })
}

pub fn write_file_chunk(
    handle: u32,
    data: Vec<u8>,
) -> core::result::Result<FsResult<()>, BlockingRequestError> {
    submit(|id, completion| Request::WriteFileChunk {
        id,
        handle,
        data,
        completion,
    })
}

pub fn write_file_finish(handle: u32) -> core::result::Result<FsResult<()>, BlockingRequestError> {
    submit(|id, completion| Request::WriteFileFinish {
        id,
        handle,
        completion,
    })
}

pub fn write_file_abort(handle: u32) -> core::result::Result<FsResult<()>, BlockingRequestError> {
    submit(|id, completion| Request::WriteFileAbort {
        id,
        handle,
        completion,
    })
}

pub fn html_tree(
    disk: block::DeviceHandle,
    max_entries: usize,
) -> core::result::Result<FsResult<String>, BlockingRequestError> {
    submit(|id, completion| Request::HtmlTree {
        id,
        disk,
        max_entries,
        completion,
    })
}

pub fn json_all(
    disk: block::DeviceHandle,
    max_entries: usize,
) -> core::result::Result<FsResult<String>, BlockingRequestError> {
    submit(|id, completion| Request::JsonAll {
        id,
        disk,
        max_entries,
        completion,
    })
}

pub fn list_dir(
    disk: block::DeviceHandle,
    path: String,
) -> core::result::Result<FsResult<String>, BlockingRequestError> {
    submit(|id, completion| Request::ListDir {
        id,
        disk,
        path,
        completion,
    })
}

pub fn remove(
    disk: block::DeviceHandle,
    path: String,
) -> core::result::Result<FsResult<()>, BlockingRequestError> {
    submit(|id, completion| Request::Remove {
        id,
        disk,
        path,
        completion,
    })
}

pub fn exists(
    disk: block::DeviceHandle,
    path: String,
) -> core::result::Result<FsResult<bool>, BlockingRequestError> {
    submit(|id, completion| Request::Exists {
        id,
        disk,
        path,
        completion,
    })
}

async fn process_request(request: Request) {
    let (request_id, operation) = request.metadata();
    crate::log_trace!(target: "filesystem";
        "trueosfs-request-broker: begin id={} op={} realm=bsp\n",
        request_id,
        operation,
    );
    match request {
        Request::ReadFile {
            id,
            disk,
            path,
            completion,
        } => {
            let result = match super::trueosfs::file_out_async(disk, path.as_str()).await {
                Ok(Some(bytes)) => Ok(bytes),
                Ok(None) => Err(FsError::NotFound),
                Err(error) => Err(error.into()),
            };
            finish(id, "read-file", completion, result);
        }
        Request::ReadFileLen {
            id,
            disk,
            path,
            completion,
        } => {
            let result = match super::trueosfs::file_info_async(disk, path.as_str()).await {
                Ok(Some(info)) => Ok(info.data_len as usize),
                Ok(None) => Err(FsError::NotFound),
                Err(error) => Err(error.into()),
            };
            finish(id, "read-file-len", completion, result);
        }
        Request::ReadFileRange {
            id,
            disk,
            path,
            offset,
            cap,
            completion,
        } => {
            let mut scratch = alloc::vec![0u8; cap];
            let result = match super::trueosfs::file_read_range_async(
                disk,
                path.as_str(),
                offset,
                scratch.as_mut_slice(),
            )
            .await
            {
                Ok(Some(got)) => {
                    scratch.truncate(got);
                    Ok(scratch)
                }
                Ok(None) => Err(FsError::NotFound),
                Err(error) => Err(error.into()),
            };
            finish(id, "read-file-range", completion, result);
        }
        Request::Stat {
            id,
            disk,
            path,
            completion,
        } => {
            let result = stat_async(disk, path.as_str()).await;
            finish(id, "stat", completion, result);
        }
        Request::TypedStat {
            id,
            disk,
            path,
            completion,
        } => {
            let result = match super::trueosfs::node_info_async(disk, path.as_str()).await {
                Ok(Some(info)) => Ok((
                    FsStat {
                        kind: match info.kind {
                            super::trueosfs::NodeKind::File => FsNodeKind::File,
                            super::trueosfs::NodeKind::Directory => FsNodeKind::Directory,
                        },
                        len: info.data_len,
                    },
                    info.content_type,
                )),
                Ok(None) => Err(FsError::NotFound),
                Err(error) => Err(error.into()),
            };
            finish(id, "typed-stat", completion, result);
        }
        Request::WriteFileBegin {
            id,
            disk,
            path,
            total_len,
            completion,
        } => {
            let result = match super::trueosfs::file_info_async(disk, path.as_str()).await {
                Ok(Some(info)) if info.content_type != super::trueosfs::ContentTypeId::BLOB => {
                    super::trueosfs::record_type_reject(
                        super::trueosfs::ContentIdentityRejectReason::LegacyDowngrade,
                    );
                    Err(FsError::TypeRequired)
                }
                Ok(_) => {
                    match super::trueosfs::file_write_begin_async(disk, path.as_str(), total_len)
                        .await
                    {
                        Ok(Some(handle)) => Ok(handle),
                        Ok(None) => Err(FsError::NoSpace),
                        Err(error) => Err(error.into()),
                    }
                }
                Err(error) => Err(error.into()),
            };
            finish(id, "write-file-begin", completion, result);
        }
        Request::WriteFileBeginTyped {
            id,
            disk,
            path,
            total_len,
            content_type,
            completion,
        } => {
            let result = match super::trueosfs::file_write_begin_typed_async(
                disk,
                path.as_str(),
                total_len,
                content_type,
            )
            .await
            {
                Ok(Some(handle)) => Ok(handle),
                Ok(None) => Err(FsError::NoSpace),
                Err(error) => Err(error.into()),
            };
            finish(id, "write-file-begin-typed", completion, result);
        }
        Request::CreateDirAll {
            id,
            disk,
            path,
            completion,
        } => {
            let result = create_dir_all_async(disk, path.as_str()).await;
            finish(id, "create-dir-all", completion, result);
        }
        Request::WriteFileChunk {
            id,
            handle,
            data,
            completion,
        } => {
            let result = super::trueosfs::file_write_chunk_async(handle, data.as_slice())
                .await
                .map_err(FsError::from);
            finish(id, "write-file-chunk", completion, result);
        }
        Request::WriteFileFinish {
            id,
            handle,
            completion,
        } => {
            let result = super::trueosfs::file_write_finish_async(handle)
                .await
                .map_err(FsError::from);
            finish(id, "write-file-finish", completion, result);
        }
        Request::WriteFileAbort {
            id,
            handle,
            completion,
        } => {
            let result = super::trueosfs::file_write_abort_async(handle)
                .await
                .map_err(FsError::from);
            finish(id, "write-file-abort", completion, result);
        }
        Request::HtmlTree {
            id,
            disk,
            max_entries,
            completion,
        } => {
            let result = match super::fs_html::html_tree_async(disk, max_entries).await {
                Ok(Some(value)) => Ok(value),
                Ok(None) => Err(FsError::NoRoot),
                Err(error) => Err(error.into()),
            };
            finish(id, "html-tree", completion, result);
        }
        Request::JsonAll {
            id,
            disk,
            max_entries,
            completion,
        } => {
            let result = match super::trueosfs::json_all_async(disk, max_entries).await {
                Ok(Some(value)) => Ok(value),
                Ok(None) => Err(FsError::NoRoot),
                Err(error) => Err(error.into()),
            };
            finish(id, "json-all", completion, result);
        }
        Request::ListDir {
            id,
            disk,
            path,
            completion,
        } => {
            let result = match super::trueosfs::list_dir_async(disk, path.as_str()).await {
                Ok(Some(value)) if value.truncated => {
                    Err(FsError::Device(block::Error::OutOfBounds))
                }
                Ok(Some(value)) => Ok(value
                    .entries
                    .into_iter()
                    .map(|entry| entry.name)
                    .collect::<Vec<_>>()
                    .join("\n")),
                Ok(None) => Err(FsError::NoRoot),
                Err(error) => Err(error.into()),
            };
            finish(id, "list-dir", completion, result);
        }
        Request::TypedListDir {
            id,
            disk,
            path,
            completion,
        } => {
            let result = match super::trueosfs::list_dir_async(disk, path.as_str()).await {
                Ok(Some(value)) => Ok(value),
                Ok(None) => Err(FsError::NoRoot),
                Err(error) => Err(error.into()),
            };
            finish(id, "typed-list-dir", completion, result);
        }
        Request::Remove {
            id,
            disk,
            path,
            completion,
        } => {
            let result = match super::trueosfs::remove_recursive_async(disk, path.as_str()).await {
                Ok(true) => Ok(()),
                Ok(false) => Err(FsError::NotFound),
                Err(error) => Err(error.into()),
            };
            finish(id, "remove", completion, result);
        }
        Request::Exists {
            id,
            disk,
            path,
            completion,
        } => {
            let result = super::trueosfs::node_info_async(disk, path.as_str())
                .await
                .map(|info| info.is_some())
                .map_err(FsError::from);
            finish(id, "exists", completion, result);
        }
    }
}

async fn stat_async(disk: block::DeviceHandle, path: &str) -> FsResult<FsStat> {
    if path.is_empty() {
        return Ok(FsStat {
            kind: FsNodeKind::Directory,
            len: 0,
        });
    }

    if let Some(info) = super::trueosfs::node_info_async(disk, path).await? {
        return Ok(FsStat {
            kind: match info.kind {
                super::trueosfs::NodeKind::File => FsNodeKind::File,
                super::trueosfs::NodeKind::Directory => FsNodeKind::Directory,
            },
            len: info.data_len,
        });
    }
    Err(FsError::NotFound)
}

async fn create_dir_all_async(disk: block::DeviceHandle, path: &str) -> FsResult<()> {
    match super::trueosfs::dir_create_all_async(disk, path).await? {
        true => Ok(()),
        false => Err(FsError::NoSpace),
    }
}

fn finish<T>(id: u64, operation: &'static str, completion: Completion<T>, result: FsResult<T>) {
    let status = if result.is_ok() { "ok" } else { "error" };
    if completion.complete(result).is_err() {
        crate::log_error!(target: "filesystem";
            "trueosfs-request-broker: duplicate completion id={} op={}\n",
            id,
            operation,
        );
    } else {
        crate::log_trace!(target: "filesystem";
            "trueosfs-request-broker: done id={} op={} status={}\n",
            id,
            operation,
            status,
        );
    }
}

#[trueos_executor::task]
pub async fn service_task() {
    BROKER_ONLINE.store(true, Ordering::Release);
    crate::log_info!(target: "filesystem";
        "trueosfs-request-broker: online realm=bsp callers=background-service-lanes wait=parked-no-executor-poll\n"
    );

    loop {
        let request = BROKER_REQUESTS.lock().pop_front();
        match request {
            Some(request) => process_request(request).await,
            None => BROKER_WAIT.wait_for_event().await,
        }
    }
}
