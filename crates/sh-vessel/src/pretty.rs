use core::fmt;

use crate::arg::{Argument, ArgumentKind, ArgumentTemplate};
use crate::cmd::CommandList;
use crate::help::Help;
use crate::Command;

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} - {}", self.long, self.short, self.help)
    }
}

impl fmt::Display for Help {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text)
    }
}

impl fmt::Display for CommandList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut index = 0;
        while index < self.commands.len() {
            if index != 0 {
                f.write_str("\n")?;
            }
            write!(f, "{}", self.commands[index])?;
            index += 1;
        }
        Ok(())
    }
}

impl fmt::Display for ArgumentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text => f.write_str("text"),
            Self::Bytes => f.write_str("bytes"),
            Self::Number => f.write_str("number"),
            Self::Flag => f.write_str("flag"),
        }
    }
}

impl fmt::Display for ArgumentTemplate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.optional {
            write!(f, "[{}: {}]", self.name, self.kind)
        } else {
            write!(f, "<{}: {}>", self.name, self.kind)
        }
    }
}

impl fmt::Display for Argument<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(value) => f.write_str(value),
            Self::Bytes(value) => write!(f, "{} bytes", value.len()),
            Self::Number(value) => write!(f, "{}", value),
            Self::Flag(value) => write!(f, "--{}", value),
        }
    }
}
