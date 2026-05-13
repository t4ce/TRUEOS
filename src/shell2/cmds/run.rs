use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::str::SplitWhitespace;

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use super::super::{
    MatrixTarget, ShellBackend2, UART1_COM1_BACKEND, line_width_for_backend,
    matrix_target_for_backend, print_matrix_target_line, print_shell_line,
    set_matrix_target_active,
};
use super::tlb_helper::TlbTable;
use crate::shell2::shell2_cmd::ParseOutcome;

const TABLE_HEADERS: &[&str; 3] = &["id", "module", "source"];

static APP_VM_RUN_QUEUE: Mutex<VecDeque<AppVmLaunchRequest>> = Mutex::new(VecDeque::new());

#[derive(Clone)]
struct AppVmLaunchRequest {
    archive: String,
    module_bytes: Vec<u8>,
    app_args: Vec<String>,
    target: MatrixTarget,
}

#[derive(Clone)]
enum ArchiveSource {
    TrueosfsRoot,
    EmbeddedModule { cmdline: String },
}

#[derive(Clone)]
struct ArchiveEntry {
    archive: String,
    source: ArchiveSource,
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "hv run: usage `hv run` or `hv run <id> [args...]`");
}

fn embedded_archive_name(cmdline: &[u8]) -> Option<String> {
    let suffix = cmdline.strip_prefix(b"trueos.app.")?;
    if suffix.is_empty() {
        return None;
    }
    let mut archive = String::from_utf8_lossy(suffix).into_owned();
    archive.push_str(".bp");
    Some(archive)
}

fn root_archives() -> Result<Vec<ArchiveEntry>, &'static str> {
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        return Ok(Vec::new());
    };

    let listing = crate::wait::spawn_and_wait_local(async move {
        crate::r::fs::trueosfs::list_dir_async(disk, "").await
    })
    .map_err(|_| "root listing failed")?
    .ok_or("root is not TRUEOSFS")?;

    let mut out = listing
        .lines()
        .map(str::trim)
        .filter(|name| is_runnable_root_artifact(name))
        .map(|name| ArchiveEntry {
            archive: String::from(name),
            source: ArchiveSource::TrueosfsRoot,
        })
        .collect::<Vec<_>>();
    out.sort_by(|a, b| a.archive.cmp(&b.archive));
    Ok(out)
}

fn is_runnable_root_artifact(name: &str) -> bool {
    matches_glob(name, "*.bp") || matches_glob(name, "*.vm")
}

fn matches_glob(name: &str, pattern: &str) -> bool {
    if let Some(suffix) = pattern.strip_prefix('*') {
        name.ends_with(suffix)
    } else {
        name == pattern
    }
}

fn embedded_archives() -> Vec<ArchiveEntry> {
    let Some(resp) = crate::limine::MODULE_REQUEST.response() else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for module in resp.modules().iter() {
        let cmdline = module.cmdline().as_bytes();
        let Some(archive) = embedded_archive_name(cmdline) else {
            continue;
        };
        out.push(ArchiveEntry {
            archive,
            source: ArchiveSource::EmbeddedModule {
                cmdline: String::from_utf8_lossy(cmdline).into_owned(),
            },
        });
    }
    out.sort_by(|a, b| a.archive.cmp(&b.archive));
    out
}

fn embedded_module_bytes_by_archive_name(
    archive_name: &str,
) -> Result<Option<Vec<u8>>, &'static str> {
    let Some(resp) = crate::limine::MODULE_REQUEST.response() else {
        return Ok(None);
    };

    for module in resp.modules().iter() {
        let cmdline = module.cmdline().as_bytes();
        let Some(archive) = embedded_archive_name(cmdline) else {
            continue;
        };
        if archive.as_str() != archive_name {
            continue;
        }
        let Some(module_bytes) = crate::limine::module_bytes_by_string(cmdline) else {
            return Err("failed to read selected embedded module");
        };
        return Ok(Some(module_bytes.to_vec()));
    }

    Ok(None)
}

fn archive_entries() -> Result<Vec<ArchiveEntry>, &'static str> {
    let mut out = root_archives()?;
    for entry in embedded_archives() {
        if !out.iter().any(|existing| existing.archive == entry.archive) {
            out.push(entry);
        }
    }
    out.sort_by(|a, b| a.archive.cmp(&b.archive));
    Ok(out)
}

fn source_label(source: &ArchiveSource) -> &'static str {
    match source {
        ArchiveSource::TrueosfsRoot => "TRUEOSFS root",
        ArchiveSource::EmbeddedModule { .. } => "boot embedded",
    }
}

fn print_archive_table(io: &'static dyn ShellBackend2, archives: &[ArchiveEntry]) {
    let table = TlbTable::with_width(TABLE_HEADERS, line_width_for_backend(io).saturating_sub(2));
    table.emit_header(|text| print_shell_line(io, text));
    for (idx, archive) in archives.iter().enumerate() {
        let id = alloc::format!("{}", idx + 1);
        let row = [
            id.as_str(),
            archive.archive.as_str(),
            source_label(&archive.source),
        ];
        table.emit_row(&row, |text| print_shell_line(io, text));
    }
    table.emit_footer(|text| print_shell_line(io, text));
}

pub(crate) fn print_app_archive_table(io: &'static dyn ShellBackend2) {
    match archive_entries() {
        Ok(archives) if archives.is_empty() => {
            print_shell_line(io, "apps: no .bp or .vm modules available");
        }
        Ok(archives) => print_archive_table(io, archives.as_slice()),
        Err(err) => print_shell_line(io, alloc::format!("apps: {}", err).as_str()),
    }
}

fn enqueue_request(request: AppVmLaunchRequest) {
    APP_VM_RUN_QUEUE.lock().push_back(request);
}

fn dequeue_request() -> Option<AppVmLaunchRequest> {
    APP_VM_RUN_QUEUE.lock().pop_front()
}

async fn execute_request(_spawner: &Spawner, request: AppVmLaunchRequest) {
    let target = request.target.clone();
    let log = |line: &str| {
        print_matrix_target_line(&target, line);
    };

    log(alloc::format!("hv run: worker start module={}", request.archive.as_str()).as_str());
    log(alloc::format!("hv run: module bytes={}", request.module_bytes.len()).as_str());

    if request.module_bytes.starts_with(b"TC4O") || request.archive.ends_with(".vm") {
        execute_tc4o(request.module_bytes.as_slice(), &log);
        return;
    }

    if request.archive.ends_with(".bp") {
        execute_blueprint(&request, &log);
        return;
    }

    log("hv run: blueprint payload support disabled");
}

fn execute_blueprint(request: &AppVmLaunchRequest, log: &dyn Fn(&str)) {
    let module = match crate::hv::blueprint::parse_blueprint(request.module_bytes.as_slice()) {
        Ok(module) => module,
        Err(err) => {
            log(alloc::format!("hv run: blueprint parse failed: {}", err).as_str());
            return;
        }
    };

    let unpacked = match crate::hv::blueprint::unpack_blueprint(&module) {
        Ok(unpacked) => unpacked,
        Err(err) => {
            log(alloc::format!("hv run: blueprint unpack failed: {}", err).as_str());
            return;
        }
    };

    let app_fs_root = crate::hv::blueprint::app_fs_root_for_archive(
        request.archive.as_str(),
        request.module_bytes.as_slice(),
    );
    let process_args = crate::hv::blueprint::build_process_args(
        request.archive.as_str(),
        request.app_args.as_slice(),
    );
    let process_env = crate::hv::blueprint::build_process_env(
        request.archive.as_str(),
        Some(app_fs_root.as_str()),
    );

    log(
        alloc::format!(
            "hv run: blueprint version={} flags={} entry=0x{:016x} payload={} raw={}",
            module.version,
            module.flags,
            module.entry,
            module.payload.len(),
            module.raw_payload_len
        )
        .as_str(),
    );

    match crate::hv::blueprint::invoke_host_rel(
        unpacked.as_slice(),
        module.entry,
        process_args,
        process_env,
        Some(request.target.clone()),
        Some(app_fs_root),
    ) {
        Ok(()) => log("hv run: blueprint completed"),
        Err(err) => log(alloc::format!("hv run: blueprint failed: {}", err).as_str()),
    }
}

fn execute_tc4o(module_bytes: &[u8], log: &dyn Fn(&str)) {
    match trueos_c4::run_vm_object(module_bytes, 100_000) {
        Ok(report) => {
            log(format!(
                "hv run: TC4O ok code={} symbols={} stack={} steps={}",
                report.code_len, report.symbol_count, report.stack_bytes, report.steps
            )
            .as_str());
            for local in report.locals.iter() {
                log(format!("hv run: local {}", format_tc4o_local(local)).as_str());
            }
        }
        Err(err) => {
            log(format!("hv run: TC4O failed: {:?}", err).as_str());
        }
    }
}

fn format_tc4o_local(local: &trueos_c4::VmLocalReport) -> String {
    match &local.value {
        trueos_c4::VmValue::Int(value) => format!("{}={}", local.name, value),
        trueos_c4::VmValue::Bool(value) => format!("{}={}", local.name, value),
        trueos_c4::VmValue::FloatBits(value) => format!("{}=f64bits:0x{:016x}", local.name, value),
        trueos_c4::VmValue::Bytes(bytes) => {
            let mut out = format!("{}=[", local.name);
            for (idx, chunk) in bytes.chunks(4).enumerate() {
                if idx != 0 {
                    out.push_str(", ");
                }
                if chunk.len() == 4 {
                    let value = i32::from_le_bytes(chunk.try_into().unwrap());
                    out.push_str(format!("{}", value).as_str());
                } else {
                    out.push('?');
                }
            }
            out.push(']');
            out
        }
    }
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn app_vm_run_queue_task(spawner: Spawner) {
    loop {
        let Some(request) = dequeue_request() else {
            Timer::after(EmbassyDuration::from_millis(25)).await;
            continue;
        };

        let target = request.target.clone();
        set_matrix_target_active(&target, true);
        execute_request(&spawner, request).await;
        set_matrix_target_active(&target, false);
    }
}

pub(crate) fn enqueue_blueprint_bytes(
    target: MatrixTarget,
    archive: String,
    module_bytes: Vec<u8>,
    app_args: Vec<String>,
) {
    print_matrix_target_line(
        &target,
        alloc::format!("hv run: queued {}", archive.as_str()).as_str(),
    );
    enqueue_request(AppVmLaunchRequest {
        archive,
        module_bytes,
        app_args,
        target,
    });
}

pub(crate) async fn submit_archive_name_to_target_prefer_trueosfs_async(
    target: MatrixTarget,
    archive_name: &str,
    app_args: Vec<String>,
) -> Result<&'static str, &'static str> {
    if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
        if let Some(module_bytes) = crate::r::fs::trueosfs::file_out_async(disk, archive_name)
            .await
            .map_err(|_| "failed to read selected module from TRUEOSFS")?
        {
            enqueue_blueprint_bytes(target, String::from(archive_name), module_bytes, app_args);
            return Ok("TRUEOSFS root");
        }
    }

    if let Some(module_bytes) = embedded_module_bytes_by_archive_name(archive_name)? {
        enqueue_blueprint_bytes(target, String::from(archive_name), module_bytes, app_args);
        return Ok("boot embedded");
    }

    Err("archive not found")
}

pub(crate) fn submit_run(io: &'static dyn ShellBackend2, archive: String, app_args: Vec<String>) {
    let target = matrix_target_for_backend(io);
    let module_bytes = match crate::r::io::kfs::read_file(archive.as_str()) {
        Ok(bytes) => bytes,
        Err(_) => {
            print_shell_line(io, "hv run: failed to read selected module from TRUEOSFS");
            return;
        }
    };

    enqueue_blueprint_bytes(target, archive, module_bytes, app_args);
}

fn submit_archive_entry(
    io: &'static dyn ShellBackend2,
    entry: &ArchiveEntry,
    app_args: Vec<String>,
) {
    match &entry.source {
        ArchiveSource::TrueosfsRoot => {
            submit_run(io, entry.archive.clone(), app_args);
        }
        ArchiveSource::EmbeddedModule { cmdline } => {
            let target = matrix_target_for_backend(io);
            let Some(module_bytes) = crate::limine::module_bytes_by_string(cmdline.as_bytes())
            else {
                print_shell_line(io, "hv run: failed to read selected embedded module");
                return;
            };
            enqueue_blueprint_bytes(target, entry.archive.clone(), module_bytes.to_vec(), app_args);
        }
    }
}

pub(crate) fn submit_archive_id(
    io: &'static dyn ShellBackend2,
    id: usize,
    app_args: Vec<String>,
) -> bool {
    let archives = match archive_entries() {
        Ok(archives) => archives,
        Err(err) => {
            print_shell_line(io, alloc::format!("apps: {}", err).as_str());
            return false;
        }
    };
    let Some(archive) = id.checked_sub(1).and_then(|idx| archives.get(idx)) else {
        print_shell_line(io, "apps: unknown app id");
        print_archive_table(io, archives.as_slice());
        return false;
    };
    submit_archive_entry(io, archive, app_args);
    true
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let _ = spawner;
    let archives = match archive_entries() {
        Ok(archives) => archives,
        Err(err) => {
            print_shell_line(io, alloc::format!("hv run: {}", err).as_str());
            return ParseOutcome::Handled;
        }
    };

    let Some(id_text) = args.next() else {
        if archives.is_empty() {
            print_shell_line(io, "hv run: no .bp or .vm modules available");
            return ParseOutcome::Handled;
        }
        print_archive_table(io, archives.as_slice());
        return ParseOutcome::Handled;
    };

    let archive_index = match id_text.parse::<usize>() {
        Ok(id) if id > 0 => id - 1,
        _ => {
            print_usage(io);
            return ParseOutcome::Handled;
        }
    };

    let Some(archive) = archives.get(archive_index) else {
        print_shell_line(io, "hv run: unknown archive id");
        print_archive_table(io, archives.as_slice());
        return ParseOutcome::Handled;
    };

    let app_args = args.map(String::from).collect::<Vec<_>>();
    submit_archive_entry(io, archive, app_args);

    ParseOutcome::Handled
}
