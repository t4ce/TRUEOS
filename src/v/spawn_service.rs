use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::{SpawnError, SpawnToken, Spawner};
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
    required: u32,
    started: &'static AtomicBool,
    spawn: fn(Spawner) -> SpawnAttempt,
}

enum SpawnAttempt {
    Spawned,
    Skipped,
    Failed(SpawnError),
}

// --- one-shot guards (kept here so boot/task wiring is centralized) ---

static VGA_FONT_CACHE_STARTED: AtomicBool = AtomicBool::new(false);
static BSP_SMOKE_SERVICE_STARTED: AtomicBool = AtomicBool::new(false);
static TRUEOSFS_MOUNT_SERVICE_STARTED: AtomicBool = AtomicBool::new(false);

static NET_POLL_STARTED: AtomicBool = AtomicBool::new(false);
static NET_SERVICE_STARTED: AtomicBool = AtomicBool::new(false);
static TLS_SOCKET_SERVICE_STARTED: AtomicBool = AtomicBool::new(false);
static NET_SHELL_STARTED: AtomicBool = AtomicBool::new(false);
static HTTP_TRUEOSFS_STARTED: AtomicBool = AtomicBool::new(false);

static TGA_TASK_STARTED: AtomicBool = AtomicBool::new(false);

static USB_CONTROLLER_TASKS_STARTED: AtomicBool = AtomicBool::new(false);
static HID_INPUT_LOGGER_STARTED: AtomicBool = AtomicBool::new(false);
static UAC_SINE_STARTED: AtomicBool = AtomicBool::new(false);
static UAC_SONG_STARTED: AtomicBool = AtomicBool::new(false);
static VLEDS_MUX_STARTED: AtomicBool = AtomicBool::new(false);
static VLEDS_CYCLE_STARTED: AtomicBool = AtomicBool::new(false);
static TRUEKEY_DRAIN_STARTED: AtomicBool = AtomicBool::new(false);
static PIANO_DRAIN_STARTED: AtomicBool = AtomicBool::new(false);
static TASKMON_REPORTER_STARTED: AtomicBool = AtomicBool::new(false);

static BENCH_NETWORK_STARTED: AtomicBool = AtomicBool::new(false);

static BOOT_FETCH_SMOKE_STARTED: AtomicBool = AtomicBool::new(false);
static BOOT_PARSE5_SMOKE_STARTED: AtomicBool = AtomicBool::new(false);
static NALGEBRA_DEMO_STARTED: AtomicBool = AtomicBool::new(false);
static PCI_IDS_CACHE_STARTED: AtomicBool = AtomicBool::new(false);

static UART_SHELL_STARTED: AtomicBool = AtomicBool::new(false);
static NET_TCP_SHELL_STARTED: AtomicBool = AtomicBool::new(false);

// --- spawn wrappers (keep per-task logic out of main.rs) ---

fn spawn_monitored<T>(spawner: Spawner, name: &'static str, token: SpawnToken<T>) -> SpawnAttempt {
    match crate::v::taskmon::spawn(&spawner, name, token) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_vga_font_cache(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(spawner, "vga-font-cache", crate::vga::init_font_cache_task())
}

fn spawn_bsp_smoke_service(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(
        spawner,
        "bsp-smoke-service",
        crate::tst::smoke_fs::bsp_smoke_service_task(),
    )
}

fn spawn_trueosfs_mount_service(spawner: Spawner) -> SpawnAttempt {
    spawn_monitored(
        spawner,
        "trueosfs-mount-service",
        crate::v::fs::trueosfs::mount_service_task(),
    )
}


fn spawn_net_poll_tasks(spawner: Spawner) -> SpawnAttempt {
    // Some drivers may fail to report a MAC early; treat any detected NIC as usable.
    let count = crate::net::device_count();
    if count == 0 {
        return SpawnAttempt::Skipped;
    }
    for idx in 0..count {
        if let Err(e) = crate::v::taskmon::spawn(
            &spawner,
            "net-poll",
            crate::net::adapter::net_poll_task(idx),
        ) {
            crate::log!("net: spawn net_poll_task({}) failed: {:?}\n", idx, e);
        }
    }
    SpawnAttempt::Spawned
}

fn spawn_net_service(spawner: Spawner) -> SpawnAttempt {
    if crate::net::device_count() == 0 {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(spawner, "net-service", crate::net::adapter::net_service_task())
}

fn spawn_tls_socket_service(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(
        spawner,
        "tls-socket-service",
        crate::net::tls_socket::tls_socket_service_task(),
    )
}

fn spawn_net_shell(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(spawner, "net-shell", crate::net::adapter::net_shell_task())
}

fn spawn_http_trueosfs(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(
        spawner,
        "http-trueosfs",
        crate::tst::http_trueosfs::http_trueosfs_task(),
    )
}

fn spawn_tga_task(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(spawner, "tga", crate::tga::tga_task())
}


fn spawn_usb_controller_tasks(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    for info in crate::usb::xhci::xhc_list().iter().copied() {
        // reads from hardware into dma buffs
        let _ = crate::v::taskmon::spawn(
            &spawner,
            "usb-xhci-poll",
            crate::usb::xhci::poll_task(info),
        );
        // reads from our dma buffs into usb rings
        let _ = crate::v::taskmon::spawn(&spawner, "usb-poll", crate::usb::poll_task(info));
        // Single long-lived scout per controller. Rescans are triggered via a flag.
        let _ = crate::v::taskmon::spawn(
            &spawner,
            "usb-scout",
            crate::usb::usb_scout_service(info),
        );
    }
    SpawnAttempt::Spawned
}

fn spawn_hid_input_logger(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(spawner, "hid-input-logger", crate::usb::hid::input_logger())
}

fn spawn_uac_sine(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(spawner, "uac-sine", crate::usb::uac::sine_task())
}

fn spawn_uac_song(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    let Some(ap1_spawner) = crate::runtime::first_ap_spawner() else {
        // Wait until AP1 executor is online so this task runs there.
        return SpawnAttempt::Skipped;
    };
    let _ = spawner; // keep signature stable; song intentionally targets AP1.
    match crate::v::taskmon::spawn_send(&ap1_spawner, "uac-song", crate::usb::uac::song_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_vleds_mux(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(spawner, "vleds-mux", crate::v::leds::task())
}

fn spawn_vleds_cycle(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(spawner, "vleds-cycle", crate::v::leds::color_cycle_task())
}

fn spawn_truekey_drain(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(spawner, "truekey-drain", crate::usb::truekey::drain_loop())
}

fn spawn_piano_drain(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(spawner, "piano-drain", crate::usb::midi::piano_drain_loop())
}

fn spawn_boot_fetch_smoke(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(
        spawner,
        "boot-fetch-smoke",
        crate::tst::boot_fetch_to_file_smoke_task(),
    )
}

fn spawn_boot_parse5_smoke(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(
        spawner,
        "boot-parse5-smoke",
        crate::tst::boot_parse5_smoke_task(),
    )
}

fn spawn_nalgebra_demo(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(
        spawner,
        "boot-nalgebra-demo",
        crate::tst::nalgebra_demo::boot_nalgebra_demo_task(),
    )
}

fn spawn_pci_ids_cache(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(
        spawner,
        "pciids-boot-cache",
        crate::pci::pciids::boot_cache_pci_ids_task(),
    )
}

fn spawn_uart_shell(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(
        spawner,
        "uart-shell",
        crate::shell::task(spawner, &crate::shell::UART1_COM1_BACKEND),
    )
}

fn spawn_net_tcp_shell(spawner: Spawner) -> SpawnAttempt {
    if crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(
        spawner,
        "net-tcp-shell",
        crate::shell::task(spawner, &crate::shell::NET_TCP_SHELL_BACKEND),
    )
}

fn spawn_taskmon_reporter(_: Spawner) -> SpawnAttempt { SpawnAttempt::Skipped }
/*
fn spawn_taskmon_reporter(spawner: Spawner) -> SpawnAttempt { 
    spawn_monitored(
        spawner,
        "taskmon-reporter",
        crate::v::taskmon::taskmon_reporter_task(),
    )
}*/

fn spawn_bench_network(spawner: Spawner) -> SpawnAttempt {
    if !crate::v::mode::is_benchmark() {
        return SpawnAttempt::Skipped;
    }
    spawn_monitored(
        spawner,
        "bench-network",
        crate::tst::bench::raw_network_bench_task(),
    )
}


// --- registry ---

const HID_ANY_CLAIMED: u32 =
    crate::v::readiness::HID_MOUSE_CLAIMED | crate::v::readiness::HID_KEYBOARD_CLAIMED;

const NET_AND_ROOT_READY: u32 =
    crate::v::readiness::NET_GATEWAY_REACHABLE | crate::v::readiness::TRUEOSFS_ROOT_MOUNTED;
const UAC_SONG_READY: u32 =
    crate::v::readiness::UAC_SINE_DONE
    | crate::v::readiness::UAC_ATTACHED;

static TASKS: &[TaskSpec] = &[
    // Core background services (always-on / request-driven)
    TaskSpec {
        name: "vga-font-cache",
        required: 0,
        started: &VGA_FONT_CACHE_STARTED,
        spawn: spawn_vga_font_cache,
    },
    TaskSpec {
        name: "bsp-smoke-service",
        required: 0,
        started: &BSP_SMOKE_SERVICE_STARTED,
        spawn: spawn_bsp_smoke_service,
    },
    TaskSpec {
        name: "trueosfs-mount-service",
        required: 0,
        started: &TRUEOSFS_MOUNT_SERVICE_STARTED,
        spawn: spawn_trueosfs_mount_service,
    },
    TaskSpec {
        name: "taskmon-reporter",
        required: 0,
        started: &TASKMON_REPORTER_STARTED,
        spawn: spawn_taskmon_reporter,
    },

    // Network producers (may no-op if no NIC exists)
    TaskSpec {
        name: "net-poll-tasks",
        required: 0,
        started: &NET_POLL_STARTED,
        spawn: spawn_net_poll_tasks,
    },
    TaskSpec {
        name: "net-service",
        required: 0,
        started: &NET_SERVICE_STARTED,
        spawn: spawn_net_service,
    },

    // Network consumers
    TaskSpec {
        name: "tls-socket-service",
        required: crate::v::readiness::NET_GATEWAY_REACHABLE,
        started: &TLS_SOCKET_SERVICE_STARTED,
        spawn: spawn_tls_socket_service,
    },
    TaskSpec {
        name: "net-shell",
        required: crate::v::readiness::NET_GATEWAY_REACHABLE,
        started: &NET_SHELL_STARTED,
        spawn: spawn_net_shell,
    },
    TaskSpec {
        name: "http-trueosfs",
        required: NET_AND_ROOT_READY,
        started: &HTTP_TRUEOSFS_STARTED,
        spawn: spawn_http_trueosfs,
    },

    // USB core + peripherals
    TaskSpec {
        name: "tga",
        required: 0,
        started: &TGA_TASK_STARTED,
        spawn: spawn_tga_task,
    },

    TaskSpec {
        name: "usb-controller-tasks",
        required: 0,
        started: &USB_CONTROLLER_TASKS_STARTED,
        spawn: spawn_usb_controller_tasks,
    },
    TaskSpec {
        name: "hid-input-logger",
        required: HID_ANY_CLAIMED,
        started: &HID_INPUT_LOGGER_STARTED,
        spawn: spawn_hid_input_logger,
    },
    TaskSpec {
        name: "uac-sine",
        required: 0,
        started: &UAC_SINE_STARTED,
        spawn: spawn_uac_sine,
    },
    TaskSpec {
        name: "uac-song",
        required: UAC_SONG_READY,
        started: &UAC_SONG_STARTED,
        spawn: spawn_uac_song,
    },
    TaskSpec {
        name: "vleds-mux",
        required: 0,
        started: &VLEDS_MUX_STARTED,
        spawn: spawn_vleds_mux,
    },
    TaskSpec {
        name: "vleds-cycle",
        required: 0,
        started: &VLEDS_CYCLE_STARTED,
        spawn: spawn_vleds_cycle,
    },
    TaskSpec {
        name: "truekey-drain",
        required: 0,
        started: &TRUEKEY_DRAIN_STARTED,
        spawn: spawn_truekey_drain,
    },
    TaskSpec {
        name: "piano-drain",
        required: crate::v::readiness::PIANO_CLAIMED,
        started: &PIANO_DRAIN_STARTED,
        spawn: spawn_piano_drain,
    },

    // Boot-time gated tasks
    TaskSpec {
        name: "boot-fetch-smoke",
        required: NET_AND_ROOT_READY,
        started: &BOOT_FETCH_SMOKE_STARTED,
        spawn: spawn_boot_fetch_smoke,
    },
    TaskSpec {
        name: "boot-parse5-smoke",
        required: NET_AND_ROOT_READY,
        started: &BOOT_PARSE5_SMOKE_STARTED,
        spawn: spawn_boot_parse5_smoke,
    },
    TaskSpec {
        name: "boot-nalgebra-demo",
        required: 0,
        started: &NALGEBRA_DEMO_STARTED,
        spawn: spawn_nalgebra_demo,
    },
    TaskSpec {
        name: "pciids-boot-cache",
        required: NET_AND_ROOT_READY,
        started: &PCI_IDS_CACHE_STARTED,
        spawn: spawn_pci_ids_cache,
    },
    TaskSpec {
        name: "uart-shell",
        required: 0,
        started: &UART_SHELL_STARTED,
        spawn: spawn_uart_shell,
    },
    TaskSpec {
        name: "net-tcp-shell",
        required: crate::v::readiness::NET_GATEWAY_REACHABLE,
        started: &NET_TCP_SHELL_STARTED,
        spawn: spawn_net_tcp_shell,
    },

    // Benchmark mode tasks (only spawn when SystemMode::Benchmark is active)
    TaskSpec {
        name: "bench-network",
        required: 0,
        started: &BENCH_NETWORK_STARTED,
        spawn: spawn_bench_network,
    },
];

#[embassy_executor::task]
pub async fn spawn_service_task(spawner: Spawner) {
    crate::v::taskmon::run("spawn-service", async move {
        // Poll quickly until we have started everything; then back off.
        loop {
            let ready = crate::v::readiness::mask();
            let mut pending = 0usize;
            let mut started_any = false;

            for spec in TASKS {
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
                    }
                    SpawnAttempt::Skipped => {
                        // Not applicable right now (e.g. no NIC). Allow re-attempt later.
                        spec.started.store(false, Ordering::Release);
                        pending += 1;
                    }
                    SpawnAttempt::Failed(e) => {
                        // Allow retry.
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

            // If we made progress, poll again quickly so chains of dependent tasks start promptly.
            // If nothing changed, back off to reduce idle overhead.
            let sleep_ms = if started_any { 10 } else if pending == 0 { 250 } else { 50 };
            Timer::after(EmbassyDuration::from_millis(sleep_ms)).await;
        }
    })
    .await;
}
