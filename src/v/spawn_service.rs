use alloc::string::String;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::{SendSpawner, SpawnError, Spawner};
use embassy_time::{Duration as EmbassyDuration, Timer};
// NOTE: This file is intended to become the single source of truth for Embassy task startup.

/// Central task orchestrator ("FSM spawn service").
///
/// Ideal-world model:
/// - One file owns the boot task registry (what runs + under which readiness conditions).
/// - Individual tasks can still contain internal gating today; later we can delete those
///   once this registry is trusted.
/// - Readiness is monotonic, so this service only ever adds tasks; it never stops them.
///
/// This is intentionally simple: a small polling loop over a static registry.

struct TaskSpec {
    name: &'static str,
    disabled: bool,
    required: u32,
    started: &'static AtomicBool,
    spawn: fn(Spawner) -> SpawnAttempt,
}

impl TaskSpec {
    const fn enabled(
        name: &'static str,
        required: u32,
        started: &'static AtomicBool,
        spawn: fn(Spawner) -> SpawnAttempt,
    ) -> Self {
        Self {
            name,
            disabled: false,
            required,
            started,
            spawn,
        }
    }

    const fn disabled(
        name: &'static str,
        required: u32,
        started: &'static AtomicBool,
        spawn: fn(Spawner) -> SpawnAttempt,
    ) -> Self {
        Self {
            name,
            disabled: true,
            required,
            started,
            spawn,
        }
    }
}

enum SpawnAttempt {
    Spawned,
    Skipped,
    Failed(SpawnError),
}

macro_rules! define_started_flags {
    ($($name:ident),+ $(,)?) => {
        $(static $name: AtomicBool = AtomicBool::new(false);)+
    };
}

define_started_flags!(
    JOB_RUNNER_STARTED,
    VGA_FONT_CACHE_STARTED,
    GLOBALOG_PERSIST_ONCE_STARTED,
    QJS_ASYNC_FS_SERVICE_STARTED,
    TRUEOSFS_MOUNT_SERVICE_STARTED,
    HV_VM_STORE_STARTED,
    HV_VM_STORE_NET_STARTED,
    NET_POLL_STARTED,
    NET_SERVICE_STARTED,
    TLS_SOCKET_SERVICE_STARTED,
    NTP_SYNC_STARTED,
    NET_SHELL_STARTED,
    AI_QJS_ONESHOT_STARTED,
    HTTP_TRUEOSFS_STARTED,
    WS_TIME_STARTED,
    FTP_SERVER_STARTED,
    TGA_TASK_STARTED,
    GFX_VIRGL_READY_TASK_STARTED,
    GFX_VIRGL_CURSOR_OVERLAY_STARTED,
    GFX_TEXTURE_UPLOAD_SERVICE_STARTED,
    GFX_LOADSCREEN_STARTED,
    BROWSER_NET_STARTED,
    BROWSER_PRIMARY_STARTUP_HTML_LOADER_STARTED,
    BROWSER_SECONDARY_STARTUP_ROUTE_STARTED,
    WEBGPU_BROWSER_PRIMARY_STARTED,
    WEBGPU_BROWSER_SECONDARY_STARTED,
    UI2_STARTED,
    UI2_GFX_TETRIS_STARTED,
    UI2_TRIANGLE_DEMO_STARTED,
    UI2_MANDELBROT_DEMO_STARTED,
    GFX_INTEL_TRIANGLE_DEMO_STARTED,
    CRABUSB_BSP_SERVICE_STARTED,
    CRABUSB_EVENT_PUMP_STARTED,
    CRABUSB_AUDIO_STARTED,
    CRABUSB_TRUEKEY_STARTED,
    USB_CONTROLLER_TASKS_STARTED,
    UAC_EVENT_DRAIN_STARTED,
    UAC_SONG_STARTED,
    VLEDS_MUX_STARTED,
    VLEDS_CYCLE_STARTED,
    TRUEKEY_DRAIN_STARTED,
    PIANO_DRAIN_STARTED,
    BOOT_WS_SMOKE_STARTED,
    BOOT_NETBENCH_STARTED,
    UART_SHELL_STARTED,
    NET_TCP_SHELL_STARTED,
    ATOMIC_BOMB_STARTED,
);

const ENABLE_BROWSER_2: bool = false;
const PRIMARY_BROWSER_INSTANCE_ID: u32 = trueos_qjs::browser_task::PRIMARY_BROWSER_INSTANCE_ID;
const SECONDARY_BROWSER_INSTANCE_ID: u32 = PRIMARY_BROWSER_INSTANCE_ID + 1;

#[inline]
fn boot_probe_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

#[inline]
pub const fn secondary_browser_enabled() -> bool {
    ENABLE_BROWSER_2
}

#[inline]
fn spawn_local(
    spawner: Spawner,
    task: impl FnOnce(Spawner) -> Result<(), SpawnError>,
) -> SpawnAttempt {
    match task(spawner) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

#[inline]
fn spawn_on_ap1(
    spawner: Spawner,
    task: impl FnOnce(SendSpawner) -> Result<(), SpawnError>,
) -> SpawnAttempt {
    let _ = spawner; // keep signature stable; this task intentionally targets AP1.
    let Some(profile) = crate::cpu::CpuProfile::for_slot(1) else {
        return SpawnAttempt::Skipped;
    };
    let Some(ap1_spawner) = trueos_qjs::workers::spawner_for_slot(profile.slot()) else {
        return SpawnAttempt::Skipped;
    };
    match task(ap1_spawner) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

#[inline]
fn spawn_on_worker(
    spawner: Spawner,
    task: impl FnOnce(SendSpawner) -> Result<(), SpawnError>,
) -> SpawnAttempt {
    let Some(worker_spawner) = trueos_qjs::workers::pick_background_spawner() else {
        let _ = spawner;
        return SpawnAttempt::Skipped;
    };
    let _ = spawner;
    match task(worker_spawner) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_vga_font_cache(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::vga::init_font_cache_task())
    })
}

fn spawn_job_runner(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::wait::job_runner_task())
    })
}

fn spawn_globalog_persist_once(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::globalog::persist_once_task())
    })
}

fn spawn_qjs_async_fs_service(spawner: Spawner) -> SpawnAttempt {
    if !trueos_qjs::async_fs::claim_service_start() {
        crate::v::readiness::set(crate::v::readiness::QJS_ASYNC_FS_READY);
        return SpawnAttempt::Spawned;
    }

    match spawner.spawn(trueos_qjs::async_fs::async_fs_service_task()) {
        Ok(()) => {
            crate::v::readiness::set(crate::v::readiness::QJS_ASYNC_FS_READY);
            SpawnAttempt::Spawned
        }
        Err(e) => {
            trueos_qjs::async_fs::clear_service_start_claim();
            SpawnAttempt::Failed(e)
        }
    }
}

fn spawn_trueosfs_mount_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::v::fs::trueosfs::mount_service_task())
    })
}

fn spawn_hv_vm_store(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |ap1_spawner| {
        ap1_spawner.spawn(crate::hv::store::vm_store_task())
    })
}

fn spawn_hv_vm_store_net(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |ap1_spawner| {
        ap1_spawner.spawn(crate::hv::store::vm_store_replication_task())
    })
}

fn spawn_net_poll_tasks(spawner: Spawner) -> SpawnAttempt {
    // Some drivers may fail to report a MAC early; treat any detected NIC as usable.
    let count = crate::net::device_count();
    if count == 0 {
        return SpawnAttempt::Skipped;
    }
    for idx in 0..count {
        if let Err(e) = spawner.spawn(crate::net::adapter::net_poll_task(idx)) {
            crate::log!("net: spawn net_poll_task({}) failed: {:?}\n", idx, e);
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
        match spawner.spawn(crate::net::adapter::net_service_task(idx)) {
            Ok(()) => {
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
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::net::tls_socket::tls_socket_service_task())
    })
}

fn spawn_ntp_sync(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::v::net::ntp::ntp_sync_task())
    })
}

fn spawn_net_shell(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::tst_net_tcp_shell::net_shell_task())
    })
}

fn spawn_ai_qjs_oneshot(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(trueos_qjs::ai_task::run_once())
    })
}

fn spawn_http_trueosfs(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::tst_http_trueosfs::http_trueosfs_task())
    })
}

fn spawn_ws_time(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::tst_ws_time::ws_time_task())
    })
}

fn spawn_ftp_server(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::v::net::ftp::ftp_server_task())
    })
}

fn spawn_tga_task(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(crate::tga::tga_task()))
}

#[embassy_executor::task]
async fn gfx_virgl_ready_task() {
    crate::gfx::init(crate::limine::framebuffer_response());

    if crate::v::readiness::is_set(crate::v::readiness::GFX_BACKEND_READY) {
        return;
    }

    #[cfg(not(feature = "gfx_virgl"))]
    {
        return;
    }

    #[cfg(feature = "gfx_virgl")]
    {
        for _ in 0..400 {
            if crate::v::readiness::is_set(crate::v::readiness::GFX_BACKEND_READY) {
                return;
            }
            if crate::v::readiness::is_set(crate::v::readiness::GFX_VIRGL_READY) {
                crate::v::readiness::set(crate::v::readiness::GFX_BACKEND_READY);
                crate::log!("boot-probe: gfx-backend-ready ms={}\n", boot_probe_ms());
                return;
            }
            if gfx_switched() {
                crate::v::readiness::set(crate::v::readiness::GFX_BACKEND_READY);
                crate::log!(
                    "boot-probe: gfx-backend-ready(switched) ms={}\n",
                    boot_probe_ms()
                );
                return;
            }
            Timer::after(EmbassyDuration::from_millis(25)).await;
        }
        crate::log!(
            "gfx-backend-ready: timeout virgl_active={} virgl_present_cached={} ready_mask=0x{:08X}\n",
            crate::gfx::is_virgl_active() as u8,
            crate::gfx::is_virgl_present_cached() as u8,
            crate::v::readiness::mask()
        );
    }
}

fn spawn_gfx_virgl_ready_task(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(gfx_virgl_ready_task()))
}

#[embassy_executor::task]
async fn gfx_virgl_cursor_overlay_task() {
    crate::log!(
        "boot-probe: gfx-cursor-overlay task start ms={}\n",
        boot_probe_ms()
    );
    #[cfg(not(feature = "gfx_virgl"))]
    {
        return;
    }

    #[cfg(feature = "gfx_virgl")]
    {
        loop {
            if !crate::v::readiness::is_set(crate::v::readiness::GFX_BACKEND_READY) {
                Timer::after(EmbassyDuration::from_millis(16)).await;
                continue;
            }

            let _ = crate::gfx::cursor_overlay_tick();

            Timer::after(EmbassyDuration::from_millis(16)).await;
        }
    }
}

fn spawn_gfx_virgl_cursor_overlay_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |ap1_spawner| {
        ap1_spawner.spawn(gfx_virgl_cursor_overlay_task())
    })
}

fn spawn_gfx_texture_upload_service(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |ap1_spawner| {
        ap1_spawner.spawn(crate::v::io::cabi::texture_upload_service_task())
    })
}

#[inline]
fn gfx_switched() -> bool {
    #[cfg(feature = "gfx_virgl")]
    {
        if crate::gfx::is_virgl_active() {
            return true;
        }
        if crate::gfx::is_virgl_present_cached() {
            return crate::gfx::switch_to_virgl();
        }
        false
    }

    #[cfg(not(feature = "gfx_virgl"))]
    {
        false
    }
}

fn spawn_gfx_loadscreen(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::gfx::loadscreen::gfx_loadscreen_task())
    })
}

#[embassy_executor::task]
async fn browser_startup_html_loader_task(browser_instance_id: u32) {
    const STARTUP_URL: &str = "https://www.google.de";
    let op_id = crate::v::browser_net::submit_navigation(browser_instance_id, STARTUP_URL);
    if op_id == 0 {
        crate::log!(
            "browser-html-loader: failed to queue startup html browser_instance={}\n",
            browser_instance_id
        );
        return;
    }
    crate::log!(
        "browser-html-loader: queued startup html browser_instance={} op={}\n",
        browser_instance_id,
        op_id
    );
}

fn spawn_browser_net(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::v::browser_net::browser_net_task())
    })
}

#[embassy_executor::task]
async fn browser_svg_startup_route_task(browser_instance_id: u32) {
    const HANDOFF_RETRY_MS: u64 = 100;

    loop {
        let window_id =
            trueos_qjs::browser_task::browser_window_id_for_instance(browser_instance_id);
        if window_id != 0 {
            let rpc_id = trueos_qjs::browser_task::queue_browser_rpc_for_browser(
                browser_instance_id,
                String::from("navigate"),
                String::from("[{\"url\":\"trueos://ui/svg-demo\"}]"),
                window_id,
            );
            if rpc_id != 0 {
                crate::log!(
                    "browser-html-loader: queued svg demo route for browser_instance={} window={}\n",
                    browser_instance_id,
                    window_id
                );
                Timer::after(EmbassyDuration::from_secs(60)).await;
                continue;
            }
        }

        Timer::after(EmbassyDuration::from_millis(HANDOFF_RETRY_MS)).await;
    }
}

fn spawn_primary_browser_startup_html_loader(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(browser_startup_html_loader_task(
            PRIMARY_BROWSER_INSTANCE_ID,
        ))
    })
}

fn spawn_secondary_browser_startup_route(spawner: Spawner) -> SpawnAttempt {
    if !secondary_browser_enabled() {
        return SpawnAttempt::Skipped;
    }
    spawn_local(spawner, |spawner| {
        spawner.spawn(browser_svg_startup_route_task(
            SECONDARY_BROWSER_INSTANCE_ID,
        ))
    })
}

fn spawn_primary_webgpu_browser(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(trueos_qjs::browser_task::boot_browser(
            PRIMARY_BROWSER_INSTANCE_ID,
        ))
    })
}

fn spawn_secondary_webgpu_browser(spawner: Spawner) -> SpawnAttempt {
    if !secondary_browser_enabled() {
        return SpawnAttempt::Skipped;
    }
    spawn_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(trueos_qjs::browser_task::boot_browser(
            SECONDARY_BROWSER_INSTANCE_ID,
        ))
    })
}

fn spawn_ui2(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |ap1_spawner| {
        ap1_spawner.spawn(crate::v::ui2::ui2_task())
    })
}

fn spawn_ui2_gfx_tetris(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::tst_gfx_tetris::ui2_gfx_tetris_task())
    })
}

fn spawn_ui2_triangle_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::tst_ui2_triangle_demo::ui2_triangle_demo_task())
    })
}

fn spawn_ui2_mandelbrot_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::tst_ui2_mandelbrot_demo::ui2_mandelbrot_demo_task())
    })
}

fn spawn_gfx_intel_triangle_demo(spawner: Spawner) -> SpawnAttempt {
    #[cfg(not(feature = "gfx_intel"))]
    {
        let _ = spawner;
        return SpawnAttempt::Skipped;
    }

    #[cfg(feature = "gfx_intel")]
    {
        spawn_local(spawner, |spawner| {
            spawner.spawn(crate::gfx::intel::scanout_smoke_task())
        })
    }
}

fn spawn_crabusb_audio(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::usb2::crabusb_audio_task())
    })
}

fn spawn_crabusb_bsp_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::usb2::crabusb_bsp_service(spawner))
    })
}

fn spawn_crabusb_event_pump(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::usb2::crabusb_event_pump_task())
    })
}

fn spawn_crabusb_truekey(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::usb2::crabusb_truekey_task())
    })
}

fn spawn_boot_ws_smoke(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    SpawnAttempt::Skipped
}

fn spawn_boot_netbench(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    SpawnAttempt::Skipped
}

fn spawn_uart_shell(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::shell2::task(
            spawner,
            &crate::shell2::UART1_COM1_BACKEND,
        ))
    })
}

fn spawn_net_tcp_shell(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::shell2::task(
            spawner,
            &crate::shell2::NET_TCP_SHELL_BACKEND,
        ))
    })
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
    spawn_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(atomic_bomb_task())
    })
}

// --- registry ---

const NET_CONFIGURED_AND_ROOT_READY: u32 =
    crate::v::readiness::NET_CONFIGURED | crate::v::readiness::TRUEOSFS_ROOT_MOUNTED;
const AI_QJS_ONESHOT_READY: u32 = crate::v::readiness::NET_CONFIGURED
    | crate::v::readiness::TRUEOSFS_ROOT_MOUNTED
    | crate::v::readiness::QJS_ASYNC_FS_READY;
const WS_BOOT_READY: u32 = crate::v::readiness::NET_GATEWAY_REACHABLE
    | crate::v::readiness::TLS_SOCKET_SERVICE_READY
    | crate::v::readiness::TRUEOSFS_ROOT_MOUNTED;
const WEBGPU_BROWSER_READY: u32 = crate::v::readiness::UI2_READY;

static TASKS: &[TaskSpec] = &[
    TaskSpec::enabled("job-runner", 0, &JOB_RUNNER_STARTED, spawn_job_runner),
    TaskSpec::enabled(
        "vga-font-cache",
        0,
        &VGA_FONT_CACHE_STARTED,
        spawn_vga_font_cache,
    ),
    TaskSpec::enabled(
        "globalog-persist-once",
        0,
        &GLOBALOG_PERSIST_ONCE_STARTED,
        spawn_globalog_persist_once,
    ),
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
    TaskSpec::enabled("hv-vm-store", 0, &HV_VM_STORE_STARTED, spawn_hv_vm_store),
    TaskSpec::enabled(
        "hv-vm-store-net",
        0,
        &HV_VM_STORE_NET_STARTED,
        spawn_hv_vm_store_net,
    ),
    TaskSpec::enabled("net-poll-tasks", 0, &NET_POLL_STARTED, spawn_net_poll_tasks),
    TaskSpec::enabled("net-service", 0, &NET_SERVICE_STARTED, spawn_net_service),
    TaskSpec::enabled(
        "tls-socket-service",
        crate::v::readiness::NET_CONFIGURED,
        &TLS_SOCKET_SERVICE_STARTED,
        spawn_tls_socket_service,
    ),
    TaskSpec::enabled(
        "ntp-sync",
        crate::v::readiness::NET_CONFIGURED,
        &NTP_SYNC_STARTED,
        spawn_ntp_sync,
    ),
    TaskSpec::enabled("net-shell", 0, &NET_SHELL_STARTED, spawn_net_shell),
    TaskSpec::disabled(
        "ai-task",
        AI_QJS_ONESHOT_READY,
        &AI_QJS_ONESHOT_STARTED,
        spawn_ai_qjs_oneshot,
    ),
    TaskSpec::enabled(
        "http-trueosfs",
        NET_CONFIGURED_AND_ROOT_READY,
        &HTTP_TRUEOSFS_STARTED,
        spawn_http_trueosfs,
    ),
    TaskSpec::enabled(
        "ws-time",
        crate::v::readiness::NET_CONFIGURED,
        &WS_TIME_STARTED,
        spawn_ws_time,
    ),
    TaskSpec::disabled(
        "ftp-server",
        NET_CONFIGURED_AND_ROOT_READY,
        &FTP_SERVER_STARTED,
        spawn_ftp_server,
    ),
    TaskSpec::disabled("tga", 0, &TGA_TASK_STARTED, spawn_tga_task),
    TaskSpec::enabled(
        "gfx-backend-ready",
        0,
        &GFX_VIRGL_READY_TASK_STARTED,
        spawn_gfx_virgl_ready_task,
    ),
    TaskSpec::enabled(
        "gfx-virgl-cursor-overlay",
        crate::v::readiness::LOADSCREEN_END,
        &GFX_VIRGL_CURSOR_OVERLAY_STARTED,
        spawn_gfx_virgl_cursor_overlay_task,
    ),
    TaskSpec::enabled(
        "gfx_loadscreen",
        crate::v::readiness::GFX_BACKEND_READY,
        &GFX_LOADSCREEN_STARTED,
        spawn_gfx_loadscreen,
    ),
    TaskSpec::enabled("browser-net", 0, &BROWSER_NET_STARTED, spawn_browser_net),
    TaskSpec::enabled(
        "gfx-texture-upload-service",
        crate::v::readiness::GFX_BACKEND_READY,
        &GFX_TEXTURE_UPLOAD_SERVICE_STARTED,
        spawn_gfx_texture_upload_service,
    ),
    TaskSpec::enabled(
        "browser-startup-html-loader-primary",
        crate::v::readiness::NET_CONFIGURED,
        &BROWSER_PRIMARY_STARTUP_HTML_LOADER_STARTED,
        spawn_primary_browser_startup_html_loader,
    ),
    if ENABLE_BROWSER_2 {
        TaskSpec::disabled(
            "browser-startup-route-secondary",
            crate::v::readiness::NET_CONFIGURED,
            &BROWSER_SECONDARY_STARTUP_ROUTE_STARTED,
            spawn_secondary_browser_startup_route,
        )
    } else {
        TaskSpec::disabled(
            "browser-startup-route-secondary",
            crate::v::readiness::NET_CONFIGURED,
            &BROWSER_SECONDARY_STARTUP_ROUTE_STARTED,
            spawn_secondary_browser_startup_route,
        )
    },
    TaskSpec::enabled(
        "ui2",
        crate::v::readiness::GFX_BACKEND_READY,
        &UI2_STARTED,
        spawn_ui2,
    ),
    TaskSpec::enabled(
        "ui2-gfx-tetris",
        crate::v::readiness::UI2_READY,
        &UI2_GFX_TETRIS_STARTED,
        spawn_ui2_gfx_tetris,
    ),
    TaskSpec::enabled(
        "ui2-triangle-demo",
        crate::v::readiness::UI2_READY,
        &UI2_TRIANGLE_DEMO_STARTED,
        spawn_ui2_triangle_demo,
    ),
    TaskSpec::enabled(
        "ui2-mandelbrot-demo",
        crate::v::readiness::UI2_READY,
        &UI2_MANDELBROT_DEMO_STARTED,
        spawn_ui2_mandelbrot_demo,
    ),
    TaskSpec::disabled(
        "webgpu-browser-primary",
        WEBGPU_BROWSER_READY,
        &WEBGPU_BROWSER_PRIMARY_STARTED,
        spawn_primary_webgpu_browser,
    ),
    if ENABLE_BROWSER_2 {
        TaskSpec::disabled(
            "webgpu-browser-secondary",
            WEBGPU_BROWSER_READY,
            &WEBGPU_BROWSER_SECONDARY_STARTED,
            spawn_secondary_webgpu_browser,
        )
    } else {
        TaskSpec::disabled(
            "webgpu-browser-secondary",
            WEBGPU_BROWSER_READY,
            &WEBGPU_BROWSER_SECONDARY_STARTED,
            spawn_secondary_webgpu_browser,
        )
    },
    TaskSpec::enabled(
        "gfx-intel-scanout-demo",
        crate::v::readiness::GFX_INTEL_CLAIMED,
        &GFX_INTEL_TRIANGLE_DEMO_STARTED,
        spawn_gfx_intel_triangle_demo,
    ),
    TaskSpec::enabled(
        "crabusb-bsp-service",
        0,
        &CRABUSB_BSP_SERVICE_STARTED,
        spawn_crabusb_bsp_service,
    ),
    TaskSpec::enabled(
        "crabusb-event-pump",
        0,
        &CRABUSB_EVENT_PUMP_STARTED,
        spawn_crabusb_event_pump,
    ),
    TaskSpec::enabled(
        "crabusb-audio",
        0,
        &CRABUSB_AUDIO_STARTED,
        spawn_crabusb_audio,
    ),
    TaskSpec::enabled(
        "crabusb-truekey",
        0,
        &CRABUSB_TRUEKEY_STARTED,
        spawn_crabusb_truekey,
    ),
    TaskSpec::disabled(
        "boot-ws-smoke",
        WS_BOOT_READY,
        &BOOT_WS_SMOKE_STARTED,
        spawn_boot_ws_smoke,
    ),
    TaskSpec::disabled(
        "boot-netbench",
        0,
        &BOOT_NETBENCH_STARTED,
        spawn_boot_netbench,
    ),
    TaskSpec::enabled("uart-shell", 0, &UART_SHELL_STARTED, spawn_uart_shell),
    TaskSpec::enabled(
        "net-tcp-shell",
        0,
        &NET_TCP_SHELL_STARTED,
        spawn_net_tcp_shell,
    ),
    TaskSpec::disabled("atomic_bomb", 0, &ATOMIC_BOMB_STARTED, spawn_atomic_bomb),
];

#[embassy_executor::task]
pub async fn spawn_service_task(spawner: Spawner) {
    async move {
        loop {
            let ready = crate::v::readiness::mask();
            let mut pending = 0usize;
            let mut started_any = false;

            for spec in TASKS {
                if spec.disabled {
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
                                | "ui2-gfx-tetris"
                                | "ui2-triangle-demo"
                                | "ui2-mandelbrot-demo"
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
                10
            } else if pending == 0 {
                250
            } else {
                50
            };
            Timer::after(EmbassyDuration::from_millis(sleep_ms)).await;
        }
    }
    .await;
}
