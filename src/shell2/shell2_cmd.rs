use trueos_executor::Spawner;

use super::ShellBackend2;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandSessionKind {
    FormatSure(u32),
}

impl CommandSessionKind {
    pub(crate) const fn shows_session_activity(self) -> bool {
        match self {
            Self::FormatSure(_) => true,
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
    if let Some(outcome) = super::cmds::cry::try_parse_slot_input(io, line) {
        return outcome;
    }

    let submitted = line.trim();
    if let Some(command) = submitted.split_whitespace().next() {
        if command.eq_ignore_ascii_case("bios") {
            let rest = submitted.get(command.len()..).unwrap_or("").trim_start();
            return super::cmds::bios::try_parse(io, rest);
        }
    }

    super::shell2_cmd_registry::try_dispatch(spawner, io, submitted)
}
