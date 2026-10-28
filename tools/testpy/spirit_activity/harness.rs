#![allow(dead_code)]

extern crate self as trueos_time;
extern crate self as trueos_terminal;

#[path = "@TRUEOS_ROOT@/crates/trueos-terminal/src/program_status.rs"]
pub mod program_status;

mod shell3 {
    pub mod tui {
        pub static STATUS: spin::Mutex<Option<crate::program_status::ProgramStatus>> = spin::Mutex::new(None);
        pub fn program_status() -> Option<crate::program_status::ProgramStatus> { *STATUS.lock() }
    }
}

use std::sync::atomic::{AtomicU64, Ordering};

static NOW: AtomicU64 = AtomicU64::new(0);
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct Instant(u64);
#[derive(Copy, Clone)]
pub struct Duration(u64);
impl Instant {
    pub fn now() -> Self { Self(NOW.load(Ordering::Relaxed)) }
    pub fn as_millis(self) -> u64 { self.0 }
    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        Duration(self.0.saturating_sub(earlier.0))
    }
}
impl Duration {
    pub fn from_millis(value: u64) -> Self { Self(value) }
    pub fn as_millis(self) -> u64 { self.0 }
}
impl std::ops::Add<Duration> for Instant {
    type Output = Self;
    fn add(self, rhs: Duration) -> Self { Self(self.0 + rhs.0) }
}

#[macro_export]
macro_rules! log_info {
    (target: $target:expr; $($arg:tt)*) => { let _ = format_args!($($arg)*); };
}
pub use log_info as log_warn;

mod tyche {
    use std::sync::atomic::{AtomicUsize, Ordering};
    pub static GATE: AtomicUsize = AtomicUsize::new(0);
    pub static RUNNING_OFFSET: AtomicUsize = AtomicUsize::new(5000);
    pub static INITIAL_WAITING_VARIANT: AtomicUsize = AtomicUsize::new(1);
    pub struct SoftRng;
    pub fn soft_rng() -> SoftRng { SoftRng }
    impl SoftRng {
        pub fn usize_below(&mut self, max: usize) -> usize {
            match max {
                2 => GATE.load(Ordering::Relaxed).min(1),
                3 => INITIAL_WAITING_VARIANT.load(Ordering::Relaxed).min(2),
                10001 => RUNNING_OFFSET.load(Ordering::Relaxed).min(10000),
                _ => max / 2,
            }
        }
    }
}

mod ai {
    pub mod ai_activity {
        use std::sync::atomic::{AtomicBool, Ordering};
        pub static ACTIVE: AtomicBool = AtomicBool::new(false);
        pub static EVENT: spin::Mutex<Option<AiReasoningEvent>> = spin::Mutex::new(None);
        #[derive(Copy, Clone)]
        pub enum AiReasoningPhase { Started, Finished }
        pub struct AiReasoningEvent { pub phase: AiReasoningPhase, pub source: &'static str, pub turn: u64 }
        pub fn try_take_reasoning_event() -> Option<AiReasoningEvent> { EVENT.lock().take() }
        pub fn reasoning_active() -> bool { ACTIVE.load(Ordering::Relaxed) }
    }
}

mod spirit {
    pub mod lilly {
        include!("lilly_types.rs");
        pub fn resident_frame_count() -> usize { 490 }
        pub fn resident_animation(key: &str) -> Option<LillyResidentAnimation> {
            // The real catalog plus the unmodified production animation types
            // and frame helper; only resident GPU surfaces are fake.
            let line = include_str!("@TRUEOS_ROOT@/tools/Lilly.catalog")
                .lines().find(|line| line.split('|').next() == Some(key))?;
            let mut fields = line.split('|');
            let key = fields.next()?;
            fields.next()?;
            let playback = match fields.next()? {
                "loop" => LillyPlayback::Loop,
                "once" => LillyPlayback::Once,
                "once_hold" => LillyPlayback::OnceHold,
                _ => return None,
            };
            let frame_period_ms = fields.next()?.parse().ok()?;
            let parts: Vec<_> = fields.next()?.split(',').collect();
            Some(LillyResidentAnimation {
                key, playback, frame_period_ms,
                frames: std::array::from_fn(|index| LillyResidentPart {
                    semantic: parts[index],
                    surface: LillyResidentFrame { phys: index as u64, gpu: 0, bytes: 0, width: 0, height: 0, pitch_bytes: 0 },
                }),
            })
        }
    }
    pub mod spirit_vfx { pub fn set_idle_vfx(_: bool) {} }
    #[path = "@TRUEOS_ROOT@/src/spirit/lilly_protocol.rs"]
    pub mod lilly_protocol;
}

#[cfg(test)]
mod tests {
    use super::*;
    use spirit::lilly_protocol as protocol;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());
    fn now(ms: u64) { NOW.store(ms, Ordering::Relaxed); }
    fn reset(start: u64) -> std::sync::MutexGuard<'static, ()> {
        let lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        now(start);
        protocol::stop_after_boundary();
        protocol::set_lumen_speech_playing(false);
        ai::ai_activity::ACTIVE.store(false, Ordering::Relaxed);
        *ai::ai_activity::EVENT.lock() = None;
        *shell3::tui::STATUS.lock() = None;
        tyche::GATE.store(0, Ordering::Relaxed);
        tyche::RUNNING_OFFSET.store(5000, Ordering::Relaxed);
        tyche::INITIAL_WAITING_VARIANT.store(1, Ordering::Relaxed);
        lock
    }
    fn next() -> protocol::LillyScheduledAnimation { protocol::next_animation().unwrap() }
    fn failure(key: &str) -> bool { ["angry.fists_clenched", "reaction.facepalm"].contains(&key) }
    fn status(value: Option<program_status::ProgramStatus>) { *shell3::tui::STATUS.lock() = value; }

    #[test]
    fn centers_once_and_uses_existing_retimed_frame_helper() {
        let _lock = reset(0);
        protocol::schedule_failed_reaction(0, 3000).unwrap();
        now(1219);
        assert_ne!(next().animation.key, "angry.fists_clenched");
        now(1220);
        let scheduled = next();
        assert_eq!(scheduled.animation.key, "angry.fists_clenched");
        assert_eq!(scheduled.animation.frame_period_ms, 80);
        assert_eq!(scheduled.boundary_ms, 560);
        assert_eq!(scheduled.frame_at_elapsed(0).unwrap().semantic, "tense");
        assert_eq!(scheduled.frame_at_elapsed(79).unwrap().semantic, "tense");
        assert_eq!(scheduled.frame_at_elapsed(80).unwrap().semantic, "tense_to_raise_fists");
        assert_eq!(scheduled.frame_at_elapsed(559).unwrap().semantic, "hold_fists");
        assert!(scheduled.frame_at_elapsed(560).is_none());
        now(1780);
        assert_ne!(next().animation.key, "angry.fists_clenched");
    }

    #[test]
    fn centers_for_long_envelope_without_another_duration_selection() {
        let _lock = reset(10000);
        protocol::schedule_failed_reaction(10000, 5000).unwrap();
        now(12219);
        assert_ne!(next().animation.key, "angry.fists_clenched");
        now(12220);
        assert_eq!(next().animation.key, "angry.fists_clenched");
    }

    #[test]
    fn drops_when_full_clip_no_longer_fits_and_preserves_package() {
        let _lock = reset(20000);
        protocol::schedule_failed_reaction(20000, 3000).unwrap();
        protocol::enqueue_animation("wave.shy", protocol::SpiritRgba8::WHITE).unwrap();
        now(22441);
        assert_eq!(next().animation.key, "wave.shy");
        now(23000);
        assert_ne!(next().animation.key, "angry.fists_clenched");
    }

    #[test]
    fn full_clip_can_end_exactly_at_expiry() {
        let _lock = reset(30000);
        protocol::schedule_failed_reaction(30000, 3000).unwrap();
        now(32440);
        assert_eq!(next().animation.key, "angry.fists_clenched");
    }

    #[test]
    fn failure_does_not_drain_or_clear_other_packages() {
        let _lock = reset(40000);
        protocol::enqueue_animation("wave.shy", protocol::SpiritRgba8::WHITE).unwrap();
        protocol::schedule_failed_reaction(40000, 3000).unwrap();
        now(41220);
        assert_eq!(next().animation.key, "angry.fists_clenched");
        now(41780);
        assert_eq!(next().animation.key, "wave.shy");
    }

    #[test]
    fn reasoning_event_active_reasoning_and_live_speech_keep_priority() {
        let _lock = reset(50000);
        protocol::schedule_failed_reaction(50000, 5000).unwrap();
        now(52220);
        *ai::ai_activity::EVENT.lock() = Some(ai::ai_activity::AiReasoningEvent {
            phase: ai::ai_activity::AiReasoningPhase::Started, source: "test", turn: 1,
        });
        assert_eq!(next().animation.key, "agree.firm_nod");
        ai::ai_activity::ACTIVE.store(true, Ordering::Relaxed);
        assert_ne!(next().animation.key, "angry.fists_clenched");
        ai::ai_activity::ACTIVE.store(false, Ordering::Relaxed);
        protocol::set_lumen_speech_playing(true);
        assert_eq!(next().animation.key, "talk.calm.uncrossed");
        protocol::set_lumen_speech_playing(false);
        assert_eq!(next().animation.key, "angry.fists_clenched");
    }

    #[test]
    fn new_failure_replaces_only_pending_failure() {
        let _lock = reset(60000);
        protocol::schedule_failed_reaction(60000, 3000).unwrap();
        protocol::enqueue_animation("wave.shy", protocol::SpiritRgba8::WHITE).unwrap();
        protocol::schedule_failed_reaction(61000, 5000).unwrap();
        now(61220);
        assert_eq!(next().animation.key, "wave.shy");
        now(63220);
        assert_eq!(next().animation.key, "reaction.facepalm");
        assert!(!failure(next().animation.key));
    }

    #[test]
    fn expired_or_too_short_failure_never_plays_later() {
        let _lock = reset(70000);
        protocol::schedule_failed_reaction(70000, 3000).unwrap();
        now(73000);
        assert_ne!(next().animation.key, "angry.fists_clenched");
        protocol::schedule_failed_reaction(73000, 559).unwrap();
        assert_ne!(next().animation.key, "angry.fists_clenched");
    }

    #[test]
    fn running_waits_for_cadence_prepares_crossed_and_returns_coherent_idle() {
        let _lock = reset(0);
        status(Some(program_status::ProgramStatus::Working));
        assert!(!failure(next().animation.key));
        now(14999);
        protocol::set_lumen_speech_playing(true);
        assert_eq!(next().animation.key, "talk.calm.uncrossed");
        protocol::set_lumen_speech_playing(false);
        now(15000);
        let prep = next();
        assert_eq!(prep.animation.key, "transition.neutral_to_crossed");
        assert_eq!(prep.boundary_ms, 399);
        now(15399);
        assert_eq!(next().animation.key, "idea.finger_up");
        now(15924);
        assert_eq!(next().animation.key, "idle.crossed.soft_blink");
    }

    #[test]
    fn rejected_running_opportunity_does_not_retry_until_next_interval() {
        let _lock = reset(0);
        tyche::GATE.store(1, Ordering::Relaxed);
        status(Some(program_status::ProgramStatus::Working));
        next();
        // Keep higher-priority speech live so autonomous arm transitions do not
        // accidentally become the same preparation clip under test.
        protocol::set_lumen_speech_playing(true);
        now(15000); next();
        tyche::GATE.store(0, Ordering::Relaxed);
        protocol::set_lumen_speech_playing(false);
        now(15001);
        assert_ne!(next().animation.key, "idea.finger_up");
        now(29999);
        assert_ne!(next().animation.key, "idea.finger_up");
        now(30000);
        let first = next();
        if first.animation.key == "transition.neutral_to_crossed" {
            now(30399);
            assert_eq!(next().animation.key, "idea.finger_up");
        } else {
            assert_eq!(first.animation.key, "idea.finger_up");
        }
    }

    #[test]
    fn running_cadence_supports_both_rng_endpoints() {
        let _lock = reset(0);
        tyche::RUNNING_OFFSET.store(0, Ordering::Relaxed);
        status(Some(program_status::ProgramStatus::Working)); next();
        now(9999);
        assert_ne!(next().animation.key, "idea.finger_up");
        now(10000);
        assert_eq!(next().animation.key, "transition.neutral_to_crossed");
        now(10399);
        assert_eq!(next().animation.key, "idea.finger_up");
        status(None); now(11000); next();
        tyche::RUNNING_OFFSET.store(10000, Ordering::Relaxed);
        status(Some(program_status::ProgramStatus::Working)); next();
        now(30999);
        protocol::set_lumen_speech_playing(true); next();
        protocol::set_lumen_speech_playing(false);
        now(31000);
        assert_eq!(next().animation.key, "idea.finger_up");
    }

    #[test]
    fn waiting_only_suggests_once_per_state_edge_and_idle_cancels() {
        let _lock = reset(0);
        status(Some(program_status::ProgramStatus::Blocked(program_status::ProgramStatusKind::Question)));
        assert_eq!(next().animation.key, "wave.neutral");
        now(455);
        assert!(next().animation.key.starts_with("idle."));
        status(Some(program_status::ProgramStatus::Blocked(program_status::ProgramStatusKind::Permission)));
        assert!(next().animation.key.starts_with("idle."));
        status(None); next();
        status(Some(program_status::ProgramStatus::Blocked(program_status::ProgramStatusKind::Question)));
        assert_eq!(next().animation.key, "confused.question");
        status(None); next();
        protocol::set_lumen_speech_playing(true);
        status(Some(program_status::ProgramStatus::Done)); next();
        status(None);
        protocol::set_lumen_speech_playing(false);
        assert_ne!(next().animation.key, "gesture.finger_guns");
    }

    #[test]
    fn review_uses_finger_guns_once_and_respects_gate() {
        let _lock = reset(0);
        status(Some(program_status::ProgramStatus::Done));
        assert_eq!(next().animation.key, "gesture.finger_guns");
        now(385);
        assert!(next().animation.key.starts_with("idle."));
        status(None); next();
        tyche::GATE.store(1, Ordering::Relaxed);
        status(Some(program_status::ProgramStatus::Done));
        assert!(next().animation.key.starts_with("idle."));
        tyche::GATE.store(0, Ordering::Relaxed);
        now(400);
        assert!(next().animation.key.starts_with("idle."));
    }

    #[test]
    fn ordinary_packages_precede_suggestions_and_expire_them_without_backlog() {
        let _lock = reset(0);
        protocol::enqueue_animation("wave.shy", protocol::SpiritRgba8::WHITE).unwrap();
        status(Some(program_status::ProgramStatus::Done));
        assert_eq!(next().animation.key, "wave.shy");
        now(490);
        assert_eq!(next().animation.key, "gesture.finger_guns");
        status(None); next();
        status(Some(program_status::ProgramStatus::Done));
        for index in 0..8 {
            protocol::enqueue_animation("wave.shy", protocol::SpiritRgba8::WHITE).unwrap();
            now(1000 + index * 490);
            assert_eq!(next().animation.key, "wave.shy");
        }
        now(5000);
        assert_ne!(next().animation.key, "gesture.finger_guns");
    }

    #[test]
    fn speech_and_reasoning_can_expire_low_priority_suggestion() {
        let _lock = reset(0);
        status(Some(program_status::ProgramStatus::Done));
        protocol::set_lumen_speech_playing(true);
        assert_eq!(next().animation.key, "talk.calm.uncrossed");
        protocol::set_lumen_speech_playing(false);
        ai::ai_activity::ACTIVE.store(true, Ordering::Relaxed);
        now(3000);
        assert_ne!(next().animation.key, "gesture.finger_guns");
        ai::ai_activity::ACTIVE.store(false, Ordering::Relaxed);
        assert_ne!(next().animation.key, "gesture.finger_guns");
    }

    #[test]
    fn failed_variant_alternates_without_changing_burst_or_frame_duration() {
        let _lock = reset(0);
        protocol::schedule_failed_reaction(0, 3000).unwrap();
        now(1220);
        assert_eq!(next().animation.key, "angry.fists_clenched");
        protocol::schedule_failed_reaction(3000, 5000).unwrap();
        now(5220);
        let second = next();
        assert_eq!(second.animation.key, "reaction.facepalm");
        assert_eq!(second.boundary_ms, 560);
        assert_eq!(second.animation.frame_period_ms, 80);
    }

    #[test]
    fn failed_preparation_must_fit_and_idle_inherits_actual_uncrossed_pose() {
        let _lock = reset(0);
        status(Some(program_status::ProgramStatus::Working)); next();
        now(15000);
        assert_eq!(next().animation.key, "transition.neutral_to_crossed");
        now(15399);
        assert_eq!(next().animation.key, "idea.finger_up");
        status(None);
        protocol::schedule_failed_reaction(16000, 3000).unwrap();
        now(17220);
        assert_eq!(next().animation.key, "transition.uncross_arms");
        now(17619);
        assert_eq!(next().animation.key, "angry.fists_clenched");
        now(18179);
        assert_eq!(next().animation.key, "idle.uncrossed.soft_blink");
    }

    #[test]
    fn failed_drops_if_preparation_and_gesture_cannot_both_finish() {
        let _lock = reset(0);
        status(Some(program_status::ProgramStatus::Working)); next();
        now(15000); next();
        now(15399); assert_eq!(next().animation.key, "idea.finger_up");
        status(None);
        protocol::schedule_failed_reaction(16000, 3000).unwrap();
        now(18200); // 800ms remains; 399ms transition +560ms gesture do not fit.
        assert_eq!(next().animation.key, "idle.crossed.soft_blink");
        now(18300);
        assert!(!failure(next().animation.key));
    }

    #[test]
    fn waiting_rotates_through_real_variants_without_immediate_repeat() {
        let _lock = reset(0);
        tyche::INITIAL_WAITING_VARIANT.store(2, Ordering::Relaxed);
        for (index, expected) in ["wave.excited", "confused.question", "wave.neutral"].iter().enumerate() {
            now(index as u64 * 1000);
            status(Some(program_status::ProgramStatus::Blocked(program_status::ProgramStatusKind::Question)));
            assert_eq!(next().animation.key, *expected);
            status(None);
            next();
        }
    }
}
