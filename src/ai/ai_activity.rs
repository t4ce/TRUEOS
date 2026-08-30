//! Decoupled publication of AI turn activity.
//!
//! Model services publish generic lifecycle edges here. Presentation systems
//! may consume those edges without either side depending on the other.

use core::sync::atomic::{AtomicU32, Ordering};

use heapless::Deque;
use spin::Mutex;

const AI_ACTIVITY_EVENT_CAPACITY: usize = 64;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum AiActivitySource {
    Lumen,
}

impl AiActivitySource {
    const fn name(self) -> &'static str {
        match self {
            Self::Lumen => "lumen",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum AiReasoningPhase {
    Started,
    Finished,
}

impl AiReasoningPhase {
    const fn name(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Finished => "finished",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct AiReasoningEvent {
    pub(crate) source: AiActivitySource,
    pub(crate) turn: u64,
    pub(crate) phase: AiReasoningPhase,
}

static ACTIVE_REASONING_COUNT: AtomicU32 = AtomicU32::new(0);
static AI_ACTIVITY_EVENTS: Mutex<Deque<AiReasoningEvent, AI_ACTIVITY_EVENT_CAPACITY>> =
    Mutex::new(Deque::new());

#[must_use = "keep the guard alive for the whole AI turn"]
pub(crate) struct AiReasoningGuard {
    source: AiActivitySource,
    turn: u64,
    active: bool,
}

impl AiReasoningGuard {
    pub(crate) fn finish(mut self) {
        self.publish_finished();
    }

    fn publish_finished(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        publish(AiReasoningEvent {
            source: self.source,
            turn: self.turn,
            phase: AiReasoningPhase::Finished,
        });
        ACTIVE_REASONING_COUNT.fetch_sub(1, Ordering::AcqRel);
    }
}

impl Drop for AiReasoningGuard {
    fn drop(&mut self) {
        self.publish_finished();
    }
}

pub(crate) fn begin_reasoning(source: AiActivitySource, turn: u64) -> AiReasoningGuard {
    ACTIVE_REASONING_COUNT.fetch_add(1, Ordering::AcqRel);
    publish(AiReasoningEvent {
        source,
        turn,
        phase: AiReasoningPhase::Started,
    });
    AiReasoningGuard {
        source,
        turn,
        active: true,
    }
}

#[inline]
pub(crate) fn reasoning_active() -> bool {
    ACTIVE_REASONING_COUNT.load(Ordering::Acquire) != 0
}

pub(crate) fn try_take_reasoning_event() -> Option<AiReasoningEvent> {
    AI_ACTIVITY_EVENTS.lock().pop_front()
}

fn publish(event: AiReasoningEvent) {
    let queued = AI_ACTIVITY_EVENTS.lock().push_back(event).is_ok();
    crate::log_info!(
        target: "service";
        "ai-activity: source={} turn={} phase={} queued={}\n",
        event.source.name(),
        event.turn,
        event.phase.name(),
        queued,
    );
    if !queued {
        crate::log_warn!(
            target: "service";
            "ai-activity: event queue full capacity={} source={} turn={} phase={} action=drop\n",
            AI_ACTIVITY_EVENT_CAPACITY,
            event.source.name(),
            event.turn,
            event.phase.name(),
        );
    }
}
