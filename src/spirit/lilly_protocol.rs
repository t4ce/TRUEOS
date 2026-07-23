//! High-level Lilly command protocol and clip-boundary sequencer.
//!
//! Producers enqueue owned, JSON-compatible Spirit packages. Text and gesture
//! are independent optional fields; producers never enqueue archive paths,
//! numbered frames, posture changes, or return-to-idle clips. The single reader
//! calls [`next_animation`] only when it is ready to begin another complete
//! four-frame clip. That boundary is the state-machine clock.

extern crate alloc;

use alloc::string::String;
use serde::{Deserialize, Serialize};
use spin::Mutex;

use super::lilly::{self, LillyPose, LillyResidentAnimation};

const PACKAGE_RING_CAPACITY: usize = 32;
const PACKAGE_VERSION: u8 = 1;
const MAX_JSON_BYTES: usize = 4 * 1024;
const MAX_TEXT_BYTES: usize = 2 * 1024;
const MAX_TAG_BYTES: usize = 96;
const CROSSED_IDLE: &str = "idle.crossed.soft_blink";
const UNCROSSED_IDLE: &str = "idle.uncrossed.soft_blink";
const CROSS_ARMS_TRANSITION: &str = "transition.neutral_to_crossed";
const UNCROSS_ARMS_TRANSITION: &str = "transition.uncross_arms";

/// Straight RGBA modulation supplied by the producer. The eventual sprite
/// compositor applies alpha while preserving the resident premultiplied-RGBA8
/// pixel contract.
#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SpiritRgba8 {
    pub(crate) red: u8,
    pub(crate) green: u8,
    pub(crate) blue: u8,
    pub(crate) alpha: u8,
}

impl SpiritRgba8 {
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

impl Default for SpiritRgba8 {
    fn default() -> Self {
        Self::WHITE
    }
}

/// Optional visual portion of a Spirit package.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct SpiritGesture {
    /// Semantic Lilly catalog key, for example `wave.shy`.
    pub(crate) tag: String,
    #[serde(default)]
    pub(crate) rgba: SpiritRgba8,
}

/// Versioned package stored in the ring. Its JSON shape is intentionally flat
/// at the top level and keeps future text presentation independent of gesture:
/// `{ "version":1, "text":"Hello", "gesture":{"tag":"wave.shy",
/// "rgba":{"red":255,"green":255,"blue":255,"alpha":255}} }`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct SpiritPackage {
    #[serde(default = "current_package_version")]
    pub(crate) version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) gesture: Option<SpiritGesture>,
}

impl SpiritPackage {
    pub(crate) fn new(text: Option<String>, gesture: Option<SpiritGesture>) -> Self {
        Self {
            version: PACKAGE_VERSION,
            text,
            gesture,
        }
    }
}

const fn current_package_version() -> u8 {
    PACKAGE_VERSION
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum LillyProtocolError {
    AssetsNotReady,
    InvalidJson,
    UnsupportedVersion,
    EmptyPackage,
    PackageTooLarge,
    TextTooLong,
    TagTooLong,
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
    pub(crate) rgba: SpiritRgba8,
    pub(crate) source: LillySequenceSource,
    /// The sequencer boundary. Even a catalog `Loop` yields after one cycle so
    /// newly queued behavior cannot be starved behind an infinite idle/talk.
    pub(crate) boundary_ms: u64,
    pub(crate) animation: LillyResidentAnimation,
}

struct SpiritPackageRing {
    slots: [Option<SpiritPackage>; PACKAGE_RING_CAPACITY],
    read: usize,
    write: usize,
    len: usize,
}

impl SpiritPackageRing {
    const fn new() -> Self {
        Self {
            slots: [const { None }; PACKAGE_RING_CAPACITY],
            read: 0,
            write: 0,
            len: 0,
        }
    }

    fn push(&mut self, package: SpiritPackage) -> Result<usize, LillyProtocolError> {
        if self.len == PACKAGE_RING_CAPACITY {
            return Err(LillyProtocolError::RingFull);
        }
        self.slots[self.write] = Some(package);
        self.write = (self.write + 1) % PACKAGE_RING_CAPACITY;
        self.len += 1;
        Ok(self.len)
    }

    fn pop(&mut self) -> Option<(SpiritPackage, usize)> {
        let package = self.slots[self.read].take()?;
        self.read = (self.read + 1) % PACKAGE_RING_CAPACITY;
        self.len -= 1;
        Some((package, self.len))
    }
}

struct LillySequenceState {
    current_pose: LillyPose,
    pending_after_transition: Option<SpiritGesture>,
    return_idle: Option<SpiritRgba8>,
    last_rgba: SpiritRgba8,
}

impl LillySequenceState {
    const fn new() -> Self {
        Self {
            // The split static still is the deterministic cold/fallback pose.
            current_pose: LillyPose::CrossedArms,
            pending_after_transition: None,
            return_idle: None,
            last_rgba: SpiritRgba8::WHITE,
        }
    }
}

static PACKAGE_RING: Mutex<SpiritPackageRing> = Mutex::new(SpiritPackageRing::new());
static SEQUENCE: Mutex<LillySequenceState> = Mutex::new(LillySequenceState::new());

/// Queue one owned API package. Validation happens before publication, so the
/// reader never encounters an unsupported schema or a bad animation tag.
#[allow(dead_code)]
pub(crate) fn enqueue_package(package: SpiritPackage) -> Result<usize, LillyProtocolError> {
    validate_package(&package)?;
    PACKAGE_RING.lock().push(package)
}

/// JSON-facing ingress for an eventual service/API endpoint.
#[allow(dead_code)]
pub(crate) fn enqueue_json(json: &str) -> Result<usize, LillyProtocolError> {
    if json.len() > MAX_JSON_BYTES {
        return Err(LillyProtocolError::PackageTooLarge);
    }
    let package =
        serde_json::from_str::<SpiritPackage>(json).map_err(|_| LillyProtocolError::InvalidJson)?;
    enqueue_package(package)
}

fn validate_package(package: &SpiritPackage) -> Result<(), LillyProtocolError> {
    if package.version != PACKAGE_VERSION {
        return Err(LillyProtocolError::UnsupportedVersion);
    }
    if package
        .text
        .as_ref()
        .is_some_and(|text| text.len() > MAX_TEXT_BYTES)
    {
        return Err(LillyProtocolError::TextTooLong);
    }
    if package.text.as_ref().is_none_or(String::is_empty) && package.gesture.is_none() {
        return Err(LillyProtocolError::EmptyPackage);
    }
    let Some(gesture) = package.gesture.as_ref() else {
        return Ok(());
    };
    if gesture.tag.len() > MAX_TAG_BYTES {
        return Err(LillyProtocolError::TagTooLong);
    }
    if lilly::resident_frame_count() == 0 {
        return Err(LillyProtocolError::AssetsNotReady);
    }
    if lilly::resident_animation(gesture.tag.as_str()).is_none() {
        return Err(LillyProtocolError::UnknownAnimation);
    }
    Ok(())
}

/// Convenience wrapper for an animation-only producer.
#[allow(dead_code)]
pub(crate) fn enqueue_animation(tag: &str, rgba: SpiritRgba8) -> Result<usize, LillyProtocolError> {
    enqueue_package(SpiritPackage::new(
        None,
        Some(SpiritGesture {
            tag: String::from(tag),
            rgba,
        }),
    ))
}

/// Convenience wrapper for text-only packages. Text is logged and discarded
/// by the current reader until the visual text presenter is connected.
#[allow(dead_code)]
pub(crate) fn enqueue_text(text: &str) -> Result<usize, LillyProtocolError> {
    enqueue_package(SpiritPackage::new(Some(String::from(text)), None))
}

pub(super) fn has_queued_packages() -> bool {
    PACKAGE_RING.lock().len != 0
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

    if let Some(gesture) = sequence.pending_after_transition.take() {
        return schedule_requested(&mut sequence, gesture);
    }
    if let Some(rgba) = sequence.return_idle.take() {
        return resolve(idle_for(sequence.current_pose), rgba, LillySequenceSource::AutomaticIdle);
    }

    loop {
        let Some((package, ring_remaining)) = PACKAGE_RING.lock().pop() else {
            return resolve(
                idle_for(sequence.current_pose),
                sequence.last_rgba,
                LillySequenceSource::AutomaticIdle,
            );
        };
        log_drained_package(&package, ring_remaining);
        let Some(gesture) = package.gesture else {
            // Text has reached Spirit and was logged above. Keep draining so a
            // later gesture in the same reader turn can flow normally.
            continue;
        };
        let requested = lilly::resident_animation(gesture.tag.as_str())
            .ok_or(LillyProtocolError::CatalogInvariant)?;
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
            let rgba = gesture.rgba;
            sequence.pending_after_transition = Some(gesture);
            sequence.current_pose = transition.exit_pose;
            sequence.last_rgba = rgba;
            return Ok(LillyScheduledAnimation {
                rgba,
                source: LillySequenceSource::AutomaticTransition,
                boundary_ms: transition.cycle_duration_ms(),
                animation: transition,
            });
        }
        return schedule_requested(&mut sequence, gesture);
    }
}

fn schedule_requested(
    sequence: &mut LillySequenceState,
    gesture: SpiritGesture,
) -> Result<LillyScheduledAnimation, LillyProtocolError> {
    let animation = lilly::resident_animation(gesture.tag.as_str())
        .ok_or(LillyProtocolError::CatalogInvariant)?;
    if animation.entry_pose != sequence.current_pose {
        return Err(LillyProtocolError::CatalogInvariant);
    }
    sequence.current_pose = animation.exit_pose;
    sequence.return_idle = Some(gesture.rgba);
    sequence.last_rgba = gesture.rgba;
    Ok(LillyScheduledAnimation {
        rgba: gesture.rgba,
        source: LillySequenceSource::Requested,
        boundary_ms: animation.cycle_duration_ms(),
        animation,
    })
}

fn log_drained_package(package: &SpiritPackage, ring_remaining: usize) {
    let gesture_tag = package.gesture.as_ref().map(|gesture| gesture.tag.as_str());
    let rgba = package.gesture.as_ref().map(|gesture| gesture.rgba);
    crate::log_info!(
        target: "gfx";
        "trueos-spirit: package received/drained version={} text={:?} gesture={:?} rgba={:?} ring_remaining={} text_action={} gesture_action={}\n",
        package.version,
        package.text.as_deref(),
        gesture_tag,
        rgba,
        ring_remaining,
        if package.text.is_some() { "log-only" } else { "none" },
        if package.gesture.is_some() { "sequence" } else { "none" },
    );
}

fn resolve(
    tag: &'static str,
    rgba: SpiritRgba8,
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
