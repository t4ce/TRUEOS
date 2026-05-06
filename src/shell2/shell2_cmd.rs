use embassy_executor::Spawner;

use super::ShellBackend2;

#[derive(Clone, Copy)]
pub(crate) enum CommandSessionKind {
    Ample,
    BenchRunning(u64),
    FormatSure(u32),
}

impl CommandSessionKind {
    pub(crate) const fn shows_session_activity(self) -> bool {
        match self {
            Self::Ample => false,
            Self::BenchRunning(_) => false,
            Self::FormatSure(_) => true,
        }
    }

    pub(crate) const fn accepts_broadcast_input(self) -> bool {
        match self {
            Self::Ample => false,
            Self::BenchRunning(_) => true,
            Self::FormatSure(_) => false,
        }
    }
}

pub(crate) enum ParseOutcome {
    Handled,
    NotCommand,
    SetLineWidth(usize),
    StartSession(CommandSessionKind),
}

impl ParseOutcome {
    pub(crate) const fn handled(self) -> bool {
        match self {
            Self::Handled | Self::SetLineWidth(_) | Self::StartSession(_) => true,
            Self::NotCommand => false,
        }
    }
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    line: &str,
) -> ParseOutcome {
    super::shell2_cmd_registry::try_dispatch(spawner, io, line.trim())
}
