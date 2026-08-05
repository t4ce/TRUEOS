use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use sha2::{Digest, Sha256};
use spin::Mutex;

use super::super::{
    MatrixTarget, matrix_target_interrupted, print_matrix_target_system_line,
    release_matrix_target_vm_reservation, reserve_matrix_target_for_vm_slot_selected,
    set_matrix_target_active, set_matrix_target_app_label,
};
use super::tlb_helper::TlbTable;
use crate::hv::BlueprintConsoleSurface;

const TABLE_HEADERS: &[&str; 4] = &["id", "module", "source", "updated"];
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
    Trueosfs { path: String },
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

// Shell2 runs inside the BSP executor. Keep the complete app discovery and
// loading path natively async: `kfs` is a compatibility API for AP blocking
// lanes, and synchronously polling this executor recursively can stall USB.
async fn trueosfs_archives() -> Result<Vec<ArchiveEntry>, &'static str> {
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        return Ok(Vec::new());
    };

    let root_listing = crate::r::fs::trueosfs::list_dir_async(disk, "")
        .await
        .map_err(|_| "root listing failed")?;
    let apps_listing = crate::r::fs::trueosfs::list_dir_async(disk, "apps")
        .await
        .map_err(|_| "app root listing failed")?;
    let local_compile_listing =
        crate::r::fs::trueosfs::list_dir_async(disk, "apps/common/localcompile")
            .await
            .ok()
            .flatten();
    let root_listing = root_listing.ok_or("root is not TRUEOSFS")?;

    let mut out = Vec::new();
    for name in root_listing
        .lines()
        .map(str::trim)
        .filter(|name| is_runnable_root_artifact(name))
    {
        out.push(ArchiveEntry {
            archive: String::from(name),
            source: ArchiveSource::Trueosfs {
                path: String::from(name),
            },
            updated: root_archive_updated(disk, name).await,
        });
    }

    for app_dir in apps_listing.unwrap_or_default().lines().map(str::trim) {
        if app_dir.is_empty() || app_dir == "common" || app_dir == ".keep" {
            continue;
        }
        let dir = alloc::format!("apps/{}", app_dir);
        let listing = crate::r::fs::trueosfs::list_dir_async(disk, dir.as_str())
            .await
            .map_err(|_| "app directory listing failed")?
            .unwrap_or_default();
        for name in listing
            .lines()
            .map(str::trim)
            .filter(|name| is_runnable_root_artifact(name))
        {
            let path = alloc::format!("apps/{}/{}", app_dir, name);
            if out.iter().any(|entry| entry.archive == name) {
                continue;
            }
            out.push(ArchiveEntry {
                archive: String::from(name),
                source: ArchiveSource::Trueosfs { path },
                updated: None,
            });
        }
    }

    // The native compiler publishes launchable results to the one explicit
    // `/common/localcompile` handoff directory. Keep all other common files
    // out of the app namespace.
    for name in local_compile_listing
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|name| is_runnable_root_artifact(name))
    {
        if out.iter().any(|entry| entry.archive == name) {
            continue;
        }
        out.push(ArchiveEntry {
            archive: String::from(name),
            source: ArchiveSource::Trueosfs {
                path: alloc::format!("apps/common/localcompile/{}", name),
            },
            updated: None,
        });
    }
    out.sort_by(|a, b| a.archive.cmp(&b.archive));
    Ok(out)
}

fn root_archive_timestamp_path(name: &str) -> String {
    alloc::format!("apps/common/.bp-meta/root/{}.updated", name)
}

async fn root_archive_updated(
    disk: crate::disc::block::DeviceHandle,
    name: &str,
) -> Option<String> {
    let stamp_path = root_archive_timestamp_path(name);
    let bytes = crate::r::fs::trueosfs::file_out_async(disk, stamp_path.as_str())
        .await
        .ok()??;
    let text = String::from_utf8_lossy(bytes.as_slice()).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn is_runnable_root_artifact(name: &str) -> bool {
    matches_glob(name, "*.bp")
}

async fn trueosfs_module_by_archive_name(
    disk: crate::disc::block::DeviceHandle,
    archive_name: &str,
) -> Result<Option<(Vec<u8>, &'static str)>, String> {
    if let Some(module_bytes) = crate::r::fs::trueosfs::file_out_async(disk, archive_name)
        .await
        .map_err(|_| String::from("failed to read selected module from TRUEOSFS"))?
    {
        verify_trueosfs_module_hash(disk, archive_name, module_bytes.as_slice()).await?;
        return Ok(Some((module_bytes, "TRUEOSFS root")));
    }

    let archive_leaf = archive_name.rsplit('/').next().unwrap_or(archive_name);
    let app_dir = crate::hv::blueprint::app_fs_root_for_archive(archive_leaf, &[]);
    let app_path = alloc::format!("{}/{}", app_dir, archive_leaf);
    let module_bytes = crate::r::fs::trueosfs::file_out_async(disk, app_path.as_str())
        .await
        .map_err(|_| String::from("failed to read selected module from TRUEOSFS"))?;
    if let Some(bytes) = module_bytes {
        verify_trueosfs_module_hash(disk, app_path.as_str(), bytes.as_slice()).await?;
        Ok(Some((bytes, "TRUEOSFS app")))
    } else {
        Ok(None)
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

async fn verify_trueosfs_module_hash(
    disk: crate::disc::block::DeviceHandle,
    path: &str,
    bytes: &[u8],
) -> Result<Option<String>, String> {
    let hash_path = alloc::format!("{}.sha256", path);
    let expected_bytes =
        match crate::r::fs::trueosfs::file_out_async(disk, hash_path.as_str()).await {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Ok(None),
            Err(err) => {
                return Err(alloc::format!(
                    "SHA-256 metadata read failed path={} err={:?}",
                    hash_path,
                    err
                ));
            }
        };
    let expected = core::str::from_utf8(expected_bytes.as_slice())
        .map(str::trim)
        .map_err(|_| alloc::format!("invalid SHA-256 metadata path={}", hash_path))?;
    if expected.len() != 64 || !expected.as_bytes().iter().all(u8::is_ascii_hexdigit) {
        return Err(alloc::format!("invalid SHA-256 metadata path={}", hash_path));
    }
    let actual = sha256_hex(bytes);
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(alloc::format!(
            "SHA-256 mismatch path={} expected={} actual={}",
            path,
            expected,
            actual
        ));
    }
    Ok(Some(String::from(expected)))
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

async fn archive_entries() -> Result<Vec<ArchiveEntry>, &'static str> {
    let mut out = trueosfs_archives().await?;
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
        ArchiveSource::Trueosfs { path } if path.starts_with("apps/common/") => "TRUEOSFS common",
        ArchiveSource::Trueosfs { path } if path.starts_with("apps/") => "TRUEOSFS app",
        ArchiveSource::Trueosfs { .. } => "TRUEOSFS root",
        ArchiveSource::EmbeddedModule { .. } => "boot embedded",
    }
}

fn archive_display_path(entry: &ArchiveEntry) -> &str {
    match &entry.source {
        ArchiveSource::Trueosfs { path } if path.starts_with("apps/") => path.as_str(),
        _ => entry.archive.as_str(),
    }
}

fn print_archive_table(target: &MatrixTarget, width: usize, archives: &[ArchiveEntry]) {
    let table = TlbTable::with_width(TABLE_HEADERS, width.saturating_sub(2));
    table.emit_header(|text| print_matrix_target_system_line(target, text));
    for (idx, archive) in archives.iter().enumerate() {
        let id = alloc::format!("{}", idx + 1);
        let row = [
            id.as_str(),
            archive_display_path(archive),
            source_label(&archive.source),
            archive.updated.as_deref().unwrap_or("-"),
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
    });
}

fn log_run_target_line(_target: &MatrixTarget, line: &str) {
    log_blueprint_line(line);
}

fn log_blueprint_line(line: &str) {
    crate::log_os::blueprint_line(log::Level::Info, format_args!("{}\n", line));
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
    let uses_unix_raw_tty = import_name_is(imports, "cfmakeraw")
        || import_name_is(imports, "tcsetattr")
        || (import_name_is(imports, "tcgetattr") && import_name_is(imports, "isatty"));

    if uses_konsole || uses_raw_shell2 || uses_unix_raw_tty {
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
        release_matrix_target_vm_reservation(&target);
    }
}

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
    set_matrix_target_app_label(&target, app_label.as_str());
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
    );
    Ok(())
}

async fn preflight_archive_name_to_target_async(
    target: &MatrixTarget,
    archive_name: &str,
    module_bytes: &[u8],
) -> Result<(), String> {
    let log = |line: &str| log_run_target_line(target, line);
    preflight_blueprint_launch(archive_name, module_bytes, &log)
        .await
        .map(|_| ())
}

async fn submit_module_bytes_to_target_async(
    target: MatrixTarget,
    archive_name: &str,
    module_bytes: Vec<u8>,
    app_args: Vec<String>,
    instance: crate::hv::BlueprintInstanceRequest,
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
    let instance = name_occupied_default_instance(&target, archive_name, instance);
    crate::allocators::with_host_alloc_domain(|| {
        let target = reserve_target_for_archive(&target, archive_name);
        let app_label = app_label_for_instance(archive_name, &instance);
        set_matrix_target_app_label(&target, app_label.as_str());
        let line = alloc::format!("apps: queued {}", app_label);
        log_run_target_line(&target, line.as_str());
        enqueue_blueprint_request(
            target,
            String::from(archive_name),
            source,
            module_bytes,
            app_args,
            None,
            instance,
            true,
        );
    });
    Ok(source)
}

pub(crate) async fn submit_archive_name_to_target_prefer_trueosfs_async(
    target: MatrixTarget,
    archive_name: &str,
    app_args: Vec<String>,
) -> Result<&'static str, String> {
    submit_archive_name_to_target_prefer_trueosfs_with_instance_async(
        target,
        archive_name,
        app_args,
        crate::hv::BlueprintInstanceRequest::default(),
    )
    .await
}

/// Launch an archive as a specific instance.
///
/// The default request claims the single shared default slot, so a caller that
/// wants a second live copy of the same archive must name it. Naming is the
/// only difference: the archive itself is unchanged.
pub(crate) async fn submit_archive_name_to_target_prefer_trueosfs_with_instance_async(
    target: MatrixTarget,
    archive_name: &str,
    app_args: Vec<String>,
    instance: crate::hv::BlueprintInstanceRequest,
) -> Result<&'static str, String> {
    if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
        if let Some((module_bytes, source)) =
            trueosfs_module_by_archive_name(disk, archive_name).await?
        {
            return submit_module_bytes_to_target_async(
                target,
                archive_name,
                module_bytes,
                app_args,
                instance,
                source,
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
            instance,
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
            crate::hv::BlueprintInstanceRequest::default(),
            "boot embedded",
        )
        .await;
    }

    if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
        if let Some((module_bytes, source)) =
            trueosfs_module_by_archive_name(disk, archive_name).await?
        {
            return submit_module_bytes_to_target_async(
                target,
                archive_name,
                module_bytes,
                app_args,
                crate::hv::BlueprintInstanceRequest::default(),
                source,
            )
            .await;
        }
    }

    Err(String::from("archive not found"))
}

async fn submit_archive_entry(
    target: MatrixTarget,
    entry: &ArchiveEntry,
    app_args: Vec<String>,
    instance: crate::hv::BlueprintInstanceRequest,
) -> bool {
    let (module_bytes, source) = match &entry.source {
        ArchiveSource::Trueosfs { path } => {
            let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
                print_matrix_target_system_line(&target, "apps: no TRUEOSFS root mounted");
                return false;
            };
            let module_bytes =
                match crate::r::fs::trueosfs::file_out_async(disk, path.as_str()).await {
                    Ok(Some(bytes)) => bytes,
                    Ok(None) => {
                        print_matrix_target_system_line(
                            &target,
                            "apps: selected TRUEOSFS module disappeared",
                        );
                        return false;
                    }
                    Err(err) => {
                        print_matrix_target_system_line(
                            &target,
                            alloc::format!(
                                "apps: failed to read selected module from TRUEOSFS: {:?}",
                                err
                            )
                            .as_str(),
                        );
                        return false;
                    }
                };
            match verify_trueosfs_module_hash(disk, path.as_str(), module_bytes.as_slice()).await {
                Ok(Some(hash)) => print_matrix_target_system_line(
                    &target,
                    alloc::format!("apps: SHA-256 verified {} {}", entry.archive, hash).as_str(),
                ),
                Ok(None) => {}
                Err(err) => {
                    print_matrix_target_system_line(
                        &target,
                        alloc::format!("apps: start refused: {}", err).as_str(),
                    );
                    return false;
                }
            }
            (module_bytes, source_label(&entry.source))
        }
        ArchiveSource::EmbeddedModule { cmdline } => {
            let Some(module_bytes) = crate::limine::module_bytes_by_string(cmdline.as_bytes())
            else {
                print_matrix_target_system_line(
                    &target,
                    "apps: failed to read selected embedded module",
                );
                return false;
            };
            let bytes = crate::allocators::with_host_alloc_domain(|| module_bytes.to_vec());
            (bytes, "boot embedded")
        }
    };

    match submit_module_bytes_to_target_async(
        target.clone(),
        entry.archive.as_str(),
        module_bytes,
        app_args,
        instance,
        source,
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
