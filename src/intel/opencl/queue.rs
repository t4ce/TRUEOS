extern crate alloc;

use alloc::{collections::VecDeque, vec::Vec};

use super::types;

#[derive(Debug)]
pub(crate) enum CommandKind {
    WriteBuffer {
        mem: types::MemId,
        offset: usize,
        bytes: Vec<u8>,
    },
    ReadBuffer {
        mem: types::MemId,
        offset: usize,
        byte_len: usize,
    },
    Kernel {
        kernel: types::KernelId,
        nd_range: types::NdRange,
    },
}

impl CommandKind {
    pub(crate) fn byte_len(&self) -> usize {
        match self {
            Self::WriteBuffer { bytes, .. } => bytes.len(),
            Self::ReadBuffer { byte_len, .. } => *byte_len,
            Self::Kernel { .. } => 0,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Command {
    pub(crate) event: types::EventId,
    pub(crate) kind: CommandKind,
    pub(crate) wait_for: Vec<types::EventId>,
    pub(crate) sequence: u64,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum EventStatus {
    Queued,
    Submitted,
    Running,
    Complete,
    Failed(i32),
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct EventRecord {
    pub(crate) event: types::EventId,
    pub(crate) queue: types::QueueId,
    pub(crate) status: EventStatus,
    pub(crate) sequence: u64,
}

#[derive(Debug)]
pub(crate) struct CommandQueue {
    pub(crate) id: types::QueueId,
    pub(crate) context: types::ContextId,
    pub(crate) device: types::DeviceId,
    pub(crate) properties: types::QueueProperties,
    pending: VecDeque<Command>,
    events: Vec<EventRecord>,
    next_sequence: u64,
}

impl CommandQueue {
    pub(crate) fn new(
        id: types::QueueId,
        context: types::ContextId,
        device: types::DeviceId,
        properties: types::QueueProperties,
    ) -> Self {
        Self {
            id,
            context,
            device,
            properties,
            pending: VecDeque::new(),
            events: Vec::new(),
            next_sequence: 0,
        }
    }

    pub(crate) fn enqueue_write_buffer(
        &mut self,
        event: types::EventId,
        mem: types::MemId,
        offset: usize,
        bytes: &[u8],
        wait_for: &[types::EventId],
    ) -> types::ClResult<types::EventId> {
        self.enqueue(
            event,
            CommandKind::WriteBuffer {
                mem,
                offset,
                bytes: bytes.to_vec(),
            },
            wait_for,
        )
    }

    pub(crate) fn enqueue_read_buffer(
        &mut self,
        event: types::EventId,
        mem: types::MemId,
        offset: usize,
        byte_len: usize,
        wait_for: &[types::EventId],
    ) -> types::ClResult<types::EventId> {
        self.enqueue(
            event,
            CommandKind::ReadBuffer {
                mem,
                offset,
                byte_len,
            },
            wait_for,
        )
    }

    pub(crate) fn enqueue_kernel(
        &mut self,
        event: types::EventId,
        kernel: types::KernelId,
        nd_range: types::NdRange,
        wait_for: &[types::EventId],
    ) -> types::ClResult<types::EventId> {
        self.enqueue(event, CommandKind::Kernel { kernel, nd_range }, wait_for)
    }

    pub(crate) fn finish_with<F>(&mut self, mut backend: F) -> types::ClResult<usize>
    where
        F: FnMut(&Command) -> types::ClResult<()>,
    {
        let mut completed = 0usize;

        while let Some(command) = self.pending.pop_front() {
            self.set_event_status(command.event, EventStatus::Submitted);
            self.set_event_status(command.event, EventStatus::Running);

            if let Err(err) = backend(&command) {
                self.set_event_status(command.event, EventStatus::Failed(-1));
                return Err(err);
            }

            self.set_event_status(command.event, EventStatus::Complete);
            completed = completed.saturating_add(1);
        }

        Ok(completed)
    }

    pub(crate) fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub(crate) fn event_count(&self) -> usize {
        self.events.len()
    }

    pub(crate) fn events(&self) -> &[EventRecord] {
        self.events.as_slice()
    }

    pub(crate) fn event(&self, event: types::EventId) -> Option<&EventRecord> {
        self.events.iter().find(|record| record.event == event)
    }

    fn enqueue(
        &mut self,
        event: types::EventId,
        kind: CommandKind,
        wait_for: &[types::EventId],
    ) -> types::ClResult<types::EventId> {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);

        self.events.push(EventRecord {
            event,
            queue: self.id,
            status: EventStatus::Queued,
            sequence,
        });
        self.pending.push_back(Command {
            event,
            kind,
            wait_for: wait_for.to_vec(),
            sequence,
        });

        Ok(event)
    }

    fn set_event_status(&mut self, event: types::EventId, status: EventStatus) {
        if let Some(record) = self.events.iter_mut().find(|record| record.event == event) {
            record.status = status;
        }
    }
}
