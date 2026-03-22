use alloc::string::String as AllocString;

use spin::Mutex;

use super::{ShellBackend2, print_shell_line};

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

pub(crate) struct IrcIdentity {
    pub(crate) nick: AllocString,
    pub(crate) username: AllocString,
    pub(crate) password: Option<AllocString>,
}

impl IrcIdentity {
    const fn empty() -> Self {
        Self {
            nick: AllocString::new(),
            username: AllocString::new(),
            password: None,
        }
    }
}

static IDENTITY: Mutex<IrcIdentity> = Mutex::new(IrcIdentity::empty());

pub(crate) fn identity_nick() -> AllocString {
    IDENTITY.lock().nick.clone()
}

pub(crate) fn submit_user(io: &'static dyn ShellBackend2, line: &str) {
    let mut parts = line.split_whitespace();
    let Some(nick) = parts.next() else {
        print_shell_line(io, "irc: usage: <nick> [<username>] [<password>]");
        return;
    };
    let username = parts.next().unwrap_or(nick);
    let password = parts.next().map(AllocString::from);

    let mut id = IDENTITY.lock();
    id.nick = AllocString::from(nick);
    id.username = AllocString::from(username);
    id.password = password;
    let has_pass = id.password.is_some();
    drop(id);

    if has_pass {
        print_shell_line(io, "irc: identity set (nick+user+pass)");
    } else {
        print_shell_line(io, "irc: identity set (nick+user)");
    }
}

pub(crate) fn submit(io: &'static dyn ShellBackend2, mode: IrcPromptMode, line: &str) {
    match mode {
        IrcPromptMode::User => submit_user(io, line),
        IrcPromptMode::Join | IrcPromptMode::Pmsg => {}
    }
}
