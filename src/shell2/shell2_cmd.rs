use embassy_executor::Spawner;

use super::ShellBackend2;

#[derive(Clone, Copy)]
pub(crate) enum CommandSessionKind {
    FormatSure,
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
