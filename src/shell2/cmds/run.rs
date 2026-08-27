use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use sha2::{Digest, Sha256};
use spin::Mutex;
use trueos_executor::Spawner;
use trueos_time::{Duration as EmbassyDuration, Timer};

use super::super::{
    MatrixTarget, matrix_target_interrupted, print_matrix_target_system_line,
    release_matrix_target_vm_reservation, reserve_matrix_target_for_vm_slot_selected,
    set_matrix_target_active, set_matrix_target_app_identity,
};
use super::tlb_helper::TlbTable;
use crate::hv::BlueprintConsoleSurface;

const TABLE_HEADERS: &[&str; 4] = &["id", "app", "sha", "updated"];
const BLUEPRINT_READINESS_TIMEOUT: EmbassyDuration = EmbassyDuration::from_secs(30);
const MIB: usize = 1024 * 1024;

use alloc::collections::VecDeque;

static APP_VM_RUN_QUEUE: Mutex<VecDeque<AppVmLaunchRequest>> = Mutex::new(VecDeque::new());
static AUTO_CONTAINER_SEQUENCE: AtomicU32 = AtomicU32::new(0);

fn preferred_slot_for_archive(archive: &str) -> String {
    if archive == "hello_world" || archive == "hello_world.bp" {
        return String::from("h_w");
    }

    let stem = archive.trim().trim_end_matches(".bp");
    let mut words = stem
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty());

    let first = words.next().unwrap_or("bp");
    let mut out = String::new();
    out.push(first.chars().next().unwrap_or('b').to_ascii_lowercase());
    for word in words {
        if out.len() >= 3 {
            break;
        }
        out.push(word.chars().next().unwrap_or('p').to_ascii_lowercase());
    }
    if out.len() < 2 {
        for ch in first.chars().skip(1) {
            if out.len() >= 3 {
                break;
            }
            if ch.is_ascii_alphanumeric() {
                out.push(ch.to_ascii_lowercase());
            }
        }
    }
    if out.is_empty() {
        String::from("bp")
    } else {
        out
    }
}

fn app_label_for_archive(archive: &str) -> &str {
    archive.trim().trim_end_matches(".bp")
}

fn app_label_for_instance(archive: &str, instance: &crate::hv::BlueprintInstanceRequest) -> String {
    match (instance.peer.as_deref(), instance.name.as_deref()) {
        (Some(peer), Some(name)) => {
            alloc::format!("{} [peer:{} / {}]", app_label_for_archive(archive), peer, name)
        }
        (_, Some(name)) => alloc::format!("{} [{}]", app_label_for_archive(archive), name),
        _ => String::from(app_label_for_archive(archive)),
    }
}

/// Preserve the first, historical unnamed launch as the default instance.
/// A later plain launch must still start an app: give it a visible container
/// identity before reserving a Matrix/VM target.
fn name_occupied_default_instance(
    target: &MatrixTarget,
    archive: &str,
    instance: crate::hv::BlueprintInstanceRequest,
) -> crate::hv::BlueprintInstanceRequest {
    let Some(existing_vm) = instance
        .is_default()
        .then(|| crate::hv::default_app_instance_vm(archive))
        .flatten()
    else {
        return instance;
    };
    let name = loop {
        let sequence = AUTO_CONTAINER_SEQUENCE
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        let candidate = alloc::format!("container_{sequence}");
        if !crate::hv::named_app_instance_vms(archive)
            .iter()
            .any(|(_, live_name)| live_name == &candidate)
        {
            break candidate;
        }
    };
    log_run_target_line(
        target,
        alloc::format!(
            "apps: default {} is vm{}; starting this additional copy as `{}`",
            app_label_for_archive(archive),
            existing_vm,
            name,
        )
        .as_str(),
    );
    crate::hv::BlueprintInstanceRequest::named(name)
}

fn reserve_target_for_archive(target: &MatrixTarget, archive: &str) -> MatrixTarget {
    let preferred = preferred_slot_for_archive(archive);
    reserve_matrix_target_for_vm_slot_selected(target, preferred.as_str())
}

#[derive(Clone)]
struct AppVmLaunchRequest {
    archive: String,
    module_bytes: Vec<u8>,
    app_args: Vec<String>,
    launch_script: Option<String>,
    instance: crate::hv::BlueprintInstanceRequest,
    target: MatrixTarget,
    preflight_complete: bool,
    console_surface_override: Option<BlueprintConsoleSurface>,
}

#[derive(Copy, Clone)]
struct BlueprintLaunchPlan {
    console_surface: BlueprintConsoleSurface,
}

#[derive(Copy, Clone)]
enum BlueprintMemoryClass {
    TokioRuntime,
    AudioPlayer,
    NetworkClient,
    NetworkServer,
    ServerRuntime,
    HeavyGraphics,
    Unknown,
}

impl BlueprintMemoryClass {
    const fn label(self) -> &'static str {
        match self {
            Self::TokioRuntime => "tokio-runtime",
            Self::AudioPlayer => "audio-player",
            Self::NetworkClient => "network-client",
            Self::NetworkServer => "network-server",
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
struct ArchiveEntry {
    archive: String,
    sha256: String,
    updated: String,
}

async fn archive_entries() -> Result<Vec<ArchiveEntry>, &'static str> {
    crate::app_db::list()
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| ArchiveEntry {
                    archive: entry.archive,
                    sha256: entry.sha256,
                    updated: entry.updated,
                })
                .collect()
        })
        .map_err(|_| "app.db query failed")
}

fn archive_display_path(entry: &ArchiveEntry) -> &str {
    entry.archive.as_str()
}

fn short_sha256(value: &str) -> String {
    if value.len() != 64 {
        return String::from(value);
    }
    alloc::format!("{}…{}", &value[..4], &value[value.len() - 4..])
}

fn print_archive_table(target: &MatrixTarget, width: usize, archives: &[ArchiveEntry]) {
    let id_width = archives
        .len()
        .saturating_sub(1)
        .to_string()
        .len()
        .max(TABLE_HEADERS[0].len());
    let table = TlbTable::with_width(TABLE_HEADERS, width.saturating_sub(2))
        .with_max_col_widths(&[id_width, 0, 9, 18]);
    table.emit_header(|text| print_matrix_target_system_line(target, text));
    for (idx, archive) in archives.iter().enumerate() {
        let id = alloc::format!("{idx}");
        let sha256 = short_sha256(archive.sha256.as_str());
        let row = [
            id.as_str(),
            archive_display_path(archive),
            sha256.as_str(),
            archive.updated.as_str(),
        ];
        table.emit_row(&row, |text| print_matrix_target_system_line(target, text));
    }
    table.emit_footer(|text| print_matrix_target_system_line(target, text));
}

pub(crate) async fn print_app_archive_table(target: &MatrixTarget, width: usize) {
    match archive_entries().await {
        Ok(archives) if archives.is_empty() => {
            print_matrix_target_system_line(target, "apps: no .bp modules available");
        }
        Ok(archives) => print_archive_table(target, width, archives.as_slice()),
        Err(err) => {
            print_matrix_target_system_line(target, alloc::format!("apps: {}", err).as_str())
        }
    }
}

fn enqueue_request(request: AppVmLaunchRequest) {
    APP_VM_RUN_QUEUE.lock().push_back(request);
}

fn enqueue_blueprint_request(
    target: MatrixTarget,
    archive: String,
    source: &'static str,
    module_bytes: Vec<u8>,
    app_args: Vec<String>,
    launch_script: Option<String>,
    instance: crate::hv::BlueprintInstanceRequest,
    preflight_complete: bool,
    console_surface_override: Option<BlueprintConsoleSurface>,
) {
    crate::log!(
        "app-vm-run-queue: enqueue archive={} source={} bytes={} args={} launch_script={} preflight={}\n",
        archive.as_str(),
        source,
        module_bytes.len(),
        app_args.len(),
        launch_script.is_some() as u8,
        preflight_complete as u8
    );
    enqueue_request(AppVmLaunchRequest {
        archive,
        module_bytes,
        app_args,
        launch_script,
        instance,
        target,
        preflight_complete,
        console_surface_override,
    });
}

fn log_run_target_line(_target: &MatrixTarget, line: &str) {
    log_blueprint_line(line);
}

fn log_blueprint_line(line: &str) {
    crate::log_os::blueprint_line(log_os_core::LogLevel::Info, format_args!("{}\n", line));
}

fn dequeue_request() -> Option<AppVmLaunchRequest> {
    APP_VM_RUN_QUEUE.lock().pop_front()
}

async fn execute_request(spawner: &Spawner, request: AppVmLaunchRequest) {
    let target = request.target.clone();
    let log = |line: &str| {
        log_blueprint_line(line);
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
    let mut plan = if request.preflight_complete {
        blueprint_launch_plan(request.archive.as_str(), request.module_bytes.as_slice())
            .unwrap_or_else(|err| {
                log(alloc::format!("apps: launch plan fallback: {}", err).as_str());
                BlueprintLaunchPlan {
                    console_surface: BlueprintConsoleSurface::Text,
                }
            })
    } else {
        match preflight_blueprint_launch(
            request.archive.as_str(),
            request.module_bytes.as_slice(),
            log,
            0,
        )
        .await
        {
            Ok(plan) => plan,
            Err(err) => {
                log(alloc::format!("apps: {}", err).as_str());
                return;
            }
        }
    };
    if request
        .app_args
        .iter()
        .any(|arg| arg == crate::hv::BLUEPRINT_VMX_MINISHELL_ARG)
    {
        plan.console_surface = BlueprintConsoleSurface::Text;
        log("apps: console surface Text (VMX-minishell override; terminal TUI disabled)");
    }
    if let Some(console_surface) = request.console_surface_override {
        plan.console_surface = console_surface;
        log("apps: console surface Text (explicit launch contract; terminal handoff disabled)");
    }
    if matrix_target_interrupted(&request.target) {
        log("apps: interrupted before vm start");
        return;
    }

    crate::allocators::with_host_alloc_domain(|| {
        start_blueprint_launch(spawner, request, plan, log)
    });
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

fn imports_libc_tcp_listener(imports: &[crate::hv::blueprint::ElfImport<'_>]) -> bool {
    let has = |name| imports.iter().any(|import| import.name == name);
    has("bind") && has("listen") && (has("accept") || has("accept4"))
}

fn import_name_is(imports: &[crate::hv::blueprint::ElfImport<'_>], name: &str) -> bool {
    imports.iter().any(|import| import.name == name)
}

fn archive_has(archive: &str, needle: &str) -> bool {
    archive.contains(needle)
}

fn classify_blueprint_console_surface(
    imports: &[crate::hv::blueprint::ElfImport<'_>],
) -> BlueprintConsoleSurface {
    let uses_konsole = imports
        .iter()
        .any(|import| import.name.starts_with("trueos_cabi_konsole_"));
    let uses_raw_shell2 = import_name_is(imports, "trueos_cabi_shell2_raw_write");
    let uses_terminal_lease =
        import_name_is(imports, "trueos_cabi_blueprint_terminal_lease_current_v1");
    let uses_unix_raw_tty = import_name_is(imports, "cfmakeraw")
        || import_name_is(imports, "tcsetattr")
        || (import_name_is(imports, "tcgetattr") && import_name_is(imports, "isatty"));

    if uses_konsole || uses_raw_shell2 || uses_terminal_lease || uses_unix_raw_tty {
        BlueprintConsoleSurface::Terminal
    } else {
        BlueprintConsoleSurface::Text
    }
}

fn blueprint_launch_plan(
    archive: &str,
    module_bytes: &[u8],
) -> Result<BlueprintLaunchPlan, String> {
    let module = crate::hv::blueprint::parse_blueprint(module_bytes)?;
    let unpacked = crate::hv::blueprint::unpack_blueprint(&module)?;
    if !unpacked.starts_with(b"\x7fELF") {
        return Ok(BlueprintLaunchPlan {
            console_surface: BlueprintConsoleSurface::Text,
        });
    }
    let imports = crate::hv::blueprint::elf_imports(unpacked.as_slice())
        .map_err(|err| alloc::format!("ELF import scan failed: {}", err))?;
    let console_surface = classify_blueprint_console_surface(imports.as_slice());
    let _ = archive;
    Ok(BlueprintLaunchPlan { console_surface })
}

fn classify_blueprint_memory(
    archive: &str,
    raw_payload_len: usize,
    stats: crate::hv::blueprint::ElfAllocStats,
    imports: &[crate::hv::blueprint::ElfImport<'_>],
) -> BlueprintMemoryClass {
    let audio_player_signal = archive_has(archive, "scope-tui")
        || archive_has(archive, "scope_tui")
        || archive_has(archive, "aud-player-scope-tui")
        || archive_has(archive, "aud_player_scope_tui")
        || import_name_has(imports, "trueos_cabi_audio_")
        || import_name_has(imports, "audio_open_playback")
        || import_name_has(imports, "audio_write_i16");
    if audio_player_signal {
        return BlueprintMemoryClass::AudioPlayer;
    }

    let network_server_signal = archive_has(archive, "server")
        || import_name_has(imports, "trueos_mio_tcp_listener_")
        || import_name_has(imports, "tcp_listener")
        || imports_libc_tcp_listener(imports);
    if network_server_signal {
        return BlueprintMemoryClass::NetworkServer;
    }

    let server_signal = archive_has(archive, "horizon")
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

    let heavy_graphics_signal = archive_has(archive, "mandelbrot")
        || archive_has(archive, "skybox")
        || archive_has(archive, "particle")
        || archive_has(archive, "virgl")
        || stats.alloc_bytes > 4 * MIB
        || raw_payload_len > 8 * MIB;
    if heavy_graphics_signal {
        return BlueprintMemoryClass::HeavyGraphics;
    }

    let tokio_signal = archive_has(archive, "tokio")
        || import_name_has(imports, "trueos_tokio_")
        || import_name_has(imports, "tokio_")
        || import_name_has(imports, "sleep_ms");
    if tokio_signal {
        return BlueprintMemoryClass::TokioRuntime;
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
            BlueprintMemoryClass::TokioRuntime => (
                64,
                round_pow2_mib(base_live_mib.saturating_mul(12).saturating_add(64)).max(128),
                256,
                8,
                16,
                64,
            ),
            BlueprintMemoryClass::AudioPlayer => (
                128,
                round_pow2_mib(base_live_mib.saturating_mul(24).saturating_add(128)).max(256),
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
            BlueprintMemoryClass::NetworkServer => (
                128,
                round_pow2_mib(base_live_mib.saturating_mul(64).saturating_add(256)).max(512),
                1024,
                16,
                32,
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
        "apps: profile {} heap={}/{}/{}MiB stack={}/{}/{}MiB",
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

fn log_blueprint_import(import: &crate::hv::blueprint::ElfImport<'_>, log: &dyn Fn(&str)) {
    match import.resolved_addr {
        Some(addr) if crate::hv::blueprint::is_joker_import(import.name) => {
            let note = crate::hv::blueprint::rustc_runtime_import_note(import.name);
            let marker_only =
                note.is_some_and(|note| note.starts_with("class=rustc-no-alloc-shim "));
            let line = if let Some(note) = note {
                alloc::format!("rust-runtime import {} -> 0x{:x} {}", import.name, addr, note)
            } else {
                alloc::format!("rust-runtime import {} -> 0x{:x}", import.name, addr)
            };
            if marker_only {
                crate::log_info!(target: "apps"; "{}\n", line.as_str());
            } else {
                crate::log_warn!(target: "apps"; "{}\n", line.as_str());
            }
            log(line.as_str());
        }
        Some(addr) => {
            crate::log_info!(
                target: "apps";
                "import {} -> 0x{:x}\n",
                import.name,
                addr
            );
        }
        None => {
            let line = alloc::format!("unresolved import {}", import.name);
            crate::log_warn!(target: "apps"; "{}\n", line.as_str());
            log(line.as_str());
        }
    }
}

async fn preflight_blueprint_launch(
    archive: &str,
    module_bytes: &[u8],
    log: &dyn Fn(&str),
    waived_readiness: u32,
) -> Result<BlueprintLaunchPlan, String> {
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
        crate::log_warn!(
            target: "apps";
            "apps: unpacked payload size does not match header_raw\n"
        );
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
            Err(err) => {
                crate::log_warn!(target: "apps"; "apps: ELF diag failed: {}\n", err);
                log(alloc::format!("apps: ELF diag failed: {}", err).as_str());
            }
        }
    } else {
        crate::log_warn!(target: "apps"; "apps: unpacked payload does not look like ELF\n");
        log("apps: unpacked payload does not look like ELF");
    }

    let mut required_readiness = crate::hv::blueprint::prebind_base_readiness();
    let mut imports_for_profile = Vec::new();
    let mut console_surface = BlueprintConsoleSurface::Text;
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
                        if let Some(err) = crate::hv::blueprint::prebind_import_error(import.name) {
                            return Err(String::from(err));
                        }
                        required_readiness |=
                            crate::hv::blueprint::prebind_import_readiness(import.name);
                        log_blueprint_import(import, log);
                    }
                }
                console_surface = classify_blueprint_console_surface(imports.as_slice());
                imports_for_profile = imports;
            }
            Err(err) => {
                crate::log_warn!(target: "apps"; "apps: ELF import scan failed: {}\n", err);
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
    log(alloc::format!("apps: console surface {:?}", console_surface).as_str());
    log_blueprint_memory_profile(profile, log);

    if !unpacked.starts_with(b"\x7fELF")
        || !matches!(crate::hv::blueprint::elf_type_name(unpacked.as_slice()), Some("REL"))
    {
        return Err(String::from("only ELF REL blueprints are supported for app-vm launch"));
    }

    required_readiness &= !waived_readiness;
    let missing_readiness = required_readiness & !crate::r::readiness::mask();
    log(alloc::format!(
        "apps: Blueprint CAPS {} missing={}",
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
                "readiness timeout after {}ms caps={} missing={}",
                BLUEPRINT_READINESS_TIMEOUT.as_millis(),
                readiness_mask_text(required_readiness).as_str(),
                readiness_mask_text(still_missing).as_str()
            ));
        }
        log(alloc::format!(
            "apps: Blueprint CAPS ready {}",
            readiness_mask_text(required_readiness).as_str()
        )
        .as_str());
    }

    Ok(BlueprintLaunchPlan { console_surface })
}

fn start_blueprint_launch(
    spawner: &Spawner,
    request: &AppVmLaunchRequest,
    plan: BlueprintLaunchPlan,
    log: &dyn Fn(&str),
) {
    if request.instance.is_default()
        && let Some(existing_vm) = crate::hv::default_app_instance_vm(request.archive.as_str())
    {
        let label = app_label_for_archive(request.archive.as_str());
        log(alloc::format!(
            "apps: {} default instance is already vm{}",
            request.archive,
            existing_vm
        )
        .as_str());
        log(alloc::format!("apps: launch `{}` again to create an automatic container", label,)
            .as_str());
        log(alloc::format!(
            "apps: use `vmx_stop` inside vm{} to close the default instance",
            existing_vm,
        )
        .as_str());
        let named = crate::hv::named_app_instance_vms(request.archive.as_str());
        if !named.is_empty() {
            let mut live = String::new();
            for (vm_id, name) in named.iter() {
                if !live.is_empty() {
                    live.push_str(", ");
                }
                live.push_str(alloc::format!("vm{} [{}]", vm_id, name).as_str());
            }
            log(alloc::format!("apps: already running containers: {}", live.as_str()).as_str());
        }
        return;
    }
    let Some(vm_id) = crate::hv::first_free_vm_id() else {
        log("apps: no free app-vm ids");
        return;
    };

    match crate::hv::start_blueprint_app_vm(
        vm_id,
        spawner,
        request.archive.clone(),
        request.module_bytes.clone(),
        request.app_args.clone(),
        request.launch_script.clone(),
        request.instance.clone(),
        Some(request.target.clone()),
        plan.console_surface,
    ) {
        Ok(()) => {
            crate::log!(
                "app-vm-run-queue: hv start ok vm={} archive={}\n",
                vm_id,
                request.archive.as_str()
            );
            log(alloc::format!("apps: vm{} launch requested", vm_id).as_str());
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
    crate::r::readiness::for_each_flag(mask, |flag, name| {
        if !out.is_empty() {
            out.push('|');
        }
        out.push_str(readiness_friendly_label(flag, name));
    });
    if out.is_empty() {
        alloc::format!("0x{:08x}", mask)
    } else {
        out
    }
}

fn readiness_friendly_label(flag: u32, fallback: &'static str) -> &'static str {
    match flag {
        crate::r::readiness::NET_ANY_CONFIGURED => "BP_NETWORK",
        crate::r::readiness::NET_SOCKET_READY => "SOCK_NETWORK",
        crate::r::readiness::TLS_SOCKET_SERVICE_READY => "TLS_NETWORK",
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED => "FILESYSTEM_MOUNTED",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY => "WORKER_AP_RDY",
        crate::r::readiness::RAYON_READY => "RAYON_READY",
        crate::r::readiness::TRUEOSFS_INDEX_READY => "FILESYSTEM_INDEX_RDY",
        _ => fallback,
    }
}

#[trueos_executor::task(pool_size = 1)]
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
        release_matrix_target_vm_reservation(&target);
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn enqueue_blueprint_bytes(
    target: MatrixTarget,
    archive: String,
    module_bytes: Vec<u8>,
    app_args: Vec<String>,
) -> Result<(), String> {
    enqueue_blueprint_bytes_with_instance(
        target,
        archive,
        module_bytes,
        app_args,
        crate::hv::BlueprintInstanceRequest::default(),
    )
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn enqueue_blueprint_bytes_with_instance(
    target: MatrixTarget,
    archive: String,
    module_bytes: Vec<u8>,
    app_args: Vec<String>,
    instance: crate::hv::BlueprintInstanceRequest,
) -> Result<(), String> {
    enqueue_blueprint_bytes_with_instance_and_launch_script(
        target,
        archive,
        module_bytes,
        app_args,
        instance,
        None,
    )
}

pub(crate) fn enqueue_blueprint_bytes_with_instance_and_launch_script(
    target: MatrixTarget,
    archive: String,
    module_bytes: Vec<u8>,
    app_args: Vec<String>,
    instance: crate::hv::BlueprintInstanceRequest,
    launch_script: Option<String>,
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

    let instance = name_occupied_default_instance(&target, archive.as_str(), instance);
    let target = reserve_target_for_archive(&target, archive.as_str());
    let app_label = app_label_for_instance(archive.as_str(), &instance);
    let app_sha256 = Sha256::digest(module_bytes.as_slice()).into();
    set_matrix_target_app_identity(&target, app_label.as_str(), app_sha256);
    let line = alloc::format!("apps: queued {}", app_label);
    log_run_target_line(&target, line.as_str());
    enqueue_blueprint_request(
        target,
        archive,
        "direct",
        module_bytes,
        app_args,
        launch_script,
        instance,
        false,
        None,
    );
    Ok(())
}

async fn preflight_archive_name_to_target_async(
    target: &MatrixTarget,
    archive_name: &str,
    module_bytes: &[u8],
    waived_readiness: u32,
) -> Result<(), String> {
    let log = |line: &str| log_run_target_line(target, line);
    preflight_blueprint_launch(archive_name, module_bytes, &log, waived_readiness)
        .await
        .map(|_| ())
}

async fn submit_module_bytes_to_target_async(
    target: MatrixTarget,
    archive_name: &str,
    module_bytes: Vec<u8>,
    app_args: Vec<String>,
    instance: crate::hv::BlueprintInstanceRequest,
    launch_script: Option<String>,
    source: &'static str,
    waived_readiness: u32,
    console_surface_override: Option<BlueprintConsoleSurface>,
) -> Result<&'static str, String> {
    preflight_archive_name_to_target_async(
        &target,
        archive_name,
        module_bytes.as_slice(),
        waived_readiness,
    )
    .await?;
    let mut required_readiness = crate::hv::blueprint::prebind_required_readiness(
        module_bytes.as_slice(),
    )
    .map_err(|err| {
        let line = alloc::format!("apps: not queued {} {}", archive_name, err.as_str());
        log_run_target_line(&target, line.as_str());
        line
    })?;
    required_readiness &= !waived_readiness;
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
    let instance = name_occupied_default_instance(&target, archive_name, instance);
    crate::allocators::with_host_alloc_domain(|| {
        let target = reserve_target_for_archive(&target, archive_name);
        let app_label = app_label_for_instance(archive_name, &instance);
        let app_sha256 = Sha256::digest(module_bytes.as_slice()).into();
        set_matrix_target_app_identity(&target, app_label.as_str(), app_sha256);
        let line = alloc::format!("apps: queued {}", app_label);
        log_run_target_line(&target, line.as_str());
        enqueue_blueprint_request(
            target,
            String::from(archive_name),
            source,
            module_bytes,
            app_args,
            launch_script,
            instance,
            true,
            console_surface_override,
        );
    });
    Ok(source)
}

pub(crate) async fn submit_archive_name_to_target_from_app_db_async(
    target: MatrixTarget,
    archive_name: &str,
    app_args: Vec<String>,
) -> Result<&'static str, String> {
    submit_archive_name_to_target_from_app_db_with_instance_async(
        target,
        archive_name,
        app_args,
        crate::hv::BlueprintInstanceRequest::default(),
    )
    .await
}

pub(crate) async fn submit_archive_name_to_target_from_app_db_with_launch_script_async(
    target: MatrixTarget,
    archive_name: &str,
    launch_script: String,
) -> Result<&'static str, String> {
    submit_archive_name_to_target_from_app_db_with_instance_and_launch_script_async(
        target,
        archive_name,
        Vec::new(),
        crate::hv::BlueprintInstanceRequest::default(),
        Some(launch_script),
    )
    .await
}

/// Launch an archive as a specific instance.
///
/// The default request claims the single shared default slot, so a caller that
/// wants a second live copy of the same archive must name it. Naming is the
/// only difference: the archive itself is unchanged.
pub(crate) async fn submit_archive_name_to_target_from_app_db_with_instance_async(
    target: MatrixTarget,
    archive_name: &str,
    app_args: Vec<String>,
    instance: crate::hv::BlueprintInstanceRequest,
) -> Result<&'static str, String> {
    submit_archive_name_to_target_from_app_db_with_instance_and_launch_script_async(
        target,
        archive_name,
        app_args,
        instance,
        None,
    )
    .await
}

/// Launch a Blueprint as a self-contained UI4 application.
///
/// Unlike a launch entered from an existing Shell2 backend, a desktop launch
/// has no invoking terminal to lease. The application may still create its own
/// Shell2 frontend and receive input through its UI4 frame; only the optional
/// parent-terminal handoff is disabled.
pub(crate) async fn submit_archive_name_to_target_from_app_db_with_instance_detached_ui_async(
    target: MatrixTarget,
    archive_name: &str,
    app_args: Vec<String>,
    instance: crate::hv::BlueprintInstanceRequest,
) -> Result<&'static str, String> {
    if let Some(module_bytes) = crate::app_db::get(archive_name)? {
        return submit_module_bytes_to_target_async(
            target,
            archive_name,
            module_bytes,
            app_args,
            instance,
            None,
            "app.db",
            0,
            Some(BlueprintConsoleSurface::Text),
        )
        .await;
    }

    Err(String::from("archive not found"))
}

/// Launch a built-in Blueprint whose selected mode provably does not exercise
/// one of its optional imported services. The caller owns that narrower mode
/// contract; ordinary app launches continue to require every imported service.
pub(crate) async fn submit_archive_name_to_target_from_app_db_with_instance_waiving_readiness_noninteractive_async(
    target: MatrixTarget,
    archive_name: &str,
    app_args: Vec<String>,
    instance: crate::hv::BlueprintInstanceRequest,
    waived_readiness: u32,
) -> Result<&'static str, String> {
    if let Some(module_bytes) = crate::app_db::get(archive_name)? {
        return submit_module_bytes_to_target_async(
            target,
            archive_name,
            module_bytes,
            app_args,
            instance,
            None,
            "app.db",
            waived_readiness,
            Some(BlueprintConsoleSurface::Text),
        )
        .await;
    }

    Err(String::from("archive not found"))
}

async fn submit_archive_name_to_target_from_app_db_with_instance_and_launch_script_async(
    target: MatrixTarget,
    archive_name: &str,
    app_args: Vec<String>,
    instance: crate::hv::BlueprintInstanceRequest,
    launch_script: Option<String>,
) -> Result<&'static str, String> {
    if let Some(module_bytes) = crate::app_db::get(archive_name)? {
        return submit_module_bytes_to_target_async(
            target,
            archive_name,
            module_bytes,
            app_args,
            instance,
            launch_script,
            "app.db",
            0,
            None,
        )
        .await;
    }

    Err(String::from("archive not found"))
}

pub(crate) async fn submit_archive_name_to_target_from_app_db_default_async(
    target: MatrixTarget,
    archive_name: &str,
    app_args: Vec<String>,
) -> Result<&'static str, String> {
    if let Some(module_bytes) = crate::app_db::get(archive_name)? {
        return submit_module_bytes_to_target_async(
            target,
            archive_name,
            module_bytes,
            app_args,
            crate::hv::BlueprintInstanceRequest::default(),
            None,
            "app.db",
            0,
            None,
        )
        .await;
    }

    Err(String::from("archive not found"))
}

async fn submit_archive_entry(
    target: MatrixTarget,
    entry: &ArchiveEntry,
    app_args: Vec<String>,
    instance: crate::hv::BlueprintInstanceRequest,
) -> bool {
    let module_bytes = match crate::app_db::get(entry.archive.as_str()) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            print_matrix_target_system_line(&target, "apps: selected app.db entry disappeared");
            return false;
        }
        Err(err) => {
            print_matrix_target_system_line(
                &target,
                alloc::format!("apps: failed to read app.db: {}", err).as_str(),
            );
            return false;
        }
    };

    match submit_module_bytes_to_target_async(
        target.clone(),
        entry.archive.as_str(),
        module_bytes,
        app_args,
        instance,
        None,
        "app.db",
        0,
        None,
    )
    .await
    {
        Ok(_) => true,
        Err(err) => {
            print_matrix_target_system_line(
                &target,
                alloc::format!("apps: start refused: {}", err).as_str(),
            );
            false
        }
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) async fn submit_archive_id(
    target: MatrixTarget,
    width: usize,
    id: usize,
    app_args: Vec<String>,
) -> bool {
    let selector = id.to_string();
    submit_archive_selector(
        target,
        width,
        selector.as_str(),
        app_args,
        crate::hv::BlueprintInstanceRequest::default(),
    )
    .await
}

fn archive_match_key(value: &str) -> &str {
    let leaf = value.trim().rsplit('/').next().unwrap_or(value);
    let suffix_at = leaf.len().saturating_sub(3);
    if leaf.is_char_boundary(suffix_at)
        && leaf
            .get(suffix_at..)
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".bp"))
    {
        &leaf[..suffix_at]
    } else {
        leaf
    }
}

pub(crate) async fn submit_archive_selector(
    target: MatrixTarget,
    width: usize,
    selector: &str,
    app_args: Vec<String>,
    instance: crate::hv::BlueprintInstanceRequest,
) -> bool {
    let archives = match archive_entries().await {
        Ok(archives) => archives,
        Err(err) => {
            print_matrix_target_system_line(&target, alloc::format!("apps: {}", err).as_str());
            return false;
        }
    };
    let archive = selector
        .parse::<usize>()
        .ok()
        .and_then(|id| id.checked_sub(1))
        .and_then(|idx| archives.get(idx))
        .or_else(|| {
            let requested = archive_match_key(selector);
            archives.iter().find(|entry| {
                archive_match_key(entry.archive.as_str()).eq_ignore_ascii_case(requested)
            })
        });
    let Some(archive) = archive else {
        print_matrix_target_system_line(&target, "apps: unknown app id or name");
        print_archive_table(&target, width, archives.as_slice());
        return false;
    };
    submit_archive_entry(target, archive, app_args, instance).await
}
