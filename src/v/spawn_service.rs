use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::{Spawner, SpawnError};
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
static QJS_ASYNC_FS_SERVICE_STARTED: AtomicBool = AtomicBool::new(false);

static NET_POLL_STARTED: AtomicBool = AtomicBool::new(false);
static NET_SERVICE_STARTED: AtomicBool = AtomicBool::new(false);
static TLS_SOCKET_SERVICE_STARTED: AtomicBool = AtomicBool::new(false);
static NET_SHELL_STARTED: AtomicBool = AtomicBool::new(false);
static HTTP_TRUEOSFS_STARTED: AtomicBool = AtomicBool::new(false);

static TGA_BLINK_STARTED: AtomicBool = AtomicBool::new(false);
static USB_CONTROLLER_TASKS_STARTED: AtomicBool = AtomicBool::new(false);
static HID_INPUT_LOGGER_STARTED: AtomicBool = AtomicBool::new(false);
static UAC_SINE_STARTED: AtomicBool = AtomicBool::new(false);
static VLEDS_MUX_STARTED: AtomicBool = AtomicBool::new(false);
static VLEDS_CYCLE_STARTED: AtomicBool = AtomicBool::new(false);
static TRUEKEY_DRAIN_STARTED: AtomicBool = AtomicBool::new(false);

static BOOT_FETCH_SMOKE_STARTED: AtomicBool = AtomicBool::new(false);
static NALGEBRA_DEMO_STARTED: AtomicBool = AtomicBool::new(false);
static PCI_IDS_CACHE_STARTED: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "tst-challenge")]
static SCHED_CHALLENGE_STARTED: AtomicBool = AtomicBool::new(false);

static UART_SHELL_STARTED: AtomicBool = AtomicBool::new(false);
static NET_TCP_SHELL_STARTED: AtomicBool = AtomicBool::new(false);

// --- spawn wrappers (keep per-task logic out of main.rs) ---

fn spawn_vga_font_cache(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::vga::init_font_cache_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_bsp_smoke_service(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::tst::smoke_fs::bsp_smoke_service_task()) {
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

fn spawn_qjs_async_fs_service(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::io::cabi::qjs_async_fs_service_task()) {
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

fn spawn_net_shell(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::net::adapter::net_shell_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_http_trueosfs(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::tst::http_trueosfs::http_trueosfs_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_tga_blink(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::tga::blink_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
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

fn spawn_uac_sine(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::usb::uac::sine_task()) {
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

fn spawn_boot_fetch_smoke(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::tst::boot_fetch_to_file_smoke_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_nalgebra_demo(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::tst::nalgebra_demo::boot_nalgebra_demo_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_pci_ids_cache(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::pci::pciids::boot_cache_pci_ids_task()) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

#[cfg(feature = "tst-challenge")]
fn spawn_sched_challenge(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::tst::sched_challenge::sched_challenge_task(spawner)) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_uart_shell(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::shell::task(spawner, &crate::shell::UART1_COM1_BACKEND)) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

fn spawn_net_tcp_shell(spawner: Spawner) -> SpawnAttempt {
    match spawner.spawn(crate::shell::task(spawner, &crate::shell::NET_TCP_SHELL_BACKEND)) {
        Ok(()) => SpawnAttempt::Spawned,
        Err(e) => SpawnAttempt::Failed(e),
    }
}

// --- registry ---

const HID_ANY_CLAIMED: u32 =
    crate::v::readiness::HID_MOUSE_CLAIMED | crate::v::readiness::HID_KEYBOARD_CLAIMED;

const NET_AND_ROOT_READY: u32 =
    crate::v::readiness::NET_GATEWAY_REACHABLE | crate::v::readiness::TRUEOSFS_ROOT_MOUNTED;

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
        name: "qjs-async-fs-service",
        required: 0,
        started: &QJS_ASYNC_FS_SERVICE_STARTED,
        spawn: spawn_qjs_async_fs_service,
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

    // UI / misc
    TaskSpec {
        name: "tga-blink",
        required: 0,
        started: &TGA_BLINK_STARTED,
        spawn: spawn_tga_blink,
    },

    // USB core + peripherals
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

    // Boot-time gated tasks
    TaskSpec {
        name: "boot-fetch-smoke",
        required: NET_AND_ROOT_READY,
        started: &BOOT_FETCH_SMOKE_STARTED,
        spawn: spawn_boot_fetch_smoke,
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
    #[cfg(feature = "tst-challenge")]
    TaskSpec {
        name: "sched-challenge",
        required: 0,
        started: &SCHED_CHALLENGE_STARTED,
        spawn: spawn_sched_challenge,
    },

    // Shell backends
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
];

#[embassy_executor::task]
pub async fn spawn_service_task(spawner: Spawner) {
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
}
