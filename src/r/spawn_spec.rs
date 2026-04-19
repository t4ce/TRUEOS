use core::sync::atomic::AtomicBool;

use embassy_executor::{SpawnError, Spawner};

fn task_gate_always() -> bool {
    true
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum SpawnPlacement {
    Local,
    Ap1,
    Worker,
    ReservedVmLane,
}

pub(super) struct TaskSpec {
    pub(super) name: &'static str,
    pub(super) placement: SpawnPlacement,
    pub(super) disabled: &'static AtomicBool,
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
        Self::enabled_on(SpawnPlacement::Local, name, required, started, spawn)
    }

    pub(super) const fn enabled_on(
        placement: SpawnPlacement,
        name: &'static str,
        required: u32,
        started: &'static AtomicBool,
        spawn: fn(Spawner) -> SpawnAttempt,
    ) -> Self {
        Self {
            name,
            placement,
            disabled: &TASK_NOT_DISABLED,
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
        Self::enabled_gated_on(SpawnPlacement::Local, name, required, gate, started, spawn)
    }

    pub(super) const fn enabled_gated_on(
        placement: SpawnPlacement,
        name: &'static str,
        required: u32,
        gate: fn() -> bool,
        started: &'static AtomicBool,
        spawn: fn(Spawner) -> SpawnAttempt,
    ) -> Self {
        Self {
            name,
            placement,
            disabled: &TASK_NOT_DISABLED,
            required,
            gate,
            started,
            spawn,
        }
    }

    pub(super) const fn disabled(
        name: &'static str,
        required: u32,
        disabled_flag: &'static AtomicBool,
        started: &'static AtomicBool,
        spawn: fn(Spawner) -> SpawnAttempt,
    ) -> Self {
        Self::disabled_on(
            SpawnPlacement::Local,
            name,
            required,
            disabled_flag,
            started,
            spawn,
        )
    }

    pub(super) const fn disabled_on(
        placement: SpawnPlacement,
        name: &'static str,
        required: u32,
        disabled_flag: &'static AtomicBool,
        started: &'static AtomicBool,
        spawn: fn(Spawner) -> SpawnAttempt,
    ) -> Self {
        Self {
            name,
            placement,
            disabled: disabled_flag,
            required,
            gate: task_gate_always,
            started,
            spawn,
        }
    }

    pub(super) const fn disabled_gated(
        name: &'static str,
        required: u32,
        gate: fn() -> bool,
        disabled_flag: &'static AtomicBool,
        started: &'static AtomicBool,
        spawn: fn(Spawner) -> SpawnAttempt,
    ) -> Self {
        Self::disabled_gated_on(
            SpawnPlacement::Local,
            name,
            required,
            gate,
            disabled_flag,
            started,
            spawn,
        )
    }

    pub(super) const fn disabled_gated_on(
        placement: SpawnPlacement,
        name: &'static str,
        required: u32,
        gate: fn() -> bool,
        disabled_flag: &'static AtomicBool,
        started: &'static AtomicBool,
        spawn: fn(Spawner) -> SpawnAttempt,
    ) -> Self {
        Self {
            name,
            placement,
            disabled: disabled_flag,
            required,
            gate,
            started,
            spawn,
        }
    }
}

static TASK_NOT_DISABLED: AtomicBool = AtomicBool::new(false);

pub(super) enum SpawnAttempt {
    Spawned,
    Skipped,
    Failed(SpawnError),
}
