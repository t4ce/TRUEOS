use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use embassy_executor::Spawner;
use regex_automata::meta::Regex;
use spin::Mutex;

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active,
};
use crate::disc::block::{self, DeviceHandle};
use crate::shell2::CommandSessionInputResult;
use crate::shell2::shell2_cmd::{CommandSessionKind, ParseOutcome};

static NEXT_REMOVE_SESSION_ID: AtomicU64 = AtomicU64::new(1);
static REMOVE_SESSIONS: Mutex<Vec<RemoveSession>> = Mutex::new(Vec::new());

#[derive(Clone)]
struct PendingRemove {
    label: String,
    files: Vec<String>,
    folder_count: usize,
    confirm_total: usize,
}

enum RemoveSessionState {
    Scanning,
    Ready(PendingRemove),
}

struct RemoveSession {
    id: u64,
    state: RemoveSessionState,
}

enum RemoveRequest {
    Path(String),
    Regex { base: String, pattern: String },
}

enum PendingState {
    Missing,
    Scanning,
    Ready(PendingRemove),
}

fn parse_args(rest: &str) -> Result<Vec<String>, &'static str> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for ch in rest.trim().chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            continue;
        }
        if ch.is_whitespace() {
            if !current.is_empty() {
                args.push(current);
                current = String::new();
            }
            continue;
        }
        current.push(ch);
    }

    if quote.is_some() {
        return Err("unterminated quote");
    }
    if escaped {
        current.push('\\');
    }
    if !current.is_empty() {
        args.push(current);
    }
    Ok(args)
}

fn normalize_path(path: &str, allow_empty: bool) -> Result<String, &'static str> {
    crate::r::path::FsPath::parse(path, allow_empty)
        .map(|path| path.to_relative_string())
        .map_err(|_| "bad path")
}

fn join_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        String::from(child)
    } else {
        alloc::format!("{parent}/{child}")
    }
}

fn root_disk() -> Result<DeviceHandle, &'static str> {
    crate::r::fs::trueosfs::primary_root_handle().ok_or("no TRUEOSFS root")
}

// Shell2 is itself polled by the BSP executor. Every filesystem probe used to
// prepare `rm` must remain a native future; blocking this executor here can
// prevent the USB completion needed by the probe from ever being delivered.
async fn file_exists(disk: DeviceHandle, path: &str) -> Result<bool, block::Error> {
    crate::r::fs::trueosfs::file_info_async(disk, path)
        .await
        .map(|info| info.is_some())
}

async fn dir_exists(disk: DeviceHandle, path: &str) -> Result<bool, block::Error> {
    if path.is_empty() {
        return Ok(true);
    }
    let marker = alloc::format!("{path}/.keep");
    if crate::r::fs::trueosfs::file_exists_async(disk, marker.as_str()).await? {
        return Ok(true);
    }
    crate::r::fs::trueosfs::dir_has_children_async(disk, path).await
}

async fn list_dir(disk: DeviceHandle, path: &str) -> Result<Vec<String>, block::Error> {
    let Some(listing) = crate::r::fs::trueosfs::list_dir_async(disk, path).await? else {
        return Ok(Vec::new());
    };
    Ok(listing
        .lines()
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect())
}

async fn collect_dir(
    disk: DeviceHandle,
    path: &str,
    files: &mut Vec<String>,
    folders: &mut usize,
) -> Result<(), block::Error> {
    // An explicit stack keeps this an ordinary sized future without boxed
    // recursive async calls.
    let mut pending_dirs = alloc::vec![String::from(path)];
    while let Some(dir) = pending_dirs.pop() {
        *folders = folders.saturating_add(1);
        for child in list_dir(disk, dir.as_str()).await? {
            let child_path = join_path(dir.as_str(), child.as_str());
            if file_exists(disk, child_path.as_str()).await? {
                files.push(child_path);
            } else if dir_exists(disk, child_path.as_str()).await? {
                pending_dirs.push(child_path);
            }
        }
    }
    Ok(())
}

async fn collect_one(
    disk: DeviceHandle,
    path: &str,
) -> Result<Option<PendingRemove>, block::Error> {
    if file_exists(disk, path).await? {
        return Ok(Some(PendingRemove {
            label: String::from(path),
            files: alloc::vec![String::from(path)],
            folder_count: 0,
            confirm_total: 0,
        }));
    }

    if !dir_exists(disk, path).await? {
        return Ok(None);
    }

    let mut files = Vec::new();
    let mut folders = 0;
    collect_dir(disk, path, &mut files, &mut folders).await?;
    let confirm_total = folders.saturating_add(files.len());
    Ok(Some(PendingRemove {
        label: if path.is_empty() {
            String::from("/")
        } else {
            String::from(path)
        },
        files,
        folder_count: folders,
        confirm_total,
    }))
}

async fn collect_regex(
    disk: DeviceHandle,
    base: &str,
    pattern: &str,
) -> Result<Option<PendingRemove>, &'static str> {
    let regex = Regex::new(pattern).map_err(|_| "bad regex")?;
    if !dir_exists(disk, base)
        .await
        .map_err(|_| "filesystem error")?
    {
        return Ok(None);
    }

    let mut files = Vec::new();
    let mut folders = 0usize;
    let mut selected = 0usize;
    for child in list_dir(disk, base).await.map_err(|_| "filesystem error")? {
        let child_path = join_path(base, child.as_str());
        if !regex.is_match(child.as_str()) && !regex.is_match(child_path.as_str()) {
            continue;
        }
        selected = selected.saturating_add(1);
        if file_exists(disk, child_path.as_str())
            .await
            .map_err(|_| "filesystem error")?
        {
            files.push(child_path);
        } else if dir_exists(disk, child_path.as_str())
            .await
            .map_err(|_| "filesystem error")?
        {
            collect_dir(disk, child_path.as_str(), &mut files, &mut folders)
                .await
                .map_err(|_| "filesystem error")?;
        }
    }

    if selected == 0 {
        return Ok(None);
    }
    let confirm_total = folders.saturating_add(files.len());
    Ok(Some(PendingRemove {
        label: alloc::format!("{} -regx {pattern}", if base.is_empty() { "." } else { base }),
        files,
        folder_count: folders,
        confirm_total,
    }))
}

fn begin_pending() -> u64 {
    let id = NEXT_REMOVE_SESSION_ID.fetch_add(1, Ordering::Relaxed);
    REMOVE_SESSIONS.lock().push(RemoveSession {
        id,
        state: RemoveSessionState::Scanning,
    });
    id
}

fn complete_pending(id: u64, pending: PendingRemove) -> bool {
    let mut sessions = REMOVE_SESSIONS.lock();
    let Some(session) = sessions.iter_mut().find(|session| session.id == id) else {
        return false;
    };
    session.state = RemoveSessionState::Ready(pending);
    true
}

fn discard_pending(id: u64) {
    let mut sessions = REMOVE_SESSIONS.lock();
    if let Some(idx) = sessions.iter().position(|session| session.id == id) {
        sessions.remove(idx);
    }
}

fn take_pending(id: u64) -> PendingState {
    let mut sessions = REMOVE_SESSIONS.lock();
    let Some(idx) = sessions.iter().position(|session| session.id == id) else {
        return PendingState::Missing;
    };
    if matches!(sessions[idx].state, RemoveSessionState::Scanning) {
        return PendingState::Scanning;
    }
    match sessions.remove(idx).state {
        RemoveSessionState::Ready(pending) => PendingState::Ready(pending),
        RemoveSessionState::Scanning => PendingState::Scanning,
    }
}

pub(crate) fn session_exists(id: u64) -> bool {
    REMOVE_SESSIONS
        .lock()
        .iter()
        .any(|session| session.id == id)
}

fn print_confirmation(target: &MatrixTarget, name: &str, pending: &PendingRemove) {
    if pending.folder_count == 0 && pending.files.len() == 1 {
        print_matrix_target_line(
            target,
            alloc::format!("{name}: remove {}?", pending.label).as_str(),
        );
        print_matrix_target_line(target, alloc::format!("{name}: type `sure`").as_str());
    } else {
        print_matrix_target_line(
            target,
            alloc::format!(
                "{name}: {} contains {} folders + {} files = {} entries",
                pending.label,
                pending.folder_count,
                pending.files.len(),
                pending.confirm_total
            )
            .as_str(),
        );
        print_matrix_target_line(
            target,
            alloc::format!("{name}: type `sure {}`", pending.confirm_total).as_str(),
        );
    }
}

fn print_usage(io: &'static dyn ShellBackend2, name: &str) {
    print_shell_line(
        io,
        alloc::format!("{name}: usage `{name} <file-or-dir>` | `{name} -regx <pattern> [dir]`")
            .as_str(),
    );
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    name: &str,
    rest: &str,
) -> ParseOutcome {
    let args = match parse_args(rest) {
        Ok(args) => args,
        Err(err) => {
            print_shell_line(io, alloc::format!("{name}: {err}").as_str());
            return ParseOutcome::Handled;
        }
    };

    if args.is_empty()
        || args
            .iter()
            .any(|arg| arg == "-h" || arg == "--help" || arg == "help")
    {
        print_usage(io, name);
        return ParseOutcome::Handled;
    }

    let disk = match root_disk() {
        Ok(disk) => disk,
        Err(err) => {
            print_shell_line(io, alloc::format!("{name}: {err}").as_str());
            return ParseOutcome::Handled;
        }
    };

    let request = if args.first().map(|arg| arg.as_str()) == Some("-regx") {
        if args.len() < 2 || args.len() > 3 {
            print_usage(io, name);
            return ParseOutcome::Handled;
        }
        if Regex::new(args[1].as_str()).is_err() {
            print_shell_line(io, alloc::format!("{name}: bad regex").as_str());
            return ParseOutcome::Handled;
        }
        let base = match normalize_path(args.get(2).map(|arg| arg.as_str()).unwrap_or("."), true) {
            Ok(path) => path,
            Err(err) => {
                print_shell_line(io, alloc::format!("{name}: {err}").as_str());
                return ParseOutcome::Handled;
            }
        };
        RemoveRequest::Regex {
            base,
            pattern: args[1].clone(),
        }
    } else {
        if args.len() != 1 {
            print_usage(io, name);
            return ParseOutcome::Handled;
        }
        let path = match normalize_path(args[0].as_str(), true) {
            Ok(path) => path,
            Err(err) => {
                print_shell_line(io, alloc::format!("{name}: {err}").as_str());
                return ParseOutcome::Handled;
            }
        };
        if path.is_empty() {
            print_shell_line(
                io,
                alloc::format!("{name}: refusing to remove filesystem root").as_str(),
            );
            return ParseOutcome::Handled;
        }
        RemoveRequest::Path(path)
    };

    let id = begin_pending();
    let target = matrix_target_for_backend(io);
    print_shell_line(io, alloc::format!("{name}: scanning selection...").as_str());
    set_matrix_target_active(&target, true);
    match prepare_remove_task(target.clone(), disk, String::from(name), request, id) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            discard_pending(id);
            set_matrix_target_active(&target, false);
            print_shell_line(io, alloc::format!("{name}: scan task unavailable").as_str());
            return ParseOutcome::Handled;
        }
    }
    ParseOutcome::StartSession(CommandSessionKind::RemoveSure(id))
}

#[embassy_executor::task(pool_size = 2)]
async fn prepare_remove_task(
    target: MatrixTarget,
    disk: DeviceHandle,
    name: String,
    request: RemoveRequest,
    session_id: u64,
) {
    let no_match = match &request {
        RemoveRequest::Path(path) => alloc::format!("{}: {}: not found", name, path),
        RemoveRequest::Regex { .. } => alloc::format!("{}: no regex matches", name),
    };
    let pending = match request {
        RemoveRequest::Path(path) => collect_one(disk, path.as_str())
            .await
            .map_err(|err| alloc::format!("{:?}", err)),
        RemoveRequest::Regex { base, pattern } => {
            collect_regex(disk, base.as_str(), pattern.as_str())
                .await
                .map_err(String::from)
        }
    };

    match pending {
        Ok(Some(pending)) => {
            if complete_pending(session_id, pending.clone()) {
                print_confirmation(&target, name.as_str(), &pending);
            }
        }
        Ok(None) => {
            discard_pending(session_id);
            print_matrix_target_line(&target, no_match.as_str());
        }
        Err(err) => {
            discard_pending(session_id);
            print_matrix_target_line(
                &target,
                alloc::format!("{}: filesystem scan failed: {}", name, err).as_str(),
            );
        }
    }
    set_matrix_target_active(&target, false);
}

pub(crate) fn handle_session_input(
    spawner: &Spawner,
    target: &MatrixTarget,
    submitted: &str,
    session_id: u64,
) -> CommandSessionInputResult {
    let pending = match take_pending(session_id) {
        PendingState::Ready(pending) => pending,
        PendingState::Scanning => {
            print_matrix_target_line(
                target,
                "rm: scan still running; wait for confirmation prompt",
            );
            return CommandSessionInputResult::KeepRunning;
        }
        PendingState::Missing => {
            print_matrix_target_line(target, "rm: session expired");
            return CommandSessionInputResult::CompleteIdle;
        }
    };

    let expected = if pending.folder_count == 0 && pending.files.len() == 1 {
        String::from("sure")
    } else {
        alloc::format!("sure {}", pending.confirm_total)
    };

    if !submitted.trim().eq_ignore_ascii_case(expected.as_str()) {
        print_matrix_target_line(target, "rm: cancelled");
        return CommandSessionInputResult::CompleteIdle;
    }

    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        print_matrix_target_line(target, "rm: no TRUEOSFS root");
        return CommandSessionInputResult::CompleteIdle;
    };

    print_matrix_target_line(
        target,
        alloc::format!("rm: removing {} files...", pending.files.len()).as_str(),
    );
    set_matrix_target_active(target, true);
    match remove_command_task(target.clone(), disk, pending) {
        Ok(token) => {
            spawner.spawn(token);
            CommandSessionInputResult::CompleteRunning
        }
        Err(_) => {
            set_matrix_target_active(target, false);
            print_matrix_target_line(target, "rm: spawn failed");
            CommandSessionInputResult::CompleteIdle
        }
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn remove_command_task(target: MatrixTarget, disk: DeviceHandle, pending: PendingRemove) {
    let mut removed = 0usize;
    let mut missed = 0usize;
    for path in pending.files.iter().rev() {
        match crate::r::fs::trueosfs::file_delete_async(disk, path.as_str()).await {
            Ok(true) => removed = removed.saturating_add(1),
            Ok(false) => missed = missed.saturating_add(1),
            Err(err) => {
                print_matrix_target_line(&target, alloc::format!("rm: {path}: {:?}", err).as_str());
                missed = missed.saturating_add(1);
            }
        }
    }

    print_matrix_target_line(
        &target,
        alloc::format!("rm: removed {removed} files, {missed} missed").as_str(),
    );
    set_matrix_target_active(&target, false);
}
