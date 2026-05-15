extern crate alloc;

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

const SHADER_DIR: &str = "shader";
const SERVICE_IDLE_MS: u64 = 100;
const ARTIFACT_MAGIC: &[u8; 8] = b"TEU32\0\x01\0";
const NO_DWORD: u32 = u32::MAX;

static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);
static REQUESTS: Mutex<VecDeque<ShaderCompileRequest>> = Mutex::new(VecDeque::new());

#[derive(Clone, Debug)]
enum ShaderCompileRequest {
    CompilePath { id: u64, path: String },
    ScanDir { id: u64 },
}

#[derive(Clone, Debug)]
pub struct QueuedShaderJob {
    pub id: u64,
}

#[derive(Clone, Debug)]
pub struct ShaderCompileReport {
    pub source_path: String,
    pub artifact_path: String,
    pub source_bytes: usize,
    pub artifact_bytes: usize,
    pub words: usize,
    pub expected_store_value: u32,
}

#[derive(Clone, Debug)]
pub enum ShaderServiceError {
    NoRoot,
    BadPath,
    NotFound,
    InvalidUtf8,
    Parse(String),
    Emit(String),
    WriteFailed,
    Fs(crate::disc::block::Error),
}

impl From<crate::disc::block::Error> for ShaderServiceError {
    fn from(value: crate::disc::block::Error) -> Self {
        Self::Fs(value)
    }
}

pub fn enqueue_compile_path(path: &str) -> Result<QueuedShaderJob, ShaderServiceError> {
    let path = normalize_shader_path(path)?;
    let id = NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed);
    REQUESTS
        .lock()
        .push_back(ShaderCompileRequest::CompilePath { id, path });
    Ok(QueuedShaderJob { id })
}

pub fn enqueue_scan_dir() -> QueuedShaderJob {
    let id = NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed);
    REQUESTS
        .lock()
        .push_back(ShaderCompileRequest::ScanDir { id });
    QueuedShaderJob { id }
}

pub fn pending_jobs() -> usize {
    REQUESTS.lock().len()
}

pub fn list_compile_candidates_sync() -> Result<Vec<String>, ShaderServiceError> {
    crate::wait::spawn_and_wait_local(list_compile_candidates_async())
}

pub fn compile_path_sync(path: &str) -> Result<ShaderCompileReport, ShaderServiceError> {
    let path = normalize_shader_path(path)?;
    crate::wait::spawn_and_wait_local(compile_path_async(path))
}

async fn list_compile_candidates_async() -> Result<Vec<String>, ShaderServiceError> {
    let disk = crate::r::fs::trueosfs::primary_root_handle().ok_or(ShaderServiceError::NoRoot)?;
    let Some(listing) = crate::r::fs::trueosfs::list_dir_async(disk, SHADER_DIR).await? else {
        return Err(ShaderServiceError::NoRoot);
    };

    let mut out = Vec::new();
    for name in listing.lines() {
        if is_shader_source_name(name) {
            out.push(format!("{}/{}", SHADER_DIR, name));
        }
    }
    Ok(out)
}

async fn compile_scan_dir(id: u64) -> Result<usize, ShaderServiceError> {
    let candidates = list_compile_candidates_async().await?;
    let mut compiled = 0usize;
    for path in candidates {
        match compile_path_async(path.clone()).await {
            Ok(report) => {
                compiled = compiled.saturating_add(1);
                crate::log_info!(
                    target: "service";
                    "shader-compile: job={} source={} artifact={} words={} expected=0x{:08X}\n",
                    id,
                    report.source_path.as_str(),
                    report.artifact_path.as_str(),
                    report.words,
                    report.expected_store_value
                );
            }
            Err(err) => {
                crate::log_warn!(
                    target: "service";
                    "shader-compile: job={} source={} failed err={:?}\n",
                    id,
                    path.as_str(),
                    err
                );
            }
        }
    }
    Ok(compiled)
}

async fn compile_path_async(path: String) -> Result<ShaderCompileReport, ShaderServiceError> {
    let disk = crate::r::fs::trueosfs::primary_root_handle().ok_or(ShaderServiceError::NoRoot)?;
    let Some(bytes) = crate::r::fs::trueosfs::file_out_async(disk, path.as_str()).await? else {
        return Err(ShaderServiceError::NotFound);
    };
    let source_bytes = bytes.len();
    let source = String::from_utf8(bytes).map_err(|_| ShaderServiceError::InvalidUtf8)?;
    let program = trueos_c4::parse_program(source.as_str())
        .map_err(|err| ShaderServiceError::Parse(err.message))?;
    let object = trueos_c4::emit_eu32_object(&program)
        .map_err(|err| ShaderServiceError::Emit(err.message))?;
    let artifact = serialize_eu32_artifact(&object);
    let artifact_path = artifact_path_for_source(path.as_str());
    let ok = crate::r::fs::trueosfs::file_in_async(disk, artifact_path.as_str(), artifact.as_slice())
        .await?;
    if !ok {
        return Err(ShaderServiceError::WriteFailed);
    }

    Ok(ShaderCompileReport {
        source_path: path,
        artifact_path,
        source_bytes,
        artifact_bytes: artifact.len(),
        words: object.words.len(),
        expected_store_value: object.expected_store_value,
    })
}

fn serialize_eu32_artifact(object: &trueos_c4::Eu32Object) -> Vec<u8> {
    let mut out = Vec::with_capacity(24 + object.words.len() * core::mem::size_of::<u32>());
    out.extend_from_slice(ARTIFACT_MAGIC);
    push_u32(&mut out, object.words.len() as u32);
    push_u32(&mut out, object.expected_store_value);
    push_u32(
        &mut out,
        object
            .store_send_dword
            .map(|value| value as u32)
            .unwrap_or(NO_DWORD),
    );
    push_u32(
        &mut out,
        object
            .visible_seed_dword
            .map(|value| value as u32)
            .unwrap_or(NO_DWORD),
    );
    for word in object.words.iter().copied() {
        push_u32(&mut out, word);
    }
    out
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn normalize_shader_path(path: &str) -> Result<String, ShaderServiceError> {
    let mut trimmed = path.trim();
    if let Some(rest) = trimmed.strip_prefix('/') {
        trimmed = rest;
    }
    if trimmed.is_empty() || trimmed.contains("..") || trimmed.ends_with('/') {
        return Err(ShaderServiceError::BadPath);
    }
    if !trimmed.starts_with("shader/") {
        return Err(ShaderServiceError::BadPath);
    }
    let name = trimmed.rsplit('/').next().unwrap_or("");
    if !is_shader_source_name(name) {
        return Err(ShaderServiceError::BadPath);
    }
    Ok(trimmed.to_string())
}

fn is_shader_source_name(name: &str) -> bool {
    (name.ends_with(".c4") || name.ends_with(".shader")) && !name.ends_with(".eu32")
}

fn artifact_path_for_source(path: &str) -> String {
    if let Some(stem) = path.strip_suffix(".c4") {
        return format!("{}.eu32", stem);
    }
    if let Some(stem) = path.strip_suffix(".shader") {
        return format!("{}.eu32", stem);
    }
    format!("{}.eu32", path)
}

#[embassy_executor::task(pool_size = 1)]
pub async fn shader_compile_service_task() {
    crate::log_info!(
        target: "service";
        "shader-compile: service online dir=/{} subset=c4-out-const-eu32\n",
        SHADER_DIR
    );
    loop {
        let request = REQUESTS.lock().pop_front();
        match request {
            Some(ShaderCompileRequest::CompilePath { id, path }) => {
                match compile_path_async(path.clone()).await {
                    Ok(report) => crate::log_info!(
                        target: "service";
                        "shader-compile: job={} source={} artifact={} source_bytes={} artifact_bytes={} words={} expected=0x{:08X}\n",
                        id,
                        report.source_path.as_str(),
                        report.artifact_path.as_str(),
                        report.source_bytes,
                        report.artifact_bytes,
                        report.words,
                        report.expected_store_value
                    ),
                    Err(err) => crate::log_warn!(
                        target: "service";
                        "shader-compile: job={} source={} failed err={:?}\n",
                        id,
                        path.as_str(),
                        err
                    ),
                }
            }
            Some(ShaderCompileRequest::ScanDir { id }) => match compile_scan_dir(id).await {
                Ok(compiled) => crate::log_info!(
                    target: "service";
                    "shader-compile: scan job={} compiled={}\n",
                    id,
                    compiled
                ),
                Err(err) => crate::log_warn!(
                    target: "service";
                    "shader-compile: scan job={} failed err={:?}\n",
                    id,
                    err
                ),
            },
            None => Timer::after(EmbassyDuration::from_millis(SERVICE_IDLE_MS)).await,
        }
    }
}
