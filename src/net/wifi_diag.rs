//! Bounded Wi-Fi recovery evidence, independent of the log transport and disk.
//! Records contain static stage/error metadata and counters, never credentials,
//! SSIDs, keys or packet contents. Formatting happens after releasing the lock.

use super::{DriverStatus, WifiState};
use alloc::string::String;
use core::fmt::Write;
use spin::Mutex;

const HISTORY_CAPACITY: usize = 32;

#[derive(Clone, Copy, Debug)]
pub(super) enum Action {
    Scan,
    Connect,
    Recover,
}
impl Action {
    const fn index(self) -> usize {
        match self {
            Self::Scan => 0,
            Self::Connect => 1,
            Self::Recover => 2,
        }
    }
    const fn tag(self) -> &'static str {
        match self {
            Self::Scan => "scan",
            Self::Connect => "connect",
            Self::Recover => "recover",
        }
    }
}

#[derive(Clone, Copy)]
struct ActionCounts {
    attempts: u64,
    accepted: u64,
    failed: u64,
    started_ms: Option<u64>,
    finished_ms: Option<u64>,
}
impl ActionCounts {
    const EMPTY: Self = Self {
        attempts: 0,
        accepted: 0,
        failed: 0,
        started_ms: None,
        finished_ms: None,
    };
}

#[derive(Clone, Copy)]
struct Failure {
    at_ms: u64,
    stage: &'static str,
    reason: &'static str,
}
#[derive(Clone, Copy)]
struct Transition {
    at_ms: u64,
    stage: &'static str,
    state: WifiState,
}
#[derive(Clone, Copy)]
struct ApDisconnect {
    at_ms: u64,
    subtype: u8,
    reason: u16,
}

#[derive(Clone, Copy)]
pub(super) struct Registers {
    pub at_ms: u64,
    pub stage: &'static str,
    pub csr_int: u32,
    pub csr_reset: u32,
    pub csr_gp: u32,
    pub hw_if: u32,
    pub rx_read: u16,
    pub rx_write: u16,
    pub tx_write: u16,
}

#[derive(Clone, Copy)]
struct Diagnostics {
    observed_at_ms: Option<u64>,
    state: WifiState,
    driver_status: DriverStatus,
    stage: &'static str,
    stage_started_ms: u64,
    first_failure: Option<Failure>,
    episode_failure: Option<Failure>,
    latest_failure: Option<Failure>,
    observed_error: Option<&'static str>,
    episode: u64,
    episode_open: bool,
    last_connected_ms: Option<u64>,
    last_disconnected_ms: Option<u64>,
    failure_records: u64,
    actions: [ActionCounts; 3],
    history: [Option<Transition>; HISTORY_CAPACITY],
    history_head: usize,
    history_len: usize,
    transitions: u64,
    latest_registers: Option<Registers>,
    first_failure_registers: Option<Registers>,
    episode_failure_registers: Option<Registers>,
    episode_ap_disconnect: Option<ApDisconnect>,
    latest_ap_disconnect: Option<ApDisconnect>,
}

impl Diagnostics {
    const fn new() -> Self {
        Self {
            observed_at_ms: None,
            state: WifiState::NoHardware,
            driver_status: DriverStatus::Unloaded,
            stage: "unobserved",
            stage_started_ms: 0,
            first_failure: None,
            episode_failure: None,
            latest_failure: None,
            observed_error: None,
            episode: 0,
            episode_open: true,
            last_connected_ms: None,
            last_disconnected_ms: None,
            failure_records: 0,
            actions: [ActionCounts::EMPTY; 3],
            history: [None; HISTORY_CAPACITY],
            history_head: 0,
            history_len: 0,
            transitions: 0,
            latest_registers: None,
            first_failure_registers: None,
            episode_failure_registers: None,
            episode_ap_disconnect: None,
            latest_ap_disconnect: None,
        }
    }

    fn transition(&mut self, now_ms: u64) {
        self.history[self.history_head] = Some(Transition {
            at_ms: now_ms,
            stage: self.stage,
            state: self.state,
        });
        self.history_head = (self.history_head + 1) % HISTORY_CAPACITY;
        self.history_len = (self.history_len + 1).min(HISTORY_CAPACITY);
        self.transitions = self.transitions.saturating_add(1);
    }

    fn stage(&mut self, stage: &'static str, now_ms: u64) {
        if self.stage != stage {
            self.stage = stage;
            self.stage_started_ms = now_ms;
            self.transition(now_ms);
        }
    }

    fn begin_episode(&mut self, now_ms: u64) {
        if !self.episode_open {
            self.episode = self.episode.saturating_add(1);
            self.episode_open = true;
            self.episode_failure = None;
            self.episode_failure_registers = None;
            self.episode_ap_disconnect = None;
            self.last_disconnected_ms = Some(now_ms);
        }
    }

    fn failure(&mut self, stage: &'static str, reason: &'static str, now_ms: u64) {
        // Driver failure hooks run before poll publishes the new radio state.
        // Start the incident here too, preserving that original lower-level cause.
        self.begin_episode(now_ms);
        let failure = Failure {
            at_ms: now_ms,
            stage,
            reason,
        };
        let correlated = self
            .latest_registers
            .filter(|registers| registers.stage == stage && registers.at_ms == now_ms);
        if self.first_failure.is_none() {
            self.first_failure = Some(failure);
            self.first_failure_registers = correlated;
        }
        if self.episode_failure.is_none() {
            self.episode_failure = Some(failure);
            self.episode_failure_registers = correlated;
        }
        self.latest_failure = Some(failure);
        self.failure_records = self.failure_records.saturating_add(1);
    }

    fn failure_registers(&mut self, registers: Registers, reason: &'static str) {
        self.registers(registers);
        self.failure(registers.stage, reason, registers.at_ms);
    }

    fn observe(
        &mut self,
        state: WifiState,
        status: DriverStatus,
        error: Option<&'static str>,
        now_ms: u64,
    ) {
        let changed = self.state != state;
        if self.state == WifiState::Connected && state != WifiState::Connected {
            self.begin_episode(now_ms);
        }
        self.observed_at_ms = Some(now_ms);
        self.driver_status = status;
        self.state = state;
        if changed {
            self.transition(now_ms);
        }
        if let Some(reason) = error {
            if (changed || self.observed_error != Some(reason))
                && (self.episode_failure.is_none()
                    || self
                        .latest_failure
                        .is_none_or(|failure| failure.reason != reason))
            {
                self.failure("driver-poll", reason, now_ms);
            }
        }
        self.observed_error = error;
        if state == WifiState::Connected && changed {
            self.last_connected_ms = Some(now_ms);
            self.episode_open = false;
        }
    }

    fn action_started(&mut self, action: Action, now_ms: u64) {
        let counts = &mut self.actions[action.index()];
        counts.attempts = counts.attempts.saturating_add(1);
        counts.started_ms = Some(now_ms);
        counts.finished_ms = None;
        self.stage(action.tag(), now_ms);
    }

    fn action_finished(&mut self, action: Action, result: Result<(), &'static str>, now_ms: u64) {
        let counts = &mut self.actions[action.index()];
        counts.finished_ms = Some(now_ms);
        match result {
            Ok(()) => counts.accepted = counts.accepted.saturating_add(1),
            Err(reason) => {
                counts.failed = counts.failed.saturating_add(1);
                self.failure(action.tag(), reason, now_ms);
            }
        }
    }

    fn registers(&mut self, registers: Registers) {
        self.latest_registers = Some(registers);
    }

    fn ap_disconnect(&mut self, subtype: u8, reason: u16, now_ms: u64) {
        self.begin_episode(now_ms);
        let event = ApDisconnect {
            at_ms: now_ms,
            subtype,
            reason,
        };
        if self.episode_ap_disconnect.is_none() {
            self.episode_ap_disconnect = Some(event);
        }
        self.latest_ap_disconnect = Some(event);
    }

    fn text(&self, now_ms: u64) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "trueos wifi recovery snapshot v1");
        let _ = writeln!(
            out,
            "generated_at_ms={} observed_at_ms={:?} state={:?} driver={:?} stage={} stage_started_ms={} stage_elapsed_ms={}",
            now_ms,
            self.observed_at_ms,
            self.state,
            self.driver_status,
            self.stage,
            self.stage_started_ms,
            now_ms.saturating_sub(self.stage_started_ms)
        );
        let _ = writeln!(
            out,
            "episode={} last_connected_ms={:?} last_disconnected_ms={:?} failure_records={} transitions={} retained_transitions={}",
            self.episode,
            self.last_connected_ms,
            self.last_disconnected_ms,
            self.failure_records,
            self.transitions,
            self.history_len
        );
        for (name, failure) in [
            ("first_failure", self.first_failure),
            ("episode_first_failure", self.episode_failure),
            ("latest_failure", self.latest_failure),
        ] {
            if let Some(failure) = failure {
                let _ = writeln!(
                    out,
                    "{} at_ms={} stage={} reason={}",
                    name, failure.at_ms, failure.stage, failure.reason
                );
            } else {
                let _ = writeln!(out, "{}=-", name);
            }
        }
        for action in [Action::Scan, Action::Connect, Action::Recover] {
            let counts = self.actions[action.index()];
            let _ = writeln!(
                out,
                "action={} attempts={} accepted={} failed={} started_ms={:?} finished_ms={:?}",
                action.tag(),
                counts.attempts,
                counts.accepted,
                counts.failed,
                counts.started_ms,
                counts.finished_ms
            );
        }
        for (name, event) in [
            ("episode_ap_disconnect", self.episode_ap_disconnect),
            ("latest_ap_disconnect", self.latest_ap_disconnect),
        ] {
            if let Some(event) = event {
                let _ = writeln!(
                    out,
                    "{} at_ms={} subtype=0x{:02X} ieee_reason={}",
                    name, event.at_ms, event.subtype, event.reason
                );
            }
        }
        for (name, registers) in [
            ("first_failure_registers", self.first_failure_registers),
            (
                "episode_first_failure_registers",
                self.episode_failure_registers,
            ),
            ("latest_registers", self.latest_registers),
        ] {
            if let Some(registers) = registers {
                let _ = writeln!(
                    out,
                    "{} at_ms={} stage={} csr_int=0x{:08X} csr_reset=0x{:08X} csr_gp=0x{:08X} hw_if=0x{:08X} rx_read={} rx_write={} tx_write={}",
                    name,
                    registers.at_ms,
                    registers.stage,
                    registers.csr_int,
                    registers.csr_reset,
                    registers.csr_gp,
                    registers.hw_if,
                    registers.rx_read,
                    registers.rx_write,
                    registers.tx_write
                );
            }
        }
        for index in 0..self.history_len {
            if let Some(transition) =
                self.history[(self.history_head + HISTORY_CAPACITY - self.history_len + index)
                    % HISTORY_CAPACITY]
            {
                let _ = writeln!(
                    out,
                    "transition at_ms={} stage={} state={:?}",
                    transition.at_ms, transition.stage, transition.state
                );
            }
        }
        out
    }
}

static DIAGNOSTICS: Mutex<Diagnostics> = Mutex::new(Diagnostics::new());
pub(super) fn observe(
    state: WifiState,
    status: DriverStatus,
    error: Option<&'static str>,
    now_ms: u64,
) {
    DIAGNOSTICS.lock().observe(state, status, error, now_ms);
}
pub(super) fn stage(stage: &'static str, now_ms: u64) {
    DIAGNOSTICS.lock().stage(stage, now_ms);
}
pub(super) fn failure(stage: &'static str, reason: &'static str, now_ms: u64) {
    DIAGNOSTICS.lock().failure(stage, reason, now_ms);
}
pub(super) fn registers(registers: Registers) {
    DIAGNOSTICS.lock().registers(registers);
}
pub(super) fn failure_registers(registers: Registers, reason: &'static str) {
    DIAGNOSTICS.lock().failure_registers(registers, reason);
}
pub(super) fn ap_disconnect(subtype: u8, reason: u16, now_ms: u64) {
    DIAGNOSTICS.lock().ap_disconnect(subtype, reason, now_ms);
}
pub(super) fn action_started(action: Action, now_ms: u64) {
    DIAGNOSTICS.lock().action_started(action, now_ms);
}
pub(super) fn action_finished(action: Action, result: Result<(), &'static str>, now_ms: u64) {
    DIAGNOSTICS.lock().action_finished(action, result, now_ms);
}
pub(super) fn snapshot_text(now_ms: u64) -> String {
    let snapshot = *DIAGNOSTICS.lock();
    snapshot.text(now_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_failure_keeps_its_registers_across_clock_boundaries_and_later_resets() {
        let mut state = Diagnostics::new();
        state.observe(WifiState::Connected, DriverStatus::Running, None, 100);
        state.failure_registers(registers(201, "firmware-fault", 0x123), "original fault");
        state.observe(WifiState::Failed, DriverStatus::Loading, Some("original fault"), 202);
        state.registers(registers(5000, "recovery-before-reset", 0x456));
        state.failure_registers(registers(5025, "reset-master-stop", 0x200), "reset failed");
        assert_eq!(state.episode_failure.unwrap().at_ms, 201);
        assert_eq!(state.episode_failure_registers.unwrap().csr_reset, 0x123);
        assert_eq!(state.first_failure_registers.unwrap().at_ms, 201);
        assert_eq!(state.latest_registers.unwrap().csr_reset, 0x200);
    }

    fn registers(at_ms: u64, stage: &'static str, reset: u32) -> Registers {
        Registers {
            at_ms,
            stage,
            csr_int: 0x0200_0000,
            csr_reset: reset,
            csr_gp: 5,
            hw_if: 6,
            rx_read: 7,
            rx_write: 8,
            tx_write: 9,
        }
    }

    #[test]
    fn initial_link_loss_survives_recovery_failures_and_successful_restart() {
        let mut state = Diagnostics::new();
        state.observe(WifiState::Connected, DriverStatus::Running, None, 100);
        state.registers(registers(201, "firmware-fault", 0x123));
        state.failure("firmware-fault", "AX201 original transport failure", 201);
        state.observe(
            WifiState::Failed,
            DriverStatus::Loading,
            Some("AX201 original transport failure"),
            202,
        );
        state.action_started(Action::Recover, 5000);
        state.registers(registers(5025, "reset-master-stop", 0x200));
        state.action_finished(Action::Recover, Err("AX201 DMA master stop timeout"), 5026);
        state.action_started(Action::Recover, 10000);
        state.action_finished(Action::Recover, Ok(()), 10030);
        state.observe(WifiState::Scanning, DriverStatus::Loading, None, 10031);
        let first = state.episode_failure.unwrap();
        assert_eq!(
            (first.at_ms, first.stage, first.reason),
            (201, "firmware-fault", "AX201 original transport failure")
        );
        assert_eq!(state.episode_failure_registers.unwrap().csr_reset, 0x123);
        assert_eq!(
            state.latest_failure.unwrap().reason,
            "AX201 DMA master stop timeout"
        );
        let counts = state.actions[Action::Recover.index()];
        assert_eq!((counts.attempts, counts.accepted, counts.failed), (2, 1, 1));
        assert_eq!(
            (counts.started_ms, counts.finished_ms),
            (Some(10000), Some(10030))
        );
        state.observe(WifiState::Connected, DriverStatus::Running, None, 11000);
        assert_eq!(
            state.episode_failure.unwrap().at_ms,
            201,
            "reconnect snapshot retains its cause"
        );
        state.failure("station", "AX201 later association loss", 12000);
        assert_eq!(state.episode_failure.unwrap().at_ms, 12000);
        assert_eq!(
            state.first_failure.unwrap().at_ms,
            201,
            "lifetime first failure is never erased"
        );
    }

    #[test]
    fn repeated_poll_errors_are_deduplicated_and_history_is_fixed_capacity() {
        let mut state = Diagnostics::new();
        for time in 0..1000 {
            state.observe(
                WifiState::Failed,
                DriverStatus::Loading,
                Some("AX201 firmware failed"),
                time,
            );
        }
        assert_eq!(state.failure_records, 1);
        state.stage("reset-master-stop", 1000);
        state.stage("reset-master-stop", 1500);
        assert_eq!(state.stage_started_ms, 1000);
        for time in 2000..2100 {
            state.stage(if time % 2 == 0 { "scan" } else { "recover" }, time);
        }
        assert_eq!(state.history_len, HISTORY_CAPACITY);
        assert_eq!(state.transitions, 102);
        let text = state.text(2200);
        assert_eq!(
            text.lines()
                .filter(|line| line.starts_with("transition "))
                .count(),
            HISTORY_CAPACITY
        );
        assert!(text.contains("transition at_ms=2068"));
        assert!(!text.contains("transition at_ms=2067"));
    }

    #[test]
    fn stale_or_later_registers_are_never_mislabeled_as_original_failure_evidence() {
        let mut state = Diagnostics::new();
        state.registers(registers(10, "firmware-alive", 1));
        state.failure("station", "AX201 association lost", 5000);
        assert!(state.first_failure_registers.is_none());
        assert!(state.episode_failure_registers.is_none());
        state.registers(registers(5000, "reset-master-stop", 2));
        assert!(
            state.first_failure_registers.is_none(),
            "later readback cannot backfill original evidence"
        );
        assert!(state.episode_failure_registers.is_none());
        assert_eq!(state.latest_registers.unwrap().csr_reset, 2);
        state.observe(WifiState::Connected, DriverStatus::Running, None, 6000);
        state.registers(registers(7000, "station", 3));
        state.failure("station", "AX201 association lost", 7000);
        assert_eq!(state.episode_failure_registers.unwrap().csr_reset, 3);
        assert!(state.first_failure_registers.is_none());
    }

    #[test]
    fn validated_ap_reason_survives_reset_and_is_scoped_to_its_outage() {
        let mut state = Diagnostics::new();
        state.observe(WifiState::Connected, DriverStatus::Running, None, 100);
        state.ap_disconnect(0xc0, 4, 200);
        state.failure("station", "AX201 AP deauthenticated station", 200);
        state.observe(
            WifiState::Failed,
            DriverStatus::Loading,
            Some("AX201 AP deauthenticated station"),
            201,
        );
        assert_eq!(state.episode, 1);
        assert_eq!(
            state.latest_failure.unwrap().stage,
            "station",
            "poll must not mask the original stage"
        );
        state.action_started(Action::Recover, 5000);
        state.action_finished(Action::Recover, Ok(()), 5050);
        assert_eq!(state.episode_ap_disconnect.unwrap().reason, 4);
        state.observe(WifiState::Connected, DriverStatus::Running, None, 6000);
        state.ap_disconnect(0xa0, 8, 7000);
        assert_eq!(state.episode, 2);
        assert_eq!(
            (
                state.episode_ap_disconnect.unwrap().subtype,
                state.latest_ap_disconnect.unwrap().reason
            ),
            (0xa0, 8)
        );
    }
}
