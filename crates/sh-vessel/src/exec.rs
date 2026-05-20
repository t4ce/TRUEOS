use crate::arg::Argument;
use crate::job::{JobId, JobQueue};
use crate::reg::CommandRegistry;
use crate::ReturnCodes;

pub fn execute<'a, const COMMANDS: usize, const JOBS: usize>(
    registry: &CommandRegistry<COMMANDS>,
    jobs: &mut JobQueue<'a, JOBS>,
    name: &str,
    arguments: &'a [Argument<'a>],
) -> Result<JobId, ReturnCodes> {
    let command = registry.find(name).ok_or(ReturnCodes::NotFound)?;
    jobs.push(command.command, command.callback.call(arguments))
}
