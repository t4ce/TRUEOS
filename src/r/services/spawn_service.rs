use alloc::string::String;
use core::{
    fmt::Write,
    sync::atomic::{AtomicBool, Ordering},
};

use spin::Mutex;
use trueos_executor::{SpawnError, SpawnToken, Spawner};
use trueos_time::{Duration as EmbassyDuration, Timer};

use crate::r::spawn_spec::{SpawnAttempt, TaskSpec};
// NOTE: This file is intended to become the single source of truth for task startup.

const SPAWN_SERVICE_AFTER_START_MS: u64 = 25;
const SPAWN_SERVICE_PENDING_MS: u64 = 150;
const SPAWN_SERVICE_IDLE_MS: u64 = 250;
const SYSTEM_SERVICE_SNAPSHOT_PERIOD_MS: u64 = 1_000;

static SYSTEM_SERVICE_SNAPSHOT: Mutex<String> = Mutex::new(String::new());

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
    CODEX_PROGRESS_STARTED,
    BLUEPRINT_ASYNC_FS_SERVICE_STARTED,
    TRUEOSFS_REQUEST_BROKER_STARTED,
    DNS_REQUEST_BROKER_STARTED,
    BLOCKING_JOB_DISPATCHER_STARTED,
    MICROFONT_WARM_STARTED,
    SMP_HLT_HISTORY_STARTED,
    RAM_USAGE_HISTORY_STARTED,
    CODEC_SERVICE_STARTED,
    VMEDIA_SERVICE_STARTED,
    TRUEOSFS_MOUNT_SERVICE_STARTED,
    TRUEOSFS_INDEX_SERVICE_STARTED,
    NET_POLL_STARTED,
    NET_SERVICE_STARTED,
    WIFI_SERVICE_STARTED,
    TLS_SOCKET_SERVICE_STARTED,
    CLOCK_SIGNALS_STARTED,
    NTP_SYNC_STARTED,
    SNTP_SERVICE_STARTED,
    SH3SRV_STARTED,
    SHELL3_TCP_STARTED,
    HID_UDP_SRV_STARTED,
    HTTP_TRUEOSFS_STARTED,
    WS_TIME_STARTED,
    LAN_DISCOVERY_STARTED,
    MIDI_PIANO_UDP_STARTED,
    PRINTER_DISCOVERY_STARTED,
    PRINTER_SPOOLER_STARTED,
    GPU_COMPLETION_REAPER_STARTED,
    GPU_FAULT_CONTAINMENT_STARTED,
    TRUEOS_SPIRIT_STARTED,
    MOUSE_MOTION_SERVICE_STARTED,
    KEYBOARD_CONTROL_SERVICE_STARTED,
    GAMEPAD_CONTROL_SERVICE_STARTED,
    UI4_INPUT_SERVICE_STARTED,
    UI4_SLOT4_SERVICE_STARTED,
    UI4_START_BUTTON_SERVICE_STARTED,
    UI4_SCREENSHOT_SERVICE_STARTED,
    UI4_H264_ENCODE_STREAM_STARTED,
    UI4_COMPOSITOR_STARTED,
    UI4_COLOR_PICKER_STARTED,
    UI4_EDIT_LIFE_STARTED,
    UI4_LINK_SERVICE_STARTED,
    UI4_CODEX_PROMPT_SERVICE_STARTED,
    UI4_WINDOW_BROKER_SNAPSHOT_STARTED,
    UI4_VIDEO_CONVERSION_STARTED,
    GPGPU_UI4_PREVIEW_CONSUMER_STARTED,
    GPGPU_UI4_SVG_PROBE_CONSUMER_STARTED,
    HW_PIC_SERVICE_STARTED,
    FALLBACK_LOGO_UI_STARTED,
    INTEL_HDA_AUDIO_DEMO_STARTED,
    GNA_AUDIO_FRONTEND_SERVICE_STARTED,
    RAPLE_SERVICE_STARTED,
    THERMAL_SERVICE_STARTED,
    HTML_SHACK_SERVICE_STARTED,
    USB_CONTROLLER_TASKS_STARTED,
    USER_INPUT_RECORD_WRITER_STARTED,
    BP_AUTOSTART_STARTED,
    FACTORY_RAM_PROBE_STARTED,
    LOGTOTCP_STARTED,
    TINYAUDIO_SERVICE_STARTED,
    TINYAUDIO_LIVE_HTTP_STARTED
);

#[cfg(feature = "trueos_ttstt")]
define_started_flags!(TTSTT_CPU_SERVICE_STARTED, TTSTT_CAPTURE_WRITER_STARTED);

#[cfg(feature = "legacy_vm_checkpoint")]
define_started_flags!(HV_VM_STORE_STARTED, HV_VM_STORE_NET_STARTED);

macro_rules! define_stop_flags {
    ($($name:ident),* $(,)?) => {
        $(static $name: AtomicBool = AtomicBool::new(false);)*
    };
}

define_stop_flags!(
    STOP_UI_TEXT_INPUT_DEMO,
    STOP_UI_TEXT_AREA_DEMO,
    STOP_UI_ANALOG_CLOCK_DEMO,
    STOP_UI_BGRT_DEMO,
    STOP_UI_CORETICKS_DEMO,
    STOP_UI_CURSORPICKER_DEMO,
    STOP_UI_MANDELBROT_DEMO,
    STOP_UI_PLAYER_DEMO,
    STOP_UI_RAPLE_DEMO,
    STOP_UI_SMILEY_FOUNTAIN_DEMO,
    STOP_UI_SHELL_DEMO,
    STOP_UI_SWARM_DEMO,
);

fn stop_flag_by_task_name(name: &str) -> Option<&'static AtomicBool> {
    match name {
        "ui-text-input-demo" => Some(&STOP_UI_TEXT_INPUT_DEMO),
        "ui-text-area-demo" => Some(&STOP_UI_TEXT_AREA_DEMO),
        "ui-analog-clock-demo" => Some(&STOP_UI_ANALOG_CLOCK_DEMO),
        "ui-bgrt-demo" => Some(&STOP_UI_BGRT_DEMO),
        "ui-coreticks-demo" => Some(&STOP_UI_CORETICKS_DEMO),
        "ui-cursorpicker-demo" => Some(&STOP_UI_CURSORPICKER_DEMO),
        "ui-mandelbrot-demo" => Some(&STOP_UI_MANDELBROT_DEMO),
        "ui-player-demo" => Some(&STOP_UI_PLAYER_DEMO),
        "ui-raple-demo" => Some(&STOP_UI_RAPLE_DEMO),
        "ui-smiley-fountain-demo" => Some(&STOP_UI_SMILEY_FOUNTAIN_DEMO),
        "ui-shell-demo" => Some(&STOP_UI_SHELL_DEMO),
        "ui-swarm-demo" => Some(&STOP_UI_SWARM_DEMO),
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
fn spawn_on_ui_core<S: Send>(
    spawner: Spawner,
    task: impl FnOnce(crate::workers::WorkerSpawner) -> Result<SpawnToken<S>, SpawnError>,
) -> SpawnAttempt {
    let _ = spawner;
    let Some(ap1_spawner) = crate::workers::ui_service_spawner() else {
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
fn spawn_on_eff_worker<S: Send>(
    spawner: Spawner,
    task: impl FnOnce(crate::workers::WorkerSpawner) -> Result<SpawnToken<S>, SpawnError>,
) -> SpawnAttempt {
    let worker = crate::workers::pick_eff_background_spawner_with_slot()
        .or_else(crate::workers::pick_background_spawner_with_slot);
    let Some((_slot, _kind, worker_spawner)) = worker else {
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

fn spawn_blueprint_async_fs_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::io::async_fs_cabi::service_task())
}

fn spawn_trueosfs_request_broker(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::fs::request_broker::service_task())
}

fn spawn_dns_request_broker(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::dns_request_broker::service_task())
}

fn spawn_blocking_service_lanes(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::blocking::blocking_job_dispatcher_task())
}

// Wait for the boot topology, then retain a shared terminal executor even
// when there is no AP2 background carrier.
fn shell3_pool_gate() -> bool {
    crate::workers::all_topology_spawners_registered()
        && (crate::workers::has_background_worker_slot()
            || crate::workers::ui_service_spawner().is_some())
}

fn spawn_microfont_warm(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_| super::shell3_font_service::microfont_warm_task())
}

fn microfont_warm_gate() -> bool {
    crate::virtio_gpu_logo::output_dimensions().is_some()
        || (crate::intel::guc_submission_ready() && crate::intel::render::warm_state().is_some())
}

#[cfg(feature = "trueos_ttstt")]
fn spawn_ttstt_cpu_service(spawner: Spawner) -> SpawnAttempt {
    match crate::ai::ttstt_service::ensure_service_started(spawner) {
        Ok(_) => SpawnAttempt::Spawned,
        Err(error) => SpawnAttempt::Failed(error),
    }
}

#[cfg(feature = "trueos_ttstt")]
fn spawn_ttstt_capture_writer(spawner: Spawner) -> SpawnAttempt {
    // TRUEOSFS futures are intentionally local to the BSP executor.
    spawn_local(spawner, |_spawner| crate::ai::ttstt_capture::writer_task())
}

fn spawn_smp_hlt_history(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::smp::hlt_history_sampler_task())
}

fn spawn_ram_usage_history(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::ram_usage::history_sampler_task())
}

fn spawn_codec_service(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    if !crate::workers::all_topology_spawners_registered() {
        return SpawnAttempt::Skipped;
    }
    let worker_spawners =
        crate::workers::pick_background_spawners_with_slots(crate::r::codec::CODEC_WORKER_CAP);
    if worker_spawners.is_empty() {
        return SpawnAttempt::Skipped;
    }

    let mut spawned = 0usize;
    for (worker_id, (worker_slot, core_kind, worker_spawner)) in
        worker_spawners.into_iter().enumerate()
    {
        match crate::r::codec::codec_worker_task(worker_id, worker_slot, core_kind) {
            Ok(token) => {
                worker_spawner.spawn(token);
                spawned = spawned.saturating_add(1);
                crate::log_info!(
                    target: "service";
                    "codec: worker={} scheduled worker_slot={} core_kind={} policy=background-pcore-preferred\n",
                    worker_id,
                    worker_slot,
                    core_kind
                );
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

fn spawn_vmedia_service(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    if !crate::workers::all_topology_spawners_registered() {
        return SpawnAttempt::Skipped;
    }
    let worker_spawners = crate::workers::pick_background_spawners_with_slots(4);
    if worker_spawners.is_empty() {
        return SpawnAttempt::Skipped;
    }

    // Three independent playback futures even on a one-worker topology.
    // Each yields cooperatively; worker/core count must not reduce stream slots.
    for (_, _, worker_spawner) in worker_spawners.iter().cycle().take(3) {
        match crate::r::services::video_service::worker_task() {
            Ok(token) => worker_spawner.spawn(token),
            Err(error) => return SpawnAttempt::Failed(error),
        }
    }
    let mut spawned = 0usize;
    for (worker_id, (worker_slot, core_kind, worker_spawner)) in
        worker_spawners.into_iter().enumerate()
    {
        match crate::r::services::media_service::worker_task(worker_id, worker_slot, core_kind) {
            Ok(token) => {
                worker_spawner.spawn(token);
                spawned = spawned.saturating_add(1);
                crate::log_info!(
                    target: "service";
                    "vmedia: worker={} scheduled worker_slot={} core_kind={} policy=background-pcore-preferred\n",
                    worker_id,
                    worker_slot,
                    core_kind,
                );
            }
            Err(error) if spawned == 0 => return SpawnAttempt::Failed(error),
            Err(error) => {
                crate::log_warn!(
                    target: "service";
                    "vmedia: worker={} spawn failed err={:?}\n",
                    worker_id,
                    error,
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

fn spawn_trueosfs_mount_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::fs::trueosfs::mount_service_task())
}

fn spawn_trueosfs_index_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::fs::trueosfs::index_service_task())
}

#[cfg(feature = "legacy_vm_checkpoint")]
fn spawn_hv_vm_store(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::hv::store::vm_store_task())
}

#[cfg(feature = "legacy_vm_checkpoint")]
fn spawn_hv_vm_store_net(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::hv::store::vm_store_replication_task())
}

fn wifi_service_gate() -> bool {
    crate::net::wifi::has_active_driver()
}

fn spawn_wifi_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| crate::net::wifi::wifi_service_task(spawner))
}

fn spawn_net_poll_tasks(spawner: Spawner) -> SpawnAttempt {
    // Some drivers may fail to report a MAC early; treat any detected NIC as usable.
    let count = crate::net::device_count();
    if count == 0 {
        return SpawnAttempt::Skipped;
    }
    for idx in 0..count {
        if crate::net::wifi::owns_network_interface(idx) { continue; }
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
        if crate::net::wifi::owns_network_interface(idx) { continue; }
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

fn spawn_tls_socket_service(spawner: Spawner) -> SpawnAttempt {
    spawn_on_eff_worker(spawner, |_worker_spawner| {
        crate::net::tls_socket::tls_socket_service_task()
    })
}

fn spawn_clock_signals(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::chronos::signals::service_task())
}

fn spawn_ntp_sync(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::ntp::ntp_sync_task())
}

fn spawn_sntp_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::sntp::sntp_service_task())
}

fn spawn_sh3srv(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    spawn_bool_result_to_attempt(crate::shell3::service::start_pool().map(|count| count != 0))
}

fn spawn_shell3_tcp(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::shell3::net::terminal_task())
}

fn spawn_hid_udp_srv(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::services::hid_udp_service::hid_udp_srv_task())
}

fn codex_progress_gate() -> bool {
    crate::allcaps::hv::CODEX_PROGRESS_PROBES_ENABLED
}

fn spawn_codex_progress(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_| super::codex_progress_service::codex_progress_task())
}

fn ui4_slot4_service_gate() -> bool {
    ui4_compositor_gate()
        && !super::microfont_log_service::owns_plane(0, crate::ui4::INTERACTION_OVERLAY_PLANE_SLOT)
}

fn spawn_logtotcp(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::log_os::logtotcp::logtotcp_task())
}

fn spawn_http_trueosfs(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::fs::http_trueosfs::http_trueosfs_task())
}

fn spawn_ws_time(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::cli::ws_time::ws_time_task())
}

fn spawn_lan_discovery(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::discovery::lan_discovery_task())
}

fn spawn_printer_discovery(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::printer::printer_discovery_task())
}

fn spawn_printer_spooler(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::printer::printer_spooler_task())
}

fn spawn_midi_piano_udp(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::midi_udp::midi_piano_udp_task())
}

fn spawn_gpu_completion_reaper(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::intel::gpgpu::gpu_completion_reaper_task())
}

fn spawn_gpu_fault_containment(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::gpu::vgpu::gpu_fault_containment_task())
}

fn spawn_trueos_spirit_workers(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    let Some(ap1_spawner) = crate::workers::ui_service_spawner() else {
        return SpawnAttempt::Skipped;
    };

    let mut spawned_combos = 0usize;
    for fence in 0..crate::spirit::SPIRIT_WORKER_POOL_LIMIT {
        let frame_token = match crate::spirit::spirit_worker_task(fence as u8) {
            Ok(token) => token,
            Err(error) if spawned_combos == 0 => return SpawnAttempt::Failed(error),
            Err(error) => {
                crate::log_warn!(
                    target: "service";
                    "trueos-spirit: frame worker spawn failed fence={} error={:?}\n",
                    fence,
                    error,
                );
                continue;
            }
        };
        let cursor_token = match crate::spirit::spirit_cursor_task(fence as u8) {
            Ok(token) => token,
            Err(error) if spawned_combos == 0 => return SpawnAttempt::Failed(error),
            Err(error) => {
                crate::log_warn!(
                    target: "service";
                    "trueos-spirit: cursor worker spawn failed fence={} error={:?}\n",
                    fence,
                    error,
                );
                continue;
            }
        };
        ap1_spawner.spawn(frame_token);
        ap1_spawner.spawn(cursor_token);
        spawned_combos = spawned_combos.saturating_add(1);
    }

    if spawned_combos == 0 {
        SpawnAttempt::Skipped
    } else {
        match crate::spirit::spirit_window_selection_task() {
            Ok(token) => ap1_spawner.spawn(token),
            Err(error) => {
                crate::log_warn!(
                    target: "service";
                    "trueos-spirit: window selection task spawn failed error={:?} action=retain-frame-and-cursor-workers\n",
                    error,
                );
            }
        }
        match crate::spirit::spirit_thoughts::spirit_thoughts_task() {
            Ok(token) => ap1_spawner.spawn(token),
            Err(error) => {
                crate::log_warn!(
                    target: "service";
                    "trueos-spirit: agent marker task spawn failed error={:?}\n",
                    error,
                );
            }
        }
        SpawnAttempt::Spawned
    }
}

fn spawn_mouse_motion_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ui_core(spawner, |_ap1_spawner| {
        crate::r::services::mouse_motion_service::mouse_motion_service_task()
    })
}

fn spawn_keyboard_control_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ui_core(spawner, |_ap1_spawner| {
        crate::r::services::keyboard_control_service::keyboard_control_service_task()
    })
}

fn spawn_gamepad_control_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ui_core(spawner, |_ap1_spawner| {
        crate::r::services::gamepad_control_service::gamepad_control_service_task()
    })
}

fn spawn_ui4_input_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ui_core(spawner, |ap1_spawner| crate::ui4::ui4_input_service_task(ap1_spawner))
}

fn spawn_ui4_slot4_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ui_core(spawner, |_ap1_spawner| crate::ui4::ui4_slot4_service_task())
}

fn spawn_ui4_start_button_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ui_core(spawner, |_ap1_spawner| crate::ui4::ui4_start_button_service_task())
}

fn spawn_ui4_screenshot_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |_worker_spawner| crate::ui4::ui4_screenshot_service_task())
}

#[cfg(feature = "trueos_h264_encode_stream")]
fn spawn_ui4_h264_encode_stream_task(spawner: Spawner) -> SpawnAttempt {
    // The spawn service runs on BSP. Capture and encode are cooperative
    // tasks with a one-slot hardware ownership boundary, not private threads.
    let prepare_token = match crate::ui4::ui4_h264_encode_prepare_task() {
        Ok(token) => token,
        Err(error) => return SpawnAttempt::Failed(error),
    };
    let encode_token = match crate::ui4::ui4_h264_encode_stream_task() {
        Ok(token) => token,
        Err(error) => return SpawnAttempt::Failed(error),
    };
    let egress_token = match crate::ui4::ui4_h264_encode_udp_egress_task() {
        Ok(token) => token,
        Err(error) => return SpawnAttempt::Failed(error),
    };
    spawner.spawn(prepare_token);
    spawner.spawn(encode_token);
    spawner.spawn(egress_token);
    crate::log_info!(target: "service";
        "ui4 h264 stream pipeline assigned carrier=bsp-cooperative tasks=3 capture_encode_buffering=serialized handoff=bounded-au-queue overflow=abort-session-next-idr last_ap_reserved=0\n"
    );
    SpawnAttempt::Spawned
}

fn spawn_ui4_compositor_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ui_core(spawner, |_ap1_spawner| crate::ui4::ui4_compositor_service_task())
}

fn spawn_ui4_color_picker_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ui_core(spawner, |_ap1_spawner| crate::ui4::ui4_color_picker_service_task())
}

fn spawn_ui4_edit_life_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |_worker_spawner| crate::ui4::ui4_edit_life_service_task())
}

/// LINK owns user configuration state and remains on BSP with the kernel HID
/// combo registry. It deliberately does not claim a UI-core worker.
fn spawn_ui4_link_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::ui4::ui4_link_service_task())
}

fn spawn_ui4_codex_prompt_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::ui4::ui4_codex_prompt_service_task())
}

fn spawn_ui4_window_broker_snapshot_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::ui4::ui4_window_broker_snapshot_service_task())
}

fn spawn_ui4_video_conversion_service_task(spawner: Spawner) -> SpawnAttempt {
    let Some(worker_spawner) = crate::workers::pick_background_spawner() else {
        let _ = spawner;
        return SpawnAttempt::Skipped;
    };
    let _ = spawner;
    let mut spawned = 0usize;
    for lane in 0..2u8 {
        match crate::ui4::ui4_video_conversion_service_task(worker_spawner.cpu_slot(), lane) {
            Ok(token) => {
                worker_spawner.spawn(token);
                spawned = spawned.saturating_add(1);
            }
            Err(error) if spawned == 0 => return SpawnAttempt::Failed(error),
            Err(error) => crate::log_warn!(
                target: "service";
                "ui4 video-conversion lane spawn failed lane={} assigned_slot={} error={:?}\n",
                lane,
                worker_spawner.cpu_slot(),
                error,
            ),
        }
    }
    if spawned == 0 {
        SpawnAttempt::Skipped
    } else {
        SpawnAttempt::Spawned
    }
}

fn spawn_gpgpu_ui4_preview_consumer_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |worker_spawner| {
        crate::ui4::gpgpu_preview_consumer_service_task(worker_spawner.cpu_slot())
    })
}

fn spawn_gpgpu_ui4_svg_probe_consumer_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |worker_spawner| {
        crate::ui4::gpgpu_svg_probe_consumer_service_task(worker_spawner.cpu_slot())
    })
}

fn spawn_hw_pic_service(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ui_core(spawner, |_ap1_spawner| crate::intel::hw_pic_service())
}

fn spawn_fallback_logo_ui_task(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::virtio_gpu_logo::fallback_logo_task())
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

fn spawn_thermal_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::power::thermal::thermal_service())
}

fn html_fetch_service(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    spawn_bool_result_to_attempt(crate::surfer::spawn_html_fetch_service())
}

fn spawn_gna_audio_frontend_service(spawner: Spawner) -> SpawnAttempt {
    spawn_on_eff_worker(spawner, |_worker_spawner| {
        crate::r::services::gna_audio_frontend_service::gna_audio_frontend_service_task()
    })
}

fn spawn_tinyaudio_service(spawner: Spawner) -> SpawnAttempt {
    spawn_on_eff_worker(spawner, |_worker_spawner| crate::aud::esynth::tinyaudio_service_task())
}

fn spawn_tinyaudio_live_http(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::aud::audio_live_http::tinyaudio_live_http_task())
}

#[inline]
fn intel_device_gate() -> bool {
    crate::intel::has_claimed_device()
}

#[inline]
fn gpu_fault_containment_gate() -> bool {
    crate::intel::guc_submission_ready() && crate::gpu::physical::physical_device().is_some()
}

#[inline]
fn trueos_spirit_gate() -> bool {
    crate::spirit::hardware_ready()
        && crate::intel::complete_scanout_pipeline_slot().is_some()
        && crate::workers::ui_service_spawner().is_some()
}

#[inline]
fn ui4_compositor_gate() -> bool {
    crate::intel::has_claimed_device()
        && crate::intel::active_scanout_dimensions().is_some()
        && crate::intel::ui4_rgba8_plane_stack_is_ready()
        && crate::workers::ui_service_spawner().is_some()
}

#[inline]
fn ui4_input_gate() -> bool {
    ui4_compositor_gate()
        || (crate::virtio_gpu_logo::output_dimensions().is_some()
            && crate::workers::ui_service_spawner().is_some())
}

#[inline]
fn ui_core_ready_gate() -> bool {
    crate::workers::ui_service_spawner().is_some()
}

#[inline]
fn intel_media_engine_gate() -> bool {
    crate::intel::has_media_decode_engine()
}

#[cfg(feature = "trueos_h264_encode_stream")]
#[inline]
fn h264_stream_service_gate() -> bool {
    intel_media_engine_gate()
}

#[inline]
fn fallback_logo_ui_gate() -> bool {
    crate::virtio_gpu_logo::fallback_logo_available()
}

#[inline]
fn http_trueosfs_gate() -> bool {
    // Heal must be reachable when USB storage has not enumerated. The HTTP
    // handlers already report missing/unready roots without touching a disk.
    crate::r::readiness::is_set(crate::r::readiness::NET_ANY_CONFIGURED)
}

#[inline]
fn html_shack_gate() -> bool {
    crate::disc::install::pxeproc::cold_boot_enabled()
        || crate::r::readiness::is_set(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED)
}

#[inline]
#[cfg(feature = "trueos_ttstt")]
fn ttstt_cpu_service_gate() -> bool {
    crate::r::readiness::is_set(
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED | crate::r::readiness::TRUEOSFS_INDEX_READY,
    )
}

#[inline]
fn user_input_writer_gate() -> bool {
    crate::r::readiness::is_set(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED)
}

fn spawn_usb_controller_tasks(spawner: Spawner) -> SpawnAttempt {
    // The controller task owns the xHCI event pump that completes every USB
    // transfer. Keep it on the cooperative BSP executor with the filesystem
    // broker: background AP executors also host genuinely blocking Tokio
    // service-lane jobs, and one of those must never starve USB completions.
    spawn_local(spawner, |_spawner| crate::usb2::usb_controller_service_task())
}

fn spawn_user_input_record_writer(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::user_input_record::writer_task())
}

fn spawn_bp_autostart(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| crate::r::restart::restart_apps_task(spawner))
}

// --- registry ---

const BP_AUTOSTART_READY: u32 =
    crate::r::readiness::BACKGROUND_AP_WORKER_READY | crate::r::readiness::VTHREAD_HW_TAG_READY;
// Own the AtomicBool fields in a static array; an interior-mutable temporary
// cannot be promoted behind a shared slice reference.
const TASK_COUNT: usize = 62
    + cfg!(feature = "trueos_h264_encode_stream") as usize
    + 2 * cfg!(feature = "trueos_ttstt") as usize
    + 2 * cfg!(feature = "legacy_vm_checkpoint") as usize;
static TASKS: [TaskSpec; TASK_COUNT] = [
    TaskSpec::enabled_gated(
        "codex-progress-observer",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        codex_progress_gate,
        &CODEX_PROGRESS_STARTED,
        spawn_codex_progress,
    ),
    
    TaskSpec::enabled("job-runner", 0, &JOB_RUNNER_STARTED, spawn_job_runner),
    TaskSpec::enabled(
        "blueprint-async-fs-service",
        0,
        &BLUEPRINT_ASYNC_FS_SERVICE_STARTED,
        spawn_blueprint_async_fs_service,
    ),
    // BSP half of the synchronous-kfs compatibility bridge. It must remain a
    // local BSP task because TRUEOSFS/block futures are intentionally non-Send.
    TaskSpec::enabled(
        "trueosfs-request-broker",
        0,
        &TRUEOSFS_REQUEST_BROKER_STARTED,
        spawn_trueosfs_request_broker,
    ),
    // BSP half of the synchronous Blueprint/guest DNS ABI. Callers park on
    // background carrier lanes; only this task polls the network future.
    TaskSpec::enabled(
        "dns-request-broker",
        0,
        &DNS_REQUEST_BROKER_STARTED,
        spawn_dns_request_broker,
    ),
    TaskSpec::enabled(
        "blocking-service-lanes",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        &BLOCKING_JOB_DISPATCHER_STARTED,
        spawn_blocking_service_lanes,
    ),
    TaskSpec::enabled_gated(
        "microfont-warm",
        crate::r::readiness::UI4_COMPOSITOR_READY | crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        microfont_warm_gate,
        &MICROFONT_WARM_STARTED,
        spawn_microfont_warm,
    ),
    #[cfg(feature = "trueos_ttstt")]
    TaskSpec::configured_gated(
        crate::allcaps::ttstt::BOOT_RESIDENT_WARM_ENABLED,
        "ttstt-cpu-service",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        ttstt_cpu_service_gate,
        &TTSTT_CPU_SERVICE_STARTED,
        spawn_ttstt_cpu_service,
    ),
    #[cfg(feature = "trueos_ttstt")]
    TaskSpec::enabled(
        "ttstt-capture-writer",
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
        &TTSTT_CAPTURE_WRITER_STARTED,
        spawn_ttstt_capture_writer,
    ),
    TaskSpec::enabled("smp-hlt-history", 0, &SMP_HLT_HISTORY_STARTED, spawn_smp_hlt_history),
    TaskSpec::enabled("ram-usage-history", 0, &RAM_USAGE_HISTORY_STARTED, spawn_ram_usage_history),

    TaskSpec::enabled(
        "codec-service",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        &CODEC_SERVICE_STARTED,
        spawn_codec_service,
    ),
    TaskSpec::enabled(
        "vmedia-service",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        &VMEDIA_SERVICE_STARTED,
        spawn_vmedia_service,
    ),
    TaskSpec::enabled("factory-ram-probe", 0, &FACTORY_RAM_PROBE_STARTED, spawn_factory_ram_probe),
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
    #[cfg(feature = "legacy_vm_checkpoint")]
    TaskSpec::enabled("hv-vm-store", 0, &HV_VM_STORE_STARTED, spawn_hv_vm_store),
    #[cfg(feature = "legacy_vm_checkpoint")]
    TaskSpec::enabled(
        "hv-vm-store-net",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &HV_VM_STORE_NET_STARTED,
        spawn_hv_vm_store_net,
    ),
    TaskSpec::enabled_gated("wifi-service", 0, wifi_service_gate, &WIFI_SERVICE_STARTED, spawn_wifi_service),
    TaskSpec::enabled("net-poll-tasks", 0, &NET_POLL_STARTED, spawn_net_poll_tasks),
    TaskSpec::enabled("net-service", 0, &NET_SERVICE_STARTED, spawn_net_service),

    TaskSpec::enabled(
        "tls-socket-service",
        crate::r::readiness::NET_ANY_CONFIGURED | crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        &TLS_SOCKET_SERVICE_STARTED,
        spawn_tls_socket_service,
    ),
    TaskSpec::enabled("clock-signals", 0, &CLOCK_SIGNALS_STARTED, spawn_clock_signals),
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
    TaskSpec::enabled(
        "shell3-tcp",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &SHELL3_TCP_STARTED,
        spawn_shell3_tcp,
    ),
    TaskSpec::enabled_gated(
        "sh3srv",
        0,
        shell3_pool_gate,
        &SH3SRV_STARTED,
        spawn_sh3srv,
    ),
    TaskSpec::enabled(
        "hid-udp-srv",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &HID_UDP_SRV_STARTED,
        spawn_hid_udp_srv,
    ),
    TaskSpec::enabled(
        "logtotcp",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &LOGTOTCP_STARTED,
        spawn_logtotcp,
    ),
    TaskSpec::enabled_gated(
        "http-trueosfs",
        crate::r::readiness::NET_ANY_CONFIGURED,
        http_trueosfs_gate,
        &HTTP_TRUEOSFS_STARTED,
        spawn_http_trueosfs,
    ),

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
    TaskSpec::enabled(
        "midi-piano-udp",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &MIDI_PIANO_UDP_STARTED,
        spawn_midi_piano_udp,
    ),
    TaskSpec::enabled(
        "printer-discovery",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &PRINTER_DISCOVERY_STARTED,
        spawn_printer_discovery,
    ),
    TaskSpec::enabled(
        "printer-spooler",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &PRINTER_SPOOLER_STARTED,
        spawn_printer_spooler,
    ),

    TaskSpec::enabled_gated(
        "gpu-completion-reaper",
        0,
        intel_device_gate,
        &GPU_COMPLETION_REAPER_STARTED,
        spawn_gpu_completion_reaper,
    ),
    TaskSpec::enabled_gated(
        "gpu-fault-containment",
        0,
        gpu_fault_containment_gate,
        &GPU_FAULT_CONTAINMENT_STARTED,
        spawn_gpu_fault_containment,
    ),
    // The arithmetic probe owns a bounded normal-priority execution context
    // only. It never admits a scanout, display-plane, WD0, or BCS operation.

    // TrueOS-Spirit reserves all four cursor-pipe fences, but its sane initial
    // deployment starts one Embassy worker only after a complete scanout
    // route exists, then binds fence N directly to cursor bank N. No input,
    // UI composition, or universal-plane path writes CUR_* registers.
    TaskSpec::enabled_gated(
        "trueos-spirit",
        0,
        trueos_spirit_gate,
        &TRUEOS_SPIRIT_STARTED,
        spawn_trueos_spirit_workers,
    ),
    
    TaskSpec::enabled_gated(
        "mouse-motion-service",
        0,
        ui_core_ready_gate,
        &MOUSE_MOTION_SERVICE_STARTED,
        spawn_mouse_motion_service_task,
    ),
    TaskSpec::enabled_gated(
        "keyboard-control-service",
        0,
        ui_core_ready_gate,
        &KEYBOARD_CONTROL_SERVICE_STARTED,
        spawn_keyboard_control_service_task,
    ),
    TaskSpec::enabled_gated(
        "gamepad-control-service",
        0,
        ui_core_ready_gate,
        &GAMEPAD_CONTROL_SERVICE_STARTED,
        spawn_gamepad_control_service_task,
    ),
    TaskSpec::enabled_gated(
        "ui4-input-service",
        0,
        ui4_input_gate,
        &UI4_INPUT_SERVICE_STARTED,
        spawn_ui4_input_service_task,
    ),
    TaskSpec::enabled_gated(
        "ui4-slot4-service",
        0,
        ui4_slot4_service_gate,
        &UI4_SLOT4_SERVICE_STARTED,
        spawn_ui4_slot4_service_task,
    ),
    TaskSpec::enabled_gated(
        "ui4-start-button",
        0,
        ui4_input_gate,
        &UI4_START_BUTTON_SERVICE_STARTED,
        spawn_ui4_start_button_service_task,
    ),
    TaskSpec::enabled(
        "ui4-screenshot-service",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        &UI4_SCREENSHOT_SERVICE_STARTED,
        spawn_ui4_screenshot_service_task,
    ),
    #[cfg(feature = "trueos_h264_encode_stream")]
    TaskSpec::enabled_gated(
        "ui4-h264-encode-stream",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        h264_stream_service_gate,
        &UI4_H264_ENCODE_STREAM_STARTED,
        spawn_ui4_h264_encode_stream_task,
    ),
    TaskSpec::enabled_gated(
        "ui4-compositor-service",
        0,
        ui4_compositor_gate,
        &UI4_COMPOSITOR_STARTED,
        spawn_ui4_compositor_service_task,
    ),
    TaskSpec::enabled(
        "ui4-color-picker",
        crate::r::readiness::UI4_COMPOSITOR_READY,
        &UI4_COLOR_PICKER_STARTED,
        spawn_ui4_color_picker_service_task,
    ),
    TaskSpec::enabled(
        "ui4-edit-life",
        crate::r::readiness::UI4_COMPOSITOR_READY | crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        &UI4_EDIT_LIFE_STARTED,
        spawn_ui4_edit_life_service_task,
    ),
    TaskSpec::enabled(
        "ui4-link-service",
        crate::r::readiness::UI4_COMPOSITOR_READY,
        &UI4_LINK_SERVICE_STARTED,
        spawn_ui4_link_service_task,
    ),
    TaskSpec::enabled(
        "ui4-codex-prompt-service",
        crate::r::readiness::UI4_COMPOSITOR_READY,
        &UI4_CODEX_PROMPT_SERVICE_STARTED,
        spawn_ui4_codex_prompt_service_task,
    ),

    TaskSpec::enabled_gated(
        "ui4-window-broker-snapshot",
        0,
        ui4_input_gate,
        &UI4_WINDOW_BROKER_SNAPSHOT_STARTED,
        spawn_ui4_window_broker_snapshot_service_task,
    ),
    TaskSpec::enabled_gated(
        "ui4-video-conversion-service",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        ui4_compositor_gate,
        &UI4_VIDEO_CONVERSION_STARTED,
        spawn_ui4_video_conversion_service_task,
    ),
    // Online only exposes the shared C++ presentation controller. No compute
    // work or UI4 frame is created until a `cpp` demo or font presentation is
    // requested.
    TaskSpec::enabled_gated(
        "gpgpu-ui4-preview-consumer-service",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        ui4_compositor_gate,
        &GPGPU_UI4_PREVIEW_CONSUMER_STARTED,
        spawn_gpgpu_ui4_preview_consumer_service_task,
    ),
    // Online only exposes the Shell2 control endpoint. The inline SVG probe
    // is normalized, submitted, and assigned a UI4 frame on explicit start.
    TaskSpec::enabled_gated(
        "gpgpu-ui4-svg-probe-consumer-service",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        ui4_compositor_gate,
        &GPGPU_UI4_SVG_PROBE_CONSUMER_STARTED,
        spawn_gpgpu_ui4_svg_probe_consumer_service_task,
    ),
    TaskSpec::enabled_gated(
        "hw_pic_service",
        0,
        intel_media_engine_gate,
        &HW_PIC_SERVICE_STARTED,
        spawn_hw_pic_service,
    ),
    TaskSpec::enabled_gated(
        "fallback-logo-ui",
        0,
        fallback_logo_ui_gate,
        &FALLBACK_LOGO_UI_STARTED,
        spawn_fallback_logo_ui_task,
    ),
    TaskSpec::disabled(
        "intel-hda-audio-demo",
        0,
        &INTEL_HDA_AUDIO_DEMO_STARTED,
        spawn_intel_hda_audio_demo_task,
    ),
    TaskSpec::enabled("raple-service", 0, &RAPLE_SERVICE_STARTED, spawn_raple_service),
    TaskSpec::enabled("thermal-service", 0, &THERMAL_SERVICE_STARTED, spawn_thermal_service),
    TaskSpec::enabled_gated(
        "html_fetch_service",
        crate::r::readiness::NET_V4_CONFIGURED | crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        html_shack_gate,
        &HTML_SHACK_SERVICE_STARTED,
        html_fetch_service,
    ),
    TaskSpec::enabled(
        "gna-audio-front-end",
        crate::r::readiness::INTEL_HDA_READY | crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        &GNA_AUDIO_FRONTEND_SERVICE_STARTED,
        spawn_gna_audio_frontend_service,
    ),
    TaskSpec::enabled(
        "tinyaudio_service",
        crate::r::readiness::INTEL_HDA_READY | crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        &TINYAUDIO_SERVICE_STARTED,
        spawn_tinyaudio_service,
    ),
    TaskSpec::enabled(
        "tinyaudio-live-http",
        crate::r::readiness::NET_ANY_CONFIGURED | crate::r::readiness::INTEL_HDA_READY,
        &TINYAUDIO_LIVE_HTTP_STARTED,
        spawn_tinyaudio_live_http,
    ),
    TaskSpec::enabled_gated(
        "user-input-record-writer",
        0,
        user_input_writer_gate,
        &USER_INPUT_RECORD_WRITER_STARTED,
        spawn_user_input_record_writer,
    ),

];

pub fn task_index_by_name(name: &str) -> Option<usize> {
    TASKS.iter().position(|spec| spec.name == name)
}

fn task_kind(name: &str) -> &'static str {
    if name.contains("pool") || name.contains("lanes") || name.ends_with("-tasks") {
        "pool"
    } else {
        "service"
    }
}

fn readiness_names(mask: u32) -> String {
    if mask == 0 {
        return String::from("-");
    }

    let mut names = String::new();
    crate::r::readiness::for_each_flag(mask, |_flag, name| {
        if !names.is_empty() {
            names.push('|');
        }
        names.push_str(name);
    });
    if names.is_empty() {
        let _ = write!(names, "0x{mask:08X}");
    }
    names
}

fn format_system_service_snapshot() -> String {
    let ready = crate::r::readiness::mask();
    let mut out = String::new();
    let _ = writeln!(out, "trueos system services snapshot v1");
    let _ = writeln!(out, "generated_at_ms={}", boot_probe_ms());
    let _ = writeln!(out, "readiness_mask=0x{ready:08X}");
    let _ = writeln!(out, "service_count={}", TASKS.len());
    let _ = writeln!(
        out,
        "service\tname\tenabled\tgate_open\tstarted\trequired_mask\tmissing_mask\tkind\trequires"
    );

    for spec in TASKS.iter() {
        let enabled = !spec.disabled.load(Ordering::Acquire);
        let gate_open = (spec.gate)();
        let started = spec.started.load(Ordering::Acquire);
        let missing = spec.required & !ready;
        let _ = writeln!(
            out,
            "service\t{}\t{}\t{}\t{}\t0x{:08X}\t0x{:08X}\t{}\t{}",
            spec.name,
            enabled as u8,
            gate_open as u8,
            started as u8,
            spec.required,
            missing,
            task_kind(spec.name),
            readiness_names(spec.required),
        );
    }
    #[cfg(feature = "trueos_task_profile")]
    crate::executor_task_profile::append_snapshot_history_text(&mut out);
    out
}

fn update_system_service_snapshot() {
    *SYSTEM_SERVICE_SNAPSHOT.lock() = format_system_service_snapshot();
}

/// Latest one-second snapshot of the central task registry for v-layer consumers.
pub fn latest_system_service_snapshot_text() -> String {
    let snapshot = SYSTEM_SERVICE_SNAPSHOT.lock();
    if snapshot.is_empty() {
        drop(snapshot);
        return format_system_service_snapshot();
    }
    snapshot.clone()
}

#[trueos_executor::task]
pub async fn spawn_service_task(spawner: Spawner) {
    async move {
        crate::log_info!(target: "boot";
            "spawn-svc: boot-profile usb_uas_diag={}\n",
            crate::log_os::flags::USB_UAS_DIAG_PROFILE_ENABLED
        );
        let mut next_snapshot_ms = 0u64;
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
                                | "ui"
                                | "ui-gfx-browser"
                                | "ui-mandelbrot-demo"
                                | "ui-shell-demo"
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
            let now_ms = boot_probe_ms();
            if now_ms >= next_snapshot_ms {
                update_system_service_snapshot();
                next_snapshot_ms = now_ms.saturating_add(SYSTEM_SERVICE_SNAPSHOT_PERIOD_MS);
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
