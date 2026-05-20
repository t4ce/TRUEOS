use crate::callback::{CommandCallback, CommandResult};
use crate::{Command, ReturnCodes, COMMAND_REGISTRY_CAPACITY};

#[derive(Clone, Copy)]
pub struct RegisteredCommand {
    pub command: Command,
    pub callback: CommandCallback,
}

impl RegisteredCommand {
    pub const fn new(command: Command, callback: CommandCallback) -> Self {
        Self { command, callback }
    }
}

pub struct CommandRegistry<const CAPACITY: usize = COMMAND_REGISTRY_CAPACITY> {
    commands: [Option<RegisteredCommand>; CAPACITY],
}

impl<const CAPACITY: usize> CommandRegistry<CAPACITY> {
    pub const fn new() -> Self {
        Self {
            commands: [None; CAPACITY],
        }
    }

    pub fn register(&mut self, command: Command, callback: CommandCallback) -> CommandResult {
        let mut index = 0;
        while index < self.commands.len() {
            if self.commands[index].is_none() {
                self.commands[index] = Some(RegisteredCommand::new(command, callback));
                return Ok(());
            }
            index += 1;
        }
        Err(ReturnCodes::Full)
    }

    pub fn find(&self, name: &str) -> Option<RegisteredCommand> {
        let mut index = 0;
        while index < self.commands.len() {
            if let Some(command) = self.commands[index] {
                if command.command.matches(name) {
                    return Some(command);
                }
            }
            index += 1;
        }
        None
    }

}

impl<const CAPACITY: usize> Default for CommandRegistry<CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}
