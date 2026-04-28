extern crate alloc;

use alloc::{
    collections::VecDeque,
    format,
    string::{String, ToString},
    vec::Vec,
};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use embedded_websocket::{
    WebSocketKey, WebSocketReceiveMessageType, WebSocketSendMessageType, WebSocketServer,
};
use spin::Mutex;
use v::vnet as api;

use crate::r::net::{VNet, ports};

const HTML: &str = include_str!("code.html");
const WS_PATH: &str = "/localcoder";
const ROOT_DIR: &str = "rustprojs";
const MANIFEST_PATH: &str = "rustprojs/.projects";
const RX_BUF_MAX: usize = 96 * 1024;
const TX_BUF_MAX: usize = 16 * 1024;
const FRAME_BUF_MAX: usize = 64 * 1024;
const COMPILE_QUEUE_CAP: usize = 32;
const RESULT_QUEUE_CAP: usize = 96;
const WORKER_COUNT: u32 = 3;

static WORKERS_STARTED: AtomicBool = AtomicBool::new(false);
static NEXT_JOB_ID: AtomicU32 = AtomicU32::new(1);
static COMPILE_JOBS: Mutex<VecDeque<CompileJob>> = Mutex::new(VecDeque::new());
static COMPILE_RESULTS: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

struct Session {
    handle: api::NetHandle,
    ws: WebSocketServer,
    rx: Vec<u8>,
    open: bool,
}

impl Session {
    fn new(handle: api::NetHandle) -> Self {
        Self {
            handle,
            ws: WebSocketServer::new_server(),
            rx: Vec::new(),
            open: false,
        }
    }
}

struct CompileJob {
    id: u32,
    project: String,
    file: String,
    content: String,
    run: bool,
}

fn push_json_string(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn json_msg(kind: &str, ok: bool, message: &str) -> String {
    let mut out = format!("{{\"type\":\"{}\",\"ok\":{}", kind, if ok { "true" } else { "false" });
    out.push_str(",\"message\":");
    push_json_string(&mut out, message);
    out.push('}');
    out
}

fn json_job(job_id: u32, phase: &str, ok: bool, message: &str, stats: &str) -> String {
    let mut out = format!(
        "{{\"type\":\"compile_result\",\"job_id\":{},\"phase\":\"{}\",\"ok\":{}",
        job_id,
        phase,
        if ok { "true" } else { "false" }
    );
    out.push_str(",\"message\":");
    push_json_string(&mut out, message);
    if !stats.is_empty() {
        out.push_str(",\"stats\":");
        out.push_str(stats);
    }
    out.push('}');
    out
}

fn queue_result(msg: String) {
    let mut q = COMPILE_RESULTS.lock();
    if q.len() >= RESULT_QUEUE_CAP {
        q.pop_front();
    }
    q.push_back(msg);
}

fn sanitize_name(raw: &str) -> Option<String> {
    let name = raw.trim();
    if name.is_empty() || name.len() > 64 {
        return None;
    }
    if name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return None;
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
    {
        return None;
    }
    Some(name.to_string())
}

fn project_file_path(project: &str, file: &str) -> String {
    format!("{}/{}/{}", ROOT_DIR, project, file)
}

fn disk() -> Option<crate::disc::block::DeviceHandle> {
    crate::r::fs::trueosfs::primary_root_handle()
}

async fn read_text(path: &str) -> Option<String> {
    let disk = disk()?;
    let bytes = crate::r::fs::trueosfs::file_out_async(disk, path)
        .await
        .ok()??;
    Some(String::from_utf8_lossy(bytes.as_slice()).into_owned())
}

async fn write_text(path: &str, text: &str) -> Result<(), &'static str> {
    let Some(disk) = disk() else {
        return Err("no TRUEOSFS root mounted");
    };
    match crate::r::fs::trueosfs::file_in_async(disk, path, text.as_bytes()).await {
        Ok(true) => Ok(()),
        Ok(false) => Err("TRUEOSFS write returned false"),
        Err(_) => Err("TRUEOSFS write failed"),
    }
}

async fn delete_path(path: &str) {
    if let Some(disk) = disk() {
        let _ = crate::r::fs::trueosfs::file_delete_async(disk, path).await;
    }
}

async fn manifest_projects() -> Vec<String> {
    let Some(text) = read_text(MANIFEST_PATH).await else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        if let Some(name) = sanitize_name(line)
            && !out.iter().any(|p| p == &name)
        {
            out.push(name);
        }
    }
    out
}

async fn write_manifest(projects: &[String]) -> Result<(), &'static str> {
    let mut text = String::new();
    for name in projects {
        text.push_str(name);
        text.push('\n');
    }
    write_text(MANIFEST_PATH, text.as_str()).await
}

async fn create_project(name: &str) -> Result<String, &'static str> {
    let Some(name) = sanitize_name(name) else {
        return Err("bad project name");
    };
    let mut projects = manifest_projects().await;
    if !projects.iter().any(|p| p == &name) {
        projects.push(name.clone());
        write_manifest(projects.as_slice()).await?;
    }
    write_text(
        project_file_path(name.as_str(), "main.rs").as_str(),
        "fn main() {\n    println!(\"hello from TRUEOS\");\n}\n",
    )
    .await?;
    Ok(name)
}

async fn delete_project(name: &str) -> Result<String, &'static str> {
    let Some(name) = sanitize_name(name) else {
        return Err("bad project name");
    };
    let mut projects = manifest_projects().await;
    projects.retain(|p| p != &name);
    write_manifest(projects.as_slice()).await?;
    for file in ["main.rs", ".artifact.rs", ".artifact.vm", ".run.log"] {
        delete_path(project_file_path(name.as_str(), file).as_str()).await;
    }
    Ok(name)
}

fn extract_json_string(src: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\"", key);
    let pos = src.find(needle.as_str())?;
    let after = src[pos + needle.len()..].find(':')? + pos + needle.len() + 1;
    let bytes = src.as_bytes();
    let mut i = after;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if bytes.get(i) != Some(&b'"') {
        return None;
    }
    i += 1;
    let mut out = String::new();
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Some(out),
            b'\\' => {
                i += 1;
                match *bytes.get(i)? {
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    other => out.push(other as char),
                }
            }
            b => out.push(b as char),
        }
        i += 1;
    }
    None
}

fn extract_json_bool(src: &str, key: &str) -> bool {
    let needle = format!("\"{}\"", key);
    let Some(pos) = src.find(needle.as_str()) else {
        return false;
    };
    let Some(colon_rel) = src[pos + needle.len()..].find(':') else {
        return false;
    };
    let v = src[pos + needle.len() + colon_rel + 1..].trim_start();
    v.starts_with("true")
}

async fn handle_api_message(text: &str) -> String {
    let kind = extract_json_string(text, "type").unwrap_or_default();
    match kind.as_str() {
        "hello" | "list_projects" => {
            let projects = manifest_projects().await;
            let mut out = String::from("{\"type\":\"projects\",\"ok\":true,\"projects\":[");
            for (idx, p) in projects.iter().enumerate() {
                if idx != 0 {
                    out.push(',');
                }
                push_json_string(&mut out, p.as_str());
            }
            out.push_str("]}");
            out
        }
        "create_project" => match create_project(
            extract_json_string(text, "name")
                .unwrap_or_default()
                .as_str(),
        )
        .await
        {
            Ok(name) => {
                let mut out = json_msg("project_created", true, "project ready");
                out.insert_str(out.len() - 1, format!(",\"name\":\"{}\"", name).as_str());
                out
            }
            Err(e) => json_msg("project_created", false, e),
        },
        "delete_project" => match delete_project(
            extract_json_string(text, "name")
                .unwrap_or_default()
                .as_str(),
        )
        .await
        {
            Ok(name) => {
                let mut out = json_msg("project_deleted", true, "project deleted");
                out.insert_str(out.len() - 1, format!(",\"name\":\"{}\"", name).as_str());
                out
            }
            Err(e) => json_msg("project_deleted", false, e),
        },
        "save_file" => {
            let project =
                extract_json_string(text, "project").unwrap_or_else(|| "demo".to_string());
            let file = extract_json_string(text, "file").unwrap_or_else(|| "main.rs".to_string());
            let content = extract_json_string(text, "content").unwrap_or_default();
            let Some(project) = sanitize_name(project.as_str()) else {
                return json_msg("save_file", false, "bad project name");
            };
            let Some(file) = sanitize_name(file.as_str()) else {
                return json_msg("save_file", false, "bad file name");
            };
            match write_text(
                project_file_path(project.as_str(), file.as_str()).as_str(),
                content.as_str(),
            )
            .await
            {
                Ok(()) => json_msg("save_file", true, "saved"),
                Err(e) => json_msg("save_file", false, e),
            }
        }
        "compile" | "run" => {
            let project =
                extract_json_string(text, "project").unwrap_or_else(|| "demo".to_string());
            let file = extract_json_string(text, "file").unwrap_or_else(|| "main.rs".to_string());
            let content = extract_json_string(text, "content").unwrap_or_default();
            let Some(project) = sanitize_name(project.as_str()) else {
                return json_msg("compile_queued", false, "bad project name");
            };
            let Some(file) = sanitize_name(file.as_str()) else {
                return json_msg("compile_queued", false, "bad file name");
            };
            let job_id = NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed).max(1);
            let run = kind == "run" || extract_json_bool(text, "run");
            let mut q = COMPILE_JOBS.lock();
            if q.len() >= COMPILE_QUEUE_CAP {
                return json_msg("compile_queued", false, "compile queue full");
            }
            q.push_back(CompileJob {
                id: job_id,
                project,
                file,
                content,
                run,
            });
            format!(
                "{{\"type\":\"compile_queued\",\"ok\":true,\"job_id\":{},\"queue_depth\":{}}}",
                job_id,
                q.len()
            )
        }
        _ => json_msg("error", false, "unknown message type"),
    }
}

async fn run_compile_job(job: CompileJob) {
    queue_result(json_job(job.id, "started", true, "compile job started", ""));
    let path = project_file_path(job.project.as_str(), job.file.as_str());
    if let Err(e) = write_text(path.as_str(), job.content.as_str()).await {
        queue_result(json_job(job.id, "done", false, e, ""));
        return;
    }

    if job.file.ends_with(".c4") {
        match trueos_c4::parse_program(job.content.as_str()) {
            Ok(program) => match trueos_c4::emit_vm_object(&program) {
                Ok(vm) => {
                    let rust = trueos_c4::emit_rust(&program);
                    let _ = write_text(
                        project_file_path(job.project.as_str(), ".artifact.rs").as_str(),
                        rust.as_str(),
                    )
                    .await;
                    if let Some(disk) = disk() {
                        let _ = crate::r::fs::trueosfs::file_in_async(
                            disk,
                            project_file_path(job.project.as_str(), ".artifact.vm").as_str(),
                            vm.bytes.as_slice(),
                        )
                        .await;
                    }
                    let stats = format!(
                        "{{\"bytes\":{},\"code\":{},\"symbols\":{},\"stack\":{}}}",
                        vm.bytes.len(),
                        vm.code_len,
                        vm.symbol_count,
                        vm.stack_bytes
                    );
                    queue_result(json_job(
                        job.id,
                        "done",
                        true,
                        "C4 compiled to TC4O",
                        stats.as_str(),
                    ));
                    if job.run {
                        match trueos_c4::run_vm_object(vm.bytes.as_slice(), 100_000) {
                            Ok(report) => queue_result(json_job(
                                job.id,
                                "run",
                                true,
                                "TC4O run completed",
                                format!(
                                    "{{\"steps\":{},\"code\":{},\"symbols\":{},\"stack\":{}}}",
                                    report.steps,
                                    report.code_len,
                                    report.symbol_count,
                                    report.stack_bytes
                                )
                                .as_str(),
                            )),
                            Err(err) => queue_result(json_job(
                                job.id,
                                "run",
                                false,
                                format!("TC4O run failed: {:?}", err).as_str(),
                                "",
                            )),
                        }
                    }
                }
                Err(err) => queue_result(json_job(
                    job.id,
                    "done",
                    false,
                    format!("C4 VM emit failed: {:?}", err).as_str(),
                    "",
                )),
            },
            Err(err) => queue_result(json_job(
                job.id,
                "done",
                false,
                format!("C4 parse failed: {:?}", err).as_str(),
                "",
            )),
        }
        return;
    }

    let brace_delta = job.content.bytes().fold(0i32, |acc, b| match b {
        b'{' => acc + 1,
        b'}' => acc - 1,
        _ => acc,
    });
    if brace_delta != 0 {
        queue_result(json_job(
            job.id,
            "done",
            false,
            "Rust demo check failed: unbalanced braces",
            "",
        ));
        return;
    }

    let artifact = project_file_path(job.project.as_str(), ".artifact.rs");
    let _ = write_text(artifact.as_str(), job.content.as_str()).await;
    let lines = job.content.lines().count();
    let stats = format!(
        "{{\"bytes\":{},\"lines\":{},\"artifact\":\"{}\"}}",
        job.content.len(),
        lines,
        artifact
    );
    queue_result(json_job(
        job.id,
        "done",
        true,
        "Rust source saved and demo-checked; no in-kernel rustc yet",
        stats.as_str(),
    ));
    if job.run {
        queue_result(json_job(
            job.id,
            "run",
            true,
            "Run requested; Rust execution is stubbed for this rollback demo",
            "",
        ));
    }
}

#[embassy_executor::task(pool_size = 3)]
async fn compile_worker_task(_worker_id: u32) {
    loop {
        let job = COMPILE_JOBS.lock().pop_front();
        if let Some(job) = job {
            run_compile_job(job).await;
        } else {
            Timer::after(EmbassyDuration::from_millis(25)).await;
        }
    }
}

fn send_tcp_bytes(vnet: &VNet, handle: api::NetHandle, bytes: &[u8]) {
    for chunk in bytes.chunks(api::MAX_MSG) {
        let _ = vnet.submit(api::Command::SendTcp {
            handle,
            data: api::ByteBuf::from_slice_trunc(chunk),
        });
    }
}

fn send_ws_text(vnet: &VNet, session: &mut Session, text: &str) -> bool {
    let mut frame_buf = [0u8; TX_BUF_MAX];
    match session
        .ws
        .write(WebSocketSendMessageType::Text, true, text.as_bytes(), &mut frame_buf)
    {
        Ok(len) => {
            send_tcp_bytes(vnet, session.handle, &frame_buf[..len]);
            true
        }
        Err(_) => false,
    }
}

fn close_session(vnet: &VNet, handle: api::NetHandle) {
    let _ = vnet.submit(api::Command::Close { handle });
}

fn find_http_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn http_request_path(req: &str) -> Option<&str> {
    let line_end = req
        .find("\r\n")
        .or_else(|| req.find('\n'))
        .unwrap_or(req.len());
    let mut it = req.get(..line_end)?.split_whitespace();
    if it.next()? != "GET" {
        return None;
    }
    it.next()
}

fn http_header_value<'a>(req: &'a str, key: &str) -> Option<&'a str> {
    let mut lines = req.split('\n');
    let _ = lines.next()?;
    for line in lines {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':')
            && k.trim().eq_ignore_ascii_case(key)
        {
            return Some(v.trim());
        }
    }
    None
}

fn is_ws_upgrade(req: &str) -> bool {
    http_header_value(req, "Upgrade")
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false)
        && http_header_value(req, "Connection")
            .map(|v| {
                v.split(',')
                    .any(|part| part.trim().eq_ignore_ascii_case("Upgrade"))
            })
            .unwrap_or(false)
}

fn send_http(vnet: &VNet, handle: api::NetHandle, status: &str, content_type: &str, body: &[u8]) {
    let head = format!(
        "{}Content-Type: {}\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
        status,
        content_type,
        body.len()
    );
    send_tcp_bytes(vnet, handle, head.as_bytes());
    send_tcp_bytes(vnet, handle, body);
}

async fn try_open_or_http(vnet: &VNet, session: &mut Session) -> bool {
    let Some(header_end) = find_http_header_end(session.rx.as_slice()) else {
        return false;
    };
    let Ok(req) = core::str::from_utf8(&session.rx[..header_end]) else {
        close_session(vnet, session.handle);
        return true;
    };
    let path = http_request_path(req).unwrap_or("/");
    if path == WS_PATH && is_ws_upgrade(req) {
        let Some(key) = http_header_value(req, "Sec-WebSocket-Key") else {
            close_session(vnet, session.handle);
            return true;
        };
        let Ok(key) = WebSocketKey::try_from(key) else {
            close_session(vnet, session.handle);
            return true;
        };
        let mut response = [0u8; TX_BUF_MAX];
        let Ok(len) = session.ws.server_accept(&key, None, &mut response) else {
            close_session(vnet, session.handle);
            return true;
        };
        send_tcp_bytes(vnet, session.handle, &response[..len]);
        let remaining = session.rx.split_off(header_end);
        session.rx = remaining;
        session.open = true;
        let hello = handle_api_message("{\"type\":\"hello\"}").await;
        let _ = send_ws_text(vnet, session, hello.as_str());
        return false;
    }

    if path == "/" || path == "/index.html" {
        send_http(
            vnet,
            session.handle,
            "HTTP/1.1 200 OK\r\n",
            "text/html; charset=utf-8",
            HTML.as_bytes(),
        );
    } else {
        send_http(
            vnet,
            session.handle,
            "HTTP/1.1 404 Not Found\r\n",
            "text/plain; charset=utf-8",
            b"not found\n",
        );
    }
    true
}

async fn handle_ws_frames(vnet: &VNet, session: &mut Session) -> bool {
    let mut payload = [0u8; FRAME_BUF_MAX];
    loop {
        let res = match session.ws.read(session.rx.as_slice(), &mut payload) {
            Ok(v) => v,
            Err(embedded_websocket::Error::ReadFrameIncomplete) => break,
            Err(_) => {
                close_session(vnet, session.handle);
                return true;
            }
        };
        if res.len_from == 0 {
            break;
        }
        let remaining = session.rx.split_off(res.len_from);
        session.rx = remaining;
        match res.message_type {
            WebSocketReceiveMessageType::Text => {
                if let Ok(text) = core::str::from_utf8(&payload[..res.len_to]) {
                    let reply = handle_api_message(text).await;
                    if !send_ws_text(vnet, session, reply.as_str()) {
                        return true;
                    }
                }
            }
            WebSocketReceiveMessageType::Ping => {
                let mut frame_buf = [0u8; TX_BUF_MAX];
                if let Ok(len) = session.ws.write(
                    WebSocketSendMessageType::Pong,
                    true,
                    &payload[..res.len_to],
                    &mut frame_buf,
                ) {
                    send_tcp_bytes(vnet, session.handle, &frame_buf[..len]);
                }
            }
            WebSocketReceiveMessageType::CloseMustReply => {
                close_session(vnet, session.handle);
                return true;
            }
            WebSocketReceiveMessageType::CloseCompleted => {
                close_session(vnet, session.handle);
                return true;
            }
            WebSocketReceiveMessageType::Binary | WebSocketReceiveMessageType::Pong => {}
        }
    }
    false
}

#[embassy_executor::task]
pub async fn localcoder_web_task() {
    if WORKERS_STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        let spawner = unsafe { Spawner::for_current_executor().await };
        for id in 0..WORKER_COUNT {
            if let Ok(token) = compile_worker_task(id) {
                spawner.spawn(token);
            }
        }
    }

    let vnet = loop {
        if let Some(v) = VNet::open_primary() {
            break v;
        }
        Timer::after(EmbassyDuration::from_millis(50)).await;
    };

    let _ = vnet.submit(api::Command::OpenTcpListen {
        port: ports::LOCALCODER_WEB_TCP_PORT,
    });
    crate::log!(
        "localcoder-web: listening tcp {} http=/ ws={}\n",
        ports::LOCALCODER_WEB_TCP_PORT,
        WS_PATH
    );

    let mut listener: Option<api::NetHandle> = None;
    let mut sessions: Vec<Session> = Vec::new();

    loop {
        while let Some(ev) = vnet.pop_event() {
            match ev {
                api::Event::Opened { handle, kind } => {
                    if kind == api::SocketKind::Tcp && listener.is_none() {
                        listener = Some(handle);
                    }
                }
                api::Event::TcpEstablished { handle } => {
                    if !sessions.iter().any(|s| s.handle == handle) {
                        sessions.push(Session::new(handle));
                    }
                }
                api::Event::TcpData { handle, data } => {
                    if !sessions.iter().any(|s| s.handle == handle) {
                        sessions.push(Session::new(handle));
                    }
                    let Some(pos) = sessions.iter().position(|s| s.handle == handle) else {
                        continue;
                    };
                    if sessions[pos].rx.len().saturating_add(data.len()) > RX_BUF_MAX {
                        close_session(&vnet, handle);
                        sessions.remove(pos);
                        continue;
                    }
                    sessions[pos].rx.extend_from_slice(data.as_slice());
                    let closed = if sessions[pos].open {
                        handle_ws_frames(&vnet, &mut sessions[pos]).await
                    } else {
                        try_open_or_http(&vnet, &mut sessions[pos]).await
                    };
                    if closed {
                        sessions.remove(pos);
                    }
                }
                api::Event::Closed { handle } => {
                    sessions.retain(|s| s.handle != handle);
                    if Some(handle) == listener {
                        listener = None;
                        let _ = vnet.submit(api::Command::OpenTcpListen {
                            port: ports::LOCALCODER_WEB_TCP_PORT,
                        });
                    }
                }
                api::Event::Error { msg } => {
                    if msg != "bad handle" {
                        crate::log!("localcoder-web: error {}\n", msg);
                    }
                }
                api::Event::TcpSent { .. }
                | api::Event::UdpPacket { .. }
                | api::Event::UdpPacketV6 { .. }
                | api::Event::IcmpReply { .. }
                | api::Event::IcmpReplyV6 { .. } => {}
            }
        }

        while let Some(msg) = COMPILE_RESULTS.lock().pop_front() {
            let mut dead = Vec::new();
            for (idx, session) in sessions.iter_mut().enumerate() {
                if session.open && !send_ws_text(&vnet, session, msg.as_str()) {
                    dead.push(idx);
                }
            }
            for idx in dead.into_iter().rev() {
                let handle = sessions[idx].handle;
                close_session(&vnet, handle);
                sessions.remove(idx);
            }
        }

        Timer::after(EmbassyDuration::from_millis(5)).await;
    }
}
