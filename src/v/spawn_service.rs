use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_executor::{SpawnError, Spawner};
use embassy_time::{Duration as EmbassyDuration, Timer};
use embassy_time_driver::{TICK_HZ, now};

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

enum SpawnAttempt {
    Spawned,
    Skipped,
    Failed(SpawnError),
}

static VGA_FONT_CACHE_STARTED: AtomicBool = AtomicBool::new(false);
static TRUEOSFS_MOUNT_SERVICE_STARTED: AtomicBool = AtomicBool::new(false);
static NET_POLL_STARTED: AtomicBool = AtomicBool::new(false);
static NET_SERVICE_STARTED: AtomicBool = AtomicBool::new(false);
static TLS_SOCKET_SERVICE_STARTED: AtomicBool = AtomicBool::new(false);
static NTP_SYNC_STARTED: AtomicBool = AtomicBool::new(false);
static NET_SHELL_STARTED: AtomicBool = AtomicBool::new(false);
static AI_QJS_ONESHOT_STARTED: AtomicBool = AtomicBool::new(false);
static HTTP_TRUEOSFS_STARTED: AtomicBool = AtomicBool::new(false);
static FTP_SERVER_STARTED: AtomicBool = AtomicBool::new(false);
static TGA_TASK_STARTED: AtomicBool = AtomicBool::new(false);
static GFX_VIRGL_READY_TASK_STARTED: AtomicBool = AtomicBool::new(false);
static GFX_VIRGL_CURSOR_OVERLAY_STARTED: AtomicBool = AtomicBool::new(false);
static GFX_HW_CURSOR_STARTED: AtomicBool = AtomicBool::new(false);
static WGPU_TEXT_STARTED: AtomicBool = AtomicBool::new(false);
static WEBGPU_BROWSER_STARTED: AtomicBool = AtomicBool::new(false);
static GFX_MATMUL_DEMO_STARTED: AtomicBool = AtomicBool::new(false);
static GFX_INTEL_TRIANGLE_DEMO_STARTED: AtomicBool = AtomicBool::new(false);
static USB_CONTROLLER_TASKS_STARTED: AtomicBool = AtomicBool::new(false);
static HID_INPUT_LOGGER_STARTED: AtomicBool = AtomicBool::new(false);
static UAC_EVENT_DRAIN_STARTED: AtomicBool = AtomicBool::new(false);
static UAC_SONG_STARTED: AtomicBool = AtomicBool::new(false);
static VLEDS_MUX_STARTED: AtomicBool = AtomicBool::new(false);
static VLEDS_CYCLE_STARTED: AtomicBool = AtomicBool::new(false);
static TRUEKEY_DRAIN_STARTED: AtomicBool = AtomicBool::new(false);
static PIANO_DRAIN_STARTED: AtomicBool = AtomicBool::new(false);
static BOOT_WS_SMOKE_STARTED: AtomicBool = AtomicBool::new(false);
static BOOT_NETBENCH_STARTED: AtomicBool = AtomicBool::new(false);
static VIDEO_SMOKE_STARTED: AtomicBool = AtomicBool::new(false);

static UART_SHELL_STARTED: AtomicBool = AtomicBool::new(false);
static NET_TCP_SHELL_STARTED: AtomicBool = AtomicBool::new(false);
static QJS_SHELL_STARTED: AtomicBool = AtomicBool::new(false);
static GFX_BACKEND_READY_DELAY_DEADLINE_TICKS: AtomicU64 = AtomicU64::new(0);

fn spawn_vga_font_cache(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::vga::init_font_cache_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_trueosfs_mount_service(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::v::fs::trueosfs::mount_service_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
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
    if crate::net::device_count() == 0 {
        return SpawnAttempt::Skipped;
    }
    match spawner.spawn(crate::net::adapter::net_service_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_tls_socket_service(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::net::tls_socket::tls_socket_service_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_ntp_sync(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::v::net::ntp::ntp_sync_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_net_shell(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::shell::backends::net_tcp::net_shell_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_ai_tcp_bridge(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::shell::backends::ai_tcp::ai_tcp_bridge_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_ai_qjs_repl(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::shell::backends::ai_tcp::ai_qjs_repl_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_ai_qjs_oneshot(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(trueos_qjs::ai_task::run_once()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_http_trueosfs(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    SpawnAttempt::Skipped
}

fn spawn_ftp_server(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::v::net::ftp::ftp_server_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_tga_task(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::tga::tga_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
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
                return;
            }
            if gfx_switched() {
                crate::v::readiness::set(crate::v::readiness::GFX_BACKEND_READY);
                return;
            }
            Timer::after(EmbassyDuration::from_millis(25)).await;
        }
        crate::log!("gfx-backend-ready: timeout\n");
    }
}

fn spawn_gfx_virgl_ready_task(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(gfx_virgl_ready_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

#[embassy_executor::task]
async fn gfx_virgl_cursor_overlay_task() {
    #[cfg(not(feature = "gfx_virgl"))]
    {
        return;
    }

    #[cfg(feature = "gfx_virgl")]
    {
        loop {
            if !crate::gfx::is_virgl_active() {
                Timer::after(EmbassyDuration::from_millis(16)).await;
                continue;
            }

            let _ = crate::gfx::cursor_overlay_tick();

            Timer::after(EmbassyDuration::from_millis(16)).await;
        }
    }
}

fn spawn_gfx_virgl_cursor_overlay_task(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(gfx_virgl_cursor_overlay_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_gfx_hw_cursor_task(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::gfx::cursor::gfx_hw_cursor_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

#[inline]
fn task_start_delay(spec: &TaskSpec) -> Option<(u64, &'static AtomicU64)> {
    match spec.name {
        "gfx-backend-ready" => Some((2500, &GFX_BACKEND_READY_DELAY_DEADLINE_TICKS)),
        _ => None,
    }
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

fn spawn_wgpu_text(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::gfx::loadscreen::gfx_loadscreen_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_webgpu_browser(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(trueos_qjs::browser_task::boot_browser()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn fill_demo_matrix(out: &mut [f32], n: usize, seed: f32) {
    let mut i = 0usize;
    while i < n {
        let mut j = 0usize;
        while j < n {
            let idx = i * n + j;
            out[idx] = libm::sinf((i as f32) * 0.013 + (j as f32) * 0.021 + seed);
            j += 1;
        }
        i += 1;
    }
}

fn matmul_square(a: &[f32], b: &[f32], c: &mut [f32], n: usize) {
    let mut i = 0usize;
    while i < n {
        let mut j = 0usize;
        while j < n {
            let mut acc = 0.0f32;
            let mut k = 0usize;
            while k < n {
                acc += a[i * n + k] * b[k * n + j];
                k += 1;
            }
            c[i * n + j] = acc;
            j += 1;
        }
        i += 1;
    }
}

#[embassy_executor::task]
async fn gfx_matmul_demo_task() {
    const N: usize = 64;
    const M: usize = 32;

    crate::log!("gfx-matmul: begin\n");

    let mut a = alloc::vec![0.0f32; N * N];
    let mut b = alloc::vec![0.0f32; N * N];
    let mut c = alloc::vec![0.0f32; N * N];
    fill_demo_matrix(&mut a, N, 0.11);
    fill_demo_matrix(&mut b, N, 0.37);

    // Matrix-Matrix multiplication is embarrassingly parallel:
    // each C[i,j] can be computed independently.
    matmul_square(&a, &b, &mut c, N);

    // Also run a smaller size to show variable workloads.
    let mut a2 = alloc::vec![0.0f32; M * M];
    let mut b2 = alloc::vec![0.0f32; M * M];
    let mut c2 = alloc::vec![0.0f32; M * M];
    fill_demo_matrix(&mut a2, M, 0.07);
    fill_demo_matrix(&mut b2, M, 0.19);
    matmul_square(&a2, &b2, &mut c2, M);

    // Log compact checksums so work is observable and not optimized away.
    let mut s1 = 0.0f32;
    for v in &c {
        s1 += *v;
    }
    let mut s2 = 0.0f32;
    for v in &c2 {
        s2 += *v;
    }
    crate::log!(
        "gfx-matmul: done N={} sum={:.5} M={} sum={:.5}\n",
        N,
        s1,
        M,
        s2
    );
}

fn spawn_gfx_matmul_demo(spawner: Spawner) -> SpawnAttempt {
    #[cfg(not(feature = "gfx_virgl"))]
    {
        let _ = spawner;
        return SpawnAttempt::Skipped;
    }

    #[cfg(feature = "gfx_virgl")]
    {
        crate::gfx::init(crate::limine::framebuffer_response());
        if !crate::gfx::is_virgl_present_cached() {
            return SpawnAttempt::Skipped;
        }
        if !crate::gfx::is_virgl_active() {
            return SpawnAttempt::Skipped;
        }
        match spawner.spawn(gfx_matmul_demo_task()) {
            Ok(()) => SpawnAttempt::Spawned,
            Err(e) => SpawnAttempt::Failed(e),
        }
    }
}

fn spawn_gfx_intel_triangle_demo(spawner: Spawner) -> SpawnAttempt {
    #[cfg(not(feature = "gfx_intel"))]
    {
        let _ = spawner;
        return SpawnAttempt::Skipped;
    }

    #[cfg(feature = "gfx_intel")]
    {
        match spawner.spawn(crate::gfx::intel::centered_triangle_demo_task()) {
            Ok(()) => SpawnAttempt::Spawned,
            Err(e) => SpawnAttempt::Failed(e),
        }
    }
}

fn spawn_usb_controller_tasks(spawner: Spawner) -> SpawnAttempt {
    for info in crate::usb::xhci::xhc_list().iter().copied() {
        // reads from hardware into dma buffs
        let _ = spawner.spawn(crate::usb::xhci::poll_task(info));
        // reads from our dma buffs into usb rings
        let _ = spawner.spawn(crate::usb::poll_task(info));
        // Single long-lived scout per controller. Rescans are triggered via a flag.
        let _ = spawner.spawn(crate::usb::usb_scout_service(info));
    }
    SpawnAttempt::Spawned
}

fn spawn_hid_input_logger(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::usb::hid::input_logger()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_uac_song(spawner: Spawner) -> SpawnAttempt {
    let Some(ap1_spawner) = crate::runtime::first_ap_spawner() else {
        // Wait until AP1 executor is online so this task runs there.
        return SpawnAttempt::Skipped;
    };
    let _ = spawner; // keep signature stable; song intentionally targets AP1.
    match ap1_spawner.spawn(crate::usb::uac::song_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_uac_event_drain(spawner: Spawner) -> SpawnAttempt {
    let Some(ap1_spawner) = crate::runtime::first_ap_spawner() else {
        // Wait until AP1 executor is online so this task runs there.
        return SpawnAttempt::Skipped;
    };
    let _ = spawner; // keep signature stable; drain intentionally targets AP1.
    match ap1_spawner.spawn(crate::usb::uac::event_drain_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_vleds_mux(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::v::leds::task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_vleds_cycle(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::v::leds::color_cycle_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_truekey_drain(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::usb::truekey::drain_loop()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_piano_drain(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::usb::midi::piano_drain_loop()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_boot_ws_smoke(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    SpawnAttempt::Skipped
}

fn spawn_boot_netbench(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    SpawnAttempt::Skipped
}

fn spawn_video_smoke(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::video_smoke::video_smoke_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_uart_shell(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::shell::task(
        spawner,
        &crate::shell::UART1_COM1_BACKEND,
    )) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_net_tcp_shell(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::shell::task(
        spawner,
        &crate::shell::NET_TCP_SHELL_BACKEND,
    )) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_qjs_shell(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::shell::task(
        spawner,
        &crate::shell::backends::qjs::QJS_SHELL_BACKEND,
    )) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

// --- registry ---

const HID_ANY_CLAIMED: u32 = crate::v::readiness::HID_KEYBOARD_CLAIMED;

const NET_AND_ROOT_READY: u32 =
    crate::v::readiness::NET_GATEWAY_REACHABLE | crate::v::readiness::TRUEOSFS_ROOT_MOUNTED;
const AI_QJS_ONESHOT_READY: u32 = crate::v::readiness::NET_CONFIGURED
    | crate::v::readiness::TRUEOSFS_ROOT_MOUNTED
    | crate::v::readiness::QJS_ASYNC_FS_READY;
const WS_BOOT_READY: u32 = crate::v::readiness::NET_GATEWAY_REACHABLE
    | crate::v::readiness::TLS_SOCKET_SERVICE_READY
    | crate::v::readiness::TRUEOSFS_ROOT_MOUNTED;
const VIDEO_SMOKE_READY: u32 = crate::v::readiness::TLS_SOCKET_SERVICE_READY;

static TASKS: &[TaskSpec] = &[
    TaskSpec {
        name: "vga-font-cache",
        disabled: false,
        required: 0,
        started: &VGA_FONT_CACHE_STARTED,
        spawn: spawn_vga_font_cache,
    },
    TaskSpec {
        name: "trueosfs-mount-service",
        disabled: false,
        required: 0,
        started: &TRUEOSFS_MOUNT_SERVICE_STARTED,
        spawn: spawn_trueosfs_mount_service,
    },
    TaskSpec {
        name: "net-poll-tasks",
        disabled: false,
        required: 0,
        started: &NET_POLL_STARTED,
        spawn: spawn_net_poll_tasks,
    },
    TaskSpec {
        name: "net-service",
        disabled: false,
        required: 0,
        started: &NET_SERVICE_STARTED,
        spawn: spawn_net_service,
    },
    TaskSpec {
        name: "tls-socket-service",
        disabled: false,
        required: crate::v::readiness::NET_CONFIGURED,
        started: &TLS_SOCKET_SERVICE_STARTED,
        spawn: spawn_tls_socket_service,
    },
    TaskSpec {
        name: "ntp-sync",
        disabled: false,
        required: crate::v::readiness::NET_CONFIGURED,
        started: &NTP_SYNC_STARTED,
        spawn: spawn_ntp_sync,
    },
    TaskSpec {
        name: "net-shell",
        disabled: false,
        required: 0,
        started: &NET_SHELL_STARTED,
        spawn: spawn_net_shell,
    },
    TaskSpec {
        name: "ai-qjs-oneshot",
        disabled: true,
        required: AI_QJS_ONESHOT_READY,
        started: &AI_QJS_ONESHOT_STARTED,
        spawn: spawn_ai_qjs_oneshot,
    },
    TaskSpec {
        name: "http-trueosfs",
        disabled: false,
        required: NET_AND_ROOT_READY,
        started: &HTTP_TRUEOSFS_STARTED,
        spawn: spawn_http_trueosfs,
    },
    TaskSpec {
        name: "ftp-server",
        disabled: true,
        required: NET_AND_ROOT_READY,
        started: &FTP_SERVER_STARTED,
        spawn: spawn_ftp_server,
    },
    TaskSpec {
        name: "tga",
        disabled: true,
        required: 0,
        started: &TGA_TASK_STARTED,
        spawn: spawn_tga_task,
    },
    TaskSpec {
        name: "gfx-backend-ready",
        disabled: false,
        required: 0,
        started: &GFX_VIRGL_READY_TASK_STARTED,
        spawn: spawn_gfx_virgl_ready_task,
    },
    TaskSpec {
        name: "gfx-virgl-cursor-overlay",
        disabled: false,
        required: crate::v::readiness::WGPU_TEXT_DONE,
        started: &GFX_VIRGL_CURSOR_OVERLAY_STARTED,
        spawn: spawn_gfx_virgl_cursor_overlay_task,
    },
    TaskSpec {
        name: "gfx-hw-cursor",
        disabled: true,
        required: crate::v::readiness::GFX_BACKEND_READY,
        started: &GFX_HW_CURSOR_STARTED,
        spawn: spawn_gfx_hw_cursor_task,
    },
    TaskSpec {
        name: "wgpu_text",
        disabled: false,
        required: crate::v::readiness::GFX_BACKEND_READY,
        started: &WGPU_TEXT_STARTED,
        spawn: spawn_wgpu_text,
    },
    TaskSpec {
        name: "webgpu_browser",
        disabled: false,
        required: crate::v::readiness::WGPU_TEXT_DONE,
        started: &WEBGPU_BROWSER_STARTED,
        spawn: spawn_webgpu_browser,
    },
    TaskSpec {
        name: "gfx-matmul-demo",
        disabled: true,
        required: crate::v::readiness::GFX_VIRGL_READY,
        started: &GFX_MATMUL_DEMO_STARTED,
        spawn: spawn_gfx_matmul_demo,
    },
    TaskSpec {
        name: "gfx-intel-triangle-demo",
        disabled: true,
        required: crate::v::readiness::GFX_INTEL_CLAIMED,
        started: &GFX_INTEL_TRIANGLE_DEMO_STARTED,
        spawn: spawn_gfx_intel_triangle_demo,
    },
    TaskSpec {
        name: "usb-controller-tasks",
        disabled: false,
        required: 0,
        started: &USB_CONTROLLER_TASKS_STARTED,
        spawn: spawn_usb_controller_tasks,
    },
    TaskSpec {
        name: "hid-input-logger",
        disabled: false,
        required: HID_ANY_CLAIMED,
        started: &HID_INPUT_LOGGER_STARTED,
        spawn: spawn_hid_input_logger,
    },
    TaskSpec {
        name: "uac-event-drain",
        disabled: true,
        required: crate::v::readiness::UAC_ATTACHED,
        started: &UAC_EVENT_DRAIN_STARTED,
        spawn: spawn_uac_event_drain,
    },
    TaskSpec {
        name: "uac-song",
        disabled: true,
        required: crate::v::readiness::UAC_ATTACHED,
        started: &UAC_SONG_STARTED,
        spawn: spawn_uac_song,
    },
    TaskSpec {
        name: "vleds-mux",
        disabled: true,
        required: 0,
        started: &VLEDS_MUX_STARTED,
        spawn: spawn_vleds_mux,
    },
    TaskSpec {
        name: "vleds-cycle",
        disabled: true,
        required: 0,
        started: &VLEDS_CYCLE_STARTED,
        spawn: spawn_vleds_cycle,
    },
    TaskSpec {
        name: "truekey-drain",
        disabled: false,
        required: 0,
        started: &TRUEKEY_DRAIN_STARTED,
        spawn: spawn_truekey_drain,
    },
    TaskSpec {
        name: "piano-drain",
        disabled: false,
        required: crate::v::readiness::PIANO_CLAIMED,
        started: &PIANO_DRAIN_STARTED,
        spawn: spawn_piano_drain,
    },
    TaskSpec {
        name: "boot-ws-smoke",
        disabled: true,
        required: WS_BOOT_READY,
        started: &BOOT_WS_SMOKE_STARTED,
        spawn: spawn_boot_ws_smoke,
    },
    TaskSpec {
        name: "boot-netbench",
        disabled: true,
        required: 0,
        started: &BOOT_NETBENCH_STARTED,
        spawn: spawn_boot_netbench,
    },
    TaskSpec {
        name: "video-smoke",
        disabled: true,
        required: VIDEO_SMOKE_READY,
        started: &VIDEO_SMOKE_STARTED,
        spawn: spawn_video_smoke,
    },
    TaskSpec {
        name: "uart-shell",
        disabled: false,
        required: 0,
        started: &UART_SHELL_STARTED,
        spawn: spawn_uart_shell,
    },
    TaskSpec {
        name: "net-tcp-shell",
        disabled: false,
        required: 0,
        started: &NET_TCP_SHELL_STARTED,
        spawn: spawn_net_tcp_shell,
    },
    TaskSpec {
        name: "qjs-shell",
        disabled: false,
        required: 0,
        started: &QJS_SHELL_STARTED,
        spawn: spawn_qjs_shell,
    },
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

                if let Some((delay_ms, deadline_ticks)) = task_start_delay(spec) {
                    let mut deadline = deadline_ticks.load(Ordering::Acquire);
                    if deadline == 0 {
                        let delay_ticks = if TICK_HZ == 0 {
                            1
                        } else {
                            delay_ms.saturating_mul(TICK_HZ).div_ceil(1000).max(1)
                        };
                        let target = now().saturating_add(delay_ticks);
                        let _ = deadline_ticks.compare_exchange(
                            0,
                            target,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        );
                        deadline = deadline_ticks.load(Ordering::Acquire);
                    }
                    if now() < deadline {
                        pending += 1;
                        continue;
                    }
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
