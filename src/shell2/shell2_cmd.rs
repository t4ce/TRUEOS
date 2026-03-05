pub(crate) enum ParseOutcome {
    NotCommand,
}

impl ParseOutcome {
    pub(crate) const fn handled(self) -> bool {
        match self {
            Self::NotCommand => false,
        }
    }
}

pub(crate) fn try_parse(_line: &str) -> ParseOutcome {
    // Command surface intentionally empty for now.
    ParseOutcome::NotCommand
}
