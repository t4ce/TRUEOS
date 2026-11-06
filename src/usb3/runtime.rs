//! Per-controller state published by the task that actually owns the controller.

use alloc::{format, string::String};
use spin::Mutex;

#[derive(Clone, Debug)]
pub struct UsbRuntimeDiag {
    pub controller_phase: &'static str,
    pub root_hub_lifecycle: &'static str,
    pub event_ready: bool,
    pub root_port_change_seen: bool,
    pub empty_probe_streak: u32,
    pub probe_requested: bool,
    pub probe_fail_streak: u32,
    pub early_fatal_rebind_streak: u32,
    pub last_probe_state: &'static str,
    pub last_probe_device_count: usize,
    pub recovery_quiescent_before_bind: bool,
    pub recovery_quiescent_ms: u64,
    pub recovery_initial_settle_ms: u64,
    pub recovery_probe_quiet_ms: u64,
    pub recovery_skip_delayed_event_handler: bool,
    pub last_error: Option<String>,
    pub last_error_stage: &'static str,
    pub probe_attempts: u64,
    pub probe_successes: u64,
    pub probe_failures: u64,
    pub probe_watchdog_timeouts: u64,
    pub last_probe_started_ms: Option<u64>,
    pub last_probe_elapsed_ms: u64,
    pub snapshot_sequence: u64,
    pub snapshot_at_ms: Option<u64>,
    pub snapshot_error: Option<String>,
    pub operation_stage: &'static str,
    pub operation_requested: bool,
    pub operation_started_ms: Option<u64>,
    pub operation_elapsed_ms: u64,
    pub operation_watchdog_timeouts: u64,
}

impl UsbRuntimeDiag {
    pub const fn new() -> Self {
        Self {
            controller_phase: "discovered",
            root_hub_lifecycle: "unclaimed",
            event_ready: false,
            root_port_change_seen: false,
            empty_probe_streak: 0,
            probe_requested: false,
            probe_fail_streak: 0,
            early_fatal_rebind_streak: 0,
            last_probe_state: "never",
            last_probe_device_count: 0,
            recovery_quiescent_before_bind: false,
            recovery_quiescent_ms: 0,
            recovery_initial_settle_ms: 0,
            recovery_probe_quiet_ms: 0,
            recovery_skip_delayed_event_handler: false,
            last_error: None,
            last_error_stage: "-",
            probe_attempts: 0,
            probe_successes: 0,
            probe_failures: 0,
            probe_watchdog_timeouts: 0,
            last_probe_started_ms: None,
            last_probe_elapsed_ms: 0,
            snapshot_sequence: 0,
            snapshot_at_ms: None,
            snapshot_error: None,
            operation_stage: "never",
            operation_requested: false,
            operation_started_ms: None,
            operation_elapsed_ms: 0,
            operation_watchdog_timeouts: 0,
        }
    }

    pub(crate) fn stage(&mut self, phase: &'static str, lifecycle: &'static str) {
        self.controller_phase = phase;
        self.root_hub_lifecycle = lifecycle;
    }

    pub(crate) fn failure(&mut self, stage: &'static str, error: String) {
        self.last_error_stage = stage;
        self.last_error = Some(error);
    }

    pub(crate) fn probe_started(&mut self, label: &'static str, now_ms: u64) {
        self.controller_phase = "probing";
        self.probe_requested = true;
        self.last_probe_state = label;
        self.last_probe_device_count = 0;
        self.last_probe_started_ms = Some(now_ms);
        self.last_probe_elapsed_ms = 0;
        self.probe_attempts = self.probe_attempts.saturating_add(1);
    }

    pub(crate) fn probe_watchdog(&mut self, now_ms: u64) {
        self.probe_watchdog_timeouts = self.probe_watchdog_timeouts.saturating_add(1);
        self.last_probe_elapsed_ms =
            now_ms.saturating_sub(self.last_probe_started_ms.unwrap_or(now_ms));
        self.controller_phase = "probe-pending";
        self.last_probe_state = "pending-watchdog";
        self.failure("probe-watchdog", format!(
            "enumeration still pending after {} ms; retaining the same future and DMA ownership",
            self.last_probe_elapsed_ms
        ));
    }

    pub(crate) fn probe_finished(&mut self, result: Result<usize, String>, now_ms: u64) {
        self.probe_requested = false;
        self.last_probe_elapsed_ms =
            now_ms.saturating_sub(self.last_probe_started_ms.unwrap_or(now_ms));
        match result {
            Ok(count) => {
                self.controller_phase = "maintenance";
                self.last_probe_state = if count == 0 { "empty" } else { "ok" };
                self.last_probe_device_count = count;
                self.probe_successes = self.probe_successes.saturating_add(1);
                self.probe_fail_streak = 0;
                self.empty_probe_streak = if count == 0 {
                    self.empty_probe_streak.saturating_add(1)
                } else {
                    0
                };
            }
            Err(error) => {
                self.controller_phase = "probe-error";
                self.last_probe_state = "error";
                self.probe_failures = self.probe_failures.saturating_add(1);
                self.probe_fail_streak = self.probe_fail_streak.saturating_add(1);
                self.failure("probe", error);
            }
        }
    }

    pub(crate) fn pump_stopped(&mut self) {
        self.event_ready = false;
        self.stage("event-stopped", "failed");
        self.failure("event-pump", String::from("xHCI event handler stopped"));
    }

    pub(crate) fn operation_started(&mut self, stage: &'static str, now_ms: u64) {
        self.controller_phase = stage;
        self.operation_stage = stage;
        self.operation_requested = true;
        self.operation_started_ms = Some(now_ms);
        self.operation_elapsed_ms = 0;
    }

    pub(crate) fn operation_watchdog(&mut self, now_ms: u64) {
        self.operation_watchdog_timeouts = self.operation_watchdog_timeouts.saturating_add(1);
        self.operation_elapsed_ms =
            now_ms.saturating_sub(self.operation_started_ms.unwrap_or(now_ms));
        self.failure(
            self.operation_stage,
            format!(
                "{} still pending after {} ms; retaining operation and DMA ownership",
                self.operation_stage, self.operation_elapsed_ms
            ),
        );
    }

    pub(crate) fn operation_finished(&mut self, now_ms: u64) {
        self.operation_requested = false;
        self.operation_elapsed_ms =
            now_ms.saturating_sub(self.operation_started_ms.unwrap_or(now_ms));
        if self.operation_stage == "handoff"
            && self.root_hub_lifecycle == "active"
            && self.event_ready
        {
            self.controller_phase = "maintenance";
        }
    }
}

impl Default for UsbRuntimeDiag {
    fn default() -> Self {
        Self::new()
    }
}

static RUNTIME: [Mutex<UsbRuntimeDiag>; super::MAX_CRABUSB_CONTROLLERS] =
    [const { Mutex::new(UsbRuntimeDiag::new()) }; super::MAX_CRABUSB_CONTROLLERS];

pub(crate) fn update(index: usize, update: impl FnOnce(&mut UsbRuntimeDiag)) {
    if let Some(runtime) = RUNTIME.get(index) {
        update(&mut runtime.lock());
    }
}

pub(crate) fn snapshot(index: usize) -> UsbRuntimeDiag {
    RUNTIME
        .get(index)
        .map(|runtime| runtime.lock().clone())
        .unwrap_or_else(|| {
            let mut runtime = UsbRuntimeDiag::new();
            runtime.stage("unsupported", "unclaimed");
            runtime.failure(
                "controller-limit",
                String::from("controller exceeds the configured runtime limit"),
            );
            runtime
        })
}
