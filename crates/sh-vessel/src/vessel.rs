use core::task::{Context, Poll};

use crate::arg::Argument;
use crate::callback::{CommandCallback, CommandResult};
use crate::exec;
use crate::job::{JobId, JobQueue, JobTimeout};
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

    pub fn execute_with_timeout(
        &mut self,
        name: &str,
        arguments: &'a [Argument<'a>],
        timeout: Option<JobTimeout>,
    ) -> Result<JobId, ReturnCodes> {
        exec::execute_with_timeout(&self.registry, &mut self.jobs, name, arguments, timeout)
    }

    pub fn clean(&mut self, id: JobId) -> Result<(), ReturnCodes> {
        self.jobs.clean(id)
    }

    pub fn clean_all(&mut self) {
        self.jobs.clean_all();
    }

    pub fn poll_job(
        &mut self,
        id: JobId,
        context: &mut Context<'_>,
    ) -> Result<Poll<CommandResult>, ReturnCodes> {
        self.jobs.poll(id, context)
    }
}

impl<'a, const COMMANDS: usize, const JOBS: usize> Default for Vessel<'a, COMMANDS, JOBS> {
    fn default() -> Self {
        Self::new()
    }
}
