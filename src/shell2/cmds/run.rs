use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use core::str::SplitWhitespace;

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use super::super::{
    MatrixTarget, ShellBackend2, UART1_COM1_BACKEND, default_matrix_target, line_width_for_backend,
    matrix_target_for_backend, print_matrix_target_line, print_shell_line, set_matrix_target_active,
};
use super::tlb_helper::TlbTable;
use crate::blueprint;
use crate::shell2::shell2_cmd::ParseOutcome;

const TABLE_HEADERS: &[&str; 3] = &["id", "module", "source"];
const APP_VM_ID: u8 = 0;

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
    print_shell_line(io, "run: usage `run` or `run <id> [args...]`");
}

fn embedded_archive_name(cmdline: &[u8]) -> Option<String> {
    let suffix = cmdline.strip_prefix(b"trueos.app.")?;
    if suffix.is_empty() {
        return None;
    }
    let mut archive = String::from_utf8_lossy(suffix).into_owned();
    archive.push_str("_app.bp");
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
        .filter(|name| name.ends_with(".bp"))
        .map(|name| ArchiveEntry {
            archive: String::from(name),
            source: ArchiveSource::TrueosfsRoot,
        })
        .collect::<Vec<_>>();
    out.sort_by(|a, b| a.archive.cmp(&b.archive));
    Ok(out)
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

fn archive_entries() -> Result<Vec<ArchiveEntry>, &'static str> {
    let mut out = embedded_archives();
    out.extend(root_archives()?);
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

fn enqueue_request(request: AppVmLaunchRequest) {
    APP_VM_RUN_QUEUE.lock().push_back(request);
}

fn dequeue_request() -> Option<AppVmLaunchRequest> {
    APP_VM_RUN_QUEUE.lock().pop_front()
}

fn wait_for_vm_slot_clear() -> impl core::future::Future<Output = ()> {
    async {
        loop {
            let status = crate::hv::status();
            if !status.vm1_running && !status.vm1_starting {
                return;
            }
            Timer::after(EmbassyDuration::from_millis(50)).await;
        }
    }
}

async fn execute_request(spawner: &Spawner, request: AppVmLaunchRequest) {
    let target = request.target.clone();
    let log = |line: &str| {
        print_matrix_target_line(&target, line);
    };

    log(alloc::format!("run: worker start module={}", request.archive.as_str()).as_str());
    log(alloc::format!("run: module bytes={}", request.module_bytes.len()).as_str());

    let module = match blueprint::parse_blueprint(request.module_bytes.as_slice()) {
        Ok(module) => module,
        Err(err) => {
            log(alloc::format!("run: {}", err).as_str());
            return;
        }
    };
    let unpacked = match blueprint::unpack_blueprint(&module) {
        Ok(bytes) => bytes,
        Err(err) => {
            log(alloc::format!("run: {}", err).as_str());
            return;
        }
    };

    log(alloc::format!(
        "run: module={} version={} flags={} entry_hint=sec:{}+0x{:x}",
        request.archive,
        module.version,
        module.flags,
        blueprint::entry_hint_section(module.entry),
        blueprint::entry_hint_offset(module.entry)
    )
    .as_str());
    log(alloc::format!(
        "run: payload compressed={} unpacked={} header_raw={}",
        module.payload.len(),
        unpacked.len(),
        module.raw_payload_len
    )
    .as_str());
    if unpacked.len() != module.raw_payload_len {
        log("run: warning: unpacked payload size does not match header_raw");
    }
    if unpacked.starts_with(b"\x7fELF") {
        if let Some(kind) = blueprint::elf_type_name(unpacked.as_slice()) {
            log(alloc::format!("run: unpacked payload looks like ELF type={}", kind).as_str());
        } else {
            log("run: unpacked payload looks like ELF");
        }
    } else {
        log("run: unpacked payload does not look like ELF");
    }
    if unpacked.starts_with(b"\x7fELF") {
        match blueprint::elf_imports(unpacked.as_slice()) {
            Ok(imports) => {
                if imports.is_empty() {
                    log("run: ELF imports=0");
                } else {
                    let resolved = imports
                        .iter()
                        .filter(|import| import.resolved_addr.is_some())
                        .count();
                    log(alloc::format!(
                        "run: ELF imports={} resolved={}",
                        imports.len(),
                        resolved
                    )
                    .as_str());
                    for import in imports.iter() {
                        match import.resolved_addr {
                            Some(addr) => log(
                                alloc::format!("run: import {} -> 0x{:x}", import.name, addr)
                                    .as_str(),
                            ),
                            None => log(
                                alloc::format!("run: import {} -> unresolved", import.name)
                                    .as_str(),
                            ),
                        }
                    }
                }
            }
            Err(err) => {
                log(alloc::format!("run: ELF import scan failed: {}", err).as_str());
            }
        }
    }

    if !unpacked.starts_with(b"\x7fELF")
        || !matches!(blueprint::elf_type_name(unpacked.as_slice()), Some("REL"))
    {
        log("run: only ELF REL blueprints are supported for app-vm launch");
        return;
    }

    if !crate::r::readiness::is_set(crate::r::readiness::APP_VM_READY) {
        log("run: waiting for app_vm_ready");
        crate::r::readiness::wait_for(crate::r::readiness::APP_VM_READY).await;
        log("run: app_vm_ready=1");
    }

    wait_for_vm_slot_clear().await;

    crate::hv::stage_blueprint_launch(crate::hv::BlueprintLaunchState {
        archive: request.archive.clone(),
        module_bytes: request.module_bytes.clone(),
        app_args: request.app_args.clone(),
        console_target: Some(target.clone()),
    });

    match crate::hv::start(APP_VM_ID, spawner, &UART1_COM1_BACKEND, None) {
        Ok(()) => {
            log("run: app-vm launch requested");
        }
        Err(err) => {
            log(alloc::format!("run: app-vm start failed: {:?}", err).as_str());
            let _ = crate::hv::take_blueprint_launch();
            return;
        }
    }

    loop {
        let status = crate::hv::status();
        if !status.vm1_running && !status.vm1_starting {
            break;
        }
        Timer::after(EmbassyDuration::from_millis(50)).await;
    }

    log("run: app-vm finished");
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
    print_matrix_target_line(&target, alloc::format!("run: queued {}", archive.as_str()).as_str());
    enqueue_request(AppVmLaunchRequest {
        archive,
        module_bytes,
        app_args,
        target,
    });
}

pub(crate) fn enqueue_embedded_hello_world_once() {
    let Some(module_bytes) = crate::limine::module_bytes_by_string(b"trueos.app.hello_world") else {
        crate::log!("run: boot hello-world module missing from limine modules\n");
        return;
    };

    enqueue_blueprint_bytes(
        default_matrix_target(),
        String::from("hello_world_app.bp"),
        module_bytes.to_vec(),
        Vec::new(),
    );
}

pub(crate) fn submit_run(io: &'static dyn ShellBackend2, archive: String, app_args: Vec<String>) {
    let target = matrix_target_for_backend(io);
    let module_bytes = match crate::r::io::kfs::read_file(archive.as_str()) {
        Ok(bytes) => bytes,
        Err(_) => {
            print_shell_line(io, "run: failed to read selected module from TRUEOSFS");
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
            let Some(module_bytes) = crate::limine::module_bytes_by_string(cmdline.as_bytes()) else {
                print_shell_line(io, "run: failed to read selected embedded module");
                return;
            };
            enqueue_blueprint_bytes(target, entry.archive.clone(), module_bytes.to_vec(), app_args);
        }
    }
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
            print_shell_line(io, alloc::format!("run: {}", err).as_str());
            return ParseOutcome::Handled;
        }
    };

    let Some(id_text) = args.next() else {
        if archives.is_empty() {
            print_shell_line(io, "run: no .bp modules available");
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
        print_shell_line(io, "run: unknown archive id");
        print_archive_table(io, archives.as_slice());
        return ParseOutcome::Handled;
    };

    let app_args = args.map(String::from).collect::<Vec<_>>();
    submit_archive_entry(io, archive, app_args);

    ParseOutcome::Handled
}
