use alloc::{string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::{SpawnError, SpawnToken, Spawner};
use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::r::spawn_spec::{SpawnAttempt, TaskSpec};
// NOTE: This file is intended to become the single source of truth for Embassy task startup.

const SPAWN_SERVICE_AFTER_START_MS: u64 = 25;
const SPAWN_SERVICE_PENDING_MS: u64 = 150;
const SPAWN_SERVICE_IDLE_MS: u64 = 250;

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
    BLOCKING_JOB_DISPATCHER_STARTED,
    SMP_HLT_HISTORY_STARTED,
    CODEC_SERVICE_STARTED,
    QJS_ASYNC_FS_SERVICE_STARTED,
    TRUEOSFS_MOUNT_SERVICE_STARTED,
    TRUEOSFS_INDEX_SERVICE_STARTED,
    HV_VM_STORE_STARTED,
    HV_VM_STORE_NET_STARTED,
    NET_POLL_STARTED,
    NET_SERVICE_STARTED,
    NET_CACHE_SERVICE_STARTED,
    TLS_SOCKET_SERVICE_STARTED,
    NTP_SYNC_STARTED,
    SNTP_SERVICE_STARTED,
    NET_SHELL_STARTED,
    TACTICS_SRV_STARTED,
    HID_UDP_SRV_STARTED,
    AI_QJS_ONESHOT_STARTED,
    HTTP_TRUEOSFS_STARTED,
    WS_TIME_STARTED,
    LAN_DISCOVERY_STARTED,
    ESP_GATE_REGISTRY_STARTED,
    ESP_PIANO_AUDIO_STARTED,
    ESP_PIANO_UDP_STARTED,
    FTP_SERVER_STARTED,
    TGA_TASK_STARTED,
    INTEL_CURSOR_SERVICE_STARTED,
    HW_PIC_SERVICE_STARTED,
    HW_VID_PROBE_STARTED,
    HW_LOGO_PRESENT_TASK_STARTED,
    VIRTIO_GPU_UI_STARTED,
    INTEL_HDA_AUDIO_DEMO_STARTED,
    RAPLE_SERVICE_STARTED,
    HTML_SHACK_SERVICE_STARTED,
    ASSET_SHACK_SERVICE_STARTED,
    USB_CONTROLLER_TASKS_STARTED,
    TRUEOSFS_READY_HOOK_STARTED,
    TRUEOSFS_RW_PROBE_STARTED,
    BP_AUTOSTART_STARTED,
    APP_VM_RUN_QUEUE_STARTED,
    FACTORY_RAM_PROBE_STARTED,
    UART_SHELL_STARTED,
    NET_TCP_SHELL_STARTED,
    UI3_SHELL_STARTED,
    LOGTOTCP_STARTED,
    SHADER_COMPILE_SERVICE_STARTED,
    SILK_SERVICE_STARTED,
    ATOMIC_BOMB_STARTED,
    SURFER_PARSE_POOL_STARTED,
    UI3_ORBITS_STARTED,
    UI3_SERVICE_STARTED,
    I226_DIAGNOSTIC_DISPLAY_STARTED,
    AUD_FILE_SERVICE_STARTED,
    TINYAUDIO_SERVICE_STARTED,
    TINYAUDIO_LIVE_HTTP_STARTED,
    EXECUTOR_REALM_MIGRATION_SMOKE_STARTED
);

#[cfg(feature = "trueos_rdp")]
static RESOURCE_MONITOR_STARTED: AtomicBool = AtomicBool::new(false);

macro_rules! define_stop_flags {
    ($($name:ident),* $(,)?) => {
        $(static $name: AtomicBool = AtomicBool::new(false);)*
    };
}

define_stop_flags!(
    STOP_UI2_TEXT_INPUT_DEMO,
    STOP_UI2_TEXT_AREA_DEMO,
    STOP_UI2_ANALOG_CLOCK_DEMO,
    STOP_UI2_BGRT_DEMO,
    STOP_UI2_CORETICKS_DEMO,
    STOP_UI2_CURSORPICKER_DEMO,
    STOP_UI2_GBOI_DEMO,
    STOP_UI2_INTEL_CANVAS3D_DEMO,
    STOP_UI2_INTEL_CANVAS3D_PLANE_PATCH_DEMO,
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
        "ui2-text-area-demo" => Some(&STOP_UI2_TEXT_AREA_DEMO),
        "ui2-analog-clock-demo" => Some(&STOP_UI2_ANALOG_CLOCK_DEMO),
        "ui2-bgrt-demo" => Some(&STOP_UI2_BGRT_DEMO),
        "ui2-coreticks-demo" => Some(&STOP_UI2_CORETICKS_DEMO),
        "ui2-cursorpicker-demo" => Some(&STOP_UI2_CURSORPICKER_DEMO),
        "ui2-gboi-demo" => Some(&STOP_UI2_GBOI_DEMO),
        "ui2-intel-canvas3d-demo" => Some(&STOP_UI2_INTEL_CANVAS3D_DEMO),
        "ui2-intel-canvas3d-plane-patch-demo" => Some(&STOP_UI2_INTEL_CANVAS3D_PLANE_PATCH_DEMO),
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
    if let Some(index) = task_index_by_name(name) {
        if let Some(spec) = TASKS.get(index) {
            spec.started.store(false, Ordering::Release);
        }
    }
}

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
    task: impl FnOnce(crate::workers::WorkerSpawner) -> Result<SpawnToken<S>, SpawnError>,
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
    task: impl FnOnce(crate::workers::WorkerSpawner) -> Result<SpawnToken<S>, SpawnError>,
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

#[inline]
fn spawn_bool_result_to_attempt(result: Result<bool, SpawnError>) -> SpawnAttempt {
    match result {
        Ok(true) => SpawnAttempt::Spawned,
        Ok(false) => SpawnAttempt::Skipped,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_job_runner(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::wait::job_runner_task())
}

fn spawn_blocking_service_lanes(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::blocking::blocking_job_dispatcher_task())
}

fn spawn_smp_hlt_history(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::smp::hlt_history_sampler_task())
}

fn spawn_codec_service(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    let Some(profile) = crate::cpu::CpuProfile::for_slot(1) else {
        return SpawnAttempt::Skipped;
    };
    let Some(ap1_spawner) = crate::workers::spawner_for_slot(profile.slot()) else {
        return SpawnAttempt::Skipped;
    };

    let mut spawned = 0usize;
    for worker_id in 0..3 {
        match crate::r::codec::codec_worker_task(worker_id) {
            Ok(token) => {
                ap1_spawner.spawn(token);
                spawned = spawned.saturating_add(1);
            }
            Err(err) if spawned == 0 => return SpawnAttempt::Failed(err),
            Err(err) => {
                crate::log_warn!(
                    target: "service";
                    "codec: worker={} spawn failed err={:?}\n",
                    worker_id,
                    err
                );
            }
        }
    }

    if spawned == 0 {
        SpawnAttempt::Skipped
    } else {
        SpawnAttempt::Spawned
    }
}

fn spawn_factory_ram_probe(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::ram_probe::boot_factory_ram_probe_task())
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
            Err(e) => {
                crate::log_warn!(
                    target: "net";
                    "net: spawn net_poll_task({}) failed: {:?}\n",
                    idx,
                    e
                )
            }
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
                crate::log_warn!(
                    target: "net";
                    "net: spawn net_service_task({}) failed: {:?}\n",
                    idx,
                    e
                );
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

fn spawn_i226_diagnostic_display(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |_worker_spawner| crate::net::i226::i226_diagnostic_display_task())
}

fn spawn_net_cache_service(spawner: Spawner) -> SpawnAttempt {
    spawn_bool_result_to_attempt(crate::net::cache_service::ensure_service_started(spawner))
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

fn spawn_tactics_srv(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::tst_tactics_srv::tactics_srv_task())
}

fn spawn_hid_udp_srv(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::hid_udp_srv::hid_udp_srv_task())
}

#[cfg(feature = "trueos_rdp")]
fn spawn_resource_monitor(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::resource_monitor::resource_monitor_task())
}

fn spawn_logtotcp(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::globalog::logtotcp::logtotcp_task())
}

fn spawn_shader_compile_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::shader::shader_compile_service_task())
}

fn spawn_silk_service(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |_worker_spawner| crate::r::silk_service::silk_service_task())
}

fn spawn_ai_qjs_oneshot(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    SpawnAttempt::Skipped
}

fn spawn_http_trueosfs(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::tst_http_trueosfs::http_trueosfs_task())
}

fn spawn_ws_time(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::tst_ws_time::ws_time_task())
}

fn spawn_lan_discovery(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::discovery::lan_discovery_task())
}

fn spawn_esp_gate_registry(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::esp::esp_gate_registry_task())
}

fn spawn_esp_piano_udp(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::esp::esp_piano_udp_task())
}

fn spawn_esp_piano_audio(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |_worker_spawner| crate::aud::live_piano::task())
}

fn spawn_ftp_server(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::ftp::ftp_server_task())
}

fn spawn_tga_task(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::tga::tga_task())
}

#[embassy_executor::task]
async fn intel_cursor_service_task() {
    crate::log_info!(
        target: "gfx";
        "boot-probe: intel-cursor-service task start ms={}\n",
        boot_probe_ms()
    );
    loop {
        let _ = crate::intel::update_kernel_hw_cursor();
        Timer::after(EmbassyDuration::from_millis(16)).await;
    }
}

fn spawn_intel_cursor_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| intel_cursor_service_task())
}

fn spawn_hw_pic_service(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |_worker_spawner| crate::intel::hw_pic_service())
}

fn spawn_hw_vid_probe_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |_worker_spawner| crate::intel::hw_vid_probe_task_spawn())
}

fn spawn_hw_logo_present_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |_worker_spawner| crate::intel::hw_logo_present_task())
}

fn spawn_virtio_gpu_ui_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |_worker_spawner| crate::virtio_gpu_logo::emulator_ui_task())
}

fn spawn_intel_hda_audio_demo_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |worker_spawner| {
        let _ = worker_spawner;
        crate::intel_hda_audio_demo::task()
    })
}

fn spawn_raple_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::power::rapl::raple_service())
}

fn html_fetch_service(spawner: Spawner) -> SpawnAttempt {
    spawn_bool_result_to_attempt(crate::surfer::spawn_html_fetch_service(spawner))
}

fn asset_fetch_service(spawner: Spawner) -> SpawnAttempt {
    spawn_bool_result_to_attempt(crate::surfer::spawn_asset_fetch_service(spawner))
}

fn spawn_truesurfer_parse_pool(spawner: Spawner) -> SpawnAttempt {
    spawn_bool_result_to_attempt(crate::surfer::spawn_truesurfer_parse_pool(spawner))
}

fn spawn_ui3_service(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |_ap1_spawner| crate::ui3::ui3_service_task())
}

fn spawn_ui3_orbits(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |_ap1_spawner| crate::ui3::ui3_orbits::ui3_orbits_task())
}

fn spawn_aud_file_service(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |_ap1_spawner| crate::aud::file_service::aud_file_service_task())
}

fn spawn_tinyaudio_service(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1(spawner, |_ap1_spawner| crate::tst::esynth::tinyaudio_service_task())
}

fn spawn_tinyaudio_live_http(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::tst_audio_live_http::tinyaudio_live_http_task())
}

#[inline]
fn intel_cursor_service_gate() -> bool {
    crate::intel::has_claimed_device()
}

#[inline]
fn intel_full_ui3_gate() -> bool {
    crate::intel::full_ui3_boot_enabled()
}

#[inline]
fn i226_diagnostic_display_gate() -> bool {
    crate::intel::has_claimed_device() && crate::net::i226::has_primary_snapshot()
}

#[inline]
fn intel_media_engine_gate() -> bool {
    crate::intel::has_media_decode_engine()
}

#[inline]
fn virtio_gpu_ui_gate() -> bool {
    crate::virtio_gpu_logo::present()
}

fn spawn_usb_controller_tasks(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::usb2::usb_controller_service_task())
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
    crate::log_info!(target: "service"; "spawn-svc: trueosfs-ready-hook task online\n");
    crate::intel::run_media_source_warmup_async().await;
    loop {
        flush_user_input_record_once().await;
        Timer::after(EmbassyDuration::from_secs(USER_INPUT_RECORD_FLUSH_INTERVAL_SECS)).await;
    }
}

fn spawn_trueosfs_ready_hook(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| trueosfs_ready_hook_task())
}

const TRUEOSFS_RW_PROBE_PATH: &str = "trueos/probe/rw-500k.bin";
const TRUEOSFS_RW_PROBE_BYTES: usize = 500 * 1024;
const TRUEOSFS_RW_PROBE_CHUNK_BYTES: usize = 64 * 1024;

fn trueosfs_rw_probe_now_ms() -> u64 {
    let ticks = embassy_time_driver::now();
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        0
    } else {
        ticks.saturating_mul(1000) / hz
    }
}

fn trueosfs_rw_probe_fill(buf: &mut Vec<u8>, len: usize) {
    buf.clear();
    for i in 0..len {
        let b = ((i as u32)
            .wrapping_mul(37)
            .wrapping_add((i as u32 >> 3) ^ 0x5a)) as u8;
        buf.push(b);
    }
}

#[embassy_executor::task]
async fn trueosfs_rw_probe_task() {
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        crate::log!("trueosfs-rw-probe: result=failed phase=root err=missing\n");
        return;
    };

    let start_ms = trueosfs_rw_probe_now_ms();
    crate::log!(
        "trueosfs-rw-probe: start disk={} path={} bytes={}\n",
        disk.id().raw(),
        TRUEOSFS_RW_PROBE_PATH,
        TRUEOSFS_RW_PROBE_BYTES
    );

    let _ = crate::r::fs::trueosfs::file_delete_async(disk, TRUEOSFS_RW_PROBE_PATH).await;

    let mut expected = Vec::with_capacity(TRUEOSFS_RW_PROBE_BYTES);
    trueosfs_rw_probe_fill(&mut expected, TRUEOSFS_RW_PROBE_BYTES);

    let handle = match crate::r::fs::trueosfs::file_write_begin_async(
        disk,
        TRUEOSFS_RW_PROBE_PATH,
        TRUEOSFS_RW_PROBE_BYTES as u64,
    )
    .await
    {
        Ok(Some(handle)) => handle,
        Ok(None) => {
            crate::log!("trueosfs-rw-probe: result=failed phase=begin err=no-space-or-fs\n");
            return;
        }
        Err(err) => {
            crate::log!("trueosfs-rw-probe: result=failed phase=begin err={:?}\n", err);
            return;
        }
    };

    let mut offset = 0usize;
    while offset < expected.len() {
        let end = (offset + TRUEOSFS_RW_PROBE_CHUNK_BYTES).min(expected.len());
        if let Err(err) =
            crate::r::fs::trueosfs::file_write_chunk_async(handle, &expected[offset..end]).await
        {
            let _ = crate::r::fs::trueosfs::file_write_abort_async(handle).await;
            crate::log!(
                "trueosfs-rw-probe: result=failed phase=write offset={} len={} err={:?}\n",
                offset,
                end - offset,
                err
            );
            return;
        }
        offset = end;
    }

    if let Err(err) = crate::r::fs::trueosfs::file_write_finish_async(handle).await {
        crate::log!("trueosfs-rw-probe: result=failed phase=finish err={:?}\n", err);
        return;
    }
    let write_ms = trueosfs_rw_probe_now_ms();
    crate::log!(
        "trueosfs-rw-probe: phase=write-ok bytes={} elapsed_ms={}\n",
        TRUEOSFS_RW_PROBE_BYTES,
        write_ms.saturating_sub(start_ms)
    );

    let mut actual = Vec::new();
    actual.resize(TRUEOSFS_RW_PROBE_BYTES, 0);
    match crate::r::fs::trueosfs::file_read_range_async(
        disk,
        TRUEOSFS_RW_PROBE_PATH,
        0,
        actual.as_mut_slice(),
    )
    .await
    {
        Ok(Some(got)) if got == TRUEOSFS_RW_PROBE_BYTES => {}
        Ok(Some(got)) => {
            crate::log!(
                "trueosfs-rw-probe: result=failed phase=read got={} expected={}\n",
                got,
                TRUEOSFS_RW_PROBE_BYTES
            );
            return;
        }
        Ok(None) => {
            crate::log!("trueosfs-rw-probe: result=failed phase=read err=missing\n");
            return;
        }
        Err(err) => {
            crate::log!("trueosfs-rw-probe: result=failed phase=read err={:?}\n", err);
            return;
        }
    }

    if actual.as_slice() != expected.as_slice() {
        let mismatch = actual
            .iter()
            .zip(expected.iter())
            .position(|(got, want)| got != want)
            .unwrap_or(usize::MAX);
        crate::log!("trueosfs-rw-probe: result=failed phase=verify mismatch={}\n", mismatch);
        return;
    }
    let read_ms = trueosfs_rw_probe_now_ms();
    crate::log!(
        "trueosfs-rw-probe: phase=verify-ok bytes={} read_elapsed_ms={}\n",
        TRUEOSFS_RW_PROBE_BYTES,
        read_ms.saturating_sub(write_ms)
    );

    match crate::r::fs::trueosfs::file_delete_async(disk, TRUEOSFS_RW_PROBE_PATH).await {
        Ok(true) | Ok(false) => {
            let done_ms = trueosfs_rw_probe_now_ms();
            crate::log!(
                "trueosfs-rw-probe: result=ok bytes={} total_elapsed_ms={}\n",
                TRUEOSFS_RW_PROBE_BYTES,
                done_ms.saturating_sub(start_ms)
            );
        }
        Err(err) => {
            crate::log!("trueosfs-rw-probe: result=failed phase=delete err={:?}\n", err);
        }
    }
}

fn spawn_trueosfs_rw_probe(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| trueosfs_rw_probe_task())
}

fn spawn_app_vm_run_queue(spawner: Spawner) -> SpawnAttempt {
    match crate::shell2::spawn_app_vm_run_queue(spawner) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

#[derive(Clone, Copy)]
struct BlueprintAutostart {
    enabled: bool,
    label: &'static str,
    archive: &'static str,
    slot: &'static str,
    args: &'static [&'static str],
    settle_ms: u64,
}

const BP_AUTOSTARTS: &[BlueprintAutostart] = &[
    BlueprintAutostart {
        enabled: false,
        label: "horizon",
        archive: "horizon.bp",
        slot: "hor",
        args: &[],
        settle_ms: 250,
    },
    BlueprintAutostart {
        enabled: false,
        label: "mandelbrot",
        archive: "mandelbrot.bp",
        slot: "man",
        args: &[],
        settle_ms: 750,
    },
    BlueprintAutostart {
        enabled: false,
        label: "flags",
        archive: "flags.bp",
        slot: "flg",
        args: &[],
        settle_ms: 750,
    },
    BlueprintAutostart {
        enabled: false,
        label: "weather",
        archive: "weather.bp",
        slot: "wth",
        args: &[],
        settle_ms: 750,
    },
    BlueprintAutostart {
        enabled: false,
        label: "chart",
        archive: "chart.bp",
        slot: "chr",
        args: &[],
        settle_ms: 750,
    },
    BlueprintAutostart {
        enabled: false,
        label: "hello_world",
        archive: "hello_world.bp",
        slot: "h_w",
        args: &[],
        settle_ms: 750,
    },
    BlueprintAutostart {
        enabled: false,
        label: "chatserver",
        archive: "chatserver.bp",
        slot: "cht",
        args: &[],
        settle_ms: 750,
    },
    BlueprintAutostart {
        enabled: false,
        label: "file-system",
        archive: "file-system.bp",
        slot: "fs",
        args: &[],
        settle_ms: 750,
    },
    BlueprintAutostart {
        enabled: false,
        label: "bat",
        archive: "bat.bp",
        slot: "bat",
        args: &["--help"],
        settle_ms: 750,
    },
];

#[embassy_executor::task]
async fn bp_autostart_task() {
    crate::r::readiness::wait_for(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED).await;

    for config in BP_AUTOSTARTS {
        if !config.enabled {
            crate::log!(
                "spawn-svc: bp-autostart disabled label={} archive={} slot={}\n",
                config.label,
                config.archive,
                config.slot
            );
            continue;
        }

        if config.settle_ms != 0 {
            Timer::after(EmbassyDuration::from_millis(config.settle_ms)).await;
        }

        let target =
            crate::shell2::matrix_target_for_slot_name(crate::shell2::OUTPUT_UI3_MASK, config.slot);

        crate::log!(
            "spawn-svc: bp-autostart begin label={} archive={} slot={}\n",
            config.label,
            config.archive,
            config.slot
        );

        match crate::shell2::cmds::run::submit_archive_name_to_target_prefer_embedded_async(
            target,
            config.archive,
            config.args.iter().map(|arg| String::from(*arg)).collect(),
        )
        .await
        {
            Ok(source) => crate::log!(
                "spawn-svc: bp-autostart queued label={} archive={} slot={} source={}\n",
                config.label,
                config.archive,
                config.slot,
                source
            ),
            Err(err) => crate::log!(
                "spawn-svc: bp-autostart skipped label={} archive={} slot={} err={}\n",
                config.label,
                config.archive,
                config.slot,
                err
            ),
        }
    }

    //let html = crate::surfer::html_shack::Html::new(
    //    "inline://trueos/input.html",
    //    include_str!("../../crates/trueos-qjs/src/html/input.html"),
    //);
    //let _ = crate::surfer::html_shack::enqueue_ready_html_for_browser(html).await;
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

fn spawn_ui3_shell(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| crate::shell2::task(spawner, &crate::shell2::UI3_SHELL_BACKEND))
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

const EXECUTOR_REALM_SMOKE_HOPS: usize = 25;

#[inline]
fn executor_realm_smoke_delay_ms(hop: usize) -> u64 {
    let mixed = (hop as u64)
        .wrapping_mul(1_103_515_245)
        .wrapping_add(12_345);
    3 + ((mixed >> 16) % 23)
}

#[embassy_executor::task]
async fn executor_realm_migration_smoke_task(
    bsp_target: embassy_executor::MigrationTarget,
    ap_target: embassy_executor::MigrationTarget,
    ap_slot: u32,
    ap_kind: u8,
    bsp_executor_id: usize,
    ap_executor_id: usize,
) {
    let start_ms = boot_probe_ms();
    crate::log_info!(
        target: "executor-realm";
        "executor-realm: migration-smoke start ms={} hops={} bsp_exec=0x{:X} ap_slot={} ap_kind={} ap_exec=0x{:X}\n",
        start_ms,
        EXECUTOR_REALM_SMOKE_HOPS,
        bsp_executor_id,
        ap_slot,
        ap_kind,
        ap_executor_id
    );

    let mut ok_hops = 0usize;
    for hop in 0..EXECUTOR_REALM_SMOKE_HOPS {
        let to_ap = hop % 2 == 0;
        let target = if to_ap { ap_target } else { bsp_target };
        let to_slot = if to_ap { ap_slot as usize } else { 0 };
        let from_slot = crate::percpu::current_slot();
        let arm_ms = boot_probe_ms();
        let delay_ms = executor_realm_smoke_delay_ms(hop);

        crate::log_trace!(
            target: "executor-realm";
            "executor-realm: migration-smoke arm hop={} ms={} delay_ms={} from_cpu={} to_cpu={} from_exec=0x{:X} to_exec=0x{:X}\n",
            hop,
            arm_ms,
            delay_ms,
            from_slot,
            to_slot,
            if to_ap { bsp_executor_id } else { ap_executor_id },
            target.executor_id()
        );

        Timer::after(EmbassyDuration::from_millis(delay_ms)).await;

        let request_ms = boot_probe_ms();
        // Safety: this smoke task is spawned through SendSpawner below, so the
        // compiler verifies the whole future is Send before it may cross CPUs.
        let result = unsafe { embassy_executor::migrate_current_task_to(target) }.await;
        let done_ms = boot_probe_ms();
        let current_slot = crate::percpu::current_slot();
        let hop_ok = result.migrated && current_slot == to_slot;
        if hop_ok {
            ok_hops = ok_hops.saturating_add(1);
        }

        crate::log_trace!(
            target: "executor-realm";
            "executor-realm: migration-smoke hop={} task=0x{:X} request_ms={} done_ms={} wait_ms={} migrate_ms={} cpu_from={} cpu_to={} cpu_now={} spawner_from=0x{:X} spawner_to=0x{:X} spawner_now=0x{:X} ok={}\n",
            hop,
            result.task_id,
            request_ms,
            done_ms,
            request_ms.saturating_sub(arm_ms),
            done_ms.saturating_sub(request_ms),
            from_slot,
            to_slot,
            current_slot,
            result.from_executor_id,
            result.to_executor_id,
            result.current_executor_id,
            hop_ok
        );
    }

    crate::log_info!(
        target: "executor-realm";
        "executor-realm: migration-smoke done ms={} ok_hops={}/{} final_cpu={} bsp_exec=0x{:X} ap_slot={} ap_exec=0x{:X}\n",
        boot_probe_ms(),
        ok_hops,
        EXECUTOR_REALM_SMOKE_HOPS,
        crate::percpu::current_slot(),
        bsp_executor_id,
        ap_slot,
        ap_executor_id
    );
}

fn spawn_executor_realm_migration_smoke(spawner: Spawner) -> SpawnAttempt {
    let Some((ap_slot, ap_kind, worker_spawner)) =
        crate::workers::pick_background_spawner_with_slot()
    else {
        return SpawnAttempt::Skipped;
    };

    let bsp_spawner = spawner.make_send();
    let ap_spawner = worker_spawner.raw();
    let bsp_target = bsp_spawner.migration_target();
    let ap_target = ap_spawner.migration_target();
    let bsp_executor_id = bsp_spawner.executor_id();
    let ap_executor_id = ap_spawner.executor_id();

    match executor_realm_migration_smoke_task(
        bsp_target,
        ap_target,
        ap_slot,
        ap_kind,
        bsp_executor_id,
        ap_executor_id,
    ) {
        Ok(token) => {
            let task_id = token.id();
            crate::log_info!(
                target: "executor-realm";
                "executor-realm: migration-smoke spawn task=0x{:X} bsp_exec=0x{:X} ap_slot={} ap_kind={} ap_exec=0x{:X}\n",
                task_id,
                bsp_executor_id,
                ap_slot,
                ap_kind,
                ap_executor_id
            );
            bsp_spawner.spawn(token);
            SpawnAttempt::Spawned
        }
        Err(e) => SpawnAttempt::Failed(e),
    }
}

// --- registry ---

const NET_ANY_CONFIGURED_AND_ROOT_READY: u32 =
    crate::r::readiness::NET_ANY_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED;
const NET_ANY_CONFIGURED_AND_INDEX_READY: u32 = crate::r::readiness::NET_ANY_CONFIGURED
    | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED
    | crate::r::readiness::TRUEOSFS_INDEX_READY;
const AI_QJS_ONESHOT_READY: u32 = crate::r::readiness::NET_ANY_CONFIGURED
    | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED
    | crate::r::readiness::QJS_ASYNC_FS_READY;
const BP_AUTOSTART_READY: u32 = crate::r::readiness::TRUEOSFS_ROOT_MOUNTED
    | crate::r::readiness::BACKGROUND_AP_WORKER_READY
    | crate::r::readiness::VTHREAD_HW_TAG_READY;
const TASK_COUNT: usize = 57 + cfg!(feature = "trueos_rdp") as usize;
static TASKS: [TaskSpec; TASK_COUNT] = [
    TaskSpec::enabled("job-runner", 0, &JOB_RUNNER_STARTED, spawn_job_runner),
    TaskSpec::enabled(
        "blocking-service-lanes",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        &BLOCKING_JOB_DISPATCHER_STARTED,
        spawn_blocking_service_lanes,
    ),
    TaskSpec::enabled("smp-hlt-history", 0, &SMP_HLT_HISTORY_STARTED, spawn_smp_hlt_history),
    TaskSpec::enabled(
        "executor-realm-migration-smoke",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        &EXECUTOR_REALM_MIGRATION_SMOKE_STARTED,
        spawn_executor_realm_migration_smoke,
    ),
    TaskSpec::enabled(
        "codec-service",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        &CODEC_SERVICE_STARTED,
        spawn_codec_service,
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
    TaskSpec::enabled(
        "hv-vm-store-net",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &HV_VM_STORE_NET_STARTED,
        spawn_hv_vm_store_net,
    ),
    TaskSpec::enabled("net-poll-tasks", 0, &NET_POLL_STARTED, spawn_net_poll_tasks),
    TaskSpec::enabled("net-service", 0, &NET_SERVICE_STARTED, spawn_net_service),
    TaskSpec::enabled("net-cache-service", 0, &NET_CACHE_SERVICE_STARTED, spawn_net_cache_service),
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
        "tactics-srv",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &TACTICS_SRV_STARTED,
        spawn_tactics_srv,
    ),
    TaskSpec::enabled(
        "hid-udp-srv",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &HID_UDP_SRV_STARTED,
        spawn_hid_udp_srv,
    ),
    #[cfg(feature = "trueos_rdp")]
    TaskSpec::enabled("resource-monitor", 0, &RESOURCE_MONITOR_STARTED, spawn_resource_monitor),
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
    TaskSpec::enabled(
        "http-trueosfs",
        NET_ANY_CONFIGURED_AND_INDEX_READY,
        &HTTP_TRUEOSFS_STARTED,
        spawn_http_trueosfs,
    ),
    TaskSpec::enabled(
        "trueosfs-rw-probe",
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED | crate::r::readiness::TRUEOSFS_INDEX_READY,
        &TRUEOSFS_RW_PROBE_STARTED,
        spawn_trueosfs_rw_probe,
    ),
    TaskSpec::enabled(
        "shader-compile-service",
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
        &SHADER_COMPILE_SERVICE_STARTED,
        spawn_shader_compile_service,
    ),
    TaskSpec::disabled(
        "silk-service",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        &SILK_SERVICE_STARTED,
        spawn_silk_service,
    ),
    TaskSpec::enabled("app-vm-run-queue", 0, &APP_VM_RUN_QUEUE_STARTED, spawn_app_vm_run_queue),
    TaskSpec::enabled(
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
    TaskSpec::enabled(
        "lan-discovery",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &LAN_DISCOVERY_STARTED,
        spawn_lan_discovery,
    ),
    TaskSpec::disabled("esp-gate-registry", 0, &ESP_GATE_REGISTRY_STARTED, spawn_esp_gate_registry),
    TaskSpec::disabled("esp-piano-audio", 0, &ESP_PIANO_AUDIO_STARTED, spawn_esp_piano_audio),
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
    TaskSpec::disabled(
        "tga",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &TGA_TASK_STARTED,
        spawn_tga_task,
    ),
    TaskSpec::enabled_gated(
        "intel-cursor-service",
        0,
        intel_cursor_service_gate,
        &INTEL_CURSOR_SERVICE_STARTED,
        spawn_intel_cursor_service_task,
    ),
    TaskSpec::enabled_gated(
        "hw_pic_service",
        0,
        intel_media_engine_gate,
        &HW_PIC_SERVICE_STARTED,
        spawn_hw_pic_service,
    ),
    TaskSpec::enabled_gated(
        "hw_vid_probe_task",
        0,
        intel_media_engine_gate,
        &HW_VID_PROBE_STARTED,
        spawn_hw_vid_probe_task,
    ),
    TaskSpec::enabled_gated(
        "hw_logo_present_task",
        0,
        intel_media_engine_gate,
        &HW_LOGO_PRESENT_TASK_STARTED,
        spawn_hw_logo_present_task,
    ),
    TaskSpec::enabled_gated(
        "i226-diagnostic-display",
        0,
        i226_diagnostic_display_gate,
        &I226_DIAGNOSTIC_DISPLAY_STARTED,
        spawn_i226_diagnostic_display,
    ),
    TaskSpec::enabled_gated(
        "virtio-gpu-ui",
        0,
        virtio_gpu_ui_gate,
        &VIRTIO_GPU_UI_STARTED,
        spawn_virtio_gpu_ui_task,
    ),
    TaskSpec::disabled(
        "intel-hda-audio-demo",
        0,
        &INTEL_HDA_AUDIO_DEMO_STARTED,
        spawn_intel_hda_audio_demo_task,
    ),
    TaskSpec::enabled("raple-service", 0, &RAPLE_SERVICE_STARTED, spawn_raple_service),
    TaskSpec::enabled(
        "html_fetch_service",
        crate::r::readiness::NET_V4_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
        &HTML_SHACK_SERVICE_STARTED,
        html_fetch_service,
    ),
    TaskSpec::enabled(
        "asset_shack_service",
        crate::r::readiness::NET_V4_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
        &ASSET_SHACK_SERVICE_STARTED,
        asset_fetch_service,
    ),
    TaskSpec::enabled(
        "truesurfer-parse-pool",
        0,
        &SURFER_PARSE_POOL_STARTED,
        spawn_truesurfer_parse_pool,
    ),
    TaskSpec::enabled_gated(
        "ui3-service",
        crate::r::readiness::UI3_INTEL_PRESENT_READY,
        intel_full_ui3_gate,
        &UI3_SERVICE_STARTED,
        spawn_ui3_service,
    ),
    TaskSpec::enabled_gated(
        "ui3-orbits",
        crate::r::readiness::UI3_INTEL_PRESENT_READY,
        intel_full_ui3_gate,
        &UI3_ORBITS_STARTED,
        spawn_ui3_orbits,
    ),
    TaskSpec::enabled(
        "aud-file-service",
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED | crate::r::readiness::INTEL_HDA_READY,
        &AUD_FILE_SERVICE_STARTED,
        spawn_aud_file_service,
    ),
    TaskSpec::enabled(
        "tinyaudio_service",
        crate::r::readiness::INTEL_HDA_READY,
        &TINYAUDIO_SERVICE_STARTED,
        spawn_tinyaudio_service,
    ),
    TaskSpec::enabled(
        "tinyaudio-live-http",
        crate::r::readiness::NET_ANY_CONFIGURED | crate::r::readiness::INTEL_HDA_READY,
        &TINYAUDIO_LIVE_HTTP_STARTED,
        spawn_tinyaudio_live_http,
    ),
    TaskSpec::disabled(
        "trueosfs-ready-hook",
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
        &TRUEOSFS_READY_HOOK_STARTED,
        spawn_trueosfs_ready_hook,
    ),
    TaskSpec::enabled("uart-shell", 0, &UART_SHELL_STARTED, spawn_uart_shell),
    TaskSpec::enabled("net-tcp-shell", 0, &NET_TCP_SHELL_STARTED, spawn_net_tcp_shell),
    TaskSpec::enabled_gated(
        "ui3-shell",
        crate::r::readiness::UI3_INTEL_PRESENT_READY,
        intel_full_ui3_gate,
        &UI3_SHELL_STARTED,
        spawn_ui3_shell,
    ),
    TaskSpec::disabled("atomic_bomb", 0, &ATOMIC_BOMB_STARTED, spawn_atomic_bomb),
];

pub fn task_index_by_name(name: &str) -> Option<usize> {
    TASKS.iter().position(|spec| spec.name == name)
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
                    if spec.name == "bp-autostart" {}
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
                            crate::log_info!(
                                target: "service";
                                "boot-probe: spawn {} ms={}\n",
                                spec.name,
                                boot_probe_ms()
                            );
                        }
                    }
                    SpawnAttempt::Skipped => {
                        spec.started.store(false, Ordering::Release);
                        pending += 1;
                    }
                    SpawnAttempt::Failed(e) => {
                        spec.started.store(false, Ordering::Release);
                        pending += 1;
                        crate::log_warn!(target: "service";
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
