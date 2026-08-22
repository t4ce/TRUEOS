use alloc::{string::String, vec::Vec};
use core::{
    fmt::Write,
    sync::atomic::{AtomicBool, Ordering},
};

use spin::Mutex;
use trueos_executor::{SpawnError, SpawnToken, Spawner};
use trueos_time::{Duration as EmbassyDuration, Timer};

use crate::r::spawn_spec::{SpawnAttempt, TaskSpec};
// NOTE: This file is intended to become the single source of truth for Embassy task startup.

const SPAWN_SERVICE_AFTER_START_MS: u64 = 25;
const SPAWN_SERVICE_PENDING_MS: u64 = 150;
const SPAWN_SERVICE_IDLE_MS: u64 = 250;
const SYSTEM_SERVICE_SNAPSHOT_PERIOD_MS: u64 = 1_000;

static SYSTEM_SERVICE_SNAPSHOT: Mutex<String> = Mutex::new(String::new());
static HELIO_CARRIER_WAKE_FAILURE_LOGGED: AtomicBool = AtomicBool::new(false);

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
    BLUEPRINT_ASYNC_FS_SERVICE_STARTED,
    TRUEOSFS_REQUEST_BROKER_STARTED,
    DNS_REQUEST_BROKER_STARTED,
    BLOCKING_JOB_DISPATCHER_STARTED,
    FONT_WARM_POOL_STARTED,
    FONT_PLAN_SERVICE_STARTED,
    FONT_KERNEL_SERVICE_STARTED,
    TTSTT_CPU_SERVICE_STARTED,
    TTSTT_CAPTURE_WRITER_STARTED,
    LUMEN_BOOT_WARM_STARTED,
    SMP_HLT_HISTORY_STARTED,
    RAM_USAGE_HISTORY_STARTED,
    CODEC_SERVICE_STARTED,
    VMEDIA_SERVICE_STARTED,
    TRUEOSFS_MOUNT_SERVICE_STARTED,
    TRUEOSFS_INDEX_SERVICE_STARTED,
    HV_VM_STORE_STARTED,
    HV_VM_STORE_NET_STARTED,
    NET_POLL_STARTED,
    NET_SERVICE_STARTED,
    NET_CACHE_SERVICE_STARTED,
    NET_THROUGHPUT_BENCH_STARTED,
    TLS_SOCKET_SERVICE_STARTED,
    NTP_SYNC_STARTED,
    SNTP_SERVICE_STARTED,
    NET_SHELL_STARTED,
    LOCAL_SHELL_SESSION_POOL_STARTED,
    HELIO_GAME_STARTED,
    GRIDPAPER_SERVICE_STARTED,
    HID_UDP_SRV_STARTED,
    HTTP_TRUEOSFS_STARTED,
    WS_TIME_STARTED,
    LAN_DISCOVERY_STARTED,
    MIDI_PIANO_UDP_STARTED,
    PRINTER_DISCOVERY_STARTED,
    PRINTER_SPOOLER_STARTED,
    FTP_SERVER_STARTED,
    GPU_COMPLETION_REAPER_STARTED,
    GPU_FAULT_CONTAINMENT_STARTED,
    TRUEOS_SPIRIT_STARTED,
    SPIRIT_RESPONSE_WINDOW_STARTED,
    MOUSE_MOTION_SERVICE_STARTED,
    KEYBOARD_CONTROL_SERVICE_STARTED,
    GAMEPAD_CONTROL_SERVICE_STARTED,
    UI4_INPUT_SERVICE_STARTED,
    UI4_SLOT4_SERVICE_STARTED,
    UI4_SCREENSHOT_SERVICE_STARTED,
    UI4_H264_ENCODE_STREAM_STARTED,
    UI4_COMPOSITOR_STARTED,
    UI4_COLOR_PICKER_STARTED,
    UI4_WINDOW_BROKER_SNAPSHOT_STARTED,
    UI4_VIDEO_CONVERSION_STARTED,
    GPGPU_UI4_PREVIEW_CONSUMER_STARTED,
    GPGPU_UI4_SVG_PROBE_CONSUMER_STARTED,
    HW_PIC_SERVICE_STARTED,
    FALLBACK_LOGO_UI_STARTED,
    INTEL_HDA_AUDIO_DEMO_STARTED,
    RAPLE_SERVICE_STARTED,
    THERMAL_SERVICE_STARTED,
    HTML_SHACK_SERVICE_STARTED,
    USB_CONTROLLER_TASKS_STARTED,
    USER_INPUT_RECORD_WRITER_STARTED,
    TRUEOSFS_RW_PROBE_STARTED,
    BP_AUTOSTART_STARTED,
    WEAVE_HELLO_AUTOSTART_STARTED,
    APP_VM_RUN_QUEUE_STARTED,
    FACTORY_RAM_PROBE_STARTED,
    NET_TCP_SHELL_STARTED,
    LOGTOTCP_STARTED,
    ATOMIC_BOMB_STARTED,
    TINYAUDIO_SERVICE_STARTED,
    TINYAUDIO_LIVE_HTTP_STARTED,
    EXECUTOR_REALM_MIGRATION_SMOKE_STARTED,
    UNIX_FD_PROBE_STARTED
);

macro_rules! define_stop_flags {
    ($($name:ident),* $(,)?) => {
        $(#[allow(dead_code)] static $name: AtomicBool = AtomicBool::new(false);)*
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub struct TaskRunGuard {
    name: &'static str,
}

impl Drop for TaskRunGuard {
    fn drop(&mut self) {
        task_exited(self.name);
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn task_run_guard(name: &'static str) -> TaskRunGuard {
    if let Some(flag) = stop_flag_by_task_name(name) {
        flag.store(false, Ordering::Release);
    }
    TaskRunGuard { name }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn task_stop_requested(name: &str) -> bool {
    stop_flag_by_task_name(name)
        .map(|flag| flag.load(Ordering::Acquire))
        .unwrap_or(false)
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
fn spawn_on_ap1_ui_core<S: Send>(
    spawner: Spawner,
    task: impl FnOnce(crate::workers::WorkerSpawner) -> Result<SpawnToken<S>, SpawnError>,
) -> SpawnAttempt {
    let _ = spawner; // keep signature stable; this task intentionally targets the AP1 UI core.
    let Some(ap1_spawner) = crate::workers::ap1_ui_core_spawner() else {
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

fn font_warm_pool_gate() -> bool {
    crate::workers::all_topology_spawners_registered()
        && crate::workers::has_background_worker_slot()
}

fn spawn_font_warm_pool(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    spawn_bool_result_to_attempt(crate::graphics::font::spawn_font_warm_pool())
}

fn font_plan_pool_gate() -> bool {
    // E-core preference can only be resolved once the complete topology has
    // registered. Starting after the first AP would incorrectly conclude that
    // a later-registering efficiency core does not exist.
    crate::workers::all_topology_spawners_registered()
        && crate::workers::has_background_worker_slot()
}

fn spawn_font_plan_pool(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    spawn_bool_result_to_attempt(crate::r::font_plan_service::start_font_plan_workers())
}

pub(crate) fn retry_font_plan_pool_autostart() {
    FONT_PLAN_SERVICE_STARTED.store(false, Ordering::Release);
}

fn spawn_font_kernel_service(spawner: Spawner) -> SpawnAttempt {
    // This task is only the asynchronous queue controller. CPU font warming
    // and synchronous GPU retirement polling are dispatched through leased
    // blocking-service lanes, so a VM cannot strand the request pump.
    spawn_local(spawner, |_spawner| crate::r::font_kernel_service::font_kernel_service_task())
}

pub(crate) fn retry_font_warm_pool_autostart() {
    FONT_WARM_POOL_STARTED.store(false, Ordering::Release);
}

fn spawn_ttstt_cpu_service(spawner: Spawner) -> SpawnAttempt {
    match crate::r::ttstt_service::ensure_service_started(spawner) {
        Ok(_) => SpawnAttempt::Spawned,
        Err(error) => SpawnAttempt::Failed(error),
    }
}

fn spawn_ttstt_capture_writer(spawner: Spawner) -> SpawnAttempt {
    // TRUEOSFS futures are intentionally local to the BSP executor.
    spawn_local(spawner, |_spawner| crate::r::ttstt_capture::writer_task())
}

#[cfg(feature = "trueos_lumen")]
fn spawn_lumen_boot_warm(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    let Some((worker_slot, core_kind, worker_spawner)) =
        crate::workers::pick_perf_background_spawner_with_slot()
    else {
        return SpawnAttempt::Skipped;
    };
    let token = match crate::r::lfm25_boot_warm::service_task(worker_slot) {
        Ok(token) => token,
        Err(error) => return SpawnAttempt::Failed(error),
    };
    worker_spawner.spawn(token);
    crate::log_info!(
        target: "service";
        "lfm25: boot-warm stage=scheduled executor=background-ap{} core_kind={} core=perf policy_switch=allcaps::lumen::BOOT_RESIDENT_WARM_ENABLED\n",
        worker_slot,
        core_kind,
    );
    SpawnAttempt::Spawned
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
    let worker_spawners = crate::workers::pick_background_spawners_with_slots(3);
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
    let worker_spawners = crate::workers::pick_background_spawners_with_slots(2);
    if worker_spawners.is_empty() {
        return SpawnAttempt::Skipped;
    }

    let mut spawned = 0usize;
    for (worker_id, (worker_slot, core_kind, worker_spawner)) in
        worker_spawners.into_iter().enumerate()
    {
        match crate::r::media_service::worker_task(worker_id, worker_slot, core_kind) {
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

fn spawn_net_throughput_bench(spawner: Spawner) -> SpawnAttempt {
    if !crate::allcaps::net::THROUGHPUT_BENCH_AUTOSTART {
        return SpawnAttempt::Skipped;
    }

    let rx = match crate::r::net::throughput_bench::throughput_rx_task() {
        Ok(token) => token,
        Err(err) => return SpawnAttempt::Failed(err),
    };
    let tx = match crate::r::net::throughput_bench::throughput_tx_task() {
        Ok(token) => token,
        Err(err) => return SpawnAttempt::Failed(err),
    };
    spawner.spawn(rx);
    spawner.spawn(tx);
    SpawnAttempt::Spawned
}

fn spawn_net_cache_service(spawner: Spawner) -> SpawnAttempt {
    spawn_bool_result_to_attempt(crate::net::cache_service::ensure_service_started(spawner))
}

fn spawn_tls_socket_service(spawner: Spawner) -> SpawnAttempt {
    spawn_on_eff_worker(spawner, |_worker_spawner| {
        crate::net::tls_socket::tls_socket_service_task()
    })
}

fn spawn_ntp_sync(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::ntp::ntp_sync_task())
}

fn spawn_sntp_service(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::sntp::sntp_service_task())
}

fn spawn_net_shell(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::shell2::backends::net_tcp_shell::net_shell_task())
}

fn log_helio_carrier_wake_result(failed_mask: u8, carrier_count: u8, phase: &'static str) {
    if failed_mask != 0 {
        if !HELIO_CARRIER_WAKE_FAILURE_LOGGED.swap(true, Ordering::AcqRel) {
            crate::log_warn!(target: "service";
                "helio: remote carrier wake incomplete failed_mask=0x{:02X} carrier_count={} phase={} action=keep-service-pending-and-retry registry=withheld\n",
                failed_mask,
                carrier_count,
                phase,
            );
        }
    } else if HELIO_CARRIER_WAKE_FAILURE_LOGGED.swap(false, Ordering::AcqRel) {
        crate::log_info!(target: "service";
            "helio: remote carrier wake recovered carrier_count={} phase={} action=await-online-barrier registry=withheld-until-all-online\n",
            carrier_count,
            phase,
        );
    }
}

fn spawn_helio_game(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    if !crate::workers::all_topology_spawners_registered() {
        return SpawnAttempt::Skipped;
    }

    // Select the same carrier set on every spawn-service retry: prefer known
    // performance workers, break ties by topology slot, then assign carrier
    // ids in ascending slot order.
    let mut worker_slots = crate::workers::background_worker_slots();
    worker_slots.sort_unstable_by_key(|worker_slot| {
        let core_kind = crate::workers::core_kind_for_slot(*worker_slot);
        let preference = match core_kind {
            crate::workers::CORE_KIND_PERF => 0,
            crate::workers::CORE_KIND_EFF => 1,
            _ => 2,
        };
        (preference, *worker_slot)
    });
    worker_slots.truncate(crate::r::helio_game::CPU_CARRIER_CAPACITY);
    worker_slots.sort_unstable();
    let worker_spawners: Vec<_> = worker_slots
        .into_iter()
        .filter_map(|worker_slot| {
            let core_kind = crate::workers::core_kind_for_slot(worker_slot);
            crate::workers::spawner_for_slot(worker_slot)
                .map(|worker_spawner| (worker_slot, core_kind, worker_spawner))
        })
        .collect();
    if worker_spawners.is_empty() {
        return SpawnAttempt::Skipped;
    }

    let carrier_count = worker_spawners.len() as u8;
    let carrier_metadata: Vec<(u32, u8)> = worker_spawners
        .iter()
        .map(|(worker_slot, core_kind, _)| (*worker_slot, *core_kind))
        .collect();
    let Some(bootstrap_state) = crate::r::helio_game::prepare_cpu_carriers(&carrier_metadata)
    else {
        crate::log_warn!(target: "service";
            "helio: invalid or changed cpu carrier set count={} capacity={} action=keep-service-pending registry=withheld\n",
            carrier_metadata.len(),
            crate::r::helio_game::CPU_CARRIER_CAPACITY,
        );
        return SpawnAttempt::Skipped;
    };

    match bootstrap_state {
        crate::r::helio_game::CpuCarrierBootstrapState::Online => {
            return SpawnAttempt::Spawned;
        }
        crate::r::helio_game::CpuCarrierBootstrapState::Waiting { online_mask } => {
            let mut failed_mask = 0u8;
            for (carrier_id, (worker_slot, _, _)) in worker_spawners.iter().enumerate() {
                let carrier_bit = 1u8 << carrier_id;
                if online_mask & carrier_bit == 0
                    && !crate::remote_work_wake::wake_cpu_for_remote_work(*worker_slot)
                {
                    failed_mask |= carrier_bit;
                }
            }
            log_helio_carrier_wake_result(failed_mask, carrier_count, "handshake-retry");
            return match crate::r::helio_game::prepare_cpu_carriers(&carrier_metadata) {
                Some(crate::r::helio_game::CpuCarrierBootstrapState::Online) => {
                    SpawnAttempt::Spawned
                }
                _ => SpawnAttempt::Skipped,
            };
        }
        crate::r::helio_game::CpuCarrierBootstrapState::NeedsSchedule => {}
    }

    // Reserve every Embassy task slot before starting any carrier. A partial
    // pool would strand the deterministic shards assigned to a missing worker.
    let mut carrier_tasks = Vec::with_capacity(worker_spawners.len());
    for (carrier_id, (worker_slot, core_kind, worker_spawner)) in
        worker_spawners.into_iter().enumerate()
    {
        let token = match crate::r::helio_game::helio_game_service_task(
            carrier_id as u8,
            carrier_count,
            worker_slot,
            core_kind,
        ) {
            Ok(token) => token,
            Err(error) => return SpawnAttempt::Failed(error),
        };
        carrier_tasks.push((carrier_id as u8, worker_slot, core_kind, worker_spawner, token));
    }

    if !crate::r::helio_game::mark_cpu_carriers_scheduled(&carrier_metadata) {
        crate::log_warn!(target: "service";
            "helio: cpu carrier schedule contract changed count={} capacity={} action=keep-service-pending registry=withheld\n",
            carrier_metadata.len(),
            crate::r::helio_game::CPU_CARRIER_CAPACITY,
        );
        return SpawnAttempt::Skipped;
    }

    let mut failed_mask = 0u8;
    for (carrier_id, worker_slot, core_kind, worker_spawner, token) in carrier_tasks {
        let wake_sent = worker_spawner.spawn_and_wake_remote(token);
        if !wake_sent {
            failed_mask |= 1u8 << carrier_id;
        }
        crate::log_info!(target: "service";
            "helio: cpu carrier={} scheduled carrier_count={} worker_slot={} core_kind={} remote_wake={} placement=background-ap2+ sharding=instance-id-mod-carrier-count registry=withheld-until-all-online gpu_principal=render0 gpu_context=shared-single-render-runtime gpu_affinity=none\n",
            carrier_id,
            carrier_count,
            worker_slot,
            core_kind,
            wake_sent as u8,
        );
    }
    log_helio_carrier_wake_result(failed_mask, carrier_count, "initial-schedule");
    match crate::r::helio_game::prepare_cpu_carriers(&carrier_metadata) {
        Some(crate::r::helio_game::CpuCarrierBootstrapState::Online) => SpawnAttempt::Spawned,
        _ => SpawnAttempt::Skipped,
    }
}

fn spawn_gridpaper_service(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |_worker_spawner| {
        crate::r::gridpaper_service::gridpaper_service_task()
    })
}

fn spawn_hid_udp_srv(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::hid_udp_srv::hid_udp_srv_task())
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

fn spawn_ftp_server(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::r::net::ftp::ftp_server_task())
}

fn spawn_gpu_completion_reaper(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::intel::gpgpu::gpu_completion_reaper_task())
}

fn spawn_gpu_fault_containment(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::gpu::vgpu::gpu_fault_containment_task())
}

fn spawn_trueos_spirit_workers(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    let Some(ap1_spawner) = crate::workers::ap1_ui_core_spawner() else {
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
        SpawnAttempt::Spawned
    }
}

fn spawn_spirit_response_window_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |worker_spawner| {
        crate::spirit::spirit_response_window_service_task(worker_spawner.cpu_slot())
    })
}

fn spawn_mouse_motion_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1_ui_core(spawner, |_ap1_spawner| {
        crate::r::mouse_motion_service::mouse_motion_service_task()
    })
}

fn spawn_keyboard_control_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1_ui_core(spawner, |_ap1_spawner| {
        crate::r::keyboard_control_service::keyboard_control_service_task()
    })
}

fn spawn_gamepad_control_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1_ui_core(spawner, |_ap1_spawner| {
        crate::r::gamepad_control_service::gamepad_control_service_task()
    })
}

fn spawn_ui4_input_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1_ui_core(spawner, |ap1_spawner| crate::ui4::ui4_input_service_task(ap1_spawner))
}

fn spawn_ui4_slot4_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1_ui_core(spawner, |_ap1_spawner| crate::ui4::ui4_slot4_service_task())
}

fn spawn_ui4_screenshot_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_worker(spawner, |_worker_spawner| crate::ui4::ui4_screenshot_service_task())
}

#[cfg(feature = "trueos_h264_encode_stream")]
fn spawn_ui4_h264_encode_stream_task(spawner: Spawner) -> SpawnAttempt {
    let _ = spawner;
    let Some((lastap_slot, lastap_kind, lastap_spawner)) = crate::workers::last_ap_service_worker()
    else {
        return SpawnAttempt::Skipped;
    };

    let prepare_token = match crate::ui4::ui4_h264_encode_prepare_task(lastap_slot) {
        Ok(token) => token,
        Err(error) => return SpawnAttempt::Failed(error),
    };
    let encode_token = match crate::ui4::ui4_h264_encode_stream_task() {
        Ok(token) => token,
        Err(error) => return SpawnAttempt::Failed(error),
    };
    let egress_token = match crate::ui4::ui4_h264_encode_udp_egress_task(lastap_slot) {
        Ok(token) => token,
        Err(error) => return SpawnAttempt::Failed(error),
    };
    lastap_spawner.spawn(prepare_token);
    lastap_spawner.spawn(encode_token);
    lastap_spawner.spawn(egress_token);
    crate::log_info!(target: "service";
        "ui4 h264 stream pipeline assigned carrier=lastap slot={} core_kind={} cooperative_tasks=3 prepare_slot={} encode_slot={} egress_slot={} exclusive_from=vm-hull+blocking-lanes+background-round-robin preparation_buffering=double encoded_au_queue=bounded future_home=ap1-ui\n",
        lastap_slot,
        lastap_kind,
        lastap_slot,
        lastap_slot,
        lastap_slot,
    );
    SpawnAttempt::Spawned
}

fn spawn_ui4_compositor_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1_ui_core(spawner, |_ap1_spawner| crate::ui4::ui4_compositor_service_task())
}

fn spawn_ui4_color_picker_service_task(spawner: Spawner) -> SpawnAttempt {
    spawn_on_ap1_ui_core(spawner, |_ap1_spawner| crate::ui4::ui4_color_picker_service_task())
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
    spawn_on_ap1_ui_core(spawner, |_ap1_spawner| crate::intel::hw_pic_service())
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
        && crate::workers::ap1_ui_core_spawner().is_some()
}

#[inline]
fn ui4_compositor_gate() -> bool {
    crate::intel::has_claimed_device()
        && crate::intel::active_scanout_dimensions().is_some()
        && crate::intel::ui4_rgba8_plane_stack_is_ready()
        && crate::workers::ap1_ui_core_spawner().is_some()
}

#[inline]
fn ap1_ui_core_ready_gate() -> bool {
    crate::workers::ap1_ui_core_spawner().is_some()
}

#[inline]
fn helio_cpu_carrier_ready_gate() -> bool {
    crate::workers::all_topology_spawners_registered()
        && crate::workers::has_background_worker_slot()
}

#[inline]
fn intel_media_engine_gate() -> bool {
    crate::intel::has_media_decode_engine()
}

#[cfg(feature = "trueos_h264_encode_stream")]
#[inline]
fn h264_lastap_service_gate() -> bool {
    intel_media_engine_gate() && crate::workers::last_ap_service_worker().is_some()
}

#[inline]
fn fallback_logo_ui_gate() -> bool {
    crate::virtio_gpu_logo::fallback_logo_available()
}

#[inline]
fn http_trueosfs_gate() -> bool {
    crate::r::readiness::is_set(
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED | crate::r::readiness::TRUEOSFS_INDEX_READY,
    )
}

#[inline]
fn html_shack_gate() -> bool {
    crate::r::readiness::is_set(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED)
}

#[inline]
fn ttstt_cpu_service_gate() -> bool {
    crate::r::readiness::is_set(
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED | crate::r::readiness::TRUEOSFS_INDEX_READY,
    )
}

#[cfg(feature = "trueos_lumen")]
fn lumen_boot_warm_gate() -> bool {
    crate::intel::guc_submission_ready()
        && crate::intel::gen12_integrated_pat_ready()
        && crate::gpu::physical::physical_device().is_some_and(|device| device.ready())
        && crate::intel::gpgpu::lfm25_q8_packed_project_supported()
        && crate::workers::has_perf_background_worker_slot()
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

const TRUEOSFS_RW_PROBE_PATH: &str = "trueos/probe/rw-500k.bin";
const TRUEOSFS_RW_PROBE_BYTES: usize = 500 * 1024;
const TRUEOSFS_RW_PROBE_CHUNK_BYTES: usize = 64 * 1024;
const TRUEOSFS_RW_PROBE_BEGIN_RETRIES: usize = 100;

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

#[trueos_executor::task]
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

    let mut begin_attempt = 0usize;
    let handle = loop {
        match crate::r::fs::trueosfs::file_write_begin_async(
            disk,
            TRUEOSFS_RW_PROBE_PATH,
            TRUEOSFS_RW_PROBE_BYTES as u64,
        )
        .await
        {
            Ok(Some(handle)) => break handle,
            Ok(None) => {
                crate::log!("trueosfs-rw-probe: result=failed phase=begin err=no-space-or-fs\n");
                return;
            }
            Err(crate::disc::block::Error::NotReady)
                if begin_attempt < TRUEOSFS_RW_PROBE_BEGIN_RETRIES =>
            {
                begin_attempt = begin_attempt.saturating_add(1);
                Timer::after(EmbassyDuration::from_millis(25)).await;
            }
            Err(err) => {
                crate::log!(
                    "trueosfs-rw-probe: result=failed phase=begin attempts={} err={:?}\n",
                    begin_attempt.saturating_add(1),
                    err
                );
                return;
            }
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

fn spawn_unix_fd_probe(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| crate::unix_fd_probe::unix_fd_probe_task())
}

const fn unix_fd_probe_task_spec() -> TaskSpec {
    if crate::allcaps::probes::UNIX_FD_PROBE {
        TaskSpec::disabled(
            "unix-fd-probe",
            crate::r::readiness::TRUEOSFS_ROOT_MOUNTED | crate::r::readiness::TRUEOSFS_INDEX_READY,
            &UNIX_FD_PROBE_STARTED,
            spawn_unix_fd_probe,
        )
    } else {
        TaskSpec::disabled(
            "unix-fd-probe",
            crate::r::readiness::TRUEOSFS_ROOT_MOUNTED | crate::r::readiness::TRUEOSFS_INDEX_READY,
            &UNIX_FD_PROBE_STARTED,
            spawn_unix_fd_probe,
        )
    }
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
    online_selector: Option<&'static str>,
    slot: &'static str,
    args: &'static [&'static str],
    launch_script: Option<&'static str>,
    settle_ms: u64,
}

const BP_AUTOSTARTS: &[BlueprintAutostart] = &[
    BlueprintAutostart {
        enabled: true,
        label: "swarm",
        archive: "swarm.bp",
        online_selector: Some("swarm"),
        slot: "swm",
        args: &[],
        launch_script: None,
        settle_ms: 250,
    },
    BlueprintAutostart {
        enabled: false,
        label: "img",
        archive: "img.bp",
        online_selector: Some("img"),
        slot: "img",
        args: &[],
        launch_script: Some(
            "show kernel:logo center nohit\nshow kernel:intel-graphics bottom-left\nshow kernel:bgrt bottom-right",
        ),
        settle_ms: 250,
    },
    BlueprintAutostart {
        enabled: false,
        label: "horizon",
        archive: "horizon.bp",
        online_selector: None,
        slot: "hor",
        args: &[],
        launch_script: None,
        settle_ms: 250,
    },
    BlueprintAutostart {
        enabled: false,
        label: "mandelbrot",
        archive: "mandelbrot.bp",
        online_selector: None,
        slot: "man",
        args: &[],
        launch_script: None,
        settle_ms: 750,
    },
    BlueprintAutostart {
        enabled: false,
        label: "flags",
        archive: "flags.bp",
        online_selector: None,
        slot: "flg",
        args: &[],
        launch_script: None,
        settle_ms: 750,
    },
    BlueprintAutostart {
        enabled: false,
        label: "weather",
        archive: "weather.bp",
        online_selector: None,
        slot: "wth",
        args: &[],
        launch_script: None,
        settle_ms: 750,
    },
    BlueprintAutostart {
        enabled: false,
        label: "chart",
        archive: "chart.bp",
        online_selector: None,
        slot: "chr",
        args: &[],
        launch_script: None,
        settle_ms: 750,
    },
    BlueprintAutostart {
        enabled: false,
        label: "hello_world",
        archive: "hello_world.bp",
        online_selector: None,
        slot: "h_w",
        args: &[],
        launch_script: None,
        settle_ms: 750,
    },
    BlueprintAutostart {
        enabled: false,
        label: "websys",
        archive: "websys.bp",
        online_selector: None,
        slot: "fs",
        args: &[],
        launch_script: None,
        settle_ms: 750,
    },
    BlueprintAutostart {
        enabled: false,
        label: "bat",
        archive: "bat.bp",
        online_selector: None,
        slot: "bat",
        args: &["--help"],
        launch_script: None,
        settle_ms: 750,
    },
];

#[trueos_executor::task]
async fn bp_autostart_task(spawner: Spawner) {
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

        let target = crate::shell2::matrix_target_for_slot_name(
            crate::shell2::OUTPUT_SYSTEM_MASK,
            config.slot,
        );

        crate::log!(
            "spawn-svc: bp-autostart begin label={} archive={} slot={}\n",
            config.label,
            config.archive,
            config.slot
        );

        if let Some(selector) = config.online_selector {
            let submitted = match config.launch_script {
                Some(script) => crate::shell2::submit_online_launch_script_to_target(
                    &spawner, target, selector, script,
                ),
                None => {
                    let mut args = Vec::with_capacity(config.args.len().saturating_add(1));
                    args.push(String::from(selector));
                    args.extend(config.args.iter().map(|arg| String::from(*arg)));
                    crate::shell2::submit_online_to_target(&spawner, target, args)
                }
            };
            match submitted {
                Ok(()) => crate::log!(
                    "spawn-svc: bp-autostart submitted label={} selector={} slot={} source=online\n",
                    config.label,
                    selector,
                    config.slot
                ),
                Err(err) => crate::log!(
                    "spawn-svc: bp-autostart skipped label={} selector={} slot={} source=online err={:?}\n",
                    config.label,
                    selector,
                    config.slot,
                    err
                ),
            }
            continue;
        }

        match crate::shell2::cmds::run::submit_archive_name_to_target_from_app_db_default_async(
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
}

fn spawn_bp_autostart(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| bp_autostart_task(spawner))
}

#[trueos_executor::task]
async fn weave_hello_autostart_task() {
    // Let the app-VM queue task enter its receive loop before submitting the
    // first Windows personality Blueprint. The module itself is a Limine boot
    // module, so this path does not depend on TRUEOSFS being mounted.
    Timer::after(EmbassyDuration::from_millis(250)).await;
    let target =
        crate::shell2::matrix_target_for_slot_name(crate::shell2::OUTPUT_SYSTEM_MASK, "wve");
    crate::log!("spawn-svc: weave-hello-autostart begin archive=weave_hello.bp slot=wve\n");
    match crate::shell2::cmds::run::submit_archive_name_to_target_from_app_db_default_async(
        target,
        "weave_hello.bp",
        Vec::new(),
    )
    .await
    {
        Ok(source) => crate::log!(
            "spawn-svc: weave-hello-autostart queued archive=weave_hello.bp slot=wve source={}\n",
            source
        ),
        Err(err) => crate::log!(
            "spawn-svc: weave-hello-autostart failed archive=weave_hello.bp slot=wve err={}\n",
            err
        ),
    }
}

fn spawn_weave_hello_autostart(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |_spawner| weave_hello_autostart_task())
}

fn spawn_net_tcp_shell(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| {
        crate::shell2::task(spawner, &crate::shell2::NET_TCP_SHELL_BACKEND)
    })
}

#[trueos_executor::task]
async fn local_shell_session_pool_bootstrap_task(spawner: Spawner) {
    let spawned = crate::shell2::spawn_local_shell_session_workers(spawner);
    if spawned == crate::shell2::LOCAL_SHELL_SESSION_CAP {
        crate::log!(
            "shell2-session: local executor pool ready workers={} host-shell-cap=10 tcp-reserved=1\n",
            spawned
        );
    } else {
        crate::log_error!(target: "shell2";
            "shell2-session: local executor pool incomplete workers={} expected={} action=disable-admission\n",
            spawned,
            crate::shell2::LOCAL_SHELL_SESSION_CAP
        );
    }
}

fn spawn_local_shell_session_pool(spawner: Spawner) -> SpawnAttempt {
    spawn_local(spawner, |spawner| local_shell_session_pool_bootstrap_task(spawner))
}

#[trueos_executor::task]
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

#[trueos_executor::task]
async fn executor_realm_migration_smoke_task(
    bsp_target: trueos_executor::MigrationTarget,
    ap_target: trueos_executor::MigrationTarget,
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
        let result = unsafe { trueos_executor::migrate_current_task_to(target) }.await;
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
const BP_AUTOSTART_READY: u32 = crate::r::readiness::TRUEOSFS_ROOT_MOUNTED
    | crate::r::readiness::BACKGROUND_AP_WORKER_READY
    | crate::r::readiness::VTHREAD_HW_TAG_READY;
const TASK_COUNT: usize = 73
    + cfg!(feature = "trueos_h264_encode_stream") as usize
    + cfg!(feature = "trueos_lumen") as usize;
static TASKS: [TaskSpec; TASK_COUNT] = [
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
        "font-warm-pool",
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED
            | crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        font_warm_pool_gate,
        &FONT_WARM_POOL_STARTED,
        spawn_font_warm_pool,
    ),
    TaskSpec::enabled_gated(
        "font-plan-producer-pool",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        font_plan_pool_gate,
        &FONT_PLAN_SERVICE_STARTED,
        spawn_font_plan_pool,
    ),
    TaskSpec::enabled_gated(
        "font-kernel-service",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        intel_device_gate,
        &FONT_KERNEL_SERVICE_STARTED,
        spawn_font_kernel_service,
    ),
    TaskSpec::configured_gated(
        crate::allcaps::ttstt::BOOT_RESIDENT_WARM_ENABLED,
        "ttstt-cpu-service",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        ttstt_cpu_service_gate,
        &TTSTT_CPU_SERVICE_STARTED,
        spawn_ttstt_cpu_service,
    ),
    TaskSpec::enabled(
        "ttstt-capture-writer",
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
        &TTSTT_CAPTURE_WRITER_STARTED,
        spawn_ttstt_capture_writer,
    ),
    TaskSpec::enabled("smp-hlt-history", 0, &SMP_HLT_HISTORY_STARTED, spawn_smp_hlt_history),
    TaskSpec::enabled("ram-usage-history", 0, &RAM_USAGE_HISTORY_STARTED, spawn_ram_usage_history),
    TaskSpec::disabled(
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
        "net-throughput-bench",
        crate::r::readiness::NET_ANY_CONFIGURED,
        &NET_THROUGHPUT_BENCH_STARTED,
        spawn_net_throughput_bench,
    ),
    TaskSpec::enabled(
        "tls-socket-service",
        crate::r::readiness::NET_ANY_CONFIGURED | crate::r::readiness::BACKGROUND_AP_WORKER_READY,
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
    TaskSpec::enabled("net-shell-listener", 0, &NET_SHELL_STARTED, spawn_net_shell),
    TaskSpec::enabled_gated(
        "helio-game",
        0,
        helio_cpu_carrier_ready_gate,
        &HELIO_GAME_STARTED,
        spawn_helio_game,
    ),
    // The current Gridpaper Blueprint still submits snapshots to this
    // consumer, which owns the corresponding UI4 presentation.
    TaskSpec::enabled(
        "gridpaper-service",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        &GRIDPAPER_SERVICE_STARTED,
        spawn_gridpaper_service,
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
    TaskSpec::disabled(
        "trueosfs-rw-probe",
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED | crate::r::readiness::TRUEOSFS_INDEX_READY,
        &TRUEOSFS_RW_PROBE_STARTED,
        spawn_trueosfs_rw_probe,
    ),
    unix_fd_probe_task_spec(),
    TaskSpec::enabled("app-vm-run-queue", 0, &APP_VM_RUN_QUEUE_STARTED, spawn_app_vm_run_queue),
    TaskSpec::disabled(
        "weave-hello-autostart",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY | crate::r::readiness::VTHREAD_HW_TAG_READY,
        &WEAVE_HELLO_AUTOSTART_STARTED,
        spawn_weave_hello_autostart,
    ),
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
    TaskSpec::disabled(
        "ftp-server",
        NET_ANY_CONFIGURED_AND_ROOT_READY,
        &FTP_SERVER_STARTED,
        spawn_ftp_server,
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
    // Spirit owns one retained Gridpaper pool lease. The response worker
    // waits for its paired Lilly keyboard and UI4 presentation internally,
    // types through the ordinary input route, and hides without releasing the
    // resident scene between replies.
    TaskSpec::enabled(
        "spirit-response-gridpaper",
        crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        &SPIRIT_RESPONSE_WINDOW_STARTED,
        spawn_spirit_response_window_task,
    ),
    TaskSpec::enabled_gated(
        "mouse-motion-service",
        0,
        ap1_ui_core_ready_gate,
        &MOUSE_MOTION_SERVICE_STARTED,
        spawn_mouse_motion_service_task,
    ),
    TaskSpec::enabled_gated(
        "keyboard-control-service",
        0,
        ap1_ui_core_ready_gate,
        &KEYBOARD_CONTROL_SERVICE_STARTED,
        spawn_keyboard_control_service_task,
    ),
    TaskSpec::enabled_gated(
        "gamepad-control-service",
        0,
        ap1_ui_core_ready_gate,
        &GAMEPAD_CONTROL_SERVICE_STARTED,
        spawn_gamepad_control_service_task,
    ),
    TaskSpec::enabled_gated(
        "ui4-input-service",
        0,
        ui4_compositor_gate,
        &UI4_INPUT_SERVICE_STARTED,
        spawn_ui4_input_service_task,
    ),
    TaskSpec::enabled_gated(
        "ui4-slot4-service",
        0,
        ui4_compositor_gate,
        &UI4_SLOT4_SERVICE_STARTED,
        spawn_ui4_slot4_service_task,
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
        h264_lastap_service_gate,
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
    #[cfg(feature = "trueos_lumen")]
    TaskSpec::configured_gated(
        crate::allcaps::lumen::BOOT_RESIDENT_WARM_ENABLED,
        "lumen-boot-warm",
        crate::r::readiness::TRUEOSFS_ROOT_MOUNTED
            | crate::r::readiness::TRUEOSFS_INDEX_READY
            | crate::r::readiness::BACKGROUND_AP_WORKER_READY
            | crate::r::readiness::UI4_COMPOSITOR_READY,
        lumen_boot_warm_gate,
        &LUMEN_BOOT_WARM_STARTED,
        spawn_lumen_boot_warm,
    ),
    TaskSpec::enabled_gated(
        "ui4-window-broker-snapshot",
        0,
        ui4_compositor_gate,
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
        "tinyaudio_service",
        crate::r::readiness::INTEL_HDA_READY | crate::r::readiness::BACKGROUND_AP_WORKER_READY,
        &TINYAUDIO_SERVICE_STARTED,
        spawn_tinyaudio_service,
    ),
    TaskSpec::disabled(
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
    TaskSpec::enabled(
        "local-shell-session-pool",
        0,
        &LOCAL_SHELL_SESSION_POOL_STARTED,
        spawn_local_shell_session_pool,
    ),
    TaskSpec::enabled("net-tcp-shell", 0, &NET_TCP_SHELL_STARTED, spawn_net_tcp_shell),
    TaskSpec::disabled("atomic_bomb", 0, &ATOMIC_BOMB_STARTED, spawn_atomic_bomb),
];

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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
                        if spec.name == "net-shell-listener" {
                            // Stable fresh-boot provenance for the physical log
                            // collector. Routine service Info is intentionally
                            // filtered in the normal profile, so this exact-once
                            // milestone uses LogOs' sparse Important class.
                            crate::log_os::service_important_line(format_args!(
                                "spawn-svc: started {} (mask=0x{:08X})\n",
                                spec.name, spec.required
                            ));
                        } else {
                            crate::log!(
                                "spawn-svc: started {} (mask=0x{:08X})\n",
                                spec.name,
                                spec.required
                            );
                        }
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
