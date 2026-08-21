use core::sync::atomic::AtomicBool;

use trueos_executor::{SpawnError, Spawner};

fn task_gate_always() -> bool {
    true
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum SpawnPlacement {
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    Worker,
    ReservedVmLane,
}

pub(super) struct TaskSpec {
    pub(super) name: &'static str,
    pub(super) disabled: AtomicBool,
    pub(super) required: u32,
    pub(super) gate: fn() -> bool,
    pub(super) started: &'static AtomicBool,
    pub(super) spawn: fn(Spawner) -> SpawnAttempt,
}

impl TaskSpec {
    pub(super) const fn enabled(
        name: &'static str,
        required: u32,
        started: &'static AtomicBool,
        spawn: fn(Spawner) -> SpawnAttempt,
    ) -> Self {
        Self {
            name,
            disabled: AtomicBool::new(false),
            required,
            gate: task_gate_always,
            started,
            spawn,
        }
    }

    pub(super) const fn enabled_gated(
        name: &'static str,
        required: u32,
        gate: fn() -> bool,
        started: &'static AtomicBool,
        spawn: fn(Spawner) -> SpawnAttempt,
    ) -> Self {
        Self {
            name,
            disabled: AtomicBool::new(false),
            required,
            gate,
            started,
            spawn,
        }
    }

    pub(super) const fn configured_gated(
        enabled: bool,
        name: &'static str,
        required: u32,
        gate: fn() -> bool,
        started: &'static AtomicBool,
        spawn: fn(Spawner) -> SpawnAttempt,
    ) -> Self {
        Self {
            name,
            disabled: AtomicBool::new(!enabled),
            required,
            gate,
            started,
            spawn,
        }
    }

    pub(super) const fn disabled(
        name: &'static str,
        required: u32,
        started: &'static AtomicBool,
        spawn: fn(Spawner) -> SpawnAttempt,
    ) -> Self {
        Self {
            name,
            disabled: AtomicBool::new(true),
            required,
            gate: task_gate_always,
            started,
            spawn,
        }
    }
}

pub(super) enum SpawnAttempt {
    Spawned,
    Skipped,
    Failed(SpawnError),
}
