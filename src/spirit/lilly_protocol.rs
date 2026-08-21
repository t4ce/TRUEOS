//! High-level Lilly command protocol and clip-boundary sequencer.
//!
//! Producers enqueue owned, JSON-compatible Spirit packages. Text and gesture
//! are independent optional fields; producers never enqueue archive paths,
//! numbered frames, posture changes, or transition rules. The single reader
//! calls [`next_animation`] only when it is ready to begin another complete
//! seven-frame clip. That boundary is also where a replacement timeline takes
//! effect.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use trueos_time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use spin::Mutex;

use super::lilly::{self, LillyResidentAnimation, LillyResidentPart};

const PACKAGE_RING_CAPACITY: usize = 32;
const PACKAGE_VERSION: u8 = 1;
const MAX_JSON_BYTES: usize = 4 * 1024;
const MAX_TEXT_BYTES: usize = 2 * 1024;
const MAX_TAG_BYTES: usize = 96;
const MIN_EMOTIONS_PER_SEQUENCE: usize = 1;
const MAX_EMOTIONS_PER_SEQUENCE: usize = 3;
const IDLE_UNCROSSED_SOFT_BLINK: &str = "idle.uncrossed.soft_blink";
const IDLE_CROSSED_SOFT_BLINK: &str = "idle.crossed.soft_blink";
const IDLE_CROSS_ARMS_TRANSITION: &str = "transition.neutral_to_crossed";
const IDLE_UNCROSS_ARMS_TRANSITION: &str = "transition.uncross_arms";
const AI_REASONING_START_ANIMATION: &str = "agree.firm_nod";
const AI_REASONING_FINISH_ANIMATION: &str = "idea.finger_up";
const LUMEN_TALK_ANIMATION: &str = "talk.calm.uncrossed";
const IDLE_WINK_VARIANTS: [&str; 5] = [
    "wink.playful",
    "flirt.finger_heart",
    "blush.evil",
    IDLE_UNCROSSED_SOFT_BLINK,
    IDLE_CROSSED_SOFT_BLINK,
];
const IDLE_CONTROL_POLL_MS: u64 = 100;
const IDLE_BLINK_AVERAGE_MS: u64 = 5_000;
const IDLE_BLINK_WINDOW_MS: u64 = 5_000;
const IDLE_ARMS_AVERAGE_MS: u64 = 15_000;
const IDLE_ARMS_WINDOW_MS: u64 = 30_000;
const IDLE_WINK_AVERAGE_MS: u64 = 60_000;
const IDLE_WINK_WINDOW_MS: u64 = 60_000;
const _: () = assert!(IDLE_BLINK_WINDOW_MS <= IDLE_BLINK_AVERAGE_MS * 2);
const _: () = assert!(IDLE_ARMS_WINDOW_MS <= IDLE_ARMS_AVERAGE_MS * 2);
const _: () = assert!(IDLE_WINK_WINDOW_MS <= IDLE_WINK_AVERAGE_MS * 2);

const ANGER_VARIANTS: [&str; 3] = [
    "angry.fists_clenched",
    "angry.fists_raised",
    "threaten.fists",
];
const DISGUST_VARIANTS: [&str; 4] = [
    "disapprove.head_shake",
    "disapprove.tsk",
    "disapprove.arms_crossed",
    "reaction.facepalm",
];
const FEAR_VARIANTS: [&str; 1] = ["scared.panic"];
const JOY_VARIANTS: [&str; 3] = ["happy.cheer", "idle.uncrossed.quiet_laugh", "silly.teehee"];
const SADNESS_VARIANTS: [&str; 3] = ["cry.two_hands", "cry.wipe", "cry.tears"];
// Lilly does not have an archive category literally named `Surprise`.
// Realization and puzzled surprise are the two catalog-native expressions of
// that base emotion, so the high-level API does not leak this asset detail.
const SURPRISE_VARIANTS: [&str; 2] = ["idea.finger_up", "confused.question"];

/// The deliberately small emotion vocabulary exposed to local reasoning.
///
/// Spirit owns the mapping from these base themes to Lilly's catalog. An AI
/// producer therefore emits only a word such as `anger`; it never needs to
/// know animation tags, archive names, frame counts, or which visual variant
/// was selected.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum LillyEmotion {
    Anger,
    Disgust,
    Fear,
    Joy,
    Sadness,
    Surprise,
}

impl LillyEmotion {
    const COUNT: usize = 6;

    /// Parse one model-facing base-emotion word. Common one-word inflections
    /// are accepted, but phrases and arbitrary animation tags are not.
    pub(crate) fn from_word(word: &str) -> Option<Self> {
        let word = word.trim();
        if word.eq_ignore_ascii_case("anger") || word.eq_ignore_ascii_case("angry") {
            Some(Self::Anger)
        } else if word.eq_ignore_ascii_case("disgust") || word.eq_ignore_ascii_case("disgusted") {
            Some(Self::Disgust)
        } else if word.eq_ignore_ascii_case("fear")
            || word.eq_ignore_ascii_case("afraid")
            || word.eq_ignore_ascii_case("scared")
        {
            Some(Self::Fear)
        } else if word.eq_ignore_ascii_case("joy")
            || word.eq_ignore_ascii_case("happy")
            || word.eq_ignore_ascii_case("happiness")
        {
            Some(Self::Joy)
        } else if word.eq_ignore_ascii_case("sad") || word.eq_ignore_ascii_case("sadness") {
            Some(Self::Sadness)
        } else if word.eq_ignore_ascii_case("surprise") || word.eq_ignore_ascii_case("surprised") {
            Some(Self::Surprise)
        } else {
            None
        }
    }

    pub(crate) const fn as_word(self) -> &'static str {
        match self {
            Self::Anger => "anger",
            Self::Disgust => "disgust",
            Self::Fear => "fear",
            Self::Joy => "joy",
            Self::Sadness => "sadness",
            Self::Surprise => "surprise",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }

    const fn variants(self) -> &'static [&'static str] {
        match self {
            Self::Anger => &ANGER_VARIANTS,
            Self::Disgust => &DISGUST_VARIANTS,
            Self::Fear => &FEAR_VARIANTS,
            Self::Joy => &JOY_VARIANTS,
            Self::Sadness => &SADNESS_VARIANTS,
            Self::Surprise => &SURPRISE_VARIANTS,
        }
    }
}

struct LillyEmotionSelector {
    rng: crate::tyche::SoftRng,
    last_variant: [Option<usize>; LillyEmotion::COUNT],
}

impl LillyEmotionSelector {
    fn new() -> Self {
        Self {
            rng: crate::tyche::soft_rng(),
            last_variant: [None; LillyEmotion::COUNT],
        }
    }

    fn select(&mut self, emotion: LillyEmotion) -> (&'static str, usize, usize) {
        let variants = emotion.variants();
        let emotion_index = emotion.index();
        let previous = self.last_variant[emotion_index];
        let variant_index = next_distinct_variant(&mut self.rng, previous, variants.len());
        self.last_variant[emotion_index] = Some(variant_index);
        (variants[variant_index], variant_index, variants.len())
    }
}

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
    EmotionCount,
    UnknownEmotion,
}

/// One fully resolved clip for the presentation side. Its resident GPU
/// surfaces and timing come from the central helper; there is no per-animation
/// playback code in the consumer.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct LillyScheduledAnimation {
    pub(crate) rgba: SpiritRgba8,
    /// The sequencer boundary. Even a catalog `Loop` yields after one cycle so
    /// newly queued behavior cannot be starved behind an infinite idle/talk.
    pub(crate) boundary_ms: u64,
    pub(crate) animation: LillyResidentAnimation,
    static_frame: Option<LillyResidentPart>,
}

impl LillyScheduledAnimation {
    pub(crate) fn frame_at_elapsed(self, elapsed_ms: u64) -> Option<LillyResidentPart> {
        self.static_frame
            .or_else(|| self.animation.frame_at_elapsed(elapsed_ms))
    }
}

/// Resident idle policy. The steady state is one eyes-open frame; Tyche only
/// chooses when an existing soft-blink clip runs and when the resident
/// crossed/uncrossed posture changes.
struct LillyIdleStrategy {
    rng: crate::tyche::SoftRng,
    active: bool,
    crossed: bool,
    pending_crossed: Option<bool>,
    next_blink: Instant,
    next_arms_change: Instant,
    next_wink: Instant,
    last_wink_variant: Option<usize>,
}

impl LillyIdleStrategy {
    fn new(now: Instant) -> Self {
        let mut rng = crate::tyche::soft_rng();
        let next_blink = now
            + Duration::from_millis(centered_interval_ms(
                &mut rng,
                IDLE_BLINK_AVERAGE_MS,
                IDLE_BLINK_WINDOW_MS,
            ));
        let next_arms_change = now
            + Duration::from_millis(centered_interval_ms(
                &mut rng,
                IDLE_ARMS_AVERAGE_MS,
                IDLE_ARMS_WINDOW_MS,
            ));
        let next_wink = now
            + Duration::from_millis(centered_interval_ms(
                &mut rng,
                IDLE_WINK_AVERAGE_MS,
                IDLE_WINK_WINDOW_MS,
            ));
        Self {
            rng,
            active: true,
            crossed: false,
            pending_crossed: None,
            next_blink,
            next_arms_change,
            next_wink,
            last_wink_variant: None,
        }
    }

    fn resume(&mut self, now: Instant) {
        if self.active {
            return;
        }
        self.active = true;
        self.next_blink = now
            + Duration::from_millis(centered_interval_ms(
                &mut self.rng,
                IDLE_BLINK_AVERAGE_MS,
                IDLE_BLINK_WINDOW_MS,
            ));
        self.next_arms_change = now
            + Duration::from_millis(centered_interval_ms(
                &mut self.rng,
                IDLE_ARMS_AVERAGE_MS,
                IDLE_ARMS_WINDOW_MS,
            ));
    }

    fn schedule(
        &mut self,
        now: Instant,
        rgba: SpiritRgba8,
    ) -> Result<LillyScheduledAnimation, LillyProtocolError> {
        self.resume(now);

        if let Some(crossed) = self.pending_crossed.take() {
            self.crossed = crossed;
            crate::log_info!(
                target: "gfx";
                "trueos-spirit: idle strategy event=arms-transition-complete posture={}\n",
                idle_posture_name(self.crossed),
            );
        }

        if now >= self.next_wink {
            let next_ms =
                centered_interval_ms(&mut self.rng, IDLE_WINK_AVERAGE_MS, IDLE_WINK_WINDOW_MS);
            self.next_wink = now + Duration::from_millis(next_ms);
            let variant = next_distinct_variant(
                &mut self.rng,
                self.last_wink_variant,
                IDLE_WINK_VARIANTS.len(),
            );
            self.last_wink_variant = Some(variant);
            let animation_key = IDLE_WINK_VARIANTS[variant];
            crate::log_info!(
                target: "gfx";
                "trueos-spirit: idle strategy event=wink variant={} clip={} next_average_ms={} next_window_ms={}\n",
                variant + 1,
                animation_key,
                IDLE_WINK_AVERAGE_MS,
                IDLE_WINK_WINDOW_MS,
            );
            return resolve(animation_key, rgba);
        }

        if now >= self.next_arms_change {
            let target_crossed = !self.crossed;
            let transition_key = idle_transition_key(target_crossed);
            let scheduled = resolve(transition_key, rgba)?;
            let next_ms =
                centered_interval_ms(&mut self.rng, IDLE_ARMS_AVERAGE_MS, IDLE_ARMS_WINDOW_MS);
            self.next_arms_change = now + Duration::from_millis(next_ms);
            self.pending_crossed = Some(target_crossed);
            crate::log_info!(
                target: "gfx";
                "trueos-spirit: idle strategy event=arms-transition-start from={} to={} clip={} next_average_ms={} next_window_ms={}\n",
                idle_posture_name(self.crossed),
                idle_posture_name(target_crossed),
                transition_key,
                IDLE_ARMS_AVERAGE_MS,
                IDLE_ARMS_WINDOW_MS,
            );
            return Ok(scheduled);
        }

        if now >= self.next_blink {
            let next_ms =
                centered_interval_ms(&mut self.rng, IDLE_BLINK_AVERAGE_MS, IDLE_BLINK_WINDOW_MS);
            self.next_blink = now + Duration::from_millis(next_ms);
            crate::log_info!(
                target: "gfx";
                "trueos-spirit: idle strategy event=soft-blink posture={} next_average_ms={} next_window_ms={}\n",
                idle_posture_name(self.crossed),
                IDLE_BLINK_AVERAGE_MS,
                IDLE_BLINK_WINDOW_MS,
            );
            return resolve(idle_animation_key(self.crossed), rgba);
        }

        let until_blink_ms = self.next_blink.saturating_duration_since(now).as_millis();
        let until_arms_ms = self
            .next_arms_change
            .saturating_duration_since(now)
            .as_millis();
        let until_wink_ms = self.next_wink.saturating_duration_since(now).as_millis();
        let boundary_ms = until_blink_ms
            .min(until_arms_ms)
            .min(until_wink_ms)
            .min(IDLE_CONTROL_POLL_MS)
            .max(1);
        resolve_static_idle(idle_animation_key(self.crossed), rgba, boundary_ms)
    }
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

    fn clear(&mut self) -> usize {
        let removed = self.len;
        while self.pop().is_some() {}
        self.read = 0;
        self.write = 0;
        removed
    }

    fn replace(&mut self, packages: &[SpiritPackage]) -> Result<usize, LillyProtocolError> {
        if packages.len() > PACKAGE_RING_CAPACITY {
            return Err(LillyProtocolError::RingFull);
        }
        self.clear();
        for package in packages {
            self.push(package.clone())?;
        }
        Ok(self.len)
    }

    fn push_many(&mut self, packages: &[SpiritPackage]) -> Result<usize, LillyProtocolError> {
        if packages.len() > PACKAGE_RING_CAPACITY.saturating_sub(self.len) {
            return Err(LillyProtocolError::RingFull);
        }
        for package in packages {
            self.push(package.clone())?;
        }
        Ok(self.len)
    }
}

static PACKAGE_RING: Mutex<SpiritPackageRing> = Mutex::new(SpiritPackageRing::new());
static LAST_RGBA: Mutex<SpiritRgba8> = Mutex::new(SpiritRgba8::WHITE);
static IDLE_STRATEGY: Mutex<Option<LillyIdleStrategy>> = Mutex::new(None);
static EMOTION_SELECTOR: Mutex<Option<LillyEmotionSelector>> = Mutex::new(None);
static LUMEN_SPEECH_PLAYING: AtomicBool = AtomicBool::new(false);

/// Mixer-owned speech state. `true` is published only when Lumen PCM is about
/// to enter the live audio buffer; `false` follows its final drained sample.
pub(crate) fn set_lumen_speech_playing(playing: bool) {
    let previous = LUMEN_SPEECH_PLAYING.swap(playing, Ordering::AcqRel);
    if previous != playing {
        crate::log_info!(
            target: "gfx";
            "trueos-spirit: lumen speech playback={} clip={} synchronization=audio-mixer-boundary\n",
            if playing { "started" } else { "drained" },
            LUMEN_TALK_ANIMATION,
        );
    }
}

/// Queue one owned API package. Validation happens before publication, so the
/// reader never encounters an unsupported schema or a bad animation tag.
#[allow(dead_code)]
pub(crate) fn enqueue_package(package: SpiritPackage) -> Result<usize, LillyProtocolError> {
    if let Err(error) = validate_package(&package) {
        crate::log_warn!(
            target: "gfx";
            "trueos-spirit: package rejected version={} text_present={} gesture={:?} error={:?} action=drop\n",
            package.version,
            package.text.is_some(),
            package.gesture.as_ref().map(|gesture| gesture.tag.as_str()),
            error,
        );
        return Err(error);
    }
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

/// Convenience wrapper for text-only packages. The Spirit reader forwards
/// drained text to its retained on-demand Gridpaper response document.
#[allow(dead_code)]
pub(crate) fn enqueue_text(text: &str) -> Result<usize, LillyProtocolError> {
    enqueue_package(SpiritPackage::new(Some(String::from(text)), None))
}

/// Map and queue one to three model-facing emotion words as whole Lilly clips.
///
/// The request is all-or-nothing: every word and resident catalog mapping is
/// validated before the package ring changes. Variants are selected through
/// Tyche's soft RNG and the same theme avoids an immediate repeat whenever it
/// has more than one visual variant.
#[allow(dead_code)]
pub(crate) fn enqueue_emotion_words(words: &[&str]) -> Result<usize, LillyProtocolError> {
    if !(MIN_EMOTIONS_PER_SEQUENCE..=MAX_EMOTIONS_PER_SEQUENCE).contains(&words.len()) {
        crate::log_warn!(
            target: "gfx";
            "trueos-spirit: emotion sequence rejected count={} allowed={}..={} action=drop\n",
            words.len(),
            MIN_EMOTIONS_PER_SEQUENCE,
            MAX_EMOTIONS_PER_SEQUENCE,
        );
        return Err(LillyProtocolError::EmotionCount);
    }

    let mut emotions = Vec::with_capacity(words.len());
    for word in words {
        let Some(emotion) = LillyEmotion::from_word(word) else {
            crate::log_warn!(
                target: "gfx";
                "trueos-spirit: emotion sequence rejected word={:?} error=unknown-emotion action=drop\n",
                word,
            );
            return Err(LillyProtocolError::UnknownEmotion);
        };
        emotions.push(emotion);
    }
    enqueue_emotions(&emotions)
}

/// Strongly typed ingress for in-kernel producers which already use
/// [`LillyEmotion`]. Callers with model text should use
/// [`enqueue_emotion_words`] so the vocabulary boundary remains explicit.
#[allow(dead_code)]
pub(crate) fn enqueue_emotions(emotions: &[LillyEmotion]) -> Result<usize, LillyProtocolError> {
    if !(MIN_EMOTIONS_PER_SEQUENCE..=MAX_EMOTIONS_PER_SEQUENCE).contains(&emotions.len()) {
        return Err(LillyProtocolError::EmotionCount);
    }
    if lilly::resident_frame_count() == 0 {
        return Err(LillyProtocolError::AssetsNotReady);
    }

    let mut selected = Vec::with_capacity(emotions.len());
    {
        let mut selector = EMOTION_SELECTOR.lock();
        let selector = selector.get_or_insert_with(LillyEmotionSelector::new);
        for emotion in emotions {
            let (tag, variant_index, variant_count) = selector.select(*emotion);
            selected.push((*emotion, tag, variant_index, variant_count));
        }
    }

    let mut packages = Vec::with_capacity(selected.len());
    for (_, tag, _, _) in &selected {
        let package = SpiritPackage::new(
            None,
            Some(SpiritGesture {
                tag: String::from(*tag),
                rgba: SpiritRgba8::WHITE,
            }),
        );
        validate_package(&package)?;
        packages.push(package);
    }

    let ring_len = PACKAGE_RING.lock().push_many(&packages)?;
    for (sequence_index, (emotion, tag, variant_index, variant_count)) in
        selected.iter().enumerate()
    {
        crate::log_info!(
            target: "gfx";
            "trueos-spirit: emotion queued theme={} clip={} variant={}/{} sequence={}/{} ring_len={}\n",
            emotion.as_word(),
            tag,
            variant_index + 1,
            variant_count,
            sequence_index + 1,
            selected.len(),
            ring_len,
        );
    }
    Ok(ring_len)
}

/// Atomically discard queued work and install the next timeline. The active
/// clip is already owned by the presentation worker, so it finishes normally;
/// these packages become visible when that worker asks at the next boundary.
#[allow(dead_code)]
pub(crate) fn replace_timeline(packages: &[SpiritPackage]) -> Result<usize, LillyProtocolError> {
    for package in packages {
        validate_package(package)?;
    }
    PACKAGE_RING.lock().replace(packages)
}

/// Finish the active clip, then fall back to the default idle. This is an empty
/// timeline replacement and never interrupts a frame set in progress.
#[allow(dead_code)]
pub(crate) fn stop_after_boundary() -> usize {
    PACKAGE_RING.lock().clear()
}

/// Resolve the next whole clip at an animation boundary.
///
/// Requests run directly in queue order. With no producer work, one neutral
/// idle remains alive. This function is deliberately not a per-frame iterator:
/// the renderer uses the returned schedule for that cheap inner loop, then asks
/// here again at its next control or clip boundary.
#[allow(dead_code)]
pub(crate) fn next_animation() -> Result<LillyScheduledAnimation, LillyProtocolError> {
    loop {
        if let Some(event) = crate::r::ai_activity::try_take_reasoning_event() {
            pause_idle_strategy();
            let (event_name, animation_key) = match event.phase {
                crate::r::ai_activity::AiReasoningPhase::Started => {
                    ("reasoning-start", AI_REASONING_START_ANIMATION)
                }
                crate::r::ai_activity::AiReasoningPhase::Finished => {
                    ("reasoning-finish", AI_REASONING_FINISH_ANIMATION)
                }
            };
            crate::log_info!(
                target: "gfx";
                "trueos-spirit: ai animation event={} source={:?} turn={} clip={}\n",
                event_name,
                event.source,
                event.turn,
                animation_key,
            );
            return resolve(animation_key, *LAST_RGBA.lock());
        }

        if crate::r::ai_activity::reasoning_active() {
            return next_idle_animation(*LAST_RGBA.lock());
        }

        if LUMEN_SPEECH_PLAYING.load(Ordering::Acquire) {
            pause_idle_strategy();
            return resolve(LUMEN_TALK_ANIMATION, *LAST_RGBA.lock());
        }

        let Some((package, ring_remaining)) = PACKAGE_RING.lock().pop() else {
            return next_idle_animation(*LAST_RGBA.lock());
        };
        log_drained_package(&package, ring_remaining);
        if let Some(text) = package.text.as_deref() {
            super::response_window::enqueue_package_text(text);
        }
        let Some(gesture) = package.gesture else {
            // Text has reached the response presenter. Keep draining so a
            // later gesture in the same reader turn can flow normally.
            continue;
        };
        pause_idle_strategy();
        let animation = lilly::resident_animation(gesture.tag.as_str())
            .ok_or(LillyProtocolError::CatalogInvariant)?;
        *LAST_RGBA.lock() = gesture.rgba;
        return Ok(LillyScheduledAnimation {
            rgba: gesture.rgba,
            boundary_ms: animation.cycle_duration_ms(),
            animation,
            static_frame: None,
        });
    }
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
        if package.text.is_some() { "ui4-response" } else { "none" },
        if package.gesture.is_some() { "sequence" } else { "none" },
    );
}

fn resolve(
    tag: &'static str,
    rgba: SpiritRgba8,
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
        boundary_ms: animation.cycle_duration_ms(),
        animation,
        static_frame: None,
    })
}

fn resolve_static_idle(
    tag: &'static str,
    rgba: SpiritRgba8,
    boundary_ms: u64,
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
        boundary_ms,
        static_frame: Some(animation.frames[0]),
        animation,
    })
}

fn next_idle_animation(rgba: SpiritRgba8) -> Result<LillyScheduledAnimation, LillyProtocolError> {
    let now = Instant::now();
    let mut strategy = IDLE_STRATEGY.lock();
    let scheduled = strategy
        .get_or_insert_with(|| LillyIdleStrategy::new(now))
        .schedule(now, rgba)?;
    super::spirit_vfx::set_idle_vfx(true);
    Ok(scheduled)
}

fn pause_idle_strategy() {
    if let Some(strategy) = IDLE_STRATEGY.lock().as_mut() {
        strategy.active = false;
    }
    super::spirit_vfx::set_idle_vfx(false);
}

fn centered_interval_ms(rng: &mut crate::tyche::SoftRng, average_ms: u64, window_ms: u64) -> u64 {
    let lower_ms = average_ms.saturating_sub(window_ms / 2);
    let offset_range = usize::try_from(window_ms.saturating_add(1)).unwrap_or(usize::MAX);
    lower_ms.saturating_add(rng.usize_below(offset_range) as u64)
}

fn next_distinct_variant(
    rng: &mut crate::tyche::SoftRng,
    previous: Option<usize>,
    count: usize,
) -> usize {
    if count <= 1 {
        return 0;
    }
    let candidate = rng.usize_below(count - usize::from(previous.is_some()));
    match previous {
        Some(previous) if candidate >= previous => candidate + 1,
        _ => candidate,
    }
}

const fn idle_animation_key(crossed: bool) -> &'static str {
    if crossed {
        IDLE_CROSSED_SOFT_BLINK
    } else {
        IDLE_UNCROSSED_SOFT_BLINK
    }
}

const fn idle_transition_key(target_crossed: bool) -> &'static str {
    if target_crossed {
        IDLE_CROSS_ARMS_TRANSITION
    } else {
        IDLE_UNCROSS_ARMS_TRANSITION
    }
}

const fn idle_posture_name(crossed: bool) -> &'static str {
    if crossed { "crossed" } else { "uncrossed" }
}
