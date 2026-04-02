use alloc::{string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_executor::{SendSpawner, SpawnError, Spawner};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;
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
    GLOBALOG_PERSIST_ONCE_STARTED,
    QJS_ASYNC_FS_SERVICE_STARTED,
    TRUEOSFS_MOUNT_SERVICE_STARTED,
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
    WS_TIME_STARTED,
    ESP_GATE_STARTED,
    ESP_GATE_REGISTRY_STARTED,
    FTP_SERVER_STARTED,
    TGA_TASK_STARTED,
    GFX_VIRGL_READY_TASK_STARTED,
    GFX_VIRGL_CURSOR_OVERLAY_STARTED,
    GFX_TEXTURE_UPLOAD_SERVICE_STARTED,
    HTML_SHACK_SERVICE_STARTED,
    UI2_HOSTED_SYNC_TASK_STARTED,
    UI2_HIT_TASK_STARTED,
    UI2_STARTED,
    UI2_GFX_BROWSER_STARTED,
    UI2_GFX_TETRIS_STARTED,
    UI2_ATHLAS_THIRD_DEMO_STARTED,
    UI2_ATHLAS_HALF_DEMO_STARTED,
    UI2_ATHLAS_1X_DEMO_STARTED,
    UI2_ATHLAS_2X_DEMO_STARTED,
    UI2_PALATINO_1X_DEMO_STARTED,
    UI2_TWEMOJI_1X_STARTED,
    UI2_TRIANGLE_DEMO_STARTED,
    UI2_BGRT_DEMO_STARTED,
    UI2_MANDELBROT_DEMO_STARTED,
    UI2_PCI_DEMO_STARTED,
    UI2_PETERSEN_DEMO_STARTED,
    UI2_PARTICLE_DEMO_STARTED,
    UI2_SMILEY_FOUNTAIN_DEMO_STARTED,
    UI2_SHELL_DEMO_STARTED,
    UI2_SVG_DEMO_STARTED,
    UI2_USB_AUDIO_DEMO_STARTED,
    UI2_TRUEOSFS_EXPLORER_DEMO_STARTED,
    GFX_INTEL_READINESS_PROBE_STARTED,
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
    TRUEOSFS_READY_HOOK_STARTED,
    BOOT_WS_SMOKE_STARTED,
    BOOT_NETBENCH_STARTED,
    SMTP_SMOKE_STARTED,
    UART_SHELL_STARTED,
    NET_TCP_SHELL_STARTED,
    LOGTOTCP_STARTED,
    ATOMIC_BOMB_STARTED,
    SURFER_FACTORY_STARTED
);

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

fn spawn_job_runner(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(crate::wait::job_runner_task()))
}

fn spawn_globalog_persist_once(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(crate::globalog::persist_once_task()))
}

fn spawn_qjs_async_fs_service(spawner: Spawner) -> SpawnAttempt {
    if !trueos_qjs::async_fs::claim_service_start() {
        crate::r::readiness::set(crate::r::readiness::QJS_ASYNC_FS_READY);
        return SpawnAttempt::Spawned;
    }

    match spawner.spawn(trueos_qjs::async_fs::async_fs_service_task()) {
        Ok(()) => {
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
    spawn_local(spawner, |spawner| spawner.spawn(crate::r::fs::trueosfs::mount_service_task()))
}

fn spawn_hv_vm_store(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |ap1_spawner| ap1_spawner.spawn(crate::hv::store::vm_store_task()))
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
    spawn_local(spawner, |spawner| spawner.spawn(crate::net::tls_socket::tls_socket_service_task()))
}

fn spawn_ntp_sync(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(crate::r::net::ntp::ntp_sync_task()))
}

fn spawn_sntp_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(crate::r::net::sntp::sntp_service_task()))
}

fn spawn_net_shell(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(crate::tst_net_tcp_shell::net_shell_task()))
}

fn spawn_logtotcp(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(crate::globalog::logtotcp::logtotcp_task()))
}

fn spawn_ai_qjs_oneshot(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(trueos_qjs::ai_task::run_once()))
}

fn spawn_http_trueosfs(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(crate::tst_http_trueosfs::http_trueosfs_task()))
}

fn spawn_ws_time(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(crate::tst_ws_time::ws_time_task()))
}

fn spawn_esp_gate(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(crate::r::net::esp::esp_gate_task()))
}

fn spawn_esp_gate_registry(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(crate::r::net::esp::esp_gate_registry_task()))
}

fn spawn_ftp_server(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(crate::r::net::ftp::ftp_server_task()))
}

fn spawn_tga_task(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(crate::tga::tga_task()))
}

#[embassy_executor::task]
async fn gfx_virgl_ready_task() {
    crate::gfx::init(crate::limine::framebuffer_response());

    if crate::r::readiness::is_set(crate::r::readiness::GFX_BACKEND_READY) {
        crate::log!("boot-probe: gfx-virgl-backend-ready ms={}\n", boot_probe_ms());
        return;
    }

    if crate::intel::has_claimed_device() {
        crate::log!("boot-probe: gfx-virgl-backend-ready skipped (intel soft-detect claimed)\n");
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
    spawn_local(spawner, |spawner| spawner.spawn(gfx_virgl_ready_task()))
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
    spawn_on_ap1(spawner, |ap1_spawner| ap1_spawner.spawn(gfx_virgl_cursor_overlay_task()))
}

fn spawn_gfx_texture_upload_service(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |ap1_spawner| {
        ap1_spawner.spawn(crate::r::io::cabi::texture_upload_service_task())
    })
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
    let mut result = SpawnAttempt::Skipped;
    for _ in 0..3 {
        let attempt = spawn_on_worker(spawner, |worker_spawner| {
            worker_spawner.spawn(crate::tst_html_shack::html_fetch_service())
        });
        if matches!(attempt, SpawnAttempt::Spawned) {
            result = SpawnAttempt::Spawned;
        }
    }
    result
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
            worker_spawner.spawn(trueos_qjs::browser_task::truesurfer_task(browser_instance_id))
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

    match trueos_qjs::workers::pick_background_spawner().and_then(|worker_spawner| {
        worker_spawner
            .spawn(trueos_qjs::browser_task::truesurfer_task(browser_instance_id))
            .ok()
    }) {
        Some(()) => {
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
        None => {
            crate::log!(
                "truesurfer-factory: handoff-spawn skipped browser_instance_id={}\n",
                browser_instance_id
            );
            None
        }
    }
}

pub fn spawn_additional_truesurfers(spawner: Spawner, requested: u32) -> bool {
    matches!(spawn_truesurfer_batch(spawner, requested), SpawnAttempt::Spawned)
}

fn spawn_truesurfer_factory(spawner: Spawner) -> SpawnAttempt {
    spawn_truesurfer_batch(spawner, truesurfer_factory_boot_count())
}

fn spawn_ui2(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |ap1_spawner| ap1_spawner.spawn(crate::r::ui2::ui2_task()))
}

fn spawn_ui2_hit(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |ap1_spawner| ap1_spawner.spawn(crate::r::ui2::ui2_hit_task()))
}

fn spawn_ui2_hosted(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |ap1_spawner| ap1_spawner.spawn(crate::r::ui2::ui2_hosted_task()))
}

fn spawn_ui2_demo_on_worker<F>(spawner: Spawner, spawn: F) -> SpawnAttempt
where
    F: FnOnce(SendSpawner) -> Result<(), SpawnError>,
{
    spawn_on_worker(spawner, spawn)
}

fn spawn_ui2_gfx_tetris(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::tst_gfx_tetris::ui2_gfx_tetris_task())
    })
}

fn spawn_ui2_athlas_half_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::r::ui2::ui2_font_bucketproducer_demo_task(0))
    })
}

fn spawn_ui2_athlas_third_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::r::ui2::ui2_font_bucketproducer_demo_task(3))
    })
}

fn spawn_ui2_athlas_1x_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::r::ui2::ui2_font_bucketproducer_demo_task(1))
    })
}

fn spawn_ui2_athlas_2x_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::r::ui2::ui2_font_bucketproducer_demo_task(2))
    })
}

fn spawn_ui2_palatino_1x_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::r::ui2::ui2_font_bucketproducer_palatino_demo_task())
    })
}

fn spawn_ui2_twemoji_1x(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::r::ui2::ui2_font_twemoji_loader_task())
    })
}

fn spawn_ui2_triangle_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::tst_ui2_triangle_demo::ui2_triangle_demo_task())
    })
}

fn spawn_ui2_bgrt_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::tst_ui2_bgrt::ui2_bgrt_demo_task())
    })
}

fn spawn_ui2_mandelbrot_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::tst_ui2_mandelbrot_demo::ui2_mandelbrot_demo_task())
    })
}

fn spawn_ui2_pci_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::tst_ui2_pci_demo::ui2_pci_demo_task())
    })
}

fn spawn_ui2_petersen_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::tst_ui2_petersen_demo::ui2_petersen_demo_task())
    })
}

fn spawn_ui2_particle_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::tst_ui2_particle_demo::ui2_particle_demo_task())
    })
}

fn spawn_ui2_smiley_fountain_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::tst_ui2_smiley_fountain_demo::ui2_smiley_fountain_demo_task())
    })
}

fn spawn_ui2_shell_demo(spawner: Spawner) -> SpawnAttempt {
    if let Err(e) = spawner.spawn(crate::shell2::task(spawner, &crate::shell2::UI2_SHELL_BACKEND)) {
        return SpawnAttempt::Failed(e);
    }
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::tst_ui2_shell_demo::ui2_shell_demo_task())
    })
}

fn spawn_ui2_svg_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::tst_ui2_svg_demo::ui2_svg_demo_task())
    })
}

fn spawn_ui2_usb_audio_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::tst_ui2_usb_audio_demo::ui2_usb_audio_demo_task())
    })
}

fn spawn_ui2_trueosfs_explorer_demo(spawner: Spawner) -> SpawnAttempt {
    spawn_ui2_demo_on_worker(spawner, |worker_spawner| {
        worker_spawner.spawn(crate::tst_ui2_trueosfs_explorer_demo::ui2_trueosfs_explorer_demo_task())
    })
}

fn spawn_gfx_intel_readiness_probe(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(crate::intel::scanout_smoke_task()))
}

fn spawn_crabusb_audio(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(crate::usb2::crabusb_audio_task()))
}

fn spawn_crabusb_bsp_service(spawner: Spawner) -> SpawnAttempt {
    let count = crate::usb2::pci_usb_controllers()
        .len()
        .min(crate::usb2::xhci::MAX_XHCI_CONTROLLERS)
        .max(1);
    for i in 0..count {
        spawn_local(spawner, |spawner| spawner.spawn(crate::usb2::crabusb_bsp_service(i, spawner)));
    }
    SpawnAttempt::Spawned
}

fn spawn_crabusb_event_pump(spawner: Spawner) -> SpawnAttempt {
    let count = crate::usb2::pci_usb_controllers()
        .len()
        .min(crate::usb2::xhci::MAX_XHCI_CONTROLLERS)
        .max(1);
    for i in 0..count {
        spawn_local(spawner, |spawner| spawner.spawn(crate::usb2::crabusb_event_pump_task(i)));
    }
    SpawnAttempt::Spawned
}

fn spawn_crabusb_truekey(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(crate::usb2::crabusb_truekey_task()))
}

fn spawn_piano_drain(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(crate::usb2::midi::piano_drain_loop()))
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
    loop {
        flush_user_input_record_once().await;
        Timer::after(EmbassyDuration::from_secs(USER_INPUT_RECORD_FLUSH_INTERVAL_SECS)).await;
    }
}

fn spawn_trueosfs_ready_hook(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(trueosfs_ready_hook_task()))
}

fn spawn_boot_ws_smoke(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    SpawnAttempt::Skipped
}

fn spawn_boot_netbench(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    SpawnAttempt::Skipped
}

fn spawn_smtp_smoke(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| spawner.spawn(crate::tst_smtp_smoke::smtp_smoke_task()))
}

fn spawn_uart_shell(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::shell2::task(spawner, &crate::shell2::UART1_COM1_BACKEND))
    })
}

fn spawn_net_tcp_shell(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        spawner.spawn(crate::shell2::task(spawner, &crate::shell2::NET_TCP_SHELL_BACKEND))
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
    spawn_on_worker(spawner, |worker_spawner| worker_spawner.spawn(atomic_bomb_task()))
}

// --- registry ---

const NET_CONFIGURED_AND_ROOT_READY: u32 =
    crate::r::readiness::NET_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED;
const AI_QJS_ONESHOT_READY: u32 = crate::r::readiness::NET_CONFIGURED
    | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED
    | crate::r::readiness::QJS_ASYNC_FS_READY;
const UI2_DEMO_READY: u32 =
    crate::r::readiness::UI2_READY | crate::r::readiness::GFX_TEXTURE_UPLOAD_SERVICE_READY;
const WS_BOOT_READY: u32 = crate::r::readiness::NET_GATEWAY_REACHABLE
    | crate::r::readiness::TLS_SOCKET_SERVICE_READY
    | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED;
static TASKS: &[TaskSpec] = &[
    TaskSpec::enabled("job-runner", 0, &JOB_RUNNER_STARTED, spawn_job_runner),
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
    TaskSpec::enabled("hv-vm-store-net", 0, &HV_VM_STORE_NET_STARTED, spawn_hv_vm_store_net),
    TaskSpec::enabled("net-poll-tasks", 0, &NET_POLL_STARTED, spawn_net_poll_tasks),
    TaskSpec::enabled("net-service", 0, &NET_SERVICE_STARTED, spawn_net_service),
    TaskSpec::enabled(
        "tls-socket-service",
        crate::r::readiness::NET_CONFIGURED,
        &TLS_SOCKET_SERVICE_STARTED,
        spawn_tls_socket_service,
    ),
    TaskSpec::enabled(
        "ntp-sync",
        crate::r::readiness::NET_CONFIGURED,
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
        crate::r::readiness::NET_CONFIGURED,
        &LOGTOTCP_STARTED,
        spawn_logtotcp,
    ),
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
        crate::r::readiness::NET_CONFIGURED,
        &WS_TIME_STARTED,
        spawn_ws_time,
    ),
    TaskSpec::enabled("esp-gate", 0, &ESP_GATE_STARTED, spawn_esp_gate),
    TaskSpec::enabled("esp-gate-registry", 0, &ESP_GATE_REGISTRY_STARTED, spawn_esp_gate_registry),
    TaskSpec::disabled(
        "ftp-server",
        NET_CONFIGURED_AND_ROOT_READY,
        &FTP_SERVER_STARTED,
        spawn_ftp_server,
    ),
    TaskSpec::disabled("tga", 0, &TGA_TASK_STARTED, spawn_tga_task),
    TaskSpec::enabled(
        "gfx-virgl-backend-ready",
        0,
        &GFX_VIRGL_READY_TASK_STARTED,
        spawn_gfx_virgl_ready_task,
    ),
    TaskSpec::enabled(
        "gfx-virgl-cursor-overlay",
        crate::r::readiness::GFX_BACKEND_READY,
        &GFX_VIRGL_CURSOR_OVERLAY_STARTED,
        spawn_gfx_virgl_cursor_overlay_task,
    ),
    TaskSpec::enabled("html_fetch_service", 0, &HTML_SHACK_SERVICE_STARTED, html_fetch_service),
    TaskSpec::enabled(
        "gfx-texture-upload-service",
        crate::r::readiness::GFX_BACKEND_READY,
        &GFX_TEXTURE_UPLOAD_SERVICE_STARTED,
        spawn_gfx_texture_upload_service,
    ),
    TaskSpec::enabled("ui2", crate::r::readiness::GFX_BACKEND_READY, &UI2_STARTED, spawn_ui2),
    TaskSpec::enabled(
        "ui2-hosted",
        crate::r::readiness::GFX_BACKEND_READY,
        &UI2_HOSTED_SYNC_TASK_STARTED,
        spawn_ui2_hosted,
    ),
    TaskSpec::enabled(
        "ui2-hit",
        crate::r::readiness::GFX_BACKEND_READY,
        &UI2_HIT_TASK_STARTED,
        spawn_ui2_hit,
    ),
    TaskSpec::enabled("truesurfer-factory", 0, &SURFER_FACTORY_STARTED, spawn_truesurfer_factory),
    TaskSpec::disabled(
        "ui2-gfx-tetris",
        UI2_DEMO_READY,
        &UI2_GFX_TETRIS_STARTED,
        spawn_ui2_gfx_tetris,
    ),
    TaskSpec::disabled(
        "ui2-athlas-third-demo",
        UI2_DEMO_READY,
        &UI2_ATHLAS_THIRD_DEMO_STARTED,
        spawn_ui2_athlas_third_demo,
    ),
    TaskSpec::disabled(
        "ui2-athlas-half-demo",
        UI2_DEMO_READY,
        &UI2_ATHLAS_HALF_DEMO_STARTED,
        spawn_ui2_athlas_half_demo,
    ),
    TaskSpec::disabled(
        "ui2-athlas-1x-demo",
        UI2_DEMO_READY,
        &UI2_ATHLAS_1X_DEMO_STARTED,
        spawn_ui2_athlas_1x_demo,
    ),
    TaskSpec::disabled(
        "ui2-athlas-2x-demo",
        UI2_DEMO_READY,
        &UI2_ATHLAS_2X_DEMO_STARTED,
        spawn_ui2_athlas_2x_demo,
    ),
    TaskSpec::disabled(
        "ui2-palatino-1x-demo",
        UI2_DEMO_READY,
        &UI2_PALATINO_1X_DEMO_STARTED,
        spawn_ui2_palatino_1x_demo,
    ),
    TaskSpec::disabled(
        "ui2-twemoji-1x",
        crate::r::readiness::GFX_TEXTURE_UPLOAD_SERVICE_READY,
        &UI2_TWEMOJI_1X_STARTED,
        spawn_ui2_twemoji_1x,
    ),
    TaskSpec::enabled(
        "ui2-triangle-demo",
        UI2_DEMO_READY,
        &UI2_TRIANGLE_DEMO_STARTED,
        spawn_ui2_triangle_demo,
    ),
    TaskSpec::disabled(
        "ui2-bgrt-demo",
        UI2_DEMO_READY,
        &UI2_BGRT_DEMO_STARTED,
        spawn_ui2_bgrt_demo,
    ),
    TaskSpec::disabled(
        "ui2-mandelbrot-demo",
        UI2_DEMO_READY,
        &UI2_MANDELBROT_DEMO_STARTED,
        spawn_ui2_mandelbrot_demo,
    ),
    TaskSpec::disabled(
        "ui2-device-manager-demo",
        UI2_DEMO_READY,
        &UI2_PCI_DEMO_STARTED,
        spawn_ui2_pci_demo,
    ),
    TaskSpec::disabled(
        "ui2-petersen-demo",
        UI2_DEMO_READY,
        &UI2_PETERSEN_DEMO_STARTED,
        spawn_ui2_petersen_demo,
    ),
    TaskSpec::disabled(
        "ui2-particle-demo",
        UI2_DEMO_READY,
        &UI2_PARTICLE_DEMO_STARTED,
        spawn_ui2_particle_demo,
    ),
    TaskSpec::disabled(
        "ui2-smiley-fountain-demo",
        UI2_DEMO_READY,
        &UI2_SMILEY_FOUNTAIN_DEMO_STARTED,
        spawn_ui2_smiley_fountain_demo,
    ),
    TaskSpec::disabled(
        "ui2-shell-demo",
        UI2_DEMO_READY,
        &UI2_SHELL_DEMO_STARTED,
        spawn_ui2_shell_demo,
    ),
    TaskSpec::disabled("ui2-svg-demo", UI2_DEMO_READY, &UI2_SVG_DEMO_STARTED, spawn_ui2_svg_demo),
    TaskSpec::disabled(
        "ui2-usb-audio-demo",
        UI2_DEMO_READY,
        &UI2_USB_AUDIO_DEMO_STARTED,
        spawn_ui2_usb_audio_demo,
    ),
    TaskSpec::disabled(
        "ui2-trueosfs-explorer-demo",
        UI2_DEMO_READY | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
        &UI2_TRUEOSFS_EXPLORER_DEMO_STARTED,
        spawn_ui2_trueosfs_explorer_demo,
    ),
    TaskSpec::enabled(
        "gfx-intel-readiness-probe",
        crate::r::readiness::GFX_INTEL_CLAIMED,
        &GFX_INTEL_READINESS_PROBE_STARTED,
        spawn_gfx_intel_readiness_probe,
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
    TaskSpec::enabled("crabusb-audio", 0, &CRABUSB_AUDIO_STARTED, spawn_crabusb_audio),
    TaskSpec::enabled("crabusb-truekey", 0, &CRABUSB_TRUEKEY_STARTED, spawn_crabusb_truekey),
    TaskSpec::enabled("piano-drain", 0, &PIANO_DRAIN_STARTED, spawn_piano_drain),
    TaskSpec::enabled(
        "trueosfs-ready-hook",
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
        &TRUEOSFS_READY_HOOK_STARTED,
        spawn_trueosfs_ready_hook,
    ),
    TaskSpec::disabled("boot-ws-smoke", WS_BOOT_READY, &BOOT_WS_SMOKE_STARTED, spawn_boot_ws_smoke),
    TaskSpec::disabled("smtp-smoke", 0, &SMTP_SMOKE_STARTED, spawn_smtp_smoke),
    TaskSpec::disabled("boot-netbench", 0, &BOOT_NETBENCH_STARTED, spawn_boot_netbench),
    TaskSpec::enabled("uart-shell", 0, &UART_SHELL_STARTED, spawn_uart_shell),
    TaskSpec::enabled("net-tcp-shell", 0, &NET_TCP_SHELL_STARTED, spawn_net_tcp_shell),
    TaskSpec::disabled("atomic_bomb", 0, &ATOMIC_BOMB_STARTED, spawn_atomic_bomb),
];

#[embassy_executor::task]
pub async fn spawn_service_task(spawner: Spawner) {
    async move {
        loop {
            let ready = crate::r::readiness::mask();
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
                                | "ui2-gfx-browser"
                                | "ui2-gfx-tetris"
                                | "ui2-triangle-demo"
                                | "ui2-mandelbrot-demo"
                                | "ui2-petersen-demo"
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
