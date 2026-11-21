#![allow(dead_code)]
extern crate self as trueos_time;

use std::sync::atomic::{AtomicU64, Ordering};
static NOW: AtomicU64 = AtomicU64::new(0);
static WARNINGS: AtomicU64 = AtomicU64::new(0);

pub struct Instant(u64);
impl Instant {
    pub fn now() -> Self { Self(NOW.load(Ordering::Relaxed)) }
    pub fn as_millis(self) -> u64 { self.0 }
}

#[macro_export]
macro_rules! log_warn {
    (target: $target:expr; $($arg:tt)*) => {{
        let _ = format_args!($($arg)*);
        $crate::WARNINGS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }};
}

mod tyche {
    pub struct SoftRng(u64);
    pub fn soft_rng() -> SoftRng { SoftRng(1) }
    impl SoftRng {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
            self.0 >> 32
        }
        pub fn usize_below(&mut self, max: usize) -> usize { self.next() as usize % max }
        pub fn bool(&mut self) -> bool { self.usize_below(2) != 0 }
    }
}

mod spirit {
    pub mod lilly {
        include!("lilly_types.rs");

        pub fn resident_frame(path: &str) -> Option<LillyResidentFrame> {
            let gpu = path.bytes().fold(0u64, |sum, byte| sum.wrapping_mul(31).wrapping_add(u64::from(byte)));
            Some(LillyResidentFrame { phys: 0, gpu, bytes: 262144, width: 256, height: 256, pitch_bytes: 1024 })
        }
    }
    pub mod codex_handoff {
        use std::sync::atomic::{AtomicBool, Ordering};
        pub static REQUESTED: AtomicBool = AtomicBool::new(false);
        pub static READY: AtomicBool = AtomicBool::new(false);
        pub fn needs_crossed() -> bool { REQUESTED.load(Ordering::Relaxed) }
        pub fn crossed_ready() { READY.store(true, Ordering::Relaxed); }
    }
    #[path = "@TRUEOS_ROOT@/src/spirit/lilly_sequencer.rs"]
    mod lilly_sequencer;
    #[path = "@TRUEOS_ROOT@/src/spirit/lilly_protocol.rs"]
    mod lilly_protocol;

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::{NOW, WARNINGS, Ordering, tyche};
        use lilly_sequencer::{Contextual, IdleMode, Playback, Sequencer, TalkMoods};

        const CROSS: &str = "Static_up/static.png";
        const UNCROSS: &str = "Static_up/uncrossed.png";

        fn tick(state: &mut Sequencer, time: &mut u64) -> Playback {
            let clip = state.next(*time, false);
            *time += clip.duration_ms();
            clip
        }

        fn clip_path(clip: Playback) -> &'static str {
            clip.frames[clip.frame_index(0)]
        }

        #[test]
        fn legacy_signals_only_warn_and_collapse_has_priority_over_a_completed_turn() {
            use lilly_protocol as protocol;
            protocol::enqueue_presentation_word("vibe:happy").unwrap();
            protocol::enqueue_presentation_word("Happy").unwrap();
            assert_eq!(WARNINGS.load(Ordering::Relaxed), 2);
            assert_eq!(protocol::next_animation().unwrap().frame_at_elapsed(0), lilly::resident_frame(CROSS).unwrap());
            assert!(protocol::enqueue_contextual_word("vibe:happy").is_err());

            protocol::set_idle_mode(IdleMode::Typing);
            assert_eq!(protocol::idle_mode(), IdleMode::Typing);
            let mut time = 50;
            for path in ["Transitions_up/unfolding-arms/frame_02.png", "Freestyle_up/wireless-keyboard/frame_02.png"] {
                NOW.store(time, Ordering::Relaxed);
                let active = protocol::next_animation().unwrap();
                assert_eq!(active.frame_at_elapsed(100), lilly::resident_frame(path).unwrap());
                time += active.boundary_ms;
            }
            protocol::enqueue_contextual_word("Happy").unwrap();
            assert_eq!(protocol::idle_mode(), IdleMode::Normal);
            codex_handoff::REQUESTED.store(true, Ordering::Relaxed);
            for _ in 0..2 {
                NOW.store(time, Ordering::Relaxed);
                let active = protocol::next_animation().unwrap();
                assert!(!codex_handoff::READY.load(Ordering::Relaxed));
                time += active.boundary_ms;
            }
            NOW.store(time, Ordering::Relaxed);
            let held = protocol::next_animation().unwrap();
            assert_eq!(held.frame_at_elapsed(0), lilly::resident_frame(CROSS).unwrap());
            assert!(codex_handoff::READY.load(Ordering::Relaxed));
            codex_handoff::REQUESTED.store(false, Ordering::Relaxed);
            assert_eq!(protocol::next_animation().unwrap().frame_at_elapsed(450),
                lilly::resident_frame("Happy2_up/frame_02.png").unwrap());
        }

        #[test]
        fn lico_selection_only_releases_sound_it_enabled() {
            let mut state = Sequencer::new(tyche::soft_rng());
            state.set_window_sound(true);
            assert_eq!(state.mode(), IdleMode::Sound);
            state.set_window_sound(true);
            state.set_window_sound(false);
            assert_eq!(state.mode(), IdleMode::Normal);

            state.set_mode(IdleMode::Sound);
            state.set_window_sound(true);
            state.set_window_sound(false);
            assert_eq!(state.mode(), IdleMode::Sound);

            state.set_mode(IdleMode::Normal);
            state.set_window_sound(true);
            state.set_mode(IdleMode::Sound);
            state.set_window_sound(false);
            assert_eq!(state.mode(), IdleMode::Sound);

            state.set_window_sound(true);
            state.set_mode(IdleMode::Reading);
            state.set_window_sound(false);
            assert_eq!(state.mode(), IdleMode::Reading);

            state.set_window_sound(true);
            assert!(state.enqueue(Contextual::Nod));
            assert_eq!(state.mode(), IdleMode::Normal);
            state.set_window_sound(false);
            assert_eq!(state.mode(), IdleMode::Normal);
        }

        #[test]
        fn lico_selection_plays_cat_mouth_then_headset_and_periodic_dance() {
            const HEADSET: &str = "Headset_up/static.png";
            let mut state = Sequencer::new(tyche::soft_rng());
            let mut time = 0;
            state.set_window_sound(true);

            for prefix in [
                "Transitions_up/unfolding-arms/",
                "Silly_up/cat-mouth/",
                "Headset_up/put-on/",
            ] {
                let clip = tick(&mut state, &mut time);
                assert!(clip.frames[1].starts_with(prefix));
            }
            for _ in 0..2 {
                let rest_start = time;
                loop {
                    let start = time;
                    let clip = tick(&mut state, &mut time);
                    if clip.frames.len() > 1 {
                        assert!((3000..=5000).contains(&(start - rest_start)));
                        assert!(clip.frames[1].starts_with("Headset_up/dance/"));
                        break;
                    }
                    assert_eq!(clip_path(clip), HEADSET);
                    assert!(start - rest_start < 5000);
                }
            }
            state.set_window_sound(false);
            let exit = tick(&mut state, &mut time);
            assert!(exit.frames[1].starts_with("Headset_up/take-off/"));
        }

        #[test]
        fn repeat_speed_uses_the_same_food_and_two_different_rates() {
            let mut state = Sequencer::new(tyche::soft_rng());
            let mut time = 0;
            assert!(state.enqueue(Contextual::Nod));
            tick(&mut state, &mut time);
            tick(&mut state, &mut time);
            assert_eq!(clip_path(tick(&mut state, &mut time)), UNCROSS);
            assert!(state.enqueue(Contextual::Eat));
            let a = tick(&mut state, &mut time);
            let b = tick(&mut state, &mut time);
            assert_eq!(a.frames, b.frames);
            assert_eq!(a.frame_index(0), 0);
            assert_eq!(b.frame_index(0), 0);
            let (mut durations, authored) = ([a.duration_ms(), b.duration_ms()], a.duration_ms().min(b.duration_ms()));
            durations.sort();
            assert_eq!(durations, [authored, authored * 3 / 2]);
        }

        #[test]
        fn contextual_groups_across_poses_and_default_idle_cadence() {
            let mut state = Sequencer::new(tyche::soft_rng());
            let mut time = 0;
            assert!(state.enqueue(Contextual::Flirt));
            let mut interiors = Vec::new();
            loop {
                let clip = tick(&mut state, &mut time);
                if clip.frames.len() == 1 { break; }
                if !clip.frames[1].starts_with("Transitions_up/") { interiors.push(clip.frames[1]); }
            }
            assert_eq!(interiors.len(), 2);
            assert_ne!(interiors[0], interiors[1]);
            let settled_at = time - 50;
            while tick(&mut state, &mut time).frames.len() == 1 {}
            assert!(time > settled_at + 3000);
            assert!(time < settled_at + 6500); // Includes one scheduled clip.
        }

        #[test]
        fn selected_talk_moods_chain_two_clips_then_start_a_new_pause() {
            let mut state = Sequencer::new(tyche::soft_rng());
            state.set_mode(IdleMode::Talk(TalkMoods::from_bits(TalkMoods::CALM)));
            let mut time = 0;
            let mut starts = Vec::new();
            let mut talk_count = 0;
            loop {
                let start = time;
                let clip = tick(&mut state, &mut time);
                if clip.frames.len() == 1 && talk_count == 2 { break; }
                if clip.frames.len() > 1 {
                    starts.push(start);
                    if clip.frames[1].starts_with("Talk_up/calm") { talk_count += 1; }
                }
                assert!(time < 8000);
            }
            assert_eq!(talk_count, 2);
            assert!((1500..=2550).contains(&starts[0]));
            state.set_mode(IdleMode::Sound);
            assert_eq!(state.mode(), IdleMode::Sound);
            let mut saw_cat = false;
            loop {
                let clip = tick(&mut state, &mut time);
                if clip.frames.len() == 1 { break; }
                if clip.frames[1].contains("cat-mouth") { saw_cat = true; }
                if clip.frames[1].contains("put-on") { assert!(saw_cat); }
            }
            assert!(saw_cat);
        }
    }
}
