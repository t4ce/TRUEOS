use crate::arg::Argument;
use crate::callback::{CommandCallback, CommandResult};
use crate::exec;
use crate::job::{JobId, JobQueue};
use crate::reg::CommandRegistry;
use crate::{Command, ReturnCodes, COMMAND_REGISTRY_CAPACITY, MAX_RUNNING_JOBS};

pub struct Vessel<'a, const COMMANDS: usize = COMMAND_REGISTRY_CAPACITY, const JOBS: usize = MAX_RUNNING_JOBS> {
    registry: CommandRegistry<COMMANDS>,
    jobs: JobQueue<'a, JOBS>,
}

impl<'a, const COMMANDS: usize, const JOBS: usize> Vessel<'a, COMMANDS, JOBS> {
    pub fn new() -> Self {
        Self {
            registry: CommandRegistry::new(),
            jobs: JobQueue::new(),
        }
    }

    pub fn register(&mut self, command: Command, callback: CommandCallback) -> CommandResult {
        self.registry.register(command, callback)
    }

    pub fn execute(
        &mut self,
        name: &str,
        arguments: &'a [Argument<'a>],
    ) -> Result<JobId, ReturnCodes> {
        exec::execute(&self.registry, &mut self.jobs, name, arguments)
    }
}

impl<'a, const COMMANDS: usize, const JOBS: usize> Default for Vessel<'a, COMMANDS, JOBS> {
    fn default() -> Self {
        Self::new()
    }
}
