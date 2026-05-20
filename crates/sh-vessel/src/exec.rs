use crate::arg::Argument;
use crate::job::{JobId, JobQueue, JobTimeout};
use crate::reg::CommandRegistry;
use crate::ReturnCodes;

pub fn execute<'a, const COMMANDS: usize, const JOBS: usize>(
    registry: &CommandRegistry<COMMANDS>,
    jobs: &mut JobQueue<'a, JOBS>,
    name: &str,
    arguments: &'a [Argument<'a>],
) -> Result<JobId, ReturnCodes> {
    execute_with_timeout(registry, jobs, name, arguments, None)
}

pub fn execute_with_timeout<'a, const COMMANDS: usize, const JOBS: usize>(
    registry: &CommandRegistry<COMMANDS>,
    jobs: &mut JobQueue<'a, JOBS>,
    name: &str,
    arguments: &'a [Argument<'a>],
    timeout: Option<JobTimeout>,
) -> Result<JobId, ReturnCodes> {
    let command = registry.find(name).ok_or(ReturnCodes::NotFound)?;
    jobs.push_with_timeout(command.command, timeout, command.callback.call(arguments))
}
