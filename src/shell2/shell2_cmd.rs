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
    StartSession(CommandSessionKind),
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    line: &str,
) -> ParseOutcome {
    if let Some(outcome) = super::cmds::cry::try_parse_slot_input(spawner, io, line) {
        return outcome;
    }

    let submitted = line.trim();
    if let Some(bios_tail) = command_tail(submitted, "bios") {
        if let Some(capture_tail) = command_tail(bios_tail, "capture") {
            return super::cmds::bios_capture::try_parse(io, capture_tail);
        }
        if let Some(outcome) = super::cmds::bios_hii::try_parse(io, bios_tail) {
            return outcome;
        }
        if let Some(outcome) = super::cmds::bios_browser::try_parse(io, bios_tail) {
            return outcome;
        }
    }
    super::shell2_cmd_registry::try_dispatch(spawner, io, submitted)
}

fn command_tail<'a>(submitted: &'a str, expected: &str) -> Option<&'a str> {
    let mut parts = submitted.splitn(2, char::is_whitespace);
    let command = parts.next()?;
    if !command.eq_ignore_ascii_case(expected) {
        return None;
    }
    Some(parts.next().unwrap_or("").trim_start())
}
