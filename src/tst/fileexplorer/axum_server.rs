extern crate alloc;
extern crate std;

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};
use core::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::{io, net::SocketAddr, sync::Arc};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, OriginalUri, Path, State},
    http::{
        HeaderMap, StatusCode,
        header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_TYPE},
    },
    response::Response,
    routing::{get, patch, post},
    serve::ListenerExt,
};
use embassy_time::{Duration as EmbassyDuration, Timer};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::allports::services::{FILEEXPLORER_HTTP_TCP_PORT, FILEEXPLORER_HTTP_TCP_PORTS};

const FILEEXPLORER_HTTP_BODY_MAX: usize = 64 * 1024;
const FILEEXPLORER_UPLOAD_BODY_MAX: usize = 16 * 1024 * 1024;
const FILEEXPLORER_TEXT_OPEN_MAX: u64 = 5 * 1024 * 1024;
const FILEEXPLORER_BLOCKING_LANE_RETRY_MS: u64 = 1000;
const FILEEXPLORER_CHAT_GRACE_MS: u64 = 1500;
const FILEEXPLORER_INDEX_HTML: &str = include_str!("index.html");
const TRUEOSFS_KEEP_FILE: &str = ".keep";
const MAX_TREE_DEPTH: usize = 8;
const MAX_TREE_NODES: usize = 512;
const SCHEMA: &str = "filetree.v1";

static FILEEXPLORER_HTTP_PORT: AtomicU16 = AtomicU16::new(0);
static JOB_SEQ: AtomicU64 = AtomicU64::new(1);

type JobMap = Arc<RwLock<BTreeMap<String, JobRecord>>>;

#[derive(Clone)]
struct AppState {
    jobs: JobMap,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TreeSnapshot {
    schema: &'static str,
    version: u64,
    root: FileNode,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileNode {
    id: String,
    name: String,
    kind: NodeKind,
    size: u64,
    modified: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mime: Option<String>,
    meta: BTreeMap<String, Value>,
    actions: Vec<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<FileNode>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum NodeKind {
    File,
    Folder,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct JobRecord {
    id: String,
    operation: String,
    status: &'static str,
    progress: u8,
    description: String,
    affected_node_ids: Vec<String>,
    created_at_ms: u64,
    updated_at_ms: u64,
    result: Option<Value>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AcceptedJob {
    job_id: String,
    label: String,
    status_url: String,
    events_url: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateNodeRequest {
    parent_id: String,
    #[serde(default)]
    id: Option<String>,
    name: String,
    kind: NodeKind,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateNodeRequest {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteNodesRequest {
    ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoveNodesRequest {
    #[serde(default)]
    ids: Vec<String>,
    #[serde(default)]
    target_parent_id: Option<String>,
    #[serde(default)]
    moves: Vec<MoveInstruction>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoveInstruction {
    node_id: String,
    new_parent_id: String,
}

#[allow(dead_code)]
pub fn current_port() -> Option<u16> {
    match FILEEXPLORER_HTTP_PORT.load(Ordering::Acquire) {
        0 => None,
        port => Some(port),
    }
}

pub fn index_html() -> String {
    FILEEXPLORER_INDEX_HTML.replace(
        "<title>Async File Explorer</title>",
        "<title>TRUEOSFS File Explorer</title>\n  <script>window.FILE_EXPLORER_API_BASE = \"/api\";</script>",
    )
}

fn primary_root() -> Result<crate::disc::block::DeviceHandle, &'static str> {
    crate::r::fs::trueosfs::primary_root_handle().ok_or("no TRUEOSFS root mounted")
}

fn status_code(status: u16) -> StatusCode {
    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

fn response(status: u16, content_type: &'static str, body: Vec<u8>) -> Response {
    let mut builder = Response::builder()
        .status(status_code(status))
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, body.len().to_string())
        .header(CACHE_CONTROL, "no-store");
    if status == 200 && content_type.starts_with("text/html") {
        builder = builder.header(CACHE_CONTROL, "no-cache");
    }
    builder
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn file_response(content_type: String, filename: &str, body: Vec<u8>) -> Response {
    let safe_name = filename.replace(['"', '\r', '\n'], "_");
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, content_type)
        .header(CONTENT_LENGTH, body.len().to_string())
        .header(CACHE_CONTROL, "no-store")
        .header(CONTENT_DISPOSITION, format!("attachment; filename=\"{safe_name}\""))
        .body(Body::from(body))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn text_response(status: u16, content_type: &'static str, body: &str) -> Response {
    response(status, content_type, body.as_bytes().to_vec())
}

fn json_response<T: Serialize>(status: u16, value: &T) -> Response {
    match serde_json::to_vec(value) {
        Ok(body) => response(status, "application/json; charset=utf-8", body),
        Err(_) => text_response(500, "text/plain; charset=utf-8", "json serialization failed\n"),
    }
}

fn error_response(status: u16, error: impl ToString) -> Response {
    json_response(
        status,
        &serde_json::json!({
            "ok": false,
            "error": error.to_string(),
        }),
    )
}

async fn run_local<F, MakeFuture>(make_future: MakeFuture) -> Response
where
    F: core::future::Future<Output = Response> + 'static,
    MakeFuture: FnOnce() -> F + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    if crate::t::spawn_on_shared_tokio(move || async move {
        let _ = tx.send(make_future().await);
    })
    .is_err()
    {
        return error_response(503, "fileexplorer worker unavailable");
    }
    rx.await
        .unwrap_or_else(|_| error_response(503, "fileexplorer worker dropped response"))
}

fn now_ms() -> u64 {
    crate::r::net::ntp::current_unix_seconds()
        .or_else(crate::time::unix_time_seconds)
        .unwrap_or_else(crate::time::uptime_seconds)
        .saturating_mul(1000)
}

fn now_iso() -> String {
    let secs = crate::r::net::ntp::current_unix_seconds()
        .or_else(crate::time::unix_time_seconds)
        .unwrap_or_else(crate::time::uptime_seconds);
    let (year, month, day, hour, minute, second) = unix_timestamp_to_ymdhms(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.000Z")
}

fn unix_timestamp_to_ymdhms(ts: u64) -> (u32, u8, u8, u8, u8, u8) {
    const SECS_PER_MIN: u64 = 60;
    const SECS_PER_HOUR: u64 = 60 * SECS_PER_MIN;
    const SECS_PER_DAY: u64 = 24 * SECS_PER_HOUR;

    let mut days = ts / SECS_PER_DAY;
    let mut rem = ts % SECS_PER_DAY;
    let hour = (rem / SECS_PER_HOUR) as u8;
    rem %= SECS_PER_HOUR;
    let minute = (rem / SECS_PER_MIN) as u8;
    let second = (rem % SECS_PER_MIN) as u8;

    let mut year = 1970u32;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    let month_lengths = month_lengths(year);
    for (idx, len) in month_lengths.iter().enumerate() {
        if days < *len as u64 {
            return (year, (idx + 1) as u8, (days + 1) as u8, hour, minute, second);
        }
        days -= *len as u64;
    }
    (year, 12, 31, hour, minute, second)
}

fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && !year.is_multiple_of(100) || year.is_multiple_of(400)
}

fn month_lengths(year: u32) -> [u8; 12] {
    [
        31,
        if is_leap_year(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ]
}

fn encode_node_id(path: &str) -> String {
    if path.is_empty() {
        return "root".to_string();
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::from("p_");
    for byte in path.as_bytes() {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn decode_node_id(id: &str) -> Result<String, String> {
    if id == "root" {
        return Ok(String::new());
    }
    let hex = id
        .strip_prefix("p_")
        .ok_or_else(|| format!("invalid node id '{id}'"))?;
    if !hex.len().is_multiple_of(2) {
        return Err(format!("invalid node id '{id}'"));
    }
    let bytes = hex.as_bytes();
    let mut out = Vec::with_capacity(hex.len() / 2);
    let mut i = 0;
    while i < bytes.len() {
        let hi = hex_nibble(bytes[i]).ok_or_else(|| format!("invalid node id '{id}'"))?;
        let lo = hex_nibble(bytes[i + 1]).ok_or_else(|| format!("invalid node id '{id}'"))?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    String::from_utf8(out).map_err(|_| format!("invalid utf8 node id '{id}'"))
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err("bad percent encoding".to_string());
            }
            let hi = hex_nibble(bytes[i + 1]).ok_or_else(|| "bad percent encoding".to_string())?;
            let lo = hex_nibble(bytes[i + 2]).ok_or_else(|| "bad percent encoding".to_string())?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| "bad utf8 in encoded value".to_string())
}

fn validate_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains('/')
        || trimmed.contains('\\')
    {
        return Err(format!("invalid name '{name}'"));
    }
    Ok(())
}

fn join_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

fn parent_path(path: &str) -> String {
    match path.rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => String::new(),
    }
}

fn base_name(path: &str) -> String {
    if path.is_empty() {
        "TRUEOSFS".to_string()
    } else {
        path.rsplit('/').next().unwrap_or(path).to_string()
    }
}

fn is_child_of(path: &str, parent: &str) -> bool {
    if parent.is_empty() {
        return !path.is_empty();
    }
    path.strip_prefix(parent)
        .map(|rest| rest.starts_with('/'))
        .unwrap_or(false)
}

fn mime_from_name(name: &str) -> &'static str {
    if name.ends_with(".html") {
        "text/html"
    } else if name.ends_with(".css") {
        "text/css"
    } else if name.ends_with(".js") {
        "text/javascript"
    } else if name.ends_with(".json") {
        "application/json"
    } else if name.ends_with(".md") {
        "text/markdown"
    } else if name.ends_with(".rs") {
        "text/rust"
    } else if name.ends_with(".png") {
        "image/png"
    } else if name.ends_with(".jpg") || name.ends_with(".jpeg") {
        "image/jpeg"
    } else if name.ends_with(".webp") {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

fn default_actions(kind: NodeKind, root: bool) -> Vec<&'static str> {
    if root {
        return vec!["open", "new-file", "new-folder", "upload"];
    }
    match kind {
        NodeKind::Folder => vec![
            "open",
            "new-file",
            "new-folder",
            "upload",
            "rename",
            "move",
            "delete",
        ],
        NodeKind::File => vec!["open", "download", "rename", "move", "delete"],
    }
}

async fn build_node(
    disk: crate::disc::block::DeviceHandle,
    path: String,
    depth: usize,
    budget: &mut usize,
) -> Result<Option<FileNode>, String> {
    if *budget == 0 {
        return Ok(None);
    }
    *budget -= 1;

    let root = path.is_empty();
    if !root {
        if let Some(info) = crate::r::fs::trueosfs::file_info_async(disk, &path)
            .await
            .map_err(|err| format!("file info failed: {err:?}"))?
        {
            let name = base_name(&path);
            return Ok(Some(FileNode {
                id: encode_node_id(&path),
                name: name.clone(),
                kind: NodeKind::File,
                size: info.data_len,
                modified: now_iso(),
                mime: Some(mime_from_name(&name).to_string()),
                meta: BTreeMap::new(),
                actions: default_actions(NodeKind::File, false),
                children: Vec::new(),
            }));
        }
    }

    let listing = crate::r::fs::trueosfs::list_dir_async(disk, &path)
        .await
        .map_err(|err| format!("list failed: {err:?}"))?
        .unwrap_or_default();
    let mut children = Vec::new();
    if depth < MAX_TREE_DEPTH {
        for name in listing.lines() {
            if name.is_empty() || name == TRUEOSFS_KEEP_FILE {
                continue;
            }
            let child_path = join_path(&path, name);
            if let Some(child) = Box::pin(build_node(disk, child_path, depth + 1, budget)).await? {
                children.push(child);
            }
            if *budget == 0 {
                break;
            }
        }
    }
    children.sort_by(|a, b| {
        (a.kind != NodeKind::Folder, a.name.as_str())
            .cmp(&(b.kind != NodeKind::Folder, b.name.as_str()))
    });
    let size = children.iter().map(|child| child.size).sum();
    Ok(Some(FileNode {
        id: encode_node_id(&path),
        name: base_name(&path),
        kind: NodeKind::Folder,
        size,
        modified: now_iso(),
        mime: None,
        meta: BTreeMap::new(),
        actions: default_actions(NodeKind::Folder, root),
        children,
    }))
}

async fn tree_for_path(path: String) -> Result<TreeSnapshot, String> {
    let disk = primary_root().map_err(ToString::to_string)?;
    let mut budget = MAX_TREE_NODES;
    let root = build_node(disk, path, 0, &mut budget)
        .await?
        .ok_or_else(|| "tree budget exhausted".to_string())?;
    Ok(TreeSnapshot {
        schema: SCHEMA,
        version: now_ms(),
        root,
    })
}

async fn collect_files(
    disk: crate::disc::block::DeviceHandle,
    path: &str,
    out: &mut Vec<String>,
) -> Result<(), String> {
    if crate::r::fs::trueosfs::file_info_async(disk, path)
        .await
        .map_err(|err| format!("file info failed: {err:?}"))?
        .is_some()
    {
        out.push(path.to_string());
        return Ok(());
    }

    let listing = crate::r::fs::trueosfs::list_dir_async(disk, path)
        .await
        .map_err(|err| format!("list failed: {err:?}"))?
        .unwrap_or_default();
    for name in listing.lines() {
        if name.is_empty() {
            continue;
        }
        let child_path = join_path(path, name);
        Box::pin(collect_files(disk, &child_path, out)).await?;
    }
    Ok(())
}

async fn rename_tree(src: &str, dst: &str) -> Result<(), String> {
    let disk = primary_root().map_err(ToString::to_string)?;
    if src.is_empty() || dst.is_empty() {
        return Err("root cannot be renamed or moved".to_string());
    }
    if crate::r::fs::trueosfs::file_info_async(disk, src)
        .await
        .map_err(|err| format!("file info failed: {err:?}"))?
        .is_some()
    {
        let ok = crate::r::fs::trueosfs::file_rename_async(disk, src, dst)
            .await
            .map_err(|err| format!("rename failed: {err:?}"))?;
        return ok
            .then_some(())
            .ok_or_else(|| format!("rename refused from '{src}' to '{dst}'"));
    }

    let mut files = Vec::new();
    collect_files(disk, src, &mut files).await?;
    if files.is_empty() {
        return Err(format!("folder '{src}' is empty or missing"));
    }
    for file in files.iter() {
        let suffix = file.strip_prefix(src).unwrap_or(file);
        let target = format!("{dst}{suffix}");
        if crate::r::fs::trueosfs::file_exists_async(disk, &target)
            .await
            .map_err(|err| format!("exists failed: {err:?}"))?
        {
            return Err(format!("destination '{target}' already exists"));
        }
    }
    for file in files.iter() {
        let suffix = file.strip_prefix(src).unwrap_or(file);
        let target = format!("{dst}{suffix}");
        let ok = crate::r::fs::trueosfs::file_rename_async(disk, file, &target)
            .await
            .map_err(|err| format!("rename failed: {err:?}"))?;
        if !ok {
            return Err(format!("rename refused from '{file}' to '{target}'"));
        }
    }
    Ok(())
}

async fn delete_tree(path: &str) -> Result<(), String> {
    let disk = primary_root().map_err(ToString::to_string)?;
    if path.is_empty() {
        return Err("root cannot be deleted".to_string());
    }
    let mut files = Vec::new();
    collect_files(disk, path, &mut files).await?;
    if files.is_empty() {
        return Err(format!("'{path}' was not found"));
    }
    for file in files.iter() {
        crate::r::fs::trueosfs::file_delete_async(disk, file)
            .await
            .map_err(|err| format!("delete failed: {err:?}"))?;
    }
    Ok(())
}

async fn create_node_job(request: CreateNodeRequest) -> Result<Value, String> {
    validate_name(&request.name)?;
    let parent = decode_node_id(&request.parent_id)?;
    let path = join_path(&parent, request.name.trim());
    let disk = primary_root().map_err(ToString::to_string)?;
    if crate::r::fs::trueosfs::file_exists_async(disk, &path)
        .await
        .map_err(|err| format!("exists failed: {err:?}"))?
    {
        return Err(format!("'{path}' already exists"));
    }
    let write_path = if request.kind == NodeKind::Folder {
        join_path(&path, TRUEOSFS_KEEP_FILE)
    } else {
        path.clone()
    };
    let ok = crate::r::fs::trueosfs::file_in_async(disk, &write_path, &[])
        .await
        .map_err(|err| format!("write failed: {err:?}"))?;
    if !ok {
        return Err(format!("write refused for '{write_path}'"));
    }
    Ok(serde_json::json!({
        "nodeId": request.id.unwrap_or_else(|| encode_node_id(&path)),
        "path": path,
    }))
}

async fn upload_file_job(parent_id: String, name: String, body: Vec<u8>) -> Result<Value, String> {
    validate_name(&name)?;
    let parent = decode_node_id(&parent_id)?;
    let path = join_path(&parent, name.trim());
    let disk = primary_root().map_err(ToString::to_string)?;
    if crate::r::fs::trueosfs::file_exists_async(disk, &path)
        .await
        .map_err(|err| format!("exists failed: {err:?}"))?
    {
        return Err(format!("'{path}' already exists"));
    }
    let ok = crate::r::fs::trueosfs::file_in_async(disk, &path, body.as_slice())
        .await
        .map_err(|err| format!("upload failed: {err:?}"))?;
    if !ok {
        return Err(format!("write refused for '{path}'"));
    }
    Ok(serde_json::json!({
        "nodeId": encode_node_id(&path),
        "path": path,
    }))
}

async fn download_file(id: String) -> Result<(String, String, Vec<u8>), String> {
    let path = decode_node_id(&id)?;
    if path.is_empty() {
        return Err("root cannot be downloaded".to_string());
    }
    let disk = primary_root().map_err(ToString::to_string)?;
    let info = crate::r::fs::trueosfs::file_info_async(disk, &path)
        .await
        .map_err(|err| format!("file info failed: {err:?}"))?
        .ok_or_else(|| format!("'{path}' was not found or is not a file"))?;
    let bytes = crate::r::fs::trueosfs::file_out_async(disk, &path)
        .await
        .map_err(|err| format!("read failed: {err:?}"))?
        .ok_or_else(|| format!("'{path}' was not found"))?;
    if bytes.len() as u64 != info.data_len {
        return Err(format!("read length mismatch for '{path}'"));
    }
    let name = base_name(&path);
    Ok((name.clone(), mime_from_name(&name).to_string(), bytes))
}

async fn read_text_file(id: String) -> Result<Vec<u8>, String> {
    let path = decode_node_id(&id)?;
    if path.is_empty() {
        return Err("root cannot be opened as text".to_string());
    }
    let disk = primary_root().map_err(ToString::to_string)?;
    let info = crate::r::fs::trueosfs::file_info_async(disk, &path)
        .await
        .map_err(|err| format!("file info failed: {err:?}"))?
        .ok_or_else(|| format!("'{path}' was not found or is not a file"))?;
    if info.data_len > FILEEXPLORER_TEXT_OPEN_MAX {
        return Err(format!(
            "file is too large to open as text ({} > {})",
            info.data_len, FILEEXPLORER_TEXT_OPEN_MAX
        ));
    }
    crate::r::fs::trueosfs::file_out_async(disk, &path)
        .await
        .map_err(|err| format!("read failed: {err:?}"))?
        .ok_or_else(|| format!("'{path}' was not found"))
}

async fn update_node_job(id: String, patch: UpdateNodeRequest) -> Result<Value, String> {
    let old_path = decode_node_id(&id)?;
    let Some(name) = patch.name else {
        return Err("no supported update fields".to_string());
    };
    validate_name(&name)?;
    let new_path = join_path(&parent_path(&old_path), name.trim());
    rename_tree(&old_path, &new_path).await?;
    Ok(serde_json::json!({
        "nodeId": encode_node_id(&new_path),
        "path": new_path,
    }))
}

async fn delete_nodes_job(ids: Vec<String>) -> Result<Value, String> {
    let mut paths = Vec::new();
    for id in ids.iter() {
        let path = decode_node_id(id)?;
        delete_tree(&path).await?;
        paths.push(path);
    }
    Ok(serde_json::json!({ "deleted": paths }))
}

async fn move_nodes_job(request: MoveNodesRequest) -> Result<Value, String> {
    let moves = if request.moves.is_empty() {
        let target_parent = request
            .target_parent_id
            .ok_or_else(|| "missing targetParentId".to_string())?;
        request
            .ids
            .into_iter()
            .map(|id| MoveInstruction {
                node_id: id,
                new_parent_id: target_parent.clone(),
            })
            .collect::<Vec<_>>()
    } else {
        request.moves
    };

    let mut moved = Vec::new();
    for item in moves.iter() {
        let src = decode_node_id(&item.node_id)?;
        let parent = decode_node_id(&item.new_parent_id)?;
        if src.is_empty() || src == parent || is_child_of(&parent, &src) {
            return Err(format!("invalid move from '{src}' to '{parent}'"));
        }
        let dst = join_path(&parent, &base_name(&src));
        rename_tree(&src, &dst).await?;
        moved.push(serde_json::json!({
            "from": src,
            "to": dst,
            "nodeId": encode_node_id(&dst),
        }));
    }
    Ok(serde_json::json!({ "moved": moved }))
}

async fn record_job(
    state: AppState,
    operation: &str,
    label: String,
    affected_node_ids: Vec<String>,
    result: Result<Value, String>,
) -> Response {
    let seq = JOB_SEQ.fetch_add(1, Ordering::Relaxed).max(1);
    let id = format!("trueosfs-job-{seq}");
    let now = now_ms();
    let (status, description, result, error) = match result {
        Ok(value) => ("succeeded", "Committed".to_string(), Some(value), None),
        Err(error) => ("failed", "Failed".to_string(), None, Some(error)),
    };
    let record = JobRecord {
        id: id.clone(),
        operation: operation.to_string(),
        status,
        progress: 100,
        description,
        affected_node_ids,
        created_at_ms: now,
        updated_at_ms: now,
        result,
        error,
    };
    state.jobs.write().await.insert(id.clone(), record);
    json_response(
        202,
        &AcceptedJob {
            job_id: id.clone(),
            label,
            status_url: format!("/api/jobs/{id}"),
            events_url: "/api/jobs/events".to_string(),
        },
    )
}

async fn handle_index() -> Response {
    text_response(200, "text/html; charset=utf-8", &index_html())
}

async fn handle_healthz() -> Response {
    json_response(
        200,
        &serde_json::json!({
            "ok": true,
            "service": "fileexplorer-http",
            "port": FILEEXPLORER_HTTP_TCP_PORT,
            "ports": FILEEXPLORER_HTTP_TCP_PORTS,
            "readiness": crate::r::readiness::mask(),
        }),
    )
}

async fn handle_tree(OriginalUri(uri): OriginalUri) -> Response {
    let root_id = uri.query().and_then(|query| {
        query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == "rootId").then(|| value.to_string())
        })
    });
    let path = match root_id {
        Some(id) => match decode_node_id(&id) {
            Ok(path) => path,
            Err(err) => return error_response(400, err),
        },
        None => String::new(),
    };
    match tree_for_path(path).await {
        Ok(snapshot) => json_response(200, &snapshot),
        Err(err) => error_response(500, err),
    }
}

async fn handle_tree_route(uri: OriginalUri) -> Response {
    run_local(move || handle_tree(uri)).await
}

async fn handle_replace_tree(State(state): State<AppState>) -> Response {
    let result = Err("replace-tree is not wired to TRUEOSFS yet".to_string());
    record_job(state, "tree_replace", "Replace tree".to_string(), vec!["root".to_string()], result)
        .await
}

async fn handle_create_node(State(state): State<AppState>, body: Bytes) -> Response {
    if body.len() > FILEEXPLORER_HTTP_BODY_MAX {
        return error_response(413, "request too large");
    }
    let request = match serde_json::from_slice::<CreateNodeRequest>(&body) {
        Ok(request) => request,
        Err(_) => return error_response(400, "bad json"),
    };
    let affected = vec![request.parent_id.clone()];
    let label = format!("Create {}", request.name);
    run_local(move || async move {
        let result = create_node_job(request).await;
        record_job(state, "node_create", label, affected, result).await
    })
    .await
}

async fn handle_update_node(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    if body.len() > FILEEXPLORER_HTTP_BODY_MAX {
        return error_response(413, "request too large");
    }
    let patch = match serde_json::from_slice::<UpdateNodeRequest>(&body) {
        Ok(patch) => patch,
        Err(_) => return error_response(400, "bad json"),
    };
    let affected = vec![id.clone()];
    let label = format!("Update {id}");
    run_local(move || async move {
        let result = update_node_job(id, patch).await;
        record_job(state, "node_update", label, affected, result).await
    })
    .await
}

async fn handle_delete_node(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let affected = vec![id.clone()];
    let label = format!("Delete {id}");
    run_local(move || async move {
        let result = delete_nodes_job(vec![id]).await;
        record_job(state, "node_delete", label, affected, result).await
    })
    .await
}

async fn handle_delete_nodes(State(state): State<AppState>, body: Bytes) -> Response {
    if body.len() > FILEEXPLORER_HTTP_BODY_MAX {
        return error_response(413, "request too large");
    }
    let request = match serde_json::from_slice::<DeleteNodesRequest>(&body) {
        Ok(request) => request,
        Err(_) => return error_response(400, "bad json"),
    };
    let affected = request.ids.clone();
    let label = format!("Delete {} item(s)", affected.len());
    run_local(move || async move {
        let result = delete_nodes_job(request.ids).await;
        record_job(state, "node_delete", label, affected, result).await
    })
    .await
}

async fn handle_move_nodes(State(state): State<AppState>, body: Bytes) -> Response {
    if body.len() > FILEEXPLORER_HTTP_BODY_MAX {
        return error_response(413, "request too large");
    }
    let request = match serde_json::from_slice::<MoveNodesRequest>(&body) {
        Ok(request) => request,
        Err(_) => return error_response(400, "bad json"),
    };
    let affected = if request.ids.is_empty() {
        request
            .moves
            .iter()
            .map(|item| item.node_id.clone())
            .collect()
    } else {
        request.ids.clone()
    };
    let label = format!("Move {} item(s)", affected.len());
    run_local(move || async move {
        let result = move_nodes_job(request).await;
        record_job(state, "multi_move", label, affected, result).await
    })
    .await
}

async fn handle_upload_file(
    State(state): State<AppState>,
    Path(parent_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if body.len() > FILEEXPLORER_UPLOAD_BODY_MAX {
        return error_response(413, "upload too large");
    }
    let name = match headers
        .get("x-file-name")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(name) => match percent_decode(name) {
            Ok(name) => name,
            Err(err) => return error_response(400, err),
        },
        None => return error_response(400, "missing x-file-name"),
    };
    let affected = vec![parent_id.clone()];
    let label = format!("Upload {name}");
    run_local(move || async move {
        let result = upload_file_job(parent_id, name, body.to_vec()).await;
        record_job(state, "node_upload", label, affected, result).await
    })
    .await
}

async fn handle_download_file(Path(id): Path<String>) -> Response {
    run_local(move || async move {
        match download_file(id).await {
            Ok((name, content_type, bytes)) => file_response(content_type, &name, bytes),
            Err(err) => error_response(404, err),
        }
    })
    .await
}

async fn handle_node_content(Path(id): Path<String>) -> Response {
    run_local(move || async move {
        match read_text_file(id).await {
            Ok(bytes) => response(200, "text/plain; charset=utf-8", bytes),
            Err(err) => error_response(400, err),
        }
    })
    .await
}

async fn handle_job(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.jobs.read().await.get(&id).cloned() {
        Some(record) => json_response(200, &record),
        None => error_response(404, "job not found"),
    }
}

async fn handle_job_events() -> Response {
    text_response(200, "text/event-stream; charset=utf-8", "event: ready\ndata: {\"ok\":true}\n\n")
}

pub fn router() -> Router {
    let state = AppState {
        jobs: Arc::new(RwLock::new(BTreeMap::new())),
    };
    Router::new()
        .route("/", get(handle_index))
        .route("/index.html", get(handle_index))
        .route("/healthz", get(handle_healthz))
        .route("/api/healthz", get(handle_healthz))
        .route("/api/tree", get(handle_tree_route).put(handle_replace_tree))
        .route("/api/nodes", post(handle_create_node))
        .route("/api/nodes/{id}", patch(handle_update_node).delete(handle_delete_node))
        .route("/api/nodes/{id}/content", get(handle_node_content))
        .route("/api/nodes/{id}/download", get(handle_download_file))
        .route("/api/nodes/{id}/upload", post(handle_upload_file))
        .route("/api/nodes/delete", post(handle_delete_nodes))
        .route("/api/nodes/move", post(handle_move_nodes))
        .route("/api/jobs/{id}", get(handle_job))
        .route("/api/jobs/events", get(handle_job_events))
        .layer(DefaultBodyLimit::max(FILEEXPLORER_UPLOAD_BODY_MAX))
        .with_state(state)
}

fn primary_ipv4_addr(port: u16) -> Option<SocketAddr> {
    let dev_idx = crate::net::primary_device_index();
    let ip = crate::net::adapter::ipv4_at(dev_idx)?;
    Some(SocketAddr::from((ip, port)))
}

async fn serve_fileexplorer_port(app: Router, port: u16) {
    loop {
        let Some(addr) = primary_ipv4_addr(port) else {
            crate::log!("fileexplorer-http: waiting for primary ipv4 port={}\n", port);
            tokio::time::sleep(core::time::Duration::from_millis(100)).await;
            continue;
        };

        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(err) => {
                crate::log!(
                    "fileexplorer-http: bind {} failed port={} kind={:?} err={}\n",
                    addr,
                    port,
                    err.kind(),
                    err
                );
                tokio::time::sleep(core::time::Duration::from_millis(1000)).await;
                continue;
            }
        };

        if port == FILEEXPLORER_HTTP_TCP_PORT {
            FILEEXPLORER_HTTP_PORT.store(addr.port(), Ordering::Release);
        }
        crate::log!("fileexplorer-http: axum listening on http://{}/\n", addr);
        let listener = listener
            .tap_io(move |_| crate::log!("fileexplorer-http: tcp accepted port={}\n", port));
        if let Err(err) = axum::serve(listener, app.clone()).await {
            if port == FILEEXPLORER_HTTP_TCP_PORT {
                FILEEXPLORER_HTTP_PORT.store(0, Ordering::Release);
            }
            crate::log!(
                "fileexplorer-http: serve failed port={} kind={:?} err={}\n",
                port,
                err.kind(),
                err
            );
        }
        tokio::time::sleep(core::time::Duration::from_millis(1000)).await;
    }
}

async fn fileexplorer_http_runtime() -> Result<(), io::Error> {
    tokio::task::spawn_local(crate::t::shared_tokio_job_pump());

    let app = router();
    for port in FILEEXPLORER_HTTP_TCP_PORTS {
        tokio::task::spawn_local(serve_fileexplorer_port(app.clone(), port));
    }
    core::future::pending::<Result<(), io::Error>>().await
}

fn run_fileexplorer_http_runtime() -> Result<(), io::Error> {
    let mut builder = tokio::runtime::Builder::new_current_thread();
    builder.enable_io();
    builder.enable_time();
    let runtime = builder.build()?;
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, fileexplorer_http_runtime())
}

#[embassy_executor::task]
pub async fn fileexplorer_http_service_task() {
    crate::r::readiness::wait_for(
        crate::r::readiness::NET_V4_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
    )
    .await;
    crate::log!(
        "fileexplorer-http: launching Tokio runtime after NET_V4_CONFIGURED+TRUEOSFS_ROOT_MOUNTED\n"
    );
    if crate::tst_chatserver::current_port().is_none() {
        crate::log!(
            "fileexplorer-http: waiting {}ms for chat-http listener priority\n",
            FILEEXPLORER_CHAT_GRACE_MS
        );
        Timer::after(EmbassyDuration::from_millis(FILEEXPLORER_CHAT_GRACE_MS)).await;
    }

    loop {
        let rc = crate::trueos_tokio_worker::spawn_blocking_job_with_purpose(
            Box::new(|| {
                if let Err(err) = run_fileexplorer_http_runtime() {
                    crate::log!("fileexplorer-http: runtime failed {:?}\n", err);
                }
            }),
            "fileexplorer-http-runtime",
        );
        if rc == 0 {
            crate::log!("fileexplorer-http: submitted Tokio runtime to blocking lane\n");
            core::future::pending::<()>().await;
        }
        crate::log!(
            "fileexplorer-http: blocking lane unavailable rc={} retry={}ms\n",
            rc,
            FILEEXPLORER_BLOCKING_LANE_RETRY_MS
        );
        Timer::after(EmbassyDuration::from_millis(FILEEXPLORER_BLOCKING_LANE_RETRY_MS)).await;
    }
}
