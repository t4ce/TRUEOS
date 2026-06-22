#![allow(dead_code)]

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::shell2::MatrixTarget;

const CODEC_IDLE_MS: u64 = 25;
const COMPLETED_CAP: usize = 16;

static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);
static REQUESTS: Mutex<VecDeque<CodecRequest>> = Mutex::new(VecDeque::new());
static COMPLETED: Mutex<VecDeque<CodecCompletedJob>> = Mutex::new(VecDeque::new());

#[derive(Clone)]
enum CodecRequest {
    SevenZCompressFile {
        id: u64,
        source_path: String,
        archive_path: String,
        target: MatrixTarget,
    },
    SevenZExtractFile {
        id: u64,
        archive_path: String,
        output_path: String,
        target: MatrixTarget,
    },
    SevenZExtractMemory {
        id: u64,
        label: String,
        payload: Vec<u8>,
        wanted_name: Option<String>,
        target: Option<MatrixTarget>,
    },
}

#[derive(Clone)]
pub struct QueuedCodecJob {
    pub id: u64,
    pub slot: Option<String>,
}

#[derive(Clone)]
pub enum CodecCompletedKind {
    FileArchive {
        source_path: String,
        archive_path: String,
        source_bytes: usize,
        archive_bytes: usize,
    },
    FileExtract {
        archive_path: String,
        output_path: String,
        archive_bytes: usize,
        output_bytes: usize,
    },
    MemoryBytes {
        label: String,
        bytes: Vec<u8>,
    },
}

#[derive(Clone)]
pub struct CodecCompletedJob {
    pub id: u64,
    pub kind: CodecCompletedKind,
}

#[derive(Clone, Debug)]
pub enum CodecError {
    NoRoot,
    BadPath,
    NotFound,
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

fn next_job_id() -> u64 {
    NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed)
}

fn push_completed(job: CodecCompletedJob) {
    let mut completed = COMPLETED.lock();
    if completed.len() >= COMPLETED_CAP {
        let _ = completed.pop_front();
    }
    completed.push_back(job);
}

fn normalize_path(path: &str, allow_empty: bool) -> Result<String, CodecError> {
    crate::r::path::FsPath::parse(path, allow_empty)
        .map(|path| path.to_relative_string())
        .map_err(|_| CodecError::BadPath)
}

fn archive_path_for_source(source_path: &str) -> String {
    let mut out = String::from(source_path);
    out.push_str(".7z");
    out
}

fn output_path_for_archive(archive_path: &str) -> Result<String, CodecError> {
    archive_path
        .strip_suffix(".7z")
        .filter(|path| !path.is_empty())
        .map(String::from)
        .ok_or(CodecError::BadPath)
}

fn output_path_for_archive_entry(
    output_root: &str,
    entry_name: &str,
) -> Result<String, CodecError> {
    let entry = normalize_path(entry_name, false)?;
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

fn slot_name_for_job(id: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let value = (id % 1296) as usize;
    let hi = DIGITS[value / 36] as char;
    let lo = DIGITS[value % 36] as char;
    let mut slot = String::from("z");
    slot.push(hi);
    slot.push(lo);
    slot
}

fn log_target(target: &MatrixTarget, line: &str) {
    crate::shell2::print_matrix_target_line(target, line);
}

pub fn enqueue_7z_compress_file(
    source_path: &str,
    output_mask: u8,
) -> Result<QueuedCodecJob, CodecError> {
    let source_path = normalize_path(source_path, false)?;
    let archive_path = archive_path_for_source(source_path.as_str());
    let id = next_job_id();
    let slot = slot_name_for_job(id);
    let target = crate::shell2::matrix_target_for_slot_name(output_mask, slot.as_str());

    log_target(
        &target,
        alloc::format!(
            "7z: queued job={} source={} archive={}",
            id,
            source_path.as_str(),
            archive_path.as_str()
        )
        .as_str(),
    );
    REQUESTS.lock().push_back(CodecRequest::SevenZCompressFile {
        id,
        source_path,
        archive_path,
        target,
    });
    Ok(QueuedCodecJob {
        id,
        slot: Some(slot),
    })
}

pub fn enqueue_7z_extract_file(
    archive_path: &str,
    output_mask: u8,
) -> Result<QueuedCodecJob, CodecError> {
    let archive_path = normalize_path(archive_path, false)?;
    let output_path = output_path_for_archive(archive_path.as_str())?;
    let id = next_job_id();
    let slot = slot_name_for_job(id);
    let target = crate::shell2::matrix_target_for_slot_name(output_mask, slot.as_str());

    log_target(
        &target,
        alloc::format!(
            "7z: queued extract job={} archive={} output={}",
            id,
            archive_path.as_str(),
            output_path.as_str()
        )
        .as_str(),
    );
    REQUESTS.lock().push_back(CodecRequest::SevenZExtractFile {
        id,
        archive_path,
        output_path,
        target,
    });
    Ok(QueuedCodecJob {
        id,
        slot: Some(slot),
    })
}

pub fn enqueue_7z_extract_memory(
    label: &str,
    payload: Vec<u8>,
    wanted_name: Option<String>,
    target: Option<MatrixTarget>,
) -> QueuedCodecJob {
    let id = next_job_id();
    REQUESTS
        .lock()
        .push_back(CodecRequest::SevenZExtractMemory {
            id,
            label: String::from(label),
            payload,
            wanted_name,
            target,
        });
    QueuedCodecJob { id, slot: None }
}

pub fn take_completed(id: u64) -> Option<CodecCompletedJob> {
    let mut completed = COMPLETED.lock();
    let idx = completed.iter().position(|job| job.id == id)?;
    Some(completed.remove(idx)?)
}

fn dequeue_request() -> Option<CodecRequest> {
    REQUESTS.lock().pop_front()
}

async fn compress_file_job(
    id: u64,
    source_path: String,
    archive_path: String,
    target: MatrixTarget,
) -> Result<(), CodecError> {
    crate::shell2::set_matrix_target_active(&target, true);
    let result = async {
        let disk = crate::r::fs::trueosfs::primary_root_handle().ok_or(CodecError::NoRoot)?;
        log_target(
            &target,
            alloc::format!("7z: job={} reading {}", id, source_path.as_str()).as_str(),
        );
        let source = crate::r::fs::trueosfs::file_out_async(disk, source_path.as_str())
            .await?
            .ok_or(CodecError::NotFound)?;

        log_target(
            &target,
            alloc::format!("7z: job={} compressing source_bytes={}", id, source.len()).as_str(),
        );
        let archive =
            crate::z7::compress_single_file_to_vec(basename(source_path.as_str()), &source)?;
        log_target(
            &target,
            alloc::format!(
                "7z: job={} writing {} archive_bytes={}",
                id,
                archive_path.as_str(),
                archive.len()
            )
            .as_str(),
        );

        let ok =
            crate::r::fs::trueosfs::file_in_async(disk, archive_path.as_str(), archive.as_slice())
                .await?;
        if !ok {
            return Err(CodecError::WriteFailed);
        }

        let source_bytes = source.len();
        let archive_bytes = archive.len();
        push_completed(CodecCompletedJob {
            id,
            kind: CodecCompletedKind::FileArchive {
                source_path: source_path.clone(),
                archive_path: archive_path.clone(),
                source_bytes,
                archive_bytes,
            },
        });
        log_target(
            &target,
            alloc::format!(
                "7z: done job={} source={} bytes archive={} bytes path={}",
                id,
                source_bytes,
                archive_bytes,
                archive_path.as_str()
            )
            .as_str(),
        );
        Ok(())
    }
    .await;
    crate::shell2::set_matrix_target_active(&target, false);
    result
}

async fn extract_file_job(
    id: u64,
    archive_path: String,
    output_path: String,
    target: MatrixTarget,
) -> Result<(), CodecError> {
    crate::shell2::set_matrix_target_active(&target, true);
    let result = async {
        let disk = crate::r::fs::trueosfs::primary_root_handle().ok_or(CodecError::NoRoot)?;
        log_target(
            &target,
            alloc::format!("7z: job={} reading archive {}", id, archive_path.as_str()).as_str(),
        );
        let archive = crate::r::fs::trueosfs::file_out_async(disk, archive_path.as_str())
            .await?
            .ok_or(CodecError::NotFound)?;

        log_target(
            &target,
            alloc::format!("7z: job={} extracting archive_bytes={}", id, archive.len()).as_str(),
        );
        let entries = crate::z7::extract_all_to_vec(archive.as_slice())?;
        if entries.is_empty() {
            return Err(CodecError::Archive(crate::z7::SevenZError::BadHeader));
        }
        let archive_bytes = archive.len();
        let mut output_bytes = 0usize;
        let mut output_files = 0usize;

        if entries.len() == 1 {
            let entry = entries
                .first()
                .ok_or(CodecError::Archive(crate::z7::SevenZError::BadHeader))?;
            log_target(
                &target,
                alloc::format!(
                    "7z: job={} writing {} output_bytes={}",
                    id,
                    output_path.as_str(),
                    entry.bytes.len()
                )
                .as_str(),
            );
            let ok = crate::r::fs::trueosfs::file_in_async(
                disk,
                output_path.as_str(),
                entry.bytes.as_slice(),
            )
            .await?;
            if !ok {
                return Err(CodecError::WriteFailed);
            }
            output_bytes = entry.bytes.len();
            output_files = 1;
        } else {
            log_target(
                &target,
                alloc::format!("7z: job={} writing entries={}", id, entries.len()).as_str(),
            );
            for entry in entries {
                let path =
                    output_path_for_archive_entry(output_path.as_str(), entry.name.as_str())?;
                log_target(
                    &target,
                    alloc::format!(
                        "7z: job={} writing {} output_bytes={}",
                        id,
                        path.as_str(),
                        entry.bytes.len()
                    )
                    .as_str(),
                );
                let ok = crate::r::fs::trueosfs::file_in_async(
                    disk,
                    path.as_str(),
                    entry.bytes.as_slice(),
                )
                .await?;
                if !ok {
                    return Err(CodecError::WriteFailed);
                }
                output_bytes = output_bytes
                    .checked_add(entry.bytes.len())
                    .ok_or(CodecError::WriteFailed)?;
                output_files = output_files.checked_add(1).ok_or(CodecError::WriteFailed)?;
            }
        }

        push_completed(CodecCompletedJob {
            id,
            kind: CodecCompletedKind::FileExtract {
                archive_path: archive_path.clone(),
                output_path: output_path.clone(),
                archive_bytes,
                output_bytes,
            },
        });
        log_target(
            &target,
            alloc::format!(
                "7z: done job={} archive={} bytes output={} bytes files={} path={}",
                id,
                archive_bytes,
                output_bytes,
                output_files,
                output_path.as_str()
            )
            .as_str(),
        );
        Ok(())
    }
    .await;
    crate::shell2::set_matrix_target_active(&target, false);
    result
}

async fn extract_memory_job(
    id: u64,
    label: String,
    payload: Vec<u8>,
    wanted_name: Option<String>,
    target: Option<MatrixTarget>,
) -> Result<(), CodecError> {
    if let Some(target) = &target {
        crate::shell2::set_matrix_target_active(target, true);
        log_target(
            target,
            alloc::format!("codec: job={} decode label={} bytes={}", id, label, payload.len())
                .as_str(),
        );
    }

    let decoded = if let Some(wanted_name) = wanted_name {
        crate::z7::extract_file_to_vec(payload.as_slice(), wanted_name.as_str())?
    } else {
        crate::z7::extract_single_file_to_vec(payload.as_slice())?
    };
    let decoded_len = decoded.len();
    push_completed(CodecCompletedJob {
        id,
        kind: CodecCompletedKind::MemoryBytes {
            label: label.clone(),
            bytes: decoded,
        },
    });

    if let Some(target) = &target {
        log_target(
            target,
            alloc::format!("codec: done job={} label={} decoded_bytes={}", id, label, decoded_len)
                .as_str(),
        );
        crate::shell2::set_matrix_target_active(target, false);
    }
    Ok(())
}

async fn execute_request(worker_id: usize, request: CodecRequest) {
    let result = match request {
        CodecRequest::SevenZCompressFile {
            id,
            source_path,
            archive_path,
            target,
        } => {
            log_target(
                &target,
                alloc::format!("codec: worker={} start job={}", worker_id, id).as_str(),
            );
            let result = compress_file_job(id, source_path, archive_path, target.clone()).await;
            if let Err(err) = &result {
                log_target(&target, alloc::format!("7z: failed job={} err={}", id, err).as_str());
            }
            result
        }
        CodecRequest::SevenZExtractFile {
            id,
            archive_path,
            output_path,
            target,
        } => {
            log_target(
                &target,
                alloc::format!("codec: worker={} start job={}", worker_id, id).as_str(),
            );
            let result = extract_file_job(id, archive_path, output_path, target.clone()).await;
            if let Err(err) = &result {
                log_target(&target, alloc::format!("7z: failed job={} err={}", id, err).as_str());
            }
            result
        }
        CodecRequest::SevenZExtractMemory {
            id,
            label,
            payload,
            wanted_name,
            target,
        } => {
            let result = extract_memory_job(id, label, payload, wanted_name, target.clone()).await;
            if let (Err(err), Some(target)) = (&result, &target) {
                log_target(target, alloc::format!("codec: failed job={} err={}", id, err).as_str());
                crate::shell2::set_matrix_target_active(target, false);
            }
            result
        }
    };
    let _ = result;
}

#[embassy_executor::task(pool_size = 3)]
pub async fn codec_worker_task(worker_id: usize) {
    crate::log_info!(
        target: "service";
        "codec: worker={} online archive=7z pool=3\n",
        worker_id
    );
    loop {
        match dequeue_request() {
            Some(request) => execute_request(worker_id, request).await,
            None => Timer::after(EmbassyDuration::from_millis(CODEC_IDLE_MS)).await,
        }
    }
}
