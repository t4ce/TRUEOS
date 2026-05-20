use crate::arg::{ArgumentKind, ArgumentTemplate};
use crate::Command;

pub const MOVE: Command = Command::new("move", "mv", "move a value from source to destination");
pub const REMOVE: Command = Command::new("remove", "rm", "remove a value");
pub const LIST: Command = Command::new("list", "li", "list values");
pub const COPY: Command = Command::new("copy", "cp", "copy a value from source to destination");
pub const ENVIRONMENT: Command = Command::new("environment", "env", "show or change environment");
pub const COMMAND: Command = Command::new("command", "cmd", "list commands or show command help");

pub const COMMANDS: &[Command] = &[MOVE, REMOVE, LIST, COPY, ENVIRONMENT, COMMAND];

pub const COMMAND_ARGUMENTS: &[ArgumentTemplate] =
    &[ArgumentTemplate::optional("command", ArgumentKind::Text)];

pub fn find(name: &str) -> Option<Command> {
    let mut index = 0;
    while index < COMMANDS.len() {
        let command = COMMANDS[index];
        if command.matches(name) {
            return Some(command);
        }
        index += 1;
    }
    None
}

pub fn help(name: &str) -> Option<crate::help::Help> {
    find(name).map(|command| command.help)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandList {
    pub commands: &'static [Command],
}

impl CommandList {
    pub const fn new(commands: &'static [Command]) -> Self {
        Self { commands }
    }
}

pub const LIST_COMMANDS: CommandList = CommandList::new(COMMANDS);
