use alloc::{string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_executor::{SendSpawner, SpawnError, SpawnToken, Spawner};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::r::spawn_spec::{SpawnAttempt, SpawnPlacement, TaskSpec};
// NOTE: This file is intended to become the single source of truth for Embassy task startup.

const SPAWN_SERVICE_AFTER_START_MS: u64 = 25;
const SPAWN_SERVICE_PENDING_MS: u64 = 100;
const SPAWN_SERVICE_IDLE_MS: u64 = 500;

/// Central task orchestrator ("FSM spawn service").
///
/// Ideal-world model:
/// - One file owns the boot task registry (what runs + under which readiness conditions).
/// - Individual tasks can still contain internal gating today; later we can delete those
///   once this registry is trusted.
/// - Readiness is monotonic, so this service only ever adds tasks; it never stops them.
///
/// This is intentionally simple: a small polling loop over a static registry.

macro_rules! define_started_flags {
    ($($name:ident),+ $(,)?) => {
        $(static $name: AtomicBool = AtomicBool::new(false);)+
    };
}

define_started_flags!(
    JOB_RUNNER_STARTED,
    GLOBALOG_PERSIST_ONCE_STARTED,
    QJS_ASYNC_FS_SERVICE_STARTED,
    TRUEOSFS_MOUNT_SERVICE_STARTED,
    TRUEOSFS_INDEX_SERVICE_STARTED,
    HV_VM_STORE_STARTED,
    HV_VM_STORE_NET_STARTED,
    NET_POLL_STARTED,
    NET_SERVICE_STARTED,
    TLS_SOCKET_SERVICE_STARTED,
    NTP_SYNC_STARTED,
    SNTP_SERVICE_STARTED,
    NET_SHELL_STARTED,
    AI_QJS_ONESHOT_STARTED,
    HTTP_TRUEOSFS_STARTED,
    HYPER_HTTP1_PROBE_STARTED,
    CHAT_HTTP_STARTED,
    MAIL_HTTP_STARTED,
    AXUM_BOOT_STARTED,
    FILEEXPLORER_HTTP_STARTED,
    WS_TIME_STARTED,
    ESP_GATE_STARTED,
    ESP_GATE_REGISTRY_STARTED,
    ESP_PIANO_UDP_STARTED,
    FTP_SERVER_STARTED,
    TGA_TASK_STARTED,
    GFX_VIRGL_READY_TASK_STARTED,
    GFX_VIRGL_CURSOR_OVERLAY_STARTED,
    INTEL_CURSOR_SERVICE_STARTED,
    INTEL_HDA_PROBE_STARTED,
    RAPLE_SERVICE_STARTED,
    GFX_TEXTURE_UPLOAD_SERVICE_STARTED,
    HTML_SHACK_SERVICE_STARTED,
    UI2_HOSTED_SYNC_TASK_STARTED,
    UI2_HIT_TASK_STARTED,
    UI2_STARTED,
    UI2_ATHLAS_THIRD_DEMO_STARTED,
    UI2_ATHLAS_HALF_DEMO_STARTED,
    UI2_ATHLAS_1X_DEMO_STARTED,
    UI2_ATHLAS_2X_DEMO_STARTED,
    UI2_PALATINO_1X_DEMO_STARTED,
    UI2_TWEMOJI_1X_STARTED,
    UI2_ANALOG_CLOCK_DEMO_STARTED,
    UI2_TEXT_INPUT_DEMO_STARTED,
    UI2_BGRT_DEMO_STARTED,
    UI2_CORETICKS_DEMO_STARTED,
    UI2_CURSORPICKER_DEMO_STARTED,
    UI2_MANDELBROT_DEMO_STARTED,
    UI2_PLAYER_DEMO_STARTED,
    UI2_RAPLE_DEMO_STARTED,
    UI2_SMILEY_FOUNTAIN_DEMO_STARTED,
    UI2_SHELL_DEMO_STARTED,
    UI2_SWARM_DEMO_STARTED,
    USB_CONTROLLER_TASKS_STARTED,
    TRUEOSFS_READY_HOOK_STARTED,
    BP_AUTOSTART_STARTED,
    BOOT_WS_SMOKE_STARTED,
    BOOT_NETBENCH_STARTED,
    BOOT_ASSET_FETCH_STARTED,
    APP_VM_RUN_QUEUE_STARTED,
    FACTORY_RAM_PROBE_STARTED,
    UART_SHELL_STARTED,
    NET_TCP_SHELL_STARTED,
    LOGTOTCP_STARTED,
    LUMEN_SERVICE_STARTED,
    PCIIDS_GIT_STARTED,
    ATOMIC_BOMB_STARTED,
    HTML_DEMO_STARTED,
    SURFER_FACTORY_STARTED
);

macro_rules! define_stop_flags {
    ($($name:ident),* $(,)?) => {
        $(static $name: AtomicBool = AtomicBool::new(false);)*
    };
}

define_stop_flags!(
    STOP_UI2_TEXT_INPUT_DEMO,
    STOP_UI2_ANALOG_CLOCK_DEMO,
    STOP_UI2_BGRT_DEMO,
    STOP_UI2_CORETICKS_DEMO,
    STOP_UI2_CURSORPICKER_DEMO,
    STOP_UI2_MANDELBROT_DEMO,
    STOP_UI2_PLAYER_DEMO,
    STOP_UI2_RAPLE_DEMO,
    STOP_UI2_SMILEY_FOUNTAIN_DEMO,
    STOP_UI2_SHELL_DEMO,
    STOP_UI2_SWARM_DEMO,
);

fn stop_flag_by_task_name(name: &str) -> Option<&'static AtomicBool> {
    match name {
        "ui2-text-input-demo" => Some(&STOP_UI2_TEXT_INPUT_DEMO),
        "ui2-analog-clock-demo" => Some(&STOP_UI2_ANALOG_CLOCK_DEMO),
        "ui2-bgrt-demo" => Some(&STOP_UI2_BGRT_DEMO),
        "ui2-coreticks-demo" => Some(&STOP_UI2_CORETICKS_DEMO),
        "ui2-cursorpicker-demo" => Some(&STOP_UI2_CURSORPICKER_DEMO),
        "ui2-mandelbrot-demo" => Some(&STOP_UI2_MANDELBROT_DEMO),
        "ui2-player-demo" => Some(&STOP_UI2_PLAYER_DEMO),
        "ui2-raple-demo" => Some(&STOP_UI2_RAPLE_DEMO),
        "ui2-smiley-fountain-demo" => Some(&STOP_UI2_SMILEY_FOUNTAIN_DEMO),
        "ui2-shell-demo" => Some(&STOP_UI2_SHELL_DEMO),
        "ui2-swarm-demo" => Some(&STOP_UI2_SWARM_DEMO),
        _ => None,
    }
}

pub struct TaskRunGuard {
    name: &'static str,
}

impl Drop for TaskRunGuard {
    fn drop(&mut self) {
        task_exited(self.name);
    }
}

pub fn task_run_guard(name: &'static str) -> TaskRunGuard {
    if let Some(flag) = stop_flag_by_task_name(name) {
        flag.store(false, Ordering::Release);
    }
    TaskRunGuard { name }
}

pub fn task_stop_requested(name: &str) -> bool {
    stop_flag_by_task_name(name)
        .map(|flag| flag.load(Ordering::Acquire))
        .unwrap_or(false)
}

fn task_exited(name: &str) {
    if let Some(flag) = stop_flag_by_task_name(name) {
        flag.store(false, Ordering::Release);
    }
    if let Some(index) = task_index_by_name(name)
        && let Some(spec) = TASKS.get(index)
    {
        spec.started.store(false, Ordering::Release);
    }
}

pub async fn wait_task_or_timeout_ms(name: &str, total_ms: u64) -> bool {
    const CHUNK_MS: u64 = 50;
    let mut remaining_ms = total_ms;
    while remaining_ms != 0 {
        if task_stop_requested(name) {
            return true;
        }
        let sleep_ms = remaining_ms.min(CHUNK_MS);
        Timer::after(EmbassyDuration::from_millis(sleep_ms)).await;
        remaining_ms -= sleep_ms;
    }
    task_stop_requested(name)
}

const TRUESURFER_FACTORY_BOOT_COUNT: u32 = 0;

pub const fn truesurfer_factory_boot_count() -> u32 {
    if TRUESURFER_FACTORY_BOOT_COUNT > trueos_qjs::browser_task::MAX_BROWSER_INSTANCE_ID {
        trueos_qjs::browser_task::MAX_BROWSER_INSTANCE_ID
    } else {
        TRUESURFER_FACTORY_BOOT_COUNT
    }
}

struct TruesurferFactory {
    next_instance_id: u32,
    spawned_mask: u64,
}

impl TruesurferFactory {
    const fn new() -> Self {
        Self {
            next_instance_id: 1,
            spawned_mask: 0,
        }
    }

    fn next_instance_id(&self) -> Option<u32> {
        if self.next_instance_id > trueos_qjs::browser_task::MAX_BROWSER_INSTANCE_ID {
            None
        } else {
            Some(self.next_instance_id)
        }
    }

    fn mark_spawned(&mut self, browser_instance_id: u32) {
        self.next_instance_id = self.next_instance_id.saturating_add(1);
        let bit = 1u64 << browser_instance_id.saturating_sub(1);
        self.spawned_mask |= bit;
    }
    fn spawned_mask(&self) -> u64 {
        self.spawned_mask
    }
}

static TRUESURFER_FACTORY: Mutex<TruesurferFactory> = Mutex::new(TruesurferFactory::new());
static GFX_VIRGL_RETRY_AFTER_MS: AtomicU64 = AtomicU64::new(0);

#[inline]
fn boot_probe_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

#[inline]
fn spawn_local<S>(
    spawner: Spawner,
    task: impl FnOnce(Spawner) -> Result<SpawnToken<S>, SpawnError>,
) -> SpawnAttempt {
    match task(spawner) {
        Ok(token) => {
            spawner.spawn(token);
            SpawnAttempt::Spawned
        }
        Err(e) => SpawnAttempt::Failed(e),
    }
}

#[inline]
fn spawn_on_ap1<S: Send>(
    spawner: Spawner,
    task: impl FnOnce(SendSpawner) -> Result<SpawnToken<S>, SpawnError>,
) -> SpawnAttempt {
    let _ = spawner; // keep signature stable; this task intentionally targets AP1.
    let Some(profile) = crate::cpu::CpuProfile::for_slot(1) else {
        return SpawnAttempt::Skipped;
    };
    let Some(ap1_spawner) = crate::workers::spawner_for_slot(profile.slot()) else {
        return SpawnAttempt::Skipped;
    };
    match task(ap1_spawner) {
        Ok(token) => {
            ap1_spawner.spawn(token);
            SpawnAttempt::Spawned
        }
        Err(e) => SpawnAttempt::Failed(e),
    }
}

#[inline]
fn spawn_on_worker<S: Send>(
    spawner: Spawner,
    task: impl FnOnce(SendSpawner) -> Result<SpawnToken<S>, SpawnError>,
) -> SpawnAttempt {
    let Some(worker_spawner) = crate::workers::pick_background_spawner() else {
        let _ = spawner;
        return SpawnAttempt::Skipped;
    };
    let _ = spawner;
    match task(worker_spawner) {
        Ok(token) => {
            worker_spawner.spawn(token);
            SpawnAttempt::Spawned
        }
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_job_runner(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::wait::job_runner_task())
}

fn spawn_globalog_persist_once(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::globalog::persist_once_task())
}

fn spawn_factory_ram_probe(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| {
        crate::tst_boot_factory_ram_probe::boot_factory_ram_probe_task()
    })
}

fn spawn_qjs_async_fs_service(spawner: Spawner) -> SpawnAttempt {
    if !trueos_qjs::async_fs::claim_service_start() {
        crate::r::readiness::set(crate::r::readiness::QJS_ASYNC_FS_READY);
        return SpawnAttempt::Spawned;
    }

    match trueos_qjs::async_fs::async_fs_service_task() {
        Ok(token) => {
            spawner.spawn(token);
            crate::r::readiness::set(crate::r::readiness::QJS_ASYNC_FS_READY);
            SpawnAttempt::Spawned
        }
        Err(e) => {
            trueos_qjs::async_fs::clear_service_start_claim();
            SpawnAttempt::Failed(e)
        }
    }
}

fn spawn_trueosfs_mount_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::fs::trueosfs::mount_service_task())
}

fn spawn_trueosfs_index_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::fs::trueosfs::index_service_task())
}

fn spawn_hv_vm_store(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::hv::store::vm_store_task())
}

fn spawn_hv_vm_store_net(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::hv::store::vm_store_replication_task())
}

fn spawn_net_poll_tasks(spawner: Spawner) -> SpawnAttempt {
    // Some drivers may fail to report a MAC early; treat any detected NIC as usable.
    let count = crate::net::device_count();
    if count == 0 {
        return SpawnAttempt::Skipped;
    }
    for idx in 0..count {
        match crate::net::adapter::net_poll_task(idx) {
            Ok(token) => spawner.spawn(token),
            Err(e) => crate::log!("net: spawn net_poll_task({}) failed: {:?}\n", idx, e),
        }
    }
    SpawnAttempt::Spawned
}

fn spawn_net_service(spawner: Spawner) -> SpawnAttempt {
    let count = crate::net::device_count();
    if count == 0 {
        return SpawnAttempt::Skipped;
    }

    let mut spawned_any = false;
    for idx in 0..count {
        match crate::net::adapter::net_service_task(idx) {
            Ok(token) => {
                spawner.spawn(token);
                spawned_any = true;
            }
            Err(e) => {
                crate::log!("net: spawn net_service_task({}) failed: {:?}\n", idx, e);
                if !spawned_any {
                    return SpawnAttempt::Failed(e);
                }
            }
        }
    }

    if spawned_any {
        SpawnAttempt::Spawned
    } else {
        SpawnAttempt::Skipped
    }
}

fn spawn_tls_socket_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::net::tls_socket::tls_socket_service_task())
}

fn spawn_ntp_sync(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::ntp::ntp_sync_task())
}

fn spawn_sntp_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::sntp::sntp_service_task())
}

fn spawn_net_shell(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::tst_net_tcp_shell::net_shell_task())
}

fn spawn_logtotcp(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::globalog::logtotcp::logtotcp_task())
}

fn spawn_lumen_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::lumen::lumen_service::lumen_service_task())
}

fn boot_lumen_service_enabled() -> bool {
    crate::allcaps::lumen::BOOT_MODEL_SERVICE
}

fn boot_asset_fetch_enabled() -> bool {
    crate::allcaps::lumen::BOOT_ASSET_FETCH
}

fn spawn_ai_qjs_oneshot(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    SpawnAttempt::Skipped
}

fn spawn_html_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::tst_html_demo::html_demo_task())
}

fn spawn_http_trueosfs(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::tst_http_trueosfs::http_trueosfs_task())
}

fn spawn_hyper_http1_probe(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |_worker_spawner| crate::hyper_probe::hyper_net_probe_task())
}

fn hyper_http1_probe_enabled() -> bool {
    crate::allcaps::probes::HYPER_HTTP1_NET_PROBE
}

fn spawn_chat_http(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::srv::chat::chat_http_service_task())
}

fn spawn_mail_http(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::srv::mail::mail_http_service_task())
}

fn spawn_axum_boot(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::srv::axum_boot::axum_boot_service_task())
}

fn spawn_fileexplorer_http(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| {
        crate::tst_fileexplorer_axum::fileexplorer_http_service_task()
    })
}

fn spawn_ws_time(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::tst_ws_time::ws_time_task())
}

fn spawn_esp_gate(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::esp::esp_gate_task())
}

fn spawn_esp_gate_registry(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::esp::esp_gate_registry_task())
}

fn spawn_esp_piano_udp(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::esp::esp_piano_udp_task())
}

fn spawn_ftp_server(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::ftp::ftp_server_task())
}

fn spawn_tga_task(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::tga::tga_task())
}

#[embassy_executor::task]
async fn gfx_virgl_ready_task() {
    crate::gfx::init(None);

    if crate::r::readiness::is_set(crate::r::readiness::GFX_BACKEND_READY) {
        crate::log!("boot-probe: gfx-virgl-backend-ready ms={}\n", boot_probe_ms());
        return;
    }

    for _ in 0..400 {
        if crate::r::readiness::is_set(crate::r::readiness::GFX_BACKEND_READY) {
            return;
        }
        if crate::r::readiness::is_set(crate::r::readiness::GFX_VIRGL_READY) {
            crate::r::readiness::set(crate::r::readiness::GFX_BACKEND_READY);
            crate::log!("boot-probe: gfx-virgl-backend-ready ms={}\n", boot_probe_ms());
            return;
        }
        if gfx_switched() {
            crate::r::readiness::set(crate::r::readiness::GFX_BACKEND_READY);
            crate::log!("boot-probe: gfx-virgl-backend-ready(switched) ms={}\n", boot_probe_ms());
            return;
        }
        Timer::after(EmbassyDuration::from_millis(25)).await;
    }

    crate::log!(
        "gfx-virgl-backend-ready: timeout virgl_active={} virgl_present_cached={} ready_mask=0x{:08X}\n",
        crate::gfx::is_virgl_active() as u8,
        crate::gfx::is_virgl_present_cached() as u8,
        crate::r::readiness::mask()
    );
}

fn spawn_gfx_virgl_ready_task(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| gfx_virgl_ready_task())
}

#[embassy_executor::task]
async fn gfx_virgl_cursor_overlay_task() {
    crate::log!("boot-probe: gfx-cursor-overlay task start ms={}\n", boot_probe_ms());
    loop {
        let _ = crate::r::io::cabi::kernel_cursor_overlay_tick();
        Timer::after(EmbassyDuration::from_millis(16)).await;
    }
}

fn spawn_gfx_virgl_cursor_overlay_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |_ap1_spawner| gfx_virgl_cursor_overlay_task())
}

#[embassy_executor::task]
async fn intel_cursor_service_task() {
    crate::log!("boot-probe: intel-cursor-service task start ms={}\n", boot_probe_ms());
    loop {
        let _ = crate::intel::update_kernel_hw_cursor();
        Timer::after(EmbassyDuration::from_millis(16)).await;
    }
}

fn spawn_intel_cursor_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| intel_cursor_service_task())
}

fn spawn_intel_hda_probe_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |worker_spawner| {
        let _ = worker_spawner;
        crate::intel_hda_probe::task()
    })
}

fn spawn_raple_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::rapl::raple_service())
}

fn spawn_gfx_texture_upload_service(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |_ap1_spawner| crate::r::io::cabi::texture_upload_service_task())
}

#[inline]
fn gfx_switched() -> bool {
    let now_ms = boot_probe_ms();
    if crate::gfx::is_virgl_active() {
        GFX_VIRGL_RETRY_AFTER_MS.store(0, Ordering::Release);
        return true;
    }
    let retry_after_ms = GFX_VIRGL_RETRY_AFTER_MS.load(Ordering::Acquire);
    if retry_after_ms != 0 && now_ms < retry_after_ms {
        return false;
    }
    if crate::gfx::is_virgl_present_cached() {
        if crate::gfx::switch_to_virgl() {
            GFX_VIRGL_RETRY_AFTER_MS.store(0, Ordering::Release);
            return true;
        }

        // A failed virgl init is usually not recoverable within the next
        // scheduler tick. Back off to avoid log storms and repeated heavy
        // re-initialization while the rest of boot continues.
        GFX_VIRGL_RETRY_AFTER_MS.store(now_ms.saturating_add(1000), Ordering::Release);
    }
    false
}

fn html_fetch_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::tst_html_shack::html_fetch_service())
}

fn spawn_truesurfer_batch(spawner: Spawner, requested: u32) -> SpawnAttempt {
    if requested == 0 {
        return SpawnAttempt::Skipped;
    }

    let mut factory = TRUESURFER_FACTORY.lock();
    let mut spawned_any = false;

    for _ in 0..requested {
        let Some(browser_instance_id) = factory.next_instance_id() else {
            break;
        };

        match spawn_on_worker(spawner, |worker_spawner| {
            let _ = worker_spawner;
            trueos_qjs::browser_task::truesurfer_task(browser_instance_id)
        }) {
            SpawnAttempt::Spawned => {
                factory.mark_spawned(browser_instance_id);
                crate::r::ui2::signal_hosted_browser_factory_mask(factory.spawned_mask());
                spawned_any = true;
                crate::log!(
                    "truesurfer-factory: spawned browser_instance_id={} mask={:#x} remaining={}\n",
                    browser_instance_id,
                    factory.spawned_mask(),
                    trueos_qjs::browser_task::MAX_BROWSER_INSTANCE_ID
                        .saturating_sub(browser_instance_id)
                );
            }
            SpawnAttempt::Skipped => {
                break;
            }
            SpawnAttempt::Failed(e) => {
                if !spawned_any {
                    return SpawnAttempt::Failed(e);
                }
                crate::log!(
                    "truesurfer-factory: spawn failed browser_instance_id={} err={:?}\n",
                    browser_instance_id,
                    e
                );
                break;
            }
        }
    }

    if spawned_any {
        SpawnAttempt::Spawned
    } else {
        SpawnAttempt::Skipped
    }
}

pub fn spawn_truesurfer_tab_with_html() -> Option<u32> {
    let mut factory = TRUESURFER_FACTORY.lock();
    let browser_instance_id = factory.next_instance_id()?;

    match crate::workers::pick_background_spawner() {
        Some(worker_spawner) => {
            match trueos_qjs::browser_task::truesurfer_task(browser_instance_id) {
                Ok(token) => {
                    worker_spawner.spawn(token);
                    factory.mark_spawned(browser_instance_id);
                    crate::r::ui2::signal_hosted_browser_factory_mask(factory.spawned_mask());
                    crate::log!(
                        "truesurfer-factory: handoff-spawned browser_instance_id={} mask={:#x} remaining={}\n",
                        browser_instance_id,
                        factory.spawned_mask(),
                        trueos_qjs::browser_task::MAX_BROWSER_INSTANCE_ID
                            .saturating_sub(browser_instance_id)
                    );
                    Some(browser_instance_id)
                }
                Err(_) => {
                    crate::log!(
                        "truesurfer-factory: handoff-spawn skipped browser_instance_id={}\n",
                        browser_instance_id
                    );
                    None
                }
            }
        }
        None => {
            crate::log!(
                "truesurfer-factory: handoff-spawn skipped browser_instance_id={}\n",
                browser_instance_id
            );
            None
        }
    }
}

fn spawn_truesurfer_factory(spawner: Spawner) -> SpawnAttempt {
    spawn_truesurfer_batch(spawner, truesurfer_factory_boot_count())
}

fn spawn_ui2(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |_ap1_spawner| crate::r::ui2::ui2_task())
}

fn spawn_ui2_hit(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |_ap1_spawner| crate::r::ui2::ui2_hit_task())
}

fn spawn_ui2_hosted(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |_ap1_spawner| crate::r::ui2::ui2_hosted_task())
}

const UI2_DEMOS_ENABLED: bool = true;

#[inline]
fn gfx_backend_boot_gate() -> bool {
    true
}

#[inline]
fn intel_cursor_service_gate() -> bool {
    crate::intel::has_claimed_device()
}

#[inline]
fn ui2_core_task_gate() -> bool {
    true
}

#[inline]
fn ui2_demo_task_gate() -> bool {
    UI2_DEMOS_ENABLED
}

fn spawn_ui2_demo_on_worker<S: Send, F>(spawner: Spawner, spawn: F) -> SpawnAttempt
where
    F: FnOnce(SendSpawner) -> Result<SpawnToken<S>, SpawnError>,
{
    spawn_on_worker(spawner, spawn)
}

fn spawn_ui2_athlas_half_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |_ap1_spawner| crate::r::ui2::ui2_font_bucketproducer_demo_task(0))
}

fn spawn_ui2_athlas_third_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |_ap1_spawner| crate::r::ui2::ui2_font_bucketproducer_demo_task(3))
}

fn spawn_ui2_athlas_1x_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |_ap1_spawner| crate::r::ui2::ui2_font_bucketproducer_demo_task(1))
}

fn spawn_ui2_athlas_2x_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |_ap1_spawner| crate::r::ui2::ui2_font_bucketproducer_demo_task(2))
}

fn spawn_ui2_palatino_1x_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |ap1_spawner| {
        let token = crate::r::ui2::ui2_font_bucketproducer_palatino_demo_task()?;
        ap1_spawner.spawn(token);
        crate::r::ui2::ui2_font_bucketproducer_palatino_bw_demo_task()
    })
}

fn spawn_ui2_twemoji_1x(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |_ap1_spawner| crate::r::ui2::ui2_font_twemoji_loader_task())
}

fn spawn_ui2_text_input_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        let _ = worker_spawner;
        crate::tst_ui2_text_input_demo::ui2_text_input_demo_task()
    })
}

fn spawn_ui2_analog_clock_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        let _ = worker_spawner;
        crate::tst_ui2_analog_clock_demo::ui2_analog_clock_demo_task()
    })
}

fn spawn_ui2_bgrt_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        let _ = worker_spawner;
        crate::tst_ui2_bgrt::ui2_bgrt_demo_task()
    })
}

fn spawn_ui2_coreticks_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        let _ = worker_spawner;
        crate::tst_ui2_coreticks_demo::ui2_coreticks_demo_task()
    })
}

fn spawn_ui2_cursorpicker_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        let _ = worker_spawner;
        crate::tst_ui2_cursorpicker_demo::ui2_cursorpicker_demo_task()
    })
}

fn spawn_ui2_mandelbrot_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        let _ = worker_spawner;
        crate::tst_ui2_mandelbrot_demo::ui2_mandelbrot_demo_task()
    })
}

fn spawn_ui2_player_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        let _ = worker_spawner;
        crate::tst_ui2_player_demo::ui2_player_demo_task()
    })
}

fn spawn_ui2_raple_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        let _ = worker_spawner;
        crate::tst_ui2_raple_demo::ui2_raple_demo_task()
    })
}

fn spawn_ui2_smiley_fountain_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        let _ = worker_spawner;
        crate::tst_ui2_smiley_fountain_demo::ui2_smiley_fountain_demo_task()
    })
}

fn spawn_ui2_shell_demo(spawner: Spawner) -> SpawnAttempt {
    match crate::shell2::task(spawner, &crate::shell2::UI2_SHELL_BACKEND) {
        Ok(token) => spawner.spawn(token),
        Err(e) => return SpawnAttempt::Failed(e),
    }
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        let _ = worker_spawner;
        crate::tst_ui2_shell_demo::ui2_shell_demo_task()
    })
}

fn spawn_ui2_swarm_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        let _ = worker_spawner;
        crate::tst_ui2_swarm::ui2_swarm_demo_task()
    })
}

fn spawn_usb_controller_tasks(spawner: Spawner) -> SpawnAttempt {
    let count = crate::usb2::pci_usb_controllers()
        .len()
        .min(crate::usb2::xhci::MAX_XHCI_CONTROLLERS);
    if count == 0 {
        return SpawnAttempt::Skipped;
    }

    let mut spawned_any = false;
    for i in 0..count {
        match spawn_local(spawner, |_spawner| crate::usb2::crabusb_bsp_service(i)) {
            SpawnAttempt::Spawned => {
                spawned_any = true;
            }
            SpawnAttempt::Failed(e) => {
                crate::log!("spawn-svc: usb-controller-task({}) spawn failed: {:?}\n", i, e);
            }
            SpawnAttempt::Skipped => {}
        }
    }
    if spawned_any {
        SpawnAttempt::Spawned
    } else {
        SpawnAttempt::Skipped
    }
}

const USER_INPUT_RECORD_FLUSH_INTERVAL_SECS: u64 = 120;
const USER_INPUT_RECORD_PATH: &str = "user_input_record.txt";

fn user_input_record_payload(entries: &[String]) -> String {
    let mut payload = String::new();
    for entry in entries {
        payload.push_str(entry.as_str());
        payload.push('\n');
    }
    payload
}

async fn flush_user_input_record_once() {
    let entries: Vec<String> = crate::shell2::take_user_input_record();
    if entries.is_empty() {
        return;
    }

    let appended_lines = entries.len();
    let payload = user_input_record_payload(entries.as_slice());
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        crate::shell2::restore_user_input_record(entries);
        crate::log!("spawn-svc: user-input-record err=no-root\n");
        return;
    };

    match crate::r::fs::trueosfs::file_append_async(
        disk,
        USER_INPUT_RECORD_PATH,
        payload.as_bytes(),
    )
    .await
    {
        Ok(true) => {
            crate::log!("spawn-svc: user-input-record ok appended_lines={}\n", appended_lines);
        }
        Ok(false) => {
            crate::shell2::restore_user_input_record(entries);
            crate::log!("spawn-svc: user-input-record err=append-false\n");
        }
        Err(e) => {
            crate::shell2::restore_user_input_record(entries);
            crate::log!("spawn-svc: user-input-record err={:?}\n", e);
        }
    }
}

#[embassy_executor::task]
async fn trueosfs_ready_hook_task() {
    crate::log!("spawn-svc: trueosfs-ready-hook task online\n");
    crate::intel::run_media_source_warmup_async().await;
    loop {
        flush_user_input_record_once().await;
        Timer::after(EmbassyDuration::from_secs(USER_INPUT_RECORD_FLUSH_INTERVAL_SECS)).await;
    }
}

fn spawn_trueosfs_ready_hook(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| trueosfs_ready_hook_task())
}

fn spawn_boot_ws_smoke(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    SpawnAttempt::Skipped
}

fn spawn_boot_netbench(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    SpawnAttempt::Skipped
}

struct BootAsset {
    label: &'static str,
    url: &'static str,
    path: &'static str,
    max_bytes: usize,
}

pub(crate) const BOOT_LUMEN_WEIGHTS_PATH: &str = "model.safetensors";
pub(crate) const BOOT_LUMEN_TOKENIZER_PATH: &str = "tokenizer.json";

const BOOT_ASSETS: [BootAsset; 4] = [
    BootAsset {
        label: "weights",
        url: crate::allports::local_assets::TINYLLAMA_MODEL_URL,
        path: BOOT_LUMEN_WEIGHTS_PATH,
        max_bytes: 4 * 1024 * 1024 * 1024,
    },
    BootAsset {
        label: "tokenizer",
        url: crate::allports::local_assets::TINYLLAMA_TOKENIZER_URL,
        path: BOOT_LUMEN_TOKENIZER_PATH,
        max_bytes: 64 * 1024 * 1024,
    },
    BootAsset {
        label: "media-demo-yelly",
        url: crate::allports::local_assets::DEMO_YELLY_MP4_URL,
        path: crate::intel::xelp_media_source::MEDIA_DECODE_CACHE_PATH,
        max_bytes: 256 * 1024 * 1024,
    },
    BootAsset {
        label: "audio-demo-wav",
        url: crate::allports::local_assets::AUDIO_DEMO_URL,
        path: crate::allports::local_assets::AUDIO_DEMO_CACHE_PATH,
        max_bytes: 32 * 1024 * 1024,
    },
];
const BOOT_ASSET_MOUNT_RETRY_SECS: u64 = 10;
const BOOT_ASSET_TIMEOUT_MS: u32 = 180_000;

#[embassy_executor::task]
async fn boot_asset_fetch_task() {
    let disk = loop {
        if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
            let info = disk.info();
            crate::log!(
                "spawn-svc: boot-asset-fetch root mounted disk_id={} readonly={} label={}\n",
                info.id.raw(),
                info.is_read_only() as u8,
                info.label.as_deref().unwrap_or("-")
            );
            break disk;
        }

        crate::log!(
            "spawn-svc: boot-asset-fetch no TRUEOSFS root mounted; retry in {}s\n",
            BOOT_ASSET_MOUNT_RETRY_SECS
        );
        Timer::after(EmbassyDuration::from_secs(BOOT_ASSET_MOUNT_RETRY_SECS)).await;
    };

    let _ = crate::r::readiness::wait_for_timeout(
        crate::r::readiness::NET_ANY_CONFIGURED,
        EmbassyDuration::from_secs(30),
    )
    .await;

    if !crate::t::shared_tokio_runtime_ready() {
        crate::log!("spawn-svc: boot-asset-fetch waiting for shared tokio runtime\n");
        while !crate::t::shared_tokio_runtime_ready() {
            Timer::after(EmbassyDuration::from_millis(100)).await;
        }
        crate::log!("spawn-svc: boot-asset-fetch shared tokio runtime ready\n");
    }

    for asset in BOOT_ASSETS {
        match crate::r::fs::trueosfs::file_info_async(disk, asset.path).await {
            Ok(Some(info)) if info.data_len != 0 => {
                crate::log!(
                    "spawn-svc: boot-asset-fetch skip existing {} path={} bytes={}\n",
                    asset.label,
                    asset.path,
                    info.data_len
                );
                continue;
            }
            Ok(_) => {}
            Err(err) => {
                crate::log!(
                    "spawn-svc: boot-asset-fetch existing-file probe failed {} path={} err={:?}\n",
                    asset.label,
                    asset.path,
                    err
                );
            }
        }

        crate::log!(
            "spawn-svc: boot-asset-fetch start {} url={} path={} max_bytes={}\n",
            asset.label,
            asset.url,
            asset.path,
            asset.max_bytes
        );
        let label = asset.label;
        let url = asset.url;
        let path = asset.path;
        let max_bytes = asset.max_bytes;
        match crate::t::run_on_shared_tokio(move || async move {
            crate::t::net::http_stream::fetch_http_to_file_hyper_async(
                url,
                disk,
                path,
                BOOT_ASSET_TIMEOUT_MS,
                max_bytes,
            )
            .await
        })
        .await
        {
            Ok(Ok(())) => match crate::r::fs::trueosfs::file_info_async(disk, asset.path).await {
                Ok(Some(info)) => {
                    crate::log!(
                        "spawn-svc: boot-asset-fetch success {} path={} bytes={}\n",
                        asset.label,
                        asset.path,
                        info.data_len
                    );
                }
                Ok(None) => {
                    crate::log!(
                        "spawn-svc: boot-asset-fetch post-fetch stat missing {} path={}\n",
                        asset.label,
                        asset.path
                    );
                }
                Err(err) => {
                    crate::log!(
                        "spawn-svc: boot-asset-fetch post-fetch stat failed {} path={} err={:?}\n",
                        asset.label,
                        asset.path,
                        err
                    );
                }
            },
            Ok(Err(err)) => {
                crate::log!(
                    "spawn-svc: boot-asset-fetch failed {} url={} err={:?}\n",
                    asset.label,
                    asset.url,
                    err
                );
            }
            Err(err) => {
                crate::log!(
                    "spawn-svc: boot-asset-fetch shared-tokio failed {} url={} err={:?}\n",
                    label,
                    url,
                    err
                );
            }
        }
    }

    crate::log!(
        "spawn-svc: boot-asset-fetch done weights={} tokenizer={} media={} audio={}\n",
        BOOT_LUMEN_WEIGHTS_PATH,
        BOOT_LUMEN_TOKENIZER_PATH,
        crate::intel::xelp_media_source::MEDIA_DECODE_CACHE_PATH,
        crate::allports::local_assets::AUDIO_DEMO_CACHE_PATH
    );
}

fn spawn_boot_asset_fetch(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| boot_asset_fetch_task())
}

fn spawn_app_vm_run_queue(spawner: Spawner) -> SpawnAttempt {
    match crate::shell2::spawn_app_vm_run_queue(spawner) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

#[derive(Clone, Copy)]
enum BlueprintAutostart {
    Weather,
}

impl BlueprintAutostart {
    const fn label(self) -> &'static str {
        match self {
            Self::Weather => "weather",
        }
    }

    const fn archive(self) -> &'static str {
        match self {
            Self::Weather => "weather.bp",
        }
    }

    const fn slot(self) -> &'static str {
        match self {
            Self::Weather => "wx",
        }
    }

    const fn settle_ms(self) -> u64 {
        match self {
            Self::Weather => 0,
        }
    }
}

const BP_AUTOSTARTS: &[BlueprintAutostart] = &[BlueprintAutostart::Weather];

#[embassy_executor::task]
async fn bp_autostart_task() {
    crate::r::readiness::wait_for(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED).await;

    for config in BP_AUTOSTARTS {
        let settle_ms = config.settle_ms();
        if settle_ms != 0 {
            Timer::after(EmbassyDuration::from_millis(settle_ms)).await;
        }

        let target = crate::shell2::matrix_target_for_slot_name(
            crate::shell2::OUTPUT_UI2_MASK,
            config.slot(),
        );

        crate::log!(
            "spawn-svc: bp-autostart begin label={} archive={} slot={}\n",
            config.label(),
            config.archive(),
            config.slot()
        );

        match crate::shell2::cmds::run::submit_archive_name_to_target_prefer_trueosfs_async(
            target,
            config.archive(),
            Vec::new(),
        )
        .await
        {
            Ok(source) => crate::log!(
                "spawn-svc: bp-autostart queued label={} archive={} slot={} source={}\n",
                config.label(),
                config.archive(),
                config.slot(),
                source
            ),
            Err(err) => crate::log!(
                "spawn-svc: bp-autostart skipped label={} archive={} slot={} err={}\n",
                config.label(),
                config.archive(),
                config.slot(),
                err
            ),
        }
    }
}

fn spawn_bp_autostart(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| bp_autostart_task())
}

fn spawn_uart_shell(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| crate::shell2::task(spawner, &crate::shell2::UART1_COM1_BACKEND))
}

fn spawn_net_tcp_shell(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        crate::shell2::task(spawner, &crate::shell2::NET_TCP_SHELL_BACKEND)
    })
}

fn spawn_pciids_git(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |_worker_spawner| crate::pci::pciids::pciids_git_task())
}

#[embassy_executor::task]
async fn atomic_bomb_task() {
    Timer::after(EmbassyDuration::from_secs(5)).await;

    if let Some(profile) = crate::cpu::CpuProfile::current() {
        crate::log!(
            "PANIC PANIC PANIC: atomic_bomb firing slot={} lapic={} kind={}\n",
            profile.slot(),
            profile.lapic_id(),
            profile.core_kind_name()
        );
    } else {
        crate::log!("PANIC PANIC PANIC: atomic_bomb firing on unknown cpu\n");
    }

    panic!("PANIC PANIC PANIC: delayed atomic_bomb");
}

fn spawn_atomic_bomb(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |_worker_spawner| atomic_bomb_task())
}

// --- registry ---

const NET_ANY_CONFIGURED_AND_ROOT_READY: u32 =
    crate::r::readiness::NET_ANY_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED;
const PCIIDS_GIT_READY: u32 = crate::r::readiness::TOKIO_RUNTIME_READY
    | crate::r::readiness::NET_ANY_CONFIGURED
    | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED;
const HYPER_HTTP1_PROBE_READY: u32 =
    crate::r::readiness::NET_SOCKET_READY | crate::r::readiness::NET_V4_GATEWAY_REACHABLE;
const AI_QJS_ONESHOT_READY: u32 = crate::r::readiness::NET_ANY_CONFIGURED
    | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED
    | crate::r::readiness::QJS_ASYNC_FS_READY;
const UI2_DEMO_READY: u32 =
    crate::r::readiness::UI2_READY | crate::r::readiness::GFX_TEXTURE_UPLOAD_SERVICE_READY;
const BP_AUTOSTART_READY: u32 = crate::r::readiness::APP_VM_READY
    | crate::r::readiness::UI2_READY
    | crate::r::readiness::GFX_TEXTURE_UPLOAD_SERVICE_READY
    | crate::r::readiness::NET_SOCKET_READY
    | crate::r::readiness::TLS_SOCKET_SERVICE_READY;
const WS_BOOT_READY: u32 = crate::r::readiness::NET_GATEWAY_REACHABLE
    | crate::r::readiness::TLS_SOCKET_SERVICE_READY
    | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED;
static TASKS: [TaskSpec; 69] = [
    TaskSpec::enabled("job-runner", 0, &JOB_RUNNER_STARTED, spawn_job_runner),
    TaskSpec::enabled(
        "globalog-persist-once",
        0,
        &GLOBALOG_PERSIST_ONCE_STARTED,
        spawn_globalog_persist_once,
    ),
    TaskSpec::enabled("factory-ram-probe", 0, &FACTORY_RAM_PROBE_STARTED, spawn_factory_ram_probe),
    TaskSpec::enabled(
        "qjs-async-fs-service",
        0,
        &QJS_ASYNC_FS_SERVICE_STARTED,
        spawn_qjs_async_fs_service,
    ),
    TaskSpec::enabled(
        "trueosfs-mount-service",
        0,
        &TRUEOSFS_MOUNT_SERVICE_STARTED,
        spawn_trueosfs_mount_service,
    ),
    TaskSpec::enabled(
        "trueosfs-index-service",
        0,
        &TRUEOSFS_INDEX_SERVICE_STARTED,
        spawn_trueosfs_index_service,
    ),
    TaskSpec::enabled("hv-vm-store", 0, &HV_VM_STORE_STARTED, spawn_hv_vm_store),
    TaskSpec::enabled("hv-vm-store-net", 0, &HV_VM_STORE_NET_STARTED, spawn_hv_vm_store_net),
    TaskSpec::enabled("net-poll-tasks", 0, &NET_POLL_STARTED, spawn_net_poll_tasks),
    TaskSpec::enabled("net-service", 0, &NET_SERVICE_STARTED, spawn_net_service),
    TaskSpec::enabled(
        "tls-socket-service",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &TLS_SOCKET_SERVICE_STARTED,
        spawn_tls_socket_service,
    ),
    TaskSpec::enabled(
        "ntp-sync",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &NTP_SYNC_STARTED,
        spawn_ntp_sync,
    ),
    TaskSpec::enabled(
        "sntp-service",
        crate::r::readiness::NET_V4_CONFIGURED,
        &SNTP_SERVICE_STARTED,
        spawn_sntp_service,
    ),
    TaskSpec::enabled("net-shell", 0, &NET_SHELL_STARTED, spawn_net_shell),
    TaskSpec::enabled(
        "logtotcp",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &LOGTOTCP_STARTED,
        spawn_logtotcp,
    ),
    TaskSpec::disabled(
        "ai-task",
        AI_QJS_ONESHOT_READY,
        &AI_QJS_ONESHOT_STARTED,
        spawn_ai_qjs_oneshot,
    ),
    TaskSpec::disabled("html-demo", 0, &HTML_DEMO_STARTED, spawn_html_demo),
    TaskSpec::enabled(
        "http-trueosfs",
        NET_ANY_CONFIGURED_AND_ROOT_READY,
        &HTTP_TRUEOSFS_STARTED,
        spawn_http_trueosfs,
    ),
    TaskSpec::enabled("pciids-git", PCIIDS_GIT_READY, &PCIIDS_GIT_STARTED, spawn_pciids_git),
    TaskSpec::enabled_gated(
        "hyper-http1-probe",
        HYPER_HTTP1_PROBE_READY,
        hyper_http1_probe_enabled,
        &HYPER_HTTP1_PROBE_STARTED,
        spawn_hyper_http1_probe,
    ),
    TaskSpec::enabled(
        "chat-http",
        crate::r::readiness::NET_V4_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
        &CHAT_HTTP_STARTED,
        spawn_chat_http,
    ),
    TaskSpec::enabled(
        "webmail-http",
        crate::r::readiness::NET_V4_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
        &MAIL_HTTP_STARTED,
        spawn_mail_http,
    ),
    TaskSpec::enabled(
        "axum-boot",
        crate::r::readiness::NET_V4_CONFIGURED,
        &AXUM_BOOT_STARTED,
        spawn_axum_boot,
    ),
    TaskSpec::enabled(
        "fileexplorer-http",
        crate::r::readiness::NET_V4_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
        &FILEEXPLORER_HTTP_STARTED,
        spawn_fileexplorer_http,
    ),
    TaskSpec::enabled("app-vm-run-queue", 0, &APP_VM_RUN_QUEUE_STARTED, spawn_app_vm_run_queue),
    TaskSpec::disabled(
        "bp-autostart",
        BP_AUTOSTART_READY,
        &BP_AUTOSTART_STARTED,
        spawn_bp_autostart,
    ),
    TaskSpec::enabled(
        "ws-time",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &WS_TIME_STARTED,
        spawn_ws_time,
    ),
    TaskSpec::enabled(
        "usb-controller-tasks",
        0,
        &USB_CONTROLLER_TASKS_STARTED,
        spawn_usb_controller_tasks,
    ),
    TaskSpec::enabled("esp-gate", 0, &ESP_GATE_STARTED, spawn_esp_gate),
    TaskSpec::enabled("esp-gate-registry", 0, &ESP_GATE_REGISTRY_STARTED, spawn_esp_gate_registry),
    TaskSpec::enabled(
        "esp-piano-udp",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &ESP_PIANO_UDP_STARTED,
        spawn_esp_piano_udp,
    ),
    TaskSpec::disabled(
        "ftp-server",
        NET_ANY_CONFIGURED_AND_ROOT_READY,
        &FTP_SERVER_STARTED,
        spawn_ftp_server,
    ),
    TaskSpec::enabled(
        "tga",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &TGA_TASK_STARTED,
        spawn_tga_task,
    ),
    TaskSpec::enabled_gated(
        "gfx-virgl-backend-ready",
        0,
        gfx_backend_boot_gate,
        &GFX_VIRGL_READY_TASK_STARTED,
        spawn_gfx_virgl_ready_task,
    ),
    TaskSpec::enabled_on(
        SpawnPlacement::Ap1,
        "gfx-virgl-cursor-overlay",
        crate::r::readiness::GFX_BACKEND_READY,
        &GFX_VIRGL_CURSOR_OVERLAY_STARTED,
        spawn_gfx_virgl_cursor_overlay_task,
    ),
    TaskSpec::enabled_gated(
        "intel-cursor-service",
        0,
        intel_cursor_service_gate,
        &INTEL_CURSOR_SERVICE_STARTED,
        spawn_intel_cursor_service_task,
    ),
    TaskSpec::disabled(
        //  SpawnPlacement::Worker,
        "intel-hda-probe",
        crate::r::readiness::INTEL_HDA_READY,
        &INTEL_HDA_PROBE_STARTED,
        spawn_intel_hda_probe_task,
    ),
    TaskSpec::enabled("raple-service", 0, &RAPLE_SERVICE_STARTED, spawn_raple_service),
    TaskSpec::enabled_on(
        SpawnPlacement::Local,
        "html_fetch_service",
        crate::r::readiness::NET_V4_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
        &HTML_SHACK_SERVICE_STARTED,
        html_fetch_service,
    ),
    TaskSpec::enabled_on(
        SpawnPlacement::Ap1,
        "gfx-texture-upload-service",
        crate::r::readiness::GFX_BACKEND_READY,
        &GFX_TEXTURE_UPLOAD_SERVICE_STARTED,
        spawn_gfx_texture_upload_service,
    ),
    TaskSpec::enabled_gated_on(
        SpawnPlacement::Ap1,
        "ui2",
        crate::r::readiness::GFX_BACKEND_READY,
        ui2_core_task_gate,
        &UI2_STARTED,
        spawn_ui2,
    ),
    TaskSpec::enabled_gated_on(
        SpawnPlacement::Ap1,
        "ui2-hosted",
        crate::r::readiness::GFX_BACKEND_READY,
        ui2_core_task_gate,
        &UI2_HOSTED_SYNC_TASK_STARTED,
        spawn_ui2_hosted,
    ),
    TaskSpec::enabled_gated_on(
        SpawnPlacement::Ap1,
        "ui2-hit",
        crate::r::readiness::GFX_BACKEND_READY,
        ui2_core_task_gate,
        &UI2_HIT_TASK_STARTED,
        spawn_ui2_hit,
    ),
    TaskSpec::enabled("truesurfer-factory", 0, &SURFER_FACTORY_STARTED, spawn_truesurfer_factory),
    TaskSpec::enabled_on(
        SpawnPlacement::Ap1,
        "ui2-athlas-third-demo",
        UI2_DEMO_READY,
        &UI2_ATHLAS_THIRD_DEMO_STARTED,
        spawn_ui2_athlas_third_demo,
    ),
    TaskSpec::enabled_on(
        SpawnPlacement::Ap1,
        "ui2-athlas-half-demo",
        UI2_DEMO_READY,
        &UI2_ATHLAS_HALF_DEMO_STARTED,
        spawn_ui2_athlas_half_demo,
    ),
    TaskSpec::enabled_on(
        SpawnPlacement::Ap1,
        "ui2-athlas-1x-demo",
        UI2_DEMO_READY,
        &UI2_ATHLAS_1X_DEMO_STARTED,
        spawn_ui2_athlas_1x_demo,
    ),
    TaskSpec::enabled_on(
        SpawnPlacement::Ap1,
        "ui2-athlas-2x-demo",
        UI2_DEMO_READY,
        &UI2_ATHLAS_2X_DEMO_STARTED,
        spawn_ui2_athlas_2x_demo,
    ),
    TaskSpec::enabled_on(
        SpawnPlacement::Ap1,
        "ui2-palatino-1x-demo",
        UI2_DEMO_READY,
        &UI2_PALATINO_1X_DEMO_STARTED,
        spawn_ui2_palatino_1x_demo,
    ),
    TaskSpec::enabled_on(
        SpawnPlacement::Ap1,
        "ui2-twemoji-1x",
        crate::r::readiness::GFX_TEXTURE_UPLOAD_SERVICE_READY,
        &UI2_TWEMOJI_1X_STARTED,
        spawn_ui2_twemoji_1x,
    ),
    TaskSpec::enabled_gated(
        "ui2-text-input-demo",
        UI2_DEMO_READY,
        ui2_demo_task_gate,
        &UI2_TEXT_INPUT_DEMO_STARTED,
        spawn_ui2_text_input_demo,
    ),
    TaskSpec::enabled(
        "ui2-analog-clock-demo",
        UI2_DEMO_READY,
        &UI2_ANALOG_CLOCK_DEMO_STARTED,
        spawn_ui2_analog_clock_demo,
    ),
    TaskSpec::enabled("ui2-bgrt-demo", UI2_DEMO_READY, &UI2_BGRT_DEMO_STARTED, spawn_ui2_bgrt_demo),
    TaskSpec::enabled(
        "ui2-coreticks-demo",
        UI2_DEMO_READY,
        &UI2_CORETICKS_DEMO_STARTED,
        spawn_ui2_coreticks_demo,
    ),
    TaskSpec::enabled(
        "ui2-cursorpicker-demo",
        UI2_DEMO_READY,
        &UI2_CURSORPICKER_DEMO_STARTED,
        spawn_ui2_cursorpicker_demo,
    ),
    TaskSpec::disabled(
        "ui2-mandelbrot-demo",
        UI2_DEMO_READY,
        &UI2_MANDELBROT_DEMO_STARTED,
        spawn_ui2_mandelbrot_demo,
    ),
    // Keep the player demo opt-in because it opens the audio player on boot.
    // The old hosted-surface window-spam issue was fixed by content-id keyed reuse.
    TaskSpec::enabled(
        "ui2-player-demo",
        UI2_DEMO_READY,
        &UI2_PLAYER_DEMO_STARTED,
        spawn_ui2_player_demo,
    ),
    TaskSpec::enabled(
        "ui2-raple-demo",
        UI2_DEMO_READY,
        &UI2_RAPLE_DEMO_STARTED,
        spawn_ui2_raple_demo,
    ),
    TaskSpec::disabled(
        "ui2-smiley-fountain-demo",
        UI2_DEMO_READY,
        &UI2_SMILEY_FOUNTAIN_DEMO_STARTED,
        spawn_ui2_smiley_fountain_demo,
    ),
    TaskSpec::enabled(
        "ui2-shell-demo",
        UI2_DEMO_READY,
        &UI2_SHELL_DEMO_STARTED,
        spawn_ui2_shell_demo,
    ),
    TaskSpec::enabled(
        "ui2-swarm-demo",
        UI2_DEMO_READY | crate::r::readiness::NET_ANY_CONFIGURED,
        &UI2_SWARM_DEMO_STARTED,
        spawn_ui2_swarm_demo,
    ),
    TaskSpec::enabled(
        "trueosfs-ready-hook",
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
        &TRUEOSFS_READY_HOOK_STARTED,
        spawn_trueosfs_ready_hook,
    ),
    TaskSpec::enabled_gated(
        "lumen-service",
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
        boot_lumen_service_enabled,
        &LUMEN_SERVICE_STARTED,
        spawn_lumen_service,
    ),
    TaskSpec::disabled("boot-ws-smoke", WS_BOOT_READY, &BOOT_WS_SMOKE_STARTED, spawn_boot_ws_smoke),
    TaskSpec::disabled("boot-netbench", 0, &BOOT_NETBENCH_STARTED, spawn_boot_netbench),
    TaskSpec::enabled_gated(
        "boot-asset-fetch",
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
        boot_asset_fetch_enabled,
        &BOOT_ASSET_FETCH_STARTED,
        spawn_boot_asset_fetch,
    ),
    TaskSpec::enabled("uart-shell", 0, &UART_SHELL_STARTED, spawn_uart_shell),
    TaskSpec::enabled("net-tcp-shell", 0, &NET_TCP_SHELL_STARTED, spawn_net_tcp_shell),
    TaskSpec::disabled("atomic_bomb", 0, &ATOMIC_BOMB_STARTED, spawn_atomic_bomb),
];

// ---------------------------------------------------------------------------
// Public API: offline task list for UI2 dock.
// ---------------------------------------------------------------------------

/// An offline task entry visible to the UI2 layer.
pub struct OfflineTaskEntry {
    pub index: usize,
}

/// Return all UI2 tasks that are currently disabled.
/// This includes both tasks that have not started yet and task-backed windows
/// that were offlined while already running.
pub fn offline_ui2_demo_tasks() -> Vec<OfflineTaskEntry> {
    let mut out = Vec::new();
    for (i, spec) in TASKS.iter().enumerate() {
        if !spec.name.starts_with("ui2-") {
            continue;
        }
        if spec.disabled.load(Ordering::Acquire) {
            out.push(OfflineTaskEntry { index: i });
        }
    }
    out
}

/// Enable a disabled task by its TASKS index, making it eligible for spawn.
pub fn enable_task_by_index(index: usize) {
    if let Some(spec) = TASKS.get(index) {
        spec.disabled.store(false, Ordering::Release);
    }
}

pub fn disable_task_by_index(index: usize) {
    if let Some(spec) = TASKS.get(index) {
        spec.disabled.store(true, Ordering::Release);
    }
}

pub fn request_task_stop_by_index(index: usize) -> bool {
    let Some(spec) = TASKS.get(index) else {
        return false;
    };
    let Some(flag) = stop_flag_by_task_name(spec.name) else {
        return false;
    };
    flag.store(true, Ordering::Release);
    true
}

/// Return the name of a task by its TASKS index.
pub fn task_name_by_index(index: usize) -> Option<&'static str> {
    TASKS.get(index).map(|spec| spec.name)
}

pub fn task_index_by_name(name: &str) -> Option<usize> {
    TASKS.iter().position(|spec| spec.name == name)
}

pub fn task_started_by_index(index: usize) -> bool {
    TASKS
        .get(index)
        .map(|spec| spec.started.load(Ordering::Acquire))
        .unwrap_or(false)
}

#[embassy_executor::task]
pub async fn spawn_service_task(spawner: Spawner) {
    async move {
        loop {
            let ready = crate::r::readiness::mask();
            let mut pending = 0usize;
            let mut started_any = false;

            for spec in TASKS.iter() {
                if spec.disabled.load(Ordering::Acquire) {
                    continue;
                }
                if !(spec.gate)() {
                    continue;
                }
                if (ready & spec.required) != spec.required {
                    pending += 1;
                    continue;
                }

                if spec
                    .started
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
                {
                    continue;
                }

                match (spec.spawn)(spawner) {
                    SpawnAttempt::Spawned => {
                        started_any = true;
                        crate::log!(
                            "spawn-svc: started {} (mask=0x{:08X})\n",
                            spec.name,
                            spec.required
                        );
                        if matches!(
                            spec.name,
                            "gfx_loadscreen"
                                | "ui2"
                                | "ui2-gfx-browser"
                                | "ui2-mandelbrot-demo"
                                | "ui2-shell-demo"
                        ) {
                            crate::log!("boot-probe: spawn {} ms={}\n", spec.name, boot_probe_ms());
                        }
                    }
                    SpawnAttempt::Skipped => {
                        spec.started.store(false, Ordering::Release);
                        pending += 1;
                    }
                    SpawnAttempt::Failed(e) => {
                        spec.started.store(false, Ordering::Release);
                        pending += 1;
                        crate::log!(
                            "spawn-svc: failed to start {} (mask=0x{:08X}) err={:?}\n",
                            spec.name,
                            spec.required,
                            e
                        );
                    }
                }
            }
            let sleep_ms = if started_any {
                SPAWN_SERVICE_AFTER_START_MS
            } else if pending == 0 {
                SPAWN_SERVICE_IDLE_MS
            } else {
                SPAWN_SERVICE_PENDING_MS
            };
            Timer::after(EmbassyDuration::from_millis(sleep_ms)).await;
        }
    }
    .await;
}
