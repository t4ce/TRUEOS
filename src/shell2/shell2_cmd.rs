use embassy_executor::Spawner;

use super::ShellBackend2;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandSessionKind {
    AudControl,
    FormatSure(u32),
    GpuCanvasRunning(u64),
    RemoveSure(u64),
}

impl CommandSessionKind {
    pub(crate) const fn shows_session_activity(self) -> bool {
        match self {
            Self::AudControl => true,
            Self::FormatSure(_) => true,
            Self::GpuCanvasRunning(_) => false,
            Self::RemoveSure(_) => true,
        }
    }

    pub(crate) const fn accepts_broadcast_input(self) -> bool {
        match self {
            Self::AudControl => false,
            Self::FormatSure(_) => false,
            Self::GpuCanvasRunning(_) => true,
            Self::RemoveSure(_) => false,
        }
    }
}

pub(crate) enum ParseOutcome {
    Handled,
    NotCommand,
    SetLineWidth(usize),
    StartSession(CommandSessionKind),
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    line: &str,
) -> ParseOutcome {
    super::shell2_cmd_registry::try_dispatch(spawner, io, line.trim())
}
