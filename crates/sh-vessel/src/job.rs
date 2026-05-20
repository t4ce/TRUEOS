use core::task::{Context, Poll};

use crate::callback::{CommandFuture, CommandOutcome, CommandResult};
use crate::{Command, ReturnCodes, MAX_RUNNING_JOBS};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JobId(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JobTimeout {
    pub value: u64,
}

impl JobTimeout {
    pub const fn new(value: u64) -> Self {
        Self { value }
    }
}

pub struct CommandJob<'a> {
    pub id: JobId,
    pub command: Command,
    pub timeout: Option<JobTimeout>,
    future: Option<CommandFuture<'a>>,
    result: Option<CommandResult>,
}

impl<'a> CommandJob<'a> {
    pub const fn pending(
        id: JobId,
        command: Command,
        timeout: Option<JobTimeout>,
        future: CommandFuture<'a>,
    ) -> Self {
        Self {
            id,
            command,
            timeout,
            future: Some(future),
            result: None,
        }
    }

    pub const fn ready(
        id: JobId,
        command: Command,
        timeout: Option<JobTimeout>,
        result: CommandResult,
    ) -> Self {
        Self {
            id,
            command,
            timeout,
            future: None,
            result: Some(result),
        }
    }

    pub fn future(self) -> Option<CommandFuture<'a>> {
        self.future
    }

    pub fn result(&self) -> Option<CommandResult> {
        self.result
    }

    pub fn poll(&mut self, context: &mut Context<'_>) -> Poll<CommandResult> {
        if let Some(result) = self.result {
            return Poll::Ready(result);
        }

        let Some(future) = self.future.as_mut() else {
            return Poll::Ready(Err(ReturnCodes::Failed));
        };

        match future.as_mut().poll(context) {
            Poll::Ready(result) => {
                self.future = None;
                self.result = Some(result);
                Poll::Ready(result)
            }
            Poll::Pending => Poll::Pending,
        }
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

    pub fn push(&mut self, command: Command, outcome: CommandOutcome<'a>) -> Result<JobId, ReturnCodes> {
        self.push_with_timeout(command, None, outcome)
    }

    pub fn push_with_timeout(
        &mut self,
        command: Command,
        timeout: Option<JobTimeout>,
        outcome: CommandOutcome<'a>,
    ) -> Result<JobId, ReturnCodes> {
        let mut index = 0;
        while index < self.jobs.len() {
            if self.jobs[index].is_none() {
                let id = JobId(self.next_id);
                self.next_id = self.next_id.wrapping_add(1).max(1);
                self.jobs[index] = Some(match outcome {
                    CommandOutcome::Future(future) => CommandJob::pending(id, command, timeout, future),
                    CommandOutcome::Ready(result) => CommandJob::ready(id, command, timeout, result),
                });
                return Ok(id);
            }
            index += 1;
        }
        Err(ReturnCodes::JobsFull)
    }

    pub fn clean(&mut self, id: JobId) -> Result<(), ReturnCodes> {
        self.take(id).map(|_| ()).ok_or(ReturnCodes::NotFound)
    }

    pub fn clean_all(&mut self) {
        let mut index = 0;
        while index < self.jobs.len() {
            self.jobs[index] = None;
            index += 1;
        }
    }

    pub fn poll(
        &mut self,
        id: JobId,
        context: &mut Context<'_>,
    ) -> Result<Poll<CommandResult>, ReturnCodes> {
        let mut index = 0;
        while index < self.jobs.len() {
            if let Some(job) = self.jobs[index].as_mut() {
                if job.id == id {
                    return Ok(job.poll(context));
                }
            }
            index += 1;
        }
        Err(ReturnCodes::NotFound)
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
