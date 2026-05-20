#![no_std]

pub mod arg;
pub mod callback;
pub mod cmd;
pub mod exec;
pub mod help;
pub mod job;
pub mod path;
pub mod pretty;
pub mod reg;
pub mod vessel;

pub const COMMAND_REGISTRY_CAPACITY: usize = 1;
pub const MAX_RUNNING_JOBS: usize = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Command {
    pub long: &'static str,
    pub short: &'static str,
    pub help: help::Help,
}

impl Command {
    pub const fn new(long: &'static str, short: &'static str, help: &'static str) -> Self {
        Self {
            long,
            short,
            help: help::Help::new(help),
        }
    }

    pub fn matches(self, name: &str) -> bool {
        self.long.as_bytes() == name.as_bytes() || self.short.as_bytes() == name.as_bytes()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReturnCodes {
    NotFound,
    Full,
    JobsFull,
}
