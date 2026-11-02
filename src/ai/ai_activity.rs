//! Passive reasoning observer contract while local inference is unavailable.
//! Speech activity continues through the audio/TTSTT presentation path.

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[expect(dead_code, reason = "local reasoning producers are retired")]
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
