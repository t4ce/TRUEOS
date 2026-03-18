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
    let submitted = line.trim();
    if let Some(rest) = submitted.strip_prefix("acpi") {
        let mut args = rest.split_whitespace();
        return super::cmds::acpi::try_parse(io, &mut args);
    }
    if let Some(rest) = submitted.strip_prefix("etc") {
        let mut args = rest.split_whitespace();
        return super::cmds::etc::try_parse(io, &mut args);
    }
    if let Some(rest) = submitted.strip_prefix("format") {
        let mut args = rest.split_whitespace();
        return super::cmds::format::try_parse(io, &mut args);
    }
    if submitted.eq_ignore_ascii_case("install") {
        super::cmds::install::submit_install(spawner, io);
        return ParseOutcome::Handled;
    }
    if submitted.eq_ignore_ascii_case("update") {
        super::cmds::update::submit_update(spawner, io);
        return ParseOutcome::Handled;
    }
    if let Some(rest) = submitted.strip_prefix("set") {
        let mut args = rest.split_whitespace();
        return super::cmds::set::try_parse(io, &mut args);
    }

    ParseOutcome::NotCommand
}
