use crate::callback::CommandFuture;
use crate::{Command, ReturnCodes, MAX_RUNNING_JOBS};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JobId(pub u64);

pub struct CommandJob<'a> {
    pub id: JobId,
    pub command: Command,
    future: CommandFuture<'a>,
}

impl<'a> CommandJob<'a> {
    pub const fn new(id: JobId, command: Command, future: CommandFuture<'a>) -> Self {
        Self {
            id,
            command,
            future,
        }
    }

    pub fn future(self) -> CommandFuture<'a> {
        self.future
    }
}

pub struct JobQueue<'a, const CAPACITY: usize = MAX_RUNNING_JOBS> {
    jobs: [Option<CommandJob<'a>>; CAPACITY],
    next_id: u64,
}

impl<'a, const CAPACITY: usize> JobQueue<'a, CAPACITY> {
    pub fn new() -> Self {
        Self {
            jobs: [const { None }; CAPACITY],
            next_id: 1,
        }
    }

    pub fn push(&mut self, command: Command, future: CommandFuture<'a>) -> Result<JobId, ReturnCodes> {
        let mut index = 0;
        while index < self.jobs.len() {
            if self.jobs[index].is_none() {
                let id = JobId(self.next_id);
                self.next_id = self.next_id.wrapping_add(1).max(1);
                self.jobs[index] = Some(CommandJob::new(id, command, future));
                return Ok(id);
            }
            index += 1;
        }
        Err(ReturnCodes::JobsFull)
    }

    pub fn take(&mut self, id: JobId) -> Option<CommandJob<'a>> {
        let mut index = 0;
        while index < self.jobs.len() {
            if self.jobs[index]
                .as_ref()
                .map(|job| job.id == id)
                .unwrap_or(false)
            {
                return self.jobs[index].take();
            }
            index += 1;
        }
        None
    }
}

impl<'a, const CAPACITY: usize> Default for JobQueue<'a, CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}
