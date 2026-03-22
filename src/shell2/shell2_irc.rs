#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum IrcPromptMode {
    User,
    Join,
    Pmsg,
}

impl IrcPromptMode {
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::User => Self::Join,
            Self::Join => Self::Pmsg,
            Self::Pmsg => Self::User,
        }
    }
}
