//! High-level Lilly command protocol and clip-boundary sequencer.
//!
//! Producers name an animation and tint; they never enqueue archive paths,
//! numbered frames, posture changes, or return-to-idle clips. The single
//! reader calls [`next_animation`] only when it is ready to begin another
//! complete four-frame clip. That boundary is the state-machine clock.

use spin::Mutex;

use super::lilly::{self, LillyPose, LillyResidentAnimation};

const COMMAND_RING_CAPACITY: usize = 32;
const CROSSED_IDLE: &str = "idle.crossed.soft_blink";
const UNCROSSED_IDLE: &str = "idle.uncrossed.soft_blink";
const CROSS_ARMS_TRANSITION: &str = "transition.neutral_to_crossed";
const UNCROSS_ARMS_TRANSITION: &str = "transition.uncross_arms";

/// Straight RGBA modulation supplied by the producer. The eventual sprite
/// compositor applies alpha while preserving the resident premultiplied-RGBA8
/// pixel contract.
#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct LillyRgba8 {
    pub(crate) red: u8,
    pub(crate) green: u8,
    pub(crate) blue: u8,
    pub(crate) alpha: u8,
}

impl LillyRgba8 {
    pub(crate) const WHITE: Self = Self::rgba(0xFF, 0xFF, 0xFF, 0xFF);

    pub(crate) const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }
}

/// The intentionally small producer protocol. More high-level intents can be
/// added as variants later without changing the ring or the animation reader.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum LillyCommand {
    Animate {
        rgba: LillyRgba8,
        /// Semantic catalog key, for example `wave.shy`.
        tag: &'static str,
    },
}

impl LillyCommand {
    const fn rgba(self) -> LillyRgba8 {
        match self {
            Self::Animate { rgba, .. } => rgba,
        }
    }

    const fn tag(self) -> &'static str {
        match self {
            Self::Animate { tag, .. } => tag,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum LillyProtocolError {
    AssetsNotReady,
    UnknownAnimation,
    RingFull,
    CatalogInvariant,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum LillySequenceSource {
    Requested,
    AutomaticTransition,
    AutomaticIdle,
}

/// One fully resolved clip for the presentation side. Its four resident GPU
/// surfaces and timing come from the central helper; there is no per-animation
/// playback code in the consumer.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct LillyScheduledAnimation {
    pub(crate) rgba: LillyRgba8,
    pub(crate) source: LillySequenceSource,
    /// The sequencer boundary. Even a catalog `Loop` yields after one cycle so
    /// newly queued behavior cannot be starved behind an infinite idle/talk.
    pub(crate) boundary_ms: u64,
    pub(crate) animation: LillyResidentAnimation,
}

struct LillyCommandRing {
    slots: [Option<LillyCommand>; COMMAND_RING_CAPACITY],
    read: usize,
    write: usize,
    len: usize,
}

impl LillyCommandRing {
    const fn new() -> Self {
        Self {
            slots: [None; COMMAND_RING_CAPACITY],
            read: 0,
            write: 0,
            len: 0,
        }
    }

    fn push(&mut self, command: LillyCommand) -> Result<usize, LillyProtocolError> {
        if self.len == COMMAND_RING_CAPACITY {
            return Err(LillyProtocolError::RingFull);
        }
        self.slots[self.write] = Some(command);
        self.write = (self.write + 1) % COMMAND_RING_CAPACITY;
        self.len += 1;
        Ok(self.len)
    }

    fn pop(&mut self) -> Option<LillyCommand> {
        let command = self.slots[self.read].take()?;
        self.read = (self.read + 1) % COMMAND_RING_CAPACITY;
        self.len -= 1;
        Some(command)
    }
}

struct LillySequenceState {
    current_pose: LillyPose,
    pending_after_transition: Option<LillyCommand>,
    return_idle: Option<LillyRgba8>,
    last_rgba: LillyRgba8,
}

impl LillySequenceState {
    const fn new() -> Self {
        Self {
            // The split static still is the deterministic cold/fallback pose.
            current_pose: LillyPose::CrossedArms,
            pending_after_transition: None,
            return_idle: None,
            last_rgba: LillyRgba8::WHITE,
        }
    }
}

static COMMAND_RING: Mutex<LillyCommandRing> = Mutex::new(LillyCommandRing::new());
static SEQUENCE: Mutex<LillySequenceState> = Mutex::new(LillySequenceState::new());

/// Queue a semantic animation request. Validation happens before publication,
/// so the reader never encounters a typo in an animation tag.
#[allow(dead_code)]
pub(crate) fn enqueue(command: LillyCommand) -> Result<usize, LillyProtocolError> {
    if lilly::resident_frame_count() == 0 {
        return Err(LillyProtocolError::AssetsNotReady);
    }
    if lilly::resident_animation(command.tag()).is_none() {
        return Err(LillyProtocolError::UnknownAnimation);
    }
    COMMAND_RING.lock().push(command)
}

/// Convenience producer entry point for the protocol's current sole command.
#[allow(dead_code)]
pub(crate) fn enqueue_animation(
    tag: &'static str,
    rgba: LillyRgba8,
) -> Result<usize, LillyProtocolError> {
    enqueue(LillyCommand::Animate { rgba, tag })
}

/// Resolve the next whole clip at an animation boundary.
///
/// A pose-changing transition and a posture-matched idle are automatically
/// inserted around producer requests. With no producer work, the matching idle
/// remains alive. This function is deliberately not a per-frame iterator: the
/// renderer uses `animation.frames` and `frame_period_ms` for that cheap inner
/// loop, then asks here again only when the clip is complete.
#[allow(dead_code)]
pub(crate) fn next_animation() -> Result<LillyScheduledAnimation, LillyProtocolError> {
    let mut sequence = SEQUENCE.lock();

    if let Some(command) = sequence.pending_after_transition.take() {
        return schedule_requested(&mut sequence, command);
    }
    if let Some(rgba) = sequence.return_idle.take() {
        return resolve(idle_for(sequence.current_pose), rgba, LillySequenceSource::AutomaticIdle);
    }

    let Some(command) = COMMAND_RING.lock().pop() else {
        return resolve(
            idle_for(sequence.current_pose),
            sequence.last_rgba,
            LillySequenceSource::AutomaticIdle,
        );
    };
    let requested =
        lilly::resident_animation(command.tag()).ok_or(LillyProtocolError::CatalogInvariant)?;
    if requested.entry_pose != sequence.current_pose {
        let transition_tag = transition_for(sequence.current_pose, requested.entry_pose)
            .ok_or(LillyProtocolError::CatalogInvariant)?;
        let transition = lilly::resident_animation(transition_tag)
            .ok_or(LillyProtocolError::CatalogInvariant)?;
        if transition.entry_pose != sequence.current_pose
            || transition.exit_pose != requested.entry_pose
        {
            return Err(LillyProtocolError::CatalogInvariant);
        }
        sequence.pending_after_transition = Some(command);
        sequence.current_pose = transition.exit_pose;
        sequence.last_rgba = command.rgba();
        return Ok(LillyScheduledAnimation {
            rgba: command.rgba(),
            source: LillySequenceSource::AutomaticTransition,
            boundary_ms: transition.cycle_duration_ms(),
            animation: transition,
        });
    }
    schedule_requested(&mut sequence, command)
}

fn schedule_requested(
    sequence: &mut LillySequenceState,
    command: LillyCommand,
) -> Result<LillyScheduledAnimation, LillyProtocolError> {
    let animation =
        lilly::resident_animation(command.tag()).ok_or(LillyProtocolError::CatalogInvariant)?;
    if animation.entry_pose != sequence.current_pose {
        return Err(LillyProtocolError::CatalogInvariant);
    }
    sequence.current_pose = animation.exit_pose;
    sequence.return_idle = Some(command.rgba());
    sequence.last_rgba = command.rgba();
    Ok(LillyScheduledAnimation {
        rgba: command.rgba(),
        source: LillySequenceSource::Requested,
        boundary_ms: animation.cycle_duration_ms(),
        animation,
    })
}

fn resolve(
    tag: &'static str,
    rgba: LillyRgba8,
    source: LillySequenceSource,
) -> Result<LillyScheduledAnimation, LillyProtocolError> {
    let animation = lilly::resident_animation(tag).ok_or_else(|| {
        if lilly::resident_frame_count() == 0 {
            LillyProtocolError::AssetsNotReady
        } else {
            LillyProtocolError::CatalogInvariant
        }
    })?;
    Ok(LillyScheduledAnimation {
        rgba,
        source,
        boundary_ms: animation.cycle_duration_ms(),
        animation,
    })
}

const fn idle_for(pose: LillyPose) -> &'static str {
    match pose {
        LillyPose::CrossedArms => CROSSED_IDLE,
        LillyPose::UncrossedArms => UNCROSSED_IDLE,
    }
}

const fn transition_for(from: LillyPose, to: LillyPose) -> Option<&'static str> {
    match (from, to) {
        (LillyPose::CrossedArms, LillyPose::UncrossedArms) => Some(UNCROSS_ARMS_TRANSITION),
        (LillyPose::UncrossedArms, LillyPose::CrossedArms) => Some(CROSS_ARMS_TRANSITION),
        _ => None,
    }
}
