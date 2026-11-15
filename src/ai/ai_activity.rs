//! Passive reasoning observer contract while local inference is unavailable.

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum AiReasoningPhase {
    Started,
    Finished,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct AiReasoningEvent {
    pub(crate) source: &'static str,
    pub(crate) turn: u64,
    pub(crate) phase: AiReasoningPhase,
}

#[inline]
pub(crate) fn reasoning_active() -> bool {
    false
}

pub(crate) fn try_take_reasoning_event() -> Option<AiReasoningEvent> {
    None
}
