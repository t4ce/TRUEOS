use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use super::super::{
    MatrixTarget, ShellBackend2, UART1_COM1_BACKEND, line_width_for_backend,
    matrix_target_for_backend, matrix_target_interrupted, print_matrix_target_line,
    print_shell_line, set_matrix_target_active,
};
use super::tlb_helper::TlbTable;

const TABLE_HEADERS: &[&str; 4] = &["id", "module", "source", "updated"];
const BLUEPRINT_READINESS_TIMEOUT: EmbassyDuration = EmbassyDuration::from_secs(30);
const MIB: usize = 1024 * 1024;

use alloc::collections::VecDeque;

static APP_VM_RUN_QUEUE: Mutex<VecDeque<AppVmLaunchRequest>> = Mutex::new(VecDeque::new());

#[derive(Clone)]
struct AppVmLaunchRequest {
    archive: String,
    module_bytes: Vec<u8>,
    app_args: Vec<String>,
    target: MatrixTarget,
    preflight_complete: bool,
}

#[derive(Copy, Clone)]
enum BlueprintMemoryClass {
    TinyUi,
    Ui,
    TokioRuntime,
    NetworkClient,
    ServerRuntime,
    HeavyGraphics,
    Unknown,
}

impl BlueprintMemoryClass {
    const fn label(self) -> &'static str {
        match self {
            Self::TinyUi => "tiny-ui",
            Self::Ui => "ui",
            Self::TokioRuntime => "tokio-runtime",
            Self::NetworkClient => "network-client",
            Self::ServerRuntime => "server-runtime",
            Self::HeavyGraphics => "heavy-graphics",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Copy, Clone)]
struct BlueprintVmMemoryProfile {
    class: BlueprintMemoryClass,
    heap_lower_mib: usize,
    heap_recommended_mib: usize,
    heap_upper_mib: usize,
    stack_lower_mib: usize,
    stack_recommended_mib: usize,
    stack_upper_mib: usize,
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
    updated: Option<String>,
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
            updated: root_archive_updated(disk, name),
        })
        .collect::<Vec<_>>();
    out.sort_by(|a, b| a.archive.cmp(&b.archive));
    Ok(out)
}

fn root_archive_timestamp_path(name: &str) -> String {
    alloc::format!("apps/common/.bp-meta/root/{}.updated", name)
}

fn root_archive_updated(disk: crate::disc::block::DeviceHandle, name: &str) -> Option<String> {
    let stamp_path = root_archive_timestamp_path(name);
    let bytes = crate::wait::spawn_and_wait_local(async move {
        crate::r::fs::trueosfs::file_out_async(disk, stamp_path.as_str()).await
    })
    .ok()??;
    let text = String::from_utf8_lossy(bytes.as_slice()).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
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
            updated: None,
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
        return Ok(Some(crate::allocators::with_host_alloc_domain(|| module_bytes.to_vec())));
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
            archive.updated.as_deref().unwrap_or("-"),
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

fn enqueue_blueprint_request(
    target: MatrixTarget,
    archive: String,
    module_bytes: Vec<u8>,
    app_args: Vec<String>,
    preflight_complete: bool,
) {
    crate::log!(
        "app-vm-run-queue: enqueue archive={} bytes={} args={} preflight={}\n",
        archive.as_str(),
        module_bytes.len(),
        app_args.len(),
        preflight_complete as u8
    );
    enqueue_request(AppVmLaunchRequest {
        archive,
        module_bytes,
        app_args,
        target,
        preflight_complete,
    });
}

fn log_run_target_line(target: &MatrixTarget, line: &str) {
    print_matrix_target_line(target, line);
    crate::hv::hvlogf(format_args!("{}", line));
}

fn dequeue_request() -> Option<AppVmLaunchRequest> {
    APP_VM_RUN_QUEUE.lock().pop_front()
}

async fn execute_request(spawner: &Spawner, request: AppVmLaunchRequest) {
    let target = request.target.clone();
    let log = |line: &str| {
        print_matrix_target_line(&target, line);
        crate::hv::hvlogf(format_args!("{}", line));
    };

    log(alloc::format!("apps: worker start module={}", request.archive.as_str()).as_str());
    log(alloc::format!("apps: module bytes={}", request.module_bytes.len()).as_str());
    crate::log!(
        "app-vm-run-queue: worker start archive={} bytes={}\n",
        request.archive.as_str(),
        request.module_bytes.len()
    );
    if matrix_target_interrupted(&target) {
        log("apps: interrupted before launch");
        return;
    }

    if request.module_bytes.starts_with(b"TC4O") || request.archive.ends_with(".vm") {
        execute_tc4o(request.module_bytes.as_slice(), &log);
        return;
    }

    if request.archive.ends_with(".bp") {
        execute_blueprint(spawner, &request, &log).await;
        return;
    }

    log("apps: blueprint payload support disabled");
}

async fn execute_blueprint(spawner: &Spawner, request: &AppVmLaunchRequest, log: &dyn Fn(&str)) {
    if matrix_target_interrupted(&request.target) {
        log("apps: interrupted before preflight");
        return;
    }
    if !request.preflight_complete
        && let Err(err) = preflight_blueprint_launch(
            request.archive.as_str(),
            request.module_bytes.as_slice(),
            log,
        )
        .await
    {
        log(alloc::format!("apps: {}", err).as_str());
        return;
    }
    if matrix_target_interrupted(&request.target) {
        log("apps: interrupted before vm start");
        return;
    }

    crate::allocators::with_host_alloc_domain(|| start_blueprint_launch(spawner, request, log));
}

fn ceil_mib(bytes: usize) -> usize {
    bytes.saturating_add(MIB - 1) / MIB
}

fn clamp_mib(value: usize, lower: usize, upper: usize) -> usize {
    value.max(lower).min(upper)
}

fn round_pow2_mib(value: usize) -> usize {
    value.max(1).next_power_of_two()
}

fn import_name_has(imports: &[crate::hv::blueprint::ElfImport<'_>], needle: &str) -> bool {
    imports.iter().any(|import| import.name.contains(needle))
}

fn archive_has(archive: &str, needle: &str) -> bool {
    archive.contains(needle)
}

fn classify_blueprint_memory(
    archive: &str,
    raw_payload_len: usize,
    stats: crate::hv::blueprint::ElfAllocStats,
    imports: &[crate::hv::blueprint::ElfImport<'_>],
) -> BlueprintMemoryClass {
    let server_signal = archive_has(archive, "horizon")
        || archive_has(archive, "server")
        || archive_has(archive, "game")
        || import_name_has(imports, "pthread_");
    if server_signal {
        return BlueprintMemoryClass::ServerRuntime;
    }

    let network_signal = archive_has(archive, "weather")
        || archive_has(archive, "currency")
        || archive_has(archive, "reqwest")
        || archive_has(archive, "http")
        || archive_has(archive, "https")
        || import_name_has(imports, "trueos_mio_")
        || import_name_has(imports, "dns_resolve")
        || import_name_has(imports, "net_fetch")
        || import_name_has(imports, "tcp_stream")
        || import_name_has(imports, "tokio_spawn_blocking");
    if network_signal {
        return BlueprintMemoryClass::NetworkClient;
    }

    let tokio_signal = archive_has(archive, "tokio")
        || import_name_has(imports, "trueos_tokio_")
        || import_name_has(imports, "tokio_")
        || import_name_has(imports, "sleep_ms");
    if tokio_signal {
        return BlueprintMemoryClass::TokioRuntime;
    }

    let heavy_graphics_signal = archive_has(archive, "mandelbrot")
        || archive_has(archive, "shader")
        || archive_has(archive, "particle")
        || archive_has(archive, "virgl")
        || import_name_has(imports, "gfx")
        || stats.alloc_bytes > 4 * MIB
        || raw_payload_len > 8 * MIB;
    if heavy_graphics_signal {
        return BlueprintMemoryClass::HeavyGraphics;
    }

    let tiny_ui_signal = raw_payload_len <= MIB
        && stats.alloc_bytes <= 512 * 1024
        && import_name_has(imports, "ui2")
        && !imports.is_empty();
    if tiny_ui_signal {
        return BlueprintMemoryClass::TinyUi;
    }

    if import_name_has(imports, "ui2") || import_name_has(imports, "app_surface_window") {
        return BlueprintMemoryClass::Ui;
    }

    BlueprintMemoryClass::Unknown
}

fn estimate_blueprint_memory_profile(
    archive: &str,
    module: &crate::hv::blueprint::BlueprintModule<'_>,
    unpacked: &[u8],
    imports: &[crate::hv::blueprint::ElfImport<'_>],
) -> BlueprintVmMemoryProfile {
    let stats = crate::hv::blueprint::elf_alloc_stats(unpacked).unwrap_or_default();
    let class = classify_blueprint_memory(archive, module.raw_payload_len, stats, imports);
    let base_live_mib = ceil_mib(module.raw_payload_len).max(ceil_mib(stats.alloc_bytes));

    let (heap_lower, heap_recommended, heap_upper, stack_lower, stack_recommended, stack_upper) =
        match class {
            BlueprintMemoryClass::TinyUi => (
                16,
                round_pow2_mib(base_live_mib.saturating_mul(8).saturating_add(16)).max(32),
                128,
                8,
                8,
                32,
            ),
            BlueprintMemoryClass::Ui => (
                32,
                round_pow2_mib(base_live_mib.saturating_mul(10).saturating_add(32)).max(64),
                192,
                8,
                16,
                64,
            ),
            BlueprintMemoryClass::TokioRuntime => (
                64,
                round_pow2_mib(base_live_mib.saturating_mul(12).saturating_add(64)).max(128),
                256,
                8,
                16,
                64,
            ),
            BlueprintMemoryClass::NetworkClient => (
                64,
                round_pow2_mib(base_live_mib.saturating_mul(32).saturating_add(128)).max(512),
                512,
                8,
                16,
                64,
            ),
            BlueprintMemoryClass::ServerRuntime => (
                512,
                round_pow2_mib(base_live_mib.saturating_mul(96).saturating_add(1024)).max(4096),
                4096,
                16,
                64,
                128,
            ),
            BlueprintMemoryClass::HeavyGraphics => (
                128,
                round_pow2_mib(base_live_mib.saturating_mul(16).saturating_add(128)).max(256),
                512,
                16,
                32,
                128,
            ),
            BlueprintMemoryClass::Unknown => (64, 128, 512, 8, 16, 64),
        };

    BlueprintVmMemoryProfile {
        class,
        heap_lower_mib: heap_lower,
        heap_recommended_mib: clamp_mib(heap_recommended, heap_lower, heap_upper),
        heap_upper_mib: heap_upper,
        stack_lower_mib: stack_lower,
        stack_recommended_mib: clamp_mib(stack_recommended, stack_lower, stack_upper),
        stack_upper_mib: stack_upper,
    }
}

fn log_blueprint_memory_profile(profile: BlueprintVmMemoryProfile, log: &dyn Fn(&str)) {
    log(format!(
        "apps: memory profile class={} heap_mib={}/{}/{} stack_mib={}/{}/{}",
        profile.class.label(),
        profile.heap_lower_mib,
        profile.heap_recommended_mib,
        profile.heap_upper_mib,
        profile.stack_lower_mib,
        profile.stack_recommended_mib,
        profile.stack_upper_mib
    )
    .as_str());
}

async fn preflight_blueprint_launch(
    archive: &str,
    module_bytes: &[u8],
    log: &dyn Fn(&str),
) -> Result<(), String> {
    let module = crate::hv::blueprint::parse_blueprint(module_bytes)?;

    let unpacked = crate::hv::blueprint::unpack_blueprint(&module)?;

    log(alloc::format!(
        "apps: module={} version={} flags={} entry_hint=sec:{}+0x{:x}",
        archive,
        module.version,
        module.flags,
        crate::hv::blueprint::entry_hint_section(module.entry),
        crate::hv::blueprint::entry_hint_offset(module.entry)
    )
    .as_str());
    log(alloc::format!(
        "apps: payload compressed={} unpacked={} header_raw={}",
        module.payload.len(),
        unpacked.len(),
        module.raw_payload_len
    )
    .as_str());
    log(alloc::format!(
        "apps: blueprint version={} flags={} entry=0x{:016x} payload={} raw={}",
        module.version,
        module.flags,
        module.entry,
        module.payload.len(),
        module.raw_payload_len
    )
    .as_str());

    if unpacked.len() != module.raw_payload_len {
        log("apps: warning: unpacked payload size does not match header_raw");
    }
    if unpacked.starts_with(b"\x7fELF") {
        if let Some(kind) = crate::hv::blueprint::elf_type_name(unpacked.as_slice()) {
            log(alloc::format!("apps: unpacked payload looks like ELF type={}", kind).as_str());
        } else {
            log("apps: unpacked payload looks like ELF");
        }
        match crate::hv::blueprint::elf_rel_debug_summary(unpacked.as_slice(), module.entry) {
            Ok(summary) => log(alloc::format!("apps: {}", summary).as_str()),
            Err(err) => log(alloc::format!("apps: ELF diag failed: {}", err).as_str()),
        }
    } else {
        log("apps: unpacked payload does not look like ELF");
    }

    let mut required_readiness = crate::hv::blueprint::prebind_base_readiness();
    let mut imports_for_profile = Vec::new();
    if unpacked.starts_with(b"\x7fELF") {
        match crate::hv::blueprint::elf_imports(unpacked.as_slice()) {
            Ok(imports) => {
                if imports.is_empty() {
                    log("apps: ELF imports=0");
                } else {
                    let resolved = imports
                        .iter()
                        .filter(|import| import.resolved_addr.is_some())
                        .count();
                    log(alloc::format!(
                        "apps: ELF imports={} resolved={}",
                        imports.len(),
                        resolved
                    )
                    .as_str());
                    for import in imports.iter() {
                        required_readiness |=
                            crate::hv::blueprint::prebind_import_readiness(import.name);
                        match import.resolved_addr {
                            Some(addr) => {
                                log(alloc::format!("apps: import {} -> 0x{:x}", import.name, addr)
                                    .as_str())
                            }
                            None => {
                                log(alloc::format!("apps: import {} -> unresolved", import.name)
                                    .as_str())
                            }
                        }
                    }
                }
                imports_for_profile = imports;
            }
            Err(err) => {
                log(alloc::format!("apps: ELF import scan failed: {}", err).as_str());
            }
        }
    }

    let profile = estimate_blueprint_memory_profile(
        archive,
        &module,
        unpacked.as_slice(),
        imports_for_profile.as_slice(),
    );
    log_blueprint_memory_profile(profile, log);

    if !unpacked.starts_with(b"\x7fELF")
        || !matches!(crate::hv::blueprint::elf_type_name(unpacked.as_slice()), Some("REL"))
    {
        return Err(String::from("only ELF REL blueprints are supported for app-vm launch"));
    }

    let missing_readiness = required_readiness & !crate::r::readiness::mask();
    log(alloc::format!(
        "apps: readiness required={} missing={}",
        readiness_mask_text(required_readiness).as_str(),
        readiness_mask_text(missing_readiness).as_str()
    )
    .as_str());
    if missing_readiness != 0 {
        let ready =
            crate::r::readiness::wait_for_timeout(required_readiness, BLUEPRINT_READINESS_TIMEOUT)
                .await;
        if !ready {
            let still_missing = required_readiness & !crate::r::readiness::mask();
            return Err(alloc::format!(
                "readiness timeout after {}ms required={} missing={}",
                BLUEPRINT_READINESS_TIMEOUT.as_millis(),
                readiness_mask_text(required_readiness).as_str(),
                readiness_mask_text(still_missing).as_str()
            ));
        }
        log(alloc::format!(
            "apps: readiness ok required={}",
            readiness_mask_text(required_readiness).as_str()
        )
        .as_str());
    }

    Ok(())
}

fn start_blueprint_launch(spawner: &Spawner, request: &AppVmLaunchRequest, log: &dyn Fn(&str)) {
    let Some(vm_id) = crate::hv::first_free_vm_id() else {
        log("apps: no free app-vm ids");
        return;
    };

    match crate::hv::start_blueprint_app_vm(
        vm_id,
        spawner,
        &UART1_COM1_BACKEND,
        request.archive.clone(),
        request.module_bytes.clone(),
        request.app_args.clone(),
        Some(request.target.clone()),
    ) {
        Ok(()) => {
            crate::log!(
                "app-vm-run-queue: hv start ok vm={} archive={}\n",
                vm_id,
                request.archive.as_str()
            );
            log(alloc::format!("apps: app-vm{} launch requested", vm_id).as_str());
        }
        Err(err) => {
            crate::log_warn!(
                target: "service";
                "app-vm-run-queue: hv start failed vm={} archive={} err={:?}\n",
                vm_id,
                request.archive.as_str(),
                err
            );
            log(alloc::format!("apps: app-vm start failed: {:?}", err).as_str());
        }
    }
}

fn readiness_mask_text(mask: u32) -> String {
    if mask == 0 {
        return String::from("none");
    }

    let mut out = String::new();
    crate::r::readiness::for_each_flag(mask, |_, name| {
        if !out.is_empty() {
            out.push('|');
        }
        out.push_str(name);
    });
    if out.is_empty() {
        alloc::format!("0x{:08x}", mask)
    } else {
        out
    }
}

fn execute_tc4o(module_bytes: &[u8], log: &dyn Fn(&str)) {
    match trueos_c4::run_vm_object(module_bytes, 100_000) {
        Ok(report) => {
            log(format!(
                "apps: TC4O ok code={} symbols={} stack={} steps={}",
                report.code_len, report.symbol_count, report.stack_bytes, report.steps
            )
            .as_str());
            for local in report.locals.iter() {
                log(format!("apps: local {}", format_tc4o_local(local)).as_str());
            }
        }
        Err(err) => {
            log(format!("apps: TC4O failed: {:?}", err).as_str());
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
) -> Result<(), String> {
    let required_readiness = crate::hv::blueprint::prebind_required_readiness(
        module_bytes.as_slice(),
    )
    .map_err(|err| {
        let line = alloc::format!("apps: not queued {} {}", archive.as_str(), err.as_str());
        log_run_target_line(&target, line.as_str());
        line
    })?;
    let missing_readiness = required_readiness & !crate::r::readiness::mask();
    if missing_readiness != 0 {
        let line = alloc::format!(
            "apps: not queued {} required={} missing={}",
            archive.as_str(),
            readiness_mask_text(required_readiness).as_str(),
            readiness_mask_text(missing_readiness).as_str()
        );
        log_run_target_line(&target, line.as_str());
        return Err(line);
    }

    let line = alloc::format!("apps: queued {}", archive.as_str());
    log_run_target_line(&target, line.as_str());
    enqueue_blueprint_request(target, archive, module_bytes, app_args, false);
    Ok(())
}

async fn preflight_archive_name_to_target_async(
    target: &MatrixTarget,
    archive_name: &str,
    module_bytes: &[u8],
) -> Result<(), String> {
    let log = |line: &str| log_run_target_line(target, line);
    preflight_blueprint_launch(archive_name, module_bytes, &log).await
}

async fn submit_module_bytes_to_target_async(
    target: MatrixTarget,
    archive_name: &str,
    module_bytes: Vec<u8>,
    app_args: Vec<String>,
    source: &'static str,
) -> Result<&'static str, String> {
    preflight_archive_name_to_target_async(&target, archive_name, module_bytes.as_slice()).await?;
    let required_readiness = crate::hv::blueprint::prebind_required_readiness(
        module_bytes.as_slice(),
    )
    .map_err(|err| {
        let line = alloc::format!("apps: not queued {} {}", archive_name, err.as_str());
        log_run_target_line(&target, line.as_str());
        line
    })?;
    let missing_readiness = required_readiness & !crate::r::readiness::mask();
    if missing_readiness != 0 {
        let line = alloc::format!(
            "apps: not queued {} required={} missing={} ",
            archive_name,
            readiness_mask_text(required_readiness).as_str(),
            readiness_mask_text(missing_readiness).as_str()
        );
        log_run_target_line(&target, line.as_str());
        return Err(line);
    }
    crate::allocators::with_host_alloc_domain(|| {
        let line = alloc::format!("apps: queued {}", archive_name);
        log_run_target_line(&target, line.as_str());
        enqueue_blueprint_request(target, String::from(archive_name), module_bytes, app_args, true);
    });
    Ok(source)
}

pub(crate) async fn submit_archive_name_to_target_prefer_trueosfs_async(
    target: MatrixTarget,
    archive_name: &str,
    app_args: Vec<String>,
) -> Result<&'static str, String> {
    if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
        if let Some(module_bytes) = crate::r::fs::trueosfs::file_out_async(disk, archive_name)
            .await
            .map_err(|_| String::from("failed to read selected module from TRUEOSFS"))?
        {
            return submit_module_bytes_to_target_async(
                target,
                archive_name,
                module_bytes,
                app_args,
                "TRUEOSFS root",
            )
            .await;
        }
    }

    if let Some(module_bytes) = embedded_module_bytes_by_archive_name(archive_name)? {
        return submit_module_bytes_to_target_async(
            target,
            archive_name,
            module_bytes,
            app_args,
            "boot embedded",
        )
        .await;
    }

    Err(String::from("archive not found"))
}

pub(crate) async fn submit_archive_name_to_target_prefer_embedded_async(
    target: MatrixTarget,
    archive_name: &str,
    app_args: Vec<String>,
) -> Result<&'static str, String> {
    if let Some(module_bytes) = embedded_module_bytes_by_archive_name(archive_name)? {
        return submit_module_bytes_to_target_async(
            target,
            archive_name,
            module_bytes,
            app_args,
            "boot embedded",
        )
        .await;
    }

    if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
        if let Some(module_bytes) = crate::r::fs::trueosfs::file_out_async(disk, archive_name)
            .await
            .map_err(|_| String::from("failed to read selected module from TRUEOSFS"))?
        {
            return submit_module_bytes_to_target_async(
                target,
                archive_name,
                module_bytes,
                app_args,
                "TRUEOSFS root",
            )
            .await;
        }
    }

    Err(String::from("archive not found"))
}

pub(crate) fn submit_run(io: &'static dyn ShellBackend2, archive: String, app_args: Vec<String>) {
    let target = matrix_target_for_backend(io);
    let module_bytes = match crate::r::io::kfs::read_file(archive.as_str()) {
        Ok(bytes) => bytes,
        Err(_) => {
            print_shell_line(io, "apps: failed to read selected module from TRUEOSFS");
            return;
        }
    };

    let _ = enqueue_blueprint_bytes(target, archive, module_bytes, app_args);
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
                print_shell_line(io, "apps: failed to read selected embedded module");
                return;
            };
            let _ = enqueue_blueprint_bytes(
                target,
                entry.archive.clone(),
                module_bytes.to_vec(),
                app_args,
            );
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
