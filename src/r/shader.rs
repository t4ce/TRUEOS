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

impl core::fmt::Display for ShaderServiceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoRoot => f.write_str("no TRUEOSFS root"),
            Self::BadPath => f.write_str("bad shader path"),
            Self::NotFound => f.write_str("shader source not found"),
            Self::InvalidUtf8 => f.write_str("shader source is not utf-8"),
            Self::Parse(message) => write!(f, "parse: {}", message),
            Self::Emit(message) => write!(f, "emit: {}", message),
            Self::WriteFailed => f.write_str("artifact write failed"),
            Self::Fs(err) => write!(f, "fs: {:?}", err),
        }
    }
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
                    "shader-compile: job={} source={} failed err={}\n",
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
    let source = String::from_utf8(bytes).map_err(|_| ShaderServiceError::InvalidUtf8)?;
    let _program = trueos_c4::parse_program(source.as_str())
        .map_err(|err| ShaderServiceError::Parse(err.message))?;
    Err(ShaderServiceError::Emit("C4 EU artifact backend removed".to_string()))
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
        "shader-compile: service online dir=/{} status=disabled reason=c4-eu-backend-removed\n",
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
                        "shader-compile: job={} source={} failed err={}\n",
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
                    "shader-compile: scan job={} failed err={}\n",
                    id,
                    err
                ),
            },
            None => Timer::after(EmbassyDuration::from_millis(SERVICE_IDLE_MS)).await,
        }
    }
}
